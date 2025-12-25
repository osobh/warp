//! Raft state machine for metadata operations
//!
//! The state machine is the core of the Raft-based consistency layer.
//! It maintains the authoritative state of all metadata and applies
//! committed log entries to update that state.

use std::collections::BTreeMap;
use std::io::Cursor;

use openraft::{
    BasicNode, EntryPayload, LogId,
    Snapshot, SnapshotMeta, StoredMembership, StorageError,
    storage::{RaftSnapshotBuilder, RaftStateMachine},
    ErrorSubject, ErrorVerb,
};
use tracing::debug;

use super::types::*;
use crate::bucket::BucketConfig;
use crate::ObjectKey;

/// State machine that manages all metadata
///
/// Maintains:
/// - Bucket configurations
/// - Object metadata
/// - Versioning state
pub struct MetadataStateMachine {
    /// Last applied log ID
    pub(crate) last_applied_log: Option<LogId<NodeId>>,

    /// Last membership configuration
    pub(crate) last_membership: StoredMembership<NodeId, BasicNode>,

    /// Bucket name -> config
    pub(crate) buckets: BTreeMap<String, BucketConfig>,

    /// Object key (as string) -> metadata
    pub(crate) objects: BTreeMap<String, ObjectMetadataEntry>,

    /// Snapshot counter for unique IDs
    pub(crate) snapshot_idx: u64,
}

impl MetadataStateMachine {
    /// Create a new empty state machine
    pub fn new() -> Self {
        Self {
            last_applied_log: None,
            last_membership: StoredMembership::default(),
            buckets: BTreeMap::new(),
            objects: BTreeMap::new(),
            snapshot_idx: 0,
        }
    }

    /// Check if a bucket exists
    pub fn bucket_exists(&self, name: &str) -> bool {
        self.buckets.contains_key(name)
    }

    /// Get bucket configuration
    pub fn get_bucket_config(&self, name: &str) -> Option<BucketConfig> {
        self.buckets.get(name).cloned()
    }

    /// List all bucket names
    pub fn list_buckets(&self) -> Vec<String> {
        self.buckets.keys().cloned().collect()
    }

    /// Get object metadata
    pub fn get_object_metadata(&self, key: &ObjectKey) -> Option<ObjectMetadataEntry> {
        self.objects.get(&key.to_string()).cloned()
    }

    /// List objects in a bucket with optional prefix
    pub fn list_objects(&self, bucket: &str, prefix: &str) -> Vec<ObjectMetadataEntry> {
        let full_prefix = if prefix.is_empty() {
            format!("{}/", bucket)
        } else {
            format!("{}/{}", bucket, prefix)
        };

        self.objects
            .range(full_prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&full_prefix))
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Apply a metadata request to the state machine (public wrapper)
    pub fn apply_request(&mut self, request: MetadataRequest) -> MetadataResponse {
        self.apply(request)
    }

    /// Apply a metadata request to the state machine
    fn apply(&mut self, request: MetadataRequest) -> MetadataResponse {
        match request {
            MetadataRequest::CreateBucket { name, config } => {
                if self.buckets.contains_key(&name) {
                    return MetadataResponse::BucketAlreadyExists(name);
                }
                self.buckets.insert(name.clone(), config);
                debug!(bucket = %name, "Applied: CreateBucket");
                MetadataResponse::Ok
            }

            MetadataRequest::DeleteBucket { name } => {
                if self.buckets.remove(&name).is_none() {
                    return MetadataResponse::BucketNotFound(name);
                }
                // Also remove all objects in the bucket
                let prefix = format!("{}/", name);
                self.objects.retain(|k, _| !k.starts_with(&prefix));
                debug!(bucket = %name, "Applied: DeleteBucket");
                MetadataResponse::Ok
            }

            MetadataRequest::PutObjectMeta { key, metadata } => {
                // Ensure bucket exists
                if !self.buckets.contains_key(key.bucket()) {
                    return MetadataResponse::BucketNotFound(key.bucket().to_string());
                }
                self.objects.insert(key.to_string(), metadata);
                debug!(key = %key, "Applied: PutObjectMeta");
                MetadataResponse::Ok
            }

            MetadataRequest::DeleteObjectMeta { key } => {
                if self.objects.remove(&key.to_string()).is_none() {
                    return MetadataResponse::ObjectNotFound(key.to_string());
                }
                debug!(key = %key, "Applied: DeleteObjectMeta");
                MetadataResponse::Ok
            }

            MetadataRequest::UpdateBucketConfig { name, config } => {
                if !self.buckets.contains_key(&name) {
                    return MetadataResponse::BucketNotFound(name);
                }
                self.buckets.insert(name.clone(), config);
                debug!(bucket = %name, "Applied: UpdateBucketConfig");
                MetadataResponse::Ok
            }
        }
    }

