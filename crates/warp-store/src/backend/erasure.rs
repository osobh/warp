//! Erasure coding storage backend
//!
//! Provides fault-tolerant object storage using Reed-Solomon erasure coding.
//! Objects are split into data shards and parity shards, allowing recovery
//! from node failures.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Erasure Backend                                 │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                      │
//! │   Original Object (10KB)                                             │
//! │   ├─ Split into data shards: 10 × 1KB                               │
//! │   └─ Generate parity shards: 4 × 1KB                                │
//! │                                                                      │
//! │   ┌────────────────────────────────────────────────────────────┐    │
//! │   │                    Shard Distribution                       │    │
//! │   │                                                             │    │
//! │   │  Node 0   Node 1   Node 2   Node 3   Node 4   ...          │    │
//! │   │  ┌───┐    ┌───┐    ┌───┐    ┌───┐    ┌───┐                 │    │
//! │   │  │D0 │    │D1 │    │D2 │    │D3 │    │D4 │  ...            │    │
//! │   │  └───┘    └───┘    └───┘    └───┘    └───┘                 │    │
//! │   │                                                             │    │
//! │   │  Recovery: Any 10 of 14 shards → Original data             │    │
//! │   └─────────────────────────────────────────────────────────────┘   │
//! │                                                                      │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use warp_store::backend::{ErasureBackend, ErasureConfig as StoreErasureConfig};
//!
//! // Create backend with RS(10,4) - 40% overhead, 4 failure tolerance
//! let config = StoreErasureConfig::rs_10_4();
//! let backend = ErasureBackend::new("/data/erasure", config).await?;
//!
//! // Store object - automatically encoded to shards
//! let key = ObjectKey::new("bucket", "large-file.bin")?;
//! backend.put(&key, data, PutOptions::default()).await?;
//!
//! // Get object - automatically reconstructed from shards
//! let data = backend.get(&key).await?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use warp_ec::{ErasureConfig, ErasureDecoder, ErasureEncoder};

use crate::backend::{HpcStorageBackend, StorageBackend, StorageProof};
use crate::error::{Error, Result};
use crate::key::ObjectKey;
use crate::object::{FieldData, ListOptions, ObjectData, ObjectEntry, ObjectList, ObjectMeta, PutOptions, StorageClass};

/// Erasure coding configuration for storage
#[derive(Debug, Clone)]
pub struct StoreErasureConfig {
    /// Underlying erasure config
    inner: ErasureConfig,
    /// Minimum shards required to attempt reconstruction (optimization)
    min_shards_for_read: usize,
}

impl StoreErasureConfig {
    /// Create a new erasure config
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        let inner = ErasureConfig::new(data_shards, parity_shards)
            .map_err(|e| Error::Backend(format!("invalid erasure config: {}", e)))?;
        Ok(Self {
            min_shards_for_read: data_shards,
            inner,
        })
    }

    /// RS(4,2) - 50% overhead, tolerates 2 failures
    pub fn rs_4_2() -> Self {
        Self {
            inner: ErasureConfig::rs_4_2(),
            min_shards_for_read: 4,
        }
    }

    /// RS(6,3) - 50% overhead, tolerates 3 failures
    pub fn rs_6_3() -> Self {
        Self {
            inner: ErasureConfig::rs_6_3(),
            min_shards_for_read: 6,
        }
    }

    /// RS(10,4) - 40% overhead, tolerates 4 failures
    pub fn rs_10_4() -> Self {
        Self {
            inner: ErasureConfig::rs_10_4(),
            min_shards_for_read: 10,
        }
    }

    /// RS(16,4) - 25% overhead, tolerates 4 failures
    pub fn rs_16_4() -> Self {
        Self {
            inner: ErasureConfig::rs_16_4(),
            min_shards_for_read: 16,
        }
    }

    /// Get data shards count
    pub fn data_shards(&self) -> usize {
        self.inner.data_shards()
    }

    /// Get parity shards count
    pub fn parity_shards(&self) -> usize {
        self.inner.parity_shards()
    }

    /// Get total shards count
    pub fn total_shards(&self) -> usize {
        self.inner.total_shards()
    }

    /// Get fault tolerance (max failures)
    pub fn fault_tolerance(&self) -> usize {
        self.inner.fault_tolerance()
    }
}

