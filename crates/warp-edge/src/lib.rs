//! Edge Intelligence for Portal Distributed Storage
//!
//! This crate provides edge discovery, health tracking, and intelligent
//! peer selection for the distributed storage network:
//!
//! - Edge registration and state tracking
//! - Chunk availability mapping (chunk → edges, edge → chunks)
//! - Bandwidth and RTT estimation
//! - Health scoring for edge selection
//! - Resource constraint tracking

pub mod types;
pub mod registry;
pub mod availability;
pub mod metrics;
pub mod health;
pub mod constraints;

pub use types::{EdgeId, EdgeInfo, EdgeCapabilities, EdgeState, EdgeStatus, EdgeType};
pub use registry::{EdgeRegistry, EdgeSnapshot};
pub use availability::{ChunkAvailabilityMap, ChunkLocation};
pub use metrics::{BandwidthEstimator, BandwidthMetrics, RttEstimator, RttMetrics};
pub use health::{HealthScorer, HealthScore, HealthComponents, HealthWeights};
pub use constraints::{ConstraintTracker, ResourceConstraints, BatteryConstraints, TimeWindow};

use thiserror::Error;

/// Edge intelligence errors
#[derive(Debug, Error)]
pub enum EdgeError {
    /// Edge not found in registry
    #[error("edge not found: {0}")]
    EdgeNotFound(String),

    /// Chunk not found in availability map
    #[error("chunk not found: {0}")]
    ChunkNotFound(String),

    /// Invalid health score value
    #[error("invalid health score: {0} (must be 0.0-1.0)")]
    InvalidHealthScore(f64),

    /// Metrics calculation overflow
    #[error("metrics overflow: {0}")]
    MetricsOverflow(String),

    /// Constraint violation
    #[error("constraint violation: {0}")]
    ConstraintViolation(String),

    /// Invalid virtual IP
    #[error("invalid virtual IP: {0}")]
    InvalidVirtualIp(String),

    /// Serialization error
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Result type for warp-edge operations
pub type Result<T> = std::result::Result<T, EdgeError>;