    /// Create a snapshot of the current state
    fn snapshot(&self) -> MetadataSnapshot {
        MetadataSnapshot {
            last_applied_log: self.last_applied_log,
            last_membership: self.last_membership.clone(),
            buckets: self.buckets.clone(),
            objects: self.objects.clone(),
        }
    }

    /// Restore state from a snapshot
    fn restore(&mut self, snapshot: MetadataSnapshot) {
        self.last_applied_log = snapshot.last_applied_log;
        self.last_membership = snapshot.last_membership;
        self.buckets = snapshot.buckets;
        self.objects = snapshot.objects;
    }
}

impl Default for MetadataStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot builder for the metadata state machine
pub struct MetadataSnapshotBuilder {
    /// Snapshot data to be used
    snapshot_data: MetadataSnapshot,
    /// Snapshot index
    snapshot_idx: u64,
}

impl MetadataSnapshotBuilder {
    /// Create a new snapshot builder
    pub fn new(snapshot_data: MetadataSnapshot, snapshot_idx: u64) -> Self {
        Self { snapshot_data, snapshot_idx }
    }
}

impl RaftSnapshotBuilder<TypeConfig> for MetadataSnapshotBuilder {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeId>> {
        let bytes = self.snapshot_data.to_bytes();

        let last_applied_log = self.snapshot_data.last_applied_log;
        let last_membership = self.snapshot_data.last_membership.clone();

        let snapshot_id = if let Some(log_id) = last_applied_log {
            format!("{}-{}-{}", log_id.leader_id, log_id.index, self.snapshot_idx)
        } else {
            format!("0-0-{}", self.snapshot_idx)
        };

        let meta = SnapshotMeta {
            last_log_id: last_applied_log,
            last_membership,
            snapshot_id,
        };

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(bytes)),
        })
    }
}

impl RaftStateMachine<TypeConfig> for MetadataStateMachine {
    type SnapshotBuilder = MetadataSnapshotBuilder;

    async fn applied_state(
        &mut self,
    ) -> Result<(Option<LogId<NodeId>>, StoredMembership<NodeId, BasicNode>), StorageError<NodeId>> {
        Ok((self.last_applied_log, self.last_membership.clone()))
    }

    async fn apply<I>(&mut self, entries: I) -> Result<Vec<MetadataResponse>, StorageError<NodeId>>
    where
        I: IntoIterator<Item = openraft::Entry<TypeConfig>> + Send,
    {
        let mut responses = Vec::new();

        for entry in entries {
            self.last_applied_log = Some(entry.log_id);

            match entry.payload {
                EntryPayload::Blank => {
                    responses.push(MetadataResponse::Ok);
                }
                EntryPayload::Normal(request) => {
                    let response = self.apply(request);
                    responses.push(response);
                }
                EntryPayload::Membership(membership) => {
                    self.last_membership = StoredMembership::new(Some(entry.log_id), membership);
                    responses.push(MetadataResponse::Ok);
                }
            }
        }

        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.snapshot_idx += 1;
        MetadataSnapshotBuilder::new(self.snapshot(), self.snapshot_idx)
    }

    async fn begin_receiving_snapshot(&mut self) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        let bytes = snapshot.into_inner();
        let data = MetadataSnapshot::from_bytes(&bytes)
            .map_err(|e| StorageError::from_io_error(
                ErrorSubject::StateMachine,
                ErrorVerb::Read,
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
            ))?;

