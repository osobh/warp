//! Hybrid Logical Clock (HLC) implementation
//!
//! HLC combines physical time with logical counters to provide a total
//! ordering of events across distributed nodes. It's essential for
//! Last-Write-Wins semantics in CRDTs.
//!
//! # Properties
//!
//! - Always monotonically increasing
//! - Captures happens-before relationships
//! - Bounded drift from physical time
//! - Unique timestamps with node ID tie-breaking

use crate::{CrdtError, NodeId, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hybrid Logical Clock timestamp
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct HLC {
    /// Physical time (milliseconds since UNIX epoch)
    pub physical: u64,
    /// Logical counter for same physical time
    pub logical: u32,
    /// Node identifier for tie-breaking
    pub node_id: NodeId,
}

impl HLC {
    /// Create a new HLC for the given node
    pub fn new(node_id: NodeId) -> Self {
        Self {
            physical: current_time_ms(),
            logical: 0,
            node_id,
        }
    }

    /// Create HLC from components (for deserialization/testing)
    pub fn from_parts(physical: u64, logical: u32, node_id: NodeId) -> Self {
        Self {
            physical,
            logical,
            node_id,
        }
    }

    /// Generate next timestamp for a local event
    ///
    /// This ensures the clock always advances, even if physical time
    /// hasn't changed since the last tick.
    pub fn tick(&mut self) -> HLC {
        let now = current_time_ms();

        if now > self.physical {
            self.physical = now;
            self.logical = 0;
        } else {
            self.logical = self.logical.saturating_add(1);
        }

        *self
    }

    /// Update clock after receiving a remote timestamp
    ///
    /// This ensures causality: the local clock will always be
    /// greater than any timestamp we've observed.
    pub fn receive(&mut self, remote: &HLC) -> HLC {
        let now = current_time_ms();

        // Take max of local physical, remote physical, and current time
        if now > self.physical && now > remote.physical {
            self.physical = now;
            self.logical = 0;
        } else if self.physical > remote.physical {
            // Local clock ahead
            self.logical = self.logical.saturating_add(1);
        } else if remote.physical > self.physical {
            // Remote clock ahead
            self.physical = remote.physical;
            self.logical = remote.logical.saturating_add(1);
        } else {
            // Same physical time
            self.logical = self.logical.max(remote.logical).saturating_add(1);
        }

        *self
    }

    /// Update clock when sending a message
    ///
    /// Returns the timestamp to attach to the message.
    pub fn send(&mut self) -> HLC {
        self.tick()
    }

    /// Check if this timestamp happened before another
    pub fn happened_before(&self, other: &HLC) -> bool {
        match self.cmp(other) {
            Ordering::Less => true,
            _ => false,
        }
    }

    /// Check if this timestamp is concurrent with another
    ///
    /// Two timestamps are concurrent if neither happened before the other
    /// (excluding timestamps from the same node).
    pub fn is_concurrent(&self, other: &HLC) -> bool {
        self.node_id != other.node_id
            && self.physical == other.physical
            && self.logical == other.logical
    }

    /// Get drift from physical time in milliseconds
    pub fn drift_ms(&self) -> i64 {
        let now = current_time_ms();
        self.physical as i64 - now as i64
    }

    /// Check if clock has excessive drift
    pub fn has_excessive_drift(&self, max_drift_ms: u64) -> bool {
        self.drift_ms().unsigned_abs() > max_drift_ms
    }
}

impl Ord for HLC {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare physical time
        match self.physical.cmp(&other.physical) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Then logical counter
        match self.logical.cmp(&other.logical) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Finally node ID (arbitrary but deterministic tie-breaker)
        self.node_id.cmp(&other.node_id)
    }
}

impl PartialOrd for HLC {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Default for HLC {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Get current time in milliseconds since UNIX epoch
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Clock monitor for detecting and handling drift
pub struct ClockMonitor {
    /// Maximum allowed drift in milliseconds
    max_drift_ms: u64,
    /// Last known physical time
    last_physical: u64,
}

impl ClockMonitor {
    /// Create a new clock monitor
    pub fn new(max_drift_ms: u64) -> Self {
        Self {
            max_drift_ms,
            last_physical: current_time_ms(),
        }
    }

