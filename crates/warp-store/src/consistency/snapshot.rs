//! Raft snapshot management
//!
//! Provides snapshot creation, storage, and restoration for the Raft state machine.
//! Snapshots are used for:
//! - Log compaction (removing old entries)
//! - Fast follower catch-up
//! - Cluster recovery

use std::path::{Path, PathBuf};
use std::sync::Arc;

use openraft::{LogId, SnapshotMeta, StoredMembership};
use sled::Db;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::types::*;
use super::state_machine::MetadataStateMachine;

/// Snapshot storage configuration
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Directory to store snapshots
    pub snapshot_dir: PathBuf,

    /// Maximum number of snapshots to retain
    pub max_snapshots: usize,

    /// Minimum number of log entries before triggering snapshot
    pub snapshot_threshold: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            snapshot_dir: PathBuf::from("./raft_snapshots"),
            max_snapshots: 3,
            snapshot_threshold: 10000,
        }
    }
}

/// Manages Raft snapshots on disk
pub struct SnapshotManager {
    /// Configuration
    config: SnapshotConfig,

    /// Sled database for snapshot metadata
    db: Arc<Db>,

    /// Snapshot metadata tree
    meta_tree: sled::Tree,
}

/// Keys for snapshot metadata
const KEY_CURRENT_SNAPSHOT: &[u8] = b"current_snapshot";
const KEY_SNAPSHOT_INDEX: &[u8] = b"snapshot_index";

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(db: Arc<Db>, config: SnapshotConfig) -> Result<Self, sled::Error> {
        let meta_tree = db.open_tree("raft_snapshots")?;

        // Ensure snapshot directory exists
        if let Err(e) = std::fs::create_dir_all(&config.snapshot_dir) {
            warn!(path = ?config.snapshot_dir, error = %e, "Failed to create snapshot directory");
        }

        Ok(Self {
            config,
            db,
            meta_tree,
        })
    }

    /// Get the current snapshot metadata
    pub fn get_current_snapshot(&self) -> Option<SnapshotInfo> {
        self.meta_tree
            .get(KEY_CURRENT_SNAPSHOT)
            .ok()
            .flatten()
            .and_then(|v| rmp_serde::from_slice(&v).ok())
    }

    /// Create a snapshot from the current state machine
    pub async fn create_snapshot(
        &self,
        state_machine: &RwLock<MetadataStateMachine>,
        last_applied: LogId<NodeId>,
        membership: StoredMembership<NodeId, openraft::BasicNode>,
    ) -> Result<SnapshotInfo, SnapshotError> {
        let sm = state_machine.read().await;

        // Create snapshot data
        let snapshot = MetadataSnapshot {
            last_applied_log: Some(last_applied),
            last_membership: membership.clone(),
            buckets: sm.buckets.clone(),
            objects: sm.objects.clone(),
        };

        let snapshot_bytes = snapshot.to_bytes();
        let snapshot_id = format!("snapshot-{}-{}", last_applied.leader_id.term, last_applied.index);

        // Calculate checksum
        let checksum = blake3::hash(&snapshot_bytes);

        // Write snapshot to disk
        let snapshot_path = self.config.snapshot_dir.join(&snapshot_id);
        tokio::fs::write(&snapshot_path, &snapshot_bytes)
            .await
            .map_err(|e| SnapshotError::Io(e.to_string()))?;

        let info = SnapshotInfo {
            id: snapshot_id.clone(),
            last_log_id: last_applied,
            membership,
            size_bytes: snapshot_bytes.len() as u64,
            checksum: checksum.to_hex().to_string(),
            path: snapshot_path,
            created_at: chrono::Utc::now(),
        };

        // Save metadata
        let info_bytes = rmp_serde::to_vec(&info).map_err(|e| SnapshotError::Serialization(e.to_string()))?;
        self.meta_tree
            .insert(KEY_CURRENT_SNAPSHOT, info_bytes)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        // Update snapshot index
        self.increment_snapshot_index()?;

        // Clean up old snapshots
        self.cleanup_old_snapshots().await?;

        info!(
            snapshot_id = %snapshot_id,
            last_index = last_applied.index,
            size = snapshot_bytes.len(),
            "Created Raft snapshot"
        );

        Ok(info)
    }

    /// Restore state machine from snapshot
    pub async fn restore_snapshot(
        &self,
        snapshot_info: &SnapshotInfo,
        state_machine: &RwLock<MetadataStateMachine>,
    ) -> Result<(), SnapshotError> {
        // Read snapshot from disk
        let snapshot_bytes = tokio::fs::read(&snapshot_info.path)
            .await
            .map_err(|e| SnapshotError::Io(e.to_string()))?;

        // Verify checksum
        let checksum = blake3::hash(&snapshot_bytes);
        if checksum.to_hex().to_string() != snapshot_info.checksum {
            return Err(SnapshotError::ChecksumMismatch);
        }

        // Deserialize snapshot
        let snapshot = MetadataSnapshot::from_bytes(&snapshot_bytes)
            .map_err(|e| SnapshotError::Serialization(e.to_string()))?;

        // Restore state machine
        let mut sm = state_machine.write().await;
        sm.buckets = snapshot.buckets;
        sm.objects = snapshot.objects;

        info!(
            snapshot_id = %snapshot_info.id,
            last_index = snapshot_info.last_log_id.index,
            buckets = sm.buckets.len(),
            objects = sm.objects.len(),
            "Restored state machine from snapshot"
        );

        Ok(())
    }

    /// Install a snapshot received from the leader
    pub async fn install_snapshot(
        &self,
        meta: SnapshotMeta<NodeId, openraft::BasicNode>,
        data: Vec<u8>,
        state_machine: &RwLock<MetadataStateMachine>,
    ) -> Result<SnapshotInfo, SnapshotError> {
        let last_log_id = meta.last_log_id.ok_or_else(|| {
            SnapshotError::Serialization("Snapshot meta missing last_log_id".to_string())
        })?;

        let snapshot_id = format!("snapshot-{}-{}", last_log_id.leader_id.term, last_log_id.index);
        let checksum = blake3::hash(&data);

        // Write snapshot to disk
        let snapshot_path = self.config.snapshot_dir.join(&snapshot_id);
        tokio::fs::write(&snapshot_path, &data)
            .await
            .map_err(|e| SnapshotError::Io(e.to_string()))?;

        // Create info
        let info = SnapshotInfo {
            id: snapshot_id.clone(),
            last_log_id,
            membership: meta.last_membership.clone(),
            size_bytes: data.len() as u64,
            checksum: checksum.to_hex().to_string(),
            path: snapshot_path.clone(),
            created_at: chrono::Utc::now(),
        };

        // Restore state machine
        let snapshot = MetadataSnapshot::from_bytes(&data)
            .map_err(|e| SnapshotError::Serialization(e.to_string()))?;

        {
            let mut sm = state_machine.write().await;
            sm.buckets = snapshot.buckets;
            sm.objects = snapshot.objects;
        }

        // Save as current snapshot
        let info_bytes = rmp_serde::to_vec(&info).map_err(|e| SnapshotError::Serialization(e.to_string()))?;
        self.meta_tree
            .insert(KEY_CURRENT_SNAPSHOT, info_bytes)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        info!(
            snapshot_id = %snapshot_id,
            last_index = last_log_id.index,
            size = data.len(),
            "Installed snapshot from leader"
        );

        Ok(info)
    }

    /// Get snapshot data for sending to followers
    pub async fn get_snapshot_data(&self, snapshot_info: &SnapshotInfo) -> Result<Vec<u8>, SnapshotError> {
        tokio::fs::read(&snapshot_info.path)
            .await
            .map_err(|e| SnapshotError::Io(e.to_string()))
    }

    /// Check if a snapshot should be created based on log size
    pub fn should_snapshot(&self, entries_since_last: u64) -> bool {
        entries_since_last >= self.config.snapshot_threshold
    }

    /// List all available snapshots
    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>, SnapshotError> {
        let mut snapshots = Vec::new();

        let mut entries = tokio::fs::read_dir(&self.config.snapshot_dir)
            .await
            .map_err(|e| SnapshotError::Io(e.to_string()))?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| SnapshotError::Io(e.to_string()))? {
            let path = entry.path();
            if path.is_file() && path.file_name().map(|n| n.to_string_lossy().starts_with("snapshot-")).unwrap_or(false) {
                // Try to read metadata from the file
                if let Ok(data) = tokio::fs::read(&path).await {
                    let checksum = blake3::hash(&data);
                    if let Ok(snapshot) = MetadataSnapshot::from_bytes(&data) {
                        if let Some(last_log_id) = snapshot.last_applied_log {
                            snapshots.push(SnapshotInfo {
                                id: path.file_name().unwrap().to_string_lossy().to_string(),
                                last_log_id,
                                membership: snapshot.last_membership.clone(),
                                size_bytes: data.len() as u64,
                                checksum: checksum.to_hex().to_string(),
                                path: path.clone(),
                                created_at: chrono::Utc::now(), // Not accurate but acceptable
                            });
                        }
                    }
                }
            }
        }

        // Sort by log index descending
        snapshots.sort_by(|a, b| b.last_log_id.index.cmp(&a.last_log_id.index));

        Ok(snapshots)
    }

    /// Clean up old snapshots keeping only the most recent ones
    async fn cleanup_old_snapshots(&self) -> Result<(), SnapshotError> {
        let mut snapshots = self.list_snapshots().await?;

        if snapshots.len() <= self.config.max_snapshots {
            return Ok(());
        }

        // Remove oldest snapshots
        let to_remove = snapshots.split_off(self.config.max_snapshots);

        for snapshot in to_remove {
            if let Err(e) = tokio::fs::remove_file(&snapshot.path).await {
                warn!(snapshot_id = %snapshot.id, error = %e, "Failed to remove old snapshot");
            } else {
                debug!(snapshot_id = %snapshot.id, "Removed old snapshot");
            }
        }

        Ok(())
    }

    /// Increment snapshot index counter
    fn increment_snapshot_index(&self) -> Result<u64, SnapshotError> {
        let current = self.meta_tree
            .get(KEY_SNAPSHOT_INDEX)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?
            .and_then(|v| {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&v);
                Some(u64::from_be_bytes(bytes))
            })
            .unwrap_or(0);

        let new_index = current + 1;
        self.meta_tree
            .insert(KEY_SNAPSHOT_INDEX, &new_index.to_be_bytes())
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        Ok(new_index)
    }

    /// Get statistics about snapshots
    pub async fn stats(&self) -> SnapshotStats {
        let snapshots = self.list_snapshots().await.unwrap_or_default();
        let total_size: u64 = snapshots.iter().map(|s| s.size_bytes).sum();
        let current = self.get_current_snapshot();

        SnapshotStats {
            snapshot_count: snapshots.len(),
            total_size_bytes: total_size,
            current_snapshot_index: current.as_ref().map(|s| s.last_log_id.index),
            oldest_snapshot_index: snapshots.last().map(|s| s.last_log_id.index),
        }
    }
}

