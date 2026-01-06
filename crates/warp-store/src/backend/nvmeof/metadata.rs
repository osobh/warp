//! Metadata Storage for NVMe-oF Backend
//!
//! Stores object metadata (key -> location mapping).

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, trace};

use super::config::MetadataBackendConfig;
use super::error::{NvmeOfBackendError, NvmeOfBackendResult};
use super::mapper::ObjectLocation;

use crate::key::ObjectKey;
use crate::object::{ObjectMeta, StorageClass};

/// Bucket metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketMetadata {
    /// Bucket name
    pub name: String,

    /// Creation timestamp
    pub created_at: u64,

    /// Object count
    pub object_count: u64,

    /// Total size in bytes
    pub total_size: u64,
}

/// Object metadata entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadataEntry {
    /// Object key
    pub key: ObjectKey,

    /// Location in NVMe storage
    pub location: ObjectLocation,

    /// Object metadata
    pub meta: ObjectMeta,
}

/// Metadata store for object locations
pub struct MetadataStore {
    /// Configuration
    config: MetadataBackendConfig,

    /// Bucket metadata
    buckets: RwLock<HashMap<String, BucketMetadata>>,

    /// Object metadata (bucket/key -> entry)
    objects: RwLock<HashMap<String, ObjectMetadataEntry>>,
}

impl MetadataStore {
    /// Create a new metadata store
    pub fn new(config: MetadataBackendConfig) -> NvmeOfBackendResult<Self> {
        debug!("Creating metadata store with config: {:?}", config);

        Ok(Self {
            config,
            buckets: RwLock::new(HashMap::new()),
            objects: RwLock::new(HashMap::new()),
        })
    }

    /// Generate key for object lookup
    fn object_key(bucket: &str, key: &str) -> String {
        format!("{}/{}", bucket, key)
    }

    // ======== Bucket Operations ========

    /// Create a bucket
    pub fn create_bucket(&self, name: &str) -> NvmeOfBackendResult<()> {
        let mut buckets = self.buckets.write();

        if buckets.contains_key(name) {
            return Err(NvmeOfBackendError::Metadata(format!(
                "Bucket {} already exists",
                name
            )));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        buckets.insert(
            name.to_string(),
            BucketMetadata {
                name: name.to_string(),
                created_at: now,
                object_count: 0,
                total_size: 0,
            },
        );

        debug!("Created bucket: {}", name);
        Ok(())
    }

    /// Delete a bucket
    pub fn delete_bucket(&self, name: &str) -> NvmeOfBackendResult<()> {
        let mut buckets = self.buckets.write();

        let bucket = buckets
            .get(name)
            .ok_or_else(|| NvmeOfBackendError::BucketNotFound(name.to_string()))?;

        if bucket.object_count > 0 {
            return Err(NvmeOfBackendError::Metadata(format!(
                "Bucket {} is not empty",
                name
            )));
        }

        buckets.remove(name);
        debug!("Deleted bucket: {}", name);
        Ok(())
    }

    /// Check if bucket exists
    pub fn bucket_exists(&self, name: &str) -> bool {
        self.buckets.read().contains_key(name)
    }

    /// Get bucket metadata
    pub fn get_bucket(&self, name: &str) -> Option<BucketMetadata> {
        self.buckets.read().get(name).cloned()
    }

    /// List all buckets
    pub fn list_buckets(&self) -> Vec<BucketMetadata> {
        self.buckets.read().values().cloned().collect()
    }

    // ======== Object Operations ========

    /// Put object metadata
    pub fn put_object(
        &self,
        key: &ObjectKey,
        location: ObjectLocation,
        meta: ObjectMeta,
    ) -> NvmeOfBackendResult<()> {
        let lookup_key = Self::object_key(key.bucket(), key.key());

        // Update bucket stats
        {
            let mut buckets = self.buckets.write();
            if let Some(bucket) = buckets.get_mut(key.bucket()) {
                // Check if updating existing object
                let existing_size = self
                    .objects
                    .read()
                    .get(&lookup_key)
                    .map(|e| e.location.size)
                    .unwrap_or(0);

                if existing_size == 0 {
                    bucket.object_count += 1;
                }
                bucket.total_size = bucket.total_size - existing_size + location.size;
            } else {
                return Err(NvmeOfBackendError::BucketNotFound(key.bucket().to_string()));
            }
        }

        // Store object metadata
        let entry = ObjectMetadataEntry {
            key: key.clone(),
            location,
            meta,
        };

        self.objects.write().insert(lookup_key.clone(), entry);

        trace!("Put object metadata: {}", lookup_key);
        Ok(())
    }

    /// Get object metadata
    pub fn get_object(&self, key: &ObjectKey) -> Option<ObjectMetadataEntry> {
        let lookup_key = Self::object_key(key.bucket(), key.key());
        self.objects.read().get(&lookup_key).cloned()
    }

    /// Delete object metadata
    pub fn delete_object(&self, key: &ObjectKey) -> NvmeOfBackendResult<ObjectMetadataEntry> {
        let lookup_key = Self::object_key(key.bucket(), key.key());

        let entry = self
            .objects
            .write()
            .remove(&lookup_key)
            .ok_or_else(|| NvmeOfBackendError::ObjectNotFound(lookup_key.clone()))?;

        // Update bucket stats
        {
            let mut buckets = self.buckets.write();
            if let Some(bucket) = buckets.get_mut(key.bucket()) {
                bucket.object_count = bucket.object_count.saturating_sub(1);
                bucket.total_size = bucket.total_size.saturating_sub(entry.location.size);
            }
        }

        trace!("Deleted object metadata: {}", lookup_key);
        Ok(entry)
    }

    /// Check if object exists
    pub fn object_exists(&self, key: &ObjectKey) -> bool {
        let lookup_key = Self::object_key(key.bucket(), key.key());
        self.objects.read().contains_key(&lookup_key)
    }

    /// List objects in a bucket with prefix
    pub fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        max_keys: usize,
        continuation_token: Option<&str>,
    ) -> (Vec<ObjectMetadataEntry>, Option<String>) {
        let prefix_key = Self::object_key(bucket, prefix);
        let objects = self.objects.read();

        let mut results: Vec<_> = objects
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix_key))
            .map(|(_, v)| v.clone())
            .collect();

        // Sort by key
        results.sort_by(|a, b| a.key.key().cmp(b.key.key()));

        // Apply continuation token
        if let Some(token) = continuation_token {
            if let Some(pos) = results.iter().position(|e| e.key.key() == token) {
                results = results.split_off(pos + 1);
            }
        }

        // Apply limit
        let next_token = if results.len() > max_keys {
            let last = results[max_keys - 1].key.key().to_string();
            results.truncate(max_keys);
            Some(last)
        } else {
            None
        };

        (results, next_token)
    }

    /// Get statistics
    pub fn stats(&self) -> MetadataStats {
        let buckets = self.buckets.read();
        let objects = self.objects.read();

        MetadataStats {
            bucket_count: buckets.len(),
            object_count: objects.len(),
            total_size: buckets.values().map(|b| b.total_size).sum(),
        }
    }
}

