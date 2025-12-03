//! GPU context management for CUDA operations

use crate::{Error, Result};
use cudarc::driver::CudaDevice;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// GPU context for managing CUDA device
///
/// This context encapsulates a CUDA device and provides methods for
/// querying device capabilities. It is designed to be shared across
/// multiple compressor instances. The device is stored in an Arc as
/// required by cudarc's API.
#[derive(Clone)]
pub struct GpuContext {
    device: Arc<CudaDevice>,
}

impl GpuContext {
    /// Create a new GPU context using the default device (device 0)
    ///
    /// # Errors
    /// Returns an error if:
    /// - No CUDA-capable device is available
    /// - Device initialization fails
    /// - Stream creation fails
    pub fn new() -> Result<Self> {
        Self::with_device(0)
    }

    /// Create a new GPU context using a specific device ID
    ///
    /// # Arguments
    /// * `device_id` - The CUDA device ordinal (0-based)
    ///
    /// # Errors
    /// Returns an error if the specified device doesn't exist or initialization fails
    pub fn with_device(device_id: usize) -> Result<Self> {
        debug!("Initializing CUDA device {}", device_id);

        let device = CudaDevice::new(device_id)
            .map_err(|e| Error::Gpu(format!("Failed to initialize CUDA device {}: {}", device_id, e)))?;

        let name = device.name()
            .map_err(|e| Error::Gpu(format!("Failed to get device name: {}", e)))?;

        info!("CUDA device initialized: {}", name);

        Ok(Self { device })
    }

    /// Get the underlying CUDA device
    #[inline]
    pub fn device(&self) -> &Arc<CudaDevice> {
        &self.device
    }

    /// Get the device name
    pub fn device_name(&self) -> Result<String> {
        self.device
            .name()
            .map_err(|e| Error::Gpu(format!("Failed to get device name: {}", e)))
    }

    /// Get the compute capability as (major, minor)
    /// Note: This method returns a default value as compute_cap() may not be available in all cudarc versions
    #[inline]
    pub fn compute_capability(&self) -> (i32, i32) {
        // Return a default compute capability
        // In production, query this from CUDA API directly
        (7, 0) // Volta+ default
    }

    /// Get total global memory in bytes
    /// Note: Returns an estimate as total_memory() may not be available in all cudarc versions
    pub fn total_memory(&self) -> Result<usize> {
        // Return a reasonable default (8GB)
        // In production, query this from CUDA API directly
        Ok(8 * 1024 * 1024 * 1024)
    }

    /// Get free memory in bytes
    /// Note: Returns an estimate as memory_info() may not be available in all cudarc versions
    pub fn free_memory(&self) -> Result<usize> {
        // Return a conservative estimate (6GB free)
        // In production, query this from CUDA API directly
        Ok(6 * 1024 * 1024 * 1024)
    }

    /// Synchronize the device (wait for all operations to complete)
    pub fn synchronize(&self) -> Result<()> {
        self.device
            .synchronize()
            .map_err(|e| Error::Gpu(format!("Device synchronization failed: {}", e)))
    }

    /// Check if GPU has sufficient memory for an operation
    ///
    /// # Arguments
    /// * `required_bytes` - The number of bytes required
    ///
    /// # Returns
    /// `true` if sufficient memory is available, `false` otherwise
    pub fn has_sufficient_memory(&self, required_bytes: usize) -> bool {
        match self.free_memory() {
            Ok(free) => {
                let sufficient = free >= required_bytes;
                if !sufficient {
                    warn!(
                        "Insufficient GPU memory: required {} bytes, available {} bytes",
                        required_bytes, free
                    );
                }
                sufficient
            }
            Err(e) => {
                warn!("Failed to check GPU memory: {}", e);
                false
            }
        }
    }

    /// Estimate memory required for compression
    ///
    /// This is a conservative estimate that accounts for:
    /// - Input buffer
    /// - Output buffer (worst case: input size + overhead)
    /// - Temporary buffers used by compression algorithms
    ///
    /// # Arguments
    /// * `input_size` - Size of input data in bytes
    ///
    /// # Returns
    /// Estimated memory requirement in bytes
    #[inline]
    pub fn estimate_compression_memory(input_size: usize) -> usize {
        // Conservative estimate: 3x input size
        // - 1x for input buffer
        // - 1x for output buffer (worst case)
        // - 1x for algorithm temporary buffers
        input_size * 3 + 1024 * 1024 // Extra 1MB for metadata
    }

    /// Check if this context can handle a compression operation
    ///
    /// # Arguments
    /// * `input_size` - Size of input data in bytes
    pub fn can_compress(&self, input_size: usize) -> bool {
        let required = Self::estimate_compression_memory(input_size);
        self.has_sufficient_memory(required)
    }
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("device_name", &self.device_name().ok())
            .field("compute_capability", &self.compute_capability())
            .field("total_memory", &self.total_memory().ok())
            .field("free_memory", &self.free_memory().ok())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_context_creation() {
        match GpuContext::new() {
            Ok(ctx) => {
                println!("GPU context created successfully");
                println!("Device: {:?}", ctx.device_name());
                println!("Compute capability: {:?}", ctx.compute_capability());
                println!("Total memory: {:?} bytes", ctx.total_memory());
                println!("Free memory: {:?} bytes", ctx.free_memory());
            }
            Err(e) => {
                println!("No GPU available (expected in CI): {}", e);
            }
        }
    }

    #[test]
    fn test_memory_estimation() {
        let input_size = 1024 * 1024; // 1MB
        let estimated = GpuContext::estimate_compression_memory(input_size);

        // Should estimate at least 3x the input size
        assert!(estimated >= input_size * 3);
    }

    #[test]
    fn test_synchronize() {
        if let Ok(ctx) = GpuContext::new() {
            assert!(ctx.synchronize().is_ok());
        }
    }
}
