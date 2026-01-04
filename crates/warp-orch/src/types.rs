//! Transfer orchestration types for warp-orch.
//!
//! This module defines all core types used in transfer orchestration,
//! including transfer identifiers, states, requests, and results.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use warp_edge::EdgeId;
use warp_sched::{Assignment, ChunkId, EdgeIdx};

/// Unique identifier for a transfer operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId(pub u64);

impl TransferId {
    /// Create a new TransferId.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the inner u64 value.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Direction of data transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    /// Fetching data from edge nodes.
    Download,
    /// Pushing data to edge nodes.
    Upload,
}

/// Status of a transfer operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStatus {
    /// Transfer is queued but not yet started.
    Pending,
    /// Transfer is currently in progress.
    Active,
    /// Transfer is temporarily paused.
    Paused,
    /// Transfer completed successfully.
    Completed,
    /// Transfer failed with a reason.
    Failed { reason: String },
    /// Transfer was cancelled by user.
    Cancelled,
}

impl TransferStatus {
    /// Check if the transfer is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TransferStatus::Completed | TransferStatus::Failed { .. } | TransferStatus::Cancelled
        )
    }

    /// Check if the transfer is active.
    pub fn is_active(&self) -> bool {
        matches!(self, TransferStatus::Active)
    }
}

/// State of a single chunk transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkTransfer {
    /// Hash of the chunk being transferred.
    pub chunk_hash: [u8; 32],
    /// Size of the chunk in bytes.
    pub chunk_size: u32,
    /// Available source edges for this chunk.
    pub source_edges: Vec<EdgeIdx>,
    /// Currently active edge for transfer.
    pub active_edge: Option<EdgeIdx>,
    /// Number of bytes transferred so far.
    pub bytes_transferred: u64,
    /// Current status of this chunk transfer.
    pub status: TransferStatus,
    /// Number of retry attempts.
    pub retry_count: u8,
    /// Timestamp when transfer started (milliseconds since epoch).
    pub started_at: Option<u64>,
}

impl ChunkTransfer {
    /// Create a new ChunkTransfer in pending state.
    pub fn new(chunk_hash: [u8; 32], chunk_size: u32, source_edges: Vec<EdgeIdx>) -> Self {
        Self {
            chunk_hash,
            chunk_size,
            source_edges,
            active_edge: None,
            bytes_transferred: 0,
            status: TransferStatus::Pending,
            retry_count: 0,
            started_at: None,
        }
    }

    /// Check if this chunk transfer is complete.
    pub fn is_complete(&self) -> bool {
        self.bytes_transferred >= self.chunk_size as u64
    }

    /// Get progress as a percentage (0-100).
    pub fn progress_percent(&self) -> f64 {
        if self.chunk_size == 0 {
            return 100.0;
        }
        (self.bytes_transferred as f64 / self.chunk_size as f64 * 100.0).min(100.0)
    }

    /// Start transfer from a specific edge.
    pub fn start_transfer(&mut self, edge_idx: EdgeIdx, timestamp_ms: u64) {
        self.active_edge = Some(edge_idx);
        self.status = TransferStatus::Active;
        if self.started_at.is_none() {
            self.started_at = Some(timestamp_ms);
        }
    }

    /// Record transferred bytes.
    pub fn record_progress(&mut self, bytes: u64) {
        self.bytes_transferred += bytes;
        if self.is_complete() {
            self.status = TransferStatus::Completed;
            self.active_edge = None;
        }
    }

    /// Mark transfer as failed and increment retry count.
    pub fn mark_failed(&mut self, reason: String) {
        self.status = TransferStatus::Failed { reason };
        self.active_edge = None;
        self.retry_count += 1;
    }
}

/// Per-edge transfer statistics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeTransfer {
    /// Edge index.
    pub edge_idx: EdgeIdx,
    /// Number of chunks assigned to this edge.
    pub chunks_assigned: usize,
    /// Number of chunks completed by this edge.
    pub chunks_completed: usize,
    /// Total bytes transferred through this edge.
    pub bytes_transferred: u64,
    /// Whether this edge is currently active.
    pub is_active: bool,
    /// Last activity timestamp (milliseconds since epoch).
    pub last_activity_ms: u64,
}

impl EdgeTransfer {
    /// Create a new EdgeTransfer.
    pub fn new(edge_idx: EdgeIdx, timestamp_ms: u64) -> Self {
        Self {
            edge_idx,
            chunks_assigned: 0,
            chunks_completed: 0,
            bytes_transferred: 0,
            is_active: false,
            last_activity_ms: timestamp_ms,
        }
    }

