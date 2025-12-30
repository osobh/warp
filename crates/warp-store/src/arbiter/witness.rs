//! Witness node implementation for quorum tiebreaking

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use super::vote::{NodeId, ElectionId, Vote, VoteTracker, QuorumStatus};
use crate::replication::DomainId;

/// State of a witness node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessState {
    /// Witness is starting up
    Starting,
    /// Witness is operational
    Active,
    /// Witness is participating in an election
    Voting,
    /// Witness has lost connectivity
    Disconnected,
    /// Witness is shutting down
    ShuttingDown,
    /// Witness is stopped
    Stopped,
}

/// Configuration for a witness node
#[derive(Debug, Clone)]
pub struct WitnessConfig {
    /// Heartbeat interval
    pub heartbeat_interval: Duration,

    /// Timeout for peer connections
    pub peer_timeout: Duration,

    /// Domain ID for this witness
    pub domain_id: DomainId,

    /// Minimum peers before becoming active
    pub min_peers: usize,

    /// Whether to auto-vote in elections
    pub auto_vote: bool,

    /// Vote preference (higher = prefer this node as leader)
    pub vote_priority: u32,

    /// Maximum concurrent elections
    pub max_elections: usize,
}

impl Default for WitnessConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(1),
            peer_timeout: Duration::from_secs(5),
            domain_id: 0,
            min_peers: 2,
            auto_vote: true,
            vote_priority: 0,
            max_elections: 10,
        }
    }
}

impl WitnessConfig {
    /// Create config for a specific domain
    pub fn for_domain(domain_id: DomainId) -> Self {
        Self {
            domain_id,
            ..Default::default()
        }
    }

    /// Create config with high priority
    pub fn high_priority() -> Self {
        Self {
            vote_priority: 100,
            ..Default::default()
        }
    }
}

/// A heartbeat message from a witness
#[derive(Debug, Clone)]
pub struct WitnessHeartbeat {
    /// Source witness ID
    pub witness_id: NodeId,

    /// Sequence number
    pub sequence: u64,

    /// Timestamp
    pub timestamp: SystemTime,

    /// Current state
    pub state: WitnessState,

    /// Known peers
    pub known_peers: Vec<NodeId>,

    /// Current election (if any)
    pub current_election: Option<ElectionId>,

    /// Last vote cast
    pub last_vote: Option<Vote>,
}

impl WitnessHeartbeat {
    /// Create a new heartbeat
    pub fn new(witness_id: NodeId, sequence: u64, state: WitnessState) -> Self {
        Self {
            witness_id,
            sequence,
            timestamp: SystemTime::now(),
            state,
            known_peers: Vec::new(),
            current_election: None,
            last_vote: None,
        }
    }
}

/// Peer information tracked by witness
#[derive(Debug, Clone)]
struct PeerInfo {
    /// Peer node ID
    node_id: NodeId,
    /// Is this a storage node?
    is_storage: bool,
    /// Domain ID
    domain_id: DomainId,
    /// Last heartbeat received
    last_heartbeat: Instant,
    /// Last heartbeat sequence
    last_sequence: u64,
    /// Is peer currently reachable?
    reachable: bool,
}

impl PeerInfo {
    fn new(node_id: NodeId, is_storage: bool, domain_id: DomainId) -> Self {
        Self {
            node_id,
            is_storage,
            domain_id,
            last_heartbeat: Instant::now(),
            last_sequence: 0,
            reachable: true,
        }
    }

    fn update_heartbeat(&mut self, sequence: u64) {
        self.last_heartbeat = Instant::now();
        self.last_sequence = sequence;
        self.reachable = true;
    }

    fn is_alive(&self, timeout: Duration) -> bool {
        self.last_heartbeat.elapsed() < timeout
    }
}

/// A lightweight witness node that participates in voting but doesn't store data
pub struct WitnessNode {
    /// Node ID
    node_id: NodeId,

    /// Configuration
    config: WitnessConfig,

    /// Current state
    state: RwLock<WitnessState>,

    /// Vote tracker
    vote_tracker: Arc<VoteTracker>,

    /// Known peers
    peers: DashMap<NodeId, PeerInfo>,

    /// Heartbeat sequence counter
    sequence: AtomicU64,

    /// Active elections this witness is participating in
    active_elections: DashMap<ElectionId, ElectionParticipation>,

    /// Votes cast by this witness
    votes_cast: RwLock<Vec<Vote>>,

    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
}

/// Participation in an election
#[derive(Debug, Clone)]
struct ElectionParticipation {
    /// Election ID
    election_id: ElectionId,
    /// Term
    term: u64,
    /// When we joined
    joined_at: Instant,
    /// Our vote (if cast)
    vote: Option<Vote>,
    /// Votes we've seen
    seen_votes: HashMap<NodeId, Vote>,
}

impl WitnessNode {
    /// Create a new witness node
    pub fn new(node_id: NodeId, config: WitnessConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let vote_tracker = Arc::new(VoteTracker::new(node_id));

        Self {
            node_id,
            config,
            state: RwLock::new(WitnessState::Starting),
            vote_tracker,
            peers: DashMap::new(),
            sequence: AtomicU64::new(0),
            active_elections: DashMap::new(),
            votes_cast: RwLock::new(Vec::new()),
            shutdown_tx,
        }
    }

