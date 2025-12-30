//! Snapshot management and lifecycle

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use super::{RestoreOptions, SnapshotDiff, SnapshotGranularity};

/// Unique identifier for a snapshot
pub type SnapshotId = u64;

/// State of a snapshot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    /// Snapshot is being created
    Creating,
    /// Snapshot is complete and usable
    Active,
    /// Snapshot is being deleted
    Deleting,
    /// Snapshot is locked (cannot be deleted)
    Locked,
    /// Snapshot creation failed
    Failed,
    /// Snapshot is being restored
    Restoring,
}

/// A point-in-time snapshot
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Unique snapshot ID
    pub id: SnapshotId,

    /// Name of the snapshot
    pub name: String,

    /// Description
    pub description: Option<String>,

    /// Bucket this snapshot is for
    pub bucket: String,

    /// Prefix (for partial snapshots)
    pub prefix: Option<String>,

    /// Snapshot state
    pub state: SnapshotState,

    /// When the snapshot was created
    pub created_at: SystemTime,

    /// When the snapshot was completed
    pub completed_at: Option<SystemTime>,

    /// Parent snapshot (for incremental snapshots)
    pub parent_id: Option<SnapshotId>,

    /// Object count at snapshot time
    pub object_count: u64,

    /// Total size in bytes
    pub total_bytes: u64,

    /// Unique bytes (after dedup with parent)
    pub unique_bytes: u64,

    /// Labels/tags
    pub labels: HashMap<String, String>,

    /// Retention policy
    pub retention: Option<RetentionPolicy>,

    /// Snapshot granularity
    pub granularity: SnapshotGranularity,

    /// Object versions at snapshot time
    pub object_versions: HashMap<String, ObjectVersionRef>,
}

impl Snapshot {
    /// Create a new snapshot
    pub fn new(bucket: impl Into<String>, name: impl Into<String>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            name: name.into(),
            description: None,
            bucket: bucket.into(),
            prefix: None,
            state: SnapshotState::Creating,
            created_at: SystemTime::now(),
            completed_at: None,
            parent_id: None,
            object_count: 0,
            total_bytes: 0,
            unique_bytes: 0,
            labels: HashMap::new(),
            retention: None,
            granularity: SnapshotGranularity::Bucket,
            object_versions: HashMap::new(),
        }
    }

    /// Create a snapshot with prefix filter
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self.granularity = SnapshotGranularity::Prefix;
        self
    }

    /// Create an incremental snapshot based on parent
    pub fn incremental(mut self, parent_id: SnapshotId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Add a label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Set retention policy
    pub fn with_retention(mut self, policy: RetentionPolicy) -> Self {
        self.retention = Some(policy);
        self
    }

    /// Check if snapshot is usable
    pub fn is_active(&self) -> bool {
        self.state == SnapshotState::Active || self.state == SnapshotState::Locked
    }

    /// Check if snapshot can be deleted
    pub fn can_delete(&self) -> bool {
        self.state == SnapshotState::Active
    }

    /// Get snapshot age
    pub fn age(&self) -> Duration {
        self.created_at
            .elapsed()
            .unwrap_or(Duration::ZERO)
    }
}

/// Reference to an object version within a snapshot
#[derive(Debug, Clone)]
pub struct ObjectVersionRef {
    /// Object key
    pub key: String,
    /// Version ID
    pub version_id: String,
    /// Size in bytes
    pub size: u64,
    /// Checksum
    pub checksum: [u8; 32],
    /// Content blocks (COW references)
    pub blocks: Vec<u64>,
}

/// Retention policy for snapshots
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// Minimum retention duration
    pub min_retention: Duration,
    /// Maximum retention duration (auto-delete after)
    pub max_retention: Option<Duration>,
    /// Whether to lock the snapshot during retention
    pub lock_during_retention: bool,
}

impl RetentionPolicy {
    /// Create a policy with minimum retention
    pub fn minimum(duration: Duration) -> Self {
        Self {
            min_retention: duration,
            max_retention: None,
            lock_during_retention: true,
        }
    }

    /// Create a policy with auto-delete
    pub fn auto_delete(duration: Duration) -> Self {
        Self {
            min_retention: Duration::ZERO,
            max_retention: Some(duration),
            lock_during_retention: false,
        }
    }
}

/// Snapshot schedule for automatic snapshots
#[derive(Debug, Clone)]
pub struct SnapshotSchedule {
    /// Schedule name
    pub name: String,

    /// Bucket to snapshot
    pub bucket: String,

    /// Optional prefix
    pub prefix: Option<String>,

    /// Interval between snapshots
    pub interval: Duration,