impl Default for StoreErasureConfig {
    fn default() -> Self {
        Self::rs_10_4()
    }
}

/// Public metadata for encoded shards (used by distributed backend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedShardMeta {
    /// Original data size
    pub original_size: u64,
    /// Padded size (for even shard division)
    pub padded_size: u64,
    /// Shard size
    pub shard_size: usize,
    /// Number of data shards
    pub data_shards: usize,
    /// Number of parity shards
    pub parity_shards: usize,
    /// Content hash of original data
    pub content_hash: [u8; 32],
    /// Content type
    pub content_type: Option<String>,
}

/// Internal metadata for an erasure-coded object
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ObjectShardMeta {
    /// Original data size
    original_size: u64,
    /// Padded size (for even shard division)
    padded_size: u64,
    /// Shard size
    shard_size: usize,
    /// Number of data shards
    data_shards: usize,
    /// Number of parity shards
    parity_shards: usize,
    /// Content hash of original data
    content_hash: [u8; 32],
    /// Content type
    content_type: Option<String>,
    /// Creation time
    created_at: chrono::DateTime<Utc>,
}

/// Erasure coding storage backend
///
/// Stores objects using Reed-Solomon erasure coding for fault tolerance.
pub struct ErasureBackend {
    /// Root storage path
    root: PathBuf,
    /// Erasure configuration
    config: StoreErasureConfig,
    /// Encoder instance
    encoder: ErasureEncoder,
    /// Decoder instance
    decoder: ErasureDecoder,
    /// Bucket registry
    buckets: DashMap<String, ()>,
}

