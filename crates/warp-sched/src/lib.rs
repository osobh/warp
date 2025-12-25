//! GPU-Accelerated Chunk Scheduler for Portal Distributed Storage
//!
//! This crate provides GPU-accelerated scheduling for millions of chunks
//! across thousands of edges, achieving <10ms scheduling latency for 10M
//! chunks with <50ms failover times.
//!
//! # Key Components
//!
//! - **State Buffers**: GPU-resident chunk and edge state
//! - **Cost Matrix**: Parallel computation of transfer costs
//! - **K-Best Paths**: Selection of optimal source edges
//! - **Failover Manager**: Sub-50ms failure detection and recovery
//! - **Load Balancer**: Prevent edge bottlenecks
//! - **Dispatch Queue**: CPU-readable scheduling output

pub mod types;
pub mod state;
pub mod cost;
pub mod paths;
pub mod failover;
pub mod balance;
pub mod dispatch;
pub mod scheduler;
pub mod reoptimize;
pub mod constraints;
pub mod brain_link;

pub use types::{
    ChunkId, EdgeIdx, ChunkState, ChunkStatus, EdgeStateGpu,
    Assignment, AssignmentBatch, ScheduleRequest, SchedulerMetrics,
};
pub use state::{GpuStateBuffers, CpuStateBuffers, StateSnapshot};
pub use cost::{CostMatrix, CpuCostMatrix, CostConfig};
pub use paths::{PathSelector, CpuPathSelector, PathSelection, PathConfig};
pub use failover::{FailoverManager, CpuFailoverManager, FailoverDecision, FailoverAction};
pub use balance::{LoadBalancer, CpuLoadBalancer, LoadBalanceConfig, RebalanceOp, RebalancePlan, LoadMetrics};
pub use dispatch::DispatchQueue;
pub use scheduler::{ChunkScheduler, CpuChunkScheduler, SchedulerConfig};
pub use reoptimize::{
    ReoptScope, ReoptStrategy, Reassignment, ReassignmentReason, ReoptPlan,
    IncrementalConfig, IncrementalScheduler, ReoptMetrics, ReoptState,
};
pub use constraints::{
    TimeWindow, TimeConstraint, CostConstraint, PowerConstraint,
    EdgeConstraints, ViolationSeverity, ConstraintViolation,
    ConstraintEvaluator, SchedulePolicy, PolicyEngine,
};
pub use brain_link::{
    BrainLink, BrainLinkStats, ChunkPlacement, ChunkPlacementRequest,
    CommunicationPattern, EdgeNodeInfo, NetworkLink, TransportType,
};

use thiserror::Error;

/// Scheduler error types
#[derive(Debug, Error)]
pub enum SchedError {
    /// GPU operation failed
    #[error("GPU error: {0}")]
    Gpu(String),

    /// Edge registry error
    #[error("Edge error: {0}")]
    Edge(#[from] warp_edge::EdgeError),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Scheduler not running
    #[error("Scheduler not running")]
    NotRunning,

    /// Buffer overflow
    #[error("Buffer overflow: {0}")]
    BufferOverflow(String),

    /// Resource exhausted
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// CUDA kernel error
    #[error("CUDA kernel error: {0}")]
    Kernel(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Result type for warp-sched operations
pub type Result<T> = std::result::Result<T, SchedError>;
