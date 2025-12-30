//! Quota enforcement engine

use super::{AlertLevel, QuotaAlert, QuotaLimit, QuotaLimitType, QuotaPolicy, QuotaScope, QuotaUsage};

/// Result of quota enforcement check
#[derive(Debug, Clone)]
pub enum EnforcementResult {
    /// Operation is allowed
    Allowed,

    /// Operation allowed but quota is near limit
    AllowedWithWarning(QuotaAlert),

    /// Operation denied due to quota
    Denied(QuotaViolation),
}

impl EnforcementResult {
    /// Check if operation is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed | Self::AllowedWithWarning(_))
    }

    /// Check if operation is denied
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied(_))
    }

    /// Get alert if any
    pub fn alert(&self) -> Option<&QuotaAlert> {
        match self {
            Self::AllowedWithWarning(alert) => Some(alert),
            _ => None,
        }
    }

    /// Get violation if any
    pub fn violation(&self) -> Option<&QuotaViolation> {
        match self {
            Self::Denied(v) => Some(v),
            _ => None,
        }
    }
}

/// Details of a quota violation
#[derive(Debug, Clone)]
pub struct QuotaViolation {
    /// The scope that was violated
    pub scope: QuotaScope,

    /// Type of limit violated
    pub limit_type: QuotaLimitType,

    /// Current usage
    pub current: u64,

    /// The limit that was exceeded
    pub limit: u64,

    /// How much the operation would add
    pub requested: u64,

    /// Human-readable message
    pub message: String,
}

impl QuotaViolation {
    /// Create a new violation
    pub fn new(
        scope: QuotaScope,
        limit_type: QuotaLimitType,
        current: u64,
        limit: u64,
        requested: u64,
    ) -> Self {
        let message = format!(
            "Quota exceeded for {}: {} + {} > {} ({})",
            scope,
            current,
            requested,
            limit,
            match limit_type {
                QuotaLimitType::StorageBytes => "storage",
                QuotaLimitType::ObjectCount => "objects",
                QuotaLimitType::RequestRate => "requests/sec",
                QuotaLimitType::Bandwidth => "bandwidth",
            }
        );

        Self {
            scope,
            limit_type,
            current,
            limit,
            requested,
            message,
        }
    }
}

/// Quota enforcement engine
pub struct QuotaEnforcement {
    /// Default action when no policy matches
    default_allow: bool,

    /// Grace period tracking (scope -> first violation time)
    grace_periods: dashmap::DashMap<QuotaScope, std::time::Instant>,
}

impl QuotaEnforcement {
    /// Create a new enforcement engine
    pub fn new() -> Self {
        Self {
            default_allow: true,
            grace_periods: dashmap::DashMap::new(),
        }
    }

    /// Create with default deny
    pub fn default_deny() -> Self {
        Self {
            default_allow: false,
            grace_periods: dashmap::DashMap::new(),
        }
    }

    /// Check if a storage write is allowed
    pub fn check_storage_write(
        &self,
        policy: &QuotaPolicy,
        usage: &QuotaUsage,
        bytes_to_write: u64,
    ) -> EnforcementResult {
        if !policy.enabled {
            return EnforcementResult::Allowed;
        }

        if let Some(ref limit) = policy.storage_limit {
            let new_total = usage.storage_bytes.saturating_add(bytes_to_write);
            return self.check_limit(&policy.scope, limit, usage.storage_bytes, new_total, bytes_to_write);
        }

        EnforcementResult::Allowed
    }

    /// Check if creating a new object is allowed
    pub fn check_object_create(
        &self,
        policy: &QuotaPolicy,
        usage: &QuotaUsage,
        bytes: u64,
    ) -> EnforcementResult {
        if !policy.enabled {
            return EnforcementResult::Allowed;
        }

        // Check object count limit
        if let Some(ref limit) = policy.object_limit {
            let new_count = usage.object_count.saturating_add(1);
            let result = self.check_limit(&policy.scope, limit, usage.object_count, new_count, 1);
            if result.is_denied() {
                return result;
            }
        }

        // Also check storage limit
        self.check_storage_write(policy, usage, bytes)
    }

    /// Check request rate limit
    pub fn check_request_rate(
        &self,
        policy: &QuotaPolicy,
        usage: &QuotaUsage,
    ) -> EnforcementResult {
        if !policy.enabled {
            return EnforcementResult::Allowed;
        }

        if let Some(ref limit) = policy.rate_limit {
            let new_count = usage.request_count.saturating_add(1);
            return self.check_limit(&policy.scope, limit, usage.request_count, new_count, 1);
        }

        EnforcementResult::Allowed
    }

    /// Check bandwidth limit
    pub fn check_bandwidth(
        &self,
        policy: &QuotaPolicy,
        usage: &QuotaUsage,
        bytes: u64,
    ) -> EnforcementResult {
        if !policy.enabled {
            return EnforcementResult::Allowed;
        }

        if let Some(ref limit) = policy.bandwidth_limit {
            let new_total = usage.bandwidth_used.saturating_add(bytes);
            return self.check_limit(&policy.scope, limit, usage.bandwidth_used, new_total, bytes);
        }

        EnforcementResult::Allowed
    }

