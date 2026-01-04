//! Quota Management for WARP Storage
//!
//! Provides storage quota enforcement at multiple levels:
//! - Per-bucket quotas
//! - Per-user quotas
//! - Hierarchical (namespace) quotas
//!
//! ## Features
//!
//! - Real-time usage tracking
//! - Soft and hard limits
//! - Alert thresholds
//! - Graceful degradation modes
//! - Usage reporting and history
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │              Quota Manager                          │
//! ├─────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐                 │
//! │  │   Policy    │  │    Usage     │                 │
//! │  │   Store     │  │   Tracker    │                 │
//! │  └─────────────┘  └──────────────┘                 │
//! │                          │                          │
//! │  ┌───────────────────────▼───────────────────────┐ │
//! │  │           Enforcement Engine                  │ │
//! │  │  - Pre-write checks                           │ │
//! │  │  - Soft/hard limit handling                   │ │
//! │  │  - Graceful degradation                       │ │
//! │  └───────────────────────────────────────────────┘ │
//! │                          │                          │
//! │  ┌───────────────────────▼───────────────────────┐ │
//! │  │           Alert System                        │ │
//! │  │  - Threshold notifications                    │ │
//! │  │  - Quota exceeded alerts                      │ │
//! │  └───────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────┘
//! ```

mod enforcement;
mod manager;
mod policy;
mod tracker;

pub use enforcement::{EnforcementResult, QuotaEnforcement, QuotaViolation};
pub use manager::{QuotaConfig, QuotaManager};
pub use policy::{QuotaLimit, QuotaLimitType, QuotaPolicy, QuotaScope};
pub use tracker::{QuotaUsage, UsageSnapshot, UsageTracker};

/// Alert for quota threshold crossings
#[derive(Debug, Clone)]
pub struct QuotaAlert {
    /// The scope that triggered the alert
    pub scope: QuotaScope,

    /// Alert level
    pub level: AlertLevel,

    /// Current usage
    pub current_usage: u64,

    /// Limit that was crossed
    pub limit: u64,

    /// Percentage used
    pub percent_used: f64,

    /// Human-readable message
    pub message: String,

    /// When the alert was generated
    pub timestamp: std::time::SystemTime,
}

/// Alert severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertLevel {
    /// Informational (e.g., 50% used)
    Info,
    /// Warning (e.g., 75% used)
    Warning,
    /// Critical (e.g., 90% used)
    Critical,
    /// Exceeded (over 100%)
    Exceeded,
}

impl AlertLevel {
    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Info => "Informational",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
            Self::Exceeded => "Quota Exceeded",
        }
    }
}

impl QuotaAlert {
    /// Create a new alert
    pub fn new(scope: QuotaScope, level: AlertLevel, current_usage: u64, limit: u64) -> Self {
        let percent_used = if limit > 0 {
            (current_usage as f64 / limit as f64) * 100.0
        } else {
            0.0
        };

        let message = format!(
            "{} quota {}: {:.1}% used ({} / {})",
            scope,
            level.description().to_lowercase(),
            percent_used,
            humanize_bytes(current_usage),
            humanize_bytes(limit),
        );

        Self {
            scope,
            level,
            current_usage,
            limit,
            percent_used,
            message,
            timestamp: std::time::SystemTime::now(),
        }
    }
}

/// Format bytes as human-readable string
fn humanize_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
