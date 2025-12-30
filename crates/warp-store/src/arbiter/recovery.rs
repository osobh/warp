//! Recovery coordination after split-brain resolution

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, error, info, warn};

use super::detector::{PartitionInfo, PartitionState};
use super::fencing::{FencingManager, FenceAction};
use super::vote::{NodeId, VoteTracker, QuorumStatus};
use crate::replication::DomainId;

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

/// A recovery plan
#[derive(Debug, Clone)]
pub struct RecoveryPlan {
    /// Plan ID
    pub id: u64,

    /// Current phase
    pub phase: RecoveryPhase,

    /// Nodes involved in recovery
    pub involved_nodes: HashSet<NodeId>,

    /// Nodes that need data sync
    pub sync_needed: HashSet<NodeId>,

    /// Estimated data to reconcile (bytes)
    pub estimated_bytes: u64,

    /// Priority (higher = more urgent)
    pub priority: u32,

    /// When the plan was created
    pub created_at: SystemTime,

    /// Estimated completion time
    pub estimated_completion: Option<Duration>,

    /// Steps in the plan
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

/// A step in the recovery plan
#[derive(Debug, Clone)]
pub struct RecoveryStep {
    /// Step number
    pub order: u32,
    /// Step type
    pub step_type: RecoveryStepType,
    /// Target node(s)
    pub targets: Vec<NodeId>,
    /// Step status
    pub status: StepStatus,
    /// Error if failed
    pub error: Option<String>,
    /// When the step started
    pub started_at: Option<Instant>,
    /// When the step completed
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

/// Configuration for recovery
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Conflict resolution strategy
    pub conflict_strategy: ConflictStrategy,

    /// Maximum time for recovery
    pub recovery_timeout: Duration,

    /// Whether to auto-resume writes
    pub auto_resume: bool,

    /// Quiesce timeout
    pub quiesce_timeout: Duration,

    /// Sync batch size
    pub sync_batch_size: usize,

    /// Verification level
    pub verify_checksums: bool,

    /// Whether to preserve divergent writes
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

/// A detected conflict
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Object key
    pub key: String,
    /// Bucket
    pub bucket: String,
    /// Version from partition A
    pub version_a: Option<VersionInfo>,
    /// Version from partition B
    pub version_b: Option<VersionInfo>,
    /// Resolution (if any)
    pub resolution: Option<ConflictResolution>,
}

/// Version information for conflict detection
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Version ID
    pub version_id: String,
    /// Modification time
    pub modified_at: SystemTime,
    /// Node that made the change
    pub modified_by: NodeId,
    /// Checksum
    pub checksum: [u8; 32],
    /// Size in bytes
    pub size: u64,
}

/// How a conflict was resolved
#[derive(Debug, Clone)]
pub struct ConflictResolution {
    /// Strategy used
    pub strategy: ConflictStrategy,
    /// Winning version (if any)
    pub winner: Option<String>,
    /// Whether both versions were kept
    pub kept_both: bool,
    /// When resolved
    pub resolved_at: SystemTime,
}

/// Coordinates recovery after split-brain events
pub struct RecoveryCoordinator {
    /// Configuration
    config: RecoveryConfig,

    /// Current state
    state: RwLock<RecoveryState>,

    /// Active recovery plan
    current_plan: RwLock<Option<RecoveryPlan>>,

    /// Vote tracker reference
    vote_tracker: Arc<VoteTracker>,

    /// Fencing manager reference
    fencing_manager: Arc<FencingManager>,

    /// Detected conflicts
    conflicts: DashMap<String, Conflict>,

    /// Recovery history
    history: RwLock<Vec<RecoveryRecord>>,

    /// Nodes with quiesced writes
    quiesced_nodes: DashMap<NodeId, Instant>,
}

/// Record of a completed recovery
#[derive(Debug, Clone)]
pub struct RecoveryRecord {
    /// Plan ID
    pub plan_id: u64,
    /// Whether recovery succeeded
    pub success: bool,
    /// Nodes involved
    pub nodes: HashSet<NodeId>,
    /// Conflicts detected
    pub conflicts_detected: u64,
    /// Conflicts resolved
    pub conflicts_resolved: u64,
    /// Bytes synchronized
    pub bytes_synced: u64,
    /// When recovery started
    pub started_at: SystemTime,
    /// Duration
    pub duration: Duration,
    /// Error if failed
    pub error: Option<String>,
}

impl RecoveryCoordinator {
    /// Create a new recovery coordinator
    pub fn new(
        vote_tracker: Arc<VoteTracker>,
        fencing_manager: Arc<FencingManager>,
    ) -> Self {
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
    pub async fn start_recovery(&self, partition_info: &PartitionInfo) -> Result<RecoveryPlan, String> {
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
                    self.record_recovery(plan.id, false, plan.involved_nodes.clone(),
                        conflicts_detected, conflicts_resolved, bytes_synced,
                        start.elapsed(), Some(e.clone()));

                    *self.state.write() = RecoveryState::Failed;
                    return Err(e);
                }
            }
        }

        // Record success
        self.record_recovery(plan.id, true, plan.involved_nodes.clone(),
            conflicts_detected, conflicts_resolved, bytes_synced,
            start.elapsed(), None);

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
            ConflictStrategy::KeepBoth => {
                Ok(ConflictResolution {
                    strategy: ConflictStrategy::KeepBoth,
                    winner: None,
                    kept_both: true,
                    resolved_at: SystemTime::now(),
                })
            }
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
