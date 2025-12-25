//! Type definitions for the Raft consensus layer

use std::collections::BTreeMap;
use std::io::Cursor;

use openraft::{BasicNode, LogId, StoredMembership};
use serde::{Deserialize, Serialize};

use crate::bucket::BucketConfig;
use crate::ObjectKey;

/// Node ID type
pub type NodeId = u64;

/// Log entry ID
pub type LogEntryId = u64;

/// Raft type configuration for warp-store
///
/// Uses openraft's declare_raft_types macro with u64 for NodeId and BasicNode for Node.
openraft::declare_raft_types!(
    pub TypeConfig:
        D = MetadataRequest,
        R = MetadataResponse,
        Node = BasicNode,
);

/// The Raft node type alias
pub type RaftNode = openraft::Raft<TypeConfig>;

/// Metadata operation request (replicated via Raft)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetadataRequest {
    /// Create a new bucket
    CreateBucket {
        /// Bucket name
        name: String,
        /// Bucket configuration
        config: BucketConfig,
    },

    /// Delete a bucket
    DeleteBucket {
        /// Bucket name to delete
        name: String,
    },

    /// Put object metadata
    PutObjectMeta {
        /// Object key
        key: ObjectKey,
        /// Object metadata
        metadata: ObjectMetadataEntry,
    },

    /// Delete object metadata
    DeleteObjectMeta {
        /// Object key to delete
        key: ObjectKey,
    },

    /// Update bucket configuration
    UpdateBucketConfig {
        /// Bucket name
        name: String,
        /// New configuration
        config: BucketConfig,
    },
}

/// Response from metadata operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetadataResponse {
    /// Operation succeeded
    Ok,

    /// Bucket already exists
    BucketAlreadyExists(String),

    /// Bucket not found
    BucketNotFound(String),

    /// Object not found
    ObjectNotFound(String),

    /// Generic error
    Error(String),
}

/// Object metadata entry stored in the state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadataEntry {
    /// Full key (bucket/path)
    pub key: String,

    /// Object size in bytes
    pub size: u64,

    /// ETag (content hash)
    pub etag: String,

    /// Content type
    pub content_type: Option<String>,

    /// Version ID (if versioning enabled)
    pub version_id: Option<String>,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Custom metadata
    pub custom_metadata: BTreeMap<String, String>,
}

impl ObjectMetadataEntry {
    /// Create a new metadata entry
    pub fn new(key: &ObjectKey, size: u64, etag: String) -> Self {
        Self {
            key: key.to_string(),
            size,
            etag,
            content_type: None,
            version_id: None,
            created_at: chrono::Utc::now(),
            custom_metadata: BTreeMap::new(),
        }
    }

    /// Set content type
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Set version ID
    pub fn with_version(mut self, version_id: impl Into<String>) -> Self {
        self.version_id = Some(version_id.into());
        self
    }

    /// Add custom metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_metadata.insert(key.into(), value.into());
        self
    }
}

/// Snapshot of the entire metadata state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataSnapshot {
    /// Last applied log ID
    pub last_applied_log: Option<LogId<NodeId>>,

    /// Last membership config
    pub last_membership: StoredMembership<NodeId, BasicNode>,

    /// All buckets
    pub buckets: BTreeMap<String, BucketConfig>,

    /// All object metadata (key -> metadata)
    pub objects: BTreeMap<String, ObjectMetadataEntry>,
}

impl MetadataSnapshot {
    /// Create an empty snapshot
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize snapshot to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        rmp_serde::to_vec(self).expect("Failed to serialize snapshot")
    }

    /// Deserialize snapshot from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(bytes)
    }
}
