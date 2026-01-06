//! RDMA Transport for NVMe-oF
//!
//! Implements NVMe-oF over RDMA for high-performance, low-latency storage access.
//!
//! # Features
//!
//! - Zero-copy data transfer
//! - One-sided RDMA operations (RDMA Read/Write)
//! - Memory registration and pinning
//! - Reliable Connected (RC) queue pairs
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                   RdmaTransport                         │
//! ├─────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
//! │  │ RDMA Context │  │ Completion   │  │ Memory       │  │
//! │  │ (ibv_context)│  │ Channel      │  │ Regions      │  │
//! │  └──────────────┘  └──────────────┘  └──────────────┘  │
//! │                                                         │
//! │  ┌──────────────────────────────────────────────────┐  │
//! │  │              RdmaConnection Pool                  │  │
//! │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐           │  │
//! │  │  │  QP 1   │  │  QP 2   │  │  QP 3   │  ...      │  │
//! │  │  │ (RC)    │  │ (RC)    │  │ (RC)    │           │  │
//! │  │  └─────────┘  └─────────┘  └─────────┘           │  │
//! │  └──────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::RwLock;
use tracing::{debug, info, trace, warn};

use super::{
    ConnectionState, NvmeOfTransport, TransportAddress, TransportCapabilities, TransportConnection,
    TransportStats,
};
use crate::nvmeof::capsule::{CommandCapsule, ResponseCapsule};
use crate::nvmeof::config::TransportType;
use crate::nvmeof::error::{NvmeOfError, NvmeOfResult};

/// RDMA transport configuration
#[derive(Debug, Clone)]
pub struct RdmaConfig {
    /// Device name (e.g., "mlx5_0")
    pub device_name: Option<String>,

    /// Port number on the device
    pub port_num: u8,

    /// GID index to use
    pub gid_index: u16,

    /// Maximum send work requests
    pub max_send_wr: u32,

    /// Maximum receive work requests
    pub max_recv_wr: u32,

    /// Maximum scatter-gather elements
    pub max_sge: u32,

    /// Maximum inline data size
    pub max_inline_data: u32,

    /// Memory region size for pre-registration
    pub mr_size: usize,

    /// Enable SRQ (Shared Receive Queue)
    pub use_srq: bool,

    /// Completion queue size
    pub cq_size: u32,

    /// Use device memory (if available)
    pub use_device_memory: bool,

    /// Enable zero-copy operations
    pub enable_zero_copy: bool,
}

impl Default for RdmaConfig {
    fn default() -> Self {
        Self {
            device_name: None,
            port_num: 1,
            gid_index: 0,
            max_send_wr: 128,
            max_recv_wr: 128,
            max_sge: 4,
            max_inline_data: 256,
            mr_size: 64 * 1024 * 1024, // 64 MB
            use_srq: true,
            cq_size: 4096,
            use_device_memory: false,
            enable_zero_copy: true,
        }
    }
}

/// RDMA memory region handle
#[derive(Debug)]
pub struct MemoryRegion {
    /// Region ID
    pub id: u64,

    /// Base address
    pub addr: usize,

    /// Length
    pub length: usize,

    /// Local key
    pub lkey: u32,

    /// Remote key
    pub rkey: u32,

    /// Is registered
    pub registered: bool,
}

impl MemoryRegion {
    /// Create a new memory region (simulated)
    pub fn new(id: u64, length: usize) -> Self {
        Self {
            id,
            addr: 0,
            length,
            lkey: (id as u32) | 0x1000,
            rkey: (id as u32) | 0x2000,
            registered: true,
        }
    }
}

/// RDMA queue pair state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePairState {
    Reset,
    Init,
    Rtr, // Ready to Receive
    Rts, // Ready to Send
    Error,
}

/// RDMA transport implementation
pub struct RdmaTransport {
    /// Configuration
    config: RdmaConfig,

    /// Local address
    local_addr: RwLock<Option<SocketAddr>>,

    /// Is listening
    listening: AtomicBool,

    /// Connection counter
    conn_counter: AtomicU64,

    /// Active connections
    connections: RwLock<HashMap<u64, Arc<RdmaConnection>>>,

    /// Memory regions
    memory_regions: RwLock<Vec<Arc<MemoryRegion>>>,

    /// Statistics
    stats: RwLock<TransportStats>,

    /// RDMA context available
    context_available: bool,
}

impl RdmaTransport {
    /// Create a new RDMA transport
    pub fn new(config: RdmaConfig) -> Self {
        let context_available = Self::check_rdma_available();

        if context_available {
            info!(
                "RDMA transport initialized with device: {:?}",
                config.device_name
            );
        } else {
            warn!("RDMA not available, using simulation mode");
        }

        Self {
            config,
            local_addr: RwLock::new(None),
            listening: AtomicBool::new(false),
            conn_counter: AtomicU64::new(0),
            connections: RwLock::new(HashMap::new()),
            memory_regions: RwLock::new(Vec::new()),
            stats: RwLock::new(TransportStats::default()),
            context_available,
        }
    }

