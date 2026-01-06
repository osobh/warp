//! HTTP Request Handlers
//!
//! This module provides Axum HTTP handlers for the dashboard routes,
//! including HTML pages, JSON APIs, and Server-Sent Events for live updates.

use crate::templates::{
    EdgesTemplate, IndexTemplate, MetricsTemplate, TransferDetailTemplate, TransfersTemplate,
};
use crate::types::{DashboardState, EdgeView, MetricsSummary, TransferView};
use askama::Template;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response, Sse, sse::Event},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Dashboard state
    pub state: Arc<RwLock<DashboardState>>,
    /// Broadcast channel for state updates
    pub update_tx: tokio::sync::broadcast::Sender<()>,
}

impl AppState {
    /// Create a new application state
    pub fn new(state: DashboardState) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            state: Arc::new(RwLock::new(state)),
            update_tx: tx,
        }
    }

    /// Get a read lock on the state
    pub async fn get_state(&self) -> DashboardState {
        self.state.read().await.clone()
    }

    /// Update the dashboard state
    pub async fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut DashboardState),
    {
        let mut state = self.state.write().await;
        f(&mut state);
        let _ = self.update_tx.send(());
    }

    /// Notify listeners of state change without modifying state
    pub fn notify(&self) {
        let _ = self.update_tx.send(());
    }
}

/// Handler for the main dashboard page
pub async fn index_handler(State(app_state): State<AppState>) -> Result<Html<String>, Response> {
    let state = app_state.get_state().await;
    let template = IndexTemplate::new(state);

    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response()),
    }
}

/// Handler for the transfers list page
pub async fn transfers_handler(
    State(app_state): State<AppState>,
) -> Result<Html<String>, Response> {
    let state = app_state.get_state().await;
    let mut transfers = state.active_transfers.clone();
    transfers.extend(state.recent_transfers.clone());

    let template = TransfersTemplate::new(transfers, state.uptime_seconds);

    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response()),
    }
}

/// Handler for transfer detail page
pub async fn transfer_detail_handler(
    State(app_state): State<AppState>,
    Path(transfer_id): Path<String>,
) -> Result<Html<String>, Response> {
    let state = app_state.get_state().await;

    // Search in active transfers
    let transfer = state
        .active_transfers
        .iter()
        .find(|t| t.id == transfer_id)
        .or_else(|| state.recent_transfers.iter().find(|t| t.id == transfer_id))
        .cloned();

    match transfer {
        Some(transfer) => {
            let template = TransferDetailTemplate::new(transfer, state.uptime_seconds);
            match template.render() {
                Ok(html) => Ok(Html(html)),
                Err(e) => Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Template error: {}", e),
                )
                    .into_response()),
            }
        }
        None => Err((StatusCode::NOT_FOUND, "Transfer not found").into_response()),
    }
}

/// Handler for edges page
pub async fn edges_handler(State(app_state): State<AppState>) -> Result<Html<String>, Response> {
    let state = app_state.get_state().await;
    let template = EdgesTemplate::new(state.connected_edges.clone(), state.uptime_seconds);

    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response()),
    }
}

/// Handler for metrics page
pub async fn metrics_handler(State(app_state): State<AppState>) -> Result<Html<String>, Response> {
    let state = app_state.get_state().await;
    let template = MetricsTemplate::new(state.metrics.clone(), state.uptime_seconds);

    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response()),
    }
}

/// API handler for JSON state
pub async fn api_state_handler(State(app_state): State<AppState>) -> Json<DashboardState> {
    let state = app_state.get_state().await;
    Json(state)
}

/// API handler for JSON transfers list
pub async fn api_transfers_handler(State(app_state): State<AppState>) -> Json<Vec<TransferView>> {
    let state = app_state.get_state().await;
    let mut transfers = state.active_transfers.clone();
    transfers.extend(state.recent_transfers.clone());
    Json(transfers)
}

/// API handler for JSON metrics
pub async fn api_metrics_handler(State(app_state): State<AppState>) -> Json<MetricsSummary> {
    let state = app_state.get_state().await;
    Json(state.metrics)
}

/// API handler for JSON edges list
pub async fn api_edges_handler(State(app_state): State<AppState>) -> Json<Vec<EdgeView>> {
    let state = app_state.get_state().await;
    Json(state.connected_edges)
}

