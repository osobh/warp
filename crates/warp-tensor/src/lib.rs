//! Tensor-optimized storage for WARP
//!
//! This crate provides native tensor format support with lazy loading capabilities,
//! optimized for ML model checkpoints, training data, and inference workloads.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     ML Applications                              │
//! │    (PyTorch, TensorFlow, JAX, Custom Frameworks)                │
//! └────────────────────────────┬────────────────────────────────────┘
//!                              │
//! ┌────────────────────────────▼────────────────────────────────────┐
//! │                       warp-tensor                                │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
//! │  │  TensorIO   │  │ ModelStore  │  │    CheckpointManager    │ │
//! │  │ (formats)   │  │ (versioning)│  │ (incremental, sharded)  │ │
//! │  └──────┬──────┘  └──────┬──────┘  └────────────┬────────────┘ │
//! │         │                │                      │               │
//! │  ┌──────▼────────────────▼──────────────────────▼──────────┐   │
//! │  │                   TensorStore                            │   │
//! │  │  - Lazy loading (load tensor metadata, defer data)       │   │
//! │  │  - Chunked storage (shard large tensors)                 │   │
//! │  │  - Compression (per-tensor adaptive compression)         │   │
//! │  │  - Deduplication (hash-based tensor dedup)               │   │
//! │  └──────────────────────────┬───────────────────────────────┘   │
//! └─────────────────────────────┼───────────────────────────────────┘
//!                               │
//! ┌─────────────────────────────▼───────────────────────────────────┐
//! │                        warp-store                                │
//! │                (Object storage backend)                          │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! - **Native Tensor Formats**: Support for safetensors, GGUF, NumPy, and custom formats
//! - **Lazy Loading**: Load only tensor metadata initially, fetch data on-demand
//! - **Chunked Storage**: Shard large tensors across multiple objects
//! - **Incremental Checkpoints**: Save only changed tensors between checkpoints
//! - **Sharded Models**: Distribute model weights across storage nodes
//! - **Tensor Deduplication**: Hash-based dedup for shared weight tensors
//! - **Version Control**: Git-like versioning for model iterations
//!
//! # Example
//!
//! ```ignore
//! use warp_tensor::{TensorStore, TensorFormat, ModelCheckpoint};
//!
//! // Create tensor store
//! let store = TensorStore::new(warp_store, config).await?;
//!
//! // Save a model checkpoint
//! let checkpoint = ModelCheckpoint::builder()
//!     .name("my_model_v1")
//!     .add_tensor("weight", &weight_tensor)
//!     .add_tensor("bias", &bias_tensor)
//!     .build();
//!
//! store.save_checkpoint(&checkpoint).await?;
//!
//! // Load with lazy loading
//! let loaded = store.load_checkpoint("my_model_v1").await?;
//! let weight = loaded.get_tensor::<f32>("weight").await?; // Data fetched on-demand
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod checkpoint;
pub mod config;
pub mod error;
pub mod format;
pub mod model;
pub mod shard;
pub mod store;
pub mod tensor;

pub use checkpoint::{Checkpoint, CheckpointBuilder, CheckpointManager};
pub use config::{TensorConfig, ChunkConfig, CompressionConfig};
pub use error::{TensorError, TensorResult};
pub use format::{TensorFormat, FormatReader, FormatWriter};
pub use model::{ModelStore, ModelVersion, ModelMetadata};
pub use shard::{ShardStrategy, TensorShard, ShardedTensor};
pub use store::{TensorStore, TensorHandle, TensorQuery};
pub use tensor::{TensorData, TensorDtype, TensorMeta, TensorLayout};
