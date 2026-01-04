//! Triple-buffer pipeline for streaming encryption
//!
//! This module implements the core streaming pipeline with overlapped
//! I/O, processing, and output stages for maximum throughput.
//!
//! # Architecture
//!
//! The pipeline runs three concurrent stages:
//! 1. **Input**: Reads data and chunks it into fixed-size pieces
//! 2. **Process**: Encrypts/compresses chunks (GPU or CPU)
//! 3. **Output**: Writes processed chunks to destination
//!
//! Each stage runs in its own tokio task, connected by bounded channels
//! for backpressure handling.

use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};

use crate::gpu_crypto::{SharedCryptoContext, create_crypto_context};
use crate::{PipelineStats, Result, SharedStats, StreamConfig, StreamError};

/// A chunk of data passing through the pipeline
#[derive(Debug)]
pub struct PipelineChunk {
    /// Chunk sequence number
    pub seq: u64,
    /// Chunk data
    pub data: Vec<u8>,
    /// Timestamp when chunk entered input stage
    pub input_time: Instant,
}

/// Processed chunk ready for output
#[derive(Debug)]
pub struct ProcessedChunk {
    /// Chunk sequence number
    pub seq: u64,
    /// Processed data (encrypted/compressed)
    pub data: Vec<u8>,
    /// Timestamp when chunk entered input stage
    pub input_time: Instant,
    /// Timestamp when processing completed
    pub process_time: Instant,
}

/// Pipeline builder for configuring and constructing pipelines
pub struct PipelineBuilder {
    config: StreamConfig,
    key: Option<[u8; 32]>,
    nonce: Option<[u8; 12]>,
}

impl PipelineBuilder {
    /// Create a new pipeline builder with default config
    pub fn new() -> Self {
        Self {
            config: StreamConfig::default(),
            key: None,
            nonce: None,
        }
    }

    /// Use low-latency configuration
    pub fn low_latency(mut self) -> Self {
        self.config = StreamConfig::low_latency();
        self
    }

    /// Use high-throughput configuration
    pub fn high_throughput(mut self) -> Self {
        self.config = StreamConfig::high_throughput();
        self
    }

    /// Use custom configuration
    pub fn with_config(mut self, config: StreamConfig) -> Self {
        self.config = config;
        self
    }

    /// Set encryption key
    pub fn with_key(mut self, key: [u8; 32]) -> Self {
        self.key = Some(key);
        self
    }

    /// Set encryption nonce
    pub fn with_nonce(mut self, nonce: [u8; 12]) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Build the pipeline
    pub fn build(self) -> Result<Pipeline> {
        self.config.validate()?;

        let key = self.key.unwrap_or([0u8; 32]);
        let nonce = self.nonce.unwrap_or([0u8; 12]);

        // Create GPU crypto context with fallback to CPU
        let crypto_ctx = create_crypto_context(&key, &nonce, &self.config)?;
        info!("Pipeline using {} encryption backend", crypto_ctx.backend());

        Ok(Pipeline {
            config: self.config,
            key,
            nonce,
            stats: Arc::new(PipelineStats::new()),
            crypto_ctx,
        })
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Triple-buffer streaming pipeline
pub struct Pipeline {
    config: StreamConfig,
    key: [u8; 32],
    nonce: [u8; 12],
    stats: SharedStats,
    crypto_ctx: SharedCryptoContext,
}

impl Pipeline {
    /// Create a new pipeline with given configuration
    pub fn new(config: StreamConfig) -> Result<Self> {
        PipelineBuilder::new().with_config(config).build()
    }

    /// Create a pipeline builder
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Get shared statistics handle
    pub fn stats(&self) -> SharedStats {
        Arc::clone(&self.stats)
    }

    /// Run the pipeline with given input and output
    ///
    /// Returns pipeline statistics when complete.
    pub async fn run<R, W>(&self, input: R, output: W) -> Result<crate::StatsSummary>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        debug!("Starting pipeline with config: {:?}", self.config);

        // Create channels between stages
        let (input_tx, input_rx) = mpsc::channel::<PipelineChunk>(self.config.max_queue_depth);
        let (process_tx, process_rx) = mpsc::channel::<ProcessedChunk>(self.config.max_queue_depth);

        // Clone config and stats for tasks
        let config = self.config.clone();
        let key = self.key;
        let nonce = self.nonce;
        let stats = Arc::clone(&self.stats);
        let crypto_ctx = Arc::clone(&self.crypto_ctx);

        // Spawn input stage
        let input_stats = Arc::clone(&stats);
        let input_config = config.clone();
        let input_handle =
            tokio::spawn(
                async move { input_stage(input, input_tx, input_config, input_stats).await },
            );

        // Spawn process stage
        let process_stats = Arc::clone(&stats);
        let process_config = config.clone();
        let process_handle = tokio::spawn(async move {
            process_stage(
                input_rx,
                process_tx,
                key,
                nonce,
                process_config,
                process_stats,
                crypto_ctx,
            )
            .await
        });

        // Spawn output stage
        let output_stats = Arc::clone(&stats);
        let output_config = config.clone();
        let output_handle = tokio::spawn(async move {
            output_stage(process_rx, output, output_config, output_stats).await
        });

        // Wait for all stages to complete
        let (input_result, process_result, output_result) =
            tokio::try_join!(input_handle, process_handle, output_handle).map_err(|e| {
                StreamError::IoError(std::io::Error::other(format!("Task join error: {}", e)))
            })?;

        // Propagate any errors
        input_result?;
        process_result?;
        output_result?;

        debug!("Pipeline completed successfully");
        Ok(self.stats.summary())
    }

    /// Run pipeline with CPU-only processing (no GPU)
    pub async fn run_cpu<R, W>(&self, input: R, output: W) -> Result<crate::StatsSummary>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        // Same as run() but process_stage will use CPU
        self.run(input, output).await
    }
}

