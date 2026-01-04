//! IPC Event Definitions
//!
//! Defines events that WARP can push to Horizon via Tauri IPC.
//! Events are used for real-time updates without polling.

use serde::{Deserialize, Serialize};

use crate::types::*;

/// IPC Events pushed from WARP to Horizon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum IpcEvent {
    // ==================== Transfer Events ====================
    /// Transfer started
    TransferStarted {
        /// Transfer information
        transfer: TransferInfo,
    },

    /// Transfer progress update
    TransferProgress {
        /// Transfer ID
        transfer_id: String,
        /// Bytes transferred so far
        bytes_transferred: u64,
        /// Total bytes to transfer
        total_bytes: u64,
        /// Current speed in bytes per second
        speed_bps: u64,
        /// Progress percentage
        progress_percent: f64,
        /// Estimated time remaining in seconds
        eta_seconds: Option<u64>,
    },

    /// Transfer state changed
    TransferStateChanged {
        /// Transfer ID
        transfer_id: String,
        /// Previous status
        previous_status: TransferStatus,
        /// New status
        new_status: TransferStatus,
    },

    /// Transfer completed successfully
    TransferCompleted {
        /// Transfer ID
        transfer_id: String,
        /// Total bytes transferred
        total_bytes: u64,
        /// Transfer duration in seconds
        duration_seconds: u64,
        /// Average speed in bytes per second
        avg_speed_bps: u64,
    },

    /// Transfer failed
    TransferFailed {
        /// Transfer ID
        transfer_id: String,
        /// Error message
        error: String,
        /// Bytes transferred before failure
        bytes_transferred: u64,
    },

    // ==================== Edge Events ====================
    /// Edge connected
    EdgeConnected {
        /// Edge information
        edge: EdgeInfo,
    },

    /// Edge disconnected
    EdgeDisconnected {
        /// Edge ID
        edge_id: String,
        /// Reason for disconnection
        reason: Option<String>,
    },

    /// Edge health changed
    EdgeHealthChanged {
        /// Edge ID
        edge_id: String,
        /// New RTT in milliseconds
        rtt_ms: f64,
        /// Whether edge is considered healthy
        is_healthy: bool,
    },

    // ==================== Metrics Events ====================
    /// Metrics updated (periodic)
    MetricsUpdated {
        /// Updated metrics
        metrics: MetricsSummary,
    },

    /// Scheduler metrics updated
    SchedulerUpdated {
        /// Updated scheduler metrics
        scheduler: SchedulerMetrics,
    },

    /// Throughput spike detected
    ThroughputSpike {
        /// Current throughput in bytes per second
        current_bps: u64,
        /// Previous throughput in bytes per second
        previous_bps: u64,
        /// Percentage change
        change_percent: f64,
    },

    // ==================== Alert Events ====================
    /// New alert
    AlertRaised {
        /// Alert information
        alert: Alert,
    },

    /// Alert acknowledged
    AlertAcknowledged {
        /// Alert ID
        alert_id: String,
    },

    /// Alert cleared
    AlertCleared {
        /// Alert ID
        alert_id: String,
    },

    // ==================== System Events ====================
    /// System starting up
    SystemStarting,

    /// System ready
    SystemReady {
        /// System version
        version: String,
    },

    /// System shutting down
    SystemShutdown {
        /// Reason for shutdown
        reason: Option<String>,
    },

    /// Heartbeat (periodic, for connection keepalive)
    Heartbeat {
        /// Timestamp (ISO 8601)
        timestamp: String,
        /// System uptime in seconds
        uptime_seconds: u64,
    },
}

impl IpcEvent {
    /// Get the event name as a string
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::TransferStarted { .. } => "transfer_started",
            Self::TransferProgress { .. } => "transfer_progress",
            Self::TransferStateChanged { .. } => "transfer_state_changed",
            Self::TransferCompleted { .. } => "transfer_completed",
            Self::TransferFailed { .. } => "transfer_failed",
            Self::EdgeConnected { .. } => "edge_connected",
            Self::EdgeDisconnected { .. } => "edge_disconnected",
            Self::EdgeHealthChanged { .. } => "edge_health_changed",
            Self::MetricsUpdated { .. } => "metrics_updated",
            Self::SchedulerUpdated { .. } => "scheduler_updated",
            Self::ThroughputSpike { .. } => "throughput_spike",
            Self::AlertRaised { .. } => "alert_raised",
            Self::AlertAcknowledged { .. } => "alert_acknowledged",
            Self::AlertCleared { .. } => "alert_cleared",
            Self::SystemStarting => "system_starting",
            Self::SystemReady { .. } => "system_ready",
            Self::SystemShutdown { .. } => "system_shutdown",
            Self::Heartbeat { .. } => "heartbeat",
        }
    }

    /// Check if this is a transfer-related event
    pub fn is_transfer_event(&self) -> bool {
        matches!(
            self,
            Self::TransferStarted { .. }
                | Self::TransferProgress { .. }
                | Self::TransferStateChanged { .. }
                | Self::TransferCompleted { .. }
                | Self::TransferFailed { .. }
        )
    }

    /// Check if this is an edge-related event
    pub fn is_edge_event(&self) -> bool {
        matches!(
            self,
            Self::EdgeConnected { .. }
                | Self::EdgeDisconnected { .. }
                | Self::EdgeHealthChanged { .. }
        )
    }

    /// Check if this is a metrics-related event
    pub fn is_metrics_event(&self) -> bool {
        matches!(
            self,
            Self::MetricsUpdated { .. }
                | Self::SchedulerUpdated { .. }
                | Self::ThroughputSpike { .. }
        )
    }

    /// Check if this is an alert-related event
    pub fn is_alert_event(&self) -> bool {
        matches!(
            self,
            Self::AlertRaised { .. } | Self::AlertAcknowledged { .. } | Self::AlertCleared { .. }
        )
    }

    /// Check if this is a system-related event
    pub fn is_system_event(&self) -> bool {
        matches!(
            self,
            Self::SystemStarting
                | Self::SystemReady { .. }
                | Self::SystemShutdown { .. }
                | Self::Heartbeat { .. }
        )
    }
}

