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

mod lz4;
mod zstd;
mod batch;

// Re-export warp-gpu types for backward compatibility
pub use warp_gpu::{GpuContext, PinnedMemoryPool, GpuCompressor as GpuCompressorTrait};

// Local implementations
pub use lz4::GpuLz4Compressor;
pub use zstd::GpuZstdCompressor;
pub use batch::{BatchCompressor, CompressionAlgorithm};