/// Information about a snapshot
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotInfo {
    /// Unique snapshot ID
    pub id: String,

    /// Last log ID included in snapshot
    pub last_log_id: LogId<NodeId>,

    /// Cluster membership at snapshot time
    pub membership: StoredMembership<NodeId, openraft::BasicNode>,

    /// Size of snapshot data in bytes
    pub size_bytes: u64,

    /// BLAKE3 checksum of snapshot data
    pub checksum: String,

    /// Path to snapshot file
    #[serde(skip)]
    pub path: PathBuf,

    /// When snapshot was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Default for SnapshotInfo {
    fn default() -> Self {
        Self {
            id: String::new(),
            last_log_id: LogId::default(),
            membership: StoredMembership::default(),
            size_bytes: 0,
            checksum: String::new(),
            path: PathBuf::new(),
            created_at: chrono::Utc::now(),
        }
    }
}

/// Snapshot statistics
#[derive(Debug, Clone)]
pub struct SnapshotStats {
    /// Number of snapshots on disk
    pub snapshot_count: usize,

    /// Total size of all snapshots
    pub total_size_bytes: u64,

    /// Index of current snapshot
    pub current_snapshot_index: Option<u64>,

    /// Index of oldest snapshot
    pub oldest_snapshot_index: Option<u64>,
}

