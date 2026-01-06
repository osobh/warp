//! Grow-only Counter (G-Counter)
//!
//! A counter that can only be incremented. Each node maintains its own
//! count, and the total is the sum of all node counts.
//!
//! # Properties
//!
//! - Monotonically increasing
//! - Commutative, associative, idempotent merge
//! - Efficient: O(n) space for n nodes
//!
//! # Use Cases
//!
//! - Event counters
//! - Total downloads
//! - Page views
//! - Any counter that never decrements

use crate::{CrdtMerge, MergeStats, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Grow-only Counter CRDT
///
/// Each node has its own count. The total value is the sum of all counts.
/// Merging takes the max of each node's count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCounter {
    /// Per-node counts
    counts: HashMap<NodeId, u64>,
    /// Local node ID
    node_id: NodeId,
}

impl GCounter {
    /// Create a new counter for the given node
    pub fn new(node_id: NodeId) -> Self {
        Self {
            counts: HashMap::new(),
            node_id,
        }
    }

    /// Increment the counter by 1
    pub fn increment(&mut self) {
        self.increment_by(1);
    }

    /// Increment the counter by a specific amount
    pub fn increment_by(&mut self, amount: u64) {
        *self.counts.entry(self.node_id).or_insert(0) += amount;
    }

    /// Get the total value (sum of all node counts)
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Get this node's contribution to the counter
    pub fn local_value(&self) -> u64 {
        self.counts.get(&self.node_id).copied().unwrap_or(0)
    }

    /// Get a specific node's contribution
    pub fn node_value(&self, node_id: NodeId) -> u64 {
        self.counts.get(&node_id).copied().unwrap_or(0)
    }

    /// Merge with another counter
    ///
    /// Takes the max of each node's count.
    pub fn merge(&mut self, other: &Self) {
        for (&node_id, &count) in &other.counts {
            let local = self.counts.entry(node_id).or_insert(0);
            *local = (*local).max(count);
        }
    }

    /// Get the number of nodes that have contributed
    pub fn num_contributors(&self) -> usize {
        self.counts.len()
    }

    /// Get all node counts (for debugging/sync)
    pub fn node_counts(&self) -> &HashMap<NodeId, u64> {
        &self.counts
    }

    /// Get the node ID of this counter
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Reset local counter to zero (doesn't affect other nodes)
    ///
    /// Note: This breaks the monotonic property and should be used carefully.
    /// Consider creating a new counter instead.
    pub fn reset_local(&mut self) {
        self.counts.remove(&self.node_id);
    }

    /// Check if this counter dominates another
    ///
    /// A dominates B if all of A's counts are >= B's counts.
    pub fn dominates(&self, other: &Self) -> bool {
        for (&node_id, &count) in &other.counts {
            if self.counts.get(&node_id).copied().unwrap_or(0) < count {
                return false;
            }
        }
        true
    }

    /// Check if two counters are identical
    pub fn equals(&self, other: &Self) -> bool {
        self.dominates(other) && other.dominates(self)
    }
}

impl CrdtMerge for GCounter {
    fn merge_from(&mut self, other: &Self) -> MergeStats {
        let before = self.value();
        self.merge(other);
        let after = self.value();

        MergeStats {
            elements_added: 0,
            elements_removed: 0,
            elements_updated: (after - before) as usize,
            conflicts_resolved: 0,
        }
    }
}

impl Default for GCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl PartialEq for GCounter {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl Eq for GCounter {}

/// Bounded G-Counter with maximum value
///
/// A G-Counter that stops incrementing when it reaches a maximum value.
/// Useful for rate limiting or capped counters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedGCounter {
    /// The underlying counter
    counter: GCounter,
    /// Maximum total value
    max_value: u64,
}

impl BoundedGCounter {
    /// Create a new bounded counter
    pub fn new(node_id: NodeId, max_value: u64) -> Self {
        Self {
            counter: GCounter::new(node_id),
            max_value,
        }
    }

    /// Try to increment the counter
    ///
    /// Returns true if increment was allowed, false if at max.
    pub fn try_increment(&mut self) -> bool {
        if self.counter.value() < self.max_value {
            self.counter.increment();
            true
        } else {
            false
        }
    }

    /// Try to increment by a specific amount
    ///
    /// Returns the amount actually incremented (may be less if hitting max).
    pub fn try_increment_by(&mut self, amount: u64) -> u64 {
        let current = self.counter.value();
        let available = self.max_value.saturating_sub(current);
        let actual = amount.min(available);

        if actual > 0 {
            self.counter.increment_by(actual);
        }
        actual
    }

    /// Get the current value
    pub fn value(&self) -> u64 {
        self.counter.value()
    }

    /// Get the maximum value
    pub fn max_value(&self) -> u64 {
        self.max_value
    }

