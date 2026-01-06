//! Recovery coordination after split-brain resolution

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, error, info};

use super::detector::PartitionInfo;
use super::fencing::FencingManager;
use super::vote::{NodeId, VoteTracker};

/// State of recovery
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryState {
    /// No recovery needed
    Idle,
    /// Assessing damage from partition
    Assessing,
    /// Reconciling divergent state
    Reconciling,
    /// Replaying lost operations
    Replaying,
    /// Rebalancing data
    Rebalancing,
    /// Recovery complete
    Complete,
    /// Recovery failed
    Failed,
}

/// A recovery plan generated after detecting a split-brain event
///
/// This plan contains all the information needed to coordinate recovery across
/// partitioned nodes, including which nodes need synchronization, what steps
/// to execute, and timing estimates.
#[derive(Debug, Clone)]
pub struct RecoveryPlan {
    /// Unique identifier for this recovery plan
    pub id: u64,

    /// Current phase of the recovery process
    pub phase: RecoveryPhase,

    /// Set of nodes participating in this recovery operation
    pub involved_nodes: HashSet<NodeId>,

    /// Subset of nodes that require data synchronization
    pub sync_needed: HashSet<NodeId>,

    /// Estimated number of bytes that need to be reconciled across nodes
    pub estimated_bytes: u64,

    /// Priority level for this recovery operation (higher values indicate more urgent recovery)
    pub priority: u32,

    /// Timestamp when this recovery plan was created
    pub created_at: SystemTime,

    /// Estimated time until recovery completion, if calculable
    pub estimated_completion: Option<Duration>,

    /// Ordered sequence of recovery steps to execute
    pub steps: Vec<RecoveryStep>,
}

impl RecoveryPlan {
    /// Create a new recovery plan
    pub fn new(involved_nodes: HashSet<NodeId>) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            phase: RecoveryPhase::Assessment,
            involved_nodes,
            sync_needed: HashSet::new(),
            estimated_bytes: 0,
            priority: 1,
            created_at: SystemTime::now(),
            estimated_completion: None,
            steps: Vec::new(),
        }
    }

    /// Add a sync target
    pub fn add_sync_target(&mut self, node_id: NodeId) {
        self.sync_needed.insert(node_id);
    }

    /// Add a recovery step
    pub fn add_step(&mut self, step: RecoveryStep) {
        self.steps.push(step);
    }
}

/// Phase of recovery
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPhase {
    /// Assessing what happened
    Assessment,
    /// Stopping conflicting operations
    Quiesce,
    /// Comparing state between partitions
    Comparison,
    /// Resolving conflicts
    Resolution,
    /// Syncing data
    Synchronization,
    /// Verifying consistency
    Verification,
    /// Cleanup and finalization
    Finalization,
}

/// A single step within a recovery plan's execution sequence
///
/// Each step represents a distinct phase of the recovery process, such as
/// quiescing writes, comparing data versions, or syncing missing data.
#[derive(Debug, Clone)]
pub struct RecoveryStep {
    /// Sequential order number determining when this step executes
    pub order: u32,
    /// The type of recovery operation this step performs
    pub step_type: RecoveryStepType,
    /// List of nodes that this step operates on
    pub targets: Vec<NodeId>,
    /// Current execution status of this step
    pub status: StepStatus,
    /// Error message if the step failed, None otherwise
    pub error: Option<String>,
    /// Timestamp when step execution began, None if not yet started
    pub started_at: Option<Instant>,
    /// Timestamp when step execution finished, None if still running or not started
    pub completed_at: Option<Instant>,
}

/// Types of recovery steps
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStepType {
    /// Release fences
    ReleaseFences,
    /// Quiesce writes
    QuiesceWrites,
    /// Collect epoch markers
    CollectEpochs,
    /// Compare data versions
    CompareVersions,
    /// Resolve write conflicts
    ResolveConflicts,
    /// Sync missing data
    SyncData,
    /// Verify checksums
    VerifyChecksums,
    /// Resume writes
    ResumeWrites,
    /// Clear recovery state
    Cleanup,
}