    /// Check if RDMA is available on this system
    fn check_rdma_available() -> bool {
        // In real implementation, would check:
        // 1. /sys/class/infiniband/ exists
        // 2. ibverbs library is available
        // 3. At least one device is present

        // For now, check environment variable or return false
        std::env::var("WARP_RDMA_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    }

    /// Allocate and register a memory region
    pub fn allocate_mr(&self, size: usize) -> NvmeOfResult<Arc<MemoryRegion>> {
        let id = self.conn_counter.fetch_add(1, Ordering::Relaxed);
        let mr = Arc::new(MemoryRegion::new(id, size));

        self.memory_regions.write().push(mr.clone());
        debug!("Allocated memory region {} (size={})", id, size);

        Ok(mr)
    }

    /// Deregister a memory region
    pub fn deregister_mr(&self, mr_id: u64) -> NvmeOfResult<()> {
        let mut regions = self.memory_regions.write();
        if let Some(pos) = regions.iter().position(|mr| mr.id == mr_id) {
            regions.remove(pos);
            debug!("Deregistered memory region {}", mr_id);
        }
        Ok(())
    }

    /// Get RDMA device info
    pub fn device_info(&self) -> RdmaDeviceInfo {
        RdmaDeviceInfo {
            device_name: self
                .config
                .device_name
                .clone()
                .unwrap_or_else(|| "simulated".to_string()),
            port_num: self.config.port_num,
            available: self.context_available,
            max_qp: 16384,
            max_cq: 65535,
            max_mr: 16384,
            max_pd: 16384,
        }
    }
}

#[async_trait]
impl NvmeOfTransport for RdmaTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Rdma
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_inline_data: self.config.max_inline_data,
            max_io_size: 16 * 1024 * 1024, // 16 MB for RDMA
            header_digest: false,          // RDMA doesn't need digest
            data_digest: false,
            zero_copy: self.config.enable_zero_copy,
            memory_registration: true,
            multi_stream: false,
        }
    }

    async fn bind(&mut self, addr: SocketAddr) -> NvmeOfResult<()> {
        *self.local_addr.write() = Some(addr);
        self.listening.store(true, Ordering::Release);

        info!("RDMA transport bound to {}", addr);
        Ok(())
    }

    async fn accept(&self) -> NvmeOfResult<Box<dyn TransportConnection>> {
        if !self.listening.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not listening".to_string()));
        }

        let local_addr = self
            .local_addr
            .read()
            .ok_or_else(|| NvmeOfError::Transport("Not bound".to_string()))?;

        // In real implementation, would wait for CM (Connection Manager) event
        // For simulation, just create a new connection
        let conn_id = self.conn_counter.fetch_add(1, Ordering::Relaxed);

        // Simulated remote address
        let remote_addr = SocketAddr::from(([127, 0, 0, 1], 0));

        let conn = Arc::new(RdmaConnection::new(
            conn_id,
            local_addr,
            remote_addr,
            self.config.clone(),
        ));

        self.connections.write().insert(conn_id, conn.clone());
        self.stats.write().queue_depth += 1;

        debug!(
            "Accepted RDMA connection {} from {} (simulated)",
            conn_id, remote_addr
        );

        Ok(Box::new(RdmaConnectionHandle { inner: conn }))
    }

    async fn connect(&self, addr: &TransportAddress) -> NvmeOfResult<Box<dyn TransportConnection>> {
        if addr.transport != TransportType::Rdma {
            return Err(NvmeOfError::Transport(format!(
                "Expected RDMA transport, got {:?}",
                addr.transport
            )));
        }

        let conn_id = self.conn_counter.fetch_add(1, Ordering::Relaxed);

        let local_addr = self
            .local_addr
            .read()
            .unwrap_or(SocketAddr::from(([0, 0, 0, 0], 0)));

        let conn = Arc::new(RdmaConnection::new(
            conn_id,
            local_addr,
            addr.addr,
            self.config.clone(),
        ));

        // In real implementation, would:
        // 1. Resolve route via RDMA CM
        // 2. Create queue pair
        // 3. Exchange QP info
        // 4. Transition QP to RTS

        conn.set_state(ConnectionState::Ready);

        self.connections.write().insert(conn_id, conn.clone());
        self.stats.write().queue_depth += 1;

        debug!(
            "Connected to {} via RDMA (connection {})",
            addr.addr, conn_id
        );

        Ok(Box::new(RdmaConnectionHandle { inner: conn }))
    }

    async fn close(&self) -> NvmeOfResult<()> {
        self.listening.store(false, Ordering::Release);

        // Close all connections
        let connections: Vec<_> = self.connections.read().values().cloned().collect();
        for conn in connections {
            conn.close_internal().await?;
        }

        self.connections.write().clear();
        self.stats.write().queue_depth = 0;

        info!("RDMA transport closed");
        Ok(())
    }
}

