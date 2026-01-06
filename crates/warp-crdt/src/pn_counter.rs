//! Positive-Negative Counter (PN-Counter)
//!
//! A counter that supports both increment and decrement operations.
//! Implemented as two G-Counters: one for increments, one for decrements.
//!
//! # Properties
//!
//! - Supports both increment and decrement
//! - Commutative, associative, idempotent merge
//! - Can go negative if more decrements than increments
//!
//! # Use Cases
//!
//! - Inventory counts
//! - Vote counters (upvotes/downvotes)
//! - Balance tracking
//! - Any counter needing both operations

use crate::{CrdtMerge, GCounter, MergeStats, NodeId};
use serde::{Deserialize, Serialize};

/// Positive-Negative Counter CRDT
///
/// Uses two G-Counters internally: one for positive (increments)
/// and one for negative (decrements). The value is positive - negative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PNCounter {
    /// Counter for increments
    positive: GCounter,
    /// Counter for decrements
    negative: GCounter,
}

impl PNCounter {
    /// Create a new counter for the given node
    pub fn new(node_id: NodeId) -> Self {
        Self {
            positive: GCounter::new(node_id),
            negative: GCounter::new(node_id),
        }
    }

    /// Increment the counter by 1
    pub fn increment(&mut self) {
        self.positive.increment();
    }

    /// Increment the counter by a specific amount
    pub fn increment_by(&mut self, amount: u64) {
        self.positive.increment_by(amount);
    }

    /// Decrement the counter by 1
    pub fn decrement(&mut self) {
        self.negative.increment();
    }

    /// Decrement the counter by a specific amount
    pub fn decrement_by(&mut self, amount: u64) {
        self.negative.increment_by(amount);
    }

    /// Get the current value (may be negative)
    pub fn value(&self) -> i64 {
        self.positive.value() as i64 - self.negative.value() as i64
    }

    /// Get the total number of increments
    pub fn total_increments(&self) -> u64 {
        self.positive.value()
    }

    /// Get the total number of decrements
    pub fn total_decrements(&self) -> u64 {
        self.negative.value()
    }

    /// Get this node's net contribution
    pub fn local_value(&self) -> i64 {
        self.positive.local_value() as i64 - self.negative.local_value() as i64
    }

    /// Get a specific node's net contribution
    pub fn node_value(&self, node_id: NodeId) -> i64 {
        self.positive.node_value(node_id) as i64 - self.negative.node_value(node_id) as i64
    }

    /// Merge with another counter
    pub fn merge(&mut self, other: &Self) {
        self.positive.merge(&other.positive);
        self.negative.merge(&other.negative);
    }

    /// Get the node ID of this counter
    pub fn node_id(&self) -> NodeId {
        self.positive.node_id()
    }

    /// Check if this counter dominates another
    pub fn dominates(&self, other: &Self) -> bool {
        self.positive.dominates(&other.positive) && self.negative.dominates(&other.negative)
    }

    /// Check if two counters are identical
    pub fn equals(&self, other: &Self) -> bool {
        self.positive.equals(&other.positive) && self.negative.equals(&other.negative)
    }
}

impl CrdtMerge for PNCounter {
    fn merge_from(&mut self, other: &Self) -> MergeStats {
        let before = self.value();
        self.merge(other);
        let after = self.value();

        MergeStats {
            elements_added: 0,
            elements_removed: 0,
            elements_updated: (after - before).unsigned_abs() as usize,
            conflicts_resolved: 0,
        }
    }
}

impl Default for PNCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl PartialEq for PNCounter {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl Eq for PNCounter {}

/// Bounded PN-Counter with minimum and maximum values
///
/// A PN-Counter that enforces bounds on the value.
/// Operations that would exceed bounds are rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedPNCounter {
    /// The underlying counter
    counter: PNCounter,
    /// Minimum allowed value (default: i64::MIN)
    min_value: i64,
    /// Maximum allowed value (default: i64::MAX)
    max_value: i64,
}

