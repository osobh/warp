//! # Raft-based Consistency Module
//!
//! Provides strong consistency for metadata operations across distributed nodes
//! using the Raft consensus algorithm via the openraft crate.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    RaftStore                                     │
//! │  ┌─────────────────┐  ┌────────────────┐  ┌─────────────────┐   │
//! │  │ RaftStateMachine│  │ RaftLogStore   │  │ RaftNetwork     │   │
//! │  │ (metadata ops)  │  │ (persistent    │  │ (node comm)     │   │
//! │  │                 │  │  log entries)  │  │                 │   │
//! │  └─────────────────┘  └────────────────┘  └─────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Operations
//!
//! The state machine handles:
//! - Bucket creation/deletion
//! - Object metadata updates
//! - Versioning state changes
//! - Access control updates

mod log_store;
mod network;
mod persistent_log;
mod snapshot;
mod state_machine;
mod types;

pub use log_store::MemLogStore;
pub use network::{RaftNetworkFactory, RaftRouter};
pub use persistent_log::{SledLogStats, SledLogStore};
pub use snapshot::{SnapshotConfig, SnapshotError, SnapshotInfo, SnapshotManager, SnapshotStats};
pub use state_machine::MetadataStateMachine;
pub use types::*;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use openraft::{BasicNode, Config, Raft};
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::ObjectKey;
use crate::bucket::BucketConfig;
use crate::error::{Error, Result};

/// Raft-based distributed store wrapper
///
/// Provides strongly consistent metadata operations across a cluster of nodes.
pub struct RaftStore {
    /// The Raft instance
    raft: RaftNode,

    /// Local node ID
    node_id: NodeId,

    /// Network router for communicating with other nodes
    router: Arc<RaftRouter>,

    /// Local metadata cache (updated on reads from Raft responses)
    metadata_cache: Arc<RwLock<MetadataStateMachine>>,
}

impl RaftStore {
    /// Create a new Raft store as a single-node cluster
    pub async fn new(node_id: NodeId) -> Result<Self> {
        Self::with_config(node_id, RaftStoreConfig::default()).await
    }

    /// Create with custom configuration
    pub async fn with_config(node_id: NodeId, config: RaftStoreConfig) -> Result<Self> {
        let raft_config = Arc::new(Config {
            cluster_name: config.cluster_name,
            heartbeat_interval: config.heartbeat_interval_ms,
            election_timeout_min: config.election_timeout_min_ms,
            election_timeout_max: config.election_timeout_max_ms,
            ..Default::default()
        });

        // Create components - openraft takes ownership
        let state_machine = MetadataStateMachine::new();
        let log_store = RwLock::new(MemLogStore::new());
        let router = Arc::new(RaftRouter::new());
        let network_factory = RaftNetworkFactory::new(Arc::clone(&router));

        // Create Raft node (takes ownership of log_store and state_machine)
        let raft = Raft::new(
            node_id,
            raft_config,
            network_factory,
            log_store,
            state_machine,
        )
        .await
        .map_err(|e| Error::Raft(format!("Failed to create Raft node: {}", e)))?;

        // Create a local cache for reads (will be populated from responses)
        let metadata_cache = Arc::new(RwLock::new(MetadataStateMachine::new()));

        info!(node_id, "Created Raft store (in-memory)");

        Ok(Self {
            raft,
            node_id,
            router,
            metadata_cache,
        })
    }

    /// Create with persistent storage using sled
    ///
    /// This creates a RaftStore that persists log entries and metadata to disk,
    /// allowing recovery after restarts.
    pub async fn with_persistent_storage(node_id: NodeId, config: RaftStoreConfig) -> Result<Self> {
        let data_dir = config.data_dir.clone().ok_or_else(|| {
            Error::Raft("Persistent storage requires data_dir to be set".to_string())
        })?;

        // Ensure data directory exists
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| Error::Raft(format!("Failed to create data directory: {}", e)))?;

        let raft_config = Arc::new(Config {
            cluster_name: config.cluster_name.clone(),
            heartbeat_interval: config.heartbeat_interval_ms,
            election_timeout_min: config.election_timeout_min_ms,
            election_timeout_max: config.election_timeout_max_ms,
            ..Default::default()
        });

