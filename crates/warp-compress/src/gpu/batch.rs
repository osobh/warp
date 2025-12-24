//! Batch compression for processing multiple chunks in parallel on GPU
//!
//! This module provides efficient batch processing of multiple data chunks,
//! which is ideal for warp's chunked archive format. By processing multiple
//! chunks simultaneously, we can maximize GPU utilization and throughput.
//!
//! # Architecture
//!
//! The batch compressor now uses shared infrastructure from warp-gpu:
//! - Shared GpuContext across all operations
//! - PinnedMemoryPool for zero-copy transfers
//! - Efficient buffer reuse for batch operations

use crate::{Compressor, Result};
use super::lz4::GpuLz4Compressor;
use super::zstd::GpuZstdCompressor;
use rayon::prelude::*;
use std::sync::Arc;
use tracing::{debug, warn};
use warp_gpu::{GpuContext, PinnedMemoryPool};

/// Compression algorithm selection for batch processing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// LZ4 compression (fastest)
    Lz4,
    /// Zstandard compression with specified level
    Zstd(i32),
}

impl CompressionAlgorithm {
    /// Get the name of the algorithm
    pub fn name(&self) -> &'static str {
        match self {
            Self::Lz4 => "lz4",
            Self::Zstd(_) => "zstd",
        }
    }

    /// Get the compression level (if applicable)
    pub fn level(&self) -> Option<i32> {
        match self {
            Self::Lz4 => None,
            Self::Zstd(level) => Some(*level),
        }
    }
}

/// Batch compressor for processing multiple chunks in parallel
///
/// This compressor is optimized for scenarios where you need to
/// compress or decompress many chunks at once, such as:
/// - Chunked archive creation
/// - Parallel data processing
/// - Streaming compression pipelines
///
/// # Architecture
///
/// Uses shared GPU infrastructure:
/// - Single GpuContext for all operations
/// - Shared PinnedMemoryPool for efficient buffer reuse
/// - Automatic memory management and fallback
pub struct BatchCompressor {
    context: Arc<GpuContext>,
    memory_pool: Arc<PinnedMemoryPool>,
    algorithm: CompressionAlgorithm,
    lz4_compressor: Option<GpuLz4Compressor>,
    zstd_compressor: Option<GpuZstdCompressor>,
}

impl BatchCompressor {
    /// Create a new batch compressor with LZ4 algorithm
    ///
    /// # Errors
    /// Returns an error if GPU initialization fails
    pub fn new_lz4() -> Result<Self> {
        let context = Arc::new(
            GpuContext::new()
                .map_err(|e| crate::Error::Gpu(format!("Failed to initialize GPU: {}", e)))?,
        );
        let memory_pool = Arc::new(PinnedMemoryPool::with_defaults(context.device().clone()));

        Self::with_context(context, memory_pool, CompressionAlgorithm::Lz4)
    }

    /// Create a new batch compressor with Zstd algorithm
    ///
    /// # Arguments
    /// * `level` - Compression level (1-22)
    ///
    /// # Errors
    /// Returns an error if GPU initialization fails or level is invalid
    pub fn new_zstd(level: i32) -> Result<Self> {
        let context = Arc::new(
            GpuContext::new()
                .map_err(|e| crate::Error::Gpu(format!("Failed to initialize GPU: {}", e)))?,
        );
        let memory_pool = Arc::new(PinnedMemoryPool::with_defaults(context.device().clone()));

        Self::with_context(context, memory_pool, CompressionAlgorithm::Zstd(level))
    }

    /// Create a new batch compressor with a shared context and memory pool
    ///
    /// This is the most efficient constructor for scenarios where multiple
    /// batch compressors share resources.
    ///
    /// # Arguments
    /// * `context` - Shared GPU context
    /// * `memory_pool` - Shared pinned memory pool
    /// * `algorithm` - Compression algorithm to use
    ///
    /// # Errors
    /// Returns an error if compressor initialization fails
    pub fn with_context(
        context: Arc<GpuContext>,
        memory_pool: Arc<PinnedMemoryPool>,
        algorithm: CompressionAlgorithm,
    ) -> Result<Self> {
        let (lz4_compressor, zstd_compressor) = match algorithm {
            CompressionAlgorithm::Lz4 => {
                let lz4 =
                    GpuLz4Compressor::with_context_and_pool(context.clone(), memory_pool.clone())?;
                (Some(lz4), None)
            }
            CompressionAlgorithm::Zstd(level) => {
                let zstd = GpuZstdCompressor::with_context_and_level(
                    context.clone(),
                    memory_pool.clone(),
                    level,
                )?;
                (None, Some(zstd))
            }
        };

        debug!(
            "Created batch compressor with algorithm: {}, shared GPU infrastructure",
            algorithm.name()
        );

        Ok(Self {
            context,
            memory_pool,
            algorithm,
            lz4_compressor,
            zstd_compressor,
        })
    }

