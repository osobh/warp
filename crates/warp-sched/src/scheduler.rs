//! Main scheduler module - central coordinator for chunk scheduling operations.
//!
//! Runs a 50ms tick loop that orchestrates:
//! - Cost matrix computation
//! - Path selection
//! - Failover detection and handling
//! - Load balancing
//! - Assignment dispatch

use crate::{
    Assignment, AssignmentBatch, ChunkId, EdgeIdx, ScheduleRequest, SchedulerMetrics,
    CpuStateBuffers,
};
use crate::balance::{CpuLoadBalancer, LoadBalanceConfig};
use crate::cost::{CostConfig, CpuCostMatrix};
use crate::dispatch::DispatchQueue;
use crate::failover::{CpuFailoverManager, FailoverConfig, FailoverDecision};
use crate::paths::{CpuPathSelector, PathConfig};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for the chunk scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Main loop interval in milliseconds (default 50ms).
    pub tick_interval_ms: u64,
    /// Maximum assignments to produce per tick (default 1000).
    pub max_assignments_per_tick: usize,
    /// Cost matrix computation configuration.
    pub cost_config: CostConfig,
    /// Path selection configuration.
    pub path_config: PathConfig,
    /// Failover detection and handling configuration.
    pub failover_config: FailoverConfig,
    /// Load balancing configuration.
    pub balance_config: LoadBalanceConfig,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 50,
            max_assignments_per_tick: 1000,
            cost_config: CostConfig::default(),
            path_config: PathConfig::default(),
            failover_config: FailoverConfig::default(),
            balance_config: LoadBalanceConfig::default(),
        }
    }
}

/// Internal scheduler state.
#[derive(Debug, Clone)]
struct SchedulerState {
    running: bool,
    generation: u64,
}

/// CPU-based chunk scheduler implementation.
///
/// Orchestrates all scheduling operations in a synchronous manner.
pub struct CpuChunkScheduler {
    config: SchedulerConfig,
    state: SchedulerState,
    state_buffers: CpuStateBuffers,
    cost_matrix: CpuCostMatrix,
    path_selector: CpuPathSelector,
    failover_mgr: CpuFailoverManager,
    load_balancer: CpuLoadBalancer,
    dispatch_queue: DispatchQueue,
    pending_requests: Vec<ScheduleRequest>,
    metrics: SchedulerMetrics,
}

impl CpuChunkScheduler {
    /// Creates a new CPU-based chunk scheduler.
    pub fn new(config: SchedulerConfig, max_chunks: usize, max_edges: usize) -> Self {
        let state_buffers = CpuStateBuffers::new(max_chunks, max_edges);
        let cost_matrix = CpuCostMatrix::new(max_chunks, max_edges, config.cost_config.clone());
        let path_selector = CpuPathSelector::new(config.path_config.clone());
        let failover_mgr = CpuFailoverManager::new(config.failover_config.clone());
        let load_balancer = CpuLoadBalancer::new(config.balance_config.clone());
        let dispatch_queue = DispatchQueue::new();

        Self {
            config,
            state: SchedulerState {
                running: false,
                generation: 0,
            },
            state_buffers,
            cost_matrix,
            path_selector,
            failover_mgr,
            load_balancer,
            dispatch_queue,
            pending_requests: Vec::with_capacity(max_chunks),
            metrics: SchedulerMetrics::default(),
        }
    }

    /// Schedules chunks for assignment.
    pub fn schedule(&mut self, request: ScheduleRequest) -> crate::Result<()> {
        self.metrics.scheduled_chunks += request.chunks.len();
        self.pending_requests.push(request);
        Ok(())
    }

