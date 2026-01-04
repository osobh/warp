//! DPU Backend Abstraction
//!
//! Provides a common interface for DPU backends (BlueField, Pensando, Intel IPU).
//! Mirrors warp-gpu's GpuBackend design but adapted for DPU characteristics.

use crate::error::Result;
use std::fmt;
use std::sync::Arc;

/// DPU type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DpuType {
    /// NVIDIA BlueField DPU (DOCA SDK)
    BlueField = 0,
    /// AMD Pensando DPU (future)
    Pensando = 1,
    /// Intel IPU (future)
    IntelIpu = 2,
    /// CPU fallback (no DPU hardware)
    Cpu = 255,
}

impl DpuType {
    /// Check if this is a real DPU (not CPU fallback)
    #[must_use]
    pub fn is_hardware(&self) -> bool {
        !matches!(self, DpuType::Cpu)
    }

    /// Get the vendor name
    #[must_use]
    pub fn vendor(&self) -> &'static str {
        match self {
            DpuType::BlueField => "NVIDIA",
            DpuType::Pensando => "AMD",
            DpuType::IntelIpu => "Intel",
            DpuType::Cpu => "CPU",
        }
    }
}

impl fmt::Display for DpuType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DpuType::BlueField => write!(f, "BlueField"),
            DpuType::Pensando => write!(f, "Pensando"),
            DpuType::IntelIpu => write!(f, "Intel IPU"),
            DpuType::Cpu => write!(f, "CPU"),
        }
    }
}

/// DPU device capabilities and information
#[derive(Debug, Clone)]
pub struct DpuInfo {
    /// Device name (e.g., "BlueField-3")
    pub name: String,
    /// DPU type/vendor
    pub dpu_type: DpuType,
    /// Device index on the system
    pub device_index: usize,
    /// DPU generation (e.g., 3 for BF3)
    pub generation: u32,
    /// Number of ARM cores on the DPU
    pub arm_cores: u32,
    /// On-chip SRAM in bytes
    pub sram_bytes: usize,
    /// Host memory accessible to DPU in bytes
    pub host_memory_bytes: usize,
    /// Network bandwidth in Gbps
    pub network_bandwidth_gbps: u32,
    /// Crypto acceleration available
    pub has_crypto_accel: bool,
    /// Compression acceleration available
    pub has_compress_accel: bool,
    /// Erasure coding acceleration available
    pub has_ec_accel: bool,
    /// RDMA capable
    pub has_rdma: bool,
    /// Supports inline processing on network path
    pub has_inline_processing: bool,
}

impl DpuInfo {
    /// Create a CPU fallback info
    #[must_use]
    pub fn cpu_fallback() -> Self {
        Self {
            name: "CPU Fallback".to_string(),
            dpu_type: DpuType::Cpu,
            device_index: 0,
            generation: 0,
            arm_cores: 0,
            sram_bytes: 0,
            host_memory_bytes: 0,
            network_bandwidth_gbps: 0,
            has_crypto_accel: false,
            has_compress_accel: false,
            has_ec_accel: false,
            has_rdma: false,
            has_inline_processing: false,
        }
    }

    /// Create a BlueField-3 info (for testing/stub)
    #[must_use]
    pub fn bluefield3_stub() -> Self {
        Self {
            name: "BlueField-3 (Stub)".to_string(),
            dpu_type: DpuType::BlueField,
            device_index: 0,
            generation: 3,
            arm_cores: 16,
            sram_bytes: 64 * 1024 * 1024,               // 64MB
            host_memory_bytes: 32 * 1024 * 1024 * 1024, // 32GB
            network_bandwidth_gbps: 400,
            has_crypto_accel: true,
            has_compress_accel: true,
            has_ec_accel: true,
            has_rdma: true,
            has_inline_processing: true,
        }
    }
}

impl Default for DpuInfo {
    fn default() -> Self {
        Self::cpu_fallback()
    }
}

/// DPU buffer handle for RDMA-registered memory
///
/// Buffers are registered with the DPU for zero-copy access.
/// They can be used for inline processing on the network path.
pub trait DpuBuffer: Send + Sync + fmt::Debug {
    /// Get buffer size in bytes
    fn size(&self) -> usize;

    /// Get a slice of the buffer contents
    fn as_slice(&self) -> &[u8];

    /// Get a mutable slice of the buffer contents
    fn as_mut_slice(&mut self) -> &mut [u8];

    /// Get RDMA lkey for local access (if applicable)
    fn rdma_lkey(&self) -> Option<u32> {
        None
    }

    /// Get RDMA rkey for remote access (if applicable)
    fn rdma_rkey(&self) -> Option<u32> {
        None
    }

    /// Get physical/DMA address (if applicable)
    fn physical_addr(&self) -> Option<u64> {
        None
    }

