//! Traits for DPU-accelerated operations
//!
//! Defines the interface for DPU-accelerated hashing, encryption,
//! compression, and erasure coding operations. All operations support
//! batch processing and inline (zero-copy) modes.

use crate::backend::{DpuBuffer, DpuType};
use crate::error::Result;

/// Base trait for DPU-accelerated operations
///
/// Provides common functionality for deciding when to use DPU acceleration
/// versus CPU fallback.
pub trait DpuOp: Send + Sync {
    /// Minimum input size for DPU acceleration
    ///
    /// Below this threshold, CPU may be faster due to DPU overhead.
    /// DPUs have lower overhead than GPUs, so threshold is smaller (4KB vs 64KB).
    fn min_dpu_size(&self) -> usize {
        4 * 1024 // 4KB default
    }

    /// Check if input should use DPU based on size and availability
    fn should_use_dpu(&self, input_size: usize) -> bool {
        input_size >= self.min_dpu_size() && self.is_dpu_available()
    }

    /// Check if DPU acceleration is available
    fn is_dpu_available(&self) -> bool;

    /// Get operation name for logging/metrics
    fn name(&self) -> &'static str;

    /// Get the backend type being used
    fn backend(&self) -> DpuType;

    /// Get operation statistics
    fn stats(&self) -> DpuOpStats {
        DpuOpStats::default()
    }
}

/// Statistics for DPU operations
#[derive(Debug, Default, Clone)]
pub struct DpuOpStats {
    /// Total operations performed
    pub total_ops: u64,
    /// Operations on DPU
    pub dpu_ops: u64,
    /// Operations on CPU (fallback)
    pub cpu_ops: u64,
    /// Total bytes processed
    pub total_bytes: u64,
    /// Bytes processed on DPU
    pub dpu_bytes: u64,
    /// DPU processing time (nanoseconds)
    pub dpu_time_ns: u64,
    /// CPU fallback time (nanoseconds)
    pub cpu_time_ns: u64,
    /// Inline operations (zero-copy network path)
    pub inline_ops: u64,
}

impl DpuOpStats {
    /// Get the ratio of DPU operations to total
    #[must_use]
    pub fn dpu_ratio(&self) -> f64 {
        if self.total_ops == 0 {
            0.0
        } else {
            self.dpu_ops as f64 / self.total_ops as f64
        }
    }

    /// Get average throughput in bytes per second
    #[must_use]
    pub fn throughput_bps(&self) -> f64 {
        let total_time_s = (self.dpu_time_ns + self.cpu_time_ns) as f64 / 1_000_000_000.0;
        if total_time_s == 0.0 {
            0.0
        } else {
            self.total_bytes as f64 / total_time_s
        }
    }
}

/// DPU-accelerated hashing operations
///
/// Supports BLAKE3 (via CPU) and SHA-256/512 (via DPU when available).
pub trait DpuHasher: DpuOp {
    /// Hash a single input buffer
    fn hash(&self, input: &[u8]) -> Result<[u8; 32]>;

    /// Hash multiple buffers in batch (pipelined on DPU)
    fn hash_batch(&self, inputs: &[&[u8]]) -> Result<Vec<[u8; 32]>>;

    /// Hash with keyed mode (for MAC operations)
    fn hash_keyed(&self, key: &[u8; 32], input: &[u8]) -> Result<[u8; 32]>;

    /// Create an incremental hasher for streaming
    fn hasher(&self) -> Box<dyn IncrementalHasher>;

    /// Get the algorithm name
    fn algorithm(&self) -> &'static str;

    /// Get the digest size in bytes
    fn digest_size(&self) -> usize {
        32
    }
}

/// Incremental hasher for streaming data
pub trait IncrementalHasher: Send {
    /// Update with more data
    fn update(&mut self, data: &[u8]);

    /// Finalize and get the hash
    fn finalize(self: Box<Self>) -> [u8; 32];

    /// Reset to initial state
    fn reset(&mut self);
}

