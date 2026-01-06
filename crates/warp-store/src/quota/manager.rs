//! Quota manager - main entry point for quota management

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, info};

use super::{
    AlertLevel, EnforcementResult, QuotaAlert, QuotaEnforcement, QuotaPolicy, QuotaScope,
    QuotaUsage, UsageTracker,
};

/// Configuration for the quota manager
#[derive(Debug, Clone)]
pub struct QuotaConfig {
    /// Enable quota enforcement
    pub enabled: bool,

    /// Default storage quota per bucket (bytes)
    pub default_bucket_quota: Option<u64>,

    /// Default storage quota per user (bytes)
    pub default_user_quota: Option<u64>,

    /// Global storage quota (bytes)
    pub global_quota: Option<u64>,

    /// Alert callback interval
    pub alert_interval: Duration,

    /// Usage snapshot interval
    pub snapshot_interval: Duration,

    /// Whether to default to allow when no policy matches
    pub default_allow: bool,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_bucket_quota: None,
            default_user_quota: None,
            global_quota: None,
            alert_interval: Duration::from_secs(60),
            snapshot_interval: Duration::from_secs(300),
            default_allow: true,
        }
    }
}

impl QuotaConfig {
    /// Create with all quotas disabled (no limits)
    pub fn unlimited() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Create with default limits
    pub fn with_defaults(bucket_gb: u64, user_gb: u64) -> Self {
        Self {
            enabled: true,
            default_bucket_quota: Some(bucket_gb * 1024 * 1024 * 1024),
            default_user_quota: Some(user_gb * 1024 * 1024 * 1024),
            ..Default::default()
        }
    }
}

/// Alert handler callback type
pub type AlertHandler = Arc<dyn Fn(QuotaAlert) + Send + Sync>;

/// The quota manager
pub struct QuotaManager {
    /// Configuration
    config: QuotaConfig,

    /// Policies by scope
    policies: DashMap<QuotaScope, QuotaPolicy>,

    /// Usage tracker
    tracker: Arc<UsageTracker>,

    /// Enforcement engine
    enforcement: QuotaEnforcement,

    /// Alert handlers
    alert_handlers: RwLock<Vec<AlertHandler>>,

    /// Recent alerts (for deduplication)
    recent_alerts: DashMap<QuotaScope, (AlertLevel, std::time::Instant)>,
}

impl QuotaManager {
    /// Create a new quota manager
    pub fn new(config: QuotaConfig) -> Self {
        let enforcement = if config.default_allow {
            QuotaEnforcement::new()
        } else {
            QuotaEnforcement::default_deny()
        };

        Self {
            config,
            policies: DashMap::new(),
            tracker: Arc::new(UsageTracker::new()),
            enforcement,
            alert_handlers: RwLock::new(Vec::new()),
            recent_alerts: DashMap::new(),
        }
    }

    /// Check if quotas are enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Register an alert handler
    pub fn on_alert(&self, handler: AlertHandler) {
        self.alert_handlers.write().push(handler);
    }

    /// Set a policy for a scope
    pub fn set_policy(&self, policy: QuotaPolicy) {
        info!(scope = %policy.scope, name = %policy.name, "Setting quota policy");
        self.policies.insert(policy.scope.clone(), policy);
    }

    /// Remove a policy
    pub fn remove_policy(&self, scope: &QuotaScope) {
        self.policies.remove(scope);
    }

    /// Get a policy
    pub fn get_policy(&self, scope: &QuotaScope) -> Option<QuotaPolicy> {
        self.policies.get(scope).map(|r| r.clone())
    }

    /// Get all policies
    pub fn all_policies(&self) -> Vec<QuotaPolicy> {
        self.policies.iter().map(|r| r.value().clone()).collect()
    }