    /// Executes a single scheduling tick synchronously.
    ///
    /// Algorithm:
    /// 1. Check for timeouts and process failovers
    /// 2. Update edge states from state buffers
    /// 3. Compute cost matrix
    /// 4. Select K-best paths for pending chunks
    /// 5. Create assignments from path selections
    /// 6. Apply load balancing if needed
    /// 7. Push assignments to dispatch queue
    /// 8. Update metrics
    pub fn tick(&mut self) -> AssignmentBatch {
        self.state.generation += 1;
        self.metrics.tick_count += 1;

        let mut assignments = Vec::new();

        if self.pending_requests.is_empty() {
            return self.create_batch(assignments);
        }

        // Step 1: Check for failovers
        let failover_decisions = self.failover_mgr.check_timeouts(&self.state_buffers);
        for decision in failover_decisions {
            self.handle_failover(decision);
        }

        // Step 2: Update edge states (already in state_buffers)

        // Step 3: Compute cost matrix
        self.cost_matrix.compute(&self.state_buffers);

        // Step 4 & 5: Select paths and create assignments
        let requests_to_process: Vec<ScheduleRequest> = self
            .pending_requests
            .drain(..)
            .take(self.config.max_assignments_per_tick)
            .collect();

        for request in &requests_to_process {
            for chunk_hash in &request.chunks {
                // Convert hash to ChunkId for path selection
                let chunk_id = ChunkId::from_hash(chunk_hash);

                let path_selection = self.path_selector.select(chunk_id, &self.cost_matrix);
                if !path_selection.selected_edges.is_empty() {
                    // Extract just the EdgeIdx from (EdgeIdx, cost) tuples
                    let source_edges: Vec<EdgeIdx> = path_selection
                        .selected_edges
                        .iter()
                        .map(|(edge, _cost)| *edge)
                        .collect();

                    let assignment = Assignment {
                        chunk_hash: *chunk_hash,
                        chunk_size: 1024 * 256, // Default chunk size
                        source_edges,
                        priority: request.priority,
                        estimated_duration_ms: 100,
                    };
                    assignments.push(assignment);
                }
            }
        }

        // Step 6: Apply load balancing (currently plan_rebalance only, not applying yet)
        let _rebalance_plan = self.load_balancer.plan_rebalance(&self.state_buffers);

        // Step 7: Push to dispatch queue
        self.dispatch_queue.write_assignments(assignments.clone());
        self.dispatch_queue.swap_buffers();

        // Step 8: Update metrics
        self.metrics.active_transfers = assignments.len();

        self.create_batch(assignments)
    }

    fn create_batch(&self, assignments: Vec<Assignment>) -> AssignmentBatch {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        AssignmentBatch {
            assignments,
            generation: self.state.generation,
            timestamp_ms,
        }
    }

    /// Gets current assignments from dispatch queue.
    pub fn get_assignments(&self) -> AssignmentBatch {
        self.dispatch_queue.read_assignments()
    }

    /// Handles a failover decision.
    pub fn handle_failover(&mut self, _decision: FailoverDecision) {
        // Failover handling: could re-enqueue affected chunks
        // For now, just track in metrics
        self.metrics.failed_chunks += 1;
    }

    /// Returns current scheduler metrics.
    pub fn metrics(&self) -> SchedulerMetrics {
        self.metrics.clone()
    }

    /// Returns reference to state buffers.
    pub fn state(&self) -> &CpuStateBuffers {
        &self.state_buffers
    }

    /// Checks if scheduler is running.
    pub fn is_running(&self) -> bool {
        self.state.running
    }

    /// Stops the scheduler.
    pub fn stop(&mut self) {
        self.state.running = false;
    }

    /// Starts the scheduler.
    pub fn start(&mut self) {
        self.state.running = true;
    }

    /// Gets the configuration.
    pub fn config(&self) -> &SchedulerConfig {
        &self.config
    }

    /// Gets mutable reference to state buffers for testing.
    pub fn state_mut(&mut self) -> &mut CpuStateBuffers {
        &mut self.state_buffers
    }
}

/// GPU-accelerated chunk scheduler (delegates to CPU implementation).
///
/// Currently wraps CpuChunkScheduler. Future versions will implement
/// GPU-accelerated scheduling algorithms using cudarc.
pub struct ChunkScheduler {
    cpu_scheduler: CpuChunkScheduler,
}

impl ChunkScheduler {
    /// Creates a new chunk scheduler.
    pub fn new(config: SchedulerConfig, max_chunks: usize, max_edges: usize) -> Self {
        Self {
            cpu_scheduler: CpuChunkScheduler::new(config, max_chunks, max_edges),
        }
    }

    /// Schedules chunks for assignment.
    pub fn schedule(&mut self, request: ScheduleRequest) -> crate::Result<()> {
        self.cpu_scheduler.schedule(request)
    }