/// DPU-accelerated encryption operations
///
/// Supports ChaCha20-Poly1305 AEAD encryption with optional DPU acceleration.
pub trait DpuCipher: DpuOp {
    /// Encrypt plaintext with AEAD
    fn encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: Option<&[u8]>,
    ) -> Result<Vec<u8>>;

    /// Decrypt ciphertext with AEAD
    fn decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: Option<&[u8]>,
    ) -> Result<Vec<u8>>;

    /// Encrypt with inline processing (data stays on DPU/network path)
    ///
    /// This is the zero-copy path for maximum performance.
    fn encrypt_inline(
        &self,
        input_buffer: &dyn DpuBuffer,
        output_buffer: &mut dyn DpuBuffer,
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<usize>;

    /// Decrypt with inline processing
    fn decrypt_inline(
        &self,
        input_buffer: &dyn DpuBuffer,
        output_buffer: &mut dyn DpuBuffer,
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<usize>;

    /// Batch encrypt multiple chunks
    fn encrypt_batch(
        &self,
        plaintexts: &[&[u8]],
        key: &[u8; 32],
        nonces: &[[u8; 12]],
        aad: Option<&[u8]>,
    ) -> Result<Vec<Vec<u8>>>;

    /// Batch decrypt multiple chunks
    fn decrypt_batch(
        &self,
        ciphertexts: &[&[u8]],
        key: &[u8; 32],
        nonces: &[[u8; 12]],
        aad: Option<&[u8]>,
    ) -> Result<Vec<Vec<u8>>>;

    /// Get the algorithm name
    fn algorithm(&self) -> &'static str;

    /// Get the key size in bytes
    fn key_size(&self) -> usize {
        32
    }

    /// Get the nonce size in bytes
    fn nonce_size(&self) -> usize {
        12
    }

    /// Get the authentication tag size in bytes
    fn tag_size(&self) -> usize {
        16
    }
}

/// DPU-accelerated compression operations
///
/// Supports zstd and lz4 compression with optional DPU acceleration.
pub trait DpuCompressor: DpuOp {
    /// Compress data
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>>;

    /// Decompress data
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>>;

    /// Compress with inline processing (zero-copy on network path)
    fn compress_inline(
        &self,
        input_buffer: &dyn DpuBuffer,
        output_buffer: &mut dyn DpuBuffer,
    ) -> Result<usize>;

    /// Decompress with inline processing
    fn decompress_inline(
        &self,
        input_buffer: &dyn DpuBuffer,
        output_buffer: &mut dyn DpuBuffer,
    ) -> Result<usize>;

    /// Batch compress multiple chunks
    fn compress_batch(&self, inputs: &[&[u8]]) -> Result<Vec<Vec<u8>>>;

    /// Batch decompress multiple chunks
    fn decompress_batch(&self, inputs: &[&[u8]]) -> Result<Vec<Vec<u8>>>;

    /// Get the algorithm name
    fn algorithm(&self) -> &'static str;

    /// Get the compression level (if applicable)
    fn level(&self) -> Option<i32>;

    /// Estimate compressed size for pre-allocation
    fn estimate_compressed_size(&self, input_len: usize) -> usize {
        // Default: assume worst case (no compression)
        input_len + 32 // Add overhead for header
    }
}

/// DPU-accelerated erasure coding operations
///
/// Supports Reed-Solomon erasure coding for fault tolerance.
pub trait DpuErasureCoder: DpuOp {
    /// Encode data into shards
    fn encode(&self, data: &[u8]) -> Result<Vec<Vec<u8>>>;

    /// Decode shards back to data
    ///
    /// Takes a slice of optional shards (None for missing shards).
    fn decode(&self, shards: &[Option<Vec<u8>>]) -> Result<Vec<u8>>;

    /// Encode with inline processing (zero-copy on network path)
    fn encode_inline(
        &self,
        input_buffer: &dyn DpuBuffer,
        output_buffers: &mut [&mut dyn DpuBuffer],
    ) -> Result<()>;

    /// Decode with inline processing
    fn decode_inline(
        &self,
        input_buffers: &[Option<&dyn DpuBuffer>],
        output_buffer: &mut dyn DpuBuffer,
    ) -> Result<()>;

    /// Reconstruct missing shards from available ones
    fn reconstruct(&self, shards: &mut [Option<Vec<u8>>]) -> Result<()>;

    /// Get the number of data shards
    fn data_shards(&self) -> usize;

    /// Get the number of parity shards
    fn parity_shards(&self) -> usize;

    /// Get the total number of shards
    fn total_shards(&self) -> usize {
        self.data_shards() + self.parity_shards()
    }

    /// Get the maximum number of shards that can be lost
    fn max_recoverable_loss(&self) -> usize {
        self.parity_shards()
    }

    /// Calculate shard size for a given data size
    fn shard_size(&self, data_len: usize) -> usize {
        let data_shards = self.data_shards();
        (data_len + data_shards - 1) / data_shards
    }
}

/// Compression algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionAlgorithm {
    /// Zstandard compression (good ratio, moderate speed)
    #[default]
    Zstd,
    /// LZ4 compression (fast, moderate ratio)
    Lz4,
}

impl CompressionAlgorithm {
    /// Get the algorithm name
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            CompressionAlgorithm::Zstd => "zstd",
            CompressionAlgorithm::Lz4 => "lz4",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpu_op_stats() {
        let stats = DpuOpStats {
            total_ops: 100,
            dpu_ops: 80,
            cpu_ops: 20,
            total_bytes: 1_000_000,
            dpu_bytes: 800_000,
            dpu_time_ns: 1_000_000_000, // 1 second
            cpu_time_ns: 500_000_000,   // 0.5 seconds
            inline_ops: 50,
        };

        assert!((stats.dpu_ratio() - 0.8).abs() < 0.001);
        // 1M bytes in 1.5 seconds = ~666KB/s
        assert!(stats.throughput_bps() > 600_000.0);
    }

    #[test]
    fn test_compression_algorithm() {
        assert_eq!(CompressionAlgorithm::Zstd.name(), "zstd");
        assert_eq!(CompressionAlgorithm::Lz4.name(), "lz4");
    }
}
