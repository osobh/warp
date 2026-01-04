//! Real-time streaming pipeline with triple-buffer orchestration
//!
//! This crate provides a high-performance streaming pipeline for encrypted
//! data transfer with GPU acceleration. Key features:
//!
//! - **Triple-buffer pipeline**: Overlaps I/O, processing, and output stages
//! - **Low latency**: <5ms per 64KB chunk target
//! - **High throughput**: >10 GB/s with GPU acceleration
//! - **Backpressure handling**: Prevents memory exhaustion under load
//! - **Statistics tracking**: Real-time performance monitoring
//!
//! # Architecture
//!
//! The pipeline consists of three concurrent stages:
//!
//! ```text
//! ┌─────────┐    ┌─────────────┐    ┌────────┐
//! │  Input  │───▶│   Process   │───▶│ Output │
//! │ (Read)  │    │ (Encrypt)   │    │(Write) │
//! └─────────┘    └─────────────┘    └────────┘
//!     ▲               ▲                 ▲
//!     │               │                 │
//!   Buffer 1        Buffer 2          Buffer 3
//! ```
//!
//! Each stage processes a different buffer simultaneously, maximizing
//! throughput by hiding I/O and GPU latency.
//!
//! # Example
//!
//! ```no_run
//! use warp_stream::{StreamConfig, Pipeline, PipelineBuilder};
//! use std::io::Cursor;
//!
//! # async fn example() -> warp_stream::Result<()> {
//! let pipeline = PipelineBuilder::new()
//!     .low_latency()
//!     .with_key([0u8; 32])
//!     .with_nonce([0u8; 12])
//!     .build()?;
//!
//! // Start streaming
//! let input = Cursor::new(vec![0u8; 1024]);
//! let output = Vec::new();
//! let stats = pipeline.run(input, output).await?;
//!
//! println!("Throughput: {:.2} GB/s", stats.throughput_gbps);
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod error;
pub mod flow;
pub mod gpu_crypto;
pub mod pipeline;
pub mod pooled;
pub mod stats;

pub use config::StreamConfig;
pub use error::{Result, StreamError};
pub use flow::{BackpressureController, FlowControl};
pub use gpu_crypto::{CryptoBackend, GpuCryptoContext, SharedCryptoContext, create_crypto_context};
pub use pipeline::{Pipeline, PipelineBuilder};
pub use pooled::{BufferPoolManager, PooledBuffer, SharedPoolManager, create_pool_manager};
pub use stats::{PipelineStats, SharedStats, StageStats, StatsSummary};
