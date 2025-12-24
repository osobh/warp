//! HPC Channels integration for warp transfers.
//!
//! This module bridges warp transfer events to the hpc-channels message bus,
//! enabling real-time monitoring and coordination of file transfers.
//!
//! # Channels Used
//!
//! - `hpc.storage.upload` - Upload started event
//! - `hpc.storage.download` - Download started event
//! - `hpc.storage.progress` - Transfer progress updates
//!
//! # Example
//!
//! ```rust,ignore
//! use warp_core::channels::StorageChannelBridge;
//!
//! let bridge = StorageChannelBridge::new();
//!
//! // Publish upload started
//! bridge.publish_upload_start(&session_id, source, dest, total_bytes);
//!
//! // Publish progress update
//! bridge.publish_progress(&session_id, bytes_transferred, total_bytes, bps);
//! ```

use std::sync::Arc;

use tokio::sync::broadcast;

/// Upload started event.
#[derive(Clone, Debug)]
pub struct UploadStartEvent {
    /// Session identifier.
    pub session_id: String,
    /// Source path.
    pub source: String,
    /// Destination path/URL.
    pub destination: String,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Number of chunks.
    pub total_chunks: u64,
    /// Whether destination is remote.
    pub is_remote: bool,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Download started event.
#[derive(Clone, Debug)]
pub struct DownloadStartEvent {
    /// Session identifier.
    pub session_id: String,
    /// Source path/URL.
    pub source: String,
    /// Destination path.
    pub destination: String,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Number of chunks.
    pub total_chunks: u64,
    /// Whether source is remote.
    pub is_remote: bool,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Transfer progress update.
#[derive(Clone, Debug)]
pub struct TransferProgressEvent {
    /// Session identifier.
    pub session_id: String,
    /// Bytes transferred so far.
    pub bytes_transferred: u64,
    /// Total bytes.
    pub total_bytes: u64,
    /// Chunks completed.
    pub chunks_completed: u64,
    /// Total chunks.
    pub total_chunks: u64,
    /// Transfer speed (bytes/second).
    pub bytes_per_second: f64,
    /// Current file being processed.
    pub current_file: Option<String>,
    /// Estimated time remaining (seconds).
    pub eta_seconds: Option<u64>,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Transfer status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    /// Analyzing payload.
    Analyzing,
    /// Negotiating with remote.
    Negotiating,
    /// Transferring data.
    Transferring,
    /// Verifying integrity.
    Verifying,
    /// Transfer completed.
    Completed,
    /// Transfer failed.
    Failed(String),
    /// Transfer cancelled.
    Cancelled,
    /// Transfer paused.
    Paused,
}

/// Transfer status change event.
#[derive(Clone, Debug)]
pub struct TransferStatusEvent {
    /// Session identifier.
    pub session_id: String,
    /// New status.
    pub status: TransferStatus,
    /// Merkle root (if available).
    pub merkle_root: Option<[u8; 32]>,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

// =============================================================================
// Portal/Peer Events
// =============================================================================

/// Portal join event.
#[derive(Clone, Debug)]
pub struct PortalJoinEvent {
    /// Portal identifier.
    pub portal_id: String,
    /// Local address.
    pub local_addr: String,
    /// Passphrase hash (for verification).
    pub passphrase_hash: [u8; 32],
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Portal leave event.
#[derive(Clone, Debug)]
pub struct PortalLeaveEvent {
    /// Portal identifier.
    pub portal_id: String,
    /// Reason for leaving.
    pub reason: String,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Peer discovered event.
#[derive(Clone, Debug)]
pub struct PeerDiscoveredEvent {
    /// Peer identifier.
    pub peer_id: String,
    /// Peer address.
    pub peer_addr: String,
    /// Portal identifier.
    pub portal_id: String,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

// =============================================================================
// Stream Events
// =============================================================================

/// Stream start event.
#[derive(Clone, Debug)]
pub struct StreamStartEvent {
    /// Stream identifier.
    pub stream_id: String,
    /// Session identifier.
    pub session_id: String,
    /// Protocol (tcp, quic, etc).
    pub protocol: String,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Stream complete event.
#[derive(Clone, Debug)]
pub struct StreamCompleteEvent {
    /// Stream identifier.
    pub stream_id: String,
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Stream error event.
#[derive(Clone, Debug)]
pub struct StreamErrorEvent {
    /// Stream identifier.
    pub stream_id: String,
    /// Error message.
    pub error: String,
    /// Recoverable.
    pub recoverable: bool,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

// =============================================================================
// Protocol Stats
// =============================================================================

/// Protocol statistics event.
#[derive(Clone, Debug)]
pub struct ProtocolStatsEvent {
    /// Protocol name.
    pub protocol: String,
    /// Active connections.
    pub active_connections: u32,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Average RTT in milliseconds.
    pub avg_rtt_ms: f64,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

// =============================================================================
// Verification
// =============================================================================

/// Verification result event.
#[derive(Clone, Debug)]
pub struct VerificationResultEvent {
    /// Session identifier.
    pub session_id: String,
    /// Whether verification passed.
    pub passed: bool,
    /// Merkle root.
    pub merkle_root: [u8; 32],
    /// Chunks verified.
    pub chunks_verified: u64,
    /// Timestamp (epoch ms).
    pub timestamp_ms: u64,
}

/// Bridge between warp transfers and hpc-channels.
pub struct StorageChannelBridge {
    /// Broadcast sender for upload start events.
    upload_tx: broadcast::Sender<UploadStartEvent>,
    /// Broadcast sender for download start events.
    download_tx: broadcast::Sender<DownloadStartEvent>,
    /// Broadcast sender for progress events.
    progress_tx: broadcast::Sender<TransferProgressEvent>,
    /// Broadcast sender for status events.
    status_tx: broadcast::Sender<TransferStatusEvent>,
    /// Broadcast sender for portal join events.
    portal_join_tx: broadcast::Sender<PortalJoinEvent>,
    /// Broadcast sender for portal leave events.
    portal_leave_tx: broadcast::Sender<PortalLeaveEvent>,
    /// Broadcast sender for peer discovered events.
    peer_discovered_tx: broadcast::Sender<PeerDiscoveredEvent>,
    /// Broadcast sender for stream start events.
    stream_start_tx: broadcast::Sender<StreamStartEvent>,
    /// Broadcast sender for stream complete events.
    stream_complete_tx: broadcast::Sender<StreamCompleteEvent>,
    /// Broadcast sender for stream error events.
    stream_error_tx: broadcast::Sender<StreamErrorEvent>,
    /// Broadcast sender for protocol stats events.
    protocol_stats_tx: broadcast::Sender<ProtocolStatsEvent>,
    /// Broadcast sender for verification result events.
    verification_tx: broadcast::Sender<VerificationResultEvent>,
}

impl StorageChannelBridge {
    /// Create a new storage channel bridge.
    ///
    /// Registers channels with the hpc-channels global registry.
    pub fn new() -> Self {
        let upload_tx = hpc_channels::broadcast::<UploadStartEvent>(
            hpc_channels::channels::STORAGE_UPLOAD,
            256,
        );
        let download_tx = hpc_channels::broadcast::<DownloadStartEvent>(
            hpc_channels::channels::STORAGE_DOWNLOAD,
            256,
        );
        let progress_tx = hpc_channels::broadcast::<TransferProgressEvent>(
            hpc_channels::channels::STORAGE_PROGRESS,
            1024,
        );
        // Status events channel
        let status_tx = hpc_channels::broadcast::<TransferStatusEvent>(
            hpc_channels::channels::STORAGE_STATUS,
            256,
        );
        // Portal/peer channels
        let portal_join_tx = hpc_channels::broadcast::<PortalJoinEvent>(
            hpc_channels::channels::WARP_PORTAL_JOIN,
            128,
        );
        let portal_leave_tx = hpc_channels::broadcast::<PortalLeaveEvent>(
            hpc_channels::channels::WARP_PORTAL_LEAVE,
            128,
        );
        let peer_discovered_tx = hpc_channels::broadcast::<PeerDiscoveredEvent>(
            hpc_channels::channels::WARP_PEER_DISCOVERED,
            256,
        );
        // Stream channels
        let stream_start_tx = hpc_channels::broadcast::<StreamStartEvent>(
            hpc_channels::channels::WARP_STREAM_START,
            256,
        );
        let stream_complete_tx = hpc_channels::broadcast::<StreamCompleteEvent>(
            hpc_channels::channels::WARP_STREAM_COMPLETE,
            256,
        );
        let stream_error_tx = hpc_channels::broadcast::<StreamErrorEvent>(
            hpc_channels::channels::WARP_STREAM_ERROR,
            128,
        );
        // Protocol stats channel
        let protocol_stats_tx = hpc_channels::broadcast::<ProtocolStatsEvent>(
            hpc_channels::channels::WARP_PROTOCOL_STATS,
            256,
        );
        // Verification channel
        let verification_tx = hpc_channels::broadcast::<VerificationResultEvent>(
            hpc_channels::channels::WARP_VERIFICATION_RESULT,
            256,
        );

        Self {
            upload_tx,
            download_tx,
            progress_tx,
            status_tx,
            portal_join_tx,
            portal_leave_tx,
            peer_discovered_tx,
            stream_start_tx,
            stream_complete_tx,
            stream_error_tx,
            protocol_stats_tx,
            verification_tx,
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Publish upload started event.
    pub fn publish_upload_start(
        &self,
        session_id: &str,
        source: &str,
        destination: &str,
        total_bytes: u64,
        total_chunks: u64,
        is_remote: bool,
    ) {
        let _ = self.upload_tx.send(UploadStartEvent {
            session_id: session_id.to_string(),
            source: source.to_string(),
            destination: destination.to_string(),
            total_bytes,
            total_chunks,
            is_remote,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish download started event.
    pub fn publish_download_start(
        &self,
        session_id: &str,
        source: &str,
        destination: &str,
        total_bytes: u64,
        total_chunks: u64,
        is_remote: bool,
    ) {
        let _ = self.download_tx.send(DownloadStartEvent {
            session_id: session_id.to_string(),
            source: source.to_string(),
            destination: destination.to_string(),
            total_bytes,
            total_chunks,
            is_remote,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish transfer progress.
    pub fn publish_progress(
        &self,
        session_id: &str,
        bytes_transferred: u64,
        total_bytes: u64,
        chunks_completed: u64,
        total_chunks: u64,
        bytes_per_second: f64,
        current_file: Option<&str>,
        eta_seconds: Option<u64>,
    ) {
        let _ = self.progress_tx.send(TransferProgressEvent {
            session_id: session_id.to_string(),
            bytes_transferred,
            total_bytes,
            chunks_completed,
            total_chunks,
            bytes_per_second,
            current_file: current_file.map(String::from),
            eta_seconds,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish transfer status change.
    pub fn publish_status(
        &self,
        session_id: &str,
        status: TransferStatus,
        merkle_root: Option<[u8; 32]>,
    ) {
        let _ = self.status_tx.send(TransferStatusEvent {
            session_id: session_id.to_string(),
            status,
            merkle_root,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Subscribe to upload start events.
    pub fn subscribe_uploads(&self) -> broadcast::Receiver<UploadStartEvent> {
        self.upload_tx.subscribe()
    }

    /// Subscribe to download start events.
    pub fn subscribe_downloads(&self) -> broadcast::Receiver<DownloadStartEvent> {
        self.download_tx.subscribe()
    }

    /// Subscribe to progress events.
    pub fn subscribe_progress(&self) -> broadcast::Receiver<TransferProgressEvent> {
        self.progress_tx.subscribe()
    }

    /// Subscribe to status events.
    pub fn subscribe_status(&self) -> broadcast::Receiver<TransferStatusEvent> {
        self.status_tx.subscribe()
    }

    // =========================================================================
    // Portal/Peer methods
    // =========================================================================

    /// Publish portal join event.
    pub fn publish_portal_join(&self, portal_id: &str, local_addr: &str, passphrase_hash: [u8; 32]) {
        let _ = self.portal_join_tx.send(PortalJoinEvent {
            portal_id: portal_id.to_string(),
            local_addr: local_addr.to_string(),
            passphrase_hash,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish portal leave event.
    pub fn publish_portal_leave(&self, portal_id: &str, reason: &str) {
        let _ = self.portal_leave_tx.send(PortalLeaveEvent {
            portal_id: portal_id.to_string(),
            reason: reason.to_string(),
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish peer discovered event.
    pub fn publish_peer_discovered(&self, peer_id: &str, peer_addr: &str, portal_id: &str) {
        let _ = self.peer_discovered_tx.send(PeerDiscoveredEvent {
            peer_id: peer_id.to_string(),
            peer_addr: peer_addr.to_string(),
            portal_id: portal_id.to_string(),
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Subscribe to portal join events.
    pub fn subscribe_portal_join(&self) -> broadcast::Receiver<PortalJoinEvent> {
        self.portal_join_tx.subscribe()
    }

    /// Subscribe to portal leave events.
    pub fn subscribe_portal_leave(&self) -> broadcast::Receiver<PortalLeaveEvent> {
        self.portal_leave_tx.subscribe()
    }

    /// Subscribe to peer discovered events.
    pub fn subscribe_peer_discovered(&self) -> broadcast::Receiver<PeerDiscoveredEvent> {
        self.peer_discovered_tx.subscribe()
    }

    // =========================================================================
    // Stream methods
    // =========================================================================

    /// Publish stream start event.
    pub fn publish_stream_start(&self, stream_id: &str, session_id: &str, protocol: &str) {
        let _ = self.stream_start_tx.send(StreamStartEvent {
            stream_id: stream_id.to_string(),
            session_id: session_id.to_string(),
            protocol: protocol.to_string(),
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish stream complete event.
    pub fn publish_stream_complete(&self, stream_id: &str, bytes_transferred: u64, duration_ms: u64) {
        let _ = self.stream_complete_tx.send(StreamCompleteEvent {
            stream_id: stream_id.to_string(),
            bytes_transferred,
            duration_ms,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Publish stream error event.
    pub fn publish_stream_error(&self, stream_id: &str, error: &str, recoverable: bool) {
        let _ = self.stream_error_tx.send(StreamErrorEvent {
            stream_id: stream_id.to_string(),
            error: error.to_string(),
            recoverable,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Subscribe to stream start events.
    pub fn subscribe_stream_start(&self) -> broadcast::Receiver<StreamStartEvent> {
        self.stream_start_tx.subscribe()
    }

    /// Subscribe to stream complete events.
    pub fn subscribe_stream_complete(&self) -> broadcast::Receiver<StreamCompleteEvent> {
        self.stream_complete_tx.subscribe()
    }

    /// Subscribe to stream error events.
    pub fn subscribe_stream_error(&self) -> broadcast::Receiver<StreamErrorEvent> {
        self.stream_error_tx.subscribe()
    }

    // =========================================================================
    // Protocol stats methods
    // =========================================================================

    /// Publish protocol stats event.
    pub fn publish_protocol_stats(&self, event: ProtocolStatsEvent) {
        let _ = self.protocol_stats_tx.send(event);
    }

    /// Subscribe to protocol stats events.
    pub fn subscribe_protocol_stats(&self) -> broadcast::Receiver<ProtocolStatsEvent> {
        self.protocol_stats_tx.subscribe()
    }

    // =========================================================================
    // Verification methods
    // =========================================================================

    /// Publish verification result event.
    pub fn publish_verification_result(
        &self,
        session_id: &str,
        passed: bool,
        merkle_root: [u8; 32],
        chunks_verified: u64,
    ) {
        let _ = self.verification_tx.send(VerificationResultEvent {
            session_id: session_id.to_string(),
            passed,
            merkle_root,
            chunks_verified,
            timestamp_ms: Self::now_ms(),
        });
    }

    /// Subscribe to verification result events.
    pub fn subscribe_verification(&self) -> broadcast::Receiver<VerificationResultEvent> {
        self.verification_tx.subscribe()
    }
}

impl Default for StorageChannelBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared channel bridge type.
pub type SharedStorageChannelBridge = Arc<StorageChannelBridge>;

/// Create a new shared channel bridge.
#[must_use]
pub fn shared_channel_bridge() -> SharedStorageChannelBridge {
    Arc::new(StorageChannelBridge::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_creation() {
        let bridge = StorageChannelBridge::new();
        assert!(hpc_channels::exists(hpc_channels::channels::STORAGE_UPLOAD));
        assert!(hpc_channels::exists(hpc_channels::channels::STORAGE_DOWNLOAD));
        assert!(hpc_channels::exists(hpc_channels::channels::STORAGE_PROGRESS));
        let _ = bridge;
    }

    #[tokio::test]
    async fn test_upload_start_publishing() {
        let bridge = StorageChannelBridge::new();
        let mut rx = bridge.subscribe_uploads();

        bridge.publish_upload_start(
            "session-123",
            "/local/file.txt",
            "remote:8080/dest",
            1024 * 1024,
            16,
            true,
        );

        let event = rx.recv().await.expect("Should receive event");
        assert_eq!(event.session_id, "session-123");
        assert_eq!(event.total_bytes, 1024 * 1024);
        assert!(event.is_remote);
    }

    #[tokio::test]
    async fn test_progress_publishing() {
        let bridge = StorageChannelBridge::new();
        let mut rx = bridge.subscribe_progress();

        bridge.publish_progress(
            "session-123",
            512 * 1024,     // transferred
            1024 * 1024,    // total
            8,              // chunks done
            16,             // total chunks
            10_000_000.0,   // 10 MB/s
            Some("file.txt"),
            Some(30),       // eta
        );

        let event = rx.recv().await.expect("Should receive event");
        assert_eq!(event.bytes_transferred, 512 * 1024);
        assert_eq!(event.chunks_completed, 8);
        assert_eq!(event.current_file, Some("file.txt".to_string()));
    }

    #[tokio::test]
    async fn test_status_publishing() {
        let bridge = StorageChannelBridge::new();
        let mut rx = bridge.subscribe_status();

        bridge.publish_status("session-123", TransferStatus::Completed, None);

        let event = rx.recv().await.expect("Should receive event");
        assert_eq!(event.status, TransferStatus::Completed);
    }
}
