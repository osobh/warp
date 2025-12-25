//! IPC Command Definitions
//!
//! Defines commands that Horizon can send to WARP via Tauri IPC.
//! Each command has a corresponding response type.

use serde::{Deserialize, Serialize};

use crate::types::*;

/// IPC Commands that Horizon can invoke
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "params")]
pub enum IpcCommand {
    // ==================== Transfer Commands ====================
    /// Get all active transfers
    GetTransfers,

    /// Get a specific transfer by ID
    GetTransfer {
        /// Transfer ID
        transfer_id: String,
    },

    /// Get recent transfers (completed/failed)
    GetRecentTransfers {
        /// Maximum number of transfers to return (default: 50)
        limit: Option<usize>,
    },

    /// Pause a transfer
    PauseTransfer {
        /// Transfer ID
        transfer_id: String,
    },

    /// Resume a paused transfer
    ResumeTransfer {
        /// Transfer ID
        transfer_id: String,
    },

    /// Cancel a transfer
    CancelTransfer {
        /// Transfer ID
        transfer_id: String,
    },

    /// Start a new transfer
    StartTransfer {
        /// Source path
        source: String,
        /// Destination path
        destination: String,
        /// Remote peer (optional, for remote transfers)
        remote_peer: Option<String>,
    },

    // ==================== Edge Commands ====================
    /// Get all edge nodes
    GetEdges,

    /// Get a specific edge by ID
    GetEdge {
        /// Edge ID
        edge_id: String,
    },

    /// Connect to an edge node
    ConnectEdge {
        /// Edge address
        address: String,
    },

    /// Disconnect from an edge node
    DisconnectEdge {
        /// Edge ID
        edge_id: String,
    },

    /// Ping an edge node
    PingEdge {
        /// Edge ID
        edge_id: String,
    },

    // ==================== Metrics Commands ====================
    /// Get overall metrics summary
    GetMetrics,

    /// Get scheduler metrics
    GetSchedulerMetrics,

    /// Get full dashboard snapshot
    GetDashboardSnapshot,

    // ==================== Alert Commands ====================
    /// Get all alerts
    GetAlerts {
        /// Minimum severity level (optional)
        min_level: Option<AlertLevel>,
        /// Include acknowledged alerts (default: false)
        include_acknowledged: Option<bool>,
    },

    /// Acknowledge an alert
    AcknowledgeAlert {
        /// Alert ID
        alert_id: String,
    },

    /// Clear all acknowledged alerts
    ClearAcknowledgedAlerts,

    // ==================== Subscription Commands ====================
    /// Subscribe to real-time events
    Subscribe {
        /// Event types to subscribe to
        events: Vec<EventFilter>,
    },

    /// Unsubscribe from events
    Unsubscribe,
}

/// Event filter for subscriptions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventFilter {
    /// All transfer events
    Transfers,
    /// Transfer progress updates only
    TransferProgress,
    /// Transfer state changes only
    TransferState,
    /// All edge events
    Edges,
    /// Metrics updates
    Metrics,
    /// Alerts
    Alerts,
    /// All events
    All,
}

/// IPC Response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse<T> {
    /// Successful response
    #[serde(rename = "ok")]
    Ok {
        /// Response data
        data: T,
    },
    /// Error response
    #[serde(rename = "error")]
    Error {
        /// Error code
        code: String,
        /// Error message
        message: String,
    },
}

impl<T> IpcResponse<T> {
    /// Create a success response
    pub fn ok(data: T) -> Self {
        Self::Ok { data }
    }

    /// Create an error response
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Check if response is successful
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    /// Check if response is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// Response types for specific commands
pub mod responses {
    use super::*;

    /// Response for GetTransfers
    pub type TransfersResponse = IpcResponse<Vec<TransferInfo>>;

    /// Response for GetTransfer
    pub type TransferResponse = IpcResponse<TransferInfo>;

    /// Response for GetEdges
    pub type EdgesResponse = IpcResponse<Vec<EdgeInfo>>;

    /// Response for GetEdge
    pub type EdgeResponse = IpcResponse<EdgeInfo>;

    /// Response for GetMetrics
    pub type MetricsResponse = IpcResponse<MetricsSummary>;

    /// Response for GetSchedulerMetrics
    pub type SchedulerResponse = IpcResponse<SchedulerMetrics>;

    /// Response for GetDashboardSnapshot
    pub type DashboardResponse = IpcResponse<DashboardSnapshot>;

    /// Response for GetAlerts
    pub type AlertsResponse = IpcResponse<Vec<Alert>>;

    /// Response for operations that return nothing
    pub type VoidResponse = IpcResponse<()>;

    /// Response for StartTransfer
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StartTransferResult {
        /// Created transfer ID
        pub transfer_id: String,
    }

    /// Response for PingEdge
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PingResult {
        /// Edge ID
        pub edge_id: String,
        /// Round-trip time in milliseconds
        pub rtt_ms: f64,
        /// Whether the edge is reachable
        pub reachable: bool,
    }

    /// Response for Subscribe
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SubscribeResult {
        /// Subscription ID
        pub subscription_id: String,
        /// Subscribed event filters
        pub filters: Vec<EventFilter>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = IpcCommand::GetTransfers;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("GetTransfers"));

        let cmd = IpcCommand::GetTransfer {
            transfer_id: "abc-123".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("GetTransfer"));
        assert!(json.contains("abc-123"));
    }

    #[test]
    fn test_command_deserialization() {
        let json = r#"{"cmd":"GetTransfers"}"#;
        let cmd: IpcCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, IpcCommand::GetTransfers));

        let json = r#"{"cmd":"GetTransfer","params":{"transfer_id":"abc-123"}}"#;
        let cmd: IpcCommand = serde_json::from_str(json).unwrap();
        match cmd {
            IpcCommand::GetTransfer { transfer_id } => {
                assert_eq!(transfer_id, "abc-123");
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_response_ok() {
        let resp: IpcResponse<i32> = IpcResponse::ok(42);
        assert!(resp.is_ok());
        assert!(!resp.is_error());

        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ok"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_response_error() {
        let resp: IpcResponse<i32> = IpcResponse::error("NOT_FOUND", "Transfer not found");
        assert!(!resp.is_ok());
        assert!(resp.is_error());

        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("NOT_FOUND"));
    }

    #[test]
    fn test_event_filter_serialization() {
        let filter = EventFilter::TransferProgress;
        let json = serde_json::to_string(&filter).unwrap();
        assert_eq!(json, "\"transfer_progress\"");

        let filter = EventFilter::All;
        let json = serde_json::to_string(&filter).unwrap();
        assert_eq!(json, "\"all\"");
    }

    #[test]
    fn test_subscribe_command() {
        let cmd = IpcCommand::Subscribe {
            events: vec![EventFilter::Transfers, EventFilter::Alerts],
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("Subscribe"));
        assert!(json.contains("transfers"));
        assert!(json.contains("alerts"));
    }

    #[test]
    fn test_start_transfer_command() {
        let cmd = IpcCommand::StartTransfer {
            source: "/path/to/source".to_string(),
            destination: "/path/to/dest".to_string(),
            remote_peer: Some("peer-123".to_string()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("StartTransfer"));
        assert!(json.contains("/path/to/source"));
        assert!(json.contains("peer-123"));
    }
}
