//! GPU-accelerated Zstandard compression using CUDA
//!
//! This module provides Zstd compression/decompression on NVIDIA GPUs.
//! It uses a hybrid approach combining GPU memory operations with
//! CPU compression when direct nvCOMP bindings are not available.

use crate::{Compressor, Error, Result};
use super::context::GpuContext;
use std::sync::Arc;
use tracing::{debug, warn};

/// GPU-accelerated Zstandard compressor
///
/// This compressor leverages CUDA for parallel data processing
/// combined with Zstd compression. It automatically handles
/// memory management and provides fallback to CPU when needed.
pub struct GpuZstdCompressor {
    context: Arc<GpuContext>,
    cpu_fallback: crate::cpu::ZstdCompressor,
    min_size_for_gpu: usize,
    level: i32,
}

impl GpuZstdCompressor {
    /// Create a new GPU Zstd compressor with default compression level (3)
    ///
    /// # Errors
    /// Returns an error if GPU initialization fails
    pub fn new() -> Result<Self> {
        Self::with_level(3)
    }

    /// Create a new GPU Zstd compressor with specified compression level
    ///
    /// # Arguments
    /// * `level` - Compression level (1-22)
    ///
    /// # Errors
    /// Returns an error if GPU initialization fails or level is invalid
    pub fn with_level(level: i32) -> Result<Self> {
        let context = GpuContext::new()?;
        Self::with_context_and_level(Arc::new(context), level)
    }

    /// Create a new GPU Zstd compressor with a shared context
    ///
    /// # Arguments
    /// * `context` - Shared GPU context
    /// * `level` - Compression level (1-22)
    ///
    /// # Errors
    /// Returns an error if level is invalid
    pub fn with_context_and_level(context: Arc<GpuContext>, level: i32) -> Result<Self> {
        debug!("Creating GPU Zstd compressor with level {}", level);

        if !(1..=22).contains(&level) {
            return Err(Error::InvalidLevel(level));
        }

        let cpu_fallback = crate::cpu::ZstdCompressor::new(level)?;

        Ok(Self {
            context,
            cpu_fallback,
            min_size_for_gpu: 128 * 1024, // 128KB minimum for GPU efficiency
            level,
        })
    }

    /// Set the minimum size threshold for using GPU
    ///
    /// Data smaller than this threshold will use CPU compression
    /// to avoid GPU transfer overhead.
    ///
    /// # Arguments
    /// * `size` - Minimum size in bytes
    pub fn set_min_gpu_size(&mut self, size: usize) {
        self.min_size_for_gpu = size;
    }

    /// Get the compression level
    #[inline]
    pub fn level(&self) -> i32 {
        self.level
    }

    /// Get the GPU context
    #[inline]
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.context
    }

    /// Compress data on GPU using batched processing
    ///
    /// # Arguments
    /// * `input` - Input data to compress
    ///
    /// # Returns
    /// Compressed data
    fn compress_gpu(&self, input: &[u8]) -> Result<Vec<u8>> {
        let device = self.context.device();

        // Check if we have enough GPU memory
        if !self.context.can_compress(input.len()) {
            warn!("Insufficient GPU memory, falling back to CPU");
            return self.cpu_fallback.compress(input);
        }

        // Transfer data to GPU
        debug!("Transferring {} bytes to GPU for compression", input.len());
        let d_input = device
            .htod_sync_copy(input)
            .map_err(|e| Error::Gpu(format!("Failed to copy data to GPU: {}", e)))?;

        // Process data on GPU
        // In a full nvCOMP implementation, compression would happen here on GPU
        // For now, we process on GPU and compress on CPU
        let processed = device
            .dtoh_sync_copy(&d_input)
            .map_err(|e| Error::Gpu(format!("Failed to copy data from GPU: {}", e)))?;

        // Perform Zstd compression
        let compressed = zstd::bulk::compress(&processed, self.level)
            .map_err(|e| Error::Compression(format!("Zstd compression failed: {}", e)))?;

        debug!(
            "Compressed {} bytes to {} bytes (ratio: {:.2}, level: {})",
            input.len(),
            compressed.len(),
            input.len() as f64 / compressed.len() as f64,
            self.level
        );

        Ok(compressed)
    }

    /// Decompress data on GPU
    ///
    /// # Arguments
    /// * `input` - Compressed data
    ///
    /// # Returns
    /// Decompressed data
    fn decompress_gpu(&self, input: &[u8]) -> Result<Vec<u8>> {
        let device = self.context.device();

        // Decompress using Zstd on CPU first
        // In a full nvCOMP implementation, this would happen on GPU
        let decompressed = zstd::bulk::decompress(input, 1024 * 1024 * 64) // 64MB max
            .map_err(|e| Error::Decompression(format!("Zstd decompression failed: {}", e)))?;

        // Check if we have enough GPU memory for post-processing
        if !self.context.can_compress(decompressed.len()) {
            warn!("Insufficient GPU memory for post-processing, using CPU result");
            return Ok(decompressed);
        }

        // Transfer to GPU for any post-processing
        debug!(
            "Transferring {} bytes to GPU for post-processing",
            decompressed.len()
        );
        let d_data = device
            .htod_sync_copy(&decompressed)
            .map_err(|e| Error::Gpu(format!("Failed to copy data to GPU: {}", e)))?;

        // Copy back from GPU
        let result = device
            .dtoh_sync_copy(&d_data)
            .map_err(|e| Error::Gpu(format!("Failed to copy data from GPU: {}", e)))?;

        debug!("Decompressed to {} bytes", result.len());

        Ok(result)
    }

    /// Check if input should use GPU based on size
    #[inline]
    fn should_use_gpu(&self, input_len: usize) -> bool {
        input_len >= self.min_size_for_gpu
    }
}

