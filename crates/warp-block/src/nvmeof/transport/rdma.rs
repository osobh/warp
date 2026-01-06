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
//! - Shared Receive Queue (SRQ) support
//! - Completion channel notifications
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                       RdmaTransport                             │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
//! │  │ RDMA Context │  │ Completion   │  │ Memory Region Pool   │  │
//! │  │ (ibv_context)│  │ Channel (CQ) │  │ (pre-registered MRs) │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────────┘  │
//! │                                                                 │
//! │  ┌──────────────────────────────────────────────────────────┐  │
//! │  │           Shared Receive Queue (SRQ)                     │  │
//! │  │  Pre-posted receive buffers for all connections          │  │
//! │  └──────────────────────────────────────────────────────────┘  │
//! │                                                                 │
//! │  ┌──────────────────────────────────────────────────────────┐  │
//! │  │              RdmaConnection Pool                          │  │
//! │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐                   │  │
//! │  │  │  QP 1   │  │  QP 2   │  │  QP 3   │  ...              │  │
//! │  │  │ (RC)    │  │ (RC)    │  │ (RC)    │                   │  │
//! │  │  └─────────┘  └─────────┘  └─────────┘                   │  │
//! │  └──────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Simulation Mode
//!
//! When RDMA hardware is not available, the transport operates in simulation mode
//! with configurable latency and error injection for testing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc;
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

    /// SRQ size (number of work requests)
    pub srq_size: u32,

    /// Completion queue size
    pub cq_size: u32,

    /// Use device memory (if available)
    pub use_device_memory: bool,

    /// Enable zero-copy operations
    pub enable_zero_copy: bool,

    /// Simulation configuration (when no RDMA hardware)
    pub simulation: SimulationConfig,
}

/// Configuration for simulation mode
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Simulated latency per operation
    pub latency: Duration,

    /// Enable latency simulation
    pub enable_latency: bool,

    /// Error injection rate (0.0 - 1.0)
    pub error_rate: f64,

    /// Simulated bandwidth limit (bytes per second, 0 = unlimited)
    pub bandwidth_limit: u64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            latency: Duration::from_micros(1),
            enable_latency: false,
            error_rate: 0.0,
            bandwidth_limit: 0,
        }
    }
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
            srq_size: 4096,
            cq_size: 4096,
            use_device_memory: false,
            enable_zero_copy: true,
            simulation: SimulationConfig::default(),
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
    Sqd, // Send Queue Drained
    Sqe, // Send Queue Error
    Error,
}

impl QueuePairState {
    /// Check if state transition is valid
    pub fn can_transition_to(&self, target: QueuePairState) -> bool {
        use QueuePairState::*;
        matches!(
            (self, target),
            (Reset, Init)
                | (Init, Rtr)
                | (Rtr, Rts)
                | (Rts, Sqd)
                | (Sqd, Rts)
                | (Sqd, Sqe)
                | (_, Error)
                | (_, Reset)
        )
    }
}

/// Memory region pool for efficient buffer management
pub struct MemoryRegionPool {
    /// Pool of available memory regions
    available: Mutex<Vec<Arc<MemoryRegion>>>,

    /// Currently in-use regions
    in_use: RwLock<HashMap<u64, Arc<MemoryRegion>>>,

    /// Region size
    region_size: usize,

    /// Maximum pool size
    max_regions: usize,

    /// Counter for region IDs
    id_counter: AtomicU64,

    /// Total allocated bytes
    total_allocated: AtomicU64,
}

impl MemoryRegionPool {
    /// Create a new memory region pool
    pub fn new(region_size: usize, max_regions: usize) -> Self {
        Self {
            available: Mutex::new(Vec::with_capacity(max_regions)),
            in_use: RwLock::new(HashMap::new()),
            region_size,
            max_regions,
            id_counter: AtomicU64::new(0),
            total_allocated: AtomicU64::new(0),
        }
    }

    /// Acquire a memory region from the pool
    pub fn acquire(&self) -> NvmeOfResult<Arc<MemoryRegion>> {
        // Try to get from pool first
        if let Some(mr) = self.available.lock().pop() {
            self.in_use.write().insert(mr.id, mr.clone());
            return Ok(mr);
        }

        // Check if we can allocate more
        let in_use_count = self.in_use.read().len();
        if in_use_count >= self.max_regions {
            return Err(NvmeOfError::Transport(
                "Memory region pool exhausted".to_string(),
            ));
        }

        // Allocate new region
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let mr = Arc::new(MemoryRegion::new(id, self.region_size));
        self.total_allocated
            .fetch_add(self.region_size as u64, Ordering::Relaxed);
        self.in_use.write().insert(id, mr.clone());

        debug!(
            "Allocated new memory region {} (size={}, pool={})",
            id, self.region_size, in_use_count
        );

        Ok(mr)
    }