/// RDMA device information
#[derive(Debug, Clone)]
pub struct RdmaDeviceInfo {
    /// Device name
    pub device_name: String,
    /// Port number
    pub port_num: u8,
    /// Is available
    pub available: bool,
    /// Max queue pairs
    pub max_qp: u32,
    /// Max completion queues
    pub max_cq: u32,
    /// Max memory regions
    pub max_mr: u32,
    /// Max protection domains
    pub max_pd: u32,
}

/// Internal RDMA connection state
pub struct RdmaConnection {
    /// Connection ID
    id: u64,

    /// Local address
    local_addr: SocketAddr,

    /// Remote address
    remote_addr: SocketAddr,

    /// Configuration
    config: RdmaConfig,

    /// Connection state
    state: RwLock<ConnectionState>,

    /// Queue pair state
    qp_state: RwLock<QueuePairState>,

    /// Is connected
    connected: AtomicBool,

    /// Send buffer
    send_buffer: RwLock<Vec<u8>>,

    /// Receive buffer
    recv_buffer: RwLock<Vec<u8>>,

    /// Statistics
    stats: RwLock<TransportStats>,
}

impl RdmaConnection {
    /// Create a new RDMA connection
    fn new(id: u64, local_addr: SocketAddr, remote_addr: SocketAddr, config: RdmaConfig) -> Self {
        Self {
            id,
            local_addr,
            remote_addr,
            config,
            state: RwLock::new(ConnectionState::Connecting),
            qp_state: RwLock::new(QueuePairState::Reset),
            connected: AtomicBool::new(false),
            send_buffer: RwLock::new(Vec::with_capacity(64 * 1024)),
            recv_buffer: RwLock::new(Vec::with_capacity(64 * 1024)),
            stats: RwLock::new(TransportStats::default()),
        }
    }

    /// Set connection state
    fn set_state(&self, state: ConnectionState) {
        *self.state.write() = state;
        if state == ConnectionState::Ready {
            self.connected.store(true, Ordering::Release);
            *self.qp_state.write() = QueuePairState::Rts;
        }
    }

    /// Close the connection internally
    async fn close_internal(&self) -> NvmeOfResult<()> {
        self.connected.store(false, Ordering::Release);
        *self.state.write() = ConnectionState::Closed;
        *self.qp_state.write() = QueuePairState::Reset;
        Ok(())
    }

    /// Post a send work request (simulated)
    async fn post_send(&self, data: &[u8]) -> NvmeOfResult<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not connected".to_string()));
        }

        // In real implementation, would:
        // 1. Get send buffer from pool
        // 2. Copy data (or use zero-copy if inline)
        // 3. Post send WR to QP
        // 4. Wait for completion

        let mut stats = self.stats.write();
        stats.bytes_sent += data.len() as u64;

        trace!("Posted RDMA send of {} bytes", data.len());
        Ok(())
    }

    /// Post a receive work request (simulated)
    async fn post_recv(&self, length: usize) -> NvmeOfResult<Bytes> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not connected".to_string()));
        }

        // In real implementation, would:
        // 1. Get receive buffer from pool
        // 2. Post receive WR to QP
        // 3. Wait for completion
        // 4. Return received data

        let mut stats = self.stats.write();
        stats.bytes_received += length as u64;

        trace!("Posted RDMA receive for {} bytes", length);
        Ok(Bytes::from(vec![0u8; length]))
    }

    /// RDMA Write (one-sided, for C2H data)
    pub async fn rdma_write(&self, remote_addr: u64, rkey: u32, data: &[u8]) -> NvmeOfResult<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not connected".to_string()));
        }

        // In real implementation, would post RDMA_WRITE WR
        let mut stats = self.stats.write();
        stats.bytes_sent += data.len() as u64;

        trace!(
            "RDMA Write: {} bytes to remote 0x{:x}, rkey=0x{:x}",
            data.len(),
            remote_addr,
            rkey
        );
        Ok(())
    }

    /// RDMA Read (one-sided, for H2C data)
    pub async fn rdma_read(
        &self,
        remote_addr: u64,
        rkey: u32,
        length: usize,
    ) -> NvmeOfResult<Bytes> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not connected".to_string()));
        }

        // In real implementation, would post RDMA_READ WR
        let mut stats = self.stats.write();
        stats.bytes_received += length as u64;

        trace!(
            "RDMA Read: {} bytes from remote 0x{:x}, rkey=0x{:x}",
            length,
            remote_addr,
            rkey
        );
        Ok(Bytes::from(vec![0u8; length]))
    }
}