        self.last_applied_log = meta.last_log_id;
        self.last_membership = meta.last_membership.clone();
        self.buckets = data.buckets;
        self.objects = data.objects;

        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        let snapshot_data = self.snapshot();
        let bytes = snapshot_data.to_bytes();

        let last_applied_log = self.last_applied_log;
        let last_membership = self.last_membership.clone();

        if last_applied_log.is_none() {
            return Ok(None);
        }

        let snapshot_id = if let Some(log_id) = last_applied_log {
            format!("{}-{}-{}", log_id.leader_id, log_id.index, self.snapshot_idx)
        } else {
            format!("0-0-{}", self.snapshot_idx)
        };

        let meta = SnapshotMeta {
            last_log_id: last_applied_log,
            last_membership,
            snapshot_id,
        };

        Ok(Some(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(bytes)),
        }))
    }
}

// Helper to convert MetadataSnapshot to MetadataStateMachine
impl From<MetadataSnapshot> for MetadataStateMachine {
    fn from(snapshot: MetadataSnapshot) -> Self {
        Self {
            last_applied_log: snapshot.last_applied_log,
            last_membership: snapshot.last_membership,
            buckets: snapshot.buckets,
            objects: snapshot.objects,
            snapshot_idx: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_operations() {
        let mut sm = MetadataStateMachine::new();

        // Create bucket
        let resp = sm.apply(MetadataRequest::CreateBucket {
            name: "test".to_string(),
            config: BucketConfig::default(),
        });
        assert!(matches!(resp, MetadataResponse::Ok));
        assert!(sm.bucket_exists("test"));

        // Duplicate create
        let resp = sm.apply(MetadataRequest::CreateBucket {
            name: "test".to_string(),
            config: BucketConfig::default(),
        });
        assert!(matches!(resp, MetadataResponse::BucketAlreadyExists(_)));

        // Delete bucket
        let resp = sm.apply(MetadataRequest::DeleteBucket {
            name: "test".to_string(),
        });
        assert!(matches!(resp, MetadataResponse::Ok));
        assert!(!sm.bucket_exists("test"));
    }

    #[test]
    fn test_object_metadata() {
        let mut sm = MetadataStateMachine::new();

        // Create bucket first
        sm.apply(MetadataRequest::CreateBucket {
            name: "bucket".to_string(),
            config: BucketConfig::default(),
        });

        // Put object
        let key = ObjectKey::new("bucket", "path/to/file.txt").unwrap();
        let metadata = ObjectMetadataEntry::new(&key, 1024, "etag123".to_string());

        let resp = sm.apply(MetadataRequest::PutObjectMeta {
            key: key.clone(),
            metadata,
        });
        assert!(matches!(resp, MetadataResponse::Ok));

        // Get object
        let retrieved = sm.get_object_metadata(&key).unwrap();
        assert_eq!(retrieved.size, 1024);

        // List objects
        let objects = sm.list_objects("bucket", "path/");
        assert_eq!(objects.len(), 1);

        // Delete object
        let resp = sm.apply(MetadataRequest::DeleteObjectMeta { key: key.clone() });
        assert!(matches!(resp, MetadataResponse::Ok));
        assert!(sm.get_object_metadata(&key).is_none());
    }

    #[test]
    fn test_snapshot() {
        let mut sm = MetadataStateMachine::new();

        // Add some data
        sm.apply(MetadataRequest::CreateBucket {
            name: "bucket1".to_string(),
            config: BucketConfig::default(),
        });
        sm.apply(MetadataRequest::CreateBucket {
            name: "bucket2".to_string(),
            config: BucketConfig::default(),
        });

        // Take snapshot
        let snapshot = sm.snapshot();
        let bytes = snapshot.to_bytes();

        // Create new state machine and restore
        let mut sm2 = MetadataStateMachine::new();
        let restored = MetadataSnapshot::from_bytes(&bytes).unwrap();
        sm2.restore(restored);

        // Verify data
        assert!(sm2.bucket_exists("bucket1"));
        assert!(sm2.bucket_exists("bucket2"));
    }
}