/// Metadata store statistics
#[derive(Debug, Clone, Default)]
pub struct MetadataStats {
    /// Number of buckets
    pub bucket_count: usize,

    /// Number of objects
    pub object_count: usize,

    /// Total data size
    pub total_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::ObjectKey;
    use crate::object::ObjectMeta;
    use std::collections::HashMap;

    fn test_config() -> MetadataBackendConfig {
        MetadataBackendConfig::default()
    }

    #[test]
    fn test_bucket_operations() {
        let store = MetadataStore::new(test_config()).unwrap();

        // Create bucket
        store.create_bucket("test-bucket").unwrap();
        assert!(store.bucket_exists("test-bucket"));

        // List buckets
        let buckets = store.list_buckets();
        assert_eq!(buckets.len(), 1);

        // Delete bucket
        store.delete_bucket("test-bucket").unwrap();
        assert!(!store.bucket_exists("test-bucket"));
    }

    #[test]
    fn test_object_operations() {
        let store = MetadataStore::new(test_config()).unwrap();

        store.create_bucket("test-bucket").unwrap();

        let key = ObjectKey::new("test-bucket", "test-object").unwrap();

        let location = ObjectLocation {
            target_id: "nqn.test".to_string(),
            namespace_id: 1,
            extents: vec![],
            size: 1024,
            content_hash: [0; 32],
            created_at: 0,
            modified_at: 0,
        };

        let now = chrono::Utc::now();
        let meta = ObjectMeta {
            size: 1024,
            content_hash: [0; 32],
            etag: "\"abc\"".to_string(),
            content_type: None,
            created_at: now,
            modified_at: now,
            version_id: None,
            user_metadata: HashMap::new(),
            is_delete_marker: false,
            storage_class: StorageClass::Standard,
        };

        // Put object
        store
            .put_object(&key, location.clone(), meta.clone())
            .unwrap();
        assert!(store.object_exists(&key));

        // Get object
        let entry = store.get_object(&key).unwrap();
        assert_eq!(entry.key.bucket(), key.bucket());
        assert_eq!(entry.key.key(), key.key());
        assert_eq!(entry.location.size, 1024);

        // Delete object
        store.delete_object(&key).unwrap();
        assert!(!store.object_exists(&key));
    }
}