/// Handle wrapper for RdmaConnection that implements TransportConnection
struct RdmaConnectionHandle {
    inner: Arc<RdmaConnection>,
}

#[async_trait]
impl TransportConnection for RdmaConnectionHandle {
    fn remote_addr(&self) -> SocketAddr {
        self.inner.remote_addr
    }

    fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Rdma
    }

    async fn send_command(&self, capsule: &CommandCapsule) -> NvmeOfResult<()> {
        // Serialize capsule and send via RDMA
        let data = capsule.to_bytes();
        self.inner.post_send(&data).await?;

        let mut stats = self.inner.stats.write();
        stats.commands_sent += 1;

        Ok(())
    }

    async fn recv_command(&self) -> NvmeOfResult<CommandCapsule> {
        // Receive via RDMA and deserialize
        let data = self.inner.post_recv(128).await?;

        let mut stats = self.inner.stats.write();
        stats.commands_received += 1;

        CommandCapsule::from_bytes(&data)
    }

    async fn send_response(&self, capsule: &ResponseCapsule) -> NvmeOfResult<()> {
        let data = capsule.to_bytes();
        self.inner.post_send(&data).await?;

        let mut stats = self.inner.stats.write();
        stats.responses_sent += 1;

        Ok(())
    }

    async fn recv_response(&self) -> NvmeOfResult<ResponseCapsule> {
        let data = self.inner.post_recv(16).await?;

        let mut stats = self.inner.stats.write();
        stats.responses_received += 1;

        ResponseCapsule::from_bytes(&data)
    }

    async fn send_data(&self, data: Bytes, _offset: u64) -> NvmeOfResult<()> {
        // For RDMA, we use RDMA_WRITE for C2H transfers
        // In real implementation, would use registered memory and rkey
        self.inner.post_send(&data).await
    }

    async fn recv_data(&self, length: usize) -> NvmeOfResult<Bytes> {
        // For RDMA, we use RDMA_READ for H2C transfers
        self.inner.post_recv(length).await
    }

    fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Acquire)
    }

    async fn close(&self) -> NvmeOfResult<()> {
        self.inner.close_internal().await
    }

    fn connection_id(&self) -> u64 {
        self.inner.id
    }
}

/// RDMA completion queue entry (simulated)
#[derive(Debug, Clone)]
pub struct WorkCompletion {
    /// Work request ID
    pub wr_id: u64,
    /// Status
    pub status: WorkCompletionStatus,
    /// Opcode
    pub opcode: WorkCompletionOpcode,
    /// Bytes transferred
    pub byte_len: u32,
    /// Queue pair number
    pub qp_num: u32,
}

/// Work completion status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkCompletionStatus {
    Success,
    LocalLengthError,
    LocalQpOperationError,
    LocalProtectionError,
    WrFlushError,
    MemoryWindowBindError,
    BadResponseError,
    LocalAccessError,
    RemoteInvalidRequestError,
    RemoteAccessError,
    RemoteOperationError,
    TransportRetryExceeded,
    RnrRetryExceeded,
    GeneralError,
}

/// Work completion opcode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkCompletionOpcode {
    Send,
    RdmaWrite,
    RdmaRead,
    CompSwap,
    FetchAdd,
    Recv,
    RecvRdmaWithImm,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rdma_config_default() {
        let config = RdmaConfig::default();
        assert_eq!(config.port_num, 1);
        assert_eq!(config.max_send_wr, 128);
        assert!(config.enable_zero_copy);
    }

    #[test]
    fn test_memory_region() {
        let mr = MemoryRegion::new(1, 4096);
        assert_eq!(mr.id, 1);
        assert_eq!(mr.length, 4096);
        assert!(mr.registered);
    }

    #[tokio::test]
    async fn test_rdma_transport_creation() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        assert_eq!(transport.transport_type(), TransportType::Rdma);

        let caps = transport.capabilities();
        assert!(caps.memory_registration);
        assert!(caps.zero_copy);
    }

    #[tokio::test]
    async fn test_rdma_memory_region_allocation() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        let mr = transport.allocate_mr(4096).unwrap();
        assert_eq!(mr.length, 4096);
        assert!(mr.registered);

        transport.deregister_mr(mr.id).unwrap();
    }

    #[tokio::test]
    async fn test_rdma_device_info() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        let info = transport.device_info();
        assert!(!info.available); // No real RDMA in test environment
    }
}