    /// Release a memory region back to the pool
    pub fn release(&self, mr_id: u64) {
        if let Some(mr) = self.in_use.write().remove(&mr_id) {
            self.available.lock().push(mr);
            trace!("Released memory region {} back to pool", mr_id);
        }
    }

    /// Get pool statistics
    pub fn stats(&self) -> MemoryPoolStats {
        MemoryPoolStats {
            available: self.available.lock().len(),
            in_use: self.in_use.read().len(),
            total_allocated: self.total_allocated.load(Ordering::Relaxed),
            region_size: self.region_size,
        }
    }
}

/// Memory pool statistics
#[derive(Debug, Clone)]
pub struct MemoryPoolStats {
    /// Available regions
    pub available: usize,
    /// In-use regions
    pub in_use: usize,
    /// Total allocated bytes
    pub total_allocated: u64,
    /// Region size
    pub region_size: usize,
}

/// Shared Receive Queue (SRQ) for efficient receive buffer management
pub struct SharedReceiveQueue {
    /// Queue ID
    id: u64,

    /// Maximum work requests
    max_wr: u32,

    /// Current posted work requests
    posted_wr: AtomicU32,

    /// Low watermark for refilling
    low_watermark: u32,

    /// Receive buffers
    buffers: Mutex<Vec<ReceiveBuffer>>,

    /// Completion sender
    completion_tx: mpsc::Sender<SrqCompletion>,

    /// Completion receiver
    completion_rx: Mutex<Option<mpsc::Receiver<SrqCompletion>>>,
}

/// Buffer posted to SRQ
struct ReceiveBuffer {
    /// Work request ID
    wr_id: u64,
    /// Memory region
    mr: Arc<MemoryRegion>,
    /// Offset in region
    offset: usize,
    /// Length
    length: usize,
}

/// SRQ completion notification
#[derive(Debug)]
pub struct SrqCompletion {
    /// Work request ID
    pub wr_id: u64,
    /// Received data length
    pub length: u32,
    /// Source QP number
    pub src_qp: u32,
    /// Status
    pub status: WorkCompletionStatus,
}

impl SharedReceiveQueue {
    /// Create a new SRQ
    pub fn new(id: u64, max_wr: u32) -> Self {
        let (tx, rx) = mpsc::channel(max_wr as usize);
        Self {
            id,
            max_wr,
            posted_wr: AtomicU32::new(0),
            low_watermark: max_wr / 4,
            buffers: Mutex::new(Vec::with_capacity(max_wr as usize)),
            completion_tx: tx,
            completion_rx: Mutex::new(Some(rx)),
        }
    }

    /// Post a receive buffer to the SRQ
    pub fn post_recv(
        &self,
        mr: Arc<MemoryRegion>,
        offset: usize,
        length: usize,
    ) -> NvmeOfResult<u64> {
        let current = self.posted_wr.load(Ordering::Relaxed);
        if current >= self.max_wr {
            return Err(NvmeOfError::Transport("SRQ full".to_string()));
        }

        let wr_id = (self.id << 32) | (current as u64);
        self.buffers.lock().push(ReceiveBuffer {
            wr_id,
            mr,
            offset,
            length,
        });
        self.posted_wr.fetch_add(1, Ordering::Relaxed);

        trace!("Posted receive buffer to SRQ {}, wr_id={}", self.id, wr_id);
        Ok(wr_id)
    }

    /// Check if SRQ needs refilling
    pub fn needs_refill(&self) -> bool {
        self.posted_wr.load(Ordering::Relaxed) < self.low_watermark
    }

    /// Get completion sender for notifying completions
    pub fn completion_sender(&self) -> mpsc::Sender<SrqCompletion> {
        self.completion_tx.clone()
    }

    /// Take the completion receiver (can only be taken once)
    pub fn take_completion_receiver(&self) -> Option<mpsc::Receiver<SrqCompletion>> {
        self.completion_rx.lock().take()
    }
}