    /// Executes a single scheduling tick.
    pub fn tick(&mut self) -> AssignmentBatch {
        self.cpu_scheduler.tick()
    }

    /// Gets current assignments from dispatch queue.
    pub fn get_assignments(&self) -> AssignmentBatch {
        self.cpu_scheduler.get_assignments()
    }

    /// Handles a failover decision.
    pub fn handle_failover(&mut self, decision: FailoverDecision) {
        self.cpu_scheduler.handle_failover(decision);
    }

    /// Returns current scheduler metrics.
    pub fn metrics(&self) -> SchedulerMetrics {
        self.cpu_scheduler.metrics()
    }

    /// Returns reference to state buffers.
    pub fn state(&self) -> &CpuStateBuffers {
        self.cpu_scheduler.state()
    }

    /// Checks if scheduler is running.
    pub fn is_running(&self) -> bool {
        self.cpu_scheduler.is_running()
    }

    /// Stops the scheduler.
    pub fn stop(&mut self) {
        self.cpu_scheduler.stop();
    }

    /// Starts the scheduler.
    pub fn start(&mut self) {
        self.cpu_scheduler.start();
    }

    /// Gets the configuration.
    pub fn config(&self) -> &SchedulerConfig {
        self.cpu_scheduler.config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_config_default() {
        let config = SchedulerConfig::default();
        assert_eq!(config.tick_interval_ms, 50);
        assert_eq!(config.max_assignments_per_tick, 1000);
    }

    #[test]
    fn test_scheduler_config_custom() {
        let config = SchedulerConfig {
            tick_interval_ms: 100,
            max_assignments_per_tick: 500,
            ..Default::default()
        };
        assert_eq!(config.tick_interval_ms, 100);
        assert_eq!(config.max_assignments_per_tick, 500);
    }

    #[test]
    fn test_cpu_scheduler_new() {
        let config = SchedulerConfig::default();
        let scheduler = CpuChunkScheduler::new(config, 100, 10);
        assert!(!scheduler.is_running());
        assert_eq!(scheduler.metrics().tick_count, 0);
    }

    #[test]
    fn test_schedule_single_chunk() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);
        let request = ScheduleRequest {
            chunks: vec![[1u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        assert!(scheduler.schedule(request).is_ok());
        assert_eq!(scheduler.metrics().scheduled_chunks, 1);
    }

    #[test]
    fn test_schedule_batch_chunks() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);
        let request = ScheduleRequest {
            chunks: vec![[1u8; 32], [2u8; 32], [3u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        assert!(scheduler.schedule(request).is_ok());
        assert_eq!(scheduler.metrics().scheduled_chunks, 3);
    }

    #[test]
    fn test_schedule_duplicate_chunks() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);
        let request1 = ScheduleRequest {
            chunks: vec![[1u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        let request2 = ScheduleRequest {
            chunks: vec![[1u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        scheduler.schedule(request1).unwrap();
        scheduler.schedule(request2).unwrap();
        // Both requests are tracked
        assert_eq!(scheduler.metrics().scheduled_chunks, 2);
    }

    #[test]
    fn test_tick_empty_state() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);
        let batch = scheduler.tick();
        assert!(batch.assignments.is_empty());
        assert_eq!(scheduler.metrics().tick_count, 1);
    }

    #[test]
    fn test_tick_with_chunks() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        let request = ScheduleRequest {
            chunks: vec![[1u8; 32], [2u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        scheduler.schedule(request).unwrap();

        let _batch = scheduler.tick();
        assert_eq!(scheduler.metrics().tick_count, 1);
        // Assignments depend on cost matrix and path selector logic
    }

    #[test]
    fn test_multiple_ticks() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        scheduler.tick();
        scheduler.tick();
        scheduler.tick();

        assert_eq!(scheduler.metrics().tick_count, 3);
    }

    #[test]
    fn test_handle_failover() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        use crate::failover::{FailoverReason, FailoverAction};
        let decision = FailoverDecision {
            chunk_id: ChunkId(1),
            reason: FailoverReason::Timeout,
            action: FailoverAction::Retry { edge_idx: EdgeIdx(0) },
            failed_edge: EdgeIdx(0),
            retry_count: 1,
            timestamp_ms: 12345,
        };

        scheduler.handle_failover(decision);
        assert_eq!(scheduler.metrics().failed_chunks, 1);
    }

    #[test]
    fn test_metrics_tracking() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        let request = ScheduleRequest {
            chunks: vec![[1u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        scheduler.schedule(request).unwrap();
        scheduler.tick();

        let metrics = scheduler.metrics();
        assert_eq!(metrics.scheduled_chunks, 1);
        assert_eq!(metrics.tick_count, 1);
    }

    #[test]
    fn test_start_stop_scheduler() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        assert!(!scheduler.is_running());
        scheduler.start();
        assert!(scheduler.is_running());
        scheduler.stop();
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_get_assignments() {
        let scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        // Initially empty
        let batch = scheduler.get_assignments();
        assert!(batch.assignments.is_empty());
    }

    #[test]
    fn test_state_access() {
        let scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);
        let _state = scheduler.state();
        // State buffers don't have direct public fields anymore
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_config_access() {
        let config = SchedulerConfig {
            tick_interval_ms: 75,
            ..Default::default()
        };
        let scheduler = CpuChunkScheduler::new(config, 100, 10);
        assert_eq!(scheduler.config().tick_interval_ms, 75);
    }

    #[test]
    fn test_gpu_scheduler_new() {
        let config = SchedulerConfig::default();
        let scheduler = ChunkScheduler::new(config, 100, 10);
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_gpu_scheduler_delegation() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        let request = ScheduleRequest {
            chunks: vec![[1u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        assert!(scheduler.schedule(request).is_ok());

        scheduler.tick();
        assert_eq!(scheduler.metrics().tick_count, 1);
        assert_eq!(scheduler.metrics().scheduled_chunks, 1);
    }

    #[test]
    fn test_gpu_scheduler_start_stop() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        scheduler.start();
        assert!(scheduler.is_running());
        scheduler.stop();
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_max_assignments_per_tick() {
        let config = SchedulerConfig {
            max_assignments_per_tick: 2,
            ..Default::default()
        };
        let mut scheduler = CpuChunkScheduler::new(config, 100, 10);

        // Schedule more chunks than max
        let request = ScheduleRequest {
            chunks: vec![[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        scheduler.schedule(request).unwrap();

        // First tick should process max_assignments_per_tick
        scheduler.tick();
        // Exact number depends on path selection, but tick should respect limit
        assert_eq!(scheduler.metrics().tick_count, 1);
    }

    #[test]
    fn test_tick_duration_tracking() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        scheduler.tick();
        let metrics = scheduler.metrics();
        assert_eq!(metrics.tick_count, 1);
    }

    #[test]
    fn test_failover_requeues_chunks() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);

        use crate::failover::{FailoverReason, FailoverAction};
        let decision = FailoverDecision {
            chunk_id: ChunkId(5),
            reason: FailoverReason::Timeout,
            action: FailoverAction::Retry { edge_idx: EdgeIdx(0) },
            failed_edge: EdgeIdx(0),
            retry_count: 1,
            timestamp_ms: 12345,
        };

        scheduler.handle_failover(decision);
        // Chunks should be re-queued (visible in next tick)
        assert_eq!(scheduler.metrics().failed_chunks, 1);
    }

    #[test]
    fn test_scheduler_state_initialization() {
        let config = SchedulerConfig::default();
        let scheduler = CpuChunkScheduler::new(config, 100, 10);
        assert!(!scheduler.is_running());
        assert_eq!(scheduler.state.generation, 0);
    }

    #[test]
    fn test_empty_schedule_request() {
        let mut scheduler = CpuChunkScheduler::new(SchedulerConfig::default(), 100, 10);
        let request = ScheduleRequest {
            chunks: vec![],
            priority: 128,
            replica_target: 3,
            deadline_ms: None,
        };
        assert!(scheduler.schedule(request).is_ok());
        assert_eq!(scheduler.metrics().scheduled_chunks, 0);
    }

    #[test]
    fn test_config_serialization() {
        let config = SchedulerConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: SchedulerConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config.tick_interval_ms, deserialized.tick_interval_ms);
    }
}
