//! GPU acceleration primitives for warp
//!
//! This crate provides foundational GPU primitives for CUDA-accelerated operations:
//! - Device context management and capability queries
//! - Pinned memory pool for zero-copy DMA transfers
//! - Traits for GPU-accelerated hashing, encryption, and compression
//! - Buffer abstractions with lifetime safety
//! - CPU fallback support for portability
//!
//! # Architecture
//!
//! The design follows these principles:
//! 1. **Zero-copy transfers**: Pinned memory eliminates staging buffers
//! 2. **Memory reuse**: Pooled buffers minimize allocation overhead
//! 3. **Type safety**: Rust ownership prevents use-after-free
//! 4. **CPU fallback**: Traits enable graceful degradation
//! 5. **No mocks**: Real CUDA queries via cudarc
//!
//! # Example
//!
//! ```no_run
//! use warp_gpu::{GpuContext, PinnedMemoryPool};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = GpuContext::new()?;
//! let pool = PinnedMemoryPool::with_defaults(ctx.context().clone());
//!
//! // Acquire pinned buffer for zero-copy transfer
//! let mut buffer = pool.acquire(1024 * 1024)?; // 1MB
//! let data = vec![42u8; 1024 * 1024];
//! buffer.copy_from_slice(&data)?;
//!
//! // Transfer to GPU
//! let device_data = ctx.host_to_device(buffer.as_slice())?;
//!
//! // Return buffer to pool
//! pool.release(buffer);
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod context;
pub mod memory;
pub mod buffer;
pub mod traits;
pub mod blake3;
pub mod chacha20;
pub mod stream;
pub mod pooled;

pub use error::{Error, Result};
pub use context::{GpuContext, DeviceCapabilities};
pub use memory::{PinnedBuffer, PinnedMemoryPool, PoolConfig, PoolStatistics};
pub use buffer::{GpuBuffer, HostBuffer};
pub use traits::{GpuOp, GpuHasher, GpuCipher, GpuCompressor};
pub use blake3::{Blake3Hasher, Blake3Batch};
pub use chacha20::{ChaCha20Poly1305, EncryptionBatch};
pub use stream::{StreamConfig, StreamManager, StreamGuard, PipelineExecutor};
pub use pooled::{PooledHasher, PooledCipher, create_shared_pool};

// Re-export cudarc types for kernel operations
pub use cudarc::driver::{CudaFunction, CudaModule, CudaSlice, LaunchConfig};
pub use cudarc::nvrtc::Ptx;