    /// Check a timestamp for validity
    pub fn check(&mut self, hlc: &HLC) -> Result<()> {
        let now = current_time_ms();

        // Check for backwards clock (NTP adjustment, etc.)
        if now < self.last_physical {
            tracing::warn!(
                "Physical clock went backwards: {} -> {}",
                self.last_physical,
                now
            );
        }
        self.last_physical = now;

        // Check for excessive drift
        if hlc.has_excessive_drift(self.max_drift_ms) {
            return Err(CrdtError::ClockDrift {
                expected: now,
                actual: hlc.physical,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_hlc_creation() {
        let hlc = HLC::new(42);
        assert_eq!(hlc.node_id, 42);
        assert_eq!(hlc.logical, 0);
        assert!(hlc.physical > 0);
    }

    #[test]
    fn test_hlc_tick() {
        let mut hlc = HLC::new(1);
        let t1 = hlc.tick();
        let t2 = hlc.tick();

        // Ticks should always advance
        assert!(t2 > t1);
    }

    #[test]
    fn test_hlc_tick_rapid() {
        let mut hlc = HLC::new(1);

        // Rapid ticks within same millisecond
        let mut prev = hlc.tick();
        for _ in 0..1000 {
            let curr = hlc.tick();
            assert!(curr > prev);
            prev = curr;
        }
    }

    #[test]
    fn test_hlc_ordering() {
        let t1 = HLC::from_parts(100, 0, 1);
        let t2 = HLC::from_parts(100, 1, 1);
        let t3 = HLC::from_parts(101, 0, 1);
        let t4 = HLC::from_parts(100, 0, 2);

        assert!(t1 < t2); // Same physical, different logical
        assert!(t2 < t3); // Different physical
        assert!(t1 < t4); // Same physical+logical, different node
    }

    #[test]
    fn test_hlc_receive() {
        let mut clock_a = HLC::new(1);
        let mut clock_b = HLC::new(2);

        // A sends
        let msg = clock_a.send();

        // B receives - should be ahead
        let after = clock_b.receive(&msg);
        assert!(after > msg);
    }

    #[test]
    fn test_hlc_receive_ahead() {
        let mut clock_a = HLC::new(1);

        // Create a future timestamp
        let future = HLC::from_parts(clock_a.physical + 10000, 5, 2);

        // Receiving future timestamp advances our clock
        clock_a.receive(&future);
        assert!(clock_a.physical >= future.physical);
    }

    #[test]
    fn test_hlc_causality() {
        let mut node_a = HLC::new(1);
        let mut node_b = HLC::new(2);
        let mut node_c = HLC::new(3);

        // A sends to B
        let msg_a = node_a.send();
        let _ = node_b.receive(&msg_a);

        // B sends to C
        let msg_b = node_b.send();
        let _ = node_c.receive(&msg_b);

        // C's clock should be ahead of A's original message
        assert!(node_c.tick() > msg_a);
    }

    #[test]
    fn test_hlc_concurrent() {
        let t1 = HLC::from_parts(100, 5, 1);
        let t2 = HLC::from_parts(100, 5, 2);

        // Same physical + logical but different nodes = concurrent
        assert!(t1.is_concurrent(&t2));
    }

    #[test]
    fn test_happened_before() {
        let t1 = HLC::from_parts(100, 0, 1);
        let t2 = HLC::from_parts(100, 1, 1);

        assert!(t1.happened_before(&t2));
        assert!(!t2.happened_before(&t1));
    }

    #[test]
    fn test_clock_monitor() {
        let mut monitor = ClockMonitor::new(60_000); // 1 minute max drift
        let hlc = HLC::new(1);

        assert!(monitor.check(&hlc).is_ok());

        // Excessive drift should fail
        let future_hlc = HLC::from_parts(hlc.physical + 120_000, 0, 1);
        assert!(monitor.check(&future_hlc).is_err());
    }

    #[test]
    fn test_drift_detection() {
        let hlc = HLC::new(1);

        // Recently created HLC should have minimal drift
        assert!(!hlc.has_excessive_drift(1000));
    }

    #[test]
    fn test_hlc_physical_advance() {
        let mut hlc = HLC::new(1);
        let initial = hlc.tick();

        // Sleep to ensure physical time advances
        thread::sleep(Duration::from_millis(2));

        let after_sleep = hlc.tick();

        // Physical time should have advanced
        assert!(after_sleep.physical >= initial.physical);
    }

    #[test]
    fn test_serialization() {
        let hlc = HLC::from_parts(12345678, 42, 99);
        let json = serde_json::to_string(&hlc).unwrap();
        let deserialized: HLC = serde_json::from_str(&json).unwrap();

        assert_eq!(hlc, deserialized);
    }
}
