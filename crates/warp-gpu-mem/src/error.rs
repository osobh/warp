//! Error types for GPU memory extension

use thiserror::Error;

/// GPU memory extension errors
#[derive(Debug, Error)]
pub enum GpuMemError {
    /// Out of GPU memory
    #[error("out of GPU memory: requested {requested} bytes, available {available} bytes")]
    OutOfMemory {
        /// Bytes requested
        requested: u64,
        /// Bytes available
        available: u64,
    },

    /// Tensor not found
    #[error("tensor not found: {0}")]
    TensorNotFound(String),

    /// Tensor already exists
    #[error("tensor already exists: {0}")]
    TensorExists(String),

    /// Invalid tensor shape
    #[error("invalid tensor shape: {0}")]
    InvalidShape(String),

    /// Spill failed
    #[error("failed to spill tensor to storage: {0}")]
    SpillFailed(String),

    /// Page-in failed
    #[error("failed to page in tensor: {0}")]
    PageInFailed(String),

    /// Storage error
    #[error("storage error: {0}")]
    Storage(#[from] warp_store::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Prefetch error
    #[error("prefetch error: {0}")]
    Prefetch(String),

    /// Cache error
    #[error("cache error: {0}")]
    Cache(String),

    /// Configuration error
    #[error("configuration error: {0}")]
    Config(String),

    /// Tensor is locked
    #[error("tensor is locked by another operation")]
    TensorLocked,

    /// Invalid dtype
    #[error("invalid data type: {0}")]
    InvalidDtype(String),

    /// Alignment error
    #[error("memory alignment error: {0}")]
    Alignment(String),
}

/// Result type for GPU memory operations
pub type GpuMemResult<T> = Result<T, GpuMemError>;