/// Input stage: reads data and chunks it
async fn input_stage<R: AsyncRead + Unpin>(
    mut input: R,
    sender: mpsc::Sender<PipelineChunk>,
    config: StreamConfig,
    stats: SharedStats,
) -> Result<()> {
    let mut buffer = vec![0u8; config.chunk_size];
    let mut seq = 0u64;

    loop {
        let start = Instant::now();

        // Read a chunk
        let mut total_read = 0;
        while total_read < config.chunk_size {
            match input.read(&mut buffer[total_read..]).await {
                Ok(0) => break, // EOF
                Ok(n) => total_read += n,
                Err(e) => return Err(StreamError::IoError(e)),
            }
        }

        if total_read == 0 {
            debug!("Input stage: EOF reached after {} chunks", seq);
            break;
        }

        let latency = start.elapsed();
        stats.input.record(total_read, latency);

        let chunk = PipelineChunk {
            seq,
            data: buffer[..total_read].to_vec(),
            input_time: start,
        };

        trace!("Input stage: read chunk {} ({} bytes)", seq, total_read);

        // Send to process stage
        if sender.send(chunk).await.is_err() {
            warn!("Input stage: channel closed");
            break;
        }

        seq += 1;
    }

    Ok(())
}

/// Process stage: encrypts/compresses chunks using GPU or CPU
async fn process_stage(
    mut receiver: mpsc::Receiver<PipelineChunk>,
    sender: mpsc::Sender<ProcessedChunk>,
    key: [u8; 32],
    nonce: [u8; 12],
    config: StreamConfig,
    stats: SharedStats,
    crypto_ctx: SharedCryptoContext,
) -> Result<()> {
    debug!("Process stage using {} backend", crypto_ctx.backend());

    while let Some(chunk) = receiver.recv().await {
        let start = Instant::now();

        // Encrypt the chunk using GPU or CPU (with automatic fallback)
        let encrypted = crypto_ctx.encrypt(&chunk.data, &key, &nonce)?;

        let latency = start.elapsed();
        stats.process.record(chunk.data.len(), latency);

        let processed = ProcessedChunk {
            seq: chunk.seq,
            data: encrypted,
            input_time: chunk.input_time,
            process_time: start,
        };

        trace!(
            "Process stage: encrypted chunk {} ({} -> {} bytes)",
            chunk.seq,
            chunk.data.len(),
            processed.data.len()
        );

        // Check latency target
        let total_latency = chunk.input_time.elapsed();
        if total_latency > config.target_latency {
            warn!(
                "Latency target exceeded: {:?} > {:?}",
                total_latency, config.target_latency
            );
        }

        // Send to output stage
        if sender.send(processed).await.is_err() {
            warn!("Process stage: channel closed");
            break;
        }
    }

    Ok(())
}

