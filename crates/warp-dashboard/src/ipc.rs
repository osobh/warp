//! IPC Command Handlers for Horizon Integration
//!
//! This module provides the bridge between warp-dashboard state
//! and the warp-ipc command/event types used by Horizon.

use crate::handlers::AppState;
use crate::types::{DashboardState, EdgeView, TransferView};
use warp_ipc::{
    IpcError, IpcResult,
    commands::{EventFilter, IpcCommand, IpcResponse, responses::*},
    events::IpcEvent,
    types::*,
};

/// Convert internal TransferView to IPC TransferInfo
fn transfer_to_ipc(transfer: &TransferView) -> TransferInfo {
    TransferInfo {
        id: transfer.id.clone(),
        name: transfer.name.clone(),
        direction: match transfer.direction {
            crate::types::TransferDirection::Send => TransferDirection::Send,
            crate::types::TransferDirection::Receive => TransferDirection::Receive,
            crate::types::TransferDirection::Bidirectional => TransferDirection::Bidirectional,
        },
        status: match transfer.status {
            crate::types::TransferStatus::Queued => TransferStatus::Queued,
            crate::types::TransferStatus::Active => TransferStatus::Active,
            crate::types::TransferStatus::Paused => TransferStatus::Paused,
            crate::types::TransferStatus::Completed => TransferStatus::Completed,
            crate::types::TransferStatus::Failed => TransferStatus::Failed,
            crate::types::TransferStatus::Cancelled => TransferStatus::Cancelled,
        },
        progress_percent: transfer.progress_percent,
        speed_bps: (transfer.speed_mbps * 1_000_000.0 / 8.0) as u64,
        bytes_transferred: transfer.bytes_transferred,
        total_bytes: transfer.total_bytes,
        start_time: chrono::DateTime::from_timestamp(transfer.start_time as i64, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default(),
        eta: transfer.eta_seconds.map(|s| {
            chrono::Utc::now()
                .checked_add_signed(chrono::Duration::seconds(s as i64))
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        }),
        remote_peer: transfer.remote_peer.clone(),
        source: String::new(), // Not tracked in TransferView
        destination: String::new(),
    }
}

/// Convert internal EdgeView to IPC EdgeInfo
fn edge_to_ipc(edge: &EdgeView) -> EdgeInfo {
    EdgeInfo {
        id: edge.id.clone(),
        name: edge.id.clone(), // Use ID as name
        address: edge.address.clone(),
        status: if edge.connected {
            EdgeStatus::Connected
        } else {
            EdgeStatus::Disconnected
        },
        rtt_ms: edge.rtt_ms,
        active_transfers: edge.active_transfers,
        bytes_sent: edge.bytes_sent,
        bytes_received: edge.bytes_received,
        uptime_seconds: edge.uptime_seconds,
        last_seen: chrono::Utc::now().to_rfc3339(),
    }
}

/// Convert internal MetricsSummary to IPC MetricsSummary
fn metrics_to_ipc(
    metrics: &crate::types::MetricsSummary,
    total_edges: usize,
    uptime: u64,
) -> MetricsSummary {
    MetricsSummary {
        total_transfers: metrics.total_transfers,
        active_transfers: metrics.active_transfers,
        completed_transfers: metrics.completed_transfers,
        failed_transfers: metrics.failed_transfers,
        total_bytes_transferred: metrics.total_bytes_transferred,
        aggregate_throughput_bps: (metrics.aggregate_throughput_mbps * 1_000_000.0 / 8.0) as u64,
        connected_edges: metrics.connected_edges,
        total_edges,
        average_rtt_ms: metrics.average_rtt_ms,
        uptime_seconds: uptime,
    }
}

/// IPC command handler
pub struct IpcHandler {
    app_state: AppState,
}

impl IpcHandler {
    /// Create a new IPC handler wrapping the app state
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    /// Handle an IPC command and return a JSON response
    pub async fn handle_command(&self, command: IpcCommand) -> String {
        let result = self.execute_command(command).await;
        serde_json::to_string(&result).unwrap_or_else(|e| {
            serde_json::to_string(&IpcResponse::<()>::error(
                "SERIALIZATION_ERROR",
                e.to_string(),
            ))
            .unwrap()
        })
    }

    /// Execute an IPC command
    async fn execute_command(&self, command: IpcCommand) -> serde_json::Value {
        match command {
            // Transfer commands
            IpcCommand::GetTransfers => {
                let state = self.app_state.get_state().await;
                let transfers: Vec<TransferInfo> =
                    state.active_transfers.iter().map(transfer_to_ipc).collect();
                serde_json::to_value(IpcResponse::ok(transfers)).unwrap()
            }

            IpcCommand::GetTransfer { transfer_id } => {
                let state = self.app_state.get_state().await;
                let transfer = state
                    .active_transfers
                    .iter()
                    .chain(state.recent_transfers.iter())
                    .find(|t| t.id == transfer_id)
                    .map(transfer_to_ipc);

                match transfer {
                    Some(t) => serde_json::to_value(IpcResponse::ok(t)).unwrap(),
                    None => serde_json::to_value(IpcResponse::<TransferInfo>::error(
                        "NOT_FOUND",
                        format!("Transfer {} not found", transfer_id),
                    ))
                    .unwrap(),
                }
            }

            IpcCommand::GetRecentTransfers { limit } => {
                let state = self.app_state.get_state().await;
                let limit = limit.unwrap_or(50);
                let transfers: Vec<TransferInfo> = state
                    .recent_transfers
                    .iter()
                    .take(limit)
                    .map(transfer_to_ipc)
                    .collect();
                serde_json::to_value(IpcResponse::ok(transfers)).unwrap()
            }

            IpcCommand::PauseTransfer { transfer_id } => {
                // TODO: Implement actual pause logic
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            IpcCommand::ResumeTransfer { transfer_id } => {
                // TODO: Implement actual resume logic
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            IpcCommand::CancelTransfer { transfer_id } => {
                // TODO: Implement actual cancel logic
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            IpcCommand::StartTransfer {
                source,
                destination,
                remote_peer,
            } => {
                // TODO: Implement actual transfer start
                let result = StartTransferResult {
                    transfer_id: uuid::Uuid::new_v4().to_string(),
                };
                serde_json::to_value(IpcResponse::ok(result)).unwrap()
            }

            // Edge commands
            IpcCommand::GetEdges => {
                let state = self.app_state.get_state().await;
                let edges: Vec<EdgeInfo> = state.connected_edges.iter().map(edge_to_ipc).collect();
                serde_json::to_value(IpcResponse::ok(edges)).unwrap()
            }

            IpcCommand::GetEdge { edge_id } => {
                let state = self.app_state.get_state().await;
                let edge = state
                    .connected_edges
                    .iter()
                    .find(|e| e.id == edge_id)
                    .map(edge_to_ipc);

                match edge {
                    Some(e) => serde_json::to_value(IpcResponse::ok(e)).unwrap(),
                    None => serde_json::to_value(IpcResponse::<EdgeInfo>::error(
                        "NOT_FOUND",
                        format!("Edge {} not found", edge_id),
                    ))
                    .unwrap(),
                }
            }

            IpcCommand::ConnectEdge { address } => {
                // TODO: Implement actual edge connection
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            IpcCommand::DisconnectEdge { edge_id } => {
                // TODO: Implement actual edge disconnection
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            IpcCommand::PingEdge { edge_id } => {
                let state = self.app_state.get_state().await;
                let edge = state.connected_edges.iter().find(|e| e.id == edge_id);

                match edge {
                    Some(e) => {
                        let result = PingResult {
                            edge_id: e.id.clone(),
                            rtt_ms: e.rtt_ms,
                            reachable: e.connected,
                        };
                        serde_json::to_value(IpcResponse::ok(result)).unwrap()
                    }
                    None => serde_json::to_value(IpcResponse::<PingResult>::error(
                        "NOT_FOUND",
                        format!("Edge {} not found", edge_id),
                    ))
                    .unwrap(),
                }
            }

            // Metrics commands
            IpcCommand::GetMetrics => {
                let state = self.app_state.get_state().await;
                let metrics = metrics_to_ipc(
                    &state.metrics,
                    state.connected_edges.len(),
                    state.uptime_seconds,
                );
                serde_json::to_value(IpcResponse::ok(metrics)).unwrap()
            }

            IpcCommand::GetSchedulerMetrics => {
                // TODO: Get real scheduler metrics from warp-sched
                let scheduler = SchedulerMetrics {
                    queued_tasks: 0,
                    running_tasks: 0,
                    completed_tasks: 0,
                    load: 0.0,
                    avg_latency_us: 0,
                    peak_latency_us: 0,
                    gpu_utilization: None,
                };
                serde_json::to_value(IpcResponse::ok(scheduler)).unwrap()
            }

            IpcCommand::GetDashboardSnapshot => {
                let state = self.app_state.get_state().await;
                let snapshot = DashboardSnapshot {
                    active_transfers: state.active_transfers.iter().map(transfer_to_ipc).collect(),
                    recent_transfers: state.recent_transfers.iter().map(transfer_to_ipc).collect(),
                    edges: state.connected_edges.iter().map(edge_to_ipc).collect(),
                    metrics: metrics_to_ipc(
                        &state.metrics,
                        state.connected_edges.len(),
                        state.uptime_seconds,
                    ),
                    scheduler: SchedulerMetrics {
                        queued_tasks: 0,
                        running_tasks: 0,
                        completed_tasks: 0,
                        load: 0.0,
                        avg_latency_us: 0,
                        peak_latency_us: 0,
                        gpu_utilization: None,
                    },
                    alerts: state
                        .alerts
                        .iter()
                        .map(|a| Alert {
                            id: a.id.clone(),
                            level: match a.level {
                                crate::types::AlertLevel::Info => AlertLevel::Info,
                                crate::types::AlertLevel::Warning => AlertLevel::Warning,
                                crate::types::AlertLevel::Error => AlertLevel::Error,
                                crate::types::AlertLevel::Critical => AlertLevel::Critical,
                            },
                            message: a.message.clone(),
                            timestamp: chrono::DateTime::from_timestamp(a.timestamp as i64, 0)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_default(),
                            source: a.source.clone(),
                            acknowledged: false,
                        })
                        .collect(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                serde_json::to_value(IpcResponse::ok(snapshot)).unwrap()
            }

            // Alert commands
            IpcCommand::GetAlerts {
                min_level,
                include_acknowledged,
            } => {
                let state = self.app_state.get_state().await;
                let alerts: Vec<Alert> = state
                    .alerts
                    .iter()
                    .filter(|a| {
                        if let Some(min) = &min_level {
                            let alert_level = match a.level {
                                crate::types::AlertLevel::Info => AlertLevel::Info,
                                crate::types::AlertLevel::Warning => AlertLevel::Warning,
                                crate::types::AlertLevel::Error => AlertLevel::Error,
                                crate::types::AlertLevel::Critical => AlertLevel::Critical,
                            };
                            alert_level >= *min
                        } else {
                            true
                        }
                    })
                    .map(|a| Alert {
                        id: a.id.clone(),
                        level: match a.level {
                            crate::types::AlertLevel::Info => AlertLevel::Info,
                            crate::types::AlertLevel::Warning => AlertLevel::Warning,
                            crate::types::AlertLevel::Error => AlertLevel::Error,
                            crate::types::AlertLevel::Critical => AlertLevel::Critical,
                        },
                        message: a.message.clone(),
                        timestamp: chrono::DateTime::from_timestamp(a.timestamp as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default(),
                        source: a.source.clone(),
                        acknowledged: false,
                    })
                    .collect();
                serde_json::to_value(IpcResponse::ok(alerts)).unwrap()
            }

            IpcCommand::AcknowledgeAlert { alert_id } => {
                // TODO: Implement alert acknowledgment
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            IpcCommand::ClearAcknowledgedAlerts => {
                // TODO: Implement clearing acknowledged alerts
                serde_json::to_value(IpcResponse::<()>::ok(())).unwrap()
            }

            // Subscription commands
            IpcCommand::Subscribe { events } => {
                let result = SubscribeResult {
                    subscription_id: uuid::Uuid::new_v4().to_string(),
                    filters: events,
                };
                serde_json::to_value(IpcResponse::ok(result)).unwrap()
            }

            IpcCommand::Unsubscribe => serde_json::to_value(IpcResponse::<()>::ok(())).unwrap(),
        }
    }

    /// Create a transfer progress event
    pub fn create_transfer_progress_event(transfer: &TransferView) -> IpcEvent {
        IpcEvent::TransferProgress {
            transfer_id: transfer.id.clone(),
            bytes_transferred: transfer.bytes_transferred,
            total_bytes: transfer.total_bytes,
            speed_bps: (transfer.speed_mbps * 1_000_000.0 / 8.0) as u64,
            progress_percent: transfer.progress_percent,
            eta_seconds: transfer.eta_seconds,
        }
    }

    /// Create a metrics update event
    pub fn create_metrics_event(state: &DashboardState) -> IpcEvent {
        IpcEvent::MetricsUpdated {
            metrics: metrics_to_ipc(
                &state.metrics,
                state.connected_edges.len(),
                state.uptime_seconds,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TransferDirection as TDir, TransferStatus as TStatus};

    fn create_test_state() -> AppState {
        let state = DashboardState::new();
        AppState::new(state)
    }

    #[tokio::test]
    async fn test_get_transfers_empty() {
        let handler = IpcHandler::new(create_test_state());
        let response = handler.handle_command(IpcCommand::GetTransfers).await;
        assert!(response.contains("ok"));
        assert!(response.contains("[]") || response.contains("data\":[]"));
    }

    #[tokio::test]
    async fn test_get_transfer_not_found() {
        let handler = IpcHandler::new(create_test_state());
        let response = handler
            .handle_command(IpcCommand::GetTransfer {
                transfer_id: "nonexistent".to_string(),
            })
            .await;
        assert!(response.contains("error"));
        assert!(response.contains("NOT_FOUND"));
    }

    #[tokio::test]
    async fn test_get_edges_empty() {
        let handler = IpcHandler::new(create_test_state());
        let response = handler.handle_command(IpcCommand::GetEdges).await;
        assert!(response.contains("ok"));
    }

    #[tokio::test]
    async fn test_get_metrics() {
        let handler = IpcHandler::new(create_test_state());
        let response = handler.handle_command(IpcCommand::GetMetrics).await;
        assert!(response.contains("ok"));
        assert!(response.contains("total_transfers"));
    }

    #[tokio::test]
    async fn test_get_dashboard_snapshot() {
        let handler = IpcHandler::new(create_test_state());
        let response = handler
            .handle_command(IpcCommand::GetDashboardSnapshot)
            .await;
        assert!(response.contains("ok"));
        assert!(response.contains("active_transfers"));
        assert!(response.contains("metrics"));
    }

    #[tokio::test]
    async fn test_subscribe() {
        let handler = IpcHandler::new(create_test_state());
        let response = handler
            .handle_command(IpcCommand::Subscribe {
                events: vec![EventFilter::Transfers, EventFilter::Alerts],
            })
            .await;
        assert!(response.contains("ok"));
        assert!(response.contains("subscription_id"));
    }

    #[test]
    fn test_transfer_to_ipc() {
        let transfer = TransferView::new("t1".to_string(), "test".to_string(), TDir::Send);
        let ipc = transfer_to_ipc(&transfer);
        assert_eq!(ipc.id, "t1");
        assert_eq!(ipc.name, "test");
        assert!(matches!(ipc.direction, TransferDirection::Send));
    }

    #[test]
    fn test_edge_to_ipc() {
        let mut edge = EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string());
        edge.connected = true;
        edge.rtt_ms = 50.0;
        let ipc = edge_to_ipc(&edge);
        assert_eq!(ipc.id, "e1");
        assert_eq!(ipc.address, "127.0.0.1:8080");
        assert!(matches!(ipc.status, EdgeStatus::Connected));
        assert_eq!(ipc.rtt_ms, 50.0);
    }
}
