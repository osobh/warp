//! Vote tracking and quorum management

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::replication::DomainId;

/// Unique identifier for a node
pub type NodeId = u64;

/// Unique identifier for an election/vote round
pub type ElectionId = u64;

/// A vote cast by a node
#[derive(Debug, Clone)]
pub struct Vote {
    /// Node casting the vote
    pub node_id: NodeId,

    /// Election this vote is for
    pub election_id: ElectionId,

    /// Candidate being voted for (node_id)
    pub candidate: NodeId,

    /// When the vote was cast
    pub timestamp: SystemTime,

    /// Vote term (monotonically increasing)
    pub term: u64,

    /// Signature for vote verification (optional)
    pub signature: Option<[u8; 64]>,
}

impl Vote {
    /// Create a new vote
    pub fn new(node_id: NodeId, election_id: ElectionId, candidate: NodeId, term: u64) -> Self {
        Self {
            node_id,
            election_id,
            candidate,
            timestamp: SystemTime::now(),
            term,
            signature: None,
        }
    }

    /// Create a self-vote (nominating oneself)
    pub fn self_vote(node_id: NodeId, election_id: ElectionId, term: u64) -> Self {
        Self::new(node_id, election_id, node_id, term)
    }
}

/// Result of a vote/election
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteResult {
    /// Election won with quorum
    Won,
    /// Election lost
    Lost,
    /// Still collecting votes
    Pending,
    /// Election timed out
    Timeout,
    /// Split vote (no clear winner)
    Split,
}

/// Quorum status
#[derive(Debug, Clone)]
pub struct QuorumStatus {
    /// Total nodes in cluster
    pub total_nodes: usize,

    /// Nodes currently reachable
    pub reachable_nodes: usize,

    /// Minimum nodes needed for quorum
    pub quorum_size: usize,

    /// Whether quorum is achieved
    pub has_quorum: bool,

    /// Nodes that have voted in current election
    pub voted_nodes: HashSet<NodeId>,

    /// Current leader (if elected)
    pub leader: Option<NodeId>,

    /// Current term
    pub term: u64,
}

impl QuorumStatus {
    /// Check if we can proceed with writes
    pub fn can_write(&self) -> bool {
        self.has_quorum && self.leader.is_some()
    }

    /// Calculate votes needed to win
    pub fn votes_needed(&self) -> usize {
        if self.voted_nodes.len() >= self.quorum_size {
            0
        } else {
            self.quorum_size - self.voted_nodes.len()
        }
    }
}

/// Election state
#[derive(Debug, Clone)]
struct Election {
    /// Election ID
    id: ElectionId,
    /// Current term
    term: u64,
    /// Votes received
    votes: HashMap<NodeId, Vote>,
    /// When election started
    started_at: Instant,
    /// Election timeout
    timeout: Duration,
    /// Result (if concluded)
    result: Option<VoteResult>,
}

impl Election {
    fn new(id: ElectionId, term: u64, timeout: Duration) -> Self {
        Self {
            id,
            term,
            votes: HashMap::new(),
            started_at: Instant::now(),
            timeout,
            result: None,
        }
    }

    fn is_expired(&self) -> bool {
        self.started_at.elapsed() > self.timeout
    }

    fn add_vote(&mut self, vote: Vote) -> bool {
        // Only accept votes for this election and term
        if vote.election_id != self.id || vote.term != self.term {
            return false;
        }
        // Only one vote per node
        if self.votes.contains_key(&vote.node_id) {
            return false;
        }
        self.votes.insert(vote.node_id, vote);
        true
    }

    fn count_votes_for(&self, candidate: NodeId) -> usize {
        self.votes
            .values()
            .filter(|v| v.candidate == candidate)
            .count()
    }

    fn get_winner(&self, quorum_size: usize) -> Option<NodeId> {
        let mut vote_counts: HashMap<NodeId, usize> = HashMap::new();
        for vote in self.votes.values() {
            *vote_counts.entry(vote.candidate).or_insert(0) += 1;
        }

        for (candidate, count) in vote_counts {
            if count >= quorum_size {
                return Some(candidate);
            }
        }
        None
    }
}

