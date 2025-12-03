//! HTML Template Rendering
//!
//! This module provides Askama templates for rendering the dashboard UI.
//! All templates are type-safe and compiled at build time.

use crate::types::{DashboardState, EdgeView, MetricsSummary, TransferView};
use askama::Template;

/// Version string for the dashboard
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Main dashboard index template
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    /// Current dashboard state
    pub state: DashboardState,
    /// Dashboard version
    pub version: &'static str,
    /// System uptime formatted string
    pub uptime: String,
}

impl IndexTemplate {
    /// Create a new index template
    pub fn new(mut state: DashboardState) -> Self {
        // Limit recent transfers to 10 for display
        if state.recent_transfers.len() > 10 {
            state.recent_transfers.truncate(10);
        }
        let uptime = format_uptime(state.uptime_seconds);
        Self {
            state,
            version: VERSION,
            uptime,
        }
    }
}

/// Transfers list template
#[derive(Template)]
#[template(path = "transfers.html")]
pub struct TransfersTemplate {
    /// List of all transfers
    pub transfers: Vec<TransferView>,
    /// Count of active transfers
    pub active_count: usize,
    /// Count of completed transfers
    pub completed_count: usize,
    /// Count of failed transfers
    pub failed_count: usize,
    /// Dashboard version
    pub version: &'static str,
    /// System uptime formatted string
    pub uptime: String,
}

impl TransfersTemplate {
    /// Create a new transfers template
    pub fn new(transfers: Vec<TransferView>, uptime_seconds: u64) -> Self {
        let active_count = transfers
            .iter()
            .filter(|t| t.is_active())
            .count();
        let completed_count = transfers
            .iter()
            .filter(|t| t.is_complete())
            .count();
        let failed_count = transfers
            .iter()
            .filter(|t| matches!(t.status, crate::types::TransferStatus::Failed))
            .count();
        let uptime = format_uptime(uptime_seconds);

        Self {
            transfers,
            active_count,
            completed_count,
            failed_count,
            version: VERSION,
            uptime,
        }
    }
}

/// Transfer detail template
#[derive(Template)]
#[template(path = "transfer_detail.html")]
pub struct TransferDetailTemplate {
    /// Transfer details
    pub transfer: TransferView,
    /// Dashboard version
    pub version: &'static str,
    /// System uptime formatted string
    pub uptime: String,
}

impl TransferDetailTemplate {
    /// Create a new transfer detail template
    pub fn new(transfer: TransferView, uptime_seconds: u64) -> Self {
        let uptime = format_uptime(uptime_seconds);
        Self {
            transfer,
            version: VERSION,
            uptime,
        }
    }
}

/// Edge nodes template
#[derive(Template)]
#[template(path = "edges.html")]
pub struct EdgesTemplate {
    /// List of edge connections
    pub edges: Vec<EdgeView>,
    /// Count of connected edges
    pub connected_count: usize,
    /// Average RTT across all edges
    pub average_rtt: f64,
    /// Dashboard version
    pub version: &'static str,
    /// System uptime formatted string
    pub uptime: String,
}

impl EdgesTemplate {
    /// Create a new edges template
    pub fn new(edges: Vec<EdgeView>, uptime_seconds: u64) -> Self {
        let connected_count = edges.iter().filter(|e| e.connected).count();
        let average_rtt = if !edges.is_empty() {
            edges.iter().map(|e| e.rtt_ms).sum::<f64>() / edges.len() as f64
        } else {
            0.0
        };
        let uptime = format_uptime(uptime_seconds);

        Self {
            edges,
            connected_count,
            average_rtt,
            version: VERSION,
            uptime,
        }
    }
}

/// Metrics template
#[derive(Template)]
#[template(path = "metrics.html")]
pub struct MetricsTemplate {
    /// Metrics summary
    pub metrics: MetricsSummary,
    /// Dashboard version
    pub version: &'static str,
    /// System uptime formatted string
    pub uptime: String,
}

impl MetricsTemplate {
    /// Create a new metrics template
    pub fn new(metrics: MetricsSummary, uptime_seconds: u64) -> Self {
        let uptime = format_uptime(uptime_seconds);
        Self {
            metrics,
            version: VERSION,
            uptime,
        }
    }
}

