//! Pooled buffer management for streaming pipeline
//!
//! This module provides integration between the streaming pipeline and
//! GPU pinned memory pools. Using pooled pinned buffers enables:
//!
//! - Zero-copy DMA transfers to GPU
//! - Buffer reuse to reduce allocation overhead
//! - Efficient memory management across pipeline stages
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────┐    ┌───────────────────┐    ┌────────────────┐
//! │   Input Stage  │───▶│   Process Stage   │───▶│  Output Stage  │
//! │  (Read + Pool) │    │  (GPU Encrypt)    │    │    (Write)     │
//! └────────────────┘    └───────────────────┘    └────────────────┘
//!         │                      │
//!         ▼                      ▼
//!    ┌─────────────────────────────────────┐
//!    │         Pinned Memory Pool          │
//!    │   (Reusable page-locked buffers)    │
//!    └─────────────────────────────────────┘
//! ```

use std::sync::Arc;
use tracing::{debug, trace, warn};

use crate::{Result, StreamConfig, StreamError};

/// A buffer acquired from the pool
///
/// When dropped, the buffer is automatically returned to the pool.
pub struct PooledBuffer {
    /// The actual pinned buffer from warp-gpu
    inner: Option<warp_gpu::PinnedBuffer>,
    /// Reference to the pool for returning the buffer
    pool: Option<Arc<warp_gpu::PinnedMemoryPool>>,
    /// Data length (may be less than capacity)
    len: usize,
}

impl PooledBuffer {
    /// Create a new pooled buffer from a pinned buffer
    pub fn new(inner: warp_gpu::PinnedBuffer, pool: Arc<warp_gpu::PinnedMemoryPool>) -> Self {
        let len = inner.as_slice().len();
        Self {
            inner: Some(inner),
            pool: Some(pool),
            len,
        }
    }

    /// Create a standalone buffer without pooling (CPU fallback)
    pub fn standalone(size: usize) -> Result<Self> {
        let inner = warp_gpu::PinnedBuffer::new(size)
            .map_err(StreamError::GpuError)?;
        Ok(Self {
            inner: Some(inner),
            pool: None,
            len: size,
        })
    }

    /// Get the data as a slice
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.inner.as_ref()
            .map(|b| &b.as_slice()[..self.len])
            .unwrap_or(&[])
    }

    /// Get mutable access to the buffer
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.len;
        self.inner.as_mut()
            .map(|b| &mut b.as_mut_slice()[..len])
            .unwrap_or(&mut [])
    }

    /// Get the full capacity of the buffer
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.as_ref().map(|b| b.capacity()).unwrap_or(0)
    }

    /// Get the current data length
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if buffer is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Set the length (used after writing data)
    ///
    /// # Panics
    /// Panics if len > capacity
    pub fn set_len(&mut self, len: usize) {
        assert!(len <= self.capacity(), "Length exceeds capacity");
        self.len = len;
    }

    /// Copy data into the buffer
    pub fn copy_from_slice(&mut self, data: &[u8]) -> Result<()> {
        if data.len() > self.capacity() {
            return Err(StreamError::BufferOverflow);
        }
        if let Some(inner) = &mut self.inner {
            inner.copy_from_slice(data)
                .map_err(StreamError::GpuError)?;
            self.len = data.len();
        }
        Ok(())
    }

    /// Take ownership of the inner buffer (detach from pool)
    pub fn into_inner(mut self) -> Option<warp_gpu::PinnedBuffer> {
        self.pool = None;
        self.inner.take()
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let (Some(inner), Some(pool)) = (self.inner.take(), self.pool.take()) {
            trace!("Returning buffer to pool: {} bytes", inner.capacity());
            pool.release(inner);
        }
    }
}

/// Manager for pooled buffers in the streaming pipeline
pub struct BufferPoolManager {
    /// Pinned memory pool from warp-gpu
    pool: Option<Arc<warp_gpu::PinnedMemoryPool>>,
    /// Default buffer size
    buffer_size: usize,
    /// Whether to use pooled buffers
    use_pooled: bool,
}

impl BufferPoolManager {
    /// Create a new buffer pool manager
    ///
    /// # Arguments
    /// * `config` - Stream configuration
    ///
    /// Attempts to create a pinned memory pool if GPU is enabled.
    /// Falls back to standalone buffers if GPU is unavailable.
    pub fn new(config: &StreamConfig) -> Result<Self> {
        let buffer_size = config.chunk_size;

        if config.use_gpu {
            match Self::try_init_pool(buffer_size) {
                Ok(pool) => {
                    debug!("Buffer pool manager using pinned memory pool");
                    return Ok(Self {
                        pool: Some(Arc::new(pool)),
                        buffer_size,
                        use_pooled: true,
                    });
                }
                Err(e) => {
                    warn!("Failed to create pinned memory pool: {}", e);
                }
            }
        }

        debug!("Buffer pool manager using standalone buffers");
        Ok(Self {
            pool: None,
            buffer_size,
            use_pooled: false,
        })
    }