    /// Maximum snapshots to keep
    pub keep_count: usize,

    /// Retention policy
    pub retention: Option<RetentionPolicy>,

    /// Whether schedule is enabled
    pub enabled: bool,

    /// Last execution time
    pub last_run: Option<Instant>,
}

impl SnapshotSchedule {
    /// Create a new schedule
    pub fn new(name: impl Into<String>, bucket: impl Into<String>, interval: Duration) -> Self {
        Self {
            name: name.into(),
            bucket: bucket.into(),
            prefix: None,
            interval,
            keep_count: 24, // Keep 24 snapshots by default
            retention: None,
            enabled: true,
            last_run: None,
        }
    }

    /// Check if snapshot is due
    pub fn is_due(&self) -> bool {
        if !self.enabled {
            return false;
        }

        match self.last_run {
            Some(last) => last.elapsed() >= self.interval,
            None => true,
        }
    }

    /// Mark as executed
    pub fn mark_executed(&mut self) {
        self.last_run = Some(Instant::now());
    }
}

/// Policy for automatic snapshots
#[derive(Debug, Clone)]
pub struct SnapshotPolicy {
    /// Policy name
    pub name: String,

    /// Schedules
    pub schedules: Vec<SnapshotSchedule>,

    /// Enable incremental snapshots
    pub incremental: bool,

    /// Enable compression
    pub compress: bool,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            schedules: Vec::new(),
            incremental: true,
            compress: true,
        }
    }
}

/// Configuration for snapshot manager
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Maximum pending snapshot operations
    pub max_pending_snapshots: usize,

    /// Timeout for snapshot creation
    pub snapshot_timeout: Duration,

    /// Enable deduplication across snapshots
    pub dedup_enabled: bool,

    /// Maximum concurrent restores
    pub max_concurrent_restores: usize,

    /// Auto-cleanup expired snapshots
    pub auto_cleanup: bool,

    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            max_pending_snapshots: 10,
            snapshot_timeout: Duration::from_secs(300),
            dedup_enabled: true,
            max_concurrent_restores: 2,
            auto_cleanup: true,
            cleanup_interval: Duration::from_secs(3600),
        }
    }
}

/// Statistics for snapshot operations
#[derive(Debug, Clone, Default)]
pub struct SnapshotStats {
    /// Total snapshots created
    pub snapshots_created: u64,
    /// Total snapshots deleted
    pub snapshots_deleted: u64,
    /// Total restores performed
    pub restores_performed: u64,
    /// Total bytes in all snapshots
    pub total_snapshot_bytes: u64,
    /// Bytes saved by deduplication
    pub bytes_saved_by_dedup: u64,
    /// Current active snapshots
    pub active_snapshots: u64,
}

/// Manages snapshots and their lifecycle
pub struct SnapshotManager {
    /// Configuration
    config: SnapshotConfig,

    /// All snapshots by ID
    snapshots: DashMap<SnapshotId, Snapshot>,

    /// Snapshots by bucket
    bucket_snapshots: DashMap<String, HashSet<SnapshotId>>,

    /// Active schedules
    schedules: RwLock<HashMap<String, SnapshotSchedule>>,

    /// Statistics
    stats: RwLock<SnapshotStats>,

    /// Pending operations
    pending_ops: AtomicU64,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(config: SnapshotConfig) -> Self {
        Self {
            config,
            snapshots: DashMap::new(),
            bucket_snapshots: DashMap::new(),
            schedules: RwLock::new(HashMap::new()),
            stats: RwLock::new(SnapshotStats::default()),
            pending_ops: AtomicU64::new(0),
        }
    }

    /// Create a snapshot
    pub async fn create_snapshot(
        &self,
        bucket: &str,
        name: &str,
        prefix: Option<&str>,
    ) -> Result<Snapshot, String> {
        // Check pending limit
        let pending = self.pending_ops.fetch_add(1, Ordering::SeqCst);
        if pending >= self.config.max_pending_snapshots as u64 {
            self.pending_ops.fetch_sub(1, Ordering::SeqCst);
            return Err("Too many pending snapshot operations".to_string());
        }

        info!(bucket, name, "Creating snapshot");

        let mut snapshot = Snapshot::new(bucket, name);
        if let Some(p) = prefix {
            snapshot = snapshot.with_prefix(p);
        }

        // Look for parent snapshot (most recent active snapshot of same bucket)
        if self.config.dedup_enabled {
            if let Some(parent_id) = self.find_latest_snapshot(bucket) {
                snapshot = snapshot.incremental(parent_id);
            }
        }

        let snapshot_id = snapshot.id;

        // Store the snapshot
        self.snapshots.insert(snapshot_id, snapshot.clone());
        self.bucket_snapshots
            .entry(bucket.to_string())
            .or_insert_with(HashSet::new)
            .insert(snapshot_id);

        // In a real implementation, we would:
        // 1. Lock the bucket for consistent snapshot
        // 2. Enumerate all objects and their versions
        // 3. Create COW block references
        // 4. Update snapshot with object count and size

        // Simulate snapshot creation
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Mark as complete
        if let Some(mut snap) = self.snapshots.get_mut(&snapshot_id) {
            snap.state = SnapshotState::Active;
            snap.completed_at = Some(SystemTime::now());
        }

        self.pending_ops.fetch_sub(1, Ordering::SeqCst);

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.snapshots_created += 1;
            stats.active_snapshots += 1;
        }