/// Server-Sent Events handler for live updates
pub async fn sse_handler(
    State(app_state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = app_state.update_tx.subscribe();
    let stream = BroadcastStream::new(rx)
        .map(|_| Ok(Event::default().event("update").data("State updated")));

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(1))
            .text("keep-alive"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Alert, AlertLevel, TransferDirection, TransferStatus};

    fn create_test_state() -> AppState {
        let state = DashboardState::new();
        AppState::new(state)
    }

    #[tokio::test]
    async fn test_app_state_new() {
        let state = DashboardState::new();
        let app_state = AppState::new(state);
        let retrieved = app_state.get_state().await;
        assert_eq!(retrieved.active_transfers.len(), 0);
    }

    #[tokio::test]
    async fn test_app_state_get_state() {
        let mut state = DashboardState::new();
        state.uptime_seconds = 100;
        let app_state = AppState::new(state);
        let retrieved = app_state.get_state().await;
        assert_eq!(retrieved.uptime_seconds, 100);
    }

    #[tokio::test]
    async fn test_app_state_update_state() {
        let app_state = create_test_state();
        app_state
            .update_state(|state| {
                state.uptime_seconds = 200;
            })
            .await;
        let retrieved = app_state.get_state().await;
        assert_eq!(retrieved.uptime_seconds, 200);
    }

    #[tokio::test]
    async fn test_index_handler() {
        let app_state = create_test_state();
        let result = index_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("Warp Dashboard"));
        assert!(html.contains("Active Transfers"));
    }

    #[tokio::test]
    async fn test_index_handler_with_data() {
        let mut state = DashboardState::new();
        let mut transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        transfer.status = TransferStatus::Active;
        state.add_active_transfer(transfer);

        let app_state = AppState::new(state);
        let result = index_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("test-transfer"));
    }

    #[tokio::test]
    async fn test_transfers_handler() {
        let app_state = create_test_state();
        let result = transfers_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("All Transfers"));
    }

    #[tokio::test]
    async fn test_transfers_handler_with_data() {
        let mut state = DashboardState::new();
        let transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        state.add_active_transfer(transfer);

        let app_state = AppState::new(state);
        let result = transfers_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("test-transfer"));
    }

    #[tokio::test]
    async fn test_transfer_detail_handler_found() {
        let mut state = DashboardState::new();
        let transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        state.add_active_transfer(transfer);

        let app_state = AppState::new(state);
        let result = transfer_detail_handler(State(app_state), Path("t1".to_string())).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("test-transfer"));
        assert!(html.contains("Transfer Details"));
    }

    #[tokio::test]
    async fn test_transfer_detail_handler_not_found() {
        let app_state = create_test_state();
        let result =
            transfer_detail_handler(State(app_state), Path("nonexistent".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_transfer_detail_handler_recent_transfer() {
        let mut state = DashboardState::new();
        let mut transfer = TransferView::new(
            "t1".to_string(),
            "completed-transfer".to_string(),
            TransferDirection::Send,
        );
        transfer.status = TransferStatus::Active;
        state.add_active_transfer(transfer);
        state.complete_transfer("t1");

        let app_state = AppState::new(state);
        let result = transfer_detail_handler(State(app_state), Path("t1".to_string())).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("completed-transfer"));
    }

    #[tokio::test]
    async fn test_edges_handler() {
        let app_state = create_test_state();
        let result = edges_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("Edge Connections"));
    }

    #[tokio::test]
    async fn test_edges_handler_with_data() {
        let mut state = DashboardState::new();
        let edge = EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string());
        state.add_edge(edge);

        let app_state = AppState::new(state);
        let result = edges_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("127.0.0.1:8080"));
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        let app_state = create_test_state();
        let result = metrics_handler(State(app_state)).await;
        assert!(result.is_ok());
        let html = result.unwrap().0;
        assert!(html.contains("Transfer Metrics"));
    }

    #[tokio::test]
    async fn test_api_state_handler() {
        let mut state = DashboardState::new();
        state.uptime_seconds = 123;
        let app_state = AppState::new(state);

        let Json(result) = api_state_handler(State(app_state)).await;
        assert_eq!(result.uptime_seconds, 123);
    }

    #[tokio::test]
    async fn test_api_transfers_handler() {
        let mut state = DashboardState::new();
        let transfer = TransferView::new(
            "t1".to_string(),
            "test-transfer".to_string(),
            TransferDirection::Send,
        );
        state.add_active_transfer(transfer);

        let app_state = AppState::new(state);
        let Json(result) = api_transfers_handler(State(app_state)).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "test-transfer");
    }

    #[tokio::test]
    async fn test_api_metrics_handler() {
        let mut state = DashboardState::new();
        state.metrics.total_transfers = 42;
        let app_state = AppState::new(state);

        let Json(result) = api_metrics_handler(State(app_state)).await;
        assert_eq!(result.total_transfers, 42);
    }

    #[tokio::test]
    async fn test_api_edges_handler() {
        let mut state = DashboardState::new();
        let edge = EdgeView::new("e1".to_string(), "127.0.0.1:8080".to_string());
        state.add_edge(edge);

        let app_state = AppState::new(state);
        let Json(result) = api_edges_handler(State(app_state)).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].address, "127.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_app_state_notify() {
        let app_state = create_test_state();
        let mut rx = app_state.update_tx.subscribe();

        app_state.notify();

        // Should receive notification
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(result.is_ok());
    }
}