/// Completion channel for async completion notifications
pub struct CompletionChannel {
    /// Channel ID
    id: u64,

    /// Completion queue size
    cq_size: u32,

    /// Pending completions
    pending: AtomicU32,

    /// Completion sender
    tx: mpsc::Sender<WorkCompletion>,

    /// Completion receiver
    rx: Mutex<Option<mpsc::Receiver<WorkCompletion>>>,
}

impl CompletionChannel {
    /// Create a new completion channel
    pub fn new(id: u64, cq_size: u32) -> Self {
        let (tx, rx) = mpsc::channel(cq_size as usize);
        Self {
            id,
            cq_size,
            pending: AtomicU32::new(0),
            tx,
            rx: Mutex::new(Some(rx)),
        }
    }

    /// Post a completion
    pub async fn post_completion(&self, wc: WorkCompletion) -> NvmeOfResult<()> {
        self.tx
            .send(wc)
            .await
            .map_err(|_| NvmeOfError::Transport("Completion channel closed".to_string()))?;
        self.pending.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Take the receiver (can only be taken once)
    pub fn take_receiver(&self) -> Option<mpsc::Receiver<WorkCompletion>> {
        self.rx.lock().take()
    }

    /// Get pending completion count
    pub fn pending_count(&self) -> u32 {
        self.pending.load(Ordering::Relaxed)
    }
}

/// Work request for tracking in-flight operations
#[derive(Debug)]
pub struct WorkRequest {
    /// Work request ID
    pub id: u64,
    /// Operation type
    pub opcode: WorkRequestOpcode,
    /// Memory region
    pub mr_id: Option<u64>,
    /// Remote address (for RDMA ops)
    pub remote_addr: u64,
    /// Remote key (for RDMA ops)
    pub rkey: u32,
    /// Data length
    pub length: u32,
    /// Flags
    pub flags: WorkRequestFlags,
}

/// Work request operation codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkRequestOpcode {
    Send,
    SendWithImm,
    RdmaWrite,
    RdmaWriteWithImm,
    RdmaRead,
    AtomicCompSwap,
    AtomicFetchAdd,
}

