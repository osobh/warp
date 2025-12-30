//! Split-brain detection and partition monitoring

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use super::vote::{NodeId, VoteTracker, QuorumStatus};
use crate::replication::DomainId;

/// State of a network partition
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionState {
    /// No partition detected
    Healthy,
    /// Partition suspected (some nodes unreachable)
    Suspected,
    /// Partition confirmed (persistent unreachability)
    Confirmed,
    /// Partition is healing (nodes becoming reachable)
    Healing,
}

/// Information about a detected partition
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    /// Partition state
    pub state: PartitionState,

    /// Nodes in our partition
    pub our_partition: HashSet<NodeId>,

    /// Nodes in other partition(s)
    pub other_partitions: Vec<HashSet<NodeId>>,

    /// When partition was first detected
    pub detected_at: Option<SystemTime>,

    /// When partition was confirmed
    pub confirmed_at: Option<SystemTime>,

    /// Whether our partition has quorum
    pub we_have_quorum: bool,

    /// Estimated cause
    pub cause: Option<PartitionCause>,
}

impl Default for PartitionInfo {
    fn default() -> Self {
        Self {
            state: PartitionState::Healthy,
            our_partition: HashSet::new(),
            other_partitions: Vec::new(),
            detected_at: None,
            confirmed_at: None,
            we_have_quorum: true,
            cause: None,
        }
    }
}

/// Estimated cause of partition
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionCause {
    /// Network switch/router failure
    NetworkEquipment,
    /// WAN link failure
    WanLink,
    /// Node failure
    NodeFailure,
    /// Datacenter/zone isolation
    ZoneIsolation,
    /// Unknown cause
    Unknown,
}

/// Heartbeat record
#[derive(Debug, Clone)]
struct HeartbeatRecord {
    /// Node ID
    node_id: NodeId,
    /// Last heartbeat time
    last_heartbeat: Instant,
    /// Consecutive failures
    consecutive_failures: u32,
    /// Last latency
    latency: Option<Duration>,
    /// Historical latencies (for trend detection)
    latency_history: Vec<Duration>,
}

impl HeartbeatRecord {
    fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            last_heartbeat: Instant::now(),
            consecutive_failures: 0,
            latency: None,
            latency_history: Vec::with_capacity(100),
        }
    }

    fn record_success(&mut self, latency: Duration) {
        self.last_heartbeat = Instant::now();
        self.consecutive_failures = 0;
        self.latency = Some(latency);

        // Keep last 100 latencies
        if self.latency_history.len() >= 100 {
            self.latency_history.remove(0);
        }
        self.latency_history.push(latency);
    }

    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    fn is_responsive(&self, timeout: Duration) -> bool {
        self.last_heartbeat.elapsed() < timeout && self.consecutive_failures < 3
    }

    fn avg_latency(&self) -> Option<Duration> {
        if self.latency_history.is_empty() {
            return None;
        }
        let sum: Duration = self.latency_history.iter().sum();
        Some(sum / self.latency_history.len() as u32)
    }
}

/// Configuration for split-brain detection
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Heartbeat check interval
    pub check_interval: Duration,

    /// Time before marking node as suspected
    pub suspect_timeout: Duration,

    /// Time before confirming partition
    pub confirm_timeout: Duration,

    /// Consecutive failures before suspecting
    pub failure_threshold: u32,

    /// Whether to use latency trends
    pub use_latency_prediction: bool,

    /// Latency increase threshold for prediction (multiplier)
    pub latency_threshold: f64,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(1),
            suspect_timeout: Duration::from_secs(3),
            confirm_timeout: Duration::from_secs(10),
            failure_threshold: 3,
            use_latency_prediction: true,
            latency_threshold: 3.0, // 3x normal latency
        }
    }
}

/// Detects and monitors network partitions
pub struct SplitBrainDetector {
    /// This node's ID
    node_id: NodeId,

    /// Configuration
    config: DetectorConfig,

    /// Vote tracker reference
    vote_tracker: Arc<VoteTracker>,

    /// Heartbeat records per node
    heartbeats: DashMap<NodeId, HeartbeatRecord>,

    /// Current partition info
    partition_info: RwLock<PartitionInfo>,

    /// Nodes grouped by domain
    domain_nodes: DashMap<DomainId, HashSet<NodeId>>,

    /// History of partition events
    partition_history: RwLock<Vec<PartitionEvent>>,
}

/// A partition event for history tracking
#[derive(Debug, Clone)]
pub struct PartitionEvent {
    /// Event type
    pub event_type: PartitionEventType,
    /// Affected nodes
    pub affected_nodes: Vec<NodeId>,
    /// When it occurred
    pub timestamp: SystemTime,
    /// Duration (for healed events)
    pub duration: Option<Duration>,
}

/// Types of partition events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionEventType {
    /// Partition detected
    Detected,
    /// Partition confirmed
    Confirmed,
    /// Partition healed
    Healed,
    /// Node became unreachable
    NodeLost,
    /// Node became reachable again
    NodeRecovered,
}

