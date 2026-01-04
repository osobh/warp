//! Configuration for streaming pipeline
//!
//! This module provides configuration options for the triple-buffer
//! pipeline including buffer sizes, latency targets, and backpressure settings.

use std::time::Duration;

/// Configuration for the streaming pipeline
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Size of each chunk in bytes (default: 64KB for <5ms latency)
    pub chunk_size: usize,

    /// Number of buffers per pipeline stage (default: 3 for triple-buffering)
    pub buffer_count: usize,

    /// Maximum queue depth before backpressure kicks in
    pub max_queue_depth: usize,

    /// Target latency per chunk (default: 5ms)
    pub target_latency: Duration,

    /// Enable GPU acceleration (requires CUDA)
    pub use_gpu: bool,

    /// Number of concurrent streams for GPU operations
    pub gpu_stream_count: usize,

    /// Enable statistics collection
    pub collect_stats: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64 * 1024, // 64KB - optimized for <5ms latency
            buffer_count: 3,       // Triple buffering
            max_queue_depth: 16,   // Allow 16 chunks in flight
            target_latency: Duration::from_millis(5),
            use_gpu: true,
            gpu_stream_count: 4,
            collect_stats: true,
        }
    }
}

impl StreamConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure for low-latency streaming (<5ms per chunk)
    pub fn low_latency() -> Self {
        Self {
            chunk_size: 64 * 1024, // 64KB chunks
            buffer_count: 3,
            max_queue_depth: 8, // Smaller queue for lower latency
            target_latency: Duration::from_millis(5),
            use_gpu: true,
            gpu_stream_count: 2, // Fewer streams for lower latency
            collect_stats: true,
        }
    }

    /// Configure for high-throughput streaming (>10 GB/s)
    pub fn high_throughput() -> Self {
        Self {
            chunk_size: 1024 * 1024, // 1MB chunks
            buffer_count: 4,         // More buffers
            max_queue_depth: 32,     // Deeper queue
            target_latency: Duration::from_millis(50),
            use_gpu: true,
            gpu_stream_count: 8, // More concurrent streams
            collect_stats: true,
        }
    }

    /// Configure for CPU-only operation (no GPU)
    pub fn cpu_only() -> Self {
        Self {
            use_gpu: false,
            gpu_stream_count: 0,
            ..Self::default()
        }
    }

    /// Set custom chunk size
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }

    /// Set buffer count
    pub fn with_buffer_count(mut self, count: usize) -> Self {
        self.buffer_count = count;
        self
    }

    /// Set maximum queue depth
    pub fn with_max_queue_depth(mut self, depth: usize) -> Self {
        self.max_queue_depth = depth;
        self
    }

    /// Set target latency
    pub fn with_target_latency(mut self, latency: Duration) -> Self {
        self.target_latency = latency;
        self
    }

    /// Enable or disable GPU
    pub fn with_gpu(mut self, enabled: bool) -> Self {
        self.use_gpu = enabled;
        if !enabled {
            self.gpu_stream_count = 0;
        }
        self
    }

    /// Validate configuration
    pub fn validate(&self) -> crate::Result<()> {
        if self.chunk_size == 0 {
            return Err(crate::StreamError::InvalidConfig(
                "chunk_size must be > 0".to_string(),
            ));
        }

        if self.buffer_count < 2 {
            return Err(crate::StreamError::InvalidConfig(
                "buffer_count must be >= 2 for double-buffering".to_string(),
            ));
        }

        if self.max_queue_depth == 0 {
            return Err(crate::StreamError::InvalidConfig(
                "max_queue_depth must be > 0".to_string(),
            ));
        }

        if self.use_gpu && self.gpu_stream_count == 0 {
            return Err(crate::StreamError::InvalidConfig(
                "gpu_stream_count must be > 0 when use_gpu is true".to_string(),
            ));
        }

        Ok(())
    }

    /// Calculate memory requirement for pipeline buffers
    pub fn memory_requirement(&self) -> usize {
        // Each stage needs buffer_count buffers of chunk_size
        // 3 stages: input, process, output
        3 * self.buffer_count * self.chunk_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StreamConfig::default();
        assert_eq!(config.chunk_size, 64 * 1024);
        assert_eq!(config.buffer_count, 3);
        assert!(config.use_gpu);
    }

    #[test]
    fn test_low_latency_config() {
        let config = StreamConfig::low_latency();
        assert_eq!(config.chunk_size, 64 * 1024);
        assert_eq!(config.target_latency, Duration::from_millis(5));
        assert_eq!(config.max_queue_depth, 8);
    }

    #[test]
    fn test_high_throughput_config() {
        let config = StreamConfig::high_throughput();
        assert_eq!(config.chunk_size, 1024 * 1024);
        assert_eq!(config.max_queue_depth, 32);
    }

    #[test]
    fn test_cpu_only_config() {
        let config = StreamConfig::cpu_only();
        assert!(!config.use_gpu);
        assert_eq!(config.gpu_stream_count, 0);
    }

    #[test]
    fn test_builder_pattern() {
        let config = StreamConfig::new()
            .with_chunk_size(128 * 1024)
            .with_buffer_count(4)
            .with_max_queue_depth(32);

        assert_eq!(config.chunk_size, 128 * 1024);
        assert_eq!(config.buffer_count, 4);
        assert_eq!(config.max_queue_depth, 32);
    }

    #[test]
    fn test_validation_valid_config() {
        let config = StreamConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validation_zero_chunk_size() {
        let config = StreamConfig::new().with_chunk_size(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_insufficient_buffers() {
        let config = StreamConfig::new().with_buffer_count(1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_zero_queue_depth() {
        let config = StreamConfig::new().with_max_queue_depth(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_gpu_without_streams() {
        let mut config = StreamConfig::new();
        config.use_gpu = true;
        config.gpu_stream_count = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_memory_requirement() {
        let config = StreamConfig::new()
            .with_chunk_size(64 * 1024)
            .with_buffer_count(3);

        // 3 stages * 3 buffers * 64KB = 576KB
        assert_eq!(config.memory_requirement(), 3 * 3 * 64 * 1024);
    }
}
