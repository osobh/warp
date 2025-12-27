//! Stub DPU Backend for Testing
//!
//! Provides a mock DPU backend that uses CPU implementations.
//! Useful for testing and development without DPU hardware.

use crate::backend::{DpuBackend, DpuBuffer, DpuInfo, DpuType, DpuWorkQueue};
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicU64, Ordering};

/// Stub buffer implementation using heap-allocated memory
#[derive(Debug)]
pub struct StubBuffer {
    data: Vec<u8>,
    /// Simulated physical address
    phys_addr: u64,
}

impl StubBuffer {
    /// Create a new stub buffer with the given size
    #[must_use]
    pub fn new(size: usize) -> Self {
        static NEXT_ADDR: AtomicU64 = AtomicU64::new(0x1000_0000);
        Self {
            data: vec![0u8; size],
            phys_addr: NEXT_ADDR.fetch_add(size as u64, Ordering::Relaxed),
        }
    }

    /// Create a stub buffer from existing data
    #[must_use]
    pub fn from_data(data: Vec<u8>) -> Self {
        static NEXT_ADDR: AtomicU64 = AtomicU64::new(0x1000_0000);
        let phys_addr = NEXT_ADDR.fetch_add(data.len() as u64, Ordering::Relaxed);
        Self { data, phys_addr }
    }
}

impl DpuBuffer for StubBuffer {
    fn size(&self) -> usize {
        self.data.len()
    }

    fn as_slice(&self) -> &[u8] {
        &self.data
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    fn physical_addr(&self) -> Option<u64> {
        Some(self.phys_addr)
    }

    fn is_registered(&self) -> bool {
        // Stub is always "registered"
        true
    }
}

/// Stub work queue that completes operations immediately
#[derive(Debug)]
pub struct StubWorkQueue {
    next_token: AtomicU64,
    queue_depth: usize,
}

impl StubWorkQueue {
    /// Create a new stub work queue
    #[must_use]
    pub fn new(queue_depth: usize) -> Self {
        Self {
            next_token: AtomicU64::new(1),
            queue_depth,
        }
    }
}

impl DpuWorkQueue for StubWorkQueue {
    fn submit(&self) -> Result<u64> {
        Ok(self.next_token.fetch_add(1, Ordering::Relaxed))
    }

    fn poll(&self, _token: u64) -> Result<bool> {
        // Stub: operations complete immediately
        Ok(true)
    }

    fn wait(&self, _token: u64) -> Result<()> {
        // Stub: nothing to wait for
        Ok(())
    }

    fn pending_count(&self) -> usize {
        0
    }

    fn queue_depth(&self) -> usize {
        self.queue_depth
    }
}

/// Stub DPU backend for testing
///
/// Simulates a BlueField-3 DPU but uses CPU implementations.
#[derive(Debug)]
pub struct StubBackend {
    info: DpuInfo,
}

impl StubBackend {
    /// Create a new stub backend
    #[must_use]
    pub fn new() -> Self {
        Self {
            info: DpuInfo::bluefield3_stub(),
        }
    }

    /// Create a stub backend with custom info
    #[must_use]
    pub fn with_info(info: DpuInfo) -> Self {
        Self { info }
    }

    /// Create a CPU-only stub (no DPU capabilities)
    #[must_use]
    pub fn cpu_only() -> Self {
        Self {
            info: DpuInfo::cpu_fallback(),
        }
    }

    /// Check if stub backend is available (always true)
    #[must_use]
    pub fn is_available() -> bool {
        true
    }
}

impl Default for StubBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DpuBackend for StubBackend {
    type Buffer = StubBuffer;
    type WorkQueue = StubWorkQueue;

    fn device_name(&self) -> &str {
        &self.info.name
    }

    fn dpu_type(&self) -> DpuType {
        self.info.dpu_type
    }

    fn device_info(&self) -> &DpuInfo {
        &self.info
    }

    fn allocate(&self, bytes: usize) -> Result<Self::Buffer> {
        if bytes == 0 {
            return Err(Error::InvalidInput("Cannot allocate 0 bytes".into()));
        }
        Ok(StubBuffer::new(bytes))
    }

    fn free(&self, _buffer: Self::Buffer) -> Result<()> {
        // Buffer is dropped, memory freed
        Ok(())
    }

    fn create_work_queue(&self) -> Result<Self::WorkQueue> {
        Ok(StubWorkQueue::new(256))
    }

    fn synchronize(&self) -> Result<()> {
        // Stub: nothing to synchronize
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_backend_creation() {
        let backend = StubBackend::new();
        assert_eq!(backend.dpu_type(), DpuType::BlueField);
        assert!(backend.has_crypto());
        assert!(backend.has_compression());
    }

    #[test]
    fn test_stub_buffer_allocation() {
        let backend = StubBackend::new();
        let buffer = backend.allocate(1024).unwrap();
        assert_eq!(buffer.size(), 1024);
        assert!(buffer.is_registered());
        assert!(buffer.physical_addr().is_some());
    }

    #[test]
    fn test_stub_buffer_zero_allocation() {
        let backend = StubBackend::new();
        let result = backend.allocate(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_stub_buffer_data() {
        let backend = StubBackend::new();
        let mut buffer = backend.allocate(10).unwrap();
        buffer.as_mut_slice().copy_from_slice(b"hello test");
        assert_eq!(buffer.as_slice(), b"hello test");
    }

    #[test]
    fn test_stub_work_queue() {
        let backend = StubBackend::new();
        let wq = backend.create_work_queue().unwrap();

        let token = wq.submit().unwrap();
        assert!(wq.poll(token).unwrap());
        wq.wait(token).unwrap();
        assert_eq!(wq.pending_count(), 0);
    }

    #[test]
    fn test_cpu_only_stub() {
        let backend = StubBackend::cpu_only();
        assert_eq!(backend.dpu_type(), DpuType::Cpu);
        assert!(!backend.has_crypto());
        assert!(!backend.has_compression());
    }
}