/// Event batch for efficient transmission of multiple events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBatch {
    /// Batch timestamp (ISO 8601)
    pub timestamp: String,
    /// Events in this batch
    pub events: Vec<IpcEvent>,
}

impl EventBatch {
    /// Create a new event batch
    pub fn new(events: Vec<IpcEvent>) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            events,
        }
    }

    /// Create an empty batch
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Add an event to the batch
    pub fn push(&mut self, event: IpcEvent) {
        self.events.push(event);
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get number of events in batch
    pub fn len(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = IpcEvent::TransferProgress {
            transfer_id: "abc-123".to_string(),
            bytes_transferred: 1_000_000,
            total_bytes: 10_000_000,
            speed_bps: 100_000,
            progress_percent: 10.0,
            eta_seconds: Some(90),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("TransferProgress"));
        assert!(json.contains("abc-123"));
        assert!(json.contains("1000000"));
    }

    #[test]
    fn test_event_deserialization() {
        let json = r#"{"event":"TransferCompleted","data":{"transfer_id":"abc-123","total_bytes":10000000,"duration_seconds":100,"avg_speed_bps":100000}}"#;
        let event: IpcEvent = serde_json::from_str(json).unwrap();
        match event {
            IpcEvent::TransferCompleted {
                transfer_id,
                total_bytes,
                ..
            } => {
                assert_eq!(transfer_id, "abc-123");
                assert_eq!(total_bytes, 10_000_000);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_event_name() {
        let event = IpcEvent::TransferStarted {
            transfer: TransferInfo {
                id: "t1".to_string(),
                name: "test".to_string(),
                direction: TransferDirection::Send,
                status: TransferStatus::Active,
                progress_percent: 0.0,
                speed_bps: 0,
                bytes_transferred: 0,
                total_bytes: 1000,
                start_time: "2025-01-01T00:00:00Z".to_string(),
                eta: None,
                remote_peer: "peer1".to_string(),
                source: "/src".to_string(),
                destination: "/dst".to_string(),
            },
        };
        assert_eq!(event.event_name(), "transfer_started");

        let event = IpcEvent::EdgeConnected {
            edge: EdgeInfo {
                id: "e1".to_string(),
                name: "Edge 1".to_string(),
                address: "127.0.0.1:8080".to_string(),
                status: EdgeStatus::Connected,
                rtt_ms: 50.0,
                active_transfers: 0,
                bytes_sent: 0,
                bytes_received: 0,
                uptime_seconds: 0,
                last_seen: "2025-01-01T00:00:00Z".to_string(),
            },
        };
        assert_eq!(event.event_name(), "edge_connected");
    }

    #[test]
    fn test_event_type_checks() {
        let transfer_event = IpcEvent::TransferProgress {
            transfer_id: "t1".to_string(),
            bytes_transferred: 100,
            total_bytes: 1000,
            speed_bps: 10,
            progress_percent: 10.0,
            eta_seconds: None,
        };
        assert!(transfer_event.is_transfer_event());
        assert!(!transfer_event.is_edge_event());
        assert!(!transfer_event.is_alert_event());

        let edge_event = IpcEvent::EdgeDisconnected {
            edge_id: "e1".to_string(),
            reason: None,
        };
        assert!(!edge_event.is_transfer_event());
        assert!(edge_event.is_edge_event());

        let alert_event = IpcEvent::AlertRaised {
            alert: Alert::new(AlertLevel::Warning, "Test"),
        };
        assert!(alert_event.is_alert_event());
    }

    #[test]
    fn test_event_batch() {
        let mut batch = EventBatch::empty();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);

        batch.push(IpcEvent::Heartbeat {
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            uptime_seconds: 3600,
        });

        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn test_event_batch_serialization() {
        let batch = EventBatch::new(vec![
            IpcEvent::SystemStarting,
            IpcEvent::SystemReady {
                version: "1.0.0".to_string(),
            },
        ]);

        let json = serde_json::to_string(&batch).unwrap();
        assert!(json.contains("SystemStarting"));
        assert!(json.contains("SystemReady"));
        assert!(json.contains("1.0.0"));
    }
}
