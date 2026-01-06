//! CRDT merge trait and utilities
//!
//! Provides a common interface for merging CRDTs and tracking merge statistics.

use serde::{Deserialize, Serialize};

/// Statistics from a merge operation
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MergeStats {
    /// Number of new elements added
    pub elements_added: usize,
    /// Number of elements removed
    pub elements_removed: usize,
    /// Number of elements updated
    pub elements_updated: usize,
    /// Number of conflicts resolved
    pub conflicts_resolved: usize,
}

impl MergeStats {
    /// Create empty stats
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if any changes occurred
    pub fn has_changes(&self) -> bool {
        self.elements_added > 0
            || self.elements_removed > 0
            || self.elements_updated > 0
            || self.conflicts_resolved > 0
    }

    /// Total number of changes
    pub fn total_changes(&self) -> usize {
        self.elements_added + self.elements_removed + self.elements_updated
    }

    /// Combine with another stats
    pub fn combine(&self, other: &Self) -> Self {
        Self {
            elements_added: self.elements_added + other.elements_added,
            elements_removed: self.elements_removed + other.elements_removed,
            elements_updated: self.elements_updated + other.elements_updated,
            conflicts_resolved: self.conflicts_resolved + other.conflicts_resolved,
        }
    }
}

impl std::ops::Add for MergeStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.combine(&other)
    }
}

impl std::ops::AddAssign for MergeStats {
    fn add_assign(&mut self, other: Self) {
        *self = self.combine(&other);
    }
}

/// Trait for CRDT merge operations
///
/// All CRDTs implement this trait for consistent merge behavior.
pub trait CrdtMerge {
    /// Merge another instance into self
    ///
    /// Returns statistics about the merge operation.
    fn merge_from(&mut self, other: &Self) -> MergeStats;
}

/// Merge multiple CRDTs into one
///
/// Takes a mutable base and merges all others into it.
pub fn merge_all<T: CrdtMerge>(base: &mut T, others: impl IntoIterator<Item = T>) -> MergeStats {
    let mut total = MergeStats::empty();
    for other in others {
        total += base.merge_from(&other);
    }
    total
}

/// Merge result with optional conflict info
#[derive(Debug, Clone)]
pub struct MergeResult<T> {
    /// The merged value
    pub value: T,
    /// Merge statistics
    pub stats: MergeStats,
    /// Any conflicts that were auto-resolved
    pub auto_resolved_conflicts: Vec<ConflictResolution>,
}

/// Information about a conflict that was auto-resolved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// Resolution strategy used
    pub strategy: ResolutionStrategy,
    /// Description of what was resolved
    pub description: String,
}

/// Types of conflicts that can occur
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictType {
    /// Concurrent writes to same key
    ConcurrentWrite,
    /// Concurrent add and remove
    AddRemove,
    /// Counter overflow
    Overflow,
    /// Clock skew detected
    ClockSkew,
}

/// Strategies for resolving conflicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    /// Last write wins (by timestamp)
    LastWriteWins,
    /// Highest value wins
    MaxWins,
    /// Add wins over remove
    AddWins,
    /// First write wins
    FirstWriteWins,
    /// Custom resolution function
    Custom,
}

/// Builder for tracking merge operations
pub struct MergeTracker {
    stats: MergeStats,
    conflicts: Vec<ConflictResolution>,
}

impl MergeTracker {
    /// Create a new merge tracker
    pub fn new() -> Self {
        Self {
            stats: MergeStats::empty(),
            conflicts: Vec::new(),
        }
    }

    /// Record an element addition
    pub fn record_add(&mut self) {
        self.stats.elements_added += 1;
    }

    /// Record an element removal
    pub fn record_remove(&mut self) {
        self.stats.elements_removed += 1;
    }

    /// Record an element update
    pub fn record_update(&mut self) {
        self.stats.elements_updated += 1;
    }

    /// Record a conflict resolution
    pub fn record_conflict(
        &mut self,
        conflict_type: ConflictType,
        strategy: ResolutionStrategy,
        description: impl Into<String>,
    ) {
        self.stats.conflicts_resolved += 1;
        self.conflicts.push(ConflictResolution {
            conflict_type,
            strategy,
            description: description.into(),
        });
    }

    /// Get the statistics
    pub fn stats(&self) -> MergeStats {
        self.stats
    }

    /// Get recorded conflicts
    pub fn conflicts(&self) -> &[ConflictResolution] {
        &self.conflicts
    }

    /// Finish tracking and return stats
    pub fn finish(self) -> MergeStats {
        self.stats
    }
}

impl Default for MergeTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_stats_empty() {
        let stats = MergeStats::empty();
        assert!(!stats.has_changes());
        assert_eq!(stats.total_changes(), 0);
    }

    #[test]
    fn test_merge_stats_has_changes() {
        let stats = MergeStats {
            elements_added: 1,
            ..Default::default()
        };
        assert!(stats.has_changes());
    }

    #[test]
    fn test_merge_stats_combine() {
        let a = MergeStats {
            elements_added: 1,
            elements_removed: 2,
            elements_updated: 0,
            conflicts_resolved: 1,
        };

        let b = MergeStats {
            elements_added: 3,
            elements_removed: 0,
            elements_updated: 2,
            conflicts_resolved: 0,
        };

        let combined = a.combine(&b);
        assert_eq!(combined.elements_added, 4);
        assert_eq!(combined.elements_removed, 2);
        assert_eq!(combined.elements_updated, 2);
        assert_eq!(combined.conflicts_resolved, 1);
    }

    #[test]
    fn test_merge_stats_add() {
        let mut stats = MergeStats::empty();
        stats += MergeStats {
            elements_added: 5,
            ..Default::default()
        };
        assert_eq!(stats.elements_added, 5);
    }

    #[test]
    fn test_merge_tracker() {
        let mut tracker = MergeTracker::new();

        tracker.record_add();
        tracker.record_add();
        tracker.record_remove();
        tracker.record_conflict(
            ConflictType::ConcurrentWrite,
            ResolutionStrategy::LastWriteWins,
            "key 'foo' had concurrent writes",
        );

        let stats = tracker.stats();
        assert_eq!(stats.elements_added, 2);
        assert_eq!(stats.elements_removed, 1);
        assert_eq!(stats.conflicts_resolved, 1);
        assert_eq!(tracker.conflicts().len(), 1);
    }

    #[test]
    fn test_conflict_resolution() {
        let resolution = ConflictResolution {
            conflict_type: ConflictType::AddRemove,
            strategy: ResolutionStrategy::AddWins,
            description: "Element 'x' was added and removed concurrently".to_string(),
        };

        assert_eq!(resolution.conflict_type, ConflictType::AddRemove);
        assert_eq!(resolution.strategy, ResolutionStrategy::AddWins);
    }
}