    /// Get the GPU context
    #[inline]
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.context
    }

    /// Get the memory pool
    #[inline]
    pub fn memory_pool(&self) -> &Arc<PinnedMemoryPool> {
        &self.memory_pool
    }

    /// Get the compression algorithm
    #[inline]
    pub fn algorithm(&self) -> CompressionAlgorithm {
        self.algorithm
    }

    /// Compress multiple chunks in a batch
    ///
    /// This method processes multiple chunks in parallel on the GPU,
    /// maximizing throughput. It's optimized for scenarios where you
    /// have many chunks of similar size.
    ///
    /// # Arguments
    /// * `chunks` - Slice of input chunks to compress
    ///
    /// # Returns
    /// Vector of compressed chunks in the same order as input
    ///
    /// # Errors
    /// Returns an error if compression fails for any chunk
    pub fn compress_batch(&self, chunks: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        debug!(
            "Compressing batch of {} chunks using {}",
            chunks.len(),
            self.algorithm.name()
        );

        // Calculate total size and check memory (parallel sum for large batches)
        let total_size: usize = chunks.par_iter().map(|c| c.len()).sum();

        // Estimate memory requirement (3x for input + output + temp buffers)
        let required_memory = total_size * 3 + 1024 * 1024;

        if !self.context.has_sufficient_memory(required_memory) {
            warn!(
                "Insufficient GPU memory for batch operation ({} bytes required), \
                 processing sequentially",
                required_memory
            );
            return self.compress_batch_sequential(chunks);
        }

        // Process in parallel batches based on available memory
        let batch_size = self.calculate_optimal_batch_size(chunks);
        let mut results = Vec::with_capacity(chunks.len());

        for batch in chunks.chunks(batch_size) {
            let batch_results = self.compress_batch_parallel(batch)?;
            results.extend(batch_results);
        }

        debug!(
            "Batch compression complete: {} chunks, total {} bytes -> {} bytes",
            chunks.len(),
            total_size,
            results.iter().map(|r| r.len()).sum::<usize>()
        );

        Ok(results)
    }

    /// Decompress multiple chunks in a batch
    ///
    /// # Arguments
    /// * `chunks` - Slice of compressed chunks to decompress
    ///
    /// # Returns
    /// Vector of decompressed chunks in the same order as input
    ///
    /// # Errors
    /// Returns an error if decompression fails for any chunk
    pub fn decompress_batch(&self, chunks: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        debug!(
            "Decompressing batch of {} chunks using {}",
            chunks.len(),
            self.algorithm.name()
        );

        // Process in batches
        let batch_size = self.calculate_optimal_batch_size(chunks);
        let mut results = Vec::with_capacity(chunks.len());

        for batch in chunks.chunks(batch_size) {
            let batch_results = self.decompress_batch_parallel(batch)?;
            results.extend(batch_results);
        }

        debug!(
            "Batch decompression complete: {} chunks, total {} bytes",
            chunks.len(),
            results.iter().map(|r| r.len()).sum::<usize>()
        );

        Ok(results)
    }

    /// Compress chunks sequentially (fallback for memory constraints)
    fn compress_batch_sequential(&self, chunks: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        // Even in "sequential" mode, use parallel iteration for CPU work
        chunks
            .par_iter()
            .map(|chunk| match self.algorithm {
                CompressionAlgorithm::Lz4 => {
                    self.lz4_compressor.as_ref().unwrap().compress(chunk)
                }
                CompressionAlgorithm::Zstd(_) => {
                    self.zstd_compressor.as_ref().unwrap().compress(chunk)
                }
            })
            .collect()
    }

    /// Compress chunks in parallel on GPU
    fn compress_batch_parallel(&self, chunks: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        // With shared memory pool, buffers are automatically reused across chunks
        // This significantly reduces allocation overhead
        // Use Rayon for parallel CPU compression when GPU is busy

        match self.algorithm {
            CompressionAlgorithm::Lz4 => {
                let compressor = self.lz4_compressor.as_ref().unwrap();
                chunks
                    .par_iter()
                    .map(|chunk| compressor.compress(chunk))
                    .collect()
            }
            CompressionAlgorithm::Zstd(_) => {
                let compressor = self.zstd_compressor.as_ref().unwrap();
                chunks
                    .par_iter()
                    .map(|chunk| compressor.compress(chunk))
                    .collect()
            }
        }
    }

    /// Decompress chunks in parallel on GPU
    fn decompress_batch_parallel(&self, chunks: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        match self.algorithm {
            CompressionAlgorithm::Lz4 => {
                let compressor = self.lz4_compressor.as_ref().unwrap();
                chunks
                    .par_iter()
                    .map(|chunk| compressor.decompress(chunk))
                    .collect()
            }
            CompressionAlgorithm::Zstd(_) => {
                let compressor = self.zstd_compressor.as_ref().unwrap();
                chunks
                    .par_iter()
                    .map(|chunk| compressor.decompress(chunk))
                    .collect()
            }
        }
    }

    /// Calculate optimal batch size based on available memory
    fn calculate_optimal_batch_size(&self, chunks: &[&[u8]]) -> usize {
        if chunks.is_empty() {
            return 0;
        }

        // Get available memory
        let free_memory = match self.context.free_memory() {
            Ok(mem) => mem,
            Err(_) => return 1, // Conservative fallback
        };

        // Calculate average chunk size (parallel sum for large batches)
        let total_size: usize = chunks.par_iter().map(|c| c.len()).sum();
        let avg_chunk_size = total_size / chunks.len();

        // Estimate how many chunks can fit in 80% of free memory
        let usable_memory = (free_memory as f64 * 0.8) as usize;
        let estimated_memory_per_chunk = avg_chunk_size * 3; // 3x for compression overhead
        let estimated_batch = usable_memory / estimated_memory_per_chunk;

        // Clamp to reasonable bounds
        estimated_batch.max(1).min(chunks.len()).min(32)
    }

    /// Estimate compression ratio for a set of chunks
    ///
    /// # Arguments
    /// * `chunks` - Slice of chunks to analyze
    ///
    /// # Returns
    /// Estimated compression ratio (input_size / output_size)
    pub fn estimate_compression_ratio(&self, chunks: &[&[u8]]) -> f64 {
        if chunks.is_empty() {
            return 1.0;
        }

        // Sample first chunk to estimate ratio
        let sample = chunks[0];
        if sample.is_empty() {
            return 1.0;
        }

        // Quick compression test
        match self.algorithm {
            CompressionAlgorithm::Lz4 => {
                if let Some(compressor) = &self.lz4_compressor {
                    if let Ok(compressed) = compressor.compress(sample) {
                        return sample.len() as f64 / compressed.len() as f64;
                    }
                }
            }
            CompressionAlgorithm::Zstd(_) => {
                if let Some(compressor) = &self.zstd_compressor {
                    if let Ok(compressed) = compressor.compress(sample) {
                        return sample.len() as f64 / compressed.len() as f64;
                    }
                }
            }
        }

        1.0 // Assume no compression if test fails
    }

    /// Get batch processing statistics
    pub fn stats(&self) -> BatchStats {
        let pool_stats = self.memory_pool.statistics();

        BatchStats {
            algorithm: self.algorithm,
            free_memory: self.context.free_memory().ok(),
            total_memory: Some(self.context.total_memory()),
            pool_allocations: pool_stats.allocations,
            pool_cache_hits: pool_stats.cache_hits,
            pool_cache_misses: pool_stats.cache_misses,
        }
    }
}