    /// Get remaining capacity
    pub fn remaining(&self) -> u64 {
        self.max_value.saturating_sub(self.counter.value())
    }

    /// Check if counter is at max
    pub fn is_at_max(&self) -> bool {
        self.counter.value() >= self.max_value
    }

    /// Merge with another bounded counter
    pub fn merge(&mut self, other: &Self) {
        self.counter.merge(&other.counter);
        // Note: max_value is not merged - it's configuration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_g_counter_basic() {
        let mut counter = GCounter::new(1);
        assert_eq!(counter.value(), 0);

        counter.increment();
        assert_eq!(counter.value(), 1);

        counter.increment_by(5);
        assert_eq!(counter.value(), 6);
    }

    #[test]
    fn test_g_counter_local_value() {
        let mut counter = GCounter::new(1);
        counter.increment_by(10);

        assert_eq!(counter.local_value(), 10);
        assert_eq!(counter.node_value(1), 10);
        assert_eq!(counter.node_value(2), 0);
    }

    #[test]
    fn test_g_counter_merge() {
        let mut counter_a = GCounter::new(1);
        let mut counter_b = GCounter::new(2);

        counter_a.increment_by(5);
        counter_b.increment_by(3);

        counter_a.merge(&counter_b);

        assert_eq!(counter_a.value(), 8); // 5 + 3
        assert_eq!(counter_a.node_value(1), 5);
        assert_eq!(counter_a.node_value(2), 3);
    }

    #[test]
    fn test_g_counter_merge_same_node() {
        let mut counter_a = GCounter::new(1);
        let mut counter_b = GCounter::new(1);

        counter_a.increment_by(5);
        counter_b.increment_by(3);

        counter_a.merge(&counter_b);

        // Should take max, not sum
        assert_eq!(counter_a.value(), 5);
    }

    #[test]
    fn test_g_counter_convergence() {
        let mut counter_a = GCounter::new(1);
        let mut counter_b = GCounter::new(2);
        let mut counter_c = GCounter::new(3);

        counter_a.increment_by(5);
        counter_b.increment_by(3);
        counter_c.increment_by(7);

        // Different merge orders should converge
        let mut result1 = counter_a.clone();
        result1.merge(&counter_b);
        result1.merge(&counter_c);

        let mut result2 = counter_c.clone();
        result2.merge(&counter_a);
        result2.merge(&counter_b);

        assert_eq!(result1.value(), result2.value());
        assert_eq!(result1.value(), 15);
    }

    #[test]
    fn test_g_counter_idempotent() {
        let mut counter_a = GCounter::new(1);
        let counter_b = GCounter::new(2);

        counter_a.increment_by(5);

        let before = counter_a.value();
        counter_a.merge(&counter_b);
        counter_a.merge(&counter_b);
        counter_a.merge(&counter_b);

        assert_eq!(counter_a.value(), before);
    }

    #[test]
    fn test_g_counter_dominates() {
        let mut counter_a = GCounter::new(1);
        let mut counter_b = GCounter::new(1);

        counter_a.increment_by(10);
        counter_b.increment_by(5);

        assert!(counter_a.dominates(&counter_b));
        assert!(!counter_b.dominates(&counter_a));
    }

    #[test]
    fn test_g_counter_equals() {
        let mut counter_a = GCounter::new(1);
        let mut counter_b = GCounter::new(1);

        counter_a.increment_by(5);
        counter_b.increment_by(5);

        assert!(counter_a.equals(&counter_b));
        assert_eq!(counter_a, counter_b);
    }

    #[test]
    fn test_bounded_counter() {
        let mut counter = BoundedGCounter::new(1, 10);

        assert!(counter.try_increment());
        assert_eq!(counter.value(), 1);

        let added = counter.try_increment_by(100);
        assert_eq!(added, 9); // Only 9 remaining
        assert_eq!(counter.value(), 10);

        assert!(!counter.try_increment());
        assert!(counter.is_at_max());
    }

    #[test]
    fn test_bounded_counter_remaining() {
        let mut counter = BoundedGCounter::new(1, 100);
        counter.try_increment_by(30);

        assert_eq!(counter.remaining(), 70);
    }

    #[test]
    fn test_serialization() {
        let mut counter = GCounter::new(1);
        counter.increment_by(42);

        let json = serde_json::to_string(&counter).unwrap();
        let deserialized: GCounter = serde_json::from_str(&json).unwrap();

        assert_eq!(counter.value(), deserialized.value());
    }

    #[test]
    fn test_crdt_merge_trait() {
        let mut counter_a = GCounter::new(1);
        let mut counter_b = GCounter::new(2);

        counter_a.increment_by(5);
        counter_b.increment_by(10);

        let stats = counter_a.merge_from(&counter_b);
        assert_eq!(stats.elements_updated, 10);
        assert_eq!(counter_a.value(), 15);
    }
}
