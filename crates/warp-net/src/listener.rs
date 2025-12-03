//! QUIC listener for incoming connections

use crate::frames::Capabilities;
use crate::protocol::NegotiatedParams;
use crate::tls::{generate_self_signed, load_certs, load_private_key, server_config};
use crate::transport::WarpConnection;
use crate::{Error, Result};
use quinn::Endpoint;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

/// QUIC listener for incoming connections
pub struct WarpListener {
    endpoint: Endpoint,
    bind_addr: SocketAddr,
    local_caps: Capabilities,
}

impl WarpListener {
    /// Bind to an address with TLS certificate and key files
    pub async fn bind(
        addr: SocketAddr,
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<Self> {
        Self::bind_with_caps(addr, cert_path, key_path, Capabilities::default()).await
    }

    /// Bind with custom capabilities and certificate files
    pub async fn bind_with_caps(
        addr: SocketAddr,
        cert_path: &Path,
        key_path: &Path,
        caps: Capabilities,
    ) -> Result<Self> {
        let cert_chain = load_certs(cert_path)?;
        let key = load_private_key(key_path)?;

        Self::bind_with_tls(addr, cert_chain, key, caps).await
    }

    /// Bind with self-signed certificate (for testing)
    pub async fn bind_self_signed(addr: SocketAddr) -> Result<Self> {
        Self::bind_self_signed_with_caps(addr, Capabilities::default()).await
    }

    /// Bind with self-signed certificate and custom capabilities
    pub async fn bind_self_signed_with_caps(
        addr: SocketAddr,
        caps: Capabilities,
    ) -> Result<Self> {
        let (cert_chain, key) = generate_self_signed()?;
        Self::bind_with_tls(addr, cert_chain, key, caps).await
    }

    /// Bind with provided TLS certificate and key
    async fn bind_with_tls(
        addr: SocketAddr,
        cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
        caps: Capabilities,
    ) -> Result<Self> {
        let tls_config = server_config(cert_chain, key)?;

        let server_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(tls_config)
                .map_err(|e| Error::Tls(format!("Failed to create QUIC config: {}", e)))?,
        ));

        let endpoint = Endpoint::server(server_config, addr)
            .map_err(|e| Error::Connection(format!("Failed to bind listener: {}", e)))?;

        let bind_addr = endpoint
            .local_addr()
            .map_err(|e| Error::Connection(format!("Failed to get local address: {}", e)))?;

        tracing::info!("Listener bound to {}", bind_addr);

        Ok(Self {
            endpoint,
            bind_addr,
            local_caps: caps,
        })
    }

    /// Accept next connection
    pub async fn accept(&self) -> Result<WarpConnection> {
        let connecting = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| Error::Connection("Listener closed".into()))?;

        let remote_addr = connecting.remote_address();

        let connection = connecting
            .await
            .map_err(|e| Error::Connection(format!("Failed to accept connection: {}", e)))?;

        tracing::info!("Accepted connection from {}", remote_addr);

        let warp_conn = crate::transport::WarpConnection::from_quinn(
            connection,
            self.local_caps.clone(),
        );

        Ok(warp_conn)
    }

    /// Accept connection and perform handshake
    pub async fn accept_handshake(&self) -> Result<(WarpConnection, NegotiatedParams)> {
        let conn = self.accept().await?;
        let params = conn.handshake_server().await?;
        Ok((conn, params))
    }

    /// Accept multiple connections concurrently
    pub async fn accept_many(&self, count: usize) -> Result<Vec<WarpConnection>> {
        let mut connections = Vec::with_capacity(count);
        for _ in 0..count {
            connections.push(self.accept().await?);
        }
        Ok(connections)
    }

    /// Get bound address
    pub fn local_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    /// Get local capabilities
    pub fn capabilities(&self) -> &Capabilities {
        &self.local_caps
    }

    /// Check if listener is still active
    pub fn is_active(&self) -> bool {
        true
    }

    /// Wait for endpoint to be idle
    pub async fn wait_idle(&self) {
        self.endpoint.wait_idle().await;
    }

    /// Shutdown listener
    pub fn shutdown(&self) {
        tracing::info!("Shutting down listener on {}", self.bind_addr);
        self.endpoint.close(0u32.into(), b"listener shutdown");
    }

    /// Shutdown listener and wait for all connections to close
    pub async fn shutdown_graceful(&self) {
        self.shutdown();
        self.wait_idle().await;
    }
}

impl Drop for WarpListener {
    fn drop(&mut self) {
        if self.is_active() {
            self.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::WarpEndpoint;

    #[tokio::test]
    async fn test_bind_self_signed() {
        let listener = WarpListener::bind_self_signed("127.0.0.1:0".parse().unwrap()).await;
        assert!(listener.is_ok());

        let listener = listener.unwrap();
        assert!(listener.local_addr().port() > 0);
    }

    #[tokio::test]
    async fn test_accept_connection() {
        let listener = WarpListener::bind_self_signed("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = listener.local_addr();

        let server_task = tokio::spawn(async move {
            listener.accept().await.unwrap()
        });

        let client = WarpEndpoint::client().await.unwrap();
        let _client_conn = client.connect(addr, "localhost").await.unwrap();

        let _server_conn = server_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_accept_handshake() {
        let listener = WarpListener::bind_self_signed("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = listener.local_addr();

        let server_task = tokio::spawn(async move {
            let result = listener.accept_handshake().await.unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            result
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let client = WarpEndpoint::client().await.unwrap();
        let conn = client.connect(addr, "localhost").await.unwrap();
        let client_params = conn.handshake().await.unwrap();

        let (_server_conn, server_params) = server_task.await.unwrap();

        assert_eq!(client_params.compression, server_params.compression);
        assert_eq!(client_params.chunk_size, server_params.chunk_size);
    }

    #[tokio::test]
    async fn test_multiple_connections() {
        let listener = WarpListener::bind_self_signed("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = listener.local_addr();

        let server_task = tokio::spawn(async move {
            let conn1 = listener.accept().await.unwrap();
            let conn2 = listener.accept().await.unwrap();
            (conn1, conn2)
        });

        let client = WarpEndpoint::client().await.unwrap();
        let _conn1 = client.connect(addr, "localhost").await.unwrap();
        let _conn2 = client.connect(addr, "localhost").await.unwrap();

        let (_server_conn1, _server_conn2) = server_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_shutdown() {
        let listener = WarpListener::bind_self_signed("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();

        assert!(listener.is_active());
        listener.shutdown();
    }
}