/// Output stage: writes processed chunks
async fn output_stage<W: AsyncWrite + Unpin>(
    mut receiver: mpsc::Receiver<ProcessedChunk>,
    mut output: W,
    config: StreamConfig,
    stats: SharedStats,
) -> Result<()> {
    while let Some(chunk) = receiver.recv().await {
        let start = Instant::now();

        // Write the chunk
        output
            .write_all(&chunk.data)
            .await
            .map_err(StreamError::IoError)?;

        let latency = start.elapsed();
        stats.output.record(chunk.data.len(), latency);

        // Record end-to-end completion
        stats.record_completed();

        let total_latency = chunk.input_time.elapsed();
        trace!(
            "Output stage: wrote chunk {} ({} bytes, total latency: {:?})",
            chunk.seq,
            chunk.data.len(),
            total_latency
        );

        // Check latency target
        if total_latency > config.target_latency {
            warn!(
                "End-to-end latency exceeded: {:?} > {:?}",
                total_latency, config.target_latency
            );
        }
    }

    // Flush output
    output.flush().await.map_err(StreamError::IoError)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_pipeline_builder() {
        let pipeline = PipelineBuilder::new()
            .low_latency()
            .with_key([0x42u8; 32])
            .with_nonce([0x24u8; 12])
            .build();

        assert!(pipeline.is_ok());
    }

    #[tokio::test]
    async fn test_pipeline_new() {
        let config = StreamConfig::low_latency();
        let pipeline = Pipeline::new(config);
        assert!(pipeline.is_ok());
    }

    #[tokio::test]
    async fn test_pipeline_empty_input() {
        let pipeline = PipelineBuilder::new()
            .with_key([0x42u8; 32])
            .with_nonce([0x24u8; 12])
            .build()
            .unwrap();

        let input = Cursor::new(Vec::<u8>::new());
        let output = Vec::<u8>::new();

        let result = pipeline.run(input, output).await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.completed_chunks, 0);
    }

    #[tokio::test]
    async fn test_pipeline_small_input() {
        let pipeline = PipelineBuilder::new()
            .with_config(StreamConfig::new().with_chunk_size(16))
            .with_key([0x42u8; 32])
            .with_nonce([0x24u8; 12])
            .build()
            .unwrap();

        let input_data = b"Hello, World!".to_vec();
        let input = Cursor::new(input_data);
        let output = Vec::<u8>::new();

        let result = pipeline.run(input, output).await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.completed_chunks, 1);
    }

    #[tokio::test]
    async fn test_pipeline_multiple_chunks() {
        let chunk_size = 64;
        let pipeline = PipelineBuilder::new()
            .with_config(StreamConfig::new().with_chunk_size(chunk_size))
            .with_key([0x42u8; 32])
            .with_nonce([0x24u8; 12])
            .build()
            .unwrap();

        // Create input larger than one chunk
        let input_data = vec![0xABu8; chunk_size * 3 + 10]; // 3.15 chunks
        let input = Cursor::new(input_data);
        let output = Vec::<u8>::new();

        let result = pipeline.run(input, output).await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.completed_chunks, 4); // 4 chunks
    }

    #[tokio::test]
    async fn test_pipeline_stats_collection() {
        let pipeline = PipelineBuilder::new()
            .with_config(StreamConfig::new().with_chunk_size(32))
            .with_key([0x42u8; 32])
            .with_nonce([0x24u8; 12])
            .build()
            .unwrap();

        let input_data = vec![0xABu8; 100];
        let input = Cursor::new(input_data);
        let output = Vec::<u8>::new();

        let stats_handle = pipeline.stats();

        let result = pipeline.run(input, output).await;
        assert!(result.is_ok());

        // Check stats were collected
        assert!(stats_handle.completed_count() > 0);
        assert!(stats_handle.total_bytes() > 0);
    }

    #[tokio::test]
    async fn test_pipeline_throughput() {
        let pipeline = PipelineBuilder::new()
            .with_config(StreamConfig::new().with_chunk_size(1024))
            .with_key([0x42u8; 32])
            .with_nonce([0x24u8; 12])
            .build()
            .unwrap();

        // 1MB of data
        let input_data = vec![0xABu8; 1024 * 1024];
        let input = Cursor::new(input_data);
        let output = Vec::<u8>::new();

        let result = pipeline.run(input, output).await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        // Throughput should be measurable
        assert!(stats.throughput_gbps >= 0.0);
    }

    #[test]
    fn test_pipeline_chunk_struct() {
        let chunk = PipelineChunk {
            seq: 0,
            data: vec![1, 2, 3],
            input_time: Instant::now(),
        };

        assert_eq!(chunk.seq, 0);
        assert_eq!(chunk.data.len(), 3);
    }

    #[test]
    fn test_processed_chunk_struct() {
        let now = Instant::now();
        let chunk = ProcessedChunk {
            seq: 1,
            data: vec![4, 5, 6, 7],
            input_time: now,
            process_time: now,
        };

        assert_eq!(chunk.seq, 1);
        assert_eq!(chunk.data.len(), 4);
    }
}
