//! Dashboard HTTP Server
//!
//! This module provides the main DashboardServer implementation with routing,
//! static file serving, and middleware configuration.

use crate::handlers::{
    api_edges_handler, api_metrics_handler, api_state_handler, api_transfers_handler,
    edges_handler, index_handler, metrics_handler, sse_handler, transfer_detail_handler,
    transfers_handler, AppState,
};
use crate::types::DashboardState;
use crate::DashboardError;
use axum::{
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    services::ServeDir,
    trace::TraceLayer,
};

/// Dashboard server configuration
#[derive(Debug, Clone)]
pub struct DashboardConfig {
    /// HTTP server bind address
    pub bind_address: SocketAddr,
    /// Path to static assets directory (optional)
    pub assets_path: Option<PathBuf>,
    /// Enable CORS
    pub enable_cors: bool,
    /// Enable compression
    pub enable_compression: bool,
    /// Enable request tracing
    pub enable_tracing: bool,
}

impl DashboardConfig {
    /// Create a new dashboard configuration with defaults
    pub fn new(port: u16) -> Self {
        Self {
            bind_address: SocketAddr::from(([127, 0, 0, 1], port)),
            assets_path: None,
            enable_cors: true,
            enable_compression: true,
            enable_tracing: true,
        }
    }

    /// Set bind address
    pub fn with_bind_address(mut self, addr: SocketAddr) -> Self {
        self.bind_address = addr;
        self
    }

    /// Set assets path
    pub fn with_assets_path(mut self, path: PathBuf) -> Self {
        self.assets_path = Some(path);
        self
    }

    /// Enable or disable CORS
    pub fn with_cors(mut self, enable: bool) -> Self {
        self.enable_cors = enable;
        self
    }

    /// Enable or disable compression
    pub fn with_compression(mut self, enable: bool) -> Self {
        self.enable_compression = enable;
        self
    }

    /// Enable or disable tracing
    pub fn with_tracing(mut self, enable: bool) -> Self {
        self.enable_tracing = enable;
        self
    }
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self::new(3030)
    }
}

/// Dashboard HTTP server
pub struct DashboardServer {
    config: DashboardConfig,
    app_state: AppState,
}

impl DashboardServer {
    /// Create a new dashboard server
    pub fn new(config: DashboardConfig) -> Self {
        let state = DashboardState::new();
        let app_state = AppState::new(state);
        Self { config, app_state }
    }

    /// Create a new dashboard server with initial state
    pub fn with_state(config: DashboardConfig, state: DashboardState) -> Self {
        let app_state = AppState::new(state);
        Self { config, app_state }
    }

    /// Get reference to application state
    pub fn state(&self) -> &AppState {
        &self.app_state
    }

    /// Build the router with all routes and middleware
    pub fn router(&self) -> Router {
        let mut router = Router::new()
            // HTML pages
            .route("/", get(index_handler))
            .route("/transfers", get(transfers_handler))
            .route("/transfers/{id}", get(transfer_detail_handler))
            .route("/edges", get(edges_handler))
            .route("/metrics", get(metrics_handler))
            // JSON API
            .route("/api/state", get(api_state_handler))
            .route("/api/transfers", get(api_transfers_handler))
            .route("/api/metrics", get(api_metrics_handler))
            .route("/api/edges", get(api_edges_handler))
            // Server-Sent Events
            .route("/api/events", get(sse_handler))
            .with_state(self.app_state.clone());

        // Add static file serving if assets path is configured
        if let Some(assets_path) = &self.config.assets_path {
            router = router.nest_service("/assets", ServeDir::new(assets_path));
        }

        // Add compression middleware
        if self.config.enable_compression {
            router = router.layer(CompressionLayer::new());
        }

        // Add CORS middleware
        if self.config.enable_cors {
            router = router.layer(CorsLayer::permissive());
        }

        // Add tracing middleware
        if self.config.enable_tracing {
            router = router.layer(TraceLayer::new_for_http());
        }

        router
    }

    /// Start the HTTP server
    pub async fn start(&self) -> crate::Result<()> {
        let router = self.router();
        let listener = tokio::net::TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|e| DashboardError::Server(format!("Failed to bind: {}", e)))?;

        tracing::info!("Dashboard server listening on {}", self.config.bind_address);

        axum::serve(listener, router)
            .await
            .map_err(|e| DashboardError::Server(format!("Server error: {}", e)))?;

        Ok(())
    }

    /// Run the server with graceful shutdown
    pub async fn run_with_shutdown(
        &self,
        shutdown_signal: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> crate::Result<()> {
        let router = self.router();
        let listener = tokio::net::TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|e| DashboardError::Server(format!("Failed to bind: {}", e)))?;

        tracing::info!("Dashboard server listening on {}", self.config.bind_address);

        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal)
            .await
            .map_err(|e| DashboardError::Server(format!("Server error: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EdgeView, TransferDirection, TransferView};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn test_dashboard_config_new() {
        let config = DashboardConfig::new(8080);
        assert_eq!(config.bind_address.port(), 8080);
        assert!(config.enable_cors);
        assert!(config.enable_compression);
        assert!(config.enable_tracing);
    }

    #[test]
    fn test_dashboard_config_default() {
        let config = DashboardConfig::default();
        assert_eq!(config.bind_address.port(), 3030);
    }

    #[test]
    fn test_dashboard_config_builder() {
        let config = DashboardConfig::new(8080)
            .with_bind_address(SocketAddr::from(([0, 0, 0, 0], 9000)))
            .with_cors(false)
            .with_compression(false)
            .with_tracing(false);

        assert_eq!(config.bind_address.port(), 9000);
        assert!(!config.enable_cors);
        assert!(!config.enable_compression);
        assert!(!config.enable_tracing);
    }

    #[test]
    fn test_dashboard_config_with_assets_path() {
        let path = PathBuf::from("/tmp/assets");
        let config = DashboardConfig::new(8080).with_assets_path(path.clone());
        assert_eq!(config.assets_path, Some(path));
    }

    #[test]
    fn test_dashboard_server_new() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        assert!(server.state().state.try_read().is_ok());
    }

    #[test]
    fn test_dashboard_server_with_state() {
        let mut state = DashboardState::new();
        state.uptime_seconds = 100;

        let config = DashboardConfig::new(8080);
        let server = DashboardServer::with_state(config, state);

        // Verify state was set
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let retrieved = server.state().get_state().await;
            assert_eq!(retrieved.uptime_seconds, 100);
        });
    }

    #[tokio::test]
    async fn test_router_index_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_transfers_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/transfers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_edges_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/edges")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_metrics_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_api_state_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_api_transfers_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/transfers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_api_metrics_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_api_edges_route() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/edges")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_transfer_detail_not_found() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/transfers/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_server_state_access() {
        let config = DashboardConfig::new(8080);
        let server = DashboardServer::new(config);

        // Add a transfer to state
        server
            .state()
            .update_state(|state| {
                let transfer = TransferView::new(
                    "t1".to_string(),
                    "test-transfer".to_string(),
                    TransferDirection::Send,
                );
                state.add_active_transfer(transfer);
            })
            .await;

        // Verify through API
        let app = server.router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/transfers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
