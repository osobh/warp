//! Shared IPC Types
//!
//! Core data types used in IPC communication between WARP and Horizon.
//! These types are designed to be lightweight and serializable.

use serde::{Deserialize, Serialize};

/// Transfer direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    /// Sending data to another node
    Send,
    /// Receiving data from another node
    Receive,
    /// Bidirectional transfer
    Bidirectional,
}

/// Transfer status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    /// Transfer is queued
    Queued,
    /// Transfer is in progress
    Active,
    /// Transfer is paused
    Paused,
    /// Transfer completed successfully
    Completed,
    /// Transfer failed
    Failed,
    /// Transfer was cancelled
    Cancelled,
}

/// Transfer information for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferInfo {
    /// Unique transfer identifier
    pub id: String,
    /// Transfer name or description
    pub name: String,
    /// Transfer direction
    pub direction: TransferDirection,
    /// Current status
    pub status: TransferStatus,
    /// Progress percentage (0.0 to 100.0)
    pub progress_percent: f64,
    /// Current speed in bytes per second
    pub speed_bps: u64,
    /// Bytes transferred so far
    pub bytes_transferred: u64,
    /// Total bytes to transfer
    pub total_bytes: u64,
    /// Transfer start time (ISO 8601 string)
    pub start_time: String,
    /// Estimated completion time (ISO 8601 string, if available)
    pub eta: Option<String>,
    /// Remote peer identifier
    pub remote_peer: String,
    /// Source path
    pub source: String,
    /// Destination path
    pub destination: String,
}

impl TransferInfo {
    /// Check if transfer is active
    pub fn is_active(&self) -> bool {
        matches!(self.status, TransferStatus::Active)
    }

    /// Check if transfer is complete (success, failed, or cancelled)
    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled
        )
    }

    /// Get speed in human-readable format
    pub fn speed_human(&self) -> String {
        format_bytes_speed(self.speed_bps)
    }
}

/// Edge node status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeStatus {
    /// Edge is connected and healthy
    Connected,
    /// Edge is connecting
    Connecting,
    /// Edge is disconnected
    Disconnected,
    /// Edge has an error
    Error,
}

/// Edge node information for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeInfo {
    /// Edge node identifier
    pub id: String,
    /// Edge node name
    pub name: String,
    /// Edge node address
    pub address: String,
    /// Connection status
    pub status: EdgeStatus,
    /// Round-trip time in milliseconds
    pub rtt_ms: f64,
    /// Number of active transfers
    pub active_transfers: usize,
    /// Total bytes sent through this edge
    pub bytes_sent: u64,
    /// Total bytes received through this edge
    pub bytes_received: u64,
    /// Connection uptime in seconds
    pub uptime_seconds: u64,
    /// Last seen timestamp (ISO 8601)
    pub last_seen: String,
}

impl EdgeInfo {
    /// Check if edge is healthy (connected with reasonable RTT)
    pub fn is_healthy(&self) -> bool {
        self.status == EdgeStatus::Connected && self.rtt_ms < 500.0
    }

    /// Get total bytes transferred
    pub fn total_bytes(&self) -> u64 {
        self.bytes_sent + self.bytes_received
    }
}

/// Metrics summary for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    /// Total number of transfers (all time)
    pub total_transfers: usize,
    /// Number of active transfers
    pub active_transfers: usize,
    /// Number of completed transfers
    pub completed_transfers: usize,
    /// Number of failed transfers
    pub failed_transfers: usize,
    /// Total bytes transferred
    pub total_bytes_transferred: u64,
    /// Current aggregate throughput in bytes per second
    pub aggregate_throughput_bps: u64,
    /// Number of connected edges
    pub connected_edges: usize,
    /// Total edges (connected + disconnected)
    pub total_edges: usize,
    /// Average RTT across all edges in milliseconds
    pub average_rtt_ms: f64,
    /// System uptime in seconds
    pub uptime_seconds: u64,
}

impl MetricsSummary {
    /// Calculate success rate percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_transfers > 0 {
            (self.completed_transfers as f64 / self.total_transfers as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get aggregate throughput in human-readable format
    pub fn throughput_human(&self) -> String {
        format_bytes_speed(self.aggregate_throughput_bps)
    }
}

/// Scheduler metrics for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerMetrics {
    /// Number of queued tasks
    pub queued_tasks: usize,
    /// Number of running tasks
    pub running_tasks: usize,
    /// Number of completed tasks
    pub completed_tasks: usize,
    /// Current scheduler load (0.0 to 1.0)
    pub load: f64,
    /// Average scheduling latency in microseconds
    pub avg_latency_us: u64,
    /// Peak scheduling latency in microseconds
    pub peak_latency_us: u64,
    /// GPU utilization percentage (if available)
    pub gpu_utilization: Option<f64>,
}

/// Alert severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    /// Informational message
    Info,
    /// Warning condition
    Warning,
    /// Error condition
    Error,
    /// Critical condition requiring immediate attention
    Critical,
}

/// Alert message for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert identifier
    pub id: String,
    /// Alert severity level
    pub level: AlertLevel,
    /// Alert message
    pub message: String,
    /// Alert timestamp (ISO 8601)
    pub timestamp: String,
    /// Source component or transfer ID
    pub source: Option<String>,
    /// Whether the alert has been acknowledged
    pub acknowledged: bool,
}

impl Alert {
    /// Create a new alert
    pub fn new(level: AlertLevel, message: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            message: message.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            source: None,
            acknowledged: false,
        }
    }

    /// Create an alert with source
    pub fn with_source(
        level: AlertLevel,
        message: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let mut alert = Self::new(level, message);
        alert.source = Some(source.into());
        alert
    }
}

