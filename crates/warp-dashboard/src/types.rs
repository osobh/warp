//! Dashboard Data Types
//!
//! This module defines the core data structures for the dashboard UI,
//! including transfer views, edge status, metrics, and alerts.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Transfer direction for UI display
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

impl fmt::Display for TransferDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send => write!(f, "send"),
            Self::Receive => write!(f, "receive"),
            Self::Bidirectional => write!(f, "bidirectional"),
        }
    }
}

/// Transfer status for UI display
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    /// Transfer is queued
    Queued,
    /// Transfer is in progress
    Active,
    /// Transfer completed successfully
    Completed,
    /// Transfer failed
    Failed,
    /// Transfer was paused
    Paused,
    /// Transfer was cancelled
    Cancelled,
}

impl fmt::Display for TransferStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued => write!(f, "queued"),
            Self::Active => write!(f, "active"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Paused => write!(f, "paused"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
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

impl fmt::Display for AlertLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Transfer view for UI representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferView {
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
    /// Current transfer speed in Mbps
    pub speed_mbps: f64,
    /// Estimated time to completion in seconds
    pub eta_seconds: Option<u64>,
    /// Bytes transferred so far
    pub bytes_transferred: u64,
    /// Total bytes to transfer
    pub total_bytes: u64,
    /// Transfer start time (Unix timestamp)
    pub start_time: u64,
    /// Remote peer identifier
    pub remote_peer: String,
}

impl TransferView {
    /// Create a new transfer view
    pub fn new(id: String, name: String, direction: TransferDirection) -> Self {
        Self {
            id,
            name,
            direction,
            status: TransferStatus::Queued,
            progress_percent: 0.0,
            speed_mbps: 0.0,
            eta_seconds: None,
            bytes_transferred: 0,
            total_bytes: 0,
            start_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            remote_peer: String::new(),
        }
    }

    /// Calculate progress percentage from bytes
    pub fn calculate_progress(&mut self) {
        if self.total_bytes > 0 {
            self.progress_percent =
                (self.bytes_transferred as f64 / self.total_bytes as f64) * 100.0;
        } else {
            self.progress_percent = 0.0;
        }
    }

    /// Calculate ETA based on current speed
    pub fn calculate_eta(&mut self) {
        if self.speed_mbps > 0.0 && self.total_bytes > self.bytes_transferred {
            let remaining_bytes = self.total_bytes - self.bytes_transferred;
            let remaining_megabytes = remaining_bytes as f64 / 1_000_000.0;
            let seconds = remaining_megabytes / self.speed_mbps;
            self.eta_seconds = Some(seconds.ceil() as u64);
        } else {
            self.eta_seconds = None;
        }
    }

    /// Update transfer metrics
    pub fn update_metrics(&mut self, bytes_transferred: u64, speed_mbps: f64) {
        self.bytes_transferred = bytes_transferred;
        self.speed_mbps = speed_mbps;
        self.calculate_progress();
        self.calculate_eta();
    }

    /// Check if transfer is active
    pub fn is_active(&self) -> bool {
        matches!(self.status, TransferStatus::Active)
    }

    /// Check if transfer is complete
    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled
        )
    }
}

/// Edge connection view for UI representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeView {
    /// Edge node identifier
    pub id: String,
    /// Edge node address
    pub address: String,
    /// Connection status
    pub connected: bool,
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
}

impl EdgeView {
    /// Create a new edge view
    pub fn new(id: String, address: String) -> Self {
        Self {
            id,
            address,
            connected: false,
            rtt_ms: 0.0,
            active_transfers: 0,
            bytes_sent: 0,
            bytes_received: 0,
            uptime_seconds: 0,
        }
    }

    /// Check if edge is healthy (connected with reasonable RTT)
    pub fn is_healthy(&self) -> bool {
        self.connected && self.rtt_ms < 500.0
    }

    /// Get total bytes transferred
    pub fn total_bytes(&self) -> u64 {
        self.bytes_sent + self.bytes_received
    }
}

