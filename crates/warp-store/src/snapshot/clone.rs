//! Clone management for instant, space-efficient copies

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use super::cow::{BlockRef, CowManager};
use super::manager::{Snapshot, SnapshotId};

/// Unique identifier for a clone
pub type CloneId = u64;

/// State of a clone
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneState {
    /// Clone is being created
    Creating,
    /// Clone is active and writable
    Active,
    /// Clone is read-only
    ReadOnly,
    /// Clone is being promoted to standalone
    Promoting,
    /// Clone is now a standalone bucket (no longer a clone)
    Promoted,
    /// Clone is being deleted
    Deleting,
}

/// Information about a clone
#[derive(Debug, Clone)]
pub struct CloneInfo {
    /// Clone ID
    pub id: CloneId,

    /// Clone name
    pub name: String,

    /// Source snapshot ID
    pub source_snapshot: SnapshotId,

    /// Target bucket name
    pub target_bucket: String,

    /// Clone state
    pub state: CloneState,

    /// When the clone was created
    pub created_at: SystemTime,

    /// Objects inherited from source (COW references)
    pub inherited_objects: u64,

    /// Objects modified since clone creation
    pub modified_objects: u64,

    /// Objects added since clone creation
    pub added_objects: u64,

    /// Objects deleted since clone creation
    pub deleted_objects: u64,

    /// Total unique bytes (written after clone)
    pub unique_bytes: u64,

    /// Labels/tags
    pub labels: HashMap<String, String>,
}

impl CloneInfo {
    /// Calculate space efficiency (% of data shared with source)
    pub fn space_efficiency(&self) -> f64 {
        let total = self.inherited_objects + self.added_objects;
        if total == 0 {
            return 0.0;
        }
        (self.inherited_objects as f64 / total as f64) * 100.0
    }

    /// Get total object count
    pub fn total_objects(&self) -> u64 {
        self.inherited_objects + self.added_objects - self.deleted_objects
    }

    /// Check if clone has diverged significantly
    pub fn is_heavily_modified(&self, threshold: f64) -> bool {
        let total = self.inherited_objects + self.added_objects;
        if total == 0 {
            return false;
        }
        let modified_ratio = (self.modified_objects + self.added_objects) as f64 / total as f64;
        modified_ratio > threshold
    }
}

/// Configuration for clone manager
#[derive(Debug, Clone)]
pub struct CloneConfig {
    /// Maximum clones per snapshot
    pub max_clones_per_snapshot: usize,

    /// Maximum total clones
    pub max_total_clones: usize,

    /// Auto-promote threshold (promote to standalone when this % modified)
    pub auto_promote_threshold: Option<f64>,

    /// Enable thin provisioning
    pub thin_provisioning: bool,

    /// Track object-level changes
    pub track_changes: bool,
}

impl Default for CloneConfig {
    fn default() -> Self {
        Self {
            max_clones_per_snapshot: 64,
            max_total_clones: 1024,
            auto_promote_threshold: None, // Don't auto-promote
            thin_provisioning: true,
            track_changes: true,
        }
    }
}

/// Handle for working with a clone
#[derive(Debug, Clone)]
pub struct CloneHandle {
    /// Clone ID
    pub id: CloneId,
    /// Target bucket name
    pub bucket: String,
}

impl CloneHandle {
    /// Create a new handle
    pub fn new(id: CloneId, bucket: impl Into<String>) -> Self {
        Self {
            id,
            bucket: bucket.into(),
        }
    }
}

/// Statistics for clone operations
#[derive(Debug, Clone, Default)]
pub struct CloneStats {
    /// Total clones created
    pub clones_created: u64,
    /// Total clones deleted
    pub clones_deleted: u64,
    /// Total clones promoted
    pub clones_promoted: u64,
    /// Current active clones
    pub active_clones: u64,
    /// Total unique bytes across all clones
    pub total_unique_bytes: u64,
    /// Bytes saved by COW
    pub bytes_saved_by_cow: u64,
}

/// Manages clone creation and lifecycle
pub struct CloneManager {
    /// Configuration
    config: CloneConfig,