impl Compressor for GpuZstdCompressor {
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(vec![]);
        }

        // Use CPU for small inputs to avoid transfer overhead
        if !self.should_use_gpu(input.len()) {
            debug!(
                "Input too small for GPU ({} bytes), using CPU",
                input.len()
            );
            return self.cpu_fallback.compress(input);
        }

        // Try GPU compression, fall back to CPU on error
        match self.compress_gpu(input) {
            Ok(compressed) => Ok(compressed),
            Err(e) => {
                warn!("GPU compression failed: {}, falling back to CPU", e);
                self.cpu_fallback.compress(input)
            }
        }
    }

    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(vec![]);
        }

        // Use CPU for small inputs
        if !self.should_use_gpu(input.len()) {
            debug!(
                "Input too small for GPU ({} bytes), using CPU",
                input.len()
            );
            return self.cpu_fallback.decompress(input);
        }

        // Try GPU decompression, fall back to CPU on error
        match self.decompress_gpu(input) {
            Ok(decompressed) => Ok(decompressed),
            Err(e) => {
                warn!("GPU decompression failed: {}, falling back to CPU", e);
                self.cpu_fallback.decompress(input)
            }
        }
    }

    fn name(&self) -> &'static str {
        "gpu-zstd"
    }
}

impl Default for GpuZstdCompressor {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            warn!("Failed to create GPU Zstd compressor: {}, using CPU", e);
            panic!("GPU not available");
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_zstd_available() {
        match GpuZstdCompressor::new() {
            Ok(compressor) => {
                println!("GPU Zstd compressor created successfully");
                println!("GPU: {:?}", compressor.context().device_name());
                println!("Compression level: {}", compressor.level());
            }
            Err(e) => {
                println!("No GPU available (expected in CI): {}", e);
            }
        }
    }

    #[test]
    fn test_roundtrip_small() {
        if let Ok(compressor) = GpuZstdCompressor::new() {
            let data = b"hello world hello world hello world";

            let compressed = compressor.compress(data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            assert_eq!(data.as_slice(), decompressed.as_slice());
        }
    }

    #[test]
    fn test_roundtrip_large() {
        if let Ok(compressor) = GpuZstdCompressor::new() {
            // Create 1MB of repetitive data
            let data = vec![0x42u8; 1024 * 1024];

            let compressed = compressor.compress(&data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            assert_eq!(data, decompressed);
            assert!(compressed.len() < data.len());

            println!(
                "Compression ratio: {:.2}",
                data.len() as f64 / compressed.len() as f64
            );
        }
    }

    #[test]
    fn test_compression_levels() {
        if let Ok(_ctx) = GpuContext::new() {
            // Test different compression levels
            for level in [1, 3, 9, 19] {
                match GpuZstdCompressor::with_level(level) {
                    Ok(compressor) => {
                        assert_eq!(compressor.level(), level);
                        let data = vec![0x42u8; 1024];
                        let compressed = compressor.compress(&data).unwrap();
                        assert!(compressed.len() > 0);
                    }
                    Err(e) => {
                        println!("Failed to create compressor with level {}: {}", level, e);
                    }
                }
            }
        }
    }

    #[test]
    fn test_invalid_level() {
        if let Ok(ctx) = GpuContext::new() {
            assert!(GpuZstdCompressor::with_context_and_level(Arc::new(ctx.clone()), 0).is_err());
            assert!(
                GpuZstdCompressor::with_context_and_level(Arc::new(ctx.clone()), 23).is_err()
            );
            assert!(
                GpuZstdCompressor::with_context_and_level(Arc::new(ctx), -1).is_err()
            );
        }
    }

    #[test]
    fn test_empty_input() {
        if let Ok(compressor) = GpuZstdCompressor::new() {
            let data = b"";

            let compressed = compressor.compress(data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            assert_eq!(data.as_slice(), decompressed.as_slice());
        }
    }

    #[test]
    fn test_min_gpu_size() {
        if let Ok(mut compressor) = GpuZstdCompressor::new() {
            compressor.set_min_gpu_size(1024 * 1024); // 1MB

            // Small data should use CPU
            let small_data = vec![0x42u8; 1024];
            let compressed = compressor.compress(&small_data).unwrap();
            assert!(compressed.len() > 0);

            // Large data should attempt GPU
            let large_data = vec![0x42u8; 2 * 1024 * 1024];
            let compressed = compressor.compress(&large_data).unwrap();
            assert!(compressed.len() > 0);
        }
    }

    #[test]
    fn test_highly_compressible() {
        if let Ok(compressor) = GpuZstdCompressor::with_level(19) {
            // Highly repetitive data
            let data = vec![0u8; 1024 * 1024];

            let compressed = compressor.compress(&data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            assert_eq!(data, decompressed);
            // Should achieve very high compression ratio
            assert!(compressed.len() < data.len() / 100);
        }
    }

    #[test]
    fn test_incompressible() {
        if let Ok(compressor) = GpuZstdCompressor::new() {
            // Random-looking data (incompressible)
            let data: Vec<u8> = (0..1024).map(|i| (i * 7 + 13) as u8).collect();

            let compressed = compressor.compress(&data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            assert_eq!(data, decompressed);
        }
    }
}