        info!(
            bucket,
            name,
            snapshot_id,
            "Snapshot created"
        );

        Ok(self.snapshots.get(&snapshot_id).unwrap().clone())
    }

    /// Delete a snapshot
    pub async fn delete_snapshot(&self, snapshot_id: SnapshotId) -> Result<(), String> {
        let snapshot = self.snapshots.get(&snapshot_id)
            .ok_or_else(|| format!("Snapshot {} not found", snapshot_id))?;

        if !snapshot.can_delete() {
            return Err(format!("Snapshot {} cannot be deleted (state: {:?})",
                snapshot_id, snapshot.state));
        }

        let bucket = snapshot.bucket.clone();
        drop(snapshot);

        // Mark as deleting
        if let Some(mut snap) = self.snapshots.get_mut(&snapshot_id) {
            snap.state = SnapshotState::Deleting;
        }

        info!(snapshot_id, "Deleting snapshot");

        // In a real implementation, we would:
        // 1. Decrement COW block reference counts
        // 2. Delete unreferenced blocks
        // 3. Update dependent snapshots

        // Remove from index
        self.snapshots.remove(&snapshot_id);
        if let Some(mut bucket_snaps) = self.bucket_snapshots.get_mut(&bucket) {
            bucket_snaps.remove(&snapshot_id);
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.snapshots_deleted += 1;
            stats.active_snapshots = stats.active_snapshots.saturating_sub(1);
        }

        info!(snapshot_id, "Snapshot deleted");

        Ok(())
    }

    /// Restore from a snapshot
    pub async fn restore_snapshot(
        &self,
        snapshot_id: SnapshotId,
        options: RestoreOptions,
    ) -> Result<RestoreResult, String> {
        let snapshot = self.snapshots.get(&snapshot_id)
            .ok_or_else(|| format!("Snapshot {} not found", snapshot_id))?;

        if !snapshot.is_active() {
            return Err(format!("Snapshot {} is not active", snapshot_id));
        }

        let target_bucket = options.target_bucket
            .clone()
            .unwrap_or_else(|| snapshot.bucket.clone());

        info!(
            snapshot_id,
            source_bucket = %snapshot.bucket,
            target_bucket = %target_bucket,
            "Restoring from snapshot"
        );

        // Mark as restoring
        drop(snapshot);
        if let Some(mut snap) = self.snapshots.get_mut(&snapshot_id) {
            snap.state = SnapshotState::Restoring;
        }

        let start = Instant::now();
        let mut objects_restored = 0u64;
        let mut bytes_restored = 0u64;

        // In a real implementation, we would:
        // 1. Iterate through snapshot's object versions
        // 2. Restore each object to target bucket
        // 3. Apply filter if specified
        // 4. Handle conflicts based on options

        // Simulate restore
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Mark as active again
        if let Some(mut snap) = self.snapshots.get_mut(&snapshot_id) {
            snap.state = SnapshotState::Active;
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.restores_performed += 1;
        }

        info!(
            snapshot_id,
            objects_restored,
            bytes_restored,
            duration = ?start.elapsed(),
            "Restore complete"
        );

        Ok(RestoreResult {
            snapshot_id,
            target_bucket,
            objects_restored,
            bytes_restored,
            duration: start.elapsed(),
        })
    }

    /// Get a snapshot by ID
    pub fn get_snapshot(&self, snapshot_id: SnapshotId) -> Option<Snapshot> {
        self.snapshots.get(&snapshot_id).map(|s| s.clone())
    }

    /// List snapshots for a bucket
    pub fn list_snapshots(&self, bucket: &str) -> Vec<Snapshot> {
        self.bucket_snapshots
            .get(bucket)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.snapshots.get(id).map(|s| s.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all snapshots
    pub fn list_all_snapshots(&self) -> Vec<Snapshot> {
        self.snapshots.iter().map(|r| r.value().clone()).collect()
    }

    /// Find the latest active snapshot for a bucket
    pub fn find_latest_snapshot(&self, bucket: &str) -> Option<SnapshotId> {
        self.bucket_snapshots
            .get(bucket)
            .and_then(|ids| {
                ids.iter()
                    .filter_map(|id| self.snapshots.get(id))
                    .filter(|s| s.is_active())
                    .max_by_key(|s| s.created_at)
                    .map(|s| s.id)
            })
    }

    /// Diff two snapshots
    pub fn diff_snapshots(
        &self,
        from_id: SnapshotId,
        to_id: SnapshotId,
    ) -> Result<SnapshotDiff, String> {
        let from = self.snapshots.get(&from_id)
            .ok_or_else(|| format!("Snapshot {} not found", from_id))?;
        let to = self.snapshots.get(&to_id)
            .ok_or_else(|| format!("Snapshot {} not found", to_id))?;

        if from.bucket != to.bucket {
            return Err("Cannot diff snapshots from different buckets".to_string());
        }

        let from_keys: HashSet<_> = from.object_versions.keys().cloned().collect();
        let to_keys: HashSet<_> = to.object_versions.keys().cloned().collect();

        let added: Vec<_> = to_keys.difference(&from_keys).cloned().collect();
        let removed: Vec<_> = from_keys.difference(&to_keys).cloned().collect();

        let modified: Vec<_> = from_keys
            .intersection(&to_keys)
            .filter(|key| {
                let from_ver = from.object_versions.get(*key);
                let to_ver = to.object_versions.get(*key);
                match (from_ver, to_ver) {
                    (Some(f), Some(t)) => f.checksum != t.checksum,
                    _ => false,
                }
            })
            .cloned()
            .collect();

        // Calculate byte changes
        let bytes_added: u64 = added.iter()
            .filter_map(|key| to.object_versions.get(key))
            .map(|v| v.size)
            .sum();

        let bytes_removed: u64 = removed.iter()
            .filter_map(|key| from.object_versions.get(key))
            .map(|v| v.size)
            .sum();

        Ok(SnapshotDiff {
            from: from_id,
            to: to_id,
            added,
            removed,
            modified,
            bytes_added,
            bytes_removed,
        })
    }

    /// Add a snapshot schedule
    pub fn add_schedule(&self, schedule: SnapshotSchedule) {
        self.schedules.write().insert(schedule.name.clone(), schedule);
    }

    /// Remove a schedule
    pub fn remove_schedule(&self, name: &str) -> Option<SnapshotSchedule> {
        self.schedules.write().remove(name)
    }

    /// Get schedules that are due
    pub fn get_due_schedules(&self) -> Vec<SnapshotSchedule> {
        self.schedules
            .read()
            .values()
            .filter(|s| s.is_due())
            .cloned()
            .collect()
    }

    /// Execute scheduled snapshots
    pub async fn execute_schedules(&self) -> Vec<Result<Snapshot, String>> {
        let due_schedules = self.get_due_schedules();
        let mut results = Vec::new();

        for schedule in due_schedules {
            let result = self.create_snapshot(
                &schedule.bucket,
                &format!("scheduled-{}", chrono::Utc::now().timestamp()),
                schedule.prefix.as_deref(),
            ).await;

            // Mark as executed
            if let Some(mut s) = self.schedules.write().get_mut(&schedule.name) {
                s.mark_executed();
            }

            // Prune old snapshots if needed
            if result.is_ok() && schedule.keep_count > 0 {
                self.prune_bucket_snapshots(&schedule.bucket, schedule.keep_count).await;
            }

            results.push(result);
        }

        results
    }

    /// Prune old snapshots from a bucket
    pub async fn prune_bucket_snapshots(&self, bucket: &str, keep_count: usize) {
        let mut snapshots = self.list_snapshots(bucket);

        // Sort by creation time (newest first)
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Delete old snapshots beyond keep_count
        for snapshot in snapshots.into_iter().skip(keep_count) {
            if snapshot.can_delete() {
                if let Err(e) = self.delete_snapshot(snapshot.id).await {
                    warn!(
                        snapshot_id = snapshot.id,
                        error = %e,
                        "Failed to prune snapshot"
                    );
                }
            }
        }
    }

    /// Cleanup expired snapshots
    pub async fn cleanup_expired(&self) {
        let now = SystemTime::now();

        let expired: Vec<_> = self.snapshots
            .iter()
            .filter(|r| {
                if let Some(ref retention) = r.retention {
                    if let Some(max_retention) = retention.max_retention {
                        if let Ok(age) = now.duration_since(r.created_at) {
                            return age > max_retention;
                        }
                    }
                }
                false
            })
            .map(|r| r.id)
            .collect();

        for snapshot_id in expired {
            if let Err(e) = self.delete_snapshot(snapshot_id).await {
                warn!(snapshot_id, error = %e, "Failed to cleanup expired snapshot");
            }
        }
    }

    /// Lock a snapshot
    pub fn lock_snapshot(&self, snapshot_id: SnapshotId) -> Result<(), String> {
        if let Some(mut snap) = self.snapshots.get_mut(&snapshot_id) {
            if snap.state == SnapshotState::Active {
                snap.state = SnapshotState::Locked;
                Ok(())
            } else {
                Err(format!("Cannot lock snapshot in state {:?}", snap.state))
            }
        } else {
            Err(format!("Snapshot {} not found", snapshot_id))
        }
    }

    /// Unlock a snapshot
    pub fn unlock_snapshot(&self, snapshot_id: SnapshotId) -> Result<(), String> {
        if let Some(mut snap) = self.snapshots.get_mut(&snapshot_id) {
            if snap.state == SnapshotState::Locked {
                snap.state = SnapshotState::Active;
                Ok(())
            } else {
                Err(format!("Snapshot {} is not locked", snapshot_id))
            }
        } else {
            Err(format!("Snapshot {} not found", snapshot_id))
        }
    }

    /// Get snapshot statistics
    pub fn stats(&self) -> SnapshotStats {
        self.stats.read().clone()
    }

    /// Get snapshot count
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }
}

