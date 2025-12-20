//! Error types for erasure coding operations

use thiserror::Error;

/// Result type for erasure coding operations
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during erasure coding
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid configuration parameters
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Data size is not compatible with shard count
    #[error("Invalid data size: {0}")]
    InvalidDataSize(String),

    /// Too many shards are missing for recovery
    #[error("Too many missing shards: need {needed}, have {available}")]
    TooManyMissing {
        /// Number of shards needed for recovery
        needed: usize,
        /// Number of shards available
        available: usize,
    },

    /// Shard size mismatch during decoding
    #[error("Shard size mismatch: expected {expected}, got {actual}")]
    ShardSizeMismatch {
        /// Expected shard size
        expected: usize,
        /// Actual shard size
        actual: usize,
    },

    /// Wrong number of shards provided
    #[error("Wrong shard count: expected {expected}, got {actual}")]
    WrongShardCount {
        /// Expected number of shards
        expected: usize,
        /// Actual number of shards
        actual: usize,
    },

    /// Internal encoding/decoding error
    #[error("Encoding error: {0}")]
    EncodingError(String),
}
