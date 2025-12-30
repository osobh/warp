//! Usage tracking for quota management

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;

use super::QuotaScope;

/// Current usage for a quota scope
#[derive(Debug, Clone, Default)]
pub struct QuotaUsage {
    /// Storage bytes used
    pub storage_bytes: u64,

    /// Object count
    pub object_count: u64,

    /// Requests in current window
    pub request_count: u64,

    /// Bandwidth used in current window (bytes)
    pub bandwidth_used: u64,

    /// Last update time
    pub last_updated: Option<SystemTime>,
}

impl QuotaUsage {
    /// Create new usage
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if usage is empty
    pub fn is_empty(&self) -> bool {
        self.storage_bytes == 0 && self.object_count == 0
    }
}

/// A snapshot of usage at a point in time
#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    /// The scope
    pub scope: QuotaScope,

    /// Usage at snapshot time
    pub usage: QuotaUsage,

    /// When the snapshot was taken
    pub timestamp: SystemTime,
}

/// Atomic usage counters for thread-safe updates
struct AtomicUsage {
    storage_bytes: AtomicU64,
    object_count: AtomicU64,
    request_count: AtomicU64,
    bandwidth_used: AtomicU64,
    last_updated: RwLock<Option<Instant>>,
}

impl AtomicUsage {
    fn new() -> Self {
        Self {
            storage_bytes: AtomicU64::new(0),
            object_count: AtomicU64::new(0),
            request_count: AtomicU64::new(0),
            bandwidth_used: AtomicU64::new(0),
            last_updated: RwLock::new(None),
        }
    }

    fn from_usage(usage: &QuotaUsage) -> Self {
        Self {
            storage_bytes: AtomicU64::new(usage.storage_bytes),
            object_count: AtomicU64::new(usage.object_count),
            request_count: AtomicU64::new(usage.request_count),
            bandwidth_used: AtomicU64::new(usage.bandwidth_used),
            last_updated: RwLock::new(Some(Instant::now())),
        }
    }

    fn to_usage(&self) -> QuotaUsage {
        QuotaUsage {
            storage_bytes: self.storage_bytes.load(Ordering::Relaxed),
            object_count: self.object_count.load(Ordering::Relaxed),
            request_count: self.request_count.load(Ordering::Relaxed),
            bandwidth_used: self.bandwidth_used.load(Ordering::Relaxed),
            last_updated: Some(SystemTime::now()),
        }
    }

    fn add_storage(&self, bytes: u64) {
        self.storage_bytes.fetch_add(bytes, Ordering::Relaxed);
        *self.last_updated.write() = Some(Instant::now());
    }

    fn remove_storage(&self, bytes: u64) {
        self.storage_bytes.fetch_sub(bytes.min(self.storage_bytes.load(Ordering::Relaxed)), Ordering::Relaxed);
        *self.last_updated.write() = Some(Instant::now());
    }

    fn add_object(&self) {
        self.object_count.fetch_add(1, Ordering::Relaxed);
        *self.last_updated.write() = Some(Instant::now());
    }

    fn remove_object(&self) {
        let current = self.object_count.load(Ordering::Relaxed);
        if current > 0 {
            self.object_count.fetch_sub(1, Ordering::Relaxed);
        }
        *self.last_updated.write() = Some(Instant::now());
    }

    fn record_request(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_bandwidth(&self, bytes: u64) {
        self.bandwidth_used.fetch_add(bytes, Ordering::Relaxed);
    }

    fn reset_rate_counters(&self) {
        self.request_count.store(0, Ordering::Relaxed);
        self.bandwidth_used.store(0, Ordering::Relaxed);
    }
}

/// Tracks usage across all quota scopes
pub struct UsageTracker {
    /// Usage by scope
    usage: DashMap<QuotaScope, AtomicUsage>,

    /// Historical snapshots
    history: RwLock<VecDeque<UsageSnapshot>>,

    /// Maximum history entries
    max_history: usize,

    /// Rate window duration
    rate_window: Duration,

    /// Last rate window reset
    last_rate_reset: RwLock<Instant>,
}

impl UsageTracker {
    /// Create a new usage tracker
    pub fn new() -> Self {
        Self {
            usage: DashMap::new(),
            history: RwLock::new(VecDeque::new()),
            max_history: 1000,
            rate_window: Duration::from_secs(1),
            last_rate_reset: RwLock::new(Instant::now()),
        }
    }

    /// Create with custom settings
    pub fn with_settings(max_history: usize, rate_window: Duration) -> Self {
        Self {
            usage: DashMap::new(),
            history: RwLock::new(VecDeque::new()),
            max_history,
            rate_window,
            last_rate_reset: RwLock::new(Instant::now()),
        }
    }

