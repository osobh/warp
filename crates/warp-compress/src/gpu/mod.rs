//! GPU compression using CUDA
//!
//! This module provides GPU-accelerated compression implementations
//! using NVIDIA CUDA through the cudarc library. It includes:
//! - LZ4 and Zstd compression algorithms
//! - Batch processing for multiple chunks
//! - Automatic device management and fallback handling
//!
//! # Architecture
//!
//! This module now uses shared GPU infrastructure from warp-gpu:
//! - `GpuContext`: Enhanced device management and capability queries
//! - `PinnedMemoryPool`: Zero-copy DMA transfers with buffer reuse
//! - `GpuCompressor`: Standard trait interface for GPU compressors
//! - `StreamManager`: Multi-stream support for overlapping operations

mod batch;
mod lz4;
mod zstd;

// Re-export warp-gpu types for backward compatibility
pub use warp_gpu::{GpuCompressor as GpuCompressorTrait, GpuContext, PinnedMemoryPool};

// Local implementations
pub use batch::{BatchCompressor, CompressionAlgorithm};
pub use lz4::GpuLz4Compressor;
pub use zstd::GpuZstdCompressor;
