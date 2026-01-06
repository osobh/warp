//! NVMe-oF QUIC Transport Implementation
//!
//! This module implements the NVMe over QUIC transport, providing
//! encrypted, multiplexed connections with built-in congestion control.
//!
//! QUIC advantages for NVMe-oF:
//! - Built-in TLS 1.3 encryption
//! - Multiple streams per connection (avoids head-of-line blocking)
//! - Connection migration support
//! - Better performance over lossy networks (WAN)

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, trace};

use super::{
    ConnectionState, NvmeOfTransport, TransportAddress, TransportCapabilities, TransportConnection,
};
use crate::nvmeof::capsule::{CommandCapsule, IcReq, IcResp, PduHeader, PduType, ResponseCapsule};
use crate::nvmeof::command::{NvmeCommand, NvmeCompletion};
use crate::nvmeof::config::TransportType;
use crate::nvmeof::error::{NvmeOfError, NvmeOfResult};

/// QUIC transport configuration
#[derive(Debug, Clone)]
pub struct NvmeOfQuicConfig {
    /// Enable QUIC transport
    pub enabled: bool,

    /// Maximum concurrent bidirectional streams
    pub max_concurrent_bidi_streams: u32,

    /// Maximum concurrent unidirectional streams
    pub max_concurrent_uni_streams: u32,

    /// Keep-alive interval
    pub keep_alive_interval: Duration,

    /// Idle timeout
    pub idle_timeout: Duration,

    /// Maximum UDP payload size
    pub max_udp_payload_size: u16,

    /// Initial RTT estimate (for congestion control)
    pub initial_rtt: Duration,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Maximum inline data in capsule
    pub max_inline_data: u32,

    /// Maximum I/O size
    pub max_io_size: u32,

    /// ALPN protocol identifier
    pub alpn_protocols: Vec<Vec<u8>>,

    /// Server name for TLS (SNI)
    pub server_name: String,
}

impl Default for NvmeOfQuicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_bidi_streams: 256,
            max_concurrent_uni_streams: 256,
            keep_alive_interval: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(60),
            max_udp_payload_size: 1472,
            initial_rtt: Duration::from_millis(100),
            connect_timeout: Duration::from_secs(10),
            max_inline_data: 16384,
            max_io_size: 1024 * 1024, // 1MB
            alpn_protocols: vec![b"nvme-quic".to_vec()],
            server_name: "localhost".to_string(),
        }
    }
}

/// QUIC Transport for NVMe-oF
pub struct QuicTransport {
    /// Configuration
    config: NvmeOfQuicConfig,

    /// QUIC endpoint (server mode)
    endpoint: Mutex<Option<Endpoint>>,

    /// Connection counter for IDs
    connection_counter: AtomicU64,

    /// Local bind address
    local_addr: Mutex<Option<SocketAddr>>,
}

impl QuicTransport {
    /// Create a new QUIC transport
    pub fn new(config: NvmeOfQuicConfig) -> Self {
        Self {
            config,
            endpoint: Mutex::new(None),
            connection_counter: AtomicU64::new(0),
            local_addr: Mutex::new(None),
        }
    }

    /// Get next connection ID
    fn next_connection_id(&self) -> u64 {
        self.connection_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Create server configuration with self-signed certificate
    fn create_server_config(&self) -> NvmeOfResult<ServerConfig> {
        // Generate self-signed certificate
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .map_err(|e| NvmeOfError::Transport(format!("Certificate generation failed: {}", e)))?;

        let cert_der = CertificateDer::from(cert.cert);
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));

        let mut server_crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .map_err(|e| NvmeOfError::Transport(format!("TLS config failed: {}", e)))?;

        server_crypto.alpn_protocols = self.config.alpn_protocols.clone();

