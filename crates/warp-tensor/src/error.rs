//! Error types for tensor storage

use std::io;
use thiserror::Error;

/// Tensor storage errors
#[derive(Debug, Error)]
pub enum TensorError {
    /// Tensor not found
    #[error("tensor not found: {0}")]
    TensorNotFound(String),

    /// Model not found
    #[error("model not found: {0}")]
    ModelNotFound(String),

    /// Checkpoint not found
    #[error("checkpoint not found: {0}")]
    CheckpointNotFound(String),

    /// Shard not found
    #[error("shard not found: {shard_id} for tensor {tensor_name}")]
    ShardNotFound {
        /// Tensor name
        tensor_name: String,
        /// Shard ID
        shard_id: u32,
    },

    /// Invalid tensor format
    #[error("invalid tensor format: {0}")]
    InvalidFormat(String),

    /// Invalid tensor shape
    #[error("invalid tensor shape: expected {expected:?}, got {actual:?}")]
    InvalidShape {
        /// Expected shape
        expected: Vec<usize>,
        /// Actual shape
        actual: Vec<usize>,
    },

    /// Invalid dtype
    #[error("invalid dtype: expected {expected}, got {actual}")]
    InvalidDtype {
        /// Expected dtype
        expected: String,
        /// Actual dtype
        actual: String,
    },

    /// Tensor data corrupted
    #[error("tensor data corrupted: {0}")]
    DataCorrupted(String),

    /// Checksum mismatch
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Expected checksum
        expected: String,
        /// Actual checksum
        actual: String,
    },

    /// Compression error
    #[error("compression error: {0}")]
    CompressionError(String),

    /// Decompression error
    #[error("decompression error: {0}")]
    DecompressionError(String),

    /// Storage backend error
    #[error("storage error: {0}")]
    Storage(#[from] warp_store::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Serialization error
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Version conflict
    #[error("version conflict: {0}")]
    VersionConflict(String),

    /// Too many shards
    #[error("too many shards: {count} exceeds maximum {max}")]
    TooManyShards {
        /// Shard count
        count: u32,
        /// Maximum allowed
        max: u32,
    },

    /// Tensor too large
    #[error("tensor too large: {size} bytes exceeds maximum {max} bytes")]
    TensorTooLarge {
        /// Tensor size
        size: u64,
        /// Maximum allowed
        max: u64,
    },

    /// Unsupported format
    #[error("unsupported tensor format: {0}")]
    UnsupportedFormat(String),

    /// Configuration error
    #[error("configuration error: {0}")]
    ConfigError(String),
}

/// Result type for tensor operations
pub type TensorResult<T> = Result<T, TensorError>;