    /// Create with external vote tracker
    pub fn with_vote_tracker(
        node_id: NodeId,
        config: WitnessConfig,
        vote_tracker: Arc<VoteTracker>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            node_id,
            config,
            state: RwLock::new(WitnessState::Starting),
            vote_tracker,
            peers: DashMap::new(),
            sequence: AtomicU64::new(0),
            active_elections: DashMap::new(),
            votes_cast: RwLock::new(Vec::new()),
            shutdown_tx,
        }
    }

    /// Start the witness node
    pub async fn start(&self) -> Result<(), String> {
        let mut state = self.state.write();
        if *state != WitnessState::Starting && *state != WitnessState::Stopped {
            return Ok(());
        }
        *state = WitnessState::Active;
        drop(state);

        info!(node_id = self.node_id, "Witness node started");

        // Register ourselves with vote tracker
        self.vote_tracker.register_node(self.node_id, self.config.domain_id, true);

        // Start heartbeat loop
        self.spawn_heartbeat_loop();

        Ok(())
    }

    /// Stop the witness node
    pub async fn stop(&self) -> Result<(), String> {
        let mut state = self.state.write();
        if *state == WitnessState::Stopped {
            return Ok(());
        }
        *state = WitnessState::ShuttingDown;
        drop(state);

        info!(node_id = self.node_id, "Stopping witness node");

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        *self.state.write() = WitnessState::Stopped;

        Ok(())
    }

    /// Register a peer
    pub fn register_peer(&self, node_id: NodeId, is_storage: bool, domain_id: DomainId) {
        self.peers.insert(node_id, PeerInfo::new(node_id, is_storage, domain_id));
        self.vote_tracker.register_node(node_id, domain_id, !is_storage);

        debug!(
            node_id,
            is_storage,
            domain_id,
            "Registered peer"
        );
    }

    /// Remove a peer
    pub fn remove_peer(&self, node_id: NodeId) {
        self.peers.remove(&node_id);
        self.vote_tracker.remove_node(node_id);
    }

    /// Process a heartbeat from a peer
    pub fn receive_heartbeat(&self, heartbeat: WitnessHeartbeat) {
        if let Some(mut peer) = self.peers.get_mut(&heartbeat.witness_id) {
            peer.update_heartbeat(heartbeat.sequence);
        }

        // Update vote tracker
        self.vote_tracker.heartbeat(heartbeat.witness_id);

        // Track their election participation
        if let Some(election_id) = heartbeat.current_election {
            if let Some(vote) = heartbeat.last_vote {
                self.record_seen_vote(election_id, vote);
            }
        }
    }

    /// Generate a heartbeat message
    pub fn generate_heartbeat(&self) -> WitnessHeartbeat {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let state = *self.state.read();

        let known_peers: Vec<NodeId> = self.peers
            .iter()
            .filter(|p| p.is_alive(self.config.peer_timeout))
            .map(|p| p.node_id)
            .collect();

        // Get current election if any
        let current_election = self.active_elections
            .iter()
            .next()
            .map(|e| e.election_id);

        // Get last vote
        let last_vote = self.votes_cast.read().last().cloned();

        WitnessHeartbeat {
            witness_id: self.node_id,
            sequence,
            timestamp: SystemTime::now(),
            state,
            known_peers,
            current_election,
            last_vote,
        }
    }

    /// Participate in an election
    pub fn join_election(&self, election_id: ElectionId, term: u64) {
        if self.active_elections.len() >= self.config.max_elections {
            warn!(
                election_id,
                "Too many active elections, rejecting"
            );
            return;
        }

        self.active_elections.insert(election_id, ElectionParticipation {
            election_id,
            term,
            joined_at: Instant::now(),
            vote: None,
            seen_votes: HashMap::new(),
        });

        *self.state.write() = WitnessState::Voting;

        debug!(election_id, term, "Joined election");
    }

    /// Cast a vote in an election
    pub fn cast_vote(&self, election_id: ElectionId, candidate: NodeId, term: u64) -> Option<Vote> {
        // Check if we're in this election
        let mut participation = self.active_elections.get_mut(&election_id)?;

        // Only vote once
        if participation.vote.is_some() {
            return participation.vote.clone();
        }

        let vote = Vote::new(self.node_id, election_id, candidate, term);

        // Record our vote
        participation.vote = Some(vote.clone());
        self.votes_cast.write().push(vote.clone());

        // Submit to vote tracker
        self.vote_tracker.cast_vote(vote.clone());

        info!(
            election_id,
            candidate,
            term,
            "Cast vote"
        );

        Some(vote)
    }

    /// Auto-vote based on configuration
    pub fn auto_vote(&self, election_id: ElectionId, candidates: &[NodeId], term: u64) -> Option<Vote> {
        if !self.config.auto_vote {
            return None;
        }

        // Vote for highest priority candidate (or first if equal)
        // In a real implementation, this would use more sophisticated logic
        let candidate = candidates.first().copied()?;

        self.cast_vote(election_id, candidate, term)
    }

    /// Record a vote we've seen from another node
    fn record_seen_vote(&self, election_id: ElectionId, vote: Vote) {
        if let Some(mut participation) = self.active_elections.get_mut(&election_id) {
            participation.seen_votes.insert(vote.node_id, vote);
        }
    }

    /// Leave an election
    pub fn leave_election(&self, election_id: ElectionId) {
        self.active_elections.remove(&election_id);

        // Return to active state if no more elections
        if self.active_elections.is_empty() {
            *self.state.write() = WitnessState::Active;
        }
    }

    /// Get current state
    pub fn state(&self) -> WitnessState {
        *self.state.read()
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get quorum status
    pub fn quorum_status(&self) -> QuorumStatus {
        self.vote_tracker.quorum_status()
    }

    /// Get reachable peer count
    pub fn reachable_peers(&self) -> usize {
        self.peers
            .iter()
            .filter(|p| p.is_alive(self.config.peer_timeout))
            .count()
    }

    /// Check if we have minimum peers
    pub fn has_min_peers(&self) -> bool {
        self.reachable_peers() >= self.config.min_peers
    }

    /// Get active election count
    pub fn active_elections(&self) -> usize {
        self.active_elections.len()
    }

    /// Get votes cast
    pub fn votes_cast(&self) -> Vec<Vote> {
        self.votes_cast.read().clone()
    }

    fn spawn_heartbeat_loop(&self) -> tokio::task::JoinHandle<()> {
        let interval = self.config.heartbeat_interval;
        let timeout = self.config.peer_timeout;
        let peers = self.peers.clone();
        let vote_tracker = self.vote_tracker.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Check peer liveness
                        for mut peer in peers.iter_mut() {
                            if !peer.is_alive(timeout) {
                                if peer.reachable {
                                    peer.reachable = false;
                                    vote_tracker.mark_unreachable(peer.node_id);
                                    debug!(node_id = peer.node_id, "Peer became unreachable");
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        debug!("Heartbeat loop shutting down");
                        break;
                    }
                }
            }
        })
    }
}