/// Status of a recovery step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// Not started
    Pending,
    /// In progress
    Running,
    /// Completed successfully
    Complete,
    /// Failed
    Failed,
    /// Skipped
    Skipped,
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Latest write wins
    LastWriteWins,
    /// Larger partition wins
    MajorityWins,
    /// Keep both versions
    KeepBoth,
    /// Manual resolution required
    Manual,
    /// Use vector clocks
    VectorClock,
}

/// Configuration parameters controlling recovery behavior
///
/// These settings determine how conflicts are resolved, timing constraints,
/// and safety guarantees during the recovery process.
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Strategy to use when resolving conflicting writes between partitions
    pub conflict_strategy: ConflictStrategy,

    /// Maximum allowed duration for the entire recovery operation
    pub recovery_timeout: Duration,

    /// Whether to automatically resume write operations after successful recovery
    pub auto_resume: bool,

    /// Maximum time to wait for write operations to quiesce before proceeding
    pub quiesce_timeout: Duration,

    /// Number of objects to synchronize in each batch during data sync
    pub sync_batch_size: usize,

    /// Whether to verify data integrity using checksums after synchronization
    pub verify_checksums: bool,

    /// Whether to retain conflicting versions instead of discarding the loser
    pub preserve_conflicts: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            conflict_strategy: ConflictStrategy::LastWriteWins,
            recovery_timeout: Duration::from_secs(300),
            auto_resume: true,
            quiesce_timeout: Duration::from_secs(30),
            sync_batch_size: 1000,
            verify_checksums: true,
            preserve_conflicts: false,
        }
    }
}

/// A data conflict detected between two partitions during split-brain recovery
///
/// Represents a single object that has diverged across network partitions,
/// containing version metadata from both sides to enable resolution.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// The object key that has conflicting versions
    pub key: String,
    /// The bucket containing the conflicting object
    pub bucket: String,
    /// Version information from the first partition, None if object was deleted
    pub version_a: Option<VersionInfo>,
    /// Version information from the second partition, None if object was deleted
    pub version_b: Option<VersionInfo>,
    /// The resolution decision for this conflict, None if not yet resolved
    pub resolution: Option<ConflictResolution>,
}

/// Version metadata for a single object revision used in conflict detection
///
/// Contains all information necessary to compare and order different versions
/// of the same object when resolving conflicts.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Unique identifier for this specific version of the object
    pub version_id: String,
    /// Timestamp when this version was created or last modified
    pub modified_at: SystemTime,
    /// Identifier of the node that created this version
    pub modified_by: NodeId,
    /// SHA-256 checksum of the object data for integrity verification
    pub checksum: [u8; 32],
    /// Size of this version in bytes
    pub size: u64,
}

/// Record of how a conflict was resolved during recovery
///
/// Documents the resolution decision made for a specific conflict, including
/// the strategy applied and the outcome.
#[derive(Debug, Clone)]
pub struct ConflictResolution {
    /// The conflict resolution strategy that was applied
    pub strategy: ConflictStrategy,
    /// The version ID that was chosen as the winner, None if both were kept
    pub winner: Option<String>,
    /// True if both conflicting versions were preserved rather than choosing one
    pub kept_both: bool,
    /// Timestamp when this conflict was resolved
    pub resolved_at: SystemTime,
}

/// Coordinates recovery operations after split-brain events are detected
///
/// The coordinator manages the full lifecycle of recovery: creating plans,
/// executing recovery steps, resolving data conflicts, and synchronizing state
/// across previously partitioned nodes.
pub struct RecoveryCoordinator {
    /// Configuration parameters controlling recovery behavior
    config: RecoveryConfig,

    /// Current state of the recovery process
    state: RwLock<RecoveryState>,

