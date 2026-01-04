//! Quota policy definitions

use std::fmt;

/// Scope for quota application
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QuotaScope {
    /// Global system-wide quota
    Global,

    /// Per-bucket quota
    Bucket(String),

    /// Per-user quota
    User(String),

    /// Per-namespace (hierarchical) quota
    Namespace(String),

    /// Custom scope
    Custom(String),
}

impl fmt::Display for QuotaScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => write!(f, "Global"),
            Self::Bucket(name) => write!(f, "Bucket:{}", name),
            Self::User(name) => write!(f, "User:{}", name),
            Self::Namespace(name) => write!(f, "Namespace:{}", name),
            Self::Custom(name) => write!(f, "Custom:{}", name),
        }
    }
}

/// Type of quota limit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaLimitType {
    /// Storage space in bytes
    StorageBytes,
    /// Number of objects
    ObjectCount,
    /// Number of requests per time window
    RequestRate,
    /// Bandwidth in bytes per second
    Bandwidth,
}

/// A quota limit specification
#[derive(Debug, Clone)]
pub struct QuotaLimit {
    /// Type of limit
    pub limit_type: QuotaLimitType,

    /// Soft limit (warning threshold)
    pub soft_limit: Option<u64>,

    /// Hard limit (enforcement threshold)
    pub hard_limit: u64,

    /// Whether to enforce hard limit
    pub enforce_hard: bool,

    /// Grace period after soft limit
    pub grace_period_secs: Option<u64>,

    /// Alert thresholds (percentages)
    pub alert_thresholds: Vec<u8>,
}

impl QuotaLimit {
    /// Create a storage bytes limit
    pub fn storage_bytes(hard_limit: u64) -> Self {
        Self {
            limit_type: QuotaLimitType::StorageBytes,
            soft_limit: Some((hard_limit as f64 * 0.9) as u64), // 90%
            hard_limit,
            enforce_hard: true,
            grace_period_secs: None,
            alert_thresholds: vec![50, 75, 90, 100],
        }
    }

    /// Create an object count limit
    pub fn object_count(hard_limit: u64) -> Self {
        Self {
            limit_type: QuotaLimitType::ObjectCount,
            soft_limit: None,
            hard_limit,
            enforce_hard: true,
            grace_period_secs: None,
            alert_thresholds: vec![75, 90, 100],
        }
    }

    /// Create a request rate limit
    pub fn request_rate(requests_per_second: u64) -> Self {
        Self {
            limit_type: QuotaLimitType::RequestRate,
            soft_limit: None,
            hard_limit: requests_per_second,
            enforce_hard: true,
            grace_period_secs: None,
            alert_thresholds: vec![],
        }
    }

    /// Create a bandwidth limit
    pub fn bandwidth(bytes_per_second: u64) -> Self {
        Self {
            limit_type: QuotaLimitType::Bandwidth,
            soft_limit: None,
            hard_limit: bytes_per_second,
            enforce_hard: true,
            grace_period_secs: None,
            alert_thresholds: vec![],
        }
    }

    /// Set soft limit
    pub fn with_soft_limit(mut self, soft_limit: u64) -> Self {
        self.soft_limit = Some(soft_limit);
        self
    }

    /// Set grace period
    pub fn with_grace_period(mut self, secs: u64) -> Self {
        self.grace_period_secs = Some(secs);
        self
    }

    /// Set alert thresholds
    pub fn with_alert_thresholds(mut self, thresholds: Vec<u8>) -> Self {
        self.alert_thresholds = thresholds;
        self
    }

    /// Disable hard limit enforcement (warning only)
    pub fn warning_only(mut self) -> Self {
        self.enforce_hard = false;
        self
    }

    /// Calculate percentage used
    pub fn percent_used(&self, current: u64) -> f64 {
        if self.hard_limit == 0 {
            return 0.0;
        }
        (current as f64 / self.hard_limit as f64) * 100.0
    }

    /// Check if soft limit is exceeded
    pub fn exceeds_soft(&self, current: u64) -> bool {
        self.soft_limit.map(|s| current > s).unwrap_or(false)
    }

