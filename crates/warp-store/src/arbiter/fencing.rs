//! Fencing (STONITH) management for split-brain resolution

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, error, info, warn};

use super::vote::NodeId;

/// Fence action to take on a node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceAction {
    /// Power off the node (hard STONITH)
    PowerOff,
    /// Graceful shutdown
    Shutdown,
    /// Block I/O access (I/O fencing)
    BlockIO,
    /// Revoke storage access
    RevokeStorage,
    /// Network isolation
    NetworkIsolate,
    /// No action (warning only)
    None,
}

impl FenceAction {
    /// Check if this action is destructive
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::PowerOff | Self::Shutdown)
    }

    /// Check if this action affects I/O
    pub fn affects_io(&self) -> bool {
        matches!(self, Self::BlockIO | Self::RevokeStorage)
    }
}

/// Result of a fencing operation
#[derive(Debug, Clone)]
pub struct FenceResult {
    /// Target node
    pub node_id: NodeId,
    /// Action taken
    pub action: FenceAction,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// When the operation completed
    pub completed_at: SystemTime,
    /// How long the operation took
    pub duration: Duration,
}

impl FenceResult {
    /// Create a successful result
    pub fn success(node_id: NodeId, action: FenceAction, duration: Duration) -> Self {
        Self {
            node_id,
            action,
            success: true,
            error: None,
            completed_at: SystemTime::now(),
            duration,
        }
    }

    /// Create a failed result
    pub fn failure(
        node_id: NodeId,
        action: FenceAction,
        error: String,
        duration: Duration,
    ) -> Self {
        Self {
            node_id,
            action,
            success: false,
            error: Some(error),
            completed_at: SystemTime::now(),
            duration,
        }
    }
}

/// Configuration for fencing
#[derive(Debug, Clone)]
pub struct FencingConfig {
    /// Default fence action
    pub default_action: FenceAction,

    /// Timeout for fence operations
    pub fence_timeout: Duration,

    /// Maximum retry attempts
    pub max_retries: u32,

    /// Delay between retries
    pub retry_delay: Duration,

    /// Whether to fence aggressively (fence on first sign of trouble)
    pub aggressive: bool,

    /// Actions to try in order (fallback chain)
    pub action_chain: Vec<FenceAction>,

    /// Nodes that should never be fenced (protected)
    pub protected_nodes: HashSet<NodeId>,

    /// Enable I/O fencing (SCSI reservations, etc.)
    pub io_fencing_enabled: bool,

    /// Fence agents configuration
    pub agents: HashMap<String, AgentConfig>,
}

impl Default for FencingConfig {
    fn default() -> Self {
        Self {
            default_action: FenceAction::BlockIO,
            fence_timeout: Duration::from_secs(60),
            max_retries: 3,
            retry_delay: Duration::from_secs(5),
            aggressive: false,
            action_chain: vec![
                FenceAction::BlockIO,
                FenceAction::RevokeStorage,
                FenceAction::Shutdown,
            ],
            protected_nodes: HashSet::new(),
            io_fencing_enabled: true,
            agents: HashMap::new(),
        }
    }
}

impl FencingConfig {
    /// Create a non-destructive config (I/O fencing only)
    pub fn non_destructive() -> Self {
        Self {
            default_action: FenceAction::BlockIO,
            action_chain: vec![FenceAction::BlockIO, FenceAction::RevokeStorage],
            ..Default::default()
        }
    }

    /// Create an aggressive config
    pub fn aggressive() -> Self {
        Self {
            aggressive: true,
            default_action: FenceAction::Shutdown,
            action_chain: vec![FenceAction::Shutdown, FenceAction::PowerOff],
            ..Default::default()
        }
    }

    /// Add a protected node
    pub fn protect_node(mut self, node_id: NodeId) -> Self {
        self.protected_nodes.insert(node_id);
        self
    }
}

/// Configuration for a fence agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent type (e.g., "ipmi", "aws", "gcp")
    pub agent_type: String,
    /// Agent-specific parameters
    pub parameters: HashMap<String, String>,
    /// Timeout override
    pub timeout: Option<Duration>,
}

