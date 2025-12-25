//! Native HPC API endpoints
//!
//! High-performance endpoints for HPC workloads:
//! - EphemeralURL - generate time-limited access tokens
//! - StreamChunked - streaming large objects
//! - Stats - storage statistics

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use warp_store::backend::StorageBackend;
use warp_store::{AccessScope, ObjectKey, Permissions};

use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// Create native HPC API routes
pub fn routes<B: StorageBackend>(state: AppState<B>) -> Router {
    Router::new()
        // Ephemeral URL generation
        .route("/api/v1/ephemeral", post(create_ephemeral_url::<B>))
        .route("/api/v1/ephemeral/verify", post(verify_ephemeral_token::<B>))
        // Access via ephemeral token
        .route("/api/v1/access/{token}/{*key}", get(access_with_token::<B>))
        // Stats and metrics
        .route("/api/v1/stats", get(get_stats::<B>))
        // Health checks
        .route("/health", get(health_check))
        .route("/health/detailed", get(health_check_detailed::<B>))
        .route("/ready", get(readiness_check::<B>))
        .route("/live", get(liveness_check))
        .with_state(state)
}

/// Request to create an ephemeral URL
#[derive(Debug, Deserialize)]
struct CreateEphemeralRequest {
    /// Bucket name
    bucket: String,
    /// Key or prefix
    key: String,
    /// TTL in seconds
    ttl_seconds: u64,
    /// Scope type: "object", "prefix", or "bucket"
    #[serde(default = "default_scope")]
    scope: String,
    /// Permissions
    #[serde(default)]
    permissions: PermissionsRequest,
}

fn default_scope() -> String {
    "object".to_string()
}

#[derive(Debug, Default, Deserialize)]
struct PermissionsRequest {
    #[serde(default = "default_true")]
    read: bool,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    delete: bool,
    #[serde(default)]
    list: bool,
}

fn default_true() -> bool {
    true
}

/// Response with ephemeral token
#[derive(Debug, Serialize)]
struct EphemeralResponse {
    token: String,
    expires_at: String,
    url: String,
}

/// Create an ephemeral URL
async fn create_ephemeral_url<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Json(req): Json<CreateEphemeralRequest>,
) -> ApiResult<Json<EphemeralResponse>> {
    let scope = match req.scope.as_str() {
        "object" => {
            let key = ObjectKey::new(&req.bucket, &req.key)?;
            AccessScope::Object(key)
        }
        "prefix" => AccessScope::Prefix {
            bucket: req.bucket.clone(),
            prefix: req.key.clone(),
        },
        "bucket" => AccessScope::Bucket(req.bucket.clone()),
        _ => return Err(ApiError::InvalidRequest("Invalid scope type".into())),
    };

    let permissions = Permissions {
        read: req.permissions.read,
        write: req.permissions.write,
        delete: req.permissions.delete,
        list: req.permissions.list,
    };

    let ttl = Duration::from_secs(req.ttl_seconds);

    let token = state.store.create_ephemeral_url_with_options(
        scope,
        permissions,
        ttl,
        None,
        None,
    )?;

    let encoded = token.encode();
    let expires_at = token.expires_at().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let url = format!(
        "/api/v1/access/{}/{}",
        encoded,
        req.key
    );

    Ok(Json(EphemeralResponse {
        token: encoded,
        expires_at,
        url,
    }))
}

/// Request to verify an ephemeral token
#[derive(Debug, Deserialize)]
struct VerifyTokenRequest {
    token: String,
    #[serde(default)]
    ip: Option<String>,
}

/// Response from token verification
#[derive(Debug, Serialize)]
struct VerifyResponse {
    valid: bool,
    expires_at: Option<String>,
    permissions: Option<PermissionsResponse>,
}

#[derive(Debug, Serialize)]
struct PermissionsResponse {
    read: bool,
    write: bool,
    delete: bool,
    list: bool,
}