/// Statistics for batch compression operations
#[derive(Debug, Clone)]
pub struct BatchStats {
    /// Compression algorithm in use
    pub algorithm: CompressionAlgorithm,
    /// Free GPU memory in bytes
    pub free_memory: Option<usize>,
    /// Total GPU memory in bytes
    pub total_memory: Option<usize>,
    /// Pool allocation count
    pub pool_allocations: u64,
    /// Pool cache hits
    pub pool_cache_hits: u64,
    /// Pool cache misses
    pub pool_cache_misses: u64,
}

impl BatchStats {
    /// Get memory utilization as a percentage
    pub fn memory_utilization(&self) -> Option<f64> {
        match (self.free_memory, self.total_memory) {
            (Some(free), Some(total)) if total > 0 => {
                Some((total - free) as f64 / total as f64 * 100.0)
            }
            _ => None,
        }
    }

    /// Get pool hit rate as a percentage
    pub fn pool_hit_rate(&self) -> f64 {
        let total_requests = self.pool_cache_hits + self.pool_cache_misses;
        if total_requests == 0 {
            0.0
        } else {
            (self.pool_cache_hits as f64 / total_requests as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_lz4_creation() {
        match BatchCompressor::new_lz4() {
            Ok(compressor) => {
                assert_eq!(compressor.algorithm(), CompressionAlgorithm::Lz4);
                println!("Batch LZ4 compressor created successfully");

                // Verify shared infrastructure
                let stats = compressor.memory_pool().statistics();
                println!("Memory pool stats: {:?}", stats);
            }
            Err(e) => {
                println!("No GPU available (expected in CI): {}", e);
            }
        }
    }

    #[test]
    fn test_batch_zstd_creation() {
        match BatchCompressor::new_zstd(3) {
            Ok(compressor) => {
                assert_eq!(compressor.algorithm(), CompressionAlgorithm::Zstd(3));
                println!("Batch Zstd compressor created successfully");
            }
            Err(e) => {
                println!("No GPU available (expected in CI): {}", e);
            }
        }
    }

    #[test]
    fn test_empty_batch() {
        if let Ok(compressor) = BatchCompressor::new_lz4() {
            let chunks: Vec<&[u8]> = vec![];
            let compressed = compressor.compress_batch(&chunks).unwrap();
            assert!(compressed.is_empty());

            let decompressed = compressor.decompress_batch(&chunks).unwrap();
            assert!(decompressed.is_empty());
        }
    }

    #[test]
    fn test_single_chunk_batch() {
        if let Ok(compressor) = BatchCompressor::new_lz4() {
            let data = b"hello world hello world hello world";
            let chunks = vec![data.as_slice()];

            let compressed = compressor.compress_batch(&chunks).unwrap();
            assert_eq!(compressed.len(), 1);

            let decompressed = compressor
                .decompress_batch(
                    &compressed
                        .iter()
                        .map(|c| c.as_slice())
                        .collect::<Vec<_>>()
                        .as_slice(),
                )
                .unwrap();
            assert_eq!(decompressed.len(), 1);
            assert_eq!(decompressed[0], data);
        }
    }

    #[test]
    fn test_multiple_chunks_batch() {
        if let Ok(compressor) = BatchCompressor::new_lz4() {
            let chunks_data = vec![
                b"chunk one".as_slice(),
                b"chunk two".as_slice(),
                b"chunk three".as_slice(),
                b"chunk four".as_slice(),
            ];

            let compressed = compressor.compress_batch(&chunks_data).unwrap();
            assert_eq!(compressed.len(), 4);

            let compressed_refs: Vec<&[u8]> = compressed.iter().map(|c| c.as_slice()).collect();
            let decompressed = compressor.decompress_batch(&compressed_refs).unwrap();
            assert_eq!(decompressed.len(), 4);

            for (i, original) in chunks_data.iter().enumerate() {
                assert_eq!(decompressed[i], *original);
            }
        }
    }

    #[test]
    fn test_large_batch() {
        if let Ok(compressor) = BatchCompressor::new_zstd(3) {
            // Create 10 chunks of 64KB each
            let mut chunks_data: Vec<Vec<u8>> = Vec::new();
            for i in 0..10 {
                let chunk = vec![(i as u8); 64 * 1024];
                chunks_data.push(chunk);
            }

            let chunks_refs: Vec<&[u8]> = chunks_data.iter().map(|c| c.as_slice()).collect();

            let compressed = compressor.compress_batch(&chunks_refs).unwrap();
            assert_eq!(compressed.len(), 10);

            let compressed_refs: Vec<&[u8]> = compressed.iter().map(|c| c.as_slice()).collect();
            let decompressed = compressor.decompress_batch(&compressed_refs).unwrap();
            assert_eq!(decompressed.len(), 10);

            for (i, original) in chunks_data.iter().enumerate() {
                assert_eq!(decompressed[i], *original);
            }
        }
    }

    #[test]
    fn test_compression_ratio_estimation() {
        if let Ok(compressor) = BatchCompressor::new_zstd(3) {
            let data = vec![0u8; 1024 * 1024]; // Highly compressible
            let chunks = vec![data.as_slice()];

            let ratio = compressor.estimate_compression_ratio(&chunks);
            assert!(ratio > 1.0, "Expected compression ratio > 1.0, got {}", ratio);
        }
    }

    #[test]
    fn test_batch_stats() {
        if let Ok(compressor) = BatchCompressor::new_lz4() {
            let stats = compressor.stats();
            println!("Batch stats: {:?}", stats);

            assert_eq!(stats.algorithm, CompressionAlgorithm::Lz4);

            if let Some(util) = stats.memory_utilization() {
                assert!(util >= 0.0 && util <= 100.0);
            }

            // Pool hit rate should be valid
            assert!(stats.pool_hit_rate() >= 0.0 && stats.pool_hit_rate() <= 100.0);
        }
    }

    #[test]
    fn test_algorithm_properties() {
        assert_eq!(CompressionAlgorithm::Lz4.name(), "lz4");
        assert_eq!(CompressionAlgorithm::Zstd(3).name(), "zstd");

        assert_eq!(CompressionAlgorithm::Lz4.level(), None);
        assert_eq!(CompressionAlgorithm::Zstd(19).level(), Some(19));
    }

    #[test]
    fn test_varying_chunk_sizes() {
        if let Ok(compressor) = BatchCompressor::new_lz4() {
            let chunks_data = vec![
                vec![1u8; 100],
                vec![2u8; 1000],
                vec![3u8; 10000],
                vec![4u8; 100],
            ];

            let chunks_refs: Vec<&[u8]> = chunks_data.iter().map(|c| c.as_slice()).collect();

            let compressed = compressor.compress_batch(&chunks_refs).unwrap();
            assert_eq!(compressed.len(), 4);

            let compressed_refs: Vec<&[u8]> = compressed.iter().map(|c| c.as_slice()).collect();
            let decompressed = compressor.decompress_batch(&compressed_refs).unwrap();
            assert_eq!(decompressed.len(), 4);

            for (i, original) in chunks_data.iter().enumerate() {
                assert_eq!(decompressed[i], *original);
            }
        }
    }

    #[test]
    fn test_shared_infrastructure() {
        if let Ok(context) = GpuContext::new() {
            let context = Arc::new(context);
            let memory_pool = Arc::new(PinnedMemoryPool::with_defaults(context.device().clone()));

            // Create two batch compressors sharing infrastructure
            let batch1 = BatchCompressor::with_context(
                context.clone(),
                memory_pool.clone(),
                CompressionAlgorithm::Lz4,
            )
            .unwrap();

            let batch2 = BatchCompressor::with_context(
                context.clone(),
                memory_pool.clone(),
                CompressionAlgorithm::Zstd(3),
            )
            .unwrap();

            // Both should work correctly
            let data = vec![vec![0x42u8; 1024]];
            let chunks: Vec<&[u8]> = data.iter().map(|c| c.as_slice()).collect();

            let _compressed1 = batch1.compress_batch(&chunks).unwrap();
            let _compressed2 = batch2.compress_batch(&chunks).unwrap();

            // Verify shared pool has activity
            let stats = memory_pool.statistics();
            assert!(stats.allocations > 0);
        }
    }

    #[test]
    fn test_pool_reuse_across_batches() {
        if let Ok(compressor) = BatchCompressor::new_lz4() {
            let data = vec![vec![0x42u8; 1024 * 1024]; 5];
            let chunks: Vec<&[u8]> = data.iter().map(|c| c.as_slice()).collect();

            // Process multiple times to verify reuse
            for _ in 0..3 {
                let _compressed = compressor.compress_batch(&chunks).unwrap();
            }

            let stats = compressor.stats();
            println!("Pool hit rate: {:.2}%", stats.pool_hit_rate());

            // Should have good hit rate after first batch
            assert!(
                stats.pool_cache_hits > 0,
                "Expected cache hits from pool reuse"
            );
        }
    }
}