        let mut server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .map_err(|e| NvmeOfError::Transport(format!("QUIC config failed: {}", e)))?,
        ));

        // Configure transport parameters
        let mut transport = quinn::TransportConfig::default();
        transport.max_concurrent_bidi_streams(self.config.max_concurrent_bidi_streams.into());
        transport.max_concurrent_uni_streams(self.config.max_concurrent_uni_streams.into());
        transport.keep_alive_interval(Some(self.config.keep_alive_interval));
        transport.max_idle_timeout(Some(
            self.config
                .idle_timeout
                .try_into()
                .map_err(|_| NvmeOfError::Transport("Invalid timeout".to_string()))?,
        ));
        transport.initial_rtt(self.config.initial_rtt);

        server_config.transport_config(Arc::new(transport));

        Ok(server_config)
    }

    /// Create client configuration (accepts any certificate for testing)
    fn create_client_config(&self) -> NvmeOfResult<ClientConfig> {
        // Create client config that skips certificate verification (testing only)
        let mut client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        client_crypto.alpn_protocols = self.config.alpn_protocols.clone();

        let mut client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
                .map_err(|e| NvmeOfError::Transport(format!("QUIC config failed: {}", e)))?,
        ));

        // Configure transport parameters
        let mut transport = quinn::TransportConfig::default();
        transport.max_concurrent_bidi_streams(self.config.max_concurrent_bidi_streams.into());
        transport.max_concurrent_uni_streams(self.config.max_concurrent_uni_streams.into());
        transport.keep_alive_interval(Some(self.config.keep_alive_interval));
        transport.max_idle_timeout(Some(
            self.config
                .idle_timeout
                .try_into()
                .map_err(|_| NvmeOfError::Transport("Invalid timeout".to_string()))?,
        ));
        transport.initial_rtt(self.config.initial_rtt);

        client_config.transport_config(Arc::new(transport));

        Ok(client_config)
    }
}

/// Skip server certificate verification (for testing with self-signed certs)
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

#[async_trait]
impl NvmeOfTransport for QuicTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp // QUIC uses UDP but we report as TCP variant
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_inline_data: self.config.max_inline_data,
            max_io_size: self.config.max_io_size,
            header_digest: false, // QUIC provides integrity via TLS
            data_digest: false,   // QUIC provides integrity via TLS
            zero_copy: false,
            memory_registration: false,
            multi_stream: true, // QUIC supports multiple streams
        }
    }

    async fn bind(&mut self, addr: SocketAddr) -> NvmeOfResult<()> {
        let server_config = self.create_server_config()?;
        let endpoint = Endpoint::server(server_config, addr)?;

        // Get the actual bound address (important when port 0 is used)
        let actual_addr = endpoint.local_addr()?;
        debug!("NVMe-oF QUIC transport bound to {}", actual_addr);

        *self.local_addr.lock().await = Some(actual_addr);
        *self.endpoint.lock().await = Some(endpoint);

        Ok(())
    }

    async fn accept(&self) -> NvmeOfResult<Box<dyn TransportConnection>> {
        let guard = self.endpoint.lock().await;
        let endpoint = guard
            .as_ref()
            .ok_or_else(|| NvmeOfError::Transport("Transport not bound".to_string()))?;

        let incoming = endpoint
            .accept()
            .await
            .ok_or_else(|| NvmeOfError::Transport("Endpoint closed".to_string()))?;

        let connection = incoming
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Connection failed: {}", e)))?;

        let remote_addr = connection.remote_address();
        let local_addr = self
            .local_addr
            .lock()
            .await
            .unwrap_or_else(|| "0.0.0.0:0".parse().unwrap());
        let connection_id = self.next_connection_id();

        debug!("Accepted QUIC connection from {}", remote_addr);

        let conn = QuicConnection::new_for_target(
            connection,
            local_addr,
            remote_addr,
            connection_id,
            self.config.clone(),
        )
        .await?;

        Ok(Box::new(conn))
    }

    async fn connect(&self, addr: &TransportAddress) -> NvmeOfResult<Box<dyn TransportConnection>> {
        let client_config = self.create_client_config()?;

        // Create client endpoint bound to any available port
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
        endpoint.set_default_client_config(client_config);

        // connect() returns Result<Connecting, ConnectError>
        // Connecting is the future we need to await
        let connecting = endpoint
            .connect(addr.addr, &self.config.server_name)
            .map_err(|e| NvmeOfError::Transport(format!("Connect failed: {}", e)))?;

        let connection = tokio::time::timeout(self.config.connect_timeout, connecting)
            .await
            .map_err(|_| NvmeOfError::Timeout("Connection timeout".to_string()))?
            .map_err(|e| NvmeOfError::Transport(format!("Connection failed: {}", e)))?;

        let local_addr = endpoint.local_addr()?;
        let remote_addr = connection.remote_address();
        let connection_id = self.next_connection_id();

        debug!("Connected to NVMe-oF QUIC target at {}", remote_addr);

        let conn = QuicConnection::new_for_initiator(
            connection,
            local_addr,
            remote_addr,
            connection_id,
            self.config.clone(),
        )
        .await?;

        Ok(Box::new(conn))
    }

    async fn close(&self) -> NvmeOfResult<()> {
        let mut guard = self.endpoint.lock().await;
        if let Some(endpoint) = guard.take() {
            endpoint.close(0u32.into(), b"shutdown");
            endpoint.wait_idle().await;
        }
        Ok(())
    }

    async fn local_addr(&self) -> NvmeOfResult<SocketAddr> {
        self.local_addr
            .lock()
            .await
            .ok_or_else(|| NvmeOfError::Transport("Transport not bound".to_string()))
    }
}