/// Metrics summary for dashboard display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    /// Total number of transfers
    pub total_transfers: usize,
    /// Number of active transfers
    pub active_transfers: usize,
    /// Number of completed transfers
    pub completed_transfers: usize,
    /// Number of failed transfers
    pub failed_transfers: usize,
    /// Total bytes transferred across all transfers
    pub total_bytes_transferred: u64,
    /// Current aggregate throughput in Mbps
    pub aggregate_throughput_mbps: f64,
    /// Average transfer speed in Mbps
    pub average_speed_mbps: f64,
    /// Number of connected edges
    pub connected_edges: usize,
    /// Average RTT across all edges in milliseconds
    pub average_rtt_ms: f64,
}

impl MetricsSummary {
    /// Create a new empty metrics summary
    pub fn new() -> Self {
        Self {
            total_transfers: 0,
            active_transfers: 0,
            completed_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            aggregate_throughput_mbps: 0.0,
            average_speed_mbps: 0.0,
            connected_edges: 0,
            average_rtt_ms: 0.0,
        }
    }

    /// Calculate success rate percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_transfers > 0 {
            (self.completed_transfers as f64 / self.total_transfers as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Calculate failure rate percentage
    pub fn failure_rate(&self) -> f64 {
        if self.total_transfers > 0 {
            (self.failed_transfers as f64 / self.total_transfers as f64) * 100.0
        } else {
            0.0
        }
    }
}

impl Default for MetricsSummary {
    fn default() -> Self {
        Self::new()
    }
}

/// Alert message for dashboard notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert identifier
    pub id: String,
    /// Alert severity level
    pub level: AlertLevel,
    /// Alert message
    pub message: String,
    /// Alert timestamp (Unix timestamp)
    pub timestamp: u64,
    /// Source component or transfer ID
    pub source: Option<String>,
}

impl Alert {
    /// Create a new alert
    pub fn new(level: AlertLevel, message: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            message,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            source: None,
        }
    }

    /// Create a new alert with source
    pub fn with_source(level: AlertLevel, message: String, source: String) -> Self {
        let mut alert = Self::new(level, message);
        alert.source = Some(source);
        alert
    }

    /// Get age of alert in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.timestamp)
    }
}

/// Complete dashboard state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardState {
    /// Currently active transfers
    pub active_transfers: Vec<TransferView>,
    /// Recently completed transfers
    pub recent_transfers: Vec<TransferView>,
    /// Connected edge nodes
    pub connected_edges: Vec<EdgeView>,
    /// Aggregated metrics
    pub metrics: MetricsSummary,
    /// Active alerts
    pub alerts: Vec<Alert>,
    /// System uptime in seconds
    pub uptime_seconds: u64,
}

impl DashboardState {
    /// Create a new empty dashboard state
    pub fn new() -> Self {
        Self {
            active_transfers: Vec::new(),
            recent_transfers: Vec::new(),
            connected_edges: Vec::new(),
            metrics: MetricsSummary::new(),
            alerts: Vec::new(),
            uptime_seconds: 0,
        }
    }

    /// Add an active transfer
    pub fn add_active_transfer(&mut self, transfer: TransferView) {
        self.active_transfers.push(transfer);
        self.update_metrics();
    }

    /// Move transfer to recent transfers
    pub fn complete_transfer(&mut self, transfer_id: &str) {
        if let Some(pos) = self
            .active_transfers
            .iter()
            .position(|t| t.id == transfer_id)
        {
            let mut transfer = self.active_transfers.remove(pos);
            transfer.status = TransferStatus::Completed;
            self.recent_transfers.insert(0, transfer);
            // Keep only last 50 recent transfers
            if self.recent_transfers.len() > 50 {
                self.recent_transfers.truncate(50);
            }
            self.update_metrics();
        }
    }

    /// Add an edge connection
    pub fn add_edge(&mut self, edge: EdgeView) {
        self.connected_edges.push(edge);
        self.update_metrics();
    }