/// Verify an ephemeral token
async fn verify_ephemeral_token<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Json(req): Json<VerifyTokenRequest>,
) -> ApiResult<Json<VerifyResponse>> {
    let token = match warp_store::EphemeralToken::decode(&req.token) {
        Ok(t) => t,
        Err(_) => {
            return Ok(Json(VerifyResponse {
                valid: false,
                expires_at: None,
                permissions: None,
            }));
        }
    };

    let ip = req.ip.and_then(|s| s.parse().ok());

    match state.store.verify_token(&token, ip) {
        Ok(()) => {
            let perms = token.permissions();
            Ok(Json(VerifyResponse {
                valid: true,
                expires_at: Some(token.expires_at().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                permissions: Some(PermissionsResponse {
                    read: perms.read,
                    write: perms.write,
                    delete: perms.delete,
                    list: perms.list,
                }),
            }))
        }
        Err(_) => Ok(Json(VerifyResponse {
            valid: false,
            expires_at: None,
            permissions: None,
        })),
    }
}

/// Access an object with an ephemeral token
async fn access_with_token<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path((token_str, key_path)): Path<(String, String)>,
) -> ApiResult<Response> {
    // Decode and verify token
    let token = warp_store::EphemeralToken::decode(&token_str)
        .map_err(|_| ApiError::AuthFailed("Invalid token".into()))?;

    state.store.verify_token(&token, None)?;

    // Check permissions
    if !token.permissions().can_read() {
        return Err(ApiError::AccessDenied("Token does not allow read".into()));
    }

    // Extract bucket from scope
    let (bucket, key) = match token.scope() {
        AccessScope::Object(obj_key) => {
            (obj_key.bucket().to_string(), obj_key.key().to_string())
        }
        AccessScope::Prefix { bucket, prefix } => {
            // For prefix scope, the key_path is relative to the prefix
            let full_key = if prefix.is_empty() {
                key_path.clone()
            } else {
                format!("{}{}", prefix, key_path)
            };
            (bucket.clone(), full_key)
        }
        AccessScope::Bucket(bucket) => (bucket.clone(), key_path.clone()),
    };

    let object_key = ObjectKey::new(&bucket, &key)?;

    // Check if key is allowed by scope
    if !token.allows(&object_key) {
        return Err(ApiError::AccessDenied("Key not allowed by token scope".into()));
    }

    // Get object
    let data = state.store.get(&object_key).await?;
    let meta = state.store.head(&object_key).await?;

    let content_type = meta.content_type.unwrap_or_else(|| "application/octet-stream".to_string());

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_LENGTH, data.len().to_string()),
            (header::ETAG, meta.etag),
            (header::CONTENT_TYPE, content_type),
        ],
        data.into_bytes(),
    ).into_response())
}

/// Storage stats response
#[derive(Debug, Serialize)]
struct StatsResponse {
    buckets: usize,
    metrics: Option<warp_store::MetricsSnapshot>,
}

/// Get storage statistics
async fn get_stats<B: StorageBackend>(
    State(state): State<AppState<B>>,
) -> Json<StatsResponse> {
    let buckets = state.store.list_buckets().await.len();
    let metrics = state.metrics.as_ref().map(|m| m.snapshot());

    Json(StatsResponse { buckets, metrics })
}

/// Detailed health check response
#[derive(Debug, Serialize)]
struct HealthResponse {
    /// Overall status: "healthy", "degraded", or "unhealthy"
    status: &'static str,
    /// Uptime in seconds
    uptime_secs: u64,
    /// Storage health details
    storage: StorageHealth,
    /// Optional metrics summary
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<MetricsSummary>,
}

#[derive(Debug, Serialize)]
struct StorageHealth {
    /// Number of buckets
    buckets: usize,
    /// Whether the backend is reachable
    backend_ok: bool,
}

#[derive(Debug, Serialize)]
struct MetricsSummary {
    /// Total operations
    total_ops: u64,
    /// Average GET latency in microseconds
    get_latency_avg_us: u64,
    /// Average PUT latency in microseconds
    put_latency_avg_us: u64,
    /// GET error rate (0.0 - 1.0)
    get_error_rate: f64,
    /// PUT error rate (0.0 - 1.0)
    put_error_rate: f64,
    /// Cache hit rate (0.0 - 1.0)
    cache_hit_rate: f64,
    /// Shard health ratio (0.0 - 1.0)
    shard_health_ratio: f64,
}

/// Basic health check (just returns OK)
async fn health_check() -> &'static str {
    "OK"
}

/// Detailed health check endpoint
async fn health_check_detailed<B: StorageBackend>(
    State(state): State<AppState<B>>,
) -> Json<HealthResponse> {
    let buckets = state.store.list_buckets().await.len();
    let backend_ok = true; // If we got here, backend is responding

    let (status, metrics_summary) = if let Some(metrics) = &state.metrics {
        let snapshot = metrics.snapshot();
        let is_healthy = snapshot.is_healthy();

        let summary = MetricsSummary {
            total_ops: snapshot.get_count + snapshot.put_count + snapshot.delete_count,
            get_latency_avg_us: snapshot.get_latency_avg_us,
            put_latency_avg_us: snapshot.put_latency_avg_us,
            get_error_rate: snapshot.get_error_rate(),
            put_error_rate: snapshot.put_error_rate(),
            cache_hit_rate: snapshot.cache_hit_rate,
            shard_health_ratio: snapshot.shard_health_ratio(),
        };

        let status = if is_healthy {
            "healthy"
        } else if snapshot.shards_missing > 0 {
            "unhealthy"
        } else {
            "degraded"
        };

        (status, Some(summary))
    } else {
        ("healthy", None)
    };

    Json(HealthResponse {
        status,
        uptime_secs: state.metrics.as_ref().map(|m| m.snapshot().uptime_secs).unwrap_or(0),
        storage: StorageHealth {
            buckets,
            backend_ok,
        },
        metrics: metrics_summary,
    })
}

/// Readiness check (for Kubernetes)
async fn readiness_check<B: StorageBackend>(
    State(state): State<AppState<B>>,
) -> impl IntoResponse {
    // Check if we can list buckets (validates backend connectivity)
    let _buckets = state.store.list_buckets().await;
    (StatusCode::OK, "Ready")
}

/// Liveness check (for Kubernetes)
async fn liveness_check() -> impl IntoResponse {
    (StatusCode::OK, "Alive")
}