    /// Assign a chunk to this edge.
    pub fn assign_chunk(&mut self) {
        self.chunks_assigned += 1;
        self.is_active = true;
    }

    /// Record chunk completion.
    pub fn complete_chunk(&mut self, bytes: u64, timestamp_ms: u64) {
        self.chunks_completed += 1;
        self.bytes_transferred += bytes;
        self.last_activity_ms = timestamp_ms;
    }

    /// Record progress on current chunk.
    pub fn record_progress(&mut self, bytes: u64, timestamp_ms: u64) {
        self.bytes_transferred += bytes;
        self.last_activity_ms = timestamp_ms;
    }

    /// Mark edge as inactive.
    pub fn deactivate(&mut self, timestamp_ms: u64) {
        self.is_active = false;
        self.last_activity_ms = timestamp_ms;
    }

    /// Calculate completion rate (0.0 to 1.0).
    pub fn completion_rate(&self) -> f64 {
        if self.chunks_assigned == 0 {
            return 0.0;
        }
        self.chunks_completed as f64 / self.chunks_assigned as f64
    }
}

/// Overall state of a transfer operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferState {
    /// Unique transfer identifier.
    pub id: TransferId,
    /// Direction of transfer.
    pub direction: TransferDirection,
    /// Overall transfer status.
    pub status: TransferStatus,
    /// Total number of chunks in transfer.
    pub total_chunks: usize,
    /// Number of completed chunks.
    pub completed_chunks: usize,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Bytes transferred so far.
    pub transferred_bytes: u64,
    /// State of each chunk transfer.
    pub chunk_states: Vec<ChunkTransfer>,
    /// Per-edge transfer statistics.
    pub edge_states: HashMap<EdgeIdx, EdgeTransfer>,
    /// Creation timestamp (milliseconds since epoch).
    pub created_at_ms: u64,
    /// Start timestamp (milliseconds since epoch).
    pub started_at_ms: Option<u64>,
    /// Completion timestamp (milliseconds since epoch).
    pub completed_at_ms: Option<u64>,
}

impl TransferState {
    /// Create a new TransferState.
    pub fn new(
        id: TransferId,
        direction: TransferDirection,
        chunk_states: Vec<ChunkTransfer>,
        created_at_ms: u64,
    ) -> Self {
        let total_chunks = chunk_states.len();
        let total_bytes = chunk_states.iter().map(|c| c.chunk_size as u64).sum();

        Self {
            id,
            direction,
            status: TransferStatus::Pending,
            total_chunks,
            completed_chunks: 0,
            total_bytes,
            transferred_bytes: 0,
            chunk_states,
            edge_states: HashMap::new(),
            created_at_ms,
            started_at_ms: None,
            completed_at_ms: None,
        }
    }

    /// Start the transfer.
    pub fn start(&mut self, timestamp_ms: u64) {
        self.status = TransferStatus::Active;
        self.started_at_ms = Some(timestamp_ms);
    }

    /// Update overall progress from chunk states.
    pub fn update_progress(&mut self) {
        self.completed_chunks = self
            .chunk_states
            .iter()
            .filter(|c| c.status == TransferStatus::Completed)
            .count();

        self.transferred_bytes = self.chunk_states.iter().map(|c| c.bytes_transferred).sum();

        if self.completed_chunks == self.total_chunks && self.total_chunks > 0 {
            self.status = TransferStatus::Completed;
        }
    }

    /// Get progress as a percentage (0-100).
    pub fn progress_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        (self.transferred_bytes as f64 / self.total_bytes as f64 * 100.0).min(100.0)
    }

    /// Complete the transfer.
    pub fn complete(&mut self, timestamp_ms: u64) {
        self.status = TransferStatus::Completed;
        self.completed_at_ms = Some(timestamp_ms);
    }

    /// Fail the transfer with a reason.
    pub fn fail(&mut self, reason: String, timestamp_ms: u64) {
        self.status = TransferStatus::Failed { reason };
        self.completed_at_ms = Some(timestamp_ms);
    }

    /// Cancel the transfer.
    pub fn cancel(&mut self, timestamp_ms: u64) {
        self.status = TransferStatus::Cancelled;
        self.completed_at_ms = Some(timestamp_ms);
    }

    /// Calculate duration in milliseconds.
    pub fn duration_ms(&self) -> Option<u64> {
        let start = self.started_at_ms?;
        let end = self.completed_at_ms.unwrap_or(self.created_at_ms);
        Some(end.saturating_sub(start))
    }

    /// Get or create edge state.
    pub fn get_or_create_edge_state(
        &mut self,
        edge_idx: EdgeIdx,
        timestamp_ms: u64,
    ) -> &mut EdgeTransfer {
        self.edge_states
            .entry(edge_idx)
            .or_insert_with(|| EdgeTransfer::new(edge_idx, timestamp_ms))
    }
}