    /// Get or create default policy for scope
    fn get_effective_policy(&self, scope: &QuotaScope) -> Option<QuotaPolicy> {
        // Check for explicit policy
        if let Some(policy) = self.get_policy(scope) {
            return Some(policy);
        }

        // Check for default based on scope type
        match scope {
            QuotaScope::Bucket(name) => self.config.default_bucket_quota.map(|quota| {
                QuotaPolicy::bucket_standard(name.clone(), quota / (1024 * 1024 * 1024))
            }),
            QuotaScope::User(name) => self.config.default_user_quota.map(|quota| {
                QuotaPolicy::user_standard(name.clone(), quota / (1024 * 1024 * 1024))
            }),
            QuotaScope::Global => self.config.global_quota.map(|quota| {
                QuotaPolicy::new("global", QuotaScope::Global)
                    .with_storage_limit(super::QuotaLimit::storage_bytes(quota))
            }),
            _ => None,
        }
    }

    // =========================================================================
    // Pre-operation checks
    // =========================================================================

    /// Check if a PUT operation is allowed
    pub fn check_put(&self, bucket: &str, user: Option<&str>, bytes: u64) -> EnforcementResult {
        if !self.config.enabled {
            return EnforcementResult::Allowed;
        }

        // Check bucket quota
        let bucket_scope = QuotaScope::Bucket(bucket.to_string());
        if let Some(policy) = self.get_effective_policy(&bucket_scope) {
            let usage = self.tracker.get_usage(&bucket_scope);
            let result = self.enforcement.check_object_create(&policy, &usage, bytes);
            if result.is_denied() {
                return result;
            }
            self.handle_result(&result);
        }

        // Check user quota
        if let Some(user_name) = user {
            let user_scope = QuotaScope::User(user_name.to_string());
            if let Some(policy) = self.get_effective_policy(&user_scope) {
                let usage = self.tracker.get_usage(&user_scope);
                let result = self.enforcement.check_object_create(&policy, &usage, bytes);
                if result.is_denied() {
                    return result;
                }
                self.handle_result(&result);
            }
        }

        // Check global quota
        if let Some(policy) = self.get_effective_policy(&QuotaScope::Global) {
            let usage = self.tracker.get_usage(&QuotaScope::Global);
            let result = self.enforcement.check_object_create(&policy, &usage, bytes);
            if result.is_denied() {
                return result;
            }
            self.handle_result(&result);
        }

        EnforcementResult::Allowed
    }

    /// Check if a GET operation is allowed (bandwidth/rate limiting)
    pub fn check_get(&self, bucket: &str, _user: Option<&str>, bytes: u64) -> EnforcementResult {
        if !self.config.enabled {
            return EnforcementResult::Allowed;
        }

        // Check bucket rate/bandwidth limits
        let bucket_scope = QuotaScope::Bucket(bucket.to_string());
        if let Some(policy) = self.get_effective_policy(&bucket_scope) {
            let usage = self.tracker.get_usage(&bucket_scope);

            // Check bandwidth
            let result = self.enforcement.check_bandwidth(&policy, &usage, bytes);
            if result.is_denied() {
                return result;
            }

            // Check rate
            let result = self.enforcement.check_request_rate(&policy, &usage);
            if result.is_denied() {
                return result;
            }
        }

        EnforcementResult::Allowed
    }

    // =========================================================================
    // Usage recording
    // =========================================================================

    /// Record a successful PUT
    pub fn record_put(&self, bucket: &str, user: Option<&str>, bytes: u64) {
        let bucket_scope = QuotaScope::Bucket(bucket.to_string());
        self.tracker.record_put(&bucket_scope, bytes);

        if let Some(user_name) = user {
            let user_scope = QuotaScope::User(user_name.to_string());
            self.tracker.record_put(&user_scope, bytes);
        }

        self.tracker.record_put(&QuotaScope::Global, bytes);
    }

    /// Record a successful DELETE
    pub fn record_delete(&self, bucket: &str, user: Option<&str>, bytes: u64) {
        let bucket_scope = QuotaScope::Bucket(bucket.to_string());
        self.tracker.record_delete(&bucket_scope, bytes);

        if let Some(user_name) = user {
            let user_scope = QuotaScope::User(user_name.to_string());
            self.tracker.record_delete(&user_scope, bytes);
        }

        self.tracker.record_delete(&QuotaScope::Global, bytes);
    }