/// Dashboard state snapshot for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    /// Active transfers
    pub active_transfers: Vec<TransferInfo>,
    /// Recent transfers (last 50)
    pub recent_transfers: Vec<TransferInfo>,
    /// Connected edges
    pub edges: Vec<EdgeInfo>,
    /// Overall metrics
    pub metrics: MetricsSummary,
    /// Scheduler metrics
    pub scheduler: SchedulerMetrics,
    /// Active alerts
    pub alerts: Vec<Alert>,
    /// Snapshot timestamp (ISO 8601)
    pub timestamp: String,
}

impl Default for DashboardSnapshot {
    fn default() -> Self {
        Self {
            active_transfers: Vec::new(),
            recent_transfers: Vec::new(),
            edges: Vec::new(),
            metrics: MetricsSummary {
                total_transfers: 0,
                active_transfers: 0,
                completed_transfers: 0,
                failed_transfers: 0,
                total_bytes_transferred: 0,
                aggregate_throughput_bps: 0,
                connected_edges: 0,
                total_edges: 0,
                average_rtt_ms: 0.0,
                uptime_seconds: 0,
            },
            scheduler: SchedulerMetrics {
                queued_tasks: 0,
                running_tasks: 0,
                completed_tasks: 0,
                load: 0.0,
                avg_latency_us: 0,
                peak_latency_us: 0,
                gpu_utilization: None,
            },
            alerts: Vec::new(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Format bytes per second as human-readable speed
fn format_bytes_speed(bps: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bps >= GB {
        format!("{:.2} GB/s", bps as f64 / GB as f64)
    } else if bps >= MB {
        format!("{:.2} MB/s", bps as f64 / MB as f64)
    } else if bps >= KB {
        format!("{:.2} KB/s", bps as f64 / KB as f64)
    } else {
        format!("{} B/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_info_is_active() {
        let mut info = TransferInfo {
            id: "t1".to_string(),
            name: "test".to_string(),
            direction: TransferDirection::Send,
            status: TransferStatus::Queued,
            progress_percent: 0.0,
            speed_bps: 0,
            bytes_transferred: 0,
            total_bytes: 1000,
            start_time: "2025-01-01T00:00:00Z".to_string(),
            eta: None,
            remote_peer: "peer1".to_string(),
            source: "/src".to_string(),
            destination: "/dst".to_string(),
        };

        assert!(!info.is_active());
        info.status = TransferStatus::Active;
        assert!(info.is_active());
    }

    #[test]
    fn test_transfer_info_is_complete() {
        let mut info = TransferInfo {
            id: "t1".to_string(),
            name: "test".to_string(),
            direction: TransferDirection::Send,
            status: TransferStatus::Active,
            progress_percent: 50.0,
            speed_bps: 1000,
            bytes_transferred: 500,
            total_bytes: 1000,
            start_time: "2025-01-01T00:00:00Z".to_string(),
            eta: None,
            remote_peer: "peer1".to_string(),
            source: "/src".to_string(),
            destination: "/dst".to_string(),
        };

        assert!(!info.is_complete());
        info.status = TransferStatus::Completed;
        assert!(info.is_complete());
        info.status = TransferStatus::Failed;
        assert!(info.is_complete());
        info.status = TransferStatus::Cancelled;
        assert!(info.is_complete());
    }

    #[test]
    fn test_edge_info_is_healthy() {
        let mut edge = EdgeInfo {
            id: "e1".to_string(),
            name: "Edge 1".to_string(),
            address: "127.0.0.1:8080".to_string(),
            status: EdgeStatus::Disconnected,
            rtt_ms: 50.0,
            active_transfers: 0,
            bytes_sent: 0,
            bytes_received: 0,
            uptime_seconds: 0,
            last_seen: "2025-01-01T00:00:00Z".to_string(),
        };

        assert!(!edge.is_healthy());
        edge.status = EdgeStatus::Connected;
        assert!(edge.is_healthy());
        edge.rtt_ms = 600.0;
        assert!(!edge.is_healthy());
    }

    #[test]
    fn test_metrics_success_rate() {
        let metrics = MetricsSummary {
            total_transfers: 10,
            active_transfers: 2,
            completed_transfers: 7,
            failed_transfers: 1,
            total_bytes_transferred: 1000,
            aggregate_throughput_bps: 100,
            connected_edges: 3,
            total_edges: 5,
            average_rtt_ms: 50.0,
            uptime_seconds: 3600,
        };

        assert_eq!(metrics.success_rate(), 70.0);
    }

    #[test]
    fn test_format_bytes_speed() {
        assert_eq!(format_bytes_speed(500), "500 B/s");
        assert_eq!(format_bytes_speed(1024), "1.00 KB/s");
        assert_eq!(format_bytes_speed(1024 * 1024), "1.00 MB/s");
        assert_eq!(format_bytes_speed(1024 * 1024 * 1024), "1.00 GB/s");
    }

    #[test]
    fn test_alert_new() {
        let alert = Alert::new(AlertLevel::Warning, "Test warning");
        assert_eq!(alert.level, AlertLevel::Warning);
        assert_eq!(alert.message, "Test warning");
        assert!(alert.source.is_none());
        assert!(!alert.acknowledged);
    }

    #[test]
    fn test_alert_with_source() {
        let alert = Alert::with_source(AlertLevel::Error, "Test error", "transfer-123");
        assert_eq!(alert.level, AlertLevel::Error);
        assert_eq!(alert.source, Some("transfer-123".to_string()));
    }

    #[test]
    fn test_dashboard_snapshot_default() {
        let snapshot = DashboardSnapshot::default();
        assert_eq!(snapshot.active_transfers.len(), 0);
        assert_eq!(snapshot.metrics.total_transfers, 0);
    }
}