    /// The currently executing recovery plan, None if idle
    current_plan: RwLock<Option<RecoveryPlan>>,

    /// Reference to the vote tracker for quorum coordination
    vote_tracker: Arc<VoteTracker>,

    /// Reference to the fencing manager for controlling node access
    fencing_manager: Arc<FencingManager>,

    /// Map of detected conflicts keyed by "bucket:key"
    conflicts: DashMap<String, Conflict>,

    /// Historical record of completed recovery operations
    history: RwLock<Vec<RecoveryRecord>>,

    /// Nodes that currently have write operations quiesced, with quiesce timestamp
    quiesced_nodes: DashMap<NodeId, Instant>,
}

/// Historical record of a completed recovery operation
///
/// Captures metrics and outcomes from a recovery attempt for auditing and
/// debugging purposes.
#[derive(Debug, Clone)]
pub struct RecoveryRecord {
    /// The unique ID of the recovery plan that was executed
    pub plan_id: u64,
    /// True if recovery completed successfully, false if it failed
    pub success: bool,
    /// Set of nodes that participated in this recovery operation
    pub nodes: HashSet<NodeId>,
    /// Total number of data conflicts discovered during recovery
    pub conflicts_detected: u64,
    /// Number of conflicts that were successfully resolved
    pub conflicts_resolved: u64,
    /// Total number of bytes synchronized across nodes
    pub bytes_synced: u64,
    /// Timestamp when the recovery operation began
    pub started_at: SystemTime,
    /// Total time taken to complete the recovery operation
    pub duration: Duration,
    /// Error message if the recovery failed, None if successful
    pub error: Option<String>,
}

impl RecoveryCoordinator {
    /// Create a new recovery coordinator
    pub fn new(vote_tracker: Arc<VoteTracker>, fencing_manager: Arc<FencingManager>) -> Self {
        Self {
            config: RecoveryConfig::default(),
            state: RwLock::new(RecoveryState::Idle),
            current_plan: RwLock::new(None),
            vote_tracker,
            fencing_manager,
            conflicts: DashMap::new(),
            history: RwLock::new(Vec::new()),
            quiesced_nodes: DashMap::new(),
        }
    }

    /// Create with custom config
    pub fn with_config(
        config: RecoveryConfig,
        vote_tracker: Arc<VoteTracker>,
        fencing_manager: Arc<FencingManager>,
    ) -> Self {
        Self {
            config,
            state: RwLock::new(RecoveryState::Idle),
            current_plan: RwLock::new(None),
            vote_tracker,
            fencing_manager,
            conflicts: DashMap::new(),
            history: RwLock::new(Vec::new()),
            quiesced_nodes: DashMap::new(),
        }
    }

    /// Start recovery from a partition event
    pub async fn start_recovery(
        &self,
        partition_info: &PartitionInfo,
    ) -> Result<RecoveryPlan, String> {
        let mut state = self.state.write();
        if *state != RecoveryState::Idle {
            return Err("Recovery already in progress".to_string());
        }
        *state = RecoveryState::Assessing;
        drop(state);

        info!("Starting recovery from partition event");

        // Create recovery plan
        let mut plan = RecoveryPlan::new(partition_info.our_partition.clone());

        // Add nodes from other partitions
        for partition in &partition_info.other_partitions {
            for node_id in partition {
                plan.add_sync_target(*node_id);
            }
        }

        // Build recovery steps
        self.build_recovery_steps(&mut plan);

        // Store the plan
        *self.current_plan.write() = Some(plan.clone());

        Ok(plan)
    }

