//! Arbiter/Witness System for Split-Brain Prevention
//!
//! Provides distributed consensus voting and split-brain detection for
//! WARP storage clusters. When network partitions occur, the arbiter
//! ensures only one partition can serve writes.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │              Arbiter System                         │
//! ├─────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐                 │
//! │  │   Witness   │  │    Vote      │                 │
//! │  │   Nodes     │──▶│   Tracker    │                 │
//! │  │ (lightweight)│  │  (quorum)    │                 │
//! │  └─────────────┘  └──────┬───────┘                 │
//! │                          │                          │
//! │  ┌───────────────────────▼───────────────────────┐ │
//! │  │           Split-Brain Detector                │ │
//! │  │  - Heartbeat monitoring                       │ │
//! │  │  - Partition detection                        │ │
//! │  │  - Quorum calculation                         │ │
//! │  └───────────────────────────────────────────────┘ │
//! │                          │                          │
//! │  ┌───────────────────────▼───────────────────────┐ │
//! │  │           Fencing Manager                     │ │
//! │  │  - STONITH support                            │ │
//! │  │  - I/O fencing                                │ │
//! │  │  - Recovery coordination                      │ │
//! │  └───────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quorum Rules
//!
//! - With N nodes, quorum requires (N/2)+1 votes
//! - Witness nodes count toward quorum but don't store data
//! - Even-numbered clusters should add a witness to break ties

mod detector;
mod fencing;
mod recovery;
mod vote;
mod witness;

pub use detector::{PartitionInfo, PartitionState, SplitBrainDetector};
pub use fencing::{FenceAction, FenceResult, FencingConfig, FencingManager};
pub use recovery::{RecoveryCoordinator, RecoveryPlan, RecoveryState};
pub use vote::{QuorumStatus, Vote, VoteResult, VoteTracker};
pub use witness::{WitnessConfig, WitnessHeartbeat, WitnessNode, WitnessState};

use std::time::Duration;

/// Configuration for the arbiter system
#[derive(Debug, Clone)]
pub struct ArbiterConfig {
    /// Heartbeat interval for liveness checks
    pub heartbeat_interval: Duration,

    /// Timeout before declaring a node dead
    pub node_timeout: Duration,

    /// Minimum nodes required for quorum (auto-calculated if None)
    pub min_quorum: Option<usize>,

    /// Enable automatic fencing
    pub auto_fence: bool,

    /// Fencing configuration
    pub fencing: FencingConfig,

    /// Maximum time to wait for recovery
    pub recovery_timeout: Duration,

    /// Enable witness node mode (lightweight, no data storage)
    pub witness_mode: bool,
}

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(1),
            node_timeout: Duration::from_secs(5),
            min_quorum: None, // Auto-calculate
            auto_fence: true,
            fencing: FencingConfig::default(),
            recovery_timeout: Duration::from_secs(60),
            witness_mode: false,
        }
    }
}

impl ArbiterConfig {
    /// Create config for a witness node
    pub fn witness() -> Self {
        Self {
            witness_mode: true,
            auto_fence: false, // Witnesses don't fence
            ..Default::default()
        }
    }

    /// Create config for aggressive split-brain detection
    pub fn aggressive() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(500),
            node_timeout: Duration::from_secs(2),
            auto_fence: true,
            ..Default::default()
        }
    }

    /// Create config for relaxed detection (high-latency networks)
    pub fn relaxed() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(5),
            node_timeout: Duration::from_secs(30),
            auto_fence: true,
            recovery_timeout: Duration::from_secs(300),
            ..Default::default()
        }
    }

    /// Calculate quorum size for a given cluster size
    pub fn calculate_quorum(cluster_size: usize) -> usize {
        (cluster_size / 2) + 1
    }
}

/// Node role in the arbiter system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeRole {
    /// Full storage node (votes and stores data)
    Storage,
    /// Witness node (votes but doesn't store data)
    Witness,
    /// Arbiter node (dedicated quorum tiebreaker)
    Arbiter,
}

impl NodeRole {
    /// Check if this role stores data
    pub fn stores_data(&self) -> bool {
        matches!(self, Self::Storage)
    }

    /// Check if this role participates in voting
    pub fn can_vote(&self) -> bool {
        true // All roles can vote
    }
}

/// Status of the arbiter system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbiterStatus {
    /// System is healthy with quorum
    Healthy,
    /// Quorum achieved but some nodes degraded
    Degraded,
    /// No quorum - reads only
    NoQuorum,
    /// Split-brain detected - fencing in progress
    SplitBrain,
    /// Recovery in progress
    Recovering,
    /// System is stopped
    Stopped,
}

impl ArbiterStatus {
    /// Check if writes are allowed
    pub fn writes_allowed(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }

    /// Check if reads are allowed
    pub fn reads_allowed(&self) -> bool {
        !matches!(self, Self::SplitBrain | Self::Stopped)
    }
}