    /// Check if hard limit is exceeded
    pub fn exceeds_hard(&self, current: u64) -> bool {
        current > self.hard_limit
    }

    /// Get the next alert threshold that would be crossed
    pub fn next_threshold(&self, current_percent: f64) -> Option<u8> {
        self.alert_thresholds
            .iter()
            .find(|&&t| current_percent < t as f64)
            .copied()
    }
}

/// A complete quota policy
#[derive(Debug, Clone)]
pub struct QuotaPolicy {
    /// Policy name
    pub name: String,

    /// Policy description
    pub description: Option<String>,

    /// Scope this policy applies to
    pub scope: QuotaScope,

    /// Storage bytes limit
    pub storage_limit: Option<QuotaLimit>,

    /// Object count limit
    pub object_limit: Option<QuotaLimit>,

    /// Request rate limit
    pub rate_limit: Option<QuotaLimit>,

    /// Bandwidth limit
    pub bandwidth_limit: Option<QuotaLimit>,

    /// Whether this policy is enabled
    pub enabled: bool,

    /// Priority (higher = evaluated first)
    pub priority: i32,

    /// Inherit from parent scope
    pub inherit: bool,
}

impl QuotaPolicy {
    /// Create a new policy
    pub fn new(name: impl Into<String>, scope: QuotaScope) -> Self {
        Self {
            name: name.into(),
            description: None,
            scope,
            storage_limit: None,
            object_limit: None,
            rate_limit: None,
            bandwidth_limit: None,
            enabled: true,
            priority: 0,
            inherit: true,
        }
    }

    /// Set storage limit
    pub fn with_storage_limit(mut self, limit: QuotaLimit) -> Self {
        self.storage_limit = Some(limit);
        self
    }

    /// Set object count limit
    pub fn with_object_limit(mut self, limit: QuotaLimit) -> Self {
        self.object_limit = Some(limit);
        self
    }

    /// Set rate limit
    pub fn with_rate_limit(mut self, limit: QuotaLimit) -> Self {
        self.rate_limit = Some(limit);
        self
    }

    /// Set bandwidth limit
    pub fn with_bandwidth_limit(mut self, limit: QuotaLimit) -> Self {
        self.bandwidth_limit = Some(limit);
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Disable inheritance
    pub fn no_inherit(mut self) -> Self {
        self.inherit = false;
        self
    }

    /// Disable the policy
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Create a standard bucket policy
    pub fn bucket_standard(bucket: impl Into<String>, storage_gb: u64) -> Self {
        let storage_bytes = storage_gb * 1024 * 1024 * 1024;
        Self::new("standard-bucket", QuotaScope::Bucket(bucket.into()))
            .with_storage_limit(QuotaLimit::storage_bytes(storage_bytes))
            .with_object_limit(QuotaLimit::object_count(1_000_000))
    }

    /// Create a user policy
    pub fn user_standard(user: impl Into<String>, storage_gb: u64) -> Self {
        let storage_bytes = storage_gb * 1024 * 1024 * 1024;
        Self::new("standard-user", QuotaScope::User(user.into()))
            .with_storage_limit(QuotaLimit::storage_bytes(storage_bytes))
    }
}

impl Default for QuotaPolicy {
    fn default() -> Self {
        Self::new("default", QuotaScope::Global)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_limit() {
        let limit = QuotaLimit::storage_bytes(100 * 1024 * 1024 * 1024); // 100 GB
        assert_eq!(limit.percent_used(50 * 1024 * 1024 * 1024), 50.0);
        assert!(!limit.exceeds_hard(99 * 1024 * 1024 * 1024));
        assert!(limit.exceeds_hard(101 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_quota_policy() {
        let policy = QuotaPolicy::bucket_standard("my-bucket", 100);
        assert!(policy.storage_limit.is_some());
        assert!(policy.object_limit.is_some());
        assert!(matches!(policy.scope, QuotaScope::Bucket(_)));
    }

    #[test]
    fn test_scope_display() {
        assert_eq!(QuotaScope::Global.to_string(), "Global");
        assert_eq!(
            QuotaScope::Bucket("test".to_string()).to_string(),
            "Bucket:test"
        );
    }
}
