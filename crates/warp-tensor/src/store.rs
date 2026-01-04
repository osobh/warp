//! Main tensor store interface

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::Semaphore;

use warp_store::Store;

use crate::checkpoint::{Checkpoint, CheckpointManager, CheckpointMeta};
use crate::config::{ChunkConfig, TensorConfig};
use crate::error::{TensorError, TensorResult};
use crate::format::{FormatReader, FormatWriter, TensorFormat, WarpNativeReader, WarpNativeWriter};
use crate::model::{ModelMetadata, ModelStore, ModelVersion};
use crate::shard::{ShardStrategy, ShardedTensor, TensorShard, create_sharded_meta, shard_tensor};
use crate::tensor::{LazyTensor, TensorData, TensorMeta};

/// Tensor store - main interface for tensor storage
pub struct TensorStore {
    /// Configuration
    config: TensorConfig,
    /// Storage backend
    store: Arc<Store>,
    /// Checkpoint manager
    checkpoints: CheckpointManager,
    /// Model store
    models: ModelStore,
    /// Metadata cache
    meta_cache: DashMap<String, TensorMeta>,
    /// Data cache
    data_cache: DashMap<String, Bytes>,
    /// Concurrency control
    semaphore: Semaphore,
}

impl TensorStore {
    /// Create a new tensor store
    pub fn new(store: Arc<Store>, config: TensorConfig) -> Self {
        let semaphore = Semaphore::new(config.max_concurrent_ops);

        Self {
            checkpoints: CheckpointManager::new(config.clone()),
            models: ModelStore::new(config.clone()),
            config,
            store,
            meta_cache: DashMap::new(),
            data_cache: DashMap::new(),
            semaphore,
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &TensorConfig {
        &self.config
    }

    /// Get the checkpoint manager
    pub fn checkpoints(&self) -> &CheckpointManager {
        &self.checkpoints
    }

    /// Get the model store
    pub fn models(&self) -> &ModelStore {
        &self.models
    }

    /// Save a checkpoint to storage
    pub async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> TensorResult<()> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| TensorError::ConfigError("semaphore closed".to_string()))?;

        // Serialize tensors
        let writer = WarpNativeWriter::new();
        let tensors: Vec<&TensorData> = checkpoint.tensors().values().collect();
        let tensor_refs: Vec<TensorData> = tensors.iter().map(|t| (*t).clone()).collect();
        let data = writer.write(&tensor_refs)?;

        // Store the checkpoint data
        let key = format!(
            "{}/checkpoints/{}.warp",
            self.config.prefix, checkpoint.meta.name
        );
        self.put_object(&key, data).await?;

        // Store metadata separately for lazy loading
        let meta_key = format!(
            "{}/checkpoints/{}.meta",
            self.config.prefix, checkpoint.meta.name
        );
        let meta_bytes = rmp_serde::to_vec(&checkpoint.meta)
            .map_err(|e| TensorError::Serialization(e.to_string()))?;
        self.put_object(&meta_key, Bytes::from(meta_bytes)).await?;

        // Register with checkpoint manager
        self.checkpoints.register(checkpoint.meta.clone());

        Ok(())
    }