impl BoundedPNCounter {
    /// Create a new bounded counter
    pub fn new(node_id: NodeId, min_value: i64, max_value: i64) -> Self {
        Self {
            counter: PNCounter::new(node_id),
            min_value,
            max_value,
        }
    }

    /// Create a non-negative counter (min = 0)
    pub fn non_negative(node_id: NodeId) -> Self {
        Self::new(node_id, 0, i64::MAX)
    }

    /// Create a bounded positive counter (0 to max)
    pub fn bounded_positive(node_id: NodeId, max: u64) -> Self {
        Self::new(node_id, 0, max as i64)
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

    /// Try to decrement the counter
    ///
    /// Returns true if decrement was allowed, false if at min.
    pub fn try_decrement(&mut self) -> bool {
        if self.counter.value() > self.min_value {
            self.counter.decrement();
            true
        } else {
            false
        }
    }

    /// Get the current value
    pub fn value(&self) -> i64 {
        self.counter.value()
    }

    /// Get the minimum allowed value
    pub fn min_value(&self) -> i64 {
        self.min_value
    }

    /// Get the maximum allowed value
    pub fn max_value(&self) -> i64 {
        self.max_value
    }

    /// Merge with another bounded counter
    ///
    /// Note: After merge, value may exceed bounds if the other counter
    /// had different bounds. This maintains CRDT properties.
    pub fn merge(&mut self, other: &Self) {
        self.counter.merge(&other.counter);
    }

    /// Check if value is within bounds
    pub fn is_within_bounds(&self) -> bool {
        let v = self.counter.value();
        v >= self.min_value && v <= self.max_value
    }
}

/// Vote counter using PN-Counter
///
/// A specialized counter for upvote/downvote scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteCounter {
    counter: PNCounter,
}

impl VoteCounter {
    /// Create a new vote counter
    pub fn new(node_id: NodeId) -> Self {
        Self {
            counter: PNCounter::new(node_id),
        }
    }

    /// Cast an upvote
    pub fn upvote(&mut self) {
        self.counter.increment();
    }

    /// Cast a downvote
    pub fn downvote(&mut self) {
        self.counter.decrement();
    }

    /// Get the vote score (upvotes - downvotes)
    pub fn score(&self) -> i64 {
        self.counter.value()
    }

    /// Get total upvotes
    pub fn upvotes(&self) -> u64 {
        self.counter.total_increments()
    }

    /// Get total downvotes
    pub fn downvotes(&self) -> u64 {
        self.counter.total_decrements()
    }

    /// Get the vote ratio (upvotes / total)
    pub fn ratio(&self) -> f64 {
        let total = self.upvotes() + self.downvotes();
        if total == 0 {
            0.5 // Neutral when no votes
        } else {
            self.upvotes() as f64 / total as f64
        }
    }