impl SplitBrainDetector {
    /// Create a new detector
    pub fn new(node_id: NodeId, vote_tracker: Arc<VoteTracker>) -> Self {
        Self {
            node_id,
            config: DetectorConfig::default(),
            vote_tracker,
            heartbeats: DashMap::new(),
            partition_info: RwLock::new(PartitionInfo::default()),
            domain_nodes: DashMap::new(),
            partition_history: RwLock::new(Vec::new()),
        }
    }

    /// Create with custom config
    pub fn with_config(
        node_id: NodeId,
        vote_tracker: Arc<VoteTracker>,
        config: DetectorConfig,
    ) -> Self {
        Self {
            node_id,
            config,
            vote_tracker,
            heartbeats: DashMap::new(),
            partition_info: RwLock::new(PartitionInfo::default()),
            domain_nodes: DashMap::new(),
            partition_history: RwLock::new(Vec::new()),
        }
    }

    /// Register a node to monitor
    pub fn register_node(&self, node_id: NodeId, domain_id: DomainId) {
        self.heartbeats.insert(node_id, HeartbeatRecord::new(node_id));

        self.domain_nodes
            .entry(domain_id)
            .or_insert_with(HashSet::new)
            .insert(node_id);
    }

    /// Remove a node from monitoring
    pub fn remove_node(&self, node_id: NodeId) {
        self.heartbeats.remove(&node_id);

        // Remove from domain mapping
        for mut domain_nodes in self.domain_nodes.iter_mut() {
            domain_nodes.remove(&node_id);
        }
    }

    /// Record a successful heartbeat
    pub fn heartbeat_success(&self, node_id: NodeId, latency: Duration) {
        if let Some(mut record) = self.heartbeats.get_mut(&node_id) {
            record.record_success(latency);
        }

        // Update vote tracker
        self.vote_tracker.heartbeat(node_id);

        // Check if this heals a partition
        self.check_healing();
    }

    /// Record a failed heartbeat
    pub fn heartbeat_failure(&self, node_id: NodeId) {
        if let Some(mut record) = self.heartbeats.get_mut(&node_id) {
            record.record_failure();

            if record.consecutive_failures >= self.config.failure_threshold {
                self.vote_tracker.mark_unreachable(node_id);
            }
        }

        // Check for partition
        self.check_partition();
    }

    /// Run a full partition check
    pub fn check_partition(&self) -> PartitionState {
        let mut info = self.partition_info.write();

        // Collect unreachable nodes
        let unreachable: HashSet<NodeId> = self.heartbeats
            .iter()
            .filter(|r| !r.is_responsive(self.config.suspect_timeout))
            .map(|r| r.node_id)
            .collect();

        // Collect reachable nodes
        let reachable: HashSet<NodeId> = self.heartbeats
            .iter()
            .filter(|r| r.is_responsive(self.config.suspect_timeout))
            .map(|r| r.node_id)
            .collect();

        // Include ourselves in reachable
        let mut our_partition = reachable;
        our_partition.insert(self.node_id);

        if unreachable.is_empty() {
            // All nodes reachable
            if info.state != PartitionState::Healthy {
                self.record_event(PartitionEventType::Healed, Vec::new());
            }
            info.state = PartitionState::Healthy;
            info.our_partition = our_partition;
            info.other_partitions.clear();
            info.detected_at = None;
            info.confirmed_at = None;
            info.cause = None;
        } else {
            // Some nodes unreachable
            let quorum_status = self.vote_tracker.quorum_status();
            info.we_have_quorum = quorum_status.has_quorum;
            info.our_partition = our_partition;
            info.other_partitions = vec![unreachable.clone()];

            match info.state {
                PartitionState::Healthy => {
                    info.state = PartitionState::Suspected;
                    info.detected_at = Some(SystemTime::now());
                    info.cause = self.estimate_cause(&unreachable);
                    self.record_event(PartitionEventType::Detected, unreachable.iter().copied().collect());
                    warn!(
                        unreachable = ?unreachable,
                        cause = ?info.cause,
                        "Partition suspected"
                    );
                }
                PartitionState::Suspected => {
                    // Check if we should confirm
                    if let Some(detected) = info.detected_at {
                        if detected.elapsed().unwrap_or(Duration::ZERO) > self.config.confirm_timeout {
                            info.state = PartitionState::Confirmed;
                            info.confirmed_at = Some(SystemTime::now());
                            self.record_event(PartitionEventType::Confirmed, unreachable.iter().copied().collect());
                            warn!(
                                unreachable = ?unreachable,
                                we_have_quorum = info.we_have_quorum,
                                "Partition confirmed"
                            );
                        }
                    }
                }
                PartitionState::Confirmed | PartitionState::Healing => {
                    // Stay in confirmed/healing state
                }
            }
        }

        info.state
    }