/// Format uptime seconds into human-readable string
fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, minutes, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Alert, AlertLevel, TransferDirection, TransferStatus};

    #[test]
    fn test_format_uptime_seconds() {
        assert_eq!(format_uptime(45), "45s");
    }

    #[test]
    fn test_format_uptime_minutes() {
        assert_eq!(format_uptime(125), "2m 5s");
    }

    #[test]
    fn test_format_uptime_hours() {
        assert_eq!(format_uptime(3665), "1h 1m 5s");
    }

    #[test]
    fn test_format_uptime_days() {
        assert_eq!(format_uptime(90061), "1d 1h 1m 1s");
    }

    #[test]
    fn test_index_template_new() {
        let state = DashboardState::new();
        let template = IndexTemplate::new(state);
        assert_eq!(template.version, VERSION);
        assert!(!template.uptime.is_empty());
    }

    #[test]
    fn test_index_template_render_empty() {
        let state = DashboardState::new();
        let template = IndexTemplate::new(state);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Warp Dashboard"));
        assert!(rendered.contains("Active Transfers"));
    }

    #[test]
    fn test_index_template_render_with_transfers() {
        let mut state = DashboardState::new();
        let mut transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        transfer.status = TransferStatus::Active;
        transfer.total_bytes = 1000;
        transfer.bytes_transferred = 500;
        transfer.calculate_progress();
        state.add_active_transfer(transfer);

        let template = IndexTemplate::new(state);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("test-transfer"));
        assert!(rendered.contains("50%"));
    }

    #[test]
    fn test_index_template_render_with_alerts() {
        let mut state = DashboardState::new();
        let alert = Alert::new(AlertLevel::Warning, "Test warning".to_string());
        state.add_alert(alert);

        let template = IndexTemplate::new(state);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Test warning"));
        assert!(rendered.contains("warning"));
    }

    #[test]
    fn test_transfers_template_new() {
        let transfers = vec![
            TransferView::new("t1".to_string(), "test1".to_string(), TransferDirection::Send),
        ];
        let template = TransfersTemplate::new(transfers, 100);
        assert_eq!(template.transfers.len(), 1);
        assert_eq!(template.version, VERSION);
    }

    #[test]
    fn test_transfers_template_counts() {
        let mut transfers = vec![];

        let mut t1 = TransferView::new(
            "t1".to_string(),
            "test1".to_string(),
            TransferDirection::Send,
        );
        t1.status = TransferStatus::Active;
        transfers.push(t1);

        let mut t2 = TransferView::new(
            "t2".to_string(),
            "test2".to_string(),
            TransferDirection::Receive,
        );
        t2.status = TransferStatus::Completed;
        transfers.push(t2);

        let mut t3 = TransferView::new(
            "t3".to_string(),
            "test3".to_string(),
            TransferDirection::Send,
        );
        t3.status = TransferStatus::Failed;
        transfers.push(t3);

        let template = TransfersTemplate::new(transfers, 100);
        assert_eq!(template.active_count, 1);
        assert_eq!(template.completed_count, 2); // Both Completed and Failed are considered complete
        assert_eq!(template.failed_count, 1);
    }

    #[test]
    fn test_transfers_template_render() {
        let transfers = vec![
            TransferView::new("t1".to_string(), "test1".to_string(), TransferDirection::Send),
        ];
        let template = TransfersTemplate::new(transfers, 100);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("All Transfers"));
        assert!(rendered.contains("test1"));
    }

    #[test]
    fn test_transfer_detail_template_new() {
        let transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        let template = TransferDetailTemplate::new(transfer, 100);
        assert_eq!(template.transfer.name, "test-transfer");
        assert_eq!(template.version, VERSION);
    }

    #[test]
    fn test_transfer_detail_template_render() {
        let transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        let template = TransferDetailTemplate::new(transfer, 100);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Transfer Details"));
        assert!(rendered.contains("test-transfer"));
        assert!(rendered.contains("t1"));
    }

    #[test]
    fn test_edges_template_new() {
        let edges = vec![
            EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string()),
        ];
        let template = EdgesTemplate::new(edges, 100);
        assert_eq!(template.edges.len(), 1);
        assert_eq!(template.version, VERSION);
    }

    #[test]
    fn test_edges_template_counts() {
        let mut edges = vec![];

        let mut e1 = EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string());
        e1.connected = true;
        e1.rtt_ms = 50.0;
        edges.push(e1);

        let mut e2 = EdgeView::new("e2".to_string(), "127.0.0.1:8081".to_string());
        e2.connected = false;
        e2.rtt_ms = 100.0;
        edges.push(e2);

        let template = EdgesTemplate::new(edges, 100);
        assert_eq!(template.connected_count, 1);
        assert_eq!(template.average_rtt, 75.0);
    }

    #[test]
    fn test_edges_template_render() {
        let edges = vec![
            EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string()),
        ];
        let template = EdgesTemplate::new(edges, 100);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Edge Connections"));
        assert!(rendered.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_metrics_template_new() {
        let metrics = MetricsSummary::new();
        let template = MetricsTemplate::new(metrics, 100);
        assert_eq!(template.version, VERSION);
    }

    #[test]
    fn test_metrics_template_render() {
        let mut metrics = MetricsSummary::new();
        metrics.total_transfers = 10;
        metrics.active_transfers = 3;
        metrics.completed_transfers = 6;
        metrics.failed_transfers = 1;

        let template = MetricsTemplate::new(metrics, 100);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Transfer Metrics"));
        assert!(rendered.contains("10"));
        assert!(rendered.contains("3"));
        assert!(rendered.contains("6"));
        assert!(rendered.contains("1"));
    }

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0s");
    }

    #[test]
    fn test_format_uptime_exact_minute() {
        assert_eq!(format_uptime(60), "1m 0s");
    }

    #[test]
    fn test_format_uptime_exact_hour() {
        assert_eq!(format_uptime(3600), "1h 0m 0s");
    }

    #[test]
    fn test_format_uptime_exact_day() {
        assert_eq!(format_uptime(86400), "1d 0h 0m 0s");
    }
}
