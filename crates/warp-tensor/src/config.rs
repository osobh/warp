//! Configuration for tensor storage

use serde::{Deserialize, Serialize};

/// Tensor storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensorConfig {
    /// Bucket name for tensor storage
    pub bucket: String,
    /// Prefix for tensor objects
    pub prefix: String,
    /// Chunk configuration
    pub chunk: ChunkConfig,
    /// Compression configuration
    pub compression: CompressionConfig,
    /// Enable deduplication
    pub dedup_enabled: bool,
    /// Enable lazy loading by default
    pub lazy_loading: bool,
    /// Maximum concurrent operations
    pub max_concurrent_ops: usize,
    /// Cache configuration
    pub cache: CacheConfig,
}

impl Default for TensorConfig {
    fn default() -> Self {
        Self {
            bucket: "__warp_tensor__".to_string(),
            prefix: "tensors".to_string(),
            chunk: ChunkConfig::default(),
            compression: CompressionConfig::default(),
            dedup_enabled: true,
            lazy_loading: true,
            max_concurrent_ops: 16,
            cache: CacheConfig::default(),
        }
    }
}

/// Chunk configuration for large tensors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkConfig {
    /// Minimum tensor size to enable chunking (bytes)
    pub chunk_threshold: u64,
    /// Target chunk size (bytes)
    pub chunk_size: u64,
    /// Maximum chunks per tensor
    pub max_chunks: u32,
    /// Enable parallel chunk upload
    pub parallel_upload: bool,
    /// Parallel upload concurrency
    pub upload_concurrency: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_threshold: 64 * 1024 * 1024, // 64 MB
            chunk_size: 16 * 1024 * 1024,      // 16 MB chunks
            max_chunks: 1024,
            parallel_upload: true,
            upload_concurrency: 4,
        }
    }
}

/// Compression configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Enable compression
    pub enabled: bool,
    /// Compression algorithm
    pub algorithm: CompressionAlgorithm,
    /// Compression level (1-22 for zstd, 1-9 for lz4)
    pub level: u8,
    /// Minimum tensor size to compress (bytes)
    pub min_size: u64,
    /// Use adaptive compression (choose based on data)
    pub adaptive: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            algorithm: CompressionAlgorithm::Zstd,
            level: 3,
            min_size: 1024, // 1 KB
            adaptive: true,
        }
    }
}

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression
    None,
    /// LZ4 (fast, moderate compression)
    Lz4,
    /// Zstd (balanced speed and ratio)
    Zstd,
    /// Zstd with dictionary (better for similar data)
    ZstdDict,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Enable metadata caching
    pub metadata_cache: bool,
    /// Metadata cache size (entries)
    pub metadata_cache_size: usize,
    /// Enable data caching
    pub data_cache: bool,
    /// Data cache size (bytes)
    pub data_cache_size: u64,
    /// Cache TTL (seconds)
    pub ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            metadata_cache: true,
            metadata_cache_size: 10_000,
            data_cache: true,
            data_cache_size: 1024 * 1024 * 1024, // 1 GB
            ttl_secs: 3600,                      // 1 hour
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TensorConfig::default();
        assert!(config.dedup_enabled);
        assert!(config.lazy_loading);
        assert_eq!(config.max_concurrent_ops, 16);
    }

    #[test]
    fn test_chunk_config() {
        let config = ChunkConfig::default();
        assert_eq!(config.chunk_size, 16 * 1024 * 1024);
        assert!(config.parallel_upload);
    }

    #[test]
    fn test_compression_config() {
        let config = CompressionConfig::default();
        assert!(config.enabled);
        assert_eq!(config.algorithm, CompressionAlgorithm::Zstd);
        assert!(config.adaptive);
    }
}