    /// Try to initialize the pinned memory pool
    fn try_init_pool(buffer_size: usize) -> Result<warp_gpu::PinnedMemoryPool> {
        let ctx = warp_gpu::GpuContext::new()
            .map_err(StreamError::GpuError)?;

        // Configure pool with size classes based on buffer size
        let size_classes = vec![
            buffer_size,            // Exact chunk size
            buffer_size * 4,        // 4x for batch processing
            buffer_size * 16,       // 16x for large batches
        ];

        let config = warp_gpu::PoolConfig {
            max_total_memory: 512 * 1024 * 1024, // 512MB for streaming
            size_classes,
            max_buffers_per_class: 8,
            track_statistics: true,
        };

        Ok(warp_gpu::PinnedMemoryPool::new(ctx.context().clone(), config))
    }

    /// Acquire a buffer from the pool
    ///
    /// # Arguments
    /// * `size` - Required buffer size (optional, uses default if None)
    pub fn acquire(&self, size: Option<usize>) -> Result<PooledBuffer> {
        let size = size.unwrap_or(self.buffer_size);

        if let Some(pool) = &self.pool {
            let inner = pool.acquire(size)
                .map_err(StreamError::GpuError)?;
            Ok(PooledBuffer::new(inner, Arc::clone(pool)))
        } else {
            PooledBuffer::standalone(size)
        }
    }

    /// Check if using pooled pinned memory
    pub fn is_pooled(&self) -> bool {
        self.use_pooled
    }

    /// Get pool statistics (if available)
    pub fn statistics(&self) -> Option<warp_gpu::PoolStatistics> {
        self.pool.as_ref().map(|p| p.statistics())
    }
}

/// Shared buffer pool manager for use across pipeline stages
pub type SharedPoolManager = Arc<BufferPoolManager>;

/// Create a shared buffer pool manager
pub fn create_pool_manager(config: &StreamConfig) -> Result<SharedPoolManager> {
    let manager = BufferPoolManager::new(config)?;
    Ok(Arc::new(manager))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config(use_gpu: bool) -> StreamConfig {
        StreamConfig {
            use_gpu,
            chunk_size: 64 * 1024,
            ..StreamConfig::default()
        }
    }

    #[test]
    fn test_standalone_buffer() {
        let buffer = PooledBuffer::standalone(1024).unwrap();
        assert_eq!(buffer.capacity(), 1024);
        assert_eq!(buffer.len(), 1024);
    }

    #[test]
    fn test_buffer_copy() {
        let mut buffer = PooledBuffer::standalone(1024).unwrap();
        let data = vec![42u8; 512];

        buffer.copy_from_slice(&data).unwrap();
        assert_eq!(buffer.len(), 512);
        assert_eq!(&buffer.as_slice()[..512], &data[..]);
    }

    #[test]
    fn test_buffer_set_len() {
        let mut buffer = PooledBuffer::standalone(1024).unwrap();
        buffer.set_len(100);
        assert_eq!(buffer.len(), 100);
    }

    #[test]
    #[should_panic(expected = "Length exceeds capacity")]
    fn test_buffer_set_len_overflow() {
        let mut buffer = PooledBuffer::standalone(1024).unwrap();
        buffer.set_len(2000);
    }

    #[test]
    fn test_pool_manager_cpu_mode() {
        let config = create_test_config(false);
        let manager = BufferPoolManager::new(&config).unwrap();

        assert!(!manager.is_pooled());

        let buffer = manager.acquire(None).unwrap();
        assert_eq!(buffer.capacity(), config.chunk_size);
    }

    #[test]
    fn test_pool_manager_acquire() {
        let config = create_test_config(false);
        let manager = BufferPoolManager::new(&config).unwrap();

        // Acquire multiple buffers
        let buf1 = manager.acquire(None).unwrap();
        let buf2 = manager.acquire(None).unwrap();
        let buf3 = manager.acquire(Some(128)).unwrap();

        assert_eq!(buf1.capacity(), config.chunk_size);
        assert_eq!(buf2.capacity(), config.chunk_size);
        assert_eq!(buf3.capacity(), 128);
    }

    #[test]
    fn test_shared_pool_manager() {
        let config = create_test_config(false);
        let manager = create_pool_manager(&config).unwrap();

        let manager2 = Arc::clone(&manager);

        let buf1 = manager.acquire(None).unwrap();
        let buf2 = manager2.acquire(None).unwrap();

        assert_eq!(buf1.capacity(), buf2.capacity());
    }

    #[test]
    fn test_buffer_into_inner() {
        let buffer = PooledBuffer::standalone(1024).unwrap();
        let inner = buffer.into_inner();
        assert!(inner.is_some());
    }

    #[test]
    fn test_buffer_empty_check() {
        let mut buffer = PooledBuffer::standalone(1024).unwrap();
        assert!(!buffer.is_empty());

        buffer.set_len(0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_pool_manager_gpu_fallback() {
        // This test tries GPU and falls back to CPU if unavailable
        let config = create_test_config(true);
        let manager = BufferPoolManager::new(&config).unwrap();

        // Should work regardless of GPU availability
        let buffer = manager.acquire(None).unwrap();
        assert!(buffer.capacity() > 0);
    }
}