/// QUIC Connection implementation
pub struct QuicConnection {
    /// Connection ID
    id: u64,

    /// QUIC connection
    connection: Connection,

    /// Local address
    local_addr: SocketAddr,

    /// Remote address
    remote_addr: SocketAddr,

    /// Configuration
    _config: NvmeOfQuicConfig,

    /// Send stream for commands/responses
    send_stream: Mutex<SendStream>,

    /// Receive stream for commands/responses
    recv_stream: Mutex<RecvStream>,

    /// Connection state
    connected: AtomicBool,

    /// Connection state
    state: parking_lot::RwLock<ConnectionState>,
}

impl QuicConnection {
    /// Create a new QUIC connection for initiator (client)
    /// Opens a bidirectional stream to the server
    pub async fn new_for_initiator(
        connection: Connection,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        id: u64,
        config: NvmeOfQuicConfig,
    ) -> NvmeOfResult<Self> {
        // Client opens a bidirectional stream for NVMe commands
        let (send_stream, recv_stream) = connection
            .open_bi()
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Failed to open stream: {}", e)))?;

        Ok(Self {
            id,
            connection,
            local_addr,
            remote_addr,
            _config: config,
            send_stream: Mutex::new(send_stream),
            recv_stream: Mutex::new(recv_stream),
            connected: AtomicBool::new(true),
            state: parking_lot::RwLock::new(ConnectionState::Connecting),
        })
    }

    /// Create a new QUIC connection for target (server)
    /// Accepts a bidirectional stream from the client
    pub async fn new_for_target(
        connection: Connection,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        id: u64,
        config: NvmeOfQuicConfig,
    ) -> NvmeOfResult<Self> {
        // Server accepts a bidirectional stream from the client
        let (send_stream, recv_stream) = connection
            .accept_bi()
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Failed to accept stream: {}", e)))?;

        Ok(Self {
            id,
            connection,
            local_addr,
            remote_addr,
            _config: config,
            send_stream: Mutex::new(send_stream),
            recv_stream: Mutex::new(recv_stream),
            connected: AtomicBool::new(true),
            state: parking_lot::RwLock::new(ConnectionState::Connecting),
        })
    }

    /// Perform ICReq/ICResp handshake (initiator side)
    pub async fn initialize_connection_initiator(&self) -> NvmeOfResult<()> {
        *self.state.write() = ConnectionState::Initializing;

        let icreq = IcReq::new();
        self.send_icreq(&icreq).await?;
        let _icresp = self.recv_icresp().await?;

        *self.state.write() = ConnectionState::Ready;
        debug!("QUIC connection initialized (initiator)");
        Ok(())
    }

    /// Perform ICReq/ICResp handshake (target side)
    pub async fn initialize_connection_target(&self) -> NvmeOfResult<IcReq> {
        *self.state.write() = ConnectionState::Initializing;

        let icreq = self.recv_icreq().await?;
        let icresp = IcResp::new();
        self.send_icresp(&icresp).await?;

        *self.state.write() = ConnectionState::Ready;
        debug!("QUIC connection initialized (target)");
        Ok(icreq)
    }

