//! GPU compression using CUDA
//!
//! This module provides GPU-accelerated compression implementations
//! using NVIDIA CUDA through the cudarc library. It includes:
//! - LZ4 and Zstd compression algorithms
//! - Batch processing for multiple chunks
//! - Automatic device management and fallback handling

mod context;
mod lz4;
mod zstd;
mod batch;

pub use context::GpuContext;
pub use lz4::GpuLz4Compressor;
pub use zstd::GpuZstdCompressor;
pub use batch::{BatchCompressor, CompressionAlgorithm};
