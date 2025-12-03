//! warp-compress: Compression algorithms
//!
//! Supports both CPU and GPU compression:
//! - CPU: zstd, lz4
//! - GPU: nvCOMP (zstd, lz4, deflate) via cudarc
//!
//! Also supports dictionary compression for improved ratios on similar data.

#![warn(missing_docs)]

pub mod cpu;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod adaptive;
pub mod dictionary;

pub use cpu::{ZstdCompressor, Lz4Compressor};
pub use dictionary::{Dictionary, DictZstdCompressor};

#[cfg(feature = "gpu")]
pub use gpu::{GpuContext, GpuLz4Compressor, GpuZstdCompressor, BatchCompressor, CompressionAlgorithm};

/// Compression error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Compression failed
    #[error("Compression error: {0}")]
    Compression(String),
    
    /// Decompression failed
    #[error("Decompression error: {0}")]
    Decompression(String),
    
    /// Invalid compression level
    #[error("Invalid compression level: {0}")]
    InvalidLevel(i32),
    
    /// GPU error
    #[error("GPU error: {0}")]
    Gpu(String),
}

/// Result type for compression operations
pub type Result<T> = std::result::Result<T, Error>;

/// Compression algorithm trait
pub trait Compressor: Send + Sync {
    /// Compress data
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>>;
    
    /// Decompress data
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>>;
    
    /// Algorithm name
    fn name(&self) -> &'static str;
}