/// Request to initiate a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRequest {
    /// Hashes of chunks to transfer.
    pub chunks: Vec<[u8; 32]>,
    /// Sizes of each chunk in bytes.
    pub chunk_sizes: Vec<u32>,
    /// Direction of transfer.
    pub direction: TransferDirection,
    /// Transfer priority (higher = more important).
    pub priority: u8,
    /// Maximum concurrent edges to use.
    pub max_concurrent_edges: usize,
    /// Maximum retry attempts per chunk.
    pub max_retries: u8,
}

impl TransferRequest {
    /// Create a new TransferRequest with defaults.
    pub fn new(chunks: Vec<[u8; 32]>, chunk_sizes: Vec<u32>, direction: TransferDirection) -> Self {
        Self {
            chunks,
            chunk_sizes,
            direction,
            priority: 0,
            max_concurrent_edges: 5,
            max_retries: 3,
        }
    }

    /// Validate the request.
    pub fn validate(&self) -> Result<(), String> {
        if self.chunks.is_empty() {
            return Err("No chunks specified".to_string());
        }

        if self.chunks.len() != self.chunk_sizes.len() {
            return Err("Chunks and sizes count mismatch".to_string());
        }

        if self.max_concurrent_edges == 0 {
            return Err("max_concurrent_edges must be at least 1".to_string());
        }

        Ok(())
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Set max concurrent edges.
    pub fn with_max_concurrent_edges(mut self, max: usize) -> Self {
        self.max_concurrent_edges = max;
        self
    }

    /// Set max retries.
    pub fn with_max_retries(mut self, max: u8) -> Self {
        self.max_retries = max;
        self
    }

    /// Calculate total bytes.
    pub fn total_bytes(&self) -> u64 {
        self.chunk_sizes.iter().map(|&s| s as u64).sum()
    }
}

/// Result of a completed transfer operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferResult {
    /// Transfer identifier.
    pub id: TransferId,
    /// Whether transfer was successful.
    pub success: bool,
    /// Number of chunks successfully transferred.
    pub chunks_transferred: usize,
    /// Total bytes transferred.
    pub bytes_transferred: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Hashes of failed chunks.
    pub failed_chunks: Vec<[u8; 32]>,
    /// Average transfer speed in bits per second.
    pub average_speed_bps: u64,
}

impl TransferResult {
    /// Create a new TransferResult from TransferState.
    pub fn from_state(state: &TransferState, timestamp_ms: u64) -> Self {
        let duration_ms = state.duration_ms().unwrap_or(1);
        let average_speed_bps = if duration_ms > 0 {
            (state.transferred_bytes * 8 * 1000) / duration_ms
        } else {
            0
        };

        let failed_chunks = state
            .chunk_states
            .iter()
            .filter(|c| matches!(c.status, TransferStatus::Failed { .. }))
            .map(|c| c.chunk_hash)
            .collect();

        Self {
            id: state.id,
            success: state.status == TransferStatus::Completed,
            chunks_transferred: state.completed_chunks,
            bytes_transferred: state.transferred_bytes,
            duration_ms,
            failed_chunks,
            average_speed_bps,
        }
    }

