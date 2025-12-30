//! Snapshot and Clone API for WARP Storage
//!
//! Provides point-in-time snapshots and efficient cloning using copy-on-write
//! semantics. Key features:
//!
//! - **Point-in-Time Snapshots**: Capture bucket/object state at a specific moment
//! - **Copy-on-Write**: Space-efficient snapshots that share unchanged data
//! - **Instant Clones**: Create writable copies without data duplication
//! - **Version Tracking**: Track data lineage and enable time-travel queries
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Snapshot Manager                         │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │                 COW Layer                            │   │
//! │  │  - Block reference counting                          │   │
//! │  │  - Lazy copy on write                                │   │
//! │  │  - Space-efficient storage                           │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │              Snapshot Index                          │   │
//! │  │  - Point-in-time metadata                            │   │
//! │  │  - Object version mapping                            │   │
//! │  │  - Snapshot hierarchy (parent/child)                 │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │              Clone Manager                           │   │
//! │  │  - Instant clone creation                            │   │
//! │  │  - Clone promotion to standalone                     │   │
//! │  │  - Delta tracking                                    │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod clone;
mod cow;
mod manager;

pub use clone::{CloneManager, CloneInfo, CloneConfig, CloneState, CloneHandle};
pub use cow::{CowBlock, CowManager, CowConfig, BlockRef, BlockState};
pub use manager::{
    SnapshotManager, SnapshotConfig, Snapshot, SnapshotId, SnapshotState,
    SnapshotPolicy, SnapshotSchedule, SnapshotStats,
};

use std::time::Duration;

/// Configuration for the snapshot system
#[derive(Debug, Clone)]
pub struct SnapshotSystemConfig {
    /// Configuration for the snapshot manager
    pub snapshot: SnapshotConfig,

    /// Configuration for the clone manager
    pub clone: CloneConfig,

    /// Configuration for copy-on-write blocks
    pub cow: CowConfig,

    /// Enable automatic snapshot pruning
    pub auto_prune: bool,

    /// Maximum snapshots per bucket
    pub max_snapshots_per_bucket: usize,

    /// Enable compression for snapshot metadata
    pub compress_metadata: bool,

    /// Snapshot index update interval
    pub index_sync_interval: Duration,
}

impl Default for SnapshotSystemConfig {
    fn default() -> Self {
        Self {
            snapshot: SnapshotConfig::default(),
            clone: CloneConfig::default(),
            cow: CowConfig::default(),
            auto_prune: true,
            max_snapshots_per_bucket: 256,
            compress_metadata: true,
            index_sync_interval: Duration::from_secs(60),
        }
    }
}

impl SnapshotSystemConfig {
    /// Create a high-performance config for frequent snapshots
    pub fn high_frequency() -> Self {
        Self {
            snapshot: SnapshotConfig {
                max_pending_snapshots: 100,
                snapshot_timeout: Duration::from_secs(30),
                ..Default::default()
            },
            cow: CowConfig {
                dedup_enabled: true,
                inline_data_threshold: 4096,
                ..Default::default()
            },
            auto_prune: true,
            max_snapshots_per_bucket: 1024,
            index_sync_interval: Duration::from_secs(10),
            ..Default::default()
        }
    }

    /// Create a config optimized for large objects
    pub fn large_objects() -> Self {
        Self {
            cow: CowConfig {
                block_size: 4 * 1024 * 1024, // 4MB blocks
                dedup_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Snapshot granularity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotGranularity {
    /// Snapshot entire bucket
    Bucket,
    /// Snapshot specific prefix
    Prefix,
    /// Snapshot single object
    Object,
}

/// Restore options for snapshots
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Target bucket for restore (None = restore in place)
    pub target_bucket: Option<String>,

    /// Target prefix for restore
    pub target_prefix: Option<String>,

    /// Whether to overwrite existing objects
    pub overwrite: bool,

    /// Restore only objects matching this pattern
    pub filter: Option<String>,

    /// Skip objects that haven't changed
    pub incremental: bool,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            target_bucket: None,
            target_prefix: None,
            overwrite: false,
            filter: None,
            incremental: true,
        }
    }
}

impl RestoreOptions {
    /// Create options for in-place restore
    pub fn in_place() -> Self {
        Self {
            overwrite: true,
            ..Default::default()
        }
    }

    /// Create options for restore to new bucket
    pub fn to_bucket(bucket: impl Into<String>) -> Self {
        Self {
            target_bucket: Some(bucket.into()),
            overwrite: false,
            ..Default::default()
        }
    }
}

/// Diff between two snapshots
#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    /// Source snapshot ID
    pub from: SnapshotId,

    /// Target snapshot ID
    pub to: SnapshotId,

    /// Added objects
    pub added: Vec<String>,

    /// Removed objects
    pub removed: Vec<String>,

    /// Modified objects
    pub modified: Vec<String>,

    /// Total bytes added
    pub bytes_added: u64,

    /// Total bytes removed
    pub bytes_removed: u64,
}

impl SnapshotDiff {
    /// Check if the snapshots are identical
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    /// Get total number of changes
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = SnapshotSystemConfig::default();
        assert!(config.auto_prune);
        assert_eq!(config.max_snapshots_per_bucket, 256);
    }

    #[test]
    fn test_restore_options() {
        let opts = RestoreOptions::in_place();
        assert!(opts.overwrite);
        assert!(opts.target_bucket.is_none());

        let opts = RestoreOptions::to_bucket("backup");
        assert!(!opts.overwrite);
        assert_eq!(opts.target_bucket, Some("backup".to_string()));
    }

    #[test]
    fn test_snapshot_diff() {
        let diff = SnapshotDiff {
            from: 1,
            to: 2,
            added: vec!["new.txt".to_string()],
            removed: vec![],
            modified: vec!["changed.txt".to_string()],
            bytes_added: 1000,
            bytes_removed: 0,
        };

        assert!(!diff.is_empty());
        assert_eq!(diff.change_count(), 2);
    }
}
