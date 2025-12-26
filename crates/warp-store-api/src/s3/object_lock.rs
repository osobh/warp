//! S3-compatible Object Lock API
//!
//! Implements:
//! - PUT /{bucket}?object-lock - Enable Object Lock on bucket (at creation only)
//! - GET /{bucket}?object-lock - Get Object Lock configuration
//! - PUT /{bucket}/{key}?retention - Set object retention
//! - GET /{bucket}/{key}?retention - Get object retention
//! - PUT /{bucket}/{key}?legal-hold - Set legal hold
//! - GET /{bucket}/{key}?legal-hold - Get legal hold status
//!
//! # S3 Compatibility
//!
//! Object Lock must be enabled when creating a bucket (cannot be enabled later).
//! Once enabled, Object Lock cannot be disabled.
//!
//! ## Retention Modes
//!
//! - **Governance**: Can be overridden by users with `s3:BypassGovernanceRetention` permission
//! - **Compliance**: Cannot be overridden by anyone, including root account

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use warp_store::backend::StorageBackend;
use warp_store::object_lock::{
    LegalHoldStatus, ObjectLockConfig, ObjectRetention, RetentionMode,
};
use warp_store::version::VersionId;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

// =============================================================================
// XML Types for S3 Object Lock API
// =============================================================================

/// Object Lock configuration (S3 XML format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "ObjectLockConfiguration")]
pub struct ObjectLockConfigurationXml {
    #[serde(rename = "ObjectLockEnabled")]
    pub object_lock_enabled: String,

    #[serde(rename = "Rule", skip_serializing_if = "Option::is_none")]
    pub rule: Option<ObjectLockRuleXml>,
}

/// Object Lock rule with default retention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectLockRuleXml {
    #[serde(rename = "DefaultRetention")]
    pub default_retention: DefaultRetentionXml,
}

/// Default retention settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultRetentionXml {
    #[serde(rename = "Mode")]
    pub mode: String,

    #[serde(rename = "Days", skip_serializing_if = "Option::is_none")]
    pub days: Option<u32>,

    #[serde(rename = "Years", skip_serializing_if = "Option::is_none")]
    pub years: Option<u32>,
}

/// Object retention (S3 XML format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Retention")]
pub struct RetentionXml {
    #[serde(rename = "Mode")]
    pub mode: String,

    #[serde(rename = "RetainUntilDate")]
    pub retain_until_date: String,
}

/// Legal hold (S3 XML format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "LegalHold")]
pub struct LegalHoldXml {
    #[serde(rename = "Status")]
    pub status: String,
}

/// Query parameters for retention/legal-hold operations
#[derive(Debug, Deserialize, Default)]
pub struct ObjectLockQuery {
    /// Version ID for versioned objects
    #[serde(rename = "versionId")]
    pub version_id: Option<String>,
}

// =============================================================================
// Bucket-level Object Lock Handlers
// =============================================================================

/// Get Object Lock configuration for a bucket
///
/// GET /{bucket}?object-lock
pub async fn get_object_lock_config<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Get Object Lock config
    let config = state
        .object_lock_manager
        .as_ref()
        .and_then(|m| m.get_bucket_config(&bucket));

    match config {
        Some(config) => {
            let xml_config = to_object_lock_xml(&config);
            let xml = quick_xml::se::to_string(&xml_config)
                .map_err(|e| ApiError::Internal(format!("XML serialization error: {}", e)))?;

            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/xml")],
                format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml),
            )
                .into_response())
        }
        None => {
            // Object Lock not enabled
            let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ObjectLockConfiguration>
    <ObjectLockEnabled>Disabled</ObjectLockEnabled>
</ObjectLockConfiguration>"#;
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/xml")],
                xml,
            )
                .into_response())
        }
    }
}