/// State of a fenced node
#[derive(Debug, Clone)]
pub struct FencedNode {
    /// Node ID
    pub node_id: NodeId,
    /// Current fence state
    pub state: FenceState,
    /// Action that was taken
    pub action: FenceAction,
    /// When fencing started
    pub started_at: SystemTime,
    /// When fencing completed
    pub completed_at: Option<SystemTime>,
    /// Number of attempts
    pub attempts: u32,
    /// Last error
    pub last_error: Option<String>,
}

/// State of fencing for a node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceState {
    /// Fencing not started
    Pending,
    /// Fencing in progress
    InProgress,
    /// Successfully fenced
    Fenced,
    /// Fencing failed
    Failed,
    /// Node was unfenced
    Released,
}

/// Manages fencing operations
pub struct FencingManager {
    /// Configuration
    config: FencingConfig,

    /// Currently fenced nodes
    fenced_nodes: DashMap<NodeId, FencedNode>,

    /// Pending fence requests
    pending_fences: DashMap<NodeId, FenceAction>,

    /// Fence operation counter
    fence_counter: AtomicU64,

    /// Fence history
    history: RwLock<Vec<FenceResult>>,

    /// Active fence agents
    agents: DashMap<String, Box<dyn FenceAgent + Send + Sync>>,
}

/// Trait for fence agent implementations
pub trait FenceAgent: Send + Sync {
    /// Execute a fence action
    fn fence(&self, node_id: NodeId, action: FenceAction) -> Result<(), String>;

    /// Check if a node is fenced
    fn is_fenced(&self, node_id: NodeId) -> bool;

    /// Release a fenced node
    fn release(&self, node_id: NodeId) -> Result<(), String>;

    /// Get agent name
    fn name(&self) -> &str;
}

/// Simulated fence agent for testing
pub struct SimulatedFenceAgent {
    name: String,
    fenced: DashMap<NodeId, bool>,
}

impl SimulatedFenceAgent {
    /// Create a new simulated agent
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fenced: DashMap::new(),
        }
    }
}

impl FenceAgent for SimulatedFenceAgent {
    fn fence(&self, node_id: NodeId, _action: FenceAction) -> Result<(), String> {
        self.fenced.insert(node_id, true);
        Ok(())
    }

    fn is_fenced(&self, node_id: NodeId) -> bool {
        self.fenced.get(&node_id).map(|v| *v).unwrap_or(false)
    }