impl ErasureBackend {
    /// Create a new erasure backend
    pub async fn new(root: impl AsRef<Path>, config: StoreErasureConfig) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&root).await?;

        let encoder = ErasureEncoder::new(config.inner.clone());
        let decoder = ErasureDecoder::new(config.inner.clone());

        debug!(
            root = %root.display(),
            data_shards = config.data_shards(),
            parity_shards = config.parity_shards(),
            "Initializing erasure backend"
        );

        Ok(Self {
            root,
            config,
            encoder,
            decoder,
            buckets: DashMap::new(),
        })
    }

    /// Path for a bucket
    fn bucket_path(&self, bucket: &str) -> PathBuf {
        self.root.join(bucket)
    }

    /// Path for object metadata
    fn meta_path(&self, key: &ObjectKey) -> PathBuf {
        self.bucket_path(key.bucket()).join(format!("{}.meta", key.key()))
    }

    /// Path for a shard
    fn shard_path(&self, key: &ObjectKey, shard_index: usize) -> PathBuf {
        self.bucket_path(key.bucket()).join(format!("{}.shard.{}", key.key(), shard_index))
    }

    /// Read shard metadata
    async fn read_meta(&self, key: &ObjectKey) -> Result<ObjectShardMeta> {
        let path = self.meta_path(key);
        let data = tokio::fs::read(&path).await?;
        let meta: ObjectShardMeta = rmp_serde::from_slice(&data)?;
        Ok(meta)
    }

    /// Write shard metadata
    async fn write_meta(&self, key: &ObjectKey, meta: &ObjectShardMeta) -> Result<()> {
        let path = self.meta_path(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = rmp_serde::to_vec(meta)?;
        tokio::fs::write(&path, &data).await?;
        Ok(())
    }

    /// Read a shard from storage
    async fn read_shard(&self, key: &ObjectKey, shard_index: usize) -> Result<Option<Vec<u8>>> {
        let path = self.shard_path(key, shard_index);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write a shard to storage
    async fn write_shard(&self, key: &ObjectKey, shard_index: usize, data: &[u8]) -> Result<()> {
        let path = self.shard_path(key, shard_index);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    /// Delete all shards for an object
    async fn delete_shards(&self, key: &ObjectKey, total_shards: usize) -> Result<()> {
        for i in 0..total_shards {
            let path = self.shard_path(key, i);
            if path.exists() {
                tokio::fs::remove_file(&path).await?;
            }
        }
        // Delete metadata
        let meta_path = self.meta_path(key);
        if meta_path.exists() {
            tokio::fs::remove_file(&meta_path).await?;
        }
        Ok(())
    }

    /// Get the erasure configuration
    pub fn config(&self) -> &StoreErasureConfig {
        &self.config
    }

    // =========================================================================
    // Shard-Level Operations (for distributed storage)
    // =========================================================================

    /// Get a single shard by index
    ///
    /// Used by distributed backend to fetch individual shards from remote domains.
    pub async fn get_shard(&self, key: &ObjectKey, shard_index: usize) -> Result<Option<Vec<u8>>> {
        self.read_shard(key, shard_index).await
    }

    /// Put a single shard by index
    ///
    /// Used by distributed backend to store shards received from remote domains.
    pub async fn put_shard(&self, key: &ObjectKey, shard_index: usize, data: &[u8]) -> Result<()> {
        self.write_shard(key, shard_index, data).await
    }

    /// Encode data into shards without storing
    ///
    /// Returns (shards, metadata) for distributed placement.
    pub fn encode_to_shards(&self, data: &[u8], content_type: Option<String>) -> Result<(Vec<Vec<u8>>, EncodedShardMeta)> {
        let original_size = data.len();
        let content_hash = *blake3::hash(data).as_bytes();

        // Pad data for even shard division
        let padded_size = self.config.inner.padded_data_size(original_size);
        let mut padded = data.to_vec();
        padded.resize(padded_size, 0);

        // Encode to shards
        let shards = self.encoder.encode(&padded)
            .map_err(|e| Error::ErasureCoding(format!("encode failed: {}", e)))?;

        let meta = EncodedShardMeta {
            original_size: original_size as u64,
            padded_size: padded_size as u64,
            shard_size: shards[0].len(),
            data_shards: self.config.data_shards(),
            parity_shards: self.config.parity_shards(),
            content_hash,
            content_type,
        };

        Ok((shards, meta))
    }

    /// Decode shards back to original data
    ///
    /// Takes a sparse array of shards (Some for available, None for missing).
    /// Needs at least `data_shards` available to reconstruct.
    pub fn decode_from_shards(&self, shards: &[Option<Vec<u8>>], original_size: u64) -> Result<Vec<u8>> {
        // Count available shards
        let available = shards.iter().filter(|s| s.is_some()).count();
        if available < self.config.data_shards() {
            return Err(Error::ErasureCoding(format!(
                "insufficient shards: need {}, have {}",
                self.config.data_shards(),
                available
            )));
        }

        // Decode
        let padded = self.decoder.decode(shards)
            .map_err(|e| Error::ErasureCoding(format!("decode failed: {}", e)))?;

        // Trim padding
        Ok(padded[..original_size as usize].to_vec())
    }

    /// Store shard metadata
    pub async fn store_shard_meta(&self, key: &ObjectKey, meta: &EncodedShardMeta) -> Result<()> {
        let shard_meta = ObjectShardMeta {
            original_size: meta.original_size,
            padded_size: meta.padded_size,
            shard_size: meta.shard_size,
            data_shards: meta.data_shards,
            parity_shards: meta.parity_shards,
            content_hash: meta.content_hash,
            content_type: meta.content_type.clone(),
            created_at: Utc::now(),
        };
        self.write_meta(key, &shard_meta).await
    }

    /// Get shard metadata
    pub async fn get_shard_meta(&self, key: &ObjectKey) -> Result<EncodedShardMeta> {
        let meta = self.read_meta(key).await?;
        Ok(EncodedShardMeta {
            original_size: meta.original_size,
            padded_size: meta.padded_size,
            shard_size: meta.shard_size,
            data_shards: meta.data_shards,
            parity_shards: meta.parity_shards,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    /// Get shard health status for an object
    pub async fn shard_health(&self, key: &ObjectKey) -> Result<ShardHealth> {
        let meta = self.read_meta(key).await?;
        let total = meta.data_shards + meta.parity_shards;

        let mut available = 0;
        let mut missing = Vec::new();

        for i in 0..total {
            let path = self.shard_path(key, i);
            if path.exists() {
                available += 1;
            } else {
                missing.push(i);
            }
        }

        Ok(ShardHealth {
            total_shards: total,
            available_shards: available,
            missing_shards: missing,
            can_recover: available >= meta.data_shards,
        })
    }

    /// Repair missing shards by reconstructing from available ones
    pub async fn repair_shards(&self, key: &ObjectKey) -> Result<usize> {
        let health = self.shard_health(key).await?;

        if health.missing_shards.is_empty() {
            return Ok(0); // Nothing to repair
        }

        if !health.can_recover {
            return Err(Error::Backend(format!(
                "cannot repair: need {} shards, only {} available",
                self.config.data_shards(),
                health.available_shards
            )));
        }

        // Read available shards
        let meta = self.read_meta(key).await?;
        let total = meta.data_shards + meta.parity_shards;
        let mut shards: Vec<Option<Vec<u8>>> = vec![None; total];

        for i in 0..total {
            shards[i] = self.read_shard(key, i).await?;
        }

        // Decode to get original data
        let original = self.decoder.decode(&shards)
            .map_err(|e| Error::Backend(format!("decode failed: {}", e)))?;

        // Re-encode to regenerate all shards
        let new_shards = self.encoder.encode(&original)
            .map_err(|e| Error::Backend(format!("encode failed: {}", e)))?;

        // Write missing shards
        let mut repaired = 0;
        for &missing_idx in &health.missing_shards {
            self.write_shard(key, missing_idx, &new_shards[missing_idx]).await?;
            repaired += 1;
        }

        debug!(
            key = %key,
            repaired = repaired,
            "Repaired missing shards"
        );

        Ok(repaired)
    }
}

/// Health status of shards for an object
#[derive(Debug, Clone)]
pub struct ShardHealth {
    /// Total number of shards
    pub total_shards: usize,
    /// Number of available shards
    pub available_shards: usize,
    /// Indices of missing shards
    pub missing_shards: Vec<usize>,
    /// Whether the object can be recovered
    pub can_recover: bool,
}

#[async_trait]
impl StorageBackend for ErasureBackend {
    async fn get(&self, key: &ObjectKey) -> Result<ObjectData> {
        // Read metadata
        let meta = self.read_meta(key).await.map_err(|_| Error::ObjectNotFound {
            bucket: key.bucket().to_string(),
            key: key.key().to_string(),
        })?;

        let total = meta.data_shards + meta.parity_shards;

        // Read all available shards
        let mut shards: Vec<Option<Vec<u8>>> = Vec::with_capacity(total);
        let mut available = 0;

        for i in 0..total {
            let shard = self.read_shard(key, i).await?;
            if shard.is_some() {
                available += 1;
            }
            shards.push(shard);
        }

        if available < meta.data_shards {
            return Err(Error::Backend(format!(
                "insufficient shards: need {}, have {}",
                meta.data_shards, available
            )));
        }

        // Decode
        let padded = self.decoder.decode(&shards)
            .map_err(|e| Error::Backend(format!("decode failed: {}", e)))?;

        // Trim padding
        let original = padded[..meta.original_size as usize].to_vec();

        trace!(
            key = %key,
            size = original.len(),
            shards_used = available,
            "Retrieved erasure-coded object"
        );

        Ok(ObjectData::from(original))
    }

    async fn get_fields(&self, _key: &ObjectKey, _fields: &[&str]) -> Result<FieldData> {
        // Erasure backend doesn't support field-level access
        Ok(FieldData::new())
    }

    async fn put(&self, key: &ObjectKey, data: ObjectData, opts: PutOptions) -> Result<ObjectMeta> {
        let original_size = data.len();
        let content_hash = *blake3::hash(data.as_ref()).as_bytes();

        // Pad data for even shard division
        let padded_size = self.config.inner.padded_data_size(original_size);
        let mut padded = data.as_ref().to_vec();
        padded.resize(padded_size, 0);

        // Encode to shards
        let shards = self.encoder.encode(&padded)
            .map_err(|e| Error::Backend(format!("encode failed: {}", e)))?;

        // Write all shards
        for (i, shard) in shards.iter().enumerate() {
            self.write_shard(key, i, shard).await?;
        }

        // Write metadata
        let meta = ObjectShardMeta {
            original_size: original_size as u64,
            padded_size: padded_size as u64,
            shard_size: shards[0].len(),
            data_shards: self.config.data_shards(),
            parity_shards: self.config.parity_shards(),
            content_hash,
            content_type: opts.content_type.clone(),
            created_at: Utc::now(),
        };
        self.write_meta(key, &meta).await?;

        debug!(
            key = %key,
            original_size,
            shards = shards.len(),
            shard_size = shards[0].len(),
            "Stored erasure-coded object"
        );

        Ok(ObjectMeta {
            size: original_size as u64,
            content_hash,
            etag: format!("\"{}\"", hex::encode(&content_hash[..16])),
            content_type: opts.content_type,
            created_at: meta.created_at,
            modified_at: meta.created_at,
            version_id: None,
            user_metadata: opts.metadata,
            is_delete_marker: false,
        })
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        let meta = self.read_meta(key).await?;
        let total = meta.data_shards + meta.parity_shards;
        self.delete_shards(key, total).await?;
        debug!(key = %key, "Deleted erasure-coded object");
        Ok(())
    }

    async fn list(&self, bucket: &str, prefix: &str, opts: ListOptions) -> Result<ObjectList> {
        let bucket_path = self.bucket_path(bucket);

        if !bucket_path.exists() {
            return Err(Error::BucketNotFound(bucket.to_string()));
        }

        let mut objects = Vec::new();
        let mut common_prefixes = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&bucket_path).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Only process .meta files
            if !file_name.ends_with(".meta") {
                continue;
            }

            let key = file_name.strip_suffix(".meta").unwrap();

            if !key.starts_with(prefix) {
                continue;
            }

            // Handle delimiter for common prefixes
            if let Some(ref delimiter) = opts.delimiter {
                let suffix = &key[prefix.len()..];
                if let Some(pos) = suffix.find(delimiter.as_str()) {
                    let common = format!("{}{}{}", prefix, &suffix[..pos], delimiter);
                    if !common_prefixes.contains(&common) {
                        common_prefixes.push(common);
                    }
                    continue;
                }
            }

            // Handle start_after
            if let Some(ref start) = opts.start_after {
                if key <= start.as_str() {
                    continue;
                }
            }

            // Read metadata
            let obj_key = ObjectKey::new(bucket, key)?;
            if let Ok(meta) = self.read_meta(&obj_key).await {
                objects.push(ObjectEntry {
                    key: key.to_string(),
                    size: meta.original_size,
                    last_modified: meta.created_at,
                    etag: format!("\"{}\"", hex::encode(&meta.content_hash[..16])),
                    storage_class: StorageClass::Standard,
                    version_id: None,
                    is_latest: true,
                });
            }

            if objects.len() >= opts.max_keys {
                break;
            }
        }

        objects.sort_by(|a, b| a.key.cmp(&b.key));
        let key_count = objects.len();

        Ok(ObjectList {
            objects,
            common_prefixes,
            is_truncated: false,
            next_continuation_token: None,
            key_count,
        })
    }

    async fn head(&self, key: &ObjectKey) -> Result<ObjectMeta> {
        let meta = self.read_meta(key).await.map_err(|_| Error::ObjectNotFound {
            bucket: key.bucket().to_string(),
            key: key.key().to_string(),
        })?;

        Ok(ObjectMeta {
            size: meta.original_size,
            content_hash: meta.content_hash,
            etag: format!("\"{}\"", hex::encode(&meta.content_hash[..16])),
            content_type: meta.content_type,
            created_at: meta.created_at,
            modified_at: meta.created_at,
            version_id: None,
            user_metadata: HashMap::new(),
            is_delete_marker: false,
        })
    }

    async fn create_bucket(&self, name: &str) -> Result<()> {
        let path = self.bucket_path(name);
        tokio::fs::create_dir_all(&path).await?;
        self.buckets.insert(name.to_string(), ());
        debug!(bucket = name, "Created bucket");
        Ok(())
    }

    async fn delete_bucket(&self, name: &str) -> Result<()> {
        let path = self.bucket_path(name);
        tokio::fs::remove_dir_all(&path).await?;
        self.buckets.remove(name);
        debug!(bucket = name, "Deleted bucket");
        Ok(())
    }

    async fn bucket_exists(&self, name: &str) -> Result<bool> {
        Ok(self.bucket_path(name).exists())
    }
}

#[async_trait]
impl HpcStorageBackend for ErasureBackend {
    async fn verified_get(&self, key: &ObjectKey) -> Result<(ObjectData, StorageProof)> {
        let data = self.get(key).await?;
        let meta = self.read_meta(key).await?;

        // Use content hash from metadata as proof root
        let proof = StorageProof {
            root: meta.content_hash,
            path: vec![],
            leaf_index: 0,
        };

        Ok((data, proof))
    }

    #[cfg(feature = "gpu")]
    async fn pinned_store(
        &self,
        key: &ObjectKey,
        gpu_buffer: &warp_gpu::GpuBuffer<u8>,
    ) -> Result<ObjectMeta> {
        // Copy from GPU to CPU then store with erasure coding
        let buffer = gpu_buffer.copy_to_host()
            .map_err(|e| Error::Backend(format!("GPU copy failed: {}", e)))?;

        let data = ObjectData::from(buffer);
        self.put(key, data, PutOptions::default()).await
    }
}

// Hex encoding helper
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_erasure_backend_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = StoreErasureConfig::rs_4_2();
        let backend = ErasureBackend::new(temp_dir.path(), config).await.unwrap();

        // Create bucket
        backend.create_bucket("test").await.unwrap();
        assert!(backend.bucket_exists("test").await.unwrap());

        // Put object
        let key = ObjectKey::new("test", "hello.txt").unwrap();
        let data = b"Hello, erasure coding!".to_vec();
        let meta = backend.put(&key, ObjectData::from(data.clone()), PutOptions::default()).await.unwrap();

        assert_eq!(meta.size, data.len() as u64);

        // Get object
        let retrieved = backend.get(&key).await.unwrap();
        assert_eq!(retrieved.as_ref(), data.as_slice());

        // Head
        let head_meta = backend.head(&key).await.unwrap();
        assert_eq!(head_meta.size, data.len() as u64);

        // Delete
        backend.delete(&key).await.unwrap();
        assert!(backend.get(&key).await.is_err());
    }

    #[tokio::test]
    async fn test_erasure_recovery() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = StoreErasureConfig::rs_4_2();
        let backend = ErasureBackend::new(temp_dir.path(), config).await.unwrap();

        backend.create_bucket("test").await.unwrap();

        // Put object (data must be divisible by 4 for RS(4,2))
        let key = ObjectKey::new("test", "recoverable.bin").unwrap();
        let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        backend.put(&key, ObjectData::from(data.clone()), PutOptions::default()).await.unwrap();

        // Delete 2 shards (max we can lose with RS(4,2))
        let shard0_path = backend.shard_path(&key, 0);
        let shard3_path = backend.shard_path(&key, 3);
        tokio::fs::remove_file(&shard0_path).await.unwrap();
        tokio::fs::remove_file(&shard3_path).await.unwrap();

        // Should still be able to recover
        let retrieved = backend.get(&key).await.unwrap();
        assert_eq!(retrieved.as_ref(), data.as_slice());
    }

    #[tokio::test]
    async fn test_shard_health() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = StoreErasureConfig::rs_4_2();
        let backend = ErasureBackend::new(temp_dir.path(), config).await.unwrap();

        backend.create_bucket("test").await.unwrap();

        let key = ObjectKey::new("test", "health.bin").unwrap();
        let data: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        backend.put(&key, ObjectData::from(data), PutOptions::default()).await.unwrap();

        // Check health - all shards present
        let health = backend.shard_health(&key).await.unwrap();
        assert_eq!(health.total_shards, 6); // 4 data + 2 parity
        assert_eq!(health.available_shards, 6);
        assert!(health.missing_shards.is_empty());
        assert!(health.can_recover);

        // Delete one shard
        tokio::fs::remove_file(backend.shard_path(&key, 2)).await.unwrap();

        let health = backend.shard_health(&key).await.unwrap();
        assert_eq!(health.available_shards, 5);
        assert_eq!(health.missing_shards, vec![2]);
        assert!(health.can_recover);
    }

    #[tokio::test]
    async fn test_shard_repair() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = StoreErasureConfig::rs_4_2();
        let backend = ErasureBackend::new(temp_dir.path(), config).await.unwrap();

        backend.create_bucket("test").await.unwrap();

        let key = ObjectKey::new("test", "repair.bin").unwrap();
        let data: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        backend.put(&key, ObjectData::from(data.clone()), PutOptions::default()).await.unwrap();

        // Delete shards
        tokio::fs::remove_file(backend.shard_path(&key, 1)).await.unwrap();
        tokio::fs::remove_file(backend.shard_path(&key, 4)).await.unwrap();

        // Repair
        let repaired = backend.repair_shards(&key).await.unwrap();
        assert_eq!(repaired, 2);

        // Verify all shards restored
        let health = backend.shard_health(&key).await.unwrap();
        assert_eq!(health.available_shards, 6);
        assert!(health.missing_shards.is_empty());

        // Verify data integrity
        let retrieved = backend.get(&key).await.unwrap();
        assert_eq!(retrieved.as_ref(), data.as_slice());
    }

    #[tokio::test]
    async fn test_too_many_failures() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = StoreErasureConfig::rs_4_2();
        let backend = ErasureBackend::new(temp_dir.path(), config).await.unwrap();

        backend.create_bucket("test").await.unwrap();

        let key = ObjectKey::new("test", "unrecoverable.bin").unwrap();
        let data: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        backend.put(&key, ObjectData::from(data), PutOptions::default()).await.unwrap();

        // Delete 3 shards (more than parity count of 2)
        tokio::fs::remove_file(backend.shard_path(&key, 0)).await.unwrap();
        tokio::fs::remove_file(backend.shard_path(&key, 1)).await.unwrap();
        tokio::fs::remove_file(backend.shard_path(&key, 2)).await.unwrap();

        // Should fail to recover
        let result = backend.get(&key).await;
        assert!(result.is_err());

        let health = backend.shard_health(&key).await.unwrap();
        assert!(!health.can_recover);
    }

    #[test]
    fn test_config_presets() {
        let c1 = StoreErasureConfig::rs_4_2();
        assert_eq!(c1.data_shards(), 4);
        assert_eq!(c1.parity_shards(), 2);
        assert_eq!(c1.fault_tolerance(), 2);

        let c2 = StoreErasureConfig::rs_10_4();
        assert_eq!(c2.data_shards(), 10);
        assert_eq!(c2.parity_shards(), 4);
        assert_eq!(c2.total_shards(), 14);
    }
}