/// Enable Object Lock on a bucket
///
/// PUT /{bucket}?object-lock
///
/// Note: In S3, Object Lock can only be enabled at bucket creation.
/// This endpoint allows enabling it after creation for flexibility.
pub async fn put_object_lock_config<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Parse XML body
    let xml_str = std::str::from_utf8(&body)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

    let xml_config: ObjectLockConfigurationXml = quick_xml::de::from_str(xml_str)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid XML: {}", e)))?;

    // Convert to internal config
    let config = from_object_lock_xml(&xml_config)?;

    // Enable Object Lock
    if let Some(manager) = &state.object_lock_manager {
        manager
            .enable_bucket_lock(&bucket, config)
            .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    } else {
        return Err(ApiError::Internal(
            "Object Lock manager not configured".to_string(),
        ));
    }

    tracing::info!(bucket = %bucket, "Object Lock enabled");

    Ok(StatusCode::OK.into_response())
}

// =============================================================================
// Object-level Retention Handlers
// =============================================================================

/// Get retention settings for an object
///
/// GET /{bucket}/{key}?retention
pub async fn get_retention<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<ObjectLockQuery>,
) -> ApiResult<Response> {
    let version_id = query
        .version_id
        .map(|v| VersionId::from_string(&v))
        .unwrap_or_else(VersionId::null);

    let retention = state
        .object_lock_manager
        .as_ref()
        .and_then(|m| m.get_retention(&bucket, &key, &version_id));

    match retention {
        Some(retention) => {
            let xml = RetentionXml {
                mode: retention.mode.to_string(),
                retain_until_date: retention.retain_until_date.to_rfc3339(),
            };

            let xml_str = quick_xml::se::to_string(&xml)
                .map_err(|e| ApiError::Internal(format!("XML serialization error: {}", e)))?;

            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/xml")],
                format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml_str),
            )
                .into_response())
        }
        None => {
            let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
    <Code>NoSuchObjectLockConfiguration</Code>
    <Message>The specified object does not have a retention configuration</Message>
</Error>"#;
            Ok((
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "application/xml")],
                xml,
            )
                .into_response())
        }
    }
}

/// Set retention settings for an object
///
/// PUT /{bucket}/{key}?retention
pub async fn put_retention<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<ObjectLockQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let version_id = query
        .version_id
        .map(|v| VersionId::from_string(&v))
        .unwrap_or_else(VersionId::null);

    // Check for bypass governance header
    let bypass_governance = headers
        .get("x-amz-bypass-governance-retention")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // Parse XML body
    let xml_str = std::str::from_utf8(&body)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

    let xml_retention: RetentionXml = quick_xml::de::from_str(xml_str)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid XML: {}", e)))?;

    // Parse mode
    let mode: RetentionMode = xml_retention
        .mode
        .parse()
        .map_err(|e: warp_store::error::Error| ApiError::InvalidRequest(e.to_string()))?;

    // Parse date
    let retain_until: DateTime<Utc> = xml_retention
        .retain_until_date
        .parse()
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid date: {}", e)))?;

    let retention = ObjectRetention {
        mode,
        retain_until_date: retain_until,
    };

    // Set retention
    if let Some(manager) = &state.object_lock_manager {
        // Check if we can modify existing retention
        if let Some(existing) = manager.get_retention(&bucket, &key, &version_id) {
            if !existing.can_modify(bypass_governance) {
                return Err(ApiError::AccessDenied(
                    "Object is locked and cannot be modified".to_string(),
                ));
            }
        }

        manager
            .set_retention(&bucket, &key, version_id, retention)
            .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    } else {
        return Err(ApiError::Internal(
            "Object Lock manager not configured".to_string(),
        ));
    }

    tracing::info!(bucket = %bucket, key = %key, "Retention set");

    Ok(StatusCode::OK.into_response())
}

// =============================================================================
// Object-level Legal Hold Handlers
// =============================================================================

/// Get legal hold status for an object
///
/// GET /{bucket}/{key}?legal-hold
pub async fn get_legal_hold<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<ObjectLockQuery>,
) -> ApiResult<Response> {
    let version_id = query
        .version_id
        .map(|v| VersionId::from_string(&v))
        .unwrap_or_else(VersionId::null);

    let status = state
        .object_lock_manager
        .as_ref()
        .map(|m| m.get_legal_hold(&bucket, &key, &version_id))
        .unwrap_or(LegalHoldStatus::Off);

    let xml = LegalHoldXml {
        status: status.to_string(),
    };

    let xml_str = quick_xml::se::to_string(&xml)
        .map_err(|e| ApiError::Internal(format!("XML serialization error: {}", e)))?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml")],
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml_str),
    )
        .into_response())
}