/// Statistics about a witness node
#[derive(Debug, Clone)]
pub struct WitnessStats {
    /// Node ID
    pub node_id: NodeId,
    /// Current state
    pub state: WitnessState,
    /// Number of peers
    pub peer_count: usize,
    /// Number of reachable peers
    pub reachable_peers: usize,
    /// Active elections
    pub active_elections: usize,
    /// Total votes cast
    pub votes_cast: usize,
    /// Heartbeats sent
    pub heartbeats_sent: u64,
}

impl WitnessNode {
    /// Get witness statistics
    pub fn stats(&self) -> WitnessStats {
        WitnessStats {
            node_id: self.node_id,
            state: *self.state.read(),
            peer_count: self.peers.len(),
            reachable_peers: self.reachable_peers(),
            active_elections: self.active_elections.len(),
            votes_cast: self.votes_cast.read().len(),
            heartbeats_sent: self.sequence.load(Ordering::SeqCst),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_creation() {
        let witness = WitnessNode::new(1, WitnessConfig::default());
        assert_eq!(witness.node_id(), 1);
        assert_eq!(witness.state(), WitnessState::Starting);
    }

    #[test]
    fn test_peer_registration() {
        let witness = WitnessNode::new(1, WitnessConfig::default());

        witness.register_peer(2, true, 1);
        witness.register_peer(3, true, 1);
        witness.register_peer(4, false, 2); // Another witness

        assert_eq!(witness.reachable_peers(), 3);
    }

    #[test]
    fn test_heartbeat_generation() {
        let witness = WitnessNode::new(1, WitnessConfig::default());

        let hb1 = witness.generate_heartbeat();
        let hb2 = witness.generate_heartbeat();

        assert_eq!(hb1.witness_id, 1);
        assert!(hb2.sequence > hb1.sequence);
    }

    #[test]
    fn test_election_participation() {
        let witness = WitnessNode::new(1, WitnessConfig::default());

        witness.join_election(1, 1);
        assert_eq!(witness.active_elections(), 1);
        assert_eq!(witness.state(), WitnessState::Voting);

        let vote = witness.cast_vote(1, 2, 1);
        assert!(vote.is_some());
        assert_eq!(vote.unwrap().candidate, 2);

        // Can't vote twice
        let vote2 = witness.cast_vote(1, 3, 1);
        assert_eq!(vote2.unwrap().candidate, 2); // Returns existing vote

        witness.leave_election(1);
        assert_eq!(witness.active_elections(), 0);
    }

    #[test]
    fn test_min_peers() {
        let config = WitnessConfig {
            min_peers: 2,
            ..Default::default()
        };
        let witness = WitnessNode::new(1, config);

        assert!(!witness.has_min_peers());

        witness.register_peer(2, true, 1);
        assert!(!witness.has_min_peers());

        witness.register_peer(3, true, 1);
        assert!(witness.has_min_peers());
    }
}