    /// Merge with another vote counter
    pub fn merge(&mut self, other: &Self) {
        self.counter.merge(&other.counter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pn_counter_basic() {
        let mut counter = PNCounter::new(1);
        assert_eq!(counter.value(), 0);

        counter.increment();
        counter.increment();
        counter.decrement();

        assert_eq!(counter.value(), 1);
    }

    #[test]
    fn test_pn_counter_negative() {
        let mut counter = PNCounter::new(1);

        counter.decrement_by(10);
        assert_eq!(counter.value(), -10);

        counter.increment_by(3);
        assert_eq!(counter.value(), -7);
    }

    #[test]
    fn test_pn_counter_merge() {
        let mut counter_a = PNCounter::new(1);
        let mut counter_b = PNCounter::new(2);

        counter_a.increment_by(10);
        counter_a.decrement_by(3);

        counter_b.increment_by(5);
        counter_b.decrement_by(8);

        counter_a.merge(&counter_b);

        // Total: +10 +5 -3 -8 = 4
        assert_eq!(counter_a.value(), 4);
        assert_eq!(counter_a.total_increments(), 15);
        assert_eq!(counter_a.total_decrements(), 11);
    }

    #[test]
    fn test_pn_counter_convergence() {
        let mut counter_a = PNCounter::new(1);
        let mut counter_b = PNCounter::new(2);

        counter_a.increment_by(10);
        counter_b.decrement_by(5);

        // Different merge orders
        let mut result1 = counter_a.clone();
        result1.merge(&counter_b);

        let mut result2 = counter_b.clone();
        result2.merge(&counter_a);

        assert_eq!(result1.value(), result2.value());
        assert_eq!(result1.value(), 5);
    }

    #[test]
    fn test_pn_counter_node_value() {
        let mut counter = PNCounter::new(1);
        counter.increment_by(10);
        counter.decrement_by(3);

        assert_eq!(counter.local_value(), 7);
        assert_eq!(counter.node_value(1), 7);
        assert_eq!(counter.node_value(2), 0);
    }

    #[test]
    fn test_bounded_pn_counter() {
        let mut counter = BoundedPNCounter::non_negative(1);

        // Start at 0, can't decrement below 0
        assert!(!counter.try_decrement()); // Can't go below 0
        assert_eq!(counter.value(), 0);

        // Increment to 1
        assert!(counter.try_increment());
        assert_eq!(counter.value(), 1);

        // Can decrement back to 0
        assert!(counter.try_decrement());
        assert_eq!(counter.value(), 0);

        // Can't go below 0 again
        assert!(!counter.try_decrement());
        assert_eq!(counter.value(), 0);
    }

    #[test]
    fn test_bounded_pn_counter_max() {
        let mut counter = BoundedPNCounter::bounded_positive(1, 5);

        for _ in 0..5 {
            assert!(counter.try_increment());
        }

        assert!(!counter.try_increment()); // At max
        assert_eq!(counter.value(), 5);
    }

    #[test]
    fn test_vote_counter() {
        let mut votes = VoteCounter::new(1);

        votes.upvote();
        votes.upvote();
        votes.upvote();
        votes.downvote();

        assert_eq!(votes.score(), 2);
        assert_eq!(votes.upvotes(), 3);
        assert_eq!(votes.downvotes(), 1);
        assert_eq!(votes.ratio(), 0.75);
    }

    #[test]
    fn test_vote_counter_merge() {
        let mut votes_a = VoteCounter::new(1);
        let mut votes_b = VoteCounter::new(2);

        votes_a.upvote();
        votes_a.upvote();

        votes_b.downvote();

        votes_a.merge(&votes_b);

        assert_eq!(votes_a.score(), 1); // 2 up - 1 down
    }

    #[test]
    fn test_serialization() {
        let mut counter = PNCounter::new(1);
        counter.increment_by(10);
        counter.decrement_by(3);

        let json = serde_json::to_string(&counter).unwrap();
        let deserialized: PNCounter = serde_json::from_str(&json).unwrap();

        assert_eq!(counter.value(), deserialized.value());
    }

    #[test]
    fn test_crdt_merge_trait() {
        let mut counter_a = PNCounter::new(1);
        let mut counter_b = PNCounter::new(2);

        counter_a.increment_by(5);
        counter_b.increment_by(10);

        let stats = counter_a.merge_from(&counter_b);
        assert_eq!(stats.elements_updated, 10);
        assert_eq!(counter_a.value(), 15);
    }

    #[test]
    fn test_equals() {
        let mut counter_a = PNCounter::new(1);
        let mut counter_b = PNCounter::new(1);

        counter_a.increment_by(5);
        counter_a.decrement_by(2);

        counter_b.increment_by(5);
        counter_b.decrement_by(2);

        assert!(counter_a.equals(&counter_b));
        assert_eq!(counter_a, counter_b);
    }
}