    fn release(&self, node_id: NodeId) -> Result<(), String> {
        self.fenced.insert(node_id, false);
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl FencingManager {
    /// Create a new fencing manager
    pub fn new(config: FencingConfig) -> Self {
        Self {
            config,
            fenced_nodes: DashMap::new(),
            pending_fences: DashMap::new(),
            fence_counter: AtomicU64::new(0),
            history: RwLock::new(Vec::new()),
            agents: DashMap::new(),
        }
    }

    /// Register a fence agent
    pub fn register_agent(&self, agent: Box<dyn FenceAgent + Send + Sync>) {
        let name = agent.name().to_string();
        self.agents.insert(name, agent);
    }

    /// Request fencing of a node
    pub fn request_fence(&self, node_id: NodeId, action: Option<FenceAction>) -> bool {
        // Check if protected
        if self.config.protected_nodes.contains(&node_id) {
            warn!(node_id, "Cannot fence protected node");
            return false;
        }

        // Check if already fenced or pending
        if self.fenced_nodes.contains_key(&node_id) {
            debug!(node_id, "Node already fenced");
            return false;
        }

        let action = action.unwrap_or(self.config.default_action);
        self.pending_fences.insert(node_id, action);

        info!(node_id, ?action, "Fence requested");
        true
    }

    /// Execute pending fence operations
    pub async fn execute_pending(&self) -> Vec<FenceResult> {
        let mut results = Vec::new();

        // Collect pending fences
        let pending: Vec<(NodeId, FenceAction)> = self
            .pending_fences
            .iter()
            .map(|r| (*r.key(), *r.value()))
            .collect();

        for (node_id, action) in pending {
            self.pending_fences.remove(&node_id);

            // Create fenced node record
            let fenced = FencedNode {
                node_id,
                state: FenceState::InProgress,
                action,
                started_at: SystemTime::now(),
                completed_at: None,
                attempts: 0,
                last_error: None,
            };
            self.fenced_nodes.insert(node_id, fenced);

            // Execute fence
            let result = self.execute_fence(node_id, action).await;
            results.push(result.clone());

            // Update state
            if let Some(mut node) = self.fenced_nodes.get_mut(&node_id) {
                if result.success {
                    node.state = FenceState::Fenced;
                } else {
                    node.state = FenceState::Failed;
                    node.last_error = result.error.clone();
                }
                node.completed_at = Some(SystemTime::now());
            }

            // Record in history
            self.history.write().push(result);
        }

        results
    }

    /// Execute a fence operation
    async fn execute_fence(&self, node_id: NodeId, action: FenceAction) -> FenceResult {
        let start = Instant::now();
        let mut last_error = None;

        // Try each action in the chain
        let actions = if self.config.action_chain.contains(&action) {
            self.config.action_chain.clone()
        } else {
            vec![action]
        };

        for current_action in actions {
            for attempt in 0..self.config.max_retries {
                info!(node_id, ?current_action, attempt, "Executing fence action");

                // Try each registered agent
                for agent in self.agents.iter() {
                    match agent.fence(node_id, current_action) {
                        Ok(()) => {
                            info!(node_id, agent = agent.name(), "Fence succeeded");
                            return FenceResult::success(node_id, current_action, start.elapsed());
                        }
                        Err(e) => {
                            warn!(
                                node_id,
                                agent = agent.name(),
                                error = %e,
                                "Fence attempt failed"
                            );
                            last_error = Some(e);
                        }
                    }
                }

                // Wait before retry
                if attempt < self.config.max_retries - 1 {
                    tokio::time::sleep(self.config.retry_delay).await;
                }
            }
        }

        error!(node_id, "All fence attempts failed");
        FenceResult::failure(
            node_id,
            action,
            last_error.unwrap_or_else(|| "No fence agents available".to_string()),
            start.elapsed(),
        )
    }

    /// Release a fenced node
    pub fn release_node(&self, node_id: NodeId) -> Result<(), String> {
        if let Some(mut node) = self.fenced_nodes.get_mut(&node_id) {
            // Release through all agents
            for agent in self.agents.iter() {
                if agent.is_fenced(node_id) {
                    agent.release(node_id)?;
                }
            }

            node.state = FenceState::Released;
            info!(node_id, "Node released from fence");
            Ok(())
        } else {
            Err(format!("Node {} not fenced", node_id))
        }
    }

    /// Check if a node is fenced
    pub fn is_fenced(&self, node_id: NodeId) -> bool {
        self.fenced_nodes
            .get(&node_id)
            .map(|n| n.state == FenceState::Fenced)
            .unwrap_or(false)
    }

    /// Get fenced node info
    pub fn get_fenced_node(&self, node_id: NodeId) -> Option<FencedNode> {
        self.fenced_nodes.get(&node_id).map(|r| r.clone())
    }

    /// Get all fenced nodes
    pub fn fenced_nodes(&self) -> Vec<FencedNode> {
        self.fenced_nodes
            .iter()
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get fence history
    pub fn history(&self) -> Vec<FenceResult> {
        self.history.read().clone()
    }

    /// Clear fence history
    pub fn clear_history(&self) {
        self.history.write().clear();
    }

    /// Get pending fence count
    pub fn pending_count(&self) -> usize {
        self.pending_fences.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fencing_config() {
        let config = FencingConfig::default();
        assert!(!config.aggressive);
        assert!(!config.default_action.is_destructive());
    }

    #[test]
    fn test_fence_request() {
        let config = FencingConfig::default();
        let manager = FencingManager::new(config);

        assert!(manager.request_fence(1, None));
        assert_eq!(manager.pending_count(), 1);
    }

    #[test]
    fn test_protected_node() {
        let config = FencingConfig::default().protect_node(1);
        let manager = FencingManager::new(config);

        assert!(!manager.request_fence(1, None));
        assert_eq!(manager.pending_count(), 0);
    }

    #[test]
    fn test_simulated_agent() {
        let agent = SimulatedFenceAgent::new("test");

        assert!(!agent.is_fenced(1));
        agent.fence(1, FenceAction::BlockIO).unwrap();
        assert!(agent.is_fenced(1));
        agent.release(1).unwrap();
        assert!(!agent.is_fenced(1));
    }
}