    /// Load a checkpoint from storage
    pub async fn load_checkpoint(&self, name: &str) -> TensorResult<Checkpoint> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| TensorError::ConfigError("semaphore closed".to_string()))?;

        // Load data
        let key = format!("{}/checkpoints/{}.warp", self.config.prefix, name);
        let data = self.get_object(&key).await?;

        // Parse tensors
        let reader = WarpNativeReader::new();
        let tensors = reader.read_all(&data)?;

        // Build checkpoint
        let mut checkpoint = Checkpoint::new(name);
        for tensor in tensors {
            checkpoint.add_tensor(tensor);
        }

        Ok(checkpoint)
    }

    /// Load checkpoint metadata only (for lazy loading)
    pub async fn load_checkpoint_meta(&self, name: &str) -> TensorResult<CheckpointMeta> {
        let meta_key = format!("{}/checkpoints/{}.meta", self.config.prefix, name);
        let data = self.get_object(&meta_key).await?;

        let meta: CheckpointMeta =
            rmp_serde::from_slice(&data).map_err(|e| TensorError::Serialization(e.to_string()))?;

        Ok(meta)
    }

    /// Load a specific tensor from a checkpoint
    pub async fn load_tensor(
        &self,
        checkpoint_name: &str,
        tensor_name: &str,
    ) -> TensorResult<TensorData> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| TensorError::ConfigError("semaphore closed".to_string()))?;

        let key = format!(
            "{}/checkpoints/{}.warp",
            self.config.prefix, checkpoint_name
        );
        let data = self.get_object(&key).await?;

        let reader = WarpNativeReader::new();
        reader.read_tensor(&data, tensor_name)
    }

    /// Save a single tensor
    pub async fn save_tensor(&self, tensor: &TensorData) -> TensorResult<String> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| TensorError::ConfigError("semaphore closed".to_string()))?;

        let key = format!("{}/tensors/{}.warp", self.config.prefix, tensor.name());

        // Check if sharding is needed
        if tensor.size_bytes() > self.config.chunk.chunk_threshold {
            self.save_sharded_tensor(tensor).await
        } else {
            let writer = WarpNativeWriter::new();
            let data = writer.write(&[tensor.clone()])?;
            self.put_object(&key, data).await?;
            Ok(key)
        }
    }

    /// Save a sharded tensor
    async fn save_sharded_tensor(&self, tensor: &TensorData) -> TensorResult<String> {
        let shards = shard_tensor(
            tensor,
            self.config.chunk.chunk_size,
            self.config.chunk.max_chunks,
        )?;

        // Save each shard
        for shard in &shards {
            let shard_key = format!("{}/tensors/{}", self.config.prefix, shard.storage_key());
            self.put_object(&shard_key, shard.data.clone()).await?;
        }

        // Save shard metadata
        let meta = create_sharded_meta(
            tensor,
            ShardStrategy::FixedSize,
            self.config.chunk.chunk_size,
            &shards,
        );
        let meta_key = format!(
            "{}/tensors/{}.shard_meta",
            self.config.prefix,
            tensor.name()
        );
        let meta_bytes =
            rmp_serde::to_vec(&meta).map_err(|e| TensorError::Serialization(e.to_string()))?;
        self.put_object(&meta_key, Bytes::from(meta_bytes)).await?;

        Ok(meta_key)
    }

    /// Load a tensor by name
    pub async fn get_tensor(&self, name: &str) -> TensorResult<TensorData> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| TensorError::ConfigError("semaphore closed".to_string()))?;

        // Check cache first
        if let Some(cached_data) = self.data_cache.get(name) {
            if let Some(cached_meta) = self.meta_cache.get(name) {
                return Ok(TensorData::new(cached_meta.clone(), cached_data.clone()));
            }
        }

        let key = format!("{}/tensors/{}.warp", self.config.prefix, name);
        let data = self.get_object(&key).await?;

        let reader = WarpNativeReader::new();
        let tensors = reader.read_all(&data)?;

        tensors
            .into_iter()
            .next()
            .ok_or_else(|| TensorError::TensorNotFound(name.to_string()))
    }

    /// Delete a tensor
    pub async fn delete_tensor(&self, name: &str) -> TensorResult<()> {
        let key = format!("{}/tensors/{}.warp", self.config.prefix, name);
        self.delete_object(&key).await?;

        // Remove from cache
        self.meta_cache.remove(name);
        self.data_cache.remove(name);

        Ok(())
    }

    /// List checkpoints
    pub fn list_checkpoints(&self) -> Vec<String> {
        self.checkpoints.list()
    }

    /// Delete a checkpoint
    pub async fn delete_checkpoint(&self, name: &str) -> TensorResult<()> {
        let key = format!("{}/checkpoints/{}.warp", self.config.prefix, name);
        self.delete_object(&key).await?;

        let meta_key = format!("{}/checkpoints/{}.meta", self.config.prefix, name);
        let _ = self.delete_object(&meta_key).await;

        self.checkpoints.remove(name);
        Ok(())
    }

    /// Internal: put object to storage
    async fn put_object(&self, key: &str, data: Bytes) -> TensorResult<()> {
        // In a real implementation, this would use the warp-store API
        // For now, we'll simulate it
        Ok(())
    }

    /// Internal: get object from storage
    async fn get_object(&self, key: &str) -> TensorResult<Bytes> {
        // In a real implementation, this would use the warp-store API
        Err(TensorError::TensorNotFound(key.to_string()))
    }

    /// Internal: delete object from storage
    async fn delete_object(&self, key: &str) -> TensorResult<()> {
        Ok(())
    }
}