/// Tracks votes and manages elections
pub struct VoteTracker {
    /// This node's ID
    node_id: NodeId,

    /// Known nodes in cluster
    cluster_nodes: DashMap<NodeId, NodeInfo>,

    /// Current election (if any)
    current_election: RwLock<Option<Election>>,

    /// Current term
    current_term: AtomicU64,

    /// Current leader
    current_leader: RwLock<Option<NodeId>>,

    /// Election counter
    next_election_id: AtomicU64,

    /// Election timeout
    election_timeout: Duration,

    /// Quorum size (0 = auto-calculate)
    fixed_quorum: Option<usize>,
}

/// Information about a cluster node
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Node ID
    pub id: NodeId,
    /// Last heartbeat received
    pub last_seen: Instant,
    /// Node's domain
    pub domain_id: DomainId,
    /// Is this a witness node?
    pub is_witness: bool,
    /// Is node currently reachable?
    pub reachable: bool,
}

impl VoteTracker {
    /// Create a new vote tracker
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            cluster_nodes: DashMap::new(),
            current_election: RwLock::new(None),
            current_term: AtomicU64::new(0),
            current_leader: RwLock::new(None),
            next_election_id: AtomicU64::new(1),
            election_timeout: Duration::from_secs(5),
            fixed_quorum: None,
        }
    }

    /// Create with fixed quorum size
    pub fn with_quorum(node_id: NodeId, quorum_size: usize) -> Self {
        let mut tracker = Self::new(node_id);
        tracker.fixed_quorum = Some(quorum_size);
        tracker
    }

    /// Register a node in the cluster
    pub fn register_node(&self, id: NodeId, domain_id: DomainId, is_witness: bool) {
        self.cluster_nodes.insert(
            id,
            NodeInfo {
                id,
                last_seen: Instant::now(),
                domain_id,
                is_witness,
                reachable: true,
            },
        );
    }

    /// Remove a node from the cluster
    pub fn remove_node(&self, id: NodeId) {
        self.cluster_nodes.remove(&id);
    }

    /// Update node heartbeat
    pub fn heartbeat(&self, node_id: NodeId) {
        if let Some(mut node) = self.cluster_nodes.get_mut(&node_id) {
            node.last_seen = Instant::now();
            node.reachable = true;
        }
    }

    /// Mark node as unreachable
    pub fn mark_unreachable(&self, node_id: NodeId) {
        if let Some(mut node) = self.cluster_nodes.get_mut(&node_id) {
            node.reachable = false;
        }
    }

    /// Calculate quorum size
    pub fn quorum_size(&self) -> usize {
        self.fixed_quorum.unwrap_or_else(|| {
            let total = self.cluster_nodes.len();
            (total / 2) + 1
        })
    }

    /// Get current quorum status
    pub fn quorum_status(&self) -> QuorumStatus {
        let total_nodes = self.cluster_nodes.len();
        let reachable_nodes = self.cluster_nodes.iter().filter(|n| n.reachable).count();
        let quorum_size = self.quorum_size();

        let voted_nodes = self
            .current_election
            .read()
            .as_ref()
            .map(|e| e.votes.keys().cloned().collect())
            .unwrap_or_default();

        QuorumStatus {
            total_nodes,
            reachable_nodes,
            quorum_size,
            has_quorum: reachable_nodes >= quorum_size,
            voted_nodes,
            leader: *self.current_leader.read(),
            term: self.current_term.load(Ordering::SeqCst),
        }
    }

    /// Start a new election
    pub fn start_election(&self) -> ElectionId {
        let election_id = self.next_election_id.fetch_add(1, Ordering::SeqCst);
        let term = self.current_term.fetch_add(1, Ordering::SeqCst) + 1;

        let election = Election::new(election_id, term, self.election_timeout);
        *self.current_election.write() = Some(election);

        // Clear current leader during election
        *self.current_leader.write() = None;

        election_id
    }

    /// Cast a vote
    pub fn cast_vote(&self, vote: Vote) -> bool {
        let mut election = self.current_election.write();
        if let Some(ref mut e) = *election {
            e.add_vote(vote)
        } else {
            false
        }
    }

    /// Vote for self (start candidacy)
    pub fn vote_for_self(&self) -> Option<Vote> {
        let election = self.current_election.read();
        if let Some(ref e) = *election {
            let vote = Vote::self_vote(self.node_id, e.id, e.term);
            drop(election);
            self.cast_vote(vote.clone());
            Some(vote)
        } else {
            None
        }
    }

    /// Check election result
    pub fn check_election(&self) -> VoteResult {
        let mut election = self.current_election.write();

        if let Some(ref mut e) = *election {
            // Check if already concluded
            if let Some(result) = e.result {
                return result;
            }

            // Check timeout
            if e.is_expired() {
                e.result = Some(VoteResult::Timeout);
                return VoteResult::Timeout;
            }

            let quorum = self.quorum_size();

            // Check for winner
            if let Some(winner) = e.get_winner(quorum) {
                e.result = Some(VoteResult::Won);
                drop(election);
                *self.current_leader.write() = Some(winner);
                return VoteResult::Won;
            }

            // Check if we have all votes but no winner
            let total_votes = e.votes.len();
            let total_nodes = self.cluster_nodes.len();
            if total_votes >= total_nodes {
                e.result = Some(VoteResult::Split);
                return VoteResult::Split;
            }

            VoteResult::Pending
        } else {
            VoteResult::Pending
        }
    }

    /// Get current leader
    pub fn leader(&self) -> Option<NodeId> {
        *self.current_leader.read()
    }

    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        self.leader() == Some(self.node_id)
    }

    /// Get current term
    pub fn term(&self) -> u64 {
        self.current_term.load(Ordering::SeqCst)
    }

    /// Accept a higher term (step down if leader)
    pub fn accept_term(&self, term: u64) {
        let current = self.current_term.load(Ordering::SeqCst);
        if term > current {
            self.current_term.store(term, Ordering::SeqCst);
            // Clear leader - new election needed
            *self.current_leader.write() = None;
        }
    }

    /// Get all registered nodes
    pub fn nodes(&self) -> Vec<NodeInfo> {
        self.cluster_nodes
            .iter()
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get reachable node count
    pub fn reachable_count(&self) -> usize {
        self.cluster_nodes.iter().filter(|n| n.reachable).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_tracker_basic() {
        let tracker = VoteTracker::new(1);

        tracker.register_node(1, 1, false);
        tracker.register_node(2, 1, false);
        tracker.register_node(3, 1, false);

        assert_eq!(tracker.quorum_size(), 2); // 3 nodes, quorum = 2
    }

    #[test]
    fn test_election() {
        let tracker = VoteTracker::new(1);

        tracker.register_node(1, 1, false);
        tracker.register_node(2, 1, false);
        tracker.register_node(3, 1, false);

        let election_id = tracker.start_election();
        let term = tracker.term();

        // Node 1 votes for itself
        tracker.cast_vote(Vote::new(1, election_id, 1, term));
        assert_eq!(tracker.check_election(), VoteResult::Pending);

        // Node 2 votes for node 1
        tracker.cast_vote(Vote::new(2, election_id, 1, term));
        assert_eq!(tracker.check_election(), VoteResult::Won);

        assert_eq!(tracker.leader(), Some(1));
    }

    #[test]
    fn test_quorum_status() {
        let tracker = VoteTracker::new(1);

        tracker.register_node(1, 1, false);
        tracker.register_node(2, 1, false);
        tracker.register_node(3, 1, true); // witness

        let status = tracker.quorum_status();
        assert_eq!(status.total_nodes, 3);
        assert!(status.has_quorum);
    }

    #[test]
    fn test_heartbeat() {
        let tracker = VoteTracker::new(1);
        tracker.register_node(2, 1, false);

        tracker.mark_unreachable(2);
        assert_eq!(tracker.reachable_count(), 0);

        tracker.heartbeat(2);
        assert_eq!(tracker.reachable_count(), 1);
    }
}