    /// Get current usage for a scope
    pub fn get_usage(&self, scope: &QuotaScope) -> QuotaUsage {
        self.usage
            .get(scope)
            .map(|u| u.to_usage())
            .unwrap_or_default()
    }

    /// Set initial usage for a scope
    pub fn set_usage(&self, scope: QuotaScope, usage: QuotaUsage) {
        self.usage.insert(scope, AtomicUsage::from_usage(&usage));
    }

    /// Record object creation
    pub fn record_put(&self, scope: &QuotaScope, bytes: u64) {
        self.ensure_scope(scope);
        if let Some(usage) = self.usage.get(scope) {
            usage.add_storage(bytes);
            usage.add_object();
            usage.record_request();
            usage.record_bandwidth(bytes);
        }
    }

    /// Record object deletion
    pub fn record_delete(&self, scope: &QuotaScope, bytes: u64) {
        if let Some(usage) = self.usage.get(scope) {
            usage.remove_storage(bytes);
            usage.remove_object();
            usage.record_request();
        }
    }

    /// Record object read
    pub fn record_get(&self, scope: &QuotaScope, bytes: u64) {
        self.ensure_scope(scope);
        if let Some(usage) = self.usage.get(scope) {
            usage.record_request();
            usage.record_bandwidth(bytes);
        }
    }

    /// Record a request (without data transfer)
    pub fn record_request(&self, scope: &QuotaScope) {
        self.ensure_scope(scope);
        if let Some(usage) = self.usage.get(scope) {
            usage.record_request();
        }
    }

    /// Check and reset rate counters if window has passed
    pub fn maybe_reset_rates(&self) {
        let mut last_reset = self.last_rate_reset.write();
        if last_reset.elapsed() >= self.rate_window {
            // Reset rate counters for all scopes
            for entry in self.usage.iter() {
                entry.value().reset_rate_counters();
            }
            *last_reset = Instant::now();
        }
    }

    /// Take a snapshot of current usage
    pub fn snapshot(&self, scope: &QuotaScope) -> UsageSnapshot {
        UsageSnapshot {
            scope: scope.clone(),
            usage: self.get_usage(scope),
            timestamp: SystemTime::now(),
        }
    }

    /// Take and store a snapshot
    pub fn record_snapshot(&self, scope: &QuotaScope) {
        let snapshot = self.snapshot(scope);
        let mut history = self.history.write();
        if history.len() >= self.max_history {
            history.pop_front();
        }
        history.push_back(snapshot);
    }

    /// Get usage history for a scope
    pub fn get_history(&self, scope: &QuotaScope) -> Vec<UsageSnapshot> {
        self.history
            .read()
            .iter()
            .filter(|s| s.scope == *scope)
            .cloned()
            .collect()
    }

    /// Get all current usage
    pub fn all_usage(&self) -> Vec<(QuotaScope, QuotaUsage)> {
        self.usage
            .iter()
            .map(|r| (r.key().clone(), r.value().to_usage()))
            .collect()
    }

    /// Clear usage for a scope
    pub fn clear(&self, scope: &QuotaScope) {
        self.usage.remove(scope);
    }

    /// Clear all usage
    pub fn clear_all(&self) {
        self.usage.clear();
        self.history.write().clear();
    }

    /// Ensure scope exists
    fn ensure_scope(&self, scope: &QuotaScope) {
        if !self.usage.contains_key(scope) {
            self.usage.insert(scope.clone(), AtomicUsage::new());
        }
    }
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_tracking() {
        let tracker = UsageTracker::new();
        let scope = QuotaScope::Bucket("test".to_string());

        tracker.record_put(&scope, 1024);
        let usage = tracker.get_usage(&scope);
        assert_eq!(usage.storage_bytes, 1024);
        assert_eq!(usage.object_count, 1);

        tracker.record_delete(&scope, 1024);
        let usage = tracker.get_usage(&scope);
        assert_eq!(usage.storage_bytes, 0);
        assert_eq!(usage.object_count, 0);
    }

    #[test]
    fn test_multiple_scopes() {
        let tracker = UsageTracker::new();
        let scope1 = QuotaScope::Bucket("bucket1".to_string());
        let scope2 = QuotaScope::Bucket("bucket2".to_string());

        tracker.record_put(&scope1, 1000);
        tracker.record_put(&scope2, 2000);

        assert_eq!(tracker.get_usage(&scope1).storage_bytes, 1000);
        assert_eq!(tracker.get_usage(&scope2).storage_bytes, 2000);
    }

    #[test]
    fn test_snapshot() {
        let tracker = UsageTracker::new();
        let scope = QuotaScope::Global;

        tracker.record_put(&scope, 5000);
        let snapshot = tracker.snapshot(&scope);

        assert_eq!(snapshot.usage.storage_bytes, 5000);
        assert_eq!(snapshot.scope, QuotaScope::Global);
    }
}