    /// Check a specific limit
    fn check_limit(
        &self,
        scope: &QuotaScope,
        limit: &QuotaLimit,
        current: u64,
        new_total: u64,
        requested: u64,
    ) -> EnforcementResult {
        let percent_used = limit.percent_used(new_total);

        // Check hard limit
        if limit.exceeds_hard(new_total) {
            // Check grace period
            if let Some(grace_secs) = limit.grace_period_secs {
                let in_grace = self.grace_periods
                    .entry(scope.clone())
                    .or_insert_with(std::time::Instant::now);

                if in_grace.elapsed().as_secs() < grace_secs {
                    // Still in grace period - allow with warning
                    return EnforcementResult::AllowedWithWarning(QuotaAlert::new(
                        scope.clone(),
                        AlertLevel::Critical,
                        new_total,
                        limit.hard_limit,
                    ));
                }
            }

            if limit.enforce_hard {
                return EnforcementResult::Denied(QuotaViolation::new(
                    scope.clone(),
                    limit.limit_type,
                    current,
                    limit.hard_limit,
                    requested,
                ));
            } else {
                // Warning only mode
                return EnforcementResult::AllowedWithWarning(QuotaAlert::new(
                    scope.clone(),
                    AlertLevel::Exceeded,
                    new_total,
                    limit.hard_limit,
                ));
            }
        }

        // Clear grace period if we're back under limit
        self.grace_periods.remove(scope);

        // Check soft limit
        if limit.exceeds_soft(new_total) {
            return EnforcementResult::AllowedWithWarning(QuotaAlert::new(
                scope.clone(),
                AlertLevel::Warning,
                new_total,
                limit.hard_limit,
            ));
        }

        // Check alert thresholds
        for &threshold in &limit.alert_thresholds {
            if percent_used >= threshold as f64 {
                let level = match threshold {
                    t if t >= 90 => AlertLevel::Critical,
                    t if t >= 75 => AlertLevel::Warning,
                    _ => AlertLevel::Info,
                };
                return EnforcementResult::AllowedWithWarning(QuotaAlert::new(
                    scope.clone(),
                    level,
                    new_total,
                    limit.hard_limit,
                ));
            }
        }

        EnforcementResult::Allowed
    }

    /// Generate alerts for current usage (without checking a specific operation)
    pub fn check_alerts(
        &self,
        policy: &QuotaPolicy,
        usage: &QuotaUsage,
    ) -> Vec<QuotaAlert> {
        let mut alerts = Vec::new();

        if !policy.enabled {
            return alerts;
        }

        if let Some(ref limit) = policy.storage_limit {
            let percent = limit.percent_used(usage.storage_bytes);
            if let Some(level) = Self::threshold_to_level(percent, &limit.alert_thresholds) {
                alerts.push(QuotaAlert::new(
                    policy.scope.clone(),
                    level,
                    usage.storage_bytes,
                    limit.hard_limit,
                ));
            }
        }

        if let Some(ref limit) = policy.object_limit {
            let percent = limit.percent_used(usage.object_count);
            if let Some(level) = Self::threshold_to_level(percent, &limit.alert_thresholds) {
                alerts.push(QuotaAlert::new(
                    policy.scope.clone(),
                    level,
                    usage.object_count,
                    limit.hard_limit,
                ));
            }
        }

        alerts
    }

    /// Convert percentage to alert level based on thresholds
    fn threshold_to_level(percent: f64, thresholds: &[u8]) -> Option<AlertLevel> {
        let mut triggered_threshold = None;

        for &t in thresholds {
            if percent >= t as f64 {
                triggered_threshold = Some(t);
            }
        }

        triggered_threshold.map(|t| match t {
            t if t >= 100 => AlertLevel::Exceeded,
            t if t >= 90 => AlertLevel::Critical,
            t if t >= 75 => AlertLevel::Warning,
            _ => AlertLevel::Info,
        })
    }
}

impl Default for QuotaEnforcement {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enforcement_allowed() {
        let enforcement = QuotaEnforcement::new();
        let policy = QuotaPolicy::bucket_standard("test", 100); // 100 GB
        let usage = QuotaUsage {
            storage_bytes: 10 * 1024 * 1024 * 1024, // 10 GB used
            ..Default::default()
        };

        let result = enforcement.check_storage_write(&policy, &usage, 1024);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_enforcement_denied() {
        let enforcement = QuotaEnforcement::new();
        let policy = QuotaPolicy::bucket_standard("test", 10); // 10 GB limit
        let usage = QuotaUsage {
            storage_bytes: 10 * 1024 * 1024 * 1024, // Already at 10 GB
            ..Default::default()
        };

        // Try to write 1 more byte
        let result = enforcement.check_storage_write(&policy, &usage, 1);
        assert!(result.is_denied());
    }

    #[test]
    fn test_enforcement_warning() {
        let enforcement = QuotaEnforcement::new();
        let policy = QuotaPolicy::bucket_standard("test", 100); // 100 GB
        let usage = QuotaUsage {
            storage_bytes: 95 * 1024 * 1024 * 1024, // 95% used
            ..Default::default()
        };

        let result = enforcement.check_storage_write(&policy, &usage, 1024);
        assert!(result.is_allowed());
        assert!(result.alert().is_some());
    }

    #[test]
    fn test_object_count_limit() {
        let enforcement = QuotaEnforcement::new();
        let mut policy = QuotaPolicy::new("test", QuotaScope::Bucket("test".to_string()));
        policy.object_limit = Some(QuotaLimit::object_count(100));

        let usage = QuotaUsage {
            object_count: 100, // At limit
            ..Default::default()
        };

        let result = enforcement.check_object_create(&policy, &usage, 1024);
        assert!(result.is_denied());
    }
}