        // Create persistent log store
        let log_store_path = data_dir.join("raft_log");
        let log_store = SledLogStore::open(&log_store_path)
            .map_err(|e| Error::Raft(format!("Failed to open log store: {}", e)))?;
        let log_store = RwLock::new(log_store);

        // Create state machine
        let state_machine = MetadataStateMachine::new();

        // Create network
        let router = Arc::new(RaftRouter::new());
        let network_factory = RaftNetworkFactory::new(Arc::clone(&router));

        // Create Raft node
        let raft = Raft::new(
            node_id,
            raft_config,
            network_factory,
            log_store,
            state_machine,
        )
        .await
        .map_err(|e| Error::Raft(format!("Failed to create Raft node: {}", e)))?;

        // Create a local cache for reads
        let metadata_cache = Arc::new(RwLock::new(MetadataStateMachine::new()));

        info!(
            node_id,
            data_dir = ?data_dir,
            "Created Raft store (persistent)"
        );

        Ok(Self {
            raft,
            node_id,
            router,
            metadata_cache,
        })
    }

    /// Initialize as a single-node cluster (for leader election)
    pub async fn init_cluster(&self) -> Result<()> {
        let mut members = std::collections::BTreeMap::new();
        members.insert(
            self.node_id,
            BasicNode {
                addr: format!("node-{}", self.node_id),
            },
        );

        self.raft
            .initialize(members)
            .await
            .map_err(|e| Error::Raft(format!("Failed to initialize cluster: {}", e)))?;

        info!(node_id = self.node_id, "Initialized single-node cluster");
        Ok(())
    }

    /// Add a node to the cluster
    pub async fn add_node(&self, node_id: NodeId, addr: String) -> Result<()> {
        let node_info = BasicNode { addr };

        self.raft
            .add_learner(node_id, node_info, true)
            .await
            .map_err(|e| Error::Raft(format!("Failed to add learner: {}", e)))?;

        // Promote to voter
        let metrics = self.raft.metrics().borrow().clone();
        let mut new_members: BTreeSet<NodeId> =
            metrics.membership_config.membership().voter_ids().collect();
        new_members.insert(node_id);

        self.raft
            .change_membership(new_members, false)
            .await
            .map_err(|e| Error::Raft(format!("Failed to promote to voter: {}", e)))?;

        info!(node_id, "Added node to cluster");
        Ok(())
    }

    /// Remove a node from the cluster
    pub async fn remove_node(&self, node_id: NodeId) -> Result<()> {
        let metrics = self.raft.metrics().borrow().clone();
        let new_members: BTreeSet<NodeId> = metrics
            .membership_config
            .membership()
            .voter_ids()
            .filter(|&id| id != node_id)
            .collect();

        self.raft
            .change_membership(new_members, false)
            .await
            .map_err(|e| Error::Raft(format!("Failed to remove node: {}", e)))?;

        info!(node_id, "Removed node from cluster");
        Ok(())
    }

    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        self.raft.metrics().borrow().current_leader == Some(self.node_id)
    }

    /// Get the current leader ID
    pub fn current_leader(&self) -> Option<NodeId> {
        self.raft.metrics().borrow().current_leader
    }

    /// Wait for leadership (with timeout)
    pub async fn wait_for_leader(&self, timeout: std::time::Duration) -> Result<NodeId> {
        let start = std::time::Instant::now();

        loop {
            if let Some(leader) = self.current_leader() {
                return Ok(leader);
            }

            if start.elapsed() > timeout {
                return Err(Error::Raft("Timeout waiting for leader".to_string()));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // =========================================================================
    // Metadata Operations (replicated via Raft)
    // =========================================================================

    /// Create a bucket (replicated)
    pub async fn create_bucket(&self, name: &str, config: BucketConfig) -> Result<()> {
        let request = MetadataRequest::CreateBucket {
            name: name.to_string(),
            config: config.clone(),
        };

        self.write(request.clone()).await?;

        // Update local cache
        let mut cache = self.metadata_cache.write().await;
        cache.apply_request(request);

        debug!(bucket = name, "Created bucket via Raft");
        Ok(())
    }

    /// Delete a bucket (replicated)
    pub async fn delete_bucket(&self, name: &str) -> Result<()> {
        let request = MetadataRequest::DeleteBucket {
            name: name.to_string(),
        };

        self.write(request.clone()).await?;

        // Update local cache
        let mut cache = self.metadata_cache.write().await;
        cache.apply_request(request);

        debug!(bucket = name, "Deleted bucket via Raft");
        Ok(())
    }

    /// Update object metadata (replicated)
    pub async fn put_object_metadata(
        &self,
        key: &ObjectKey,
        metadata: ObjectMetadataEntry,
    ) -> Result<()> {
        let request = MetadataRequest::PutObjectMeta {
            key: key.clone(),
            metadata: metadata.clone(),
        };

        self.write(request.clone()).await?;

        // Update local cache
        let mut cache = self.metadata_cache.write().await;
        cache.apply_request(request);

        debug!(key = %key, "Updated object metadata via Raft");
        Ok(())
    }

    /// Delete object metadata (replicated)
    pub async fn delete_object_metadata(&self, key: &ObjectKey) -> Result<()> {
        let request = MetadataRequest::DeleteObjectMeta { key: key.clone() };

        self.write(request.clone()).await?;

        // Update local cache
        let mut cache = self.metadata_cache.write().await;
        cache.apply_request(request);

        debug!(key = %key, "Deleted object metadata via Raft");
        Ok(())
    }

    // =========================================================================
    // Read Operations (from local cache - eventually consistent)
    // =========================================================================

    /// Check if a bucket exists
    pub async fn bucket_exists(&self, name: &str) -> bool {
        let cache = self.metadata_cache.read().await;
        cache.bucket_exists(name)
    }

    /// Get bucket configuration
    pub async fn get_bucket_config(&self, name: &str) -> Option<BucketConfig> {
        let cache = self.metadata_cache.read().await;
        cache.get_bucket_config(name)
    }

    /// List all buckets
    pub async fn list_buckets(&self) -> Vec<String> {
        let cache = self.metadata_cache.read().await;
        cache.list_buckets()
    }

    /// Get object metadata
    pub async fn get_object_metadata(&self, key: &ObjectKey) -> Option<ObjectMetadataEntry> {
        let cache = self.metadata_cache.read().await;
        cache.get_object_metadata(key)
    }

    /// List objects in a bucket with prefix
    pub async fn list_objects(&self, bucket: &str, prefix: &str) -> Vec<ObjectMetadataEntry> {
        let cache = self.metadata_cache.read().await;
        cache.list_objects(bucket, prefix)
    }

    // =========================================================================
    // Internal
    // =========================================================================

    /// Submit a write request to Raft
    async fn write(&self, request: MetadataRequest) -> Result<MetadataResponse> {
        let response = self
            .raft
            .client_write(request)
            .await
            .map_err(|e| Error::Raft(format!("Write failed: {}", e)))?;

        Ok(response.data)
    }

    /// Get Raft metrics
    pub fn metrics(&self) -> RaftMetrics {
        let m = self.raft.metrics().borrow().clone();
        RaftMetrics {
            node_id: m.id,
            state: format!("{:?}", m.state),
            current_term: m.current_term,
            last_log_index: m.last_log_index.unwrap_or(0),
            last_applied: m.last_applied.map(|l| l.index).unwrap_or(0),
            current_leader: m.current_leader,
            membership: m.membership_config.membership().voter_ids().collect(),
        }
    }
}

/// Configuration for RaftStore
#[derive(Debug, Clone)]
pub struct RaftStoreConfig {
    /// Cluster name
    pub cluster_name: String,

    /// Heartbeat interval in milliseconds
    pub heartbeat_interval_ms: u64,

    /// Minimum election timeout in milliseconds
    pub election_timeout_min_ms: u64,

    /// Maximum election timeout in milliseconds
    pub election_timeout_max_ms: u64,

    /// Data directory for persistent storage (None = in-memory)
    pub data_dir: Option<PathBuf>,

    /// Snapshot configuration
    pub snapshot_config: Option<SnapshotConfig>,
}

impl Default for RaftStoreConfig {
    fn default() -> Self {
        Self {
            cluster_name: "warp-store".to_string(),
            heartbeat_interval_ms: 150,
            election_timeout_min_ms: 300,
            election_timeout_max_ms: 600,
            data_dir: None,
            snapshot_config: None,
        }
    }
}

impl RaftStoreConfig {
    /// Create a persistent configuration with the given data directory
    pub fn persistent(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            data_dir: Some(data_dir.clone()),
            snapshot_config: Some(SnapshotConfig {
                snapshot_dir: data_dir.join("snapshots"),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Set the cluster name
    pub fn with_cluster_name(mut self, name: impl Into<String>) -> Self {
        self.cluster_name = name.into();
        self
    }

    /// Set heartbeat and election timeouts
    pub fn with_timeouts(
        mut self,
        heartbeat_ms: u64,
        election_min_ms: u64,
        election_max_ms: u64,
    ) -> Self {
        self.heartbeat_interval_ms = heartbeat_ms;
        self.election_timeout_min_ms = election_min_ms;
        self.election_timeout_max_ms = election_max_ms;
        self
    }
}

/// Raft metrics snapshot
#[derive(Debug, Clone)]
pub struct RaftMetrics {
    /// This node's ID
    pub node_id: NodeId,

    /// Current Raft state (Leader, Follower, Candidate)
    pub state: String,

    /// Current term
    pub current_term: u64,

    /// Last log index
    pub last_log_index: u64,

    /// Last applied log index
    pub last_applied: u64,

    /// Current leader (if known)
    pub current_leader: Option<NodeId>,

    /// Current cluster membership
    pub membership: Vec<NodeId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_single_node_raft() {
        let store = RaftStore::new(1).await.unwrap();
        store.init_cluster().await.unwrap();

        // Wait for leader election
        let leader = store
            .wait_for_leader(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(leader, 1);
        assert!(store.is_leader());
    }

    #[tokio::test]
    async fn test_bucket_operations() {
        let store = RaftStore::new(1).await.unwrap();
        store.init_cluster().await.unwrap();
        store
            .wait_for_leader(std::time::Duration::from_secs(5))
            .await
            .unwrap();

        // Create bucket
        store
            .create_bucket("test-bucket", BucketConfig::default())
            .await
            .unwrap();
        assert!(store.bucket_exists("test-bucket").await);

        // List buckets
        let buckets = store.list_buckets().await;
        assert_eq!(buckets, vec!["test-bucket"]);

        // Delete bucket
        store.delete_bucket("test-bucket").await.unwrap();
        assert!(!store.bucket_exists("test-bucket").await);
    }

    #[tokio::test]
    async fn test_object_metadata() {
        let store = RaftStore::new(1).await.unwrap();
        store.init_cluster().await.unwrap();
        store
            .wait_for_leader(std::time::Duration::from_secs(5))
            .await
            .unwrap();

        // Create bucket first
        store
            .create_bucket("test-bucket", BucketConfig::default())
            .await
            .unwrap();

        // Put object metadata
        let key = ObjectKey::new("test-bucket", "path/to/object.bin").unwrap();
        let metadata = ObjectMetadataEntry {
            key: key.to_string(),
            size: 1024,
            etag: "abc123".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            version_id: None,
            created_at: chrono::Utc::now(),
            custom_metadata: Default::default(),
        };

        store
            .put_object_metadata(&key, metadata.clone())
            .await
            .unwrap();

        // Read back
        let retrieved = store.get_object_metadata(&key).await.unwrap();
        assert_eq!(retrieved.size, 1024);
        assert_eq!(retrieved.etag, "abc123");

        // List objects
        let objects = store.list_objects("test-bucket", "path/").await;
        assert_eq!(objects.len(), 1);

        // Delete
        store.delete_object_metadata(&key).await.unwrap();
        assert!(store.get_object_metadata(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_metrics() {
        let store = RaftStore::new(1).await.unwrap();
        store.init_cluster().await.unwrap();
        store
            .wait_for_leader(std::time::Duration::from_secs(5))
            .await
            .unwrap();

        let metrics = store.metrics();
        assert_eq!(metrics.node_id, 1);
        assert_eq!(metrics.current_leader, Some(1));
        assert!(metrics.membership.contains(&1));
    }
}