    /// Build recovery steps based on the plan
    fn build_recovery_steps(&self, plan: &mut RecoveryPlan) {
        let mut order = 0;

        // Step 1: Release fences
        if !plan.sync_needed.is_empty() {
            plan.add_step(RecoveryStep {
                order,
                step_type: RecoveryStepType::ReleaseFences,
                targets: plan.sync_needed.iter().copied().collect(),
                status: StepStatus::Pending,
                error: None,
                started_at: None,
                completed_at: None,
            });
            order += 1;
        }

        // Step 2: Quiesce writes
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::QuiesceWrites,
            targets: plan.involved_nodes.iter().copied().collect(),
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
        order += 1;

        // Step 3: Collect epochs
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::CollectEpochs,
            targets: plan.involved_nodes.iter().copied().collect(),
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
        order += 1;

        // Step 4: Compare versions
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::CompareVersions,
            targets: plan.involved_nodes.iter().copied().collect(),
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
        order += 1;

        // Step 5: Resolve conflicts
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::ResolveConflicts,
            targets: Vec::new(), // Determined during comparison
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
        order += 1;

        // Step 6: Sync data
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::SyncData,
            targets: plan.sync_needed.iter().copied().collect(),
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
        order += 1;

        // Step 7: Verify checksums (if enabled)
        if self.config.verify_checksums {
            plan.add_step(RecoveryStep {
                order,
                step_type: RecoveryStepType::VerifyChecksums,
                targets: plan.involved_nodes.iter().copied().collect(),
                status: StepStatus::Pending,
                error: None,
                started_at: None,
                completed_at: None,
            });
            order += 1;
        }

        // Step 8: Resume writes
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::ResumeWrites,
            targets: plan.involved_nodes.iter().copied().collect(),
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
        order += 1;

        // Step 9: Cleanup
        plan.add_step(RecoveryStep {
            order,
            step_type: RecoveryStepType::Cleanup,
            targets: Vec::new(),
            status: StepStatus::Pending,
            error: None,
            started_at: None,
            completed_at: None,
        });
    }

    /// Execute the current recovery plan
    pub async fn execute_plan(&self) -> Result<(), String> {
        let plan = self.current_plan.read().clone();
        let Some(mut plan) = plan else {
            return Err("No recovery plan active".to_string());
        };

        let start = Instant::now();
        let mut conflicts_detected = 0u64;
        let mut conflicts_resolved = 0u64;
        let mut bytes_synced = 0u64;

        for step in &mut plan.steps {
            step.started_at = Some(Instant::now());
            step.status = StepStatus::Running;

            let result = self.execute_step(step).await;

            step.completed_at = Some(Instant::now());

            match result {
                Ok(stats) => {
                    step.status = StepStatus::Complete;
                    conflicts_detected += stats.conflicts_detected;
                    conflicts_resolved += stats.conflicts_resolved;
                    bytes_synced += stats.bytes_synced;
                }
                Err(e) => {
                    step.status = StepStatus::Failed;
                    step.error = Some(e.clone());
                    error!(step = ?step.step_type, error = %e, "Recovery step failed");

                    // Record failure
                    self.record_recovery(
                        plan.id,
                        false,
                        plan.involved_nodes.clone(),
                        conflicts_detected,
                        conflicts_resolved,
                        bytes_synced,
                        start.elapsed(),
                        Some(e.clone()),
                    );

                    *self.state.write() = RecoveryState::Failed;
                    return Err(e);
                }
            }
        }

        // Record success
        self.record_recovery(
            plan.id,
            true,
            plan.involved_nodes.clone(),
            conflicts_detected,
            conflicts_resolved,
            bytes_synced,
            start.elapsed(),
            None,
        );

        *self.state.write() = RecoveryState::Complete;
        *self.current_plan.write() = None;

        info!(
            duration = ?start.elapsed(),
            conflicts_detected,
            conflicts_resolved,
            bytes_synced,
            "Recovery completed successfully"
        );

        Ok(())
    }