    /// Calculate average speed in megabits per second.
    pub fn average_speed_mbps(&self) -> f64 {
        self.average_speed_bps as f64 / 1_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_id_creation() {
        let id = TransferId::new(42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn test_transfer_id_equality() {
        let id1 = TransferId::new(100);
        let id2 = TransferId::new(100);
        let id3 = TransferId::new(200);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_transfer_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TransferId::new(1));
        set.insert(TransferId::new(2));
        set.insert(TransferId::new(1));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_transfer_direction() {
        let download = TransferDirection::Download;
        let upload = TransferDirection::Upload;
        assert_ne!(download, upload);
    }

    #[test]
    fn test_transfer_status_terminal() {
        assert!(!TransferStatus::Pending.is_terminal());
        assert!(!TransferStatus::Active.is_terminal());
        assert!(!TransferStatus::Paused.is_terminal());
        assert!(TransferStatus::Completed.is_terminal());
        assert!(
            TransferStatus::Failed {
                reason: "error".to_string()
            }
            .is_terminal()
        );
        assert!(TransferStatus::Cancelled.is_terminal());
    }

    #[test]
    fn test_transfer_status_active() {
        assert!(!TransferStatus::Pending.is_active());
        assert!(TransferStatus::Active.is_active());
        assert!(!TransferStatus::Completed.is_active());
    }

    #[test]
    fn test_chunk_transfer_new() {
        let hash = [0u8; 32];
        let edges = vec![EdgeIdx(0), EdgeIdx(1)];
        let chunk = ChunkTransfer::new(hash, 1024, edges.clone());

        assert_eq!(chunk.chunk_hash, hash);
        assert_eq!(chunk.chunk_size, 1024);
        assert_eq!(chunk.source_edges, edges);
        assert_eq!(chunk.active_edge, None);
        assert_eq!(chunk.bytes_transferred, 0);
        assert_eq!(chunk.status, TransferStatus::Pending);
        assert_eq!(chunk.retry_count, 0);
        assert_eq!(chunk.started_at, None);
    }

    #[test]
    fn test_chunk_transfer_progress() {
        let mut chunk = ChunkTransfer::new([0u8; 32], 1000, vec![EdgeIdx(0)]);
        assert_eq!(chunk.progress_percent(), 0.0);

        chunk.bytes_transferred = 500;
        assert_eq!(chunk.progress_percent(), 50.0);

        chunk.bytes_transferred = 1000;
        assert_eq!(chunk.progress_percent(), 100.0);

        chunk.bytes_transferred = 1500;
        assert_eq!(chunk.progress_percent(), 100.0);
    }

    #[test]
    fn test_chunk_transfer_start() {
        let mut chunk = ChunkTransfer::new([0u8; 32], 1024, vec![EdgeIdx(0)]);
        chunk.start_transfer(EdgeIdx(0), 1000);

        assert_eq!(chunk.active_edge, Some(EdgeIdx(0)));
        assert_eq!(chunk.status, TransferStatus::Active);
        assert_eq!(chunk.started_at, Some(1000));
    }

    #[test]
    fn test_chunk_transfer_record_progress() {
        let mut chunk = ChunkTransfer::new([0u8; 32], 1000, vec![EdgeIdx(0)]);
        chunk.start_transfer(EdgeIdx(0), 1000);

        chunk.record_progress(500);
        assert_eq!(chunk.bytes_transferred, 500);
        assert!(!chunk.is_complete());

        chunk.record_progress(500);
        assert_eq!(chunk.bytes_transferred, 1000);
        assert!(chunk.is_complete());
        assert_eq!(chunk.status, TransferStatus::Completed);
        assert_eq!(chunk.active_edge, None);
    }

    #[test]
    fn test_chunk_transfer_mark_failed() {
        let mut chunk = ChunkTransfer::new([0u8; 32], 1024, vec![EdgeIdx(0)]);
        chunk.start_transfer(EdgeIdx(0), 1000);

        chunk.mark_failed("Network error".to_string());
        assert_eq!(chunk.retry_count, 1);
        assert_eq!(chunk.active_edge, None);
        assert!(matches!(chunk.status, TransferStatus::Failed { .. }));
    }

    #[test]
    fn test_edge_transfer_lifecycle() {
        // Test creation
        let mut edge = EdgeTransfer::new(EdgeIdx(5), 1000);
        assert_eq!(edge.edge_idx, EdgeIdx(5));
        assert_eq!(edge.chunks_assigned, 0);
        assert_eq!(edge.chunks_completed, 0);
        assert_eq!(edge.bytes_transferred, 0);
        assert!(!edge.is_active);
        assert_eq!(edge.last_activity_ms, 1000);
        assert_eq!(edge.completion_rate(), 0.0);

        // Test assign and complete
        edge.assign_chunk();
        assert_eq!(edge.chunks_assigned, 1);
        assert!(edge.is_active);

        edge.complete_chunk(1024, 2000);
        assert_eq!(edge.chunks_completed, 1);
        assert_eq!(edge.bytes_transferred, 1024);
        assert_eq!(edge.last_activity_ms, 2000);

        // Test completion rate
        edge.chunks_assigned = 10;
        edge.chunks_completed = 5;
        assert_eq!(edge.completion_rate(), 0.5);

        // Test deactivate
        edge.deactivate(3000);
        assert!(!edge.is_active);
        assert_eq!(edge.last_activity_ms, 3000);
    }

    #[test]
    fn test_transfer_state_new() {
        let chunks = vec![
            ChunkTransfer::new([1u8; 32], 1000, vec![EdgeIdx(0)]),
            ChunkTransfer::new([2u8; 32], 2000, vec![EdgeIdx(1)]),
        ];

        let state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            chunks,
            1000,
        );

        assert_eq!(state.id, TransferId::new(1));
        assert_eq!(state.direction, TransferDirection::Download);
        assert_eq!(state.status, TransferStatus::Pending);
        assert_eq!(state.total_chunks, 2);
        assert_eq!(state.total_bytes, 3000);
        assert_eq!(state.created_at_ms, 1000);
    }

    #[test]
    fn test_transfer_state_start() {
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![],
            1000,
        );

        state.start(2000);
        assert_eq!(state.status, TransferStatus::Active);
        assert_eq!(state.started_at_ms, Some(2000));
    }

    #[test]
    fn test_transfer_state_update_progress() {
        let mut chunks = vec![
            ChunkTransfer::new([1u8; 32], 1000, vec![EdgeIdx(0)]),
            ChunkTransfer::new([2u8; 32], 2000, vec![EdgeIdx(1)]),
        ];
        chunks[0].bytes_transferred = 1000;
        chunks[0].status = TransferStatus::Completed;

        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            chunks,
            1000,
        );

        state.update_progress();
        assert_eq!(state.completed_chunks, 1);
        assert_eq!(state.transferred_bytes, 1000);
        assert_ne!(state.status, TransferStatus::Completed);

        state.chunk_states[1].bytes_transferred = 2000;
        state.chunk_states[1].status = TransferStatus::Completed;
        state.update_progress();
        assert_eq!(state.completed_chunks, 2);
        assert_eq!(state.transferred_bytes, 3000);
        assert_eq!(state.status, TransferStatus::Completed);
    }