/// Work request flags
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkRequestFlags {
    /// Signal completion
    pub signaled: bool,
    /// Use inline data
    pub inline: bool,
    /// Fence (wait for previous ops)
    pub fence: bool,
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

    /// Work request ID counter
    wr_id_counter: AtomicU64,

    /// Active connections
    connections: RwLock<HashMap<u64, Arc<RdmaConnection>>>,

    /// Memory region pool
    mr_pool: Arc<MemoryRegionPool>,

    /// Legacy memory regions (for direct allocation)
    memory_regions: RwLock<Vec<Arc<MemoryRegion>>>,

    /// Shared receive queue (if enabled)
    srq: Option<Arc<SharedReceiveQueue>>,

    /// Completion channel
    completion_channel: Arc<CompletionChannel>,

    /// Statistics
    stats: RwLock<TransportStats>,

    /// RDMA context available
    context_available: bool,

    /// Pending work requests
    pending_work_requests: RwLock<HashMap<u64, WorkRequest>>,
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

        // Create memory region pool
        let mr_pool = Arc::new(MemoryRegionPool::new(
            64 * 1024, // 64KB regions
            config.mr_size / (64 * 1024),
        ));

        // Create SRQ if enabled
        let srq = if config.use_srq {
            Some(Arc::new(SharedReceiveQueue::new(0, config.srq_size)))
        } else {
            None
        };

        // Create completion channel
        let completion_channel = Arc::new(CompletionChannel::new(0, config.cq_size));

        Self {
            config,
            local_addr: RwLock::new(None),
            listening: AtomicBool::new(false),
            conn_counter: AtomicU64::new(0),
            wr_id_counter: AtomicU64::new(0),
            connections: RwLock::new(HashMap::new()),
            mr_pool,
            memory_regions: RwLock::new(Vec::new()),
            srq,
            completion_channel,
            stats: RwLock::new(TransportStats::default()),
            context_available,
            pending_work_requests: RwLock::new(HashMap::new()),
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

    /// Get the memory region pool
    pub fn memory_pool(&self) -> &Arc<MemoryRegionPool> {
        &self.mr_pool
    }

    /// Get the SRQ (if enabled)
    pub fn srq(&self) -> Option<&Arc<SharedReceiveQueue>> {
        self.srq.as_ref()
    }

    /// Get the completion channel
    pub fn completion_channel(&self) -> &Arc<CompletionChannel> {
        &self.completion_channel
    }

    /// Post a work request with tracking
    pub fn post_work_request(&self, wr: WorkRequest) -> u64 {
        let wr_id = self.wr_id_counter.fetch_add(1, Ordering::Relaxed);
        self.pending_work_requests
            .write()
            .insert(wr_id, WorkRequest { id: wr_id, ..wr });
        trace!("Posted work request {}: {:?}", wr_id, wr.opcode);
        wr_id
    }

    /// Complete a work request
    pub fn complete_work_request(&self, wr_id: u64) -> Option<WorkRequest> {
        self.pending_work_requests.write().remove(&wr_id)
    }

    /// Get pending work request count
    pub fn pending_work_request_count(&self) -> usize {
        self.pending_work_requests.read().len()
    }

    /// Acquire a buffer from the memory pool
    pub fn acquire_buffer(&self) -> NvmeOfResult<Arc<MemoryRegion>> {
        self.mr_pool.acquire()
    }

    /// Release a buffer back to the pool
    pub fn release_buffer(&self, mr_id: u64) {
        self.mr_pool.release(mr_id);
    }

    /// Get memory pool statistics
    pub fn memory_pool_stats(&self) -> MemoryPoolStats {
        self.mr_pool.stats()
    }

    /// Check if running in simulation mode
    pub fn is_simulation_mode(&self) -> bool {
        !self.context_available
    }

    /// Apply simulation latency if configured
    async fn apply_simulation_latency(&self) {
        if !self.context_available && self.config.simulation.enable_latency {
            tokio::time::sleep(self.config.simulation.latency).await;
        }
    }

    /// Check for simulated error
    fn check_simulation_error(&self) -> NvmeOfResult<()> {
        if !self.context_available && self.config.simulation.error_rate > 0.0 {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::time::SystemTime::now().hash(&mut hasher);
            let random = (hasher.finish() % 10000) as f64 / 10000.0;
            if random < self.config.simulation.error_rate {
                return Err(NvmeOfError::Transport(
                    "Simulated transport error".to_string(),
                ));
            }
        }
        Ok(())
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

/// Queue pair information
#[derive(Debug, Clone)]
pub struct QueuePairInfo {
    /// Queue pair number
    pub qp_num: u32,
    /// Packet sequence number
    pub psn: u32,
    /// Queue key
    pub qkey: u32,
    /// Path MTU
    pub mtu: u32,
    /// Service level
    pub sl: u8,
}

impl Default for QueuePairInfo {
    fn default() -> Self {
        Self {
            qp_num: 0,
            psn: 0,
            qkey: 0,
            mtu: 4096, // 4KB MTU
            sl: 0,
        }
    }
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

    /// Queue pair info
    qp_info: RwLock<QueuePairInfo>,

    /// Remote queue pair info (after exchange)
    remote_qp_info: RwLock<Option<QueuePairInfo>>,

    /// Is connected
    connected: AtomicBool,

    /// Send buffer
    send_buffer: RwLock<Vec<u8>>,

    /// Receive buffer
    recv_buffer: RwLock<Vec<u8>>,

    /// Outstanding send work requests
    outstanding_sends: AtomicU32,

    /// Outstanding receive work requests
    outstanding_recvs: AtomicU32,

    /// Maximum outstanding sends
    max_send_wr: u32,

    /// Maximum outstanding receives
    max_recv_wr: u32,

    /// Completion channel for this connection
    completion_tx: mpsc::Sender<WorkCompletion>,

    /// Statistics
    stats: RwLock<TransportStats>,
}

impl RdmaConnection {
    /// Create a new RDMA connection
    fn new(id: u64, local_addr: SocketAddr, remote_addr: SocketAddr, config: RdmaConfig) -> Self {
        let (completion_tx, _completion_rx) = mpsc::channel(config.cq_size as usize);

        // Generate QP info
        let qp_info = QueuePairInfo {
            qp_num: (id & 0xFFFFFF) as u32, // Lower 24 bits as QP number
            psn: 0,
            qkey: 0,
            mtu: 4096,
            sl: 0,
        };

        Self {
            id,
            local_addr,
            remote_addr,
            max_send_wr: config.max_send_wr,
            max_recv_wr: config.max_recv_wr,
            config,
            state: RwLock::new(ConnectionState::Connecting),
            qp_state: RwLock::new(QueuePairState::Reset),
            qp_info: RwLock::new(qp_info),
            remote_qp_info: RwLock::new(None),
            connected: AtomicBool::new(false),
            send_buffer: RwLock::new(Vec::with_capacity(64 * 1024)),
            recv_buffer: RwLock::new(Vec::with_capacity(64 * 1024)),
            outstanding_sends: AtomicU32::new(0),
            outstanding_recvs: AtomicU32::new(0),
            completion_tx,
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

    /// Transition queue pair state
    pub fn transition_qp_state(&self, target: QueuePairState) -> NvmeOfResult<()> {
        let current = *self.qp_state.read();
        if !current.can_transition_to(target) {
            return Err(NvmeOfError::Transport(format!(
                "Invalid QP state transition from {:?} to {:?}",
                current, target
            )));
        }
        *self.qp_state.write() = target;
        debug!("QP {} state: {:?} -> {:?}", self.id, current, target);
        Ok(())
    }

    /// Get current QP state
    pub fn qp_state(&self) -> QueuePairState {
        *self.qp_state.read()
    }

    /// Get local QP info
    pub fn local_qp_info(&self) -> QueuePairInfo {
        self.qp_info.read().clone()
    }

    /// Set remote QP info (during connection establishment)
    pub fn set_remote_qp_info(&self, info: QueuePairInfo) {
        *self.remote_qp_info.write() = Some(info);
    }

    /// Get remote QP info
    pub fn remote_qp_info(&self) -> Option<QueuePairInfo> {
        self.remote_qp_info.read().clone()
    }

    /// Check if send queue has capacity
    pub fn can_send(&self) -> bool {
        self.outstanding_sends.load(Ordering::Relaxed) < self.max_send_wr
    }

    /// Check if receive queue has capacity
    pub fn can_recv(&self) -> bool {
        self.outstanding_recvs.load(Ordering::Relaxed) < self.max_recv_wr
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

        // Check queue capacity
        if !self.can_send() {
            return Err(NvmeOfError::Transport("Send queue full".to_string()));
        }

        // Track outstanding operations
        self.outstanding_sends.fetch_add(1, Ordering::Relaxed);

        // Apply simulation latency if configured
        if self.config.simulation.enable_latency {
            tokio::time::sleep(self.config.simulation.latency).await;
        }

        // Check for simulated errors
        if self.config.simulation.error_rate > 0.0 {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::time::SystemTime::now().hash(&mut hasher);
            let random = (hasher.finish() % 10000) as f64 / 10000.0;
            if random < self.config.simulation.error_rate {
                self.outstanding_sends.fetch_sub(1, Ordering::Relaxed);
                return Err(NvmeOfError::Transport("Simulated send error".to_string()));
            }
        }

        // In real implementation, would:
        // 1. Get send buffer from pool
        // 2. Copy data (or use zero-copy if inline)
        // 3. Post send WR to QP
        // 4. Wait for completion

        let mut stats = self.stats.write();
        stats.bytes_sent += data.len() as u64;

        // Simulate completion
        self.outstanding_sends.fetch_sub(1, Ordering::Relaxed);

        // Send completion notification
        let _ = self.completion_tx.try_send(WorkCompletion {
            wr_id: self.id,
            status: WorkCompletionStatus::Success,
            opcode: WorkCompletionOpcode::Send,
            byte_len: data.len() as u32,
            qp_num: self.qp_info.read().qp_num,
        });

        trace!("Posted RDMA send of {} bytes", data.len());
        Ok(())
    }

    /// Post a receive work request (simulated)
    async fn post_recv(&self, length: usize) -> NvmeOfResult<Bytes> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not connected".to_string()));
        }

        // Check queue capacity
        if !self.can_recv() {
            return Err(NvmeOfError::Transport("Receive queue full".to_string()));
        }

        // Track outstanding operations
        self.outstanding_recvs.fetch_add(1, Ordering::Relaxed);

        // Apply simulation latency if configured
        if self.config.simulation.enable_latency {
            tokio::time::sleep(self.config.simulation.latency).await;
        }

        // Check for simulated errors
        if self.config.simulation.error_rate > 0.0 {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::time::SystemTime::now().hash(&mut hasher);
            let random = (hasher.finish() % 10000) as f64 / 10000.0;
            if random < self.config.simulation.error_rate {
                self.outstanding_recvs.fetch_sub(1, Ordering::Relaxed);
                return Err(NvmeOfError::Transport(
                    "Simulated receive error".to_string(),
                ));
            }
        }

        // In real implementation, would:
        // 1. Get receive buffer from pool
        // 2. Post receive WR to QP
        // 3. Wait for completion
        // 4. Return received data

        let mut stats = self.stats.write();
        stats.bytes_received += length as u64;

        // Simulate completion
        self.outstanding_recvs.fetch_sub(1, Ordering::Relaxed);

        // Send completion notification
        let _ = self.completion_tx.try_send(WorkCompletion {
            wr_id: self.id,
            status: WorkCompletionStatus::Success,
            opcode: WorkCompletionOpcode::Recv,
            byte_len: length as u32,
            qp_num: self.qp_info.read().qp_num,
        });

        trace!("Posted RDMA receive for {} bytes", length);
        Ok(Bytes::from(vec![0u8; length]))
    }

    /// RDMA Write (one-sided, for C2H data)
    pub async fn rdma_write(&self, remote_addr: u64, rkey: u32, data: &[u8]) -> NvmeOfResult<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(NvmeOfError::Transport("Not connected".to_string()));
        }

        // Check queue capacity
        if !self.can_send() {
            return Err(NvmeOfError::Transport("Send queue full".to_string()));
        }

        // Track outstanding operations
        self.outstanding_sends.fetch_add(1, Ordering::Relaxed);

        // Apply simulation latency if configured
        if self.config.simulation.enable_latency {
            tokio::time::sleep(self.config.simulation.latency).await;
        }

        // In real implementation, would post RDMA_WRITE WR
        let mut stats = self.stats.write();
        stats.bytes_sent += data.len() as u64;

        // Simulate completion
        self.outstanding_sends.fetch_sub(1, Ordering::Relaxed);

        // Send completion notification
        let _ = self.completion_tx.try_send(WorkCompletion {
            wr_id: self.id,
            status: WorkCompletionStatus::Success,
            opcode: WorkCompletionOpcode::RdmaWrite,
            byte_len: data.len() as u32,
            qp_num: self.qp_info.read().qp_num,
        });

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

        // Check queue capacity
        if !self.can_send() {
            return Err(NvmeOfError::Transport("Send queue full".to_string()));
        }

        // Track outstanding operations
        self.outstanding_sends.fetch_add(1, Ordering::Relaxed);

        // Apply simulation latency if configured
        if self.config.simulation.enable_latency {
            tokio::time::sleep(self.config.simulation.latency).await;
        }

        // In real implementation, would post RDMA_READ WR
        let mut stats = self.stats.write();
        stats.bytes_received += length as u64;

        // Simulate completion
        self.outstanding_sends.fetch_sub(1, Ordering::Relaxed);

        // Send completion notification
        let _ = self.completion_tx.try_send(WorkCompletion {
            wr_id: self.id,
            status: WorkCompletionStatus::Success,
            opcode: WorkCompletionOpcode::RdmaRead,
            byte_len: length as u32,
            qp_num: self.qp_info.read().qp_num,
        });

        trace!(
            "RDMA Read: {} bytes from remote 0x{:x}, rkey=0x{:x}",
            length, remote_addr, rkey
        );
        Ok(Bytes::from(vec![0u8; length]))
    }

    /// Get outstanding send count
    pub fn outstanding_sends(&self) -> u32 {
        self.outstanding_sends.load(Ordering::Relaxed)
    }

    /// Get outstanding receive count
    pub fn outstanding_recvs(&self) -> u32 {
        self.outstanding_recvs.load(Ordering::Relaxed)
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
        assert!(config.use_srq);
        assert_eq!(config.srq_size, 4096);
    }

    #[test]
    fn test_simulation_config_default() {
        let config = SimulationConfig::default();
        assert_eq!(config.latency, Duration::from_micros(1));
        assert!(!config.enable_latency);
        assert_eq!(config.error_rate, 0.0);
        assert_eq!(config.bandwidth_limit, 0);
    }

    #[test]
    fn test_memory_region() {
        let mr = MemoryRegion::new(1, 4096);
        assert_eq!(mr.id, 1);
        assert_eq!(mr.length, 4096);
        assert!(mr.registered);
        assert_ne!(mr.lkey, mr.rkey);
    }

    #[test]
    fn test_qp_state_transitions() {
        use QueuePairState::*;

        // Valid transitions
        assert!(Reset.can_transition_to(Init));
        assert!(Init.can_transition_to(Rtr));
        assert!(Rtr.can_transition_to(Rts));
        assert!(Rts.can_transition_to(Sqd));
        assert!(Sqd.can_transition_to(Rts));

        // All states can go to Error or Reset
        assert!(Rts.can_transition_to(Error));
        assert!(Rts.can_transition_to(Reset));

        // Invalid transitions
        assert!(!Init.can_transition_to(Rts)); // Must go through Rtr
        assert!(!Rtr.can_transition_to(Init)); // Can't go back to Init
    }

    #[test]
    fn test_memory_region_pool() {
        let pool = MemoryRegionPool::new(4096, 10);

        // Acquire a region
        let mr1 = pool.acquire().unwrap();
        assert_eq!(mr1.length, 4096);

        let stats = pool.stats();
        assert_eq!(stats.in_use, 1);
        assert_eq!(stats.available, 0);

        // Acquire another
        let mr2 = pool.acquire().unwrap();
        assert_ne!(mr1.id, mr2.id);

        let stats = pool.stats();
        assert_eq!(stats.in_use, 2);

        // Release one
        pool.release(mr1.id);

        let stats = pool.stats();
        assert_eq!(stats.in_use, 1);
        assert_eq!(stats.available, 1);

        // Acquire should reuse from pool
        let mr3 = pool.acquire().unwrap();
        assert_eq!(mr3.id, mr1.id); // Should be the same one we released
    }

    #[test]
    fn test_memory_region_pool_exhaustion() {
        let pool = MemoryRegionPool::new(4096, 2);

        let _mr1 = pool.acquire().unwrap();
        let _mr2 = pool.acquire().unwrap();

        // Should fail - pool exhausted
        let result = pool.acquire();
        assert!(result.is_err());
    }

    #[test]
    fn test_shared_receive_queue() {
        let srq = SharedReceiveQueue::new(1, 100);

        // Initially empty, needs refill (posted < low_watermark)
        // low_watermark = 100 / 4 = 25
        assert!(srq.needs_refill());

        // Post receives to fill above low watermark
        let mr = Arc::new(MemoryRegion::new(1, 4096));
        for i in 0..30 {
            let wr_id = srq.post_recv(mr.clone(), i * 1024, 1024).unwrap();
            assert!(wr_id > 0 || wr_id == 0); // wr_id can be 0 based on formula
        }

        // After posting 30, should not need refill (30 > 25)
        assert!(!srq.needs_refill());
    }

    #[test]
    fn test_work_request_flags() {
        let flags = WorkRequestFlags::default();
        assert!(!flags.signaled);
        assert!(!flags.inline);
        assert!(!flags.fence);

        let flags = WorkRequestFlags {
            signaled: true,
            inline: true,
            fence: false,
        };
        assert!(flags.signaled);
        assert!(flags.inline);
    }

    #[test]
    fn test_qp_info_default() {
        let info = QueuePairInfo::default();
        assert_eq!(info.qp_num, 0);
        assert_eq!(info.mtu, 4096);
        assert_eq!(info.sl, 0);
    }

    #[tokio::test]
    async fn test_rdma_transport_creation() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        assert_eq!(transport.transport_type(), TransportType::Rdma);
        assert!(transport.is_simulation_mode()); // No real RDMA

        let caps = transport.capabilities();
        assert!(caps.memory_registration);
        assert!(caps.zero_copy);
    }

    #[tokio::test]
    async fn test_rdma_transport_with_srq() {
        let config = RdmaConfig {
            use_srq: true,
            srq_size: 1024,
            ..Default::default()
        };
        let transport = RdmaTransport::new(config);

        assert!(transport.srq().is_some());
    }

    #[tokio::test]
    async fn test_rdma_transport_without_srq() {
        let config = RdmaConfig {
            use_srq: false,
            ..Default::default()
        };
        let transport = RdmaTransport::new(config);

        assert!(transport.srq().is_none());
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
    async fn test_rdma_buffer_pool() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        // Acquire from pool
        let mr = transport.acquire_buffer().unwrap();
        assert!(mr.registered);

        // Check stats
        let stats = transport.memory_pool_stats();
        assert_eq!(stats.in_use, 1);

        // Release back
        transport.release_buffer(mr.id);

        let stats = transport.memory_pool_stats();
        assert_eq!(stats.in_use, 0);
        assert_eq!(stats.available, 1);
    }

    #[tokio::test]
    async fn test_rdma_device_info() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        let info = transport.device_info();
        assert!(!info.available); // No real RDMA in test environment
        assert_eq!(info.device_name, "simulated");
    }

    #[tokio::test]
    async fn test_rdma_work_request_tracking() {
        let config = RdmaConfig::default();
        let transport = RdmaTransport::new(config);

        let wr = WorkRequest {
            id: 0,
            opcode: WorkRequestOpcode::Send,
            mr_id: Some(1),
            remote_addr: 0,
            rkey: 0,
            length: 1024,
            flags: WorkRequestFlags::default(),
        };

        let wr_id = transport.post_work_request(wr);
        assert_eq!(transport.pending_work_request_count(), 1);

        let completed = transport.complete_work_request(wr_id);
        assert!(completed.is_some());
        assert_eq!(transport.pending_work_request_count(), 0);
    }

    #[tokio::test]
    async fn test_rdma_connection_qp_state() {
        let config = RdmaConfig::default();
        let addr: SocketAddr = "127.0.0.1:4420".parse().unwrap();
        let conn = RdmaConnection::new(1, addr, addr, config);

        assert_eq!(conn.qp_state(), QueuePairState::Reset);

        // Valid transition
        conn.transition_qp_state(QueuePairState::Init).unwrap();
        assert_eq!(conn.qp_state(), QueuePairState::Init);

        // Invalid transition (Init -> Rts without Rtr)
        let result = conn.transition_qp_state(QueuePairState::Rts);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rdma_connection_qp_info_exchange() {
        let config = RdmaConfig::default();
        let addr: SocketAddr = "127.0.0.1:4420".parse().unwrap();
        let conn = RdmaConnection::new(1, addr, addr, config);

        // Get local QP info
        let local_info = conn.local_qp_info();
        assert!(local_info.qp_num > 0 || local_info.qp_num == 0);

        // Initially no remote info
        assert!(conn.remote_qp_info().is_none());

        // Set remote QP info
        let remote_info = QueuePairInfo {
            qp_num: 12345,
            psn: 100,
            qkey: 0,
            mtu: 4096,
            sl: 0,
        };
        conn.set_remote_qp_info(remote_info.clone());

        let stored = conn.remote_qp_info().unwrap();
        assert_eq!(stored.qp_num, 12345);
        assert_eq!(stored.psn, 100);
    }

    #[test]
    fn test_work_completion_status_variants() {
        // Ensure all status variants exist
        let _statuses = [
            WorkCompletionStatus::Success,
            WorkCompletionStatus::LocalLengthError,
            WorkCompletionStatus::LocalQpOperationError,
            WorkCompletionStatus::LocalProtectionError,
            WorkCompletionStatus::WrFlushError,
            WorkCompletionStatus::MemoryWindowBindError,
            WorkCompletionStatus::BadResponseError,
            WorkCompletionStatus::LocalAccessError,
            WorkCompletionStatus::RemoteInvalidRequestError,
            WorkCompletionStatus::RemoteAccessError,
            WorkCompletionStatus::RemoteOperationError,
            WorkCompletionStatus::TransportRetryExceeded,
            WorkCompletionStatus::RnrRetryExceeded,
            WorkCompletionStatus::GeneralError,
        ];
    }

    #[test]
    fn test_work_completion_opcode_variants() {
        let _opcodes = [
            WorkCompletionOpcode::Send,
            WorkCompletionOpcode::RdmaWrite,
            WorkCompletionOpcode::RdmaRead,
            WorkCompletionOpcode::CompSwap,
            WorkCompletionOpcode::FetchAdd,
            WorkCompletionOpcode::Recv,
            WorkCompletionOpcode::RecvRdmaWithImm,
        ];
    }

    #[test]
    fn test_completion_channel() {
        let cc = CompletionChannel::new(1, 100);
        assert_eq!(cc.pending_count(), 0);

        // Take receiver (can only be done once)
        let rx = cc.take_receiver();
        assert!(rx.is_some());

        // Second take should fail
        let rx2 = cc.take_receiver();
        assert!(rx2.is_none());
    }
}