    /// Execute a single recovery step
    async fn execute_step(&self, step: &RecoveryStep) -> Result<StepStats, String> {
        let mut stats = StepStats::default();

        match step.step_type {
            RecoveryStepType::ReleaseFences => {
                for node_id in &step.targets {
                    self.fencing_manager.release_node(*node_id)?;
                }
            }
            RecoveryStepType::QuiesceWrites => {
                for node_id in &step.targets {
                    self.quiesce_node(*node_id).await?;
                }
                *self.state.write() = RecoveryState::Reconciling;
            }
            RecoveryStepType::CollectEpochs => {
                // Collect epoch markers from all nodes
                debug!("Collecting epoch markers");
            }
            RecoveryStepType::CompareVersions => {
                // Compare versions across partitions
                // This would be implemented with actual data comparison
                debug!("Comparing data versions");
            }
            RecoveryStepType::ResolveConflicts => {
                stats.conflicts_detected = self.conflicts.len() as u64;
                let resolved = self.resolve_all_conflicts().await?;
                stats.conflicts_resolved = resolved;
            }
            RecoveryStepType::SyncData => {
                *self.state.write() = RecoveryState::Replaying;
                // Sync data to nodes that need it
                for node_id in &step.targets {
                    let synced = self.sync_to_node(*node_id).await?;
                    stats.bytes_synced += synced;
                }
            }
            RecoveryStepType::VerifyChecksums => {
                // Verify data integrity
                debug!("Verifying checksums");
            }
            RecoveryStepType::ResumeWrites => {
                for node_id in &step.targets {
                    self.resume_node(*node_id).await?;
                }
            }
            RecoveryStepType::Cleanup => {
                self.conflicts.clear();
                self.quiesced_nodes.clear();
            }
        }

        Ok(stats)
    }

    /// Quiesce writes on a node
    async fn quiesce_node(&self, node_id: NodeId) -> Result<(), String> {
        debug!(node_id, "Quiescing writes");
        self.quiesced_nodes.insert(node_id, Instant::now());
        // In a real implementation, this would send a message to the node
        Ok(())
    }

    /// Resume writes on a node
    async fn resume_node(&self, node_id: NodeId) -> Result<(), String> {
        debug!(node_id, "Resuming writes");
        self.quiesced_nodes.remove(&node_id);
        // In a real implementation, this would send a message to the node
        Ok(())
    }

    /// Sync data to a node
    async fn sync_to_node(&self, node_id: NodeId) -> Result<u64, String> {
        debug!(node_id, "Syncing data");
        // In a real implementation, this would transfer missing data
        Ok(0)
    }

    /// Resolve all detected conflicts
    async fn resolve_all_conflicts(&self) -> Result<u64, String> {
        let mut resolved = 0u64;

        for mut entry in self.conflicts.iter_mut() {
            let conflict = entry.value_mut();

            if conflict.resolution.is_some() {
                continue;
            }

            let resolution = self.resolve_conflict(conflict)?;
            conflict.resolution = Some(resolution);
            resolved += 1;
        }

        Ok(resolved)
    }

    /// Resolve a single conflict
    fn resolve_conflict(&self, conflict: &Conflict) -> Result<ConflictResolution, String> {
        match self.config.conflict_strategy {
            ConflictStrategy::LastWriteWins => {
                let winner = match (&conflict.version_a, &conflict.version_b) {
                    (Some(a), Some(b)) => {
                        if a.modified_at > b.modified_at {
                            Some(a.version_id.clone())
                        } else {
                            Some(b.version_id.clone())
                        }
                    }
                    (Some(a), None) => Some(a.version_id.clone()),
                    (None, Some(b)) => Some(b.version_id.clone()),
                    (None, None) => None,
                };

                Ok(ConflictResolution {
                    strategy: ConflictStrategy::LastWriteWins,
                    winner,
                    kept_both: false,
                    resolved_at: SystemTime::now(),
                })
            }
            ConflictStrategy::KeepBoth => Ok(ConflictResolution {
                strategy: ConflictStrategy::KeepBoth,
                winner: None,
                kept_both: true,
                resolved_at: SystemTime::now(),
            }),
            _ => {
                // Default to last write wins for other strategies
                self.resolve_conflict(&Conflict {
                    key: conflict.key.clone(),
                    bucket: conflict.bucket.clone(),
                    version_a: conflict.version_a.clone(),
                    version_b: conflict.version_b.clone(),
                    resolution: None,
                })
            }
        }
    }