    /// Record a successful GET
    pub fn record_get(&self, bucket: &str, user: Option<&str>, bytes: u64) {
        let bucket_scope = QuotaScope::Bucket(bucket.to_string());
        self.tracker.record_get(&bucket_scope, bytes);

        if let Some(user_name) = user {
            let user_scope = QuotaScope::User(user_name.to_string());
            self.tracker.record_get(&user_scope, bytes);
        }

        self.tracker.record_get(&QuotaScope::Global, bytes);
    }

    // =========================================================================
    // Usage queries
    // =========================================================================

    /// Get current usage for a bucket
    pub fn bucket_usage(&self, bucket: &str) -> QuotaUsage {
        self.tracker
            .get_usage(&QuotaScope::Bucket(bucket.to_string()))
    }

    /// Get current usage for a user
    pub fn user_usage(&self, user: &str) -> QuotaUsage {
        self.tracker.get_usage(&QuotaScope::User(user.to_string()))
    }

    /// Get global usage
    pub fn global_usage(&self) -> QuotaUsage {
        self.tracker.get_usage(&QuotaScope::Global)
    }

    /// Get all usage
    pub fn all_usage(&self) -> Vec<(QuotaScope, QuotaUsage)> {
        self.tracker.all_usage()
    }

    /// Get the usage tracker
    pub fn tracker(&self) -> &UsageTracker {
        &self.tracker
    }

    // =========================================================================
    // Internal methods
    // =========================================================================

    fn handle_result(&self, result: &EnforcementResult) {
        if let Some(alert) = result.alert() {
            self.emit_alert(alert.clone());
        }
    }

    fn emit_alert(&self, alert: QuotaAlert) {
        // Deduplicate alerts
        let key = alert.scope.clone();
        if let Some(recent) = self.recent_alerts.get(&key) {
            let (level, time) = recent.value();
            if *level == alert.level && time.elapsed() < self.config.alert_interval {
                return; // Skip duplicate
            }
        }
        self.recent_alerts
            .insert(key, (alert.level, std::time::Instant::now()));

        debug!(
            scope = %alert.scope,
            level = ?alert.level,
            message = %alert.message,
            "Quota alert"
        );

        // Notify handlers
        let handlers = self.alert_handlers.read();
        for handler in handlers.iter() {
            handler(alert.clone());
        }
    }

    /// Generate all current alerts
    pub fn generate_alerts(&self) -> Vec<QuotaAlert> {
        let mut alerts = Vec::new();

        for policy in self.all_policies() {
            let usage = self.tracker.get_usage(&policy.scope);
            alerts.extend(self.enforcement.check_alerts(&policy, &usage));
        }

        alerts
    }
}

impl Default for QuotaManager {
    fn default() -> Self {
        Self::new(QuotaConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_manager_basic() {
        let manager = QuotaManager::new(QuotaConfig::with_defaults(100, 50));

        // Should allow write when under quota
        let result = manager.check_put("my-bucket", Some("user1"), 1024);
        assert!(result.is_allowed());

        // Record the write
        manager.record_put("my-bucket", Some("user1"), 1024);

        // Check usage
        let usage = manager.bucket_usage("my-bucket");
        assert_eq!(usage.storage_bytes, 1024);
        assert_eq!(usage.object_count, 1);
    }

    #[test]
    fn test_quota_manager_disabled() {
        let manager = QuotaManager::new(QuotaConfig::unlimited());

        // Should always allow when disabled
        let result = manager.check_put("bucket", None, u64::MAX);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_quota_policy_enforcement() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);

        // Set a small quota
        let policy = QuotaPolicy::bucket_standard("test", 1); // 1 GB
        manager.set_policy(policy);

        // Set usage near limit
        let scope = QuotaScope::Bucket("test".to_string());
        manager.tracker.set_usage(
            scope,
            QuotaUsage {
                storage_bytes: 1024 * 1024 * 1024, // 1 GB
                object_count: 100,
                ..Default::default()
            },
        );

        // Should deny over-limit write
        let result = manager.check_put("test", None, 1);
        assert!(result.is_denied());
    }
}