    #[test]
    fn test_transfer_state_progress_percent() {
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![ChunkTransfer::new([1u8; 32], 1000, vec![EdgeIdx(0)])],
            1000,
        );

        assert_eq!(state.progress_percent(), 0.0);

        state.transferred_bytes = 500;
        assert_eq!(state.progress_percent(), 50.0);

        state.transferred_bytes = 1000;
        assert_eq!(state.progress_percent(), 100.0);
    }

    #[test]
    fn test_transfer_state_terminal_states() {
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![],
            1000,
        );

        // Test complete
        state.complete(3000);
        assert_eq!(state.status, TransferStatus::Completed);
        assert_eq!(state.completed_at_ms, Some(3000));

        // Test fail
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![],
            1000,
        );
        state.fail("Connection lost".to_string(), 3000);
        assert!(matches!(state.status, TransferStatus::Failed { .. }));
        assert_eq!(state.completed_at_ms, Some(3000));

        // Test cancel
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![],
            1000,
        );
        state.cancel(3000);
        assert_eq!(state.status, TransferStatus::Cancelled);
        assert_eq!(state.completed_at_ms, Some(3000));
    }

    #[test]
    fn test_transfer_state_duration_and_edges() {
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![],
            1000,
        );
        assert_eq!(state.duration_ms(), None);

        state.start(2000);
        state.complete(5000);
        assert_eq!(state.duration_ms(), Some(3000));

        // Test edge state management
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![],
            1000,
        );
        let edge_state = state.get_or_create_edge_state(EdgeIdx(0), 2000);
        assert_eq!(edge_state.edge_idx, EdgeIdx(0));
        assert_eq!(state.edge_states.len(), 1);

        let _edge_state = state.get_or_create_edge_state(EdgeIdx(0), 3000);
        assert_eq!(state.edge_states.len(), 1);
    }

    #[test]
    fn test_transfer_request_new() {
        let chunks = vec![[1u8; 32], [2u8; 32]];
        let sizes = vec![1000, 2000];
        let req = TransferRequest::new(chunks.clone(), sizes.clone(), TransferDirection::Download);

        assert_eq!(req.chunks, chunks);
        assert_eq!(req.chunk_sizes, sizes);
        assert_eq!(req.direction, TransferDirection::Download);
        assert_eq!(req.priority, 0);
        assert_eq!(req.max_concurrent_edges, 5);
        assert_eq!(req.max_retries, 3);
    }

    #[test]
    fn test_transfer_request_validate() {
        let valid_req =
            TransferRequest::new(vec![[1u8; 32]], vec![1000], TransferDirection::Download);
        assert!(valid_req.validate().is_ok());

        let empty_req = TransferRequest::new(vec![], vec![], TransferDirection::Download);
        assert!(empty_req.validate().is_err());

        let mismatched_req = TransferRequest::new(
            vec![[1u8; 32], [2u8; 32]],
            vec![1000],
            TransferDirection::Download,
        );
        assert!(mismatched_req.validate().is_err());

        let zero_edges_req =
            TransferRequest::new(vec![[1u8; 32]], vec![1000], TransferDirection::Download)
                .with_max_concurrent_edges(0);
        assert!(zero_edges_req.validate().is_err());
    }

    #[test]
    fn test_transfer_request_builders() {
        let req = TransferRequest::new(vec![[1u8; 32]], vec![1000], TransferDirection::Download)
            .with_priority(10)
            .with_max_concurrent_edges(3)
            .with_max_retries(5);

        assert_eq!(req.priority, 10);
        assert_eq!(req.max_concurrent_edges, 3);
        assert_eq!(req.max_retries, 5);
    }

    #[test]
    fn test_transfer_request_total_bytes() {
        let req = TransferRequest::new(
            vec![[1u8; 32], [2u8; 32]],
            vec![1000, 2000],
            TransferDirection::Download,
        );
        assert_eq!(req.total_bytes(), 3000);
    }

    #[test]
    fn test_transfer_result_from_state() {
        let mut state = TransferState::new(
            TransferId::new(1),
            TransferDirection::Download,
            vec![ChunkTransfer::new([1u8; 32], 1000, vec![EdgeIdx(0)])],
            1000,
        );

        state.start(2000);
        state.transferred_bytes = 1000;
        state.completed_chunks = 1;
        state.complete(3000);

        let result = TransferResult::from_state(&state, 3000);
        assert_eq!(result.id, TransferId::new(1));
        assert!(result.success);
        assert_eq!(result.chunks_transferred, 1);
        assert_eq!(result.bytes_transferred, 1000);
        assert_eq!(result.duration_ms, 1000);
        assert!(result.failed_chunks.is_empty());
        assert_eq!(result.average_speed_bps, 8000);
    }

    #[test]
    fn test_transfer_result_average_speed_mbps() {
        let result = TransferResult {
            id: TransferId::new(1),
            success: true,
            chunks_transferred: 1,
            bytes_transferred: 1000000,
            duration_ms: 1000,
            failed_chunks: vec![],
            average_speed_bps: 8_000_000,
        };

        assert_eq!(result.average_speed_mbps(), 8.0);
    }

    #[test]
    fn test_serialization_all_types() {
        // Test TransferId
        let id = TransferId::new(42);
        let json = serde_json::to_string(&id).unwrap();
        let _: TransferId = serde_json::from_str(&json).unwrap();

        // Test TransferDirection
        let dir = TransferDirection::Download;
        let json = serde_json::to_string(&dir).unwrap();
        let _: TransferDirection = serde_json::from_str(&json).unwrap();

        // Test TransferStatus
        let status = TransferStatus::Failed {
            reason: "test".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let _: TransferStatus = serde_json::from_str(&json).unwrap();

        // Test ChunkTransfer
        let chunk = ChunkTransfer::new([1u8; 32], 1000, vec![EdgeIdx(0)]);
        let json = serde_json::to_string(&chunk).unwrap();
        let _: ChunkTransfer = serde_json::from_str(&json).unwrap();

        // Test EdgeTransfer
        let edge = EdgeTransfer::new(EdgeIdx(5), 1000);
        let json = serde_json::to_string(&edge).unwrap();
        let _: EdgeTransfer = serde_json::from_str(&json).unwrap();

        // Test TransferState
        let state = TransferState::new(TransferId::new(1), TransferDirection::Upload, vec![], 1000);
        let json = serde_json::to_string(&state).unwrap();
        let _: TransferState = serde_json::from_str(&json).unwrap();

        // Test TransferRequest
        let req = TransferRequest::new(vec![[1u8; 32]], vec![1000], TransferDirection::Download);
        let json = serde_json::to_string(&req).unwrap();
        let _: TransferRequest = serde_json::from_str(&json).unwrap();

        // Test TransferResult
        let result = TransferResult {
            id: TransferId::new(1),
            success: true,
            chunks_transferred: 10,
            bytes_transferred: 10000,
            duration_ms: 5000,
            failed_chunks: vec![],
            average_speed_bps: 16000,
        };
        let json = serde_json::to_string(&result).unwrap();
        let _: TransferResult = serde_json::from_str(&json).unwrap();
    }
}