/// Handle to a tensor (may be lazy)
pub struct TensorHandle {
    /// Tensor metadata
    pub meta: TensorMeta,
    /// Whether data is loaded
    loaded: bool,
    /// Storage key
    storage_key: String,
    /// Store reference
    store: Arc<TensorStore>,
}

impl TensorHandle {
    /// Create a new tensor handle
    pub fn new(meta: TensorMeta, storage_key: String, store: Arc<TensorStore>) -> Self {
        Self {
            meta,
            loaded: false,
            storage_key,
            store,
        }
    }

    /// Check if data is loaded
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Get tensor name
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    /// Get tensor shape
    pub fn shape(&self) -> &[usize] {
        &self.meta.shape
    }
}

/// Query builder for finding tensors
pub struct TensorQuery {
    /// Name pattern (glob-style)
    name_pattern: Option<String>,
    /// Dtype filter
    dtype: Option<crate::tensor::TensorDtype>,
    /// Minimum size
    min_size: Option<u64>,
    /// Maximum size
    max_size: Option<u64>,
}

impl TensorQuery {
    /// Create a new query
    pub fn new() -> Self {
        Self {
            name_pattern: None,
            dtype: None,
            min_size: None,
            max_size: None,
        }
    }

    /// Filter by name pattern
    pub fn name(mut self, pattern: impl Into<String>) -> Self {
        self.name_pattern = Some(pattern.into());
        self
    }

    /// Filter by dtype
    pub fn dtype(mut self, dtype: crate::tensor::TensorDtype) -> Self {
        self.dtype = Some(dtype);
        self
    }

    /// Filter by minimum size
    pub fn min_size(mut self, size: u64) -> Self {
        self.min_size = Some(size);
        self
    }

    /// Filter by maximum size
    pub fn max_size(mut self, size: u64) -> Self {
        self.max_size = Some(size);
        self
    }
}

impl Default for TensorQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp_store::StoreConfig;

    async fn create_test_store() -> TensorStore {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_config = StoreConfig {
            root_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let store = Arc::new(Store::new(store_config).await.unwrap());
        let config = TensorConfig::default();
        TensorStore::new(store, config)
    }

    #[tokio::test]
    async fn test_tensor_store_creation() {
        let store = create_test_store().await;
        assert_eq!(store.config().bucket, "__warp_tensor__");
    }

    #[tokio::test]
    async fn test_checkpoint_operations() {
        let store = create_test_store().await;

        // Build a checkpoint
        let checkpoint = Checkpoint::builder("test_v1")
            .add_f32("weight", vec![10, 20], &vec![0.0; 200])
            .add_f32("bias", vec![20], &vec![0.0; 20])
            .metadata("epoch", "1")
            .build();

        assert_eq!(checkpoint.tensor_count(), 2);
        assert_eq!(checkpoint.meta.name, "test_v1");

        // Note: actual storage operations are stubbed
    }

    #[tokio::test]
    async fn test_model_registration() {
        let store = create_test_store().await;

        let meta = ModelMetadata::builder("my_model")
            .architecture("mlp")
            .framework("pytorch", "2.0")
            .build();

        store.models().register_model(meta);
        assert!(store.models().model_exists("my_model"));
    }

    #[test]
    fn test_tensor_query_builder() {
        let query = TensorQuery::new()
            .name("layer*.weight")
            .dtype(crate::tensor::TensorDtype::Float32)
            .min_size(1024)
            .max_size(1024 * 1024);

        assert_eq!(query.name_pattern, Some("layer*.weight".to_string()));
        assert_eq!(query.dtype, Some(crate::tensor::TensorDtype::Float32));
        assert_eq!(query.min_size, Some(1024));
        assert_eq!(query.max_size, Some(1024 * 1024));
    }
}