/// Snapshot errors
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// IO error
    #[error("IO error: {0}")]
    Io(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Checksum mismatch
    #[error("Snapshot checksum mismatch")]
    ChecksumMismatch,

    /// Snapshot not found
    #[error("Snapshot not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::bucket::BucketConfig;

    async fn create_test_manager() -> (TempDir, SnapshotManager) {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(sled::open(temp_dir.path().join("db")).unwrap());

        let config = SnapshotConfig {
            snapshot_dir: temp_dir.path().join("snapshots"),
            max_snapshots: 2,
            snapshot_threshold: 100,
        };

        let manager = SnapshotManager::new(db, config).unwrap();
        (temp_dir, manager)
    }

    #[tokio::test]
    async fn test_create_and_restore_snapshot() {
        let (_temp, manager) = create_test_manager().await;

        // Create state machine with some data
        let sm = RwLock::new(MetadataStateMachine::new());
        {
            let mut inner = sm.write().await;
            inner.buckets.insert("test-bucket".to_string(), BucketConfig::default());
        }

        // Create snapshot
        let last_applied = LogId::new(openraft::CommittedLeaderId::new(1, 1), 100);
        let membership = StoredMembership::default();

        let info = manager.create_snapshot(&sm, last_applied, membership.clone()).await.unwrap();

        assert_eq!(info.last_log_id.index, 100);
        assert!(!info.checksum.is_empty());

        // Clear state machine
        {
            let mut inner = sm.write().await;
            inner.buckets.clear();
        }

        // Restore from snapshot
        manager.restore_snapshot(&info, &sm).await.unwrap();

        // Verify restored data
        {
            let inner = sm.read().await;
            assert!(inner.buckets.contains_key("test-bucket"));
        }
    }

    #[tokio::test]
    async fn test_snapshot_cleanup() {
        let (_temp, manager) = create_test_manager().await;

        let sm = RwLock::new(MetadataStateMachine::new());
        let membership = StoredMembership::default();

        // Create 3 snapshots (max is 2)
        for i in 1..=3 {
            let last_applied = LogId::new(openraft::CommittedLeaderId::new(1, 1), i * 100);
            manager.create_snapshot(&sm, last_applied, membership.clone()).await.unwrap();
        }

        // Should only have 2 snapshots
        let snapshots = manager.list_snapshots().await.unwrap();
        assert_eq!(snapshots.len(), 2);

        // Should be the newest ones
        assert_eq!(snapshots[0].last_log_id.index, 300);
        assert_eq!(snapshots[1].last_log_id.index, 200);
    }

    #[tokio::test]
    async fn test_should_snapshot() {
        let (_temp, manager) = create_test_manager().await;

        // threshold is 100
        assert!(!manager.should_snapshot(50));
        assert!(!manager.should_snapshot(99));
        assert!(manager.should_snapshot(100));
        assert!(manager.should_snapshot(150));
    }

    #[tokio::test]
    async fn test_get_current_snapshot() {
        let (_temp, manager) = create_test_manager().await;

        // Initially no snapshot
        assert!(manager.get_current_snapshot().is_none());

        // Create snapshot
        let sm = RwLock::new(MetadataStateMachine::new());
        let last_applied = LogId::new(openraft::CommittedLeaderId::new(1, 1), 100);
        let membership = StoredMembership::default();

        manager.create_snapshot(&sm, last_applied, membership).await.unwrap();

        // Now should have current snapshot
        let current = manager.get_current_snapshot().unwrap();
        assert_eq!(current.last_log_id.index, 100);
    }

    #[tokio::test]
    async fn test_checksum_verification() {
        let (_temp, manager) = create_test_manager().await;

        let sm = RwLock::new(MetadataStateMachine::new());
        let last_applied = LogId::new(openraft::CommittedLeaderId::new(1, 1), 100);
        let membership = StoredMembership::default();

        let info = manager.create_snapshot(&sm, last_applied, membership).await.unwrap();

        // Corrupt the file
        tokio::fs::write(&info.path, b"corrupted data").await.unwrap();

        // Restore should fail with checksum mismatch
        let result = manager.restore_snapshot(&info, &sm).await;
        assert!(matches!(result, Err(SnapshotError::ChecksumMismatch)));
    }

    #[tokio::test]
    async fn test_stats() {
        let (_temp, manager) = create_test_manager().await;

        let sm = RwLock::new(MetadataStateMachine::new());
        let membership = StoredMembership::default();

        // Create 2 snapshots
        for i in 1..=2 {
            let last_applied = LogId::new(openraft::CommittedLeaderId::new(1, 1), i * 100);
            manager.create_snapshot(&sm, last_applied, membership.clone()).await.unwrap();
        }

        let stats = manager.stats().await;
        assert_eq!(stats.snapshot_count, 2);
        assert!(stats.total_size_bytes > 0);
        assert_eq!(stats.current_snapshot_index, Some(200));
        assert_eq!(stats.oldest_snapshot_index, Some(100));
    }
}