    async fn send_icreq(&self, icreq: &IcReq) -> NvmeOfResult<()> {
        let mut buf = BytesMut::with_capacity(128);
        let header = PduHeader::new(PduType::IcReq, 32, 0, 128);
        buf.put(header.to_bytes());
        buf.put(icreq.to_bytes());
        buf.resize(128, 0);

        let mut send = self.send_stream.lock().await;
        send.write_all(&buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Write failed: {}", e)))?;

        trace!("Sent ICReq PDU via QUIC");
        Ok(())
    }

    async fn recv_icreq(&self) -> NvmeOfResult<IcReq> {
        let mut buf = [0u8; 128];
        let mut recv = self.recv_stream.lock().await;

        recv.read_exact(&mut buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let header = PduHeader::from_bytes(&buf[..8])?;
        if header.pdu_type != PduType::IcReq {
            return Err(NvmeOfError::Protocol(format!(
                "Expected ICReq, got {:?}",
                header.pdu_type
            )));
        }

        let icreq = IcReq::from_bytes(&buf[8..])?;
        trace!("Received ICReq PDU via QUIC");
        Ok(icreq)
    }

    async fn send_icresp(&self, icresp: &IcResp) -> NvmeOfResult<()> {
        let mut buf = BytesMut::with_capacity(128);
        let header = PduHeader::new(PduType::IcResp, 32, 0, 128);
        buf.put(header.to_bytes());
        buf.put(icresp.to_bytes());
        buf.resize(128, 0);

        let mut send = self.send_stream.lock().await;
        send.write_all(&buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Write failed: {}", e)))?;

        trace!("Sent ICResp PDU via QUIC");
        Ok(())
    }

    async fn recv_icresp(&self) -> NvmeOfResult<IcResp> {
        let mut buf = [0u8; 128];
        let mut recv = self.recv_stream.lock().await;

        recv.read_exact(&mut buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let header = PduHeader::from_bytes(&buf[..8])?;
        if header.pdu_type != PduType::IcResp {
            return Err(NvmeOfError::Protocol(format!(
                "Expected ICResp, got {:?}",
                header.pdu_type
            )));
        }

        Ok(IcResp::new())
    }

    /// Get connection state
    pub fn state(&self) -> ConnectionState {
        *self.state.read()
    }
}

#[async_trait]
impl TransportConnection for QuicConnection {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    async fn initialize_as_target(&self) -> NvmeOfResult<()> {
        self.initialize_connection_target().await.map(|_| ())
    }

    async fn initialize_as_initiator(&self) -> NvmeOfResult<()> {
        self.initialize_connection_initiator().await
    }

    async fn send_command(&self, capsule: &CommandCapsule) -> NvmeOfResult<()> {
        let cmd_bytes = capsule.to_bytes();
        let plen = 8 + cmd_bytes.len() as u32;

        let mut buf = BytesMut::with_capacity(plen as usize);

        let pdo = if capsule.data.is_some() { 16u8 } else { 0 };
        let header = PduHeader::new(
            PduType::CapsuleCmd,
            ((8 + NvmeCommand::SIZE) / 4) as u8,
            pdo,
            plen,
        );

        buf.put(header.to_bytes());
        buf.put(cmd_bytes);

        let mut send = self.send_stream.lock().await;
        send.write_all(&buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Write failed: {}", e)))?;

        trace!(
            "Sent command capsule via QUIC, cid={}",
            capsule.command.cid()
        );
        Ok(())
    }

    async fn recv_command(&self) -> NvmeOfResult<CommandCapsule> {
        let mut header_buf = [0u8; 8];
        let mut recv = self.recv_stream.lock().await;

        recv.read_exact(&mut header_buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let header = PduHeader::from_bytes(&header_buf)?;

        if header.pdu_type != PduType::CapsuleCmd {
            return Err(NvmeOfError::Protocol(format!(
                "Expected CapsuleCmd, got {:?}",
                header.pdu_type
            )));
        }

        let remaining = header.plen as usize - 8;
        let mut pdu_buf = vec![0u8; remaining];
        recv.read_exact(&mut pdu_buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let capsule = CommandCapsule::from_bytes(&pdu_buf)?;
        trace!(
            "Received command capsule via QUIC, cid={}",
            capsule.command.cid()
        );
        Ok(capsule)
    }

    async fn send_response(&self, capsule: &ResponseCapsule) -> NvmeOfResult<()> {
        let resp_bytes = capsule.to_bytes();
        let plen = 8 + resp_bytes.len() as u32;

        let mut buf = BytesMut::with_capacity(plen as usize);

        let header = PduHeader::new(
            PduType::CapsuleResp,
            ((8 + NvmeCompletion::SIZE) / 4) as u8,
            0,
            plen,
        );

        buf.put(header.to_bytes());
        buf.put(resp_bytes);

        let mut send = self.send_stream.lock().await;
        send.write_all(&buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Write failed: {}", e)))?;

        trace!(
            "Sent response capsule via QUIC, cid={}",
            capsule.completion.cid
        );
        Ok(())
    }

    async fn recv_response(&self) -> NvmeOfResult<ResponseCapsule> {
        let mut header_buf = [0u8; 8];
        let mut recv = self.recv_stream.lock().await;

        recv.read_exact(&mut header_buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let header = PduHeader::from_bytes(&header_buf)?;

        if header.pdu_type != PduType::CapsuleResp {
            return Err(NvmeOfError::Protocol(format!(
                "Expected CapsuleResp, got {:?}",
                header.pdu_type
            )));
        }

        let remaining = header.plen as usize - 8;
        let mut pdu_buf = vec![0u8; remaining];
        recv.read_exact(&mut pdu_buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let capsule = ResponseCapsule::from_bytes(&pdu_buf)?;
        trace!(
            "Received response capsule via QUIC, cid={}",
            capsule.completion.cid
        );
        Ok(capsule)
    }

    async fn send_data(&self, data: Bytes, offset: u64) -> NvmeOfResult<()> {
        let data_len = data.len();
        let plen = 24 + data_len as u32;

        let mut buf = BytesMut::with_capacity(plen as usize);

        let header = PduHeader::new(PduType::C2HData, 6, 6, plen);
        buf.put(header.to_bytes());

        // C2H Data header (16 bytes)
        buf.put_u16_le(0); // CID
        buf.put_u16_le(0); // TTAG
        buf.put_u32_le(offset as u32);
        buf.put_u32_le(data_len as u32);
        buf.put_u32_le(0); // Reserved

        buf.put(data);

        let mut send = self.send_stream.lock().await;
        send.write_all(&buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Write failed: {}", e)))?;

        trace!(
            "Sent {} bytes of data via QUIC at offset {}",
            data_len, offset
        );
        Ok(())
    }

    async fn recv_data(&self, _length: usize) -> NvmeOfResult<Bytes> {
        let mut header_buf = [0u8; 8];
        let mut recv = self.recv_stream.lock().await;

        recv.read_exact(&mut header_buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let header = PduHeader::from_bytes(&header_buf)?;

        if header.pdu_type != PduType::H2CData && header.pdu_type != PduType::C2HData {
            return Err(NvmeOfError::Protocol(format!(
                "Expected data PDU, got {:?}",
                header.pdu_type
            )));
        }

        let remaining = header.plen as usize - 8;
        let mut pdu_buf = vec![0u8; remaining];
        recv.read_exact(&mut pdu_buf)
            .await
            .map_err(|e| NvmeOfError::Transport(format!("Read failed: {}", e)))?;

        let data = if remaining > 16 {
            Bytes::copy_from_slice(&pdu_buf[16..])
        } else {
            Bytes::new()
        };

        trace!("Received {} bytes of data via QUIC", data.len());
        Ok(data)
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed) && self.connection.close_reason().is_none()
    }

    async fn close(&self) -> NvmeOfResult<()> {
        *self.state.write() = ConnectionState::Closing;
        self.connected.store(false, Ordering::Relaxed);

        // Finish the send stream
        {
            let mut send = self.send_stream.lock().await;
            let _ = send.finish();
        }

        // Close the connection gracefully
        self.connection.close(0u32.into(), b"close");

        *self.state.write() = ConnectionState::Closed;
        debug!("QUIC connection {} closed", self.id);
        Ok(())
    }

    fn connection_id(&self) -> u64 {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quic_config_default() {
        let config = NvmeOfQuicConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_concurrent_bidi_streams, 256);
        assert_eq!(config.idle_timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_quic_transport_creation() {
        let config = NvmeOfQuicConfig::default();
        let transport = QuicTransport::new(config);
        assert!(transport.capabilities().multi_stream);
    }
}