    /// Check if buffer is registered for DPU access
    fn is_registered(&self) -> bool {
        false
    }
}

/// Work queue handle for async DPU operations
///
/// DPU operations are submitted to work queues and complete asynchronously.
pub trait DpuWorkQueue: Send + Sync {
    /// Submit work and get a completion token
    fn submit(&self) -> Result<u64>;

    /// Poll for completion (non-blocking)
    fn poll(&self, token: u64) -> Result<bool>;

    /// Wait for completion (blocking)
    fn wait(&self, token: u64) -> Result<()>;

    /// Get the number of pending operations
    fn pending_count(&self) -> usize;

    /// Get the queue depth (max pending operations)
    fn queue_depth(&self) -> usize;
}

/// Abstract DPU backend trait
///
/// Provides device management, memory allocation, and work queue creation.
/// Implementations exist for BlueField (DOCA), with stubs for testing.
pub trait DpuBackend: Send + Sync {
    /// Buffer type for this backend
    type Buffer: DpuBuffer;
    /// Work queue type for async operations
    type WorkQueue: DpuWorkQueue;

    // =========================================================================
    // Device Information
    // =========================================================================

    /// Get device name
    fn device_name(&self) -> &str;

    /// Get DPU type
    fn dpu_type(&self) -> DpuType;

    /// Get detailed device information
    fn device_info(&self) -> &DpuInfo;

    /// Check if DPU has crypto acceleration
    fn has_crypto(&self) -> bool {
        self.device_info().has_crypto_accel
    }

    /// Check if DPU has compression acceleration
    fn has_compression(&self) -> bool {
        self.device_info().has_compress_accel
    }

    /// Check if DPU has erasure coding acceleration
    fn has_erasure_coding(&self) -> bool {
        self.device_info().has_ec_accel
    }

    /// Check if DPU supports RDMA
    fn has_rdma(&self) -> bool {
        self.device_info().has_rdma
    }

    /// Check if DPU supports inline processing
    fn has_inline_processing(&self) -> bool {
        self.device_info().has_inline_processing
    }

    // =========================================================================
    // Memory Operations
    // =========================================================================

    /// Allocate a DPU-accessible buffer
    ///
    /// The buffer is registered for RDMA/DMA access if supported.
    fn allocate(&self, bytes: usize) -> Result<Self::Buffer>;

    /// Allocate and initialize a buffer with data
    fn allocate_with_data(&self, data: &[u8]) -> Result<Self::Buffer> {
        let mut buffer = self.allocate(data.len())?;
        buffer.as_mut_slice().copy_from_slice(data);
        Ok(buffer)
    }

    /// Free a buffer (usually handled by Drop, but explicit free available)
    fn free(&self, buffer: Self::Buffer) -> Result<()>;

    // =========================================================================
    // Work Queue Operations
    // =========================================================================

    /// Create a work queue for async operations
    fn create_work_queue(&self) -> Result<Self::WorkQueue>;

    /// Synchronize all pending operations
    fn synchronize(&self) -> Result<()>;
}

/// Check if any DPU backend is available on the system
#[must_use]
pub fn is_dpu_available() -> bool {
    #[cfg(feature = "bluefield")]
    {
        if crate::backends::bluefield::BlueFieldBackend::is_available() {
            return true;
        }
    }
    false
}

/// Get the best available DPU backend
///
/// Returns BlueField if available, otherwise returns a stub or CPU fallback.
pub fn get_best_backend() -> Result<
    Arc<
        dyn DpuBackend<
                Buffer = crate::backends::stub::StubBuffer,
                WorkQueue = crate::backends::stub::StubWorkQueue,
            >,
    >,
> {
    // For now, return stub backend
    // When BlueField is implemented, check for it first
    Ok(Arc::new(crate::backends::stub::StubBackend::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpu_type_display() {
        assert_eq!(DpuType::BlueField.to_string(), "BlueField");
        assert_eq!(DpuType::Cpu.to_string(), "CPU");
    }

    #[test]
    fn test_dpu_type_is_hardware() {
        assert!(DpuType::BlueField.is_hardware());
        assert!(DpuType::Pensando.is_hardware());
        assert!(!DpuType::Cpu.is_hardware());
    }

    #[test]
    fn test_dpu_info_defaults() {
        let info = DpuInfo::cpu_fallback();
        assert_eq!(info.dpu_type, DpuType::Cpu);
        assert!(!info.has_crypto_accel);
    }

    #[test]
    fn test_bluefield3_stub_info() {
        let info = DpuInfo::bluefield3_stub();
        assert_eq!(info.dpu_type, DpuType::BlueField);
        assert_eq!(info.generation, 3);
        assert_eq!(info.network_bandwidth_gbps, 400);
        assert!(info.has_crypto_accel);
        assert!(info.has_compress_accel);
        assert!(info.has_ec_accel);
    }
}