    /// Check if partition is healing
    fn check_healing(&self) {
        let mut info = self.partition_info.write();

        if info.state == PartitionState::Confirmed {
            // Check if any previously unreachable nodes are now reachable
            let now_reachable: HashSet<NodeId> = self.heartbeats
                .iter()
                .filter(|r| r.is_responsive(self.config.suspect_timeout))
                .map(|r| r.node_id)
                .collect();

            let all_unreachable: HashSet<NodeId> = info.other_partitions
                .iter()
                .flatten()
                .copied()
                .collect();

            let recovered: Vec<NodeId> = all_unreachable
                .intersection(&now_reachable)
                .copied()
                .collect();

            if !recovered.is_empty() {
                info.state = PartitionState::Healing;
                for node in &recovered {
                    self.record_event(PartitionEventType::NodeRecovered, vec![*node]);
                }
                info!(recovered = ?recovered, "Partition healing");
            }
        }
    }

    /// Estimate the cause of partition
    fn estimate_cause(&self, unreachable: &HashSet<NodeId>) -> Option<PartitionCause> {
        // Check if all unreachable nodes are in the same domain
        let mut domains: HashSet<DomainId> = HashSet::new();

        for node_id in unreachable {
            for entry in self.domain_nodes.iter() {
                if entry.value().contains(node_id) {
                    domains.insert(*entry.key());
                }
            }
        }

        if domains.len() == 1 {
            // All unreachable nodes in one domain - likely zone isolation
            Some(PartitionCause::ZoneIsolation)
        } else if unreachable.len() == 1 {
            // Single node - likely node failure
            Some(PartitionCause::NodeFailure)
        } else if self.is_latency_degraded() {
            // High latency - might be WAN issue
            Some(PartitionCause::WanLink)
        } else {
            Some(PartitionCause::Unknown)
        }
    }

    /// Check if we're seeing latency degradation
    fn is_latency_degraded(&self) -> bool {
        if !self.config.use_latency_prediction {
            return false;
        }

        for record in self.heartbeats.iter() {
            if let (Some(current), Some(avg)) = (record.latency, record.avg_latency()) {
                if current > avg.mul_f64(self.config.latency_threshold) {
                    return true;
                }
            }
        }
        false
    }

    /// Record a partition event
    fn record_event(&self, event_type: PartitionEventType, affected_nodes: Vec<NodeId>) {
        let event = PartitionEvent {
            event_type,
            affected_nodes,
            timestamp: SystemTime::now(),
            duration: None,
        };

        let mut history = self.partition_history.write();
        history.push(event);

        // Keep last 1000 events
        if history.len() > 1000 {
            history.remove(0);
        }
    }

    /// Get current partition info
    pub fn partition_info(&self) -> PartitionInfo {
        self.partition_info.read().clone()
    }

    /// Get current state
    pub fn state(&self) -> PartitionState {
        self.partition_info.read().state
    }

    /// Check if we have quorum
    pub fn have_quorum(&self) -> bool {
        self.partition_info.read().we_have_quorum
    }

    /// Get partition history
    pub fn history(&self) -> Vec<PartitionEvent> {
        self.partition_history.read().clone()
    }

    /// Get node latency info
    pub fn node_latency(&self, node_id: NodeId) -> Option<Duration> {
        self.heartbeats.get(&node_id).and_then(|r| r.latency)
    }

    /// Check if a specific node is reachable
    pub fn is_node_reachable(&self, node_id: NodeId) -> bool {
        self.heartbeats
            .get(&node_id)
            .map(|r| r.is_responsive(self.config.suspect_timeout))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_healthy() {
        let vote_tracker = Arc::new(VoteTracker::new(1));
        let detector = SplitBrainDetector::new(1, vote_tracker);

        detector.register_node(2, 1);
        detector.register_node(3, 1);

        // Send heartbeats
        detector.heartbeat_success(2, Duration::from_millis(10));
        detector.heartbeat_success(3, Duration::from_millis(15));

        assert_eq!(detector.state(), PartitionState::Healthy);
        assert!(detector.have_quorum());
    }

    #[test]
    fn test_detector_suspected() {
        let vote_tracker = Arc::new(VoteTracker::new(1));
        let detector = SplitBrainDetector::with_config(
            1,
            vote_tracker,
            DetectorConfig {
                failure_threshold: 1,
                suspect_timeout: Duration::from_millis(10),
                ..Default::default()
            },
        );

        detector.register_node(2, 1);
        detector.register_node(3, 1);

        // Node 2 fails
        detector.heartbeat_failure(2);

        // Wait for suspect timeout
        std::thread::sleep(Duration::from_millis(20));

        let state = detector.check_partition();
        assert_eq!(state, PartitionState::Suspected);
    }

    #[test]
    fn test_partition_info() {
        let vote_tracker = Arc::new(VoteTracker::new(1));
        let detector = SplitBrainDetector::new(1, vote_tracker);

        let info = detector.partition_info();
        assert_eq!(info.state, PartitionState::Healthy);
        assert!(info.other_partitions.is_empty());
    }
}