/// Result of a restore operation
#[derive(Debug, Clone)]
pub struct RestoreResult {
    /// Source snapshot ID
    pub snapshot_id: SnapshotId,
    /// Target bucket
    pub target_bucket: String,
    /// Objects restored
    pub objects_restored: u64,
    /// Bytes restored
    pub bytes_restored: u64,
    /// Time taken
    pub duration: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_snapshot() {
        let manager = SnapshotManager::new(SnapshotConfig::default());

        let snapshot = manager.create_snapshot("test-bucket", "snap1", None).await.unwrap();

        assert_eq!(snapshot.bucket, "test-bucket");
        assert_eq!(snapshot.name, "snap1");
        assert_eq!(snapshot.state, SnapshotState::Active);
    }

    #[tokio::test]
    async fn test_delete_snapshot() {
        let manager = SnapshotManager::new(SnapshotConfig::default());

        let snapshot = manager.create_snapshot("test-bucket", "snap1", None).await.unwrap();
        let id = snapshot.id;

        assert!(manager.get_snapshot(id).is_some());

        manager.delete_snapshot(id).await.unwrap();

        assert!(manager.get_snapshot(id).is_none());
    }

    #[tokio::test]
    async fn test_list_snapshots() {
        let manager = SnapshotManager::new(SnapshotConfig::default());

        manager.create_snapshot("bucket1", "snap1", None).await.unwrap();
        manager.create_snapshot("bucket1", "snap2", None).await.unwrap();
        manager.create_snapshot("bucket2", "snap3", None).await.unwrap();

        let bucket1_snaps = manager.list_snapshots("bucket1");
        assert_eq!(bucket1_snaps.len(), 2);

        let bucket2_snaps = manager.list_snapshots("bucket2");
        assert_eq!(bucket2_snaps.len(), 1);
    }

    #[test]
    fn test_snapshot_schedule() {
        let mut schedule = SnapshotSchedule::new("daily", "my-bucket", Duration::from_secs(86400));

        assert!(schedule.is_due()); // Never run before

        schedule.mark_executed();
        assert!(!schedule.is_due()); // Just executed

        schedule.enabled = false;
        schedule.last_run = None;
        assert!(!schedule.is_due()); // Disabled
    }

    #[test]
    fn test_lock_unlock_snapshot() {
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let manager = SnapshotManager::new(SnapshotConfig::default());
            let snapshot = manager.create_snapshot("bucket", "snap", None).await.unwrap();
            let id = snapshot.id;

            // Lock
            manager.lock_snapshot(id).unwrap();
            let snap = manager.get_snapshot(id).unwrap();
            assert_eq!(snap.state, SnapshotState::Locked);

            // Can't delete locked snapshot
            assert!(manager.delete_snapshot(id).await.is_err());

            // Unlock
            manager.unlock_snapshot(id).unwrap();
            let snap = manager.get_snapshot(id).unwrap();
            assert_eq!(snap.state, SnapshotState::Active);

            // Now can delete
            assert!(manager.delete_snapshot(id).await.is_ok());
        });
    }
}