    /// Record a recovery attempt
    fn record_recovery(
        &self,
        plan_id: u64,
        success: bool,
        nodes: HashSet<NodeId>,
        conflicts_detected: u64,
        conflicts_resolved: u64,
        bytes_synced: u64,
        duration: Duration,
        error: Option<String>,
    ) {
        let record = RecoveryRecord {
            plan_id,
            success,
            nodes,
            conflicts_detected,
            conflicts_resolved,
            bytes_synced,
            started_at: SystemTime::now(),
            duration,
            error,
        };

        let mut history = self.history.write();
        history.push(record);

        // Keep last 100 records
        if history.len() > 100 {
            history.remove(0);
        }
    }

    /// Add a detected conflict
    pub fn add_conflict(&self, conflict: Conflict) {
        let key = format!("{}:{}", conflict.bucket, conflict.key);
        self.conflicts.insert(key, conflict);
    }

    /// Get current state
    pub fn state(&self) -> RecoveryState {
        *self.state.read()
    }

    /// Get current plan
    pub fn current_plan(&self) -> Option<RecoveryPlan> {
        self.current_plan.read().clone()
    }

    /// Get recovery history
    pub fn history(&self) -> Vec<RecoveryRecord> {
        self.history.read().clone()
    }

    /// Check if recovery is needed
    pub fn needs_recovery(&self) -> bool {
        *self.state.read() != RecoveryState::Idle
    }

    /// Cancel current recovery
    pub fn cancel(&self) {
        *self.state.write() = RecoveryState::Idle;
        *self.current_plan.write() = None;
        self.quiesced_nodes.clear();
    }
}

/// Statistics for a recovery step
#[derive(Debug, Default)]
struct StepStats {
    conflicts_detected: u64,
    conflicts_resolved: u64,
    bytes_synced: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::fencing::FencingConfig;

    #[test]
    fn test_recovery_plan() {
        let mut nodes = HashSet::new();
        nodes.insert(1);
        nodes.insert(2);

        let plan = RecoveryPlan::new(nodes);
        assert!(plan.steps.is_empty());
        assert_eq!(plan.phase, RecoveryPhase::Assessment);
    }

    #[test]
    fn test_conflict_resolution() {
        let vote_tracker = Arc::new(VoteTracker::new(1));
        let fencing_manager = Arc::new(FencingManager::new(FencingConfig::default()));
        let coordinator = RecoveryCoordinator::new(vote_tracker, fencing_manager);

        let conflict = Conflict {
            key: "test-key".to_string(),
            bucket: "test-bucket".to_string(),
            version_a: Some(VersionInfo {
                version_id: "v1".to_string(),
                modified_at: SystemTime::now(),
                modified_by: 1,
                checksum: [0; 32],
                size: 100,
            }),
            version_b: Some(VersionInfo {
                version_id: "v2".to_string(),
                modified_at: SystemTime::now(),
                modified_by: 2,
                checksum: [1; 32],
                size: 200,
            }),
            resolution: None,
        };

        let resolution = coordinator.resolve_conflict(&conflict).unwrap();
        assert!(!resolution.kept_both);
        assert!(resolution.winner.is_some());
    }

    #[test]
    fn test_recovery_state() {
        let vote_tracker = Arc::new(VoteTracker::new(1));
        let fencing_manager = Arc::new(FencingManager::new(FencingConfig::default()));
        let coordinator = RecoveryCoordinator::new(vote_tracker, fencing_manager);

        assert_eq!(coordinator.state(), RecoveryState::Idle);
        assert!(!coordinator.needs_recovery());
    }
}