    /// Add an alert
    pub fn add_alert(&mut self, alert: Alert) {
        self.alerts.insert(0, alert);
        // Keep only last 100 alerts
        if self.alerts.len() > 100 {
            self.alerts.truncate(100);
        }
    }

    /// Update aggregated metrics from current state
    pub fn update_metrics(&mut self) {
        self.metrics.active_transfers = self.active_transfers.len();
        self.metrics.completed_transfers = self
            .recent_transfers
            .iter()
            .filter(|t| matches!(t.status, TransferStatus::Completed))
            .count();
        self.metrics.failed_transfers = self
            .recent_transfers
            .iter()
            .filter(|t| matches!(t.status, TransferStatus::Failed))
            .count();
        self.metrics.total_transfers =
            self.metrics.active_transfers + self.recent_transfers.len();

        self.metrics.total_bytes_transferred = self
            .active_transfers
            .iter()
            .chain(self.recent_transfers.iter())
            .map(|t| t.bytes_transferred)
            .sum();

        self.metrics.aggregate_throughput_mbps = self
            .active_transfers
            .iter()
            .map(|t| t.speed_mbps)
            .sum();

        if !self.active_transfers.is_empty() {
            self.metrics.average_speed_mbps = self.metrics.aggregate_throughput_mbps
                / self.active_transfers.len() as f64;
        } else {
            self.metrics.average_speed_mbps = 0.0;
        }

        self.metrics.connected_edges = self
            .connected_edges
            .iter()
            .filter(|e| e.connected)
            .count();

        if !self.connected_edges.is_empty() {
            self.metrics.average_rtt_ms = self
                .connected_edges
                .iter()
                .map(|e| e.rtt_ms)
                .sum::<f64>()
                / self.connected_edges.len() as f64;
        } else {
            self.metrics.average_rtt_ms = 0.0;
        }
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_direction_serialization() {
        let dir = TransferDirection::Send;
        let json = serde_json::to_string(&dir).unwrap();
        assert_eq!(json, "\"send\"");

        let deserialized: TransferDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dir);
    }

    #[test]
    fn test_transfer_view_new() {
        let transfer = TransferView::new(
            "test-id".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        assert_eq!(transfer.id, "test-id");
        assert_eq!(transfer.name, "test-transfer");
        assert_eq!(transfer.direction, TransferDirection::Send);
        assert_eq!(transfer.status, TransferStatus::Queued);
        assert_eq!(transfer.progress_percent, 0.0);
    }

    #[test]
    fn test_transfer_view_calculate_progress() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        transfer.total_bytes = 1000;
        transfer.bytes_transferred = 250;
        transfer.calculate_progress();
        assert_eq!(transfer.progress_percent, 25.0);
    }