    /// All clones by ID
    clones: DashMap<CloneId, CloneInfo>,

    /// Clones by source snapshot
    snapshot_clones: DashMap<SnapshotId, HashSet<CloneId>>,

    /// Clones by target bucket
    bucket_clones: DashMap<String, CloneId>,

    /// COW manager reference
    cow_manager: Arc<CowManager>,

    /// Object overrides (bucket -> key -> block refs)
    object_overrides: DashMap<CloneId, HashMap<String, Vec<BlockRef>>>,

    /// Deleted objects (bucket -> set of keys)
    deleted_objects: DashMap<CloneId, HashSet<String>>,

    /// Statistics
    stats: RwLock<CloneStats>,

    /// Next clone ID
    next_id: AtomicU64,
}

impl CloneManager {
    /// Create a new clone manager
    pub fn new(config: CloneConfig, cow_manager: Arc<CowManager>) -> Self {
        Self {
            config,
            clones: DashMap::new(),
            snapshot_clones: DashMap::new(),
            bucket_clones: DashMap::new(),
            cow_manager,
            object_overrides: DashMap::new(),
            deleted_objects: DashMap::new(),
            stats: RwLock::new(CloneStats::default()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Create a clone from a snapshot
    pub async fn create_clone(
        &self,
        snapshot: &Snapshot,
        target_bucket: &str,
        name: &str,
    ) -> Result<CloneHandle, String> {
        // Check limits
        let current_clones = self
            .snapshot_clones
            .get(&snapshot.id)
            .map(|s| s.len())
            .unwrap_or(0);

        if current_clones >= self.config.max_clones_per_snapshot {
            return Err(format!(
                "Snapshot {} has reached max clones ({})",
                snapshot.id, self.config.max_clones_per_snapshot
            ));
        }

        if self.clones.len() >= self.config.max_total_clones {
            return Err(format!(
                "Max total clones reached ({})",
                self.config.max_total_clones
            ));
        }

        // Check if target bucket is already a clone
        if self.bucket_clones.contains_key(target_bucket) {
            return Err(format!("Bucket {} is already a clone", target_bucket));
        }

        info!(
            snapshot_id = snapshot.id,
            target_bucket, name, "Creating clone"
        );

        let clone_id = self.next_id.fetch_add(1, Ordering::SeqCst);

        // Create clone info
        let clone_info = CloneInfo {
            id: clone_id,
            name: name.to_string(),
            source_snapshot: snapshot.id,
            target_bucket: target_bucket.to_string(),
            state: CloneState::Active,
            created_at: SystemTime::now(),
            inherited_objects: snapshot.object_count,
            modified_objects: 0,
            added_objects: 0,
            deleted_objects: 0,
            unique_bytes: 0,
            labels: HashMap::new(),
        };

        // Store clone info
        self.clones.insert(clone_id, clone_info);

        // Index by snapshot
        self.snapshot_clones
            .entry(snapshot.id)
            .or_insert_with(HashSet::new)
            .insert(clone_id);

        // Index by bucket
        self.bucket_clones
            .insert(target_bucket.to_string(), clone_id);

        // Initialize override tracking
        self.object_overrides.insert(clone_id, HashMap::new());
        self.deleted_objects.insert(clone_id, HashSet::new());

        // Add references to all source blocks
        for (_key, version_ref) in &snapshot.object_versions {
            for block_id in &version_ref.blocks {
                self.cow_manager.add_ref(*block_id)?;
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.clones_created += 1;
            stats.active_clones += 1;
        }

        info!(
            clone_id,
            target_bucket,
            inherited_objects = snapshot.object_count,
            "Clone created"
        );

        Ok(CloneHandle::new(clone_id, target_bucket))
    }

    /// Record a write to a clone (COW operation)
    pub fn record_write(
        &self,
        clone_id: CloneId,
        object_key: &str,
        blocks: Vec<BlockRef>,
        bytes: u64,
        is_new: bool,
    ) -> Result<(), String> {
        // Update clone info
        if let Some(mut clone) = self.clones.get_mut(&clone_id) {
            if is_new {
                clone.added_objects += 1;
            } else {
                clone.modified_objects += 1;
            }
            clone.unique_bytes += bytes;

            // Check auto-promote threshold
            if let Some(threshold) = self.config.auto_promote_threshold {
                if clone.is_heavily_modified(threshold) {
                    debug!(
                        clone_id,
                        "Clone exceeds modification threshold, consider promotion"
                    );
                }
            }
        }

        // Store override
        if self.config.track_changes {
            if let Some(mut overrides) = self.object_overrides.get_mut(&clone_id) {
                overrides.insert(object_key.to_string(), blocks);
            }
        }

        // Remove from deleted set if was deleted
        if let Some(mut deleted) = self.deleted_objects.get_mut(&clone_id) {
            deleted.remove(object_key);
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.total_unique_bytes += bytes;
        }

        Ok(())
    }

    /// Record a delete in a clone
    pub fn record_delete(&self, clone_id: CloneId, object_key: &str) -> Result<(), String> {
        // Update clone info
        if let Some(mut clone) = self.clones.get_mut(&clone_id) {
            clone.deleted_objects += 1;
        }

        // Track deletion
        if self.config.track_changes {
            // Remove from overrides
            if let Some(mut overrides) = self.object_overrides.get_mut(&clone_id) {
                overrides.remove(object_key);
            }

            // Add to deleted set
            if let Some(mut deleted) = self.deleted_objects.get_mut(&clone_id) {
                deleted.insert(object_key.to_string());
            }
        }

        Ok(())
    }

    /// Check if an object exists in a clone
    pub fn object_exists(&self, clone_id: CloneId, object_key: &str) -> Option<bool> {
        // Check if deleted
        if let Some(deleted) = self.deleted_objects.get(&clone_id) {
            if deleted.contains(object_key) {
                return Some(false);
            }
        }

        // Check if overridden (definitely exists)
        if let Some(overrides) = self.object_overrides.get(&clone_id) {
            if overrides.contains_key(object_key) {
                return Some(true);
            }
        }

        // Falls back to source snapshot - return None to indicate check source
        None
    }

    /// Get object blocks for a clone
    pub fn get_object_blocks(&self, clone_id: CloneId, object_key: &str) -> Option<Vec<BlockRef>> {
        // Check overrides first
        if let Some(overrides) = self.object_overrides.get(&clone_id) {
            if let Some(blocks) = overrides.get(object_key) {
                return Some(blocks.clone());
            }
        }

        // Not overridden - will need to read from source snapshot
        None
    }

    /// Promote a clone to a standalone bucket
    pub async fn promote_clone(&self, clone_id: CloneId) -> Result<(), String> {
        // Update state
        {
            let mut clone = self
                .clones
                .get_mut(&clone_id)
                .ok_or_else(|| format!("Clone {} not found", clone_id))?;

            if clone.state != CloneState::Active {
                return Err(format!("Clone {} is not active", clone_id));
            }

            clone.state = CloneState::Promoting;
        }

        info!(clone_id, "Promoting clone to standalone bucket");

        // In a real implementation, we would:
        // 1. Copy all inherited objects that haven't been overridden
        // 2. Update block references to be owned by the new bucket
        // 3. Remove COW links

        // Simulate promotion
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Mark as promoted
        if let Some(mut clone) = self.clones.get_mut(&clone_id) {
            clone.state = CloneState::Promoted;
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.clones_promoted += 1;
            stats.active_clones = stats.active_clones.saturating_sub(1);
        }

        info!(clone_id, "Clone promoted to standalone bucket");

        Ok(())
    }

    /// Delete a clone
    pub async fn delete_clone(&self, clone_id: CloneId) -> Result<(), String> {
        let clone = self
            .clones
            .get(&clone_id)
            .ok_or_else(|| format!("Clone {} not found", clone_id))?;

        if clone.state == CloneState::Deleting {
            return Err(format!("Clone {} is already being deleted", clone_id));
        }

        let target_bucket = clone.target_bucket.clone();
        let snapshot_id = clone.source_snapshot;
        drop(clone);

        // Mark as deleting
        if let Some(mut clone) = self.clones.get_mut(&clone_id) {
            clone.state = CloneState::Deleting;
        }

        info!(clone_id, "Deleting clone");

        // Release block references for overrides
        if let Some((_, overrides)) = self.object_overrides.remove(&clone_id) {
            for (_, blocks) in overrides {
                for block_ref in blocks {
                    let _ = self.cow_manager.release_ref(block_ref.block_id);
                }
            }
        }

        // Clean up tracking
        self.deleted_objects.remove(&clone_id);

        // Remove from indexes
        self.clones.remove(&clone_id);
        self.bucket_clones.remove(&target_bucket);

        if let Some(mut snapshot_clones) = self.snapshot_clones.get_mut(&snapshot_id) {
            snapshot_clones.remove(&clone_id);
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.clones_deleted += 1;
            stats.active_clones = stats.active_clones.saturating_sub(1);
        }

        info!(clone_id, "Clone deleted");

        Ok(())
    }

    /// Get clone info by ID
    pub fn get_clone(&self, clone_id: CloneId) -> Option<CloneInfo> {
        self.clones.get(&clone_id).map(|c| c.clone())
    }

    /// Get clone by bucket name
    pub fn get_clone_by_bucket(&self, bucket: &str) -> Option<CloneInfo> {
        self.bucket_clones
            .get(bucket)
            .and_then(|id| self.clones.get(&id).map(|c| c.clone()))
    }

    /// List clones for a snapshot
    pub fn list_clones_for_snapshot(&self, snapshot_id: SnapshotId) -> Vec<CloneInfo> {
        self.snapshot_clones
            .get(&snapshot_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.clones.get(id).map(|c| c.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all clones
    pub fn list_all_clones(&self) -> Vec<CloneInfo> {
        self.clones.iter().map(|r| r.value().clone()).collect()
    }

    /// Check if bucket is a clone
    pub fn is_clone(&self, bucket: &str) -> bool {
        self.bucket_clones.contains_key(bucket)
    }

    /// Get clone statistics
    pub fn stats(&self) -> CloneStats {
        self.stats.read().clone()
    }

    /// Get clone count
    pub fn clone_count(&self) -> usize {
        self.clones.len()
    }

    /// Make a clone read-only
    pub fn set_readonly(&self, clone_id: CloneId) -> Result<(), String> {
        if let Some(mut clone) = self.clones.get_mut(&clone_id) {
            if clone.state == CloneState::Active {
                clone.state = CloneState::ReadOnly;
                Ok(())
            } else {
                Err(format!("Clone {} is not active", clone_id))
            }
        } else {
            Err(format!("Clone {} not found", clone_id))
        }
    }

    /// Make a clone writable again
    pub fn set_writable(&self, clone_id: CloneId) -> Result<(), String> {
        if let Some(mut clone) = self.clones.get_mut(&clone_id) {
            if clone.state == CloneState::ReadOnly {
                clone.state = CloneState::Active;
                Ok(())
            } else {
                Err(format!("Clone {} is not read-only", clone_id))
            }
        } else {
            Err(format!("Clone {} not found", clone_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SnapshotGranularity;
    use crate::snapshot::cow::CowConfig;

    fn create_test_snapshot(id: SnapshotId) -> Snapshot {
        Snapshot {
            id,
            name: format!("test-snapshot-{}", id),
            description: None,
            bucket: "source-bucket".to_string(),
            prefix: None,
            state: super::super::manager::SnapshotState::Active,
            created_at: SystemTime::now(),
            completed_at: Some(SystemTime::now()),
            parent_id: None,
            object_count: 10,
            total_bytes: 1000,
            unique_bytes: 1000,
            labels: HashMap::new(),
            retention: None,
            granularity: SnapshotGranularity::Bucket,
            object_versions: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_create_clone() {
        let cow_manager = Arc::new(CowManager::new(CowConfig::default()));
        let clone_manager = CloneManager::new(CloneConfig::default(), cow_manager);

        let snapshot = create_test_snapshot(1);
        let handle = clone_manager
            .create_clone(&snapshot, "clone-bucket", "test-clone")
            .await
            .unwrap();

        assert_eq!(handle.bucket, "clone-bucket");

        let clone = clone_manager.get_clone(handle.id).unwrap();
        assert_eq!(clone.name, "test-clone");
        assert_eq!(clone.state, CloneState::Active);
        assert_eq!(clone.inherited_objects, 10);
    }

    #[tokio::test]
    async fn test_clone_write_tracking() {
        let cow_manager = Arc::new(CowManager::new(CowConfig::default()));
        let clone_manager = CloneManager::new(CloneConfig::default(), cow_manager);

        let snapshot = create_test_snapshot(1);
        let handle = clone_manager
            .create_clone(&snapshot, "clone-bucket", "test-clone")
            .await
            .unwrap();

        // Record a new object
        clone_manager
            .record_write(handle.id, "new-object.txt", vec![], 100, true)
            .unwrap();

        let clone = clone_manager.get_clone(handle.id).unwrap();
        assert_eq!(clone.added_objects, 1);
        assert_eq!(clone.unique_bytes, 100);

        // Record a modification
        clone_manager
            .record_write(handle.id, "existing.txt", vec![], 50, false)
            .unwrap();

        let clone = clone_manager.get_clone(handle.id).unwrap();
        assert_eq!(clone.modified_objects, 1);
    }

    #[tokio::test]
    async fn test_clone_delete() {
        let cow_manager = Arc::new(CowManager::new(CowConfig::default()));
        let clone_manager = CloneManager::new(CloneConfig::default(), cow_manager);

        let snapshot = create_test_snapshot(1);
        let handle = clone_manager
            .create_clone(&snapshot, "clone-bucket", "test-clone")
            .await
            .unwrap();

        assert!(clone_manager.is_clone("clone-bucket"));

        clone_manager.delete_clone(handle.id).await.unwrap();

        assert!(!clone_manager.is_clone("clone-bucket"));
        assert!(clone_manager.get_clone(handle.id).is_none());
    }

    #[tokio::test]
    async fn test_promote_clone() {
        let cow_manager = Arc::new(CowManager::new(CowConfig::default()));
        let clone_manager = CloneManager::new(CloneConfig::default(), cow_manager);

        let snapshot = create_test_snapshot(1);
        let handle = clone_manager
            .create_clone(&snapshot, "clone-bucket", "test-clone")
            .await
            .unwrap();

        clone_manager.promote_clone(handle.id).await.unwrap();

        let clone = clone_manager.get_clone(handle.id).unwrap();
        assert_eq!(clone.state, CloneState::Promoted);
    }

    #[test]
    fn test_clone_space_efficiency() {
        let clone = CloneInfo {
            id: 1,
            name: "test".to_string(),
            source_snapshot: 1,
            target_bucket: "clone".to_string(),
            state: CloneState::Active,
            created_at: SystemTime::now(),
            inherited_objects: 90,
            modified_objects: 5,
            added_objects: 10,
            deleted_objects: 0,
            unique_bytes: 1000,
            labels: HashMap::new(),
        };

        // 90 inherited out of 100 total = 90% efficiency
        assert!((clone.space_efficiency() - 90.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_readonly_clone() {
        let cow_manager = Arc::new(CowManager::new(CowConfig::default()));
        let clone_manager = CloneManager::new(CloneConfig::default(), cow_manager);

        let snapshot = create_test_snapshot(1);
        let handle = clone_manager
            .create_clone(&snapshot, "clone-bucket", "test-clone")
            .await
            .unwrap();

        clone_manager.set_readonly(handle.id).unwrap();

        let clone = clone_manager.get_clone(handle.id).unwrap();
        assert_eq!(clone.state, CloneState::ReadOnly);

        clone_manager.set_writable(handle.id).unwrap();

        let clone = clone_manager.get_clone(handle.id).unwrap();
        assert_eq!(clone.state, CloneState::Active);
    }
}