/// Set legal hold status for an object
///
/// PUT /{bucket}/{key}?legal-hold
pub async fn put_legal_hold<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<ObjectLockQuery>,
    body: Bytes,
) -> ApiResult<Response> {
    let version_id = query
        .version_id
        .map(|v| VersionId::from_string(&v))
        .unwrap_or_else(VersionId::null);

    // Parse XML body
    let xml_str = std::str::from_utf8(&body)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

    let xml_hold: LegalHoldXml = quick_xml::de::from_str(xml_str)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid XML: {}", e)))?;

    // Parse status
    let status: LegalHoldStatus = xml_hold
        .status
        .parse()
        .map_err(|e: warp_store::error::Error| ApiError::InvalidRequest(e.to_string()))?;

    // Set legal hold
    if let Some(manager) = &state.object_lock_manager {
        manager
            .set_legal_hold(&bucket, &key, version_id, status)
            .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    } else {
        return Err(ApiError::Internal(
            "Object Lock manager not configured".to_string(),
        ));
    }

    tracing::info!(bucket = %bucket, key = %key, status = %status, "Legal hold set");

    Ok(StatusCode::OK.into_response())
}

// =============================================================================
// Conversion Helpers
// =============================================================================

fn to_object_lock_xml(config: &ObjectLockConfig) -> ObjectLockConfigurationXml {
    let rule = config.default_retention.as_ref().map(|dr| ObjectLockRuleXml {
        default_retention: DefaultRetentionXml {
            mode: dr.mode.to_string(),
            days: dr.days,
            years: dr.years,
        },
    });

    ObjectLockConfigurationXml {
        object_lock_enabled: if config.enabled {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        },
        rule,
    }
}

fn from_object_lock_xml(xml: &ObjectLockConfigurationXml) -> ApiResult<ObjectLockConfig> {
    let enabled = xml.object_lock_enabled.eq_ignore_ascii_case("enabled");

    let default_retention = if let Some(rule) = &xml.rule {
        let mode: RetentionMode = rule
            .default_retention
            .mode
            .parse()
            .map_err(|e: warp_store::error::Error| ApiError::InvalidRequest(e.to_string()))?;

        Some(warp_store::object_lock::DefaultRetention {
            mode,
            days: rule.default_retention.days,
            years: rule.default_retention.years,
        })
    } else {
        None
    };

    Ok(ObjectLockConfig {
        enabled,
        default_retention,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retention_xml_serialization() {
        let retention = RetentionXml {
            mode: "GOVERNANCE".to_string(),
            retain_until_date: "2025-12-31T23:59:59Z".to_string(),
        };

        let xml = quick_xml::se::to_string(&retention).unwrap();
        assert!(xml.contains("GOVERNANCE"));
        assert!(xml.contains("2025-12-31"));
    }

    #[test]
    fn test_legal_hold_xml_serialization() {
        let hold = LegalHoldXml {
            status: "ON".to_string(),
        };

        let xml = quick_xml::se::to_string(&hold).unwrap();
        assert!(xml.contains("ON"));
    }

    #[test]
    fn test_object_lock_config_xml() {
        let config = ObjectLockConfig::with_governance_days(30);
        let xml = to_object_lock_xml(&config);

        assert_eq!(xml.object_lock_enabled, "Enabled");
        assert!(xml.rule.is_some());

        let rule = xml.rule.unwrap();
        assert_eq!(rule.default_retention.mode, "GOVERNANCE");
        assert_eq!(rule.default_retention.days, Some(30));
    }

    #[test]
    fn test_parse_object_lock_config_xml() {
        let xml = ObjectLockConfigurationXml {
            object_lock_enabled: "Enabled".to_string(),
            rule: Some(ObjectLockRuleXml {
                default_retention: DefaultRetentionXml {
                    mode: "COMPLIANCE".to_string(),
                    days: None,
                    years: Some(1),
                },
            }),
        };

        let config = from_object_lock_xml(&xml).unwrap();
        assert!(config.enabled);
        assert!(config.default_retention.is_some());

        let dr = config.default_retention.unwrap();
        assert_eq!(dr.mode, RetentionMode::Compliance);
        assert_eq!(dr.years, Some(1));
    }
}
