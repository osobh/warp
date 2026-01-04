//! Self-Heal Daemon for WARP Storage
//!
//! Autonomous background service that:
//! - Monitors shard health across all domains
//! - Detects degraded or lost shards
//! - Triggers repair operations using erasure coding
//! - Rebalances data when nodes join/leave
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │              Healer Daemon                      │
//! ├─────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐             │
//! │  │   Monitor   │  │ Repair Queue │             │
//! │  │  (periodic  │──▶│  (priority   │             │
//! │  │   scans)    │  │   ordered)   │             │
//! │  └─────────────┘  └──────┬───────┘             │
//! │                          │                      │
//! │  ┌───────────────────────▼───────────────────┐ │
//! │  │           Repair Workers (N)              │ │
//! │  │  - Reconstruct from parity                │ │
//! │  │  - Re-upload to healthy nodes             │ │
//! │  │  - Verify reconstruction                  │ │
//! │  └───────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────┘
//! ```

mod daemon;
mod metrics;
mod queue;
mod worker;

pub use daemon::{HealerConfig, HealerDaemon};
pub use metrics::{HealerMetrics, HealerStats};
pub use queue::{RepairJob, RepairPriority, RepairQueue};
pub use worker::{RepairResult, RepairWorker};