    #[test]
    fn test_transfer_view_calculate_progress_zero_total() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        transfer.total_bytes = 0;
        transfer.bytes_transferred = 100;
        transfer.calculate_progress();
        assert_eq!(transfer.progress_percent, 0.0);
    }

    #[test]
    fn test_transfer_view_calculate_eta() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        transfer.total_bytes = 10_000_000; // 10 MB
        transfer.bytes_transferred = 5_000_000; // 5 MB
        transfer.speed_mbps = 1.0; // 1 Mbps
        transfer.calculate_eta();
        assert!(transfer.eta_seconds.is_some());
        // Remaining: 5 MB / 1 Mbps = 5 seconds
        assert_eq!(transfer.eta_seconds.unwrap(), 5);
    }

    #[test]
    fn test_transfer_view_calculate_eta_zero_speed() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        transfer.total_bytes = 10_000_000;
        transfer.bytes_transferred = 5_000_000;
        transfer.speed_mbps = 0.0;
        transfer.calculate_eta();
        assert!(transfer.eta_seconds.is_none());
    }

    #[test]
    fn test_transfer_view_update_metrics() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        transfer.total_bytes = 1000;
        transfer.update_metrics(500, 2.0);
        assert_eq!(transfer.bytes_transferred, 500);
        assert_eq!(transfer.speed_mbps, 2.0);
        assert_eq!(transfer.progress_percent, 50.0);
    }

    #[test]
    fn test_transfer_view_is_active() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        assert!(!transfer.is_active());
        transfer.status = TransferStatus::Active;
        assert!(transfer.is_active());
    }

    #[test]
    fn test_transfer_view_is_complete() {
        let mut transfer = TransferView::new(
            "test".to_string(),
            "test".to_string(),
            TransferDirection::Send,
        );
        assert!(!transfer.is_complete());
        transfer.status = TransferStatus::Completed;
        assert!(transfer.is_complete());
        transfer.status = TransferStatus::Failed;
        assert!(transfer.is_complete());
        transfer.status = TransferStatus::Cancelled;
        assert!(transfer.is_complete());
    }

    #[test]
    fn test_edge_view_new() {
        let edge = EdgeView::new("edge-1".to_string(), "127.0.0.1:8080".to_string());
        assert_eq!(edge.id, "edge-1");
        assert_eq!(edge.address, "127.0.0.1:8080");
        assert!(!edge.connected);
        assert_eq!(edge.active_transfers, 0);
    }

    #[test]
    fn test_edge_view_is_healthy() {
        let mut edge = EdgeView::new("edge-1".to_string(), "127.0.0.1:8080".to_string());
        assert!(!edge.is_healthy());

        edge.connected = true;
        edge.rtt_ms = 100.0;
        assert!(edge.is_healthy());

        edge.rtt_ms = 600.0;
        assert!(!edge.is_healthy());
    }

    #[test]
    fn test_edge_view_total_bytes() {
        let mut edge = EdgeView::new("edge-1".to_string(), "127.0.0.1:8080".to_string());
        edge.bytes_sent = 1000;
        edge.bytes_received = 2000;
        assert_eq!(edge.total_bytes(), 3000);
    }

    #[test]
    fn test_metrics_summary_new() {
        let metrics = MetricsSummary::new();
        assert_eq!(metrics.total_transfers, 0);
        assert_eq!(metrics.active_transfers, 0);
        assert_eq!(metrics.aggregate_throughput_mbps, 0.0);
    }

    #[test]
    fn test_metrics_summary_success_rate() {
        let mut metrics = MetricsSummary::new();
        metrics.total_transfers = 10;
        metrics.completed_transfers = 8;
        assert_eq!(metrics.success_rate(), 80.0);
    }

    #[test]
    fn test_metrics_summary_success_rate_zero_transfers() {
        let metrics = MetricsSummary::new();
        assert_eq!(metrics.success_rate(), 0.0);
    }

    #[test]
    fn test_metrics_summary_failure_rate() {
        let mut metrics = MetricsSummary::new();
        metrics.total_transfers = 10;
        metrics.failed_transfers = 2;
        assert_eq!(metrics.failure_rate(), 20.0);
    }

    #[test]
    fn test_alert_new() {
        let alert = Alert::new(AlertLevel::Warning, "Test alert".to_string());
        assert_eq!(alert.level, AlertLevel::Warning);
        assert_eq!(alert.message, "Test alert");
        assert!(alert.source.is_none());
        assert!(!alert.id.is_empty());
    }

    #[test]
    fn test_alert_with_source() {
        let alert = Alert::with_source(
            AlertLevel::Error,
            "Test error".to_string(),
            "transfer-123".to_string(),
        );
        assert_eq!(alert.level, AlertLevel::Error);
        assert_eq!(alert.source, Some("transfer-123".to_string()));
    }

    #[test]
    fn test_alert_level_ordering() {
        assert!(AlertLevel::Info < AlertLevel::Warning);
        assert!(AlertLevel::Warning < AlertLevel::Error);
        assert!(AlertLevel::Error < AlertLevel::Critical);
    }

    #[test]
    fn test_dashboard_state_new() {
        let state = DashboardState::new();
        assert_eq!(state.active_transfers.len(), 0);
        assert_eq!(state.recent_transfers.len(), 0);
        assert_eq!(state.connected_edges.len(), 0);
        assert_eq!(state.alerts.len(), 0);
    }

    #[test]
    fn test_dashboard_state_add_active_transfer() {
        let mut state = DashboardState::new();
        let transfer = TransferView::new(
            "t1".to_string(),
            "transfer1".to_string(),
            TransferDirection::Send,
        );
        state.add_active_transfer(transfer);
        assert_eq!(state.active_transfers.len(), 1);
        assert_eq!(state.metrics.active_transfers, 1);
    }

    #[test]
    fn test_dashboard_state_complete_transfer() {
        let mut state = DashboardState::new();
        let mut transfer = TransferView::new(
            "t1".to_string(),
            "transfer1".to_string(),
            TransferDirection::Send,
        );
        transfer.status = TransferStatus::Active;
        state.add_active_transfer(transfer);

        state.complete_transfer("t1");
        assert_eq!(state.active_transfers.len(), 0);
        assert_eq!(state.recent_transfers.len(), 1);
        assert_eq!(state.recent_transfers[0].status, TransferStatus::Completed);
    }

    #[test]
    fn test_dashboard_state_add_edge() {
        let mut state = DashboardState::new();
        let edge = EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string());
        state.add_edge(edge);
        assert_eq!(state.connected_edges.len(), 1);
    }

    #[test]
    fn test_dashboard_state_add_alert() {
        let mut state = DashboardState::new();
        let alert = Alert::new(AlertLevel::Info, "Test alert".to_string());
        state.add_alert(alert);
        assert_eq!(state.alerts.len(), 1);
    }

    #[test]
    fn test_dashboard_state_alert_limit() {
        let mut state = DashboardState::new();
        for i in 0..150 {
            let alert = Alert::new(AlertLevel::Info, format!("Alert {}", i));
            state.add_alert(alert);
        }
        assert_eq!(state.alerts.len(), 100);
    }

    #[test]
    fn test_dashboard_state_recent_transfer_limit() {
        let mut state = DashboardState::new();
        for i in 0..60 {
            let mut transfer = TransferView::new(
                format!("t{}", i),
                format!("transfer{}", i),
                TransferDirection::Send,
            );
            transfer.status = TransferStatus::Active;
            state.add_active_transfer(transfer);
        }
        for i in 0..60 {
            state.complete_transfer(&format!("t{}", i));
        }
        assert_eq!(state.recent_transfers.len(), 50);
    }

    #[test]
    fn test_dashboard_state_update_metrics() {
        let mut state = DashboardState::new();

        // Add active transfers
        let mut t1 = TransferView::new(
            "t1".to_string(),
            "transfer1".to_string(),
            TransferDirection::Send,
        );
        t1.status = TransferStatus::Active;
        t1.speed_mbps = 10.0;
        t1.bytes_transferred = 1000;

        let mut t2 = TransferView::new(
            "t2".to_string(),
            "transfer2".to_string(),
            TransferDirection::Receive,
        );
        t2.status = TransferStatus::Active;
        t2.speed_mbps = 20.0;
        t2.bytes_transferred = 2000;

        state.add_active_transfer(t1);
        state.add_active_transfer(t2);

        // Complete one transfer
        state.complete_transfer("t1");

        // Add edges
        let mut e1 = EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string());
        e1.connected = true;
        e1.rtt_ms = 50.0;

        let mut e2 = EdgeView::new("e2".to_string(), "127.0.0.1:8081".to_string());
        e2.connected = true;
        e2.rtt_ms = 100.0;

        state.add_edge(e1);
        state.add_edge(e2);

        assert_eq!(state.metrics.active_transfers, 1);
        assert_eq!(state.metrics.completed_transfers, 1);
        assert_eq!(state.metrics.total_transfers, 2);
        assert_eq!(state.metrics.aggregate_throughput_mbps, 20.0);
        assert_eq!(state.metrics.average_speed_mbps, 20.0);
        assert_eq!(state.metrics.connected_edges, 2);
        assert_eq!(state.metrics.average_rtt_ms, 75.0);
    }
}
