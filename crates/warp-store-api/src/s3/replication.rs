//! S3-compatible Bucket Replication API
//!
//! Implements:
//! - GET /{bucket}?replication - Get bucket replication configuration
//! - PUT /{bucket}?replication - Set bucket replication configuration
//! - DELETE /{bucket}?replication - Delete bucket replication configuration

use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use warp_store::backend::StorageBackend;
use warp_store::bucket::{ReplicationConfig, ReplicationRule};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

// =============================================================================
// XML Types for S3 Replication API
// =============================================================================

/// Replication configuration (S3 XML format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "ReplicationConfiguration")]
pub struct ReplicationConfigurationXml {
    /// IAM role ARN for replication
    #[serde(rename = "Role")]
    pub role: String,

    /// Replication rules
    #[serde(rename = "Rule")]
    pub rules: Vec<ReplicationRuleXml>,
}

/// A replication rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationRuleXml {
    /// Rule ID
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Priority (for multiple rules)
    #[serde(rename = "Priority", skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,

    /// Status: Enabled or Disabled
    #[serde(rename = "Status")]
    pub status: String,

    /// Filter for objects to replicate
    #[serde(rename = "Filter", skip_serializing_if = "Option::is_none")]
    pub filter: Option<ReplicationFilterXml>,

    /// Destination configuration
    #[serde(rename = "Destination")]
    pub destination: ReplicationDestinationXml,

    /// Delete marker replication
    #[serde(
        rename = "DeleteMarkerReplication",
        skip_serializing_if = "Option::is_none"
    )]
    pub delete_marker_replication: Option<DeleteMarkerReplicationXml>,
}

/// Filter for replication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationFilterXml {
    /// Prefix filter
    #[serde(rename = "Prefix", skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// Tag filter
    #[serde(rename = "Tag", skip_serializing_if = "Option::is_none")]
    pub tag: Option<ReplicationTagXml>,
}

/// Tag for filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationTagXml {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Value")]
    pub value: String,
}

/// Destination configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationDestinationXml {
    /// Destination bucket ARN
    #[serde(rename = "Bucket")]
    pub bucket: String,

    /// Storage class for replicated objects
    #[serde(rename = "StorageClass", skip_serializing_if = "Option::is_none")]
    pub storage_class: Option<String>,

    /// Account for cross-account replication
    #[serde(rename = "Account", skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Delete marker replication status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMarkerReplicationXml {
    #[serde(rename = "Status")]
    pub status: String,
}

// =============================================================================
// Replication Handlers
// =============================================================================

/// Get replication configuration for a bucket
///
/// GET /{bucket}?replication
pub async fn get_replication<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Get replication config from state
    let config = state
        .replication_configs
        .get(&bucket)
        .map(|r| r.value().clone());

    match config {
        Some(config) if !config.rules.is_empty() => {
            let xml_config = to_replication_xml(&config);
            let xml = quick_xml::se::to_string(&xml_config)
                .map_err(|e| ApiError::Internal(format!("XML serialization error: {}", e)))?;

            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/xml")],
                format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml),
            )
                .into_response())
        }
        _ => {
            // No replication configured
            let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
    <Code>ReplicationConfigurationNotFoundError</Code>
    <Message>The replication configuration was not found</Message>
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

/// Set replication configuration for a bucket
///
/// PUT /{bucket}?replication
pub async fn put_replication<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Check versioning is enabled (required for replication)
    let versioning_enabled = state
        .versioning_configs
        .get(&bucket)
        .map(|v| v.value().is_enabled())
        .unwrap_or(false);

    if !versioning_enabled {
        return Err(ApiError::InvalidRequest(
            "Versioning must be enabled on the source bucket for replication".to_string(),
        ));
    }

    // Parse XML body
    let xml_str = std::str::from_utf8(&body)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

    let xml_config: ReplicationConfigurationXml = quick_xml::de::from_str(xml_str)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid XML: {}", e)))?;

    // Convert to internal config
    let config = from_replication_xml(&xml_config)?;

    // Store replication config
    state.replication_configs.insert(bucket.clone(), config);

    tracing::info!(bucket = %bucket, "Bucket replication configured");

    Ok(StatusCode::OK.into_response())
}

/// Delete replication configuration for a bucket
///
/// DELETE /{bucket}?replication
pub async fn delete_replication<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Remove replication config
    state.replication_configs.remove(&bucket);

    tracing::info!(bucket = %bucket, "Bucket replication removed");

    Ok(StatusCode::NO_CONTENT.into_response())
}

// =============================================================================
// Conversion Helpers
// =============================================================================

fn to_replication_xml(config: &ReplicationConfig) -> ReplicationConfigurationXml {
    let rules = config
        .rules
        .iter()
        .map(|rule| ReplicationRuleXml {
            id: Some(rule.id.clone()),
            priority: None,
            status: if rule.enabled {
                "Enabled".to_string()
            } else {
                "Disabled".to_string()
            },
            filter: rule.prefix.as_ref().map(|p| ReplicationFilterXml {
                prefix: Some(p.clone()),
                tag: None,
            }),
            destination: ReplicationDestinationXml {
                bucket: format!("arn:aws:s3:::{}", rule.destination_bucket),
                storage_class: rule.destination_storage_class.as_ref().map(|sc| {
                    match sc {
                        warp_store::object::StorageClass::Standard => "STANDARD",
                        warp_store::object::StorageClass::InfrequentAccess => "STANDARD_IA",
                        warp_store::object::StorageClass::Archive => "GLACIER",
                        warp_store::object::StorageClass::GpuPinned => "STANDARD",
                    }
                    .to_string()
                }),
                account: None,
            },
            delete_marker_replication: Some(DeleteMarkerReplicationXml {
                status: if rule.replicate_deletes {
                    "Enabled".to_string()
                } else {
                    "Disabled".to_string()
                },
            }),
        })
        .collect();

    ReplicationConfigurationXml {
        role: "arn:aws:iam:::role/replication-role".to_string(),
        rules,
    }
}

fn from_replication_xml(xml: &ReplicationConfigurationXml) -> ApiResult<ReplicationConfig> {
    let rules = xml
        .rules
        .iter()
        .map(|rule| {
            // Parse bucket ARN to get bucket name
            let dest_bucket = rule
                .destination
                .bucket
                .strip_prefix("arn:aws:s3:::")
                .unwrap_or(&rule.destination.bucket)
                .to_string();

            // Parse storage class
            let storage_class =
                rule.destination
                    .storage_class
                    .as_ref()
                    .and_then(|sc| match sc.as_str() {
                        "STANDARD" => Some(warp_store::object::StorageClass::Standard),
                        "STANDARD_IA" => Some(warp_store::object::StorageClass::InfrequentAccess),
                        "GLACIER" | "DEEP_ARCHIVE" => {
                            Some(warp_store::object::StorageClass::Archive)
                        }
                        _ => None,
                    });

            ReplicationRule {
                id: rule
                    .id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                enabled: rule.status == "Enabled",
                prefix: rule.filter.as_ref().and_then(|f| f.prefix.clone()),
                destination_bucket: dest_bucket,
                destination_storage_class: storage_class,
                replicate_deletes: rule
                    .delete_marker_replication
                    .as_ref()
                    .map(|d| d.status == "Enabled")
                    .unwrap_or(false),
            }
        })
        .collect();

    Ok(ReplicationConfig { rules })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replication_xml_basic() {
        let config = ReplicationConfig {
            rules: vec![ReplicationRule {
                id: "rule1".to_string(),
                enabled: true,
                prefix: Some("logs/".to_string()),
                destination_bucket: "backup-bucket".to_string(),
                destination_storage_class: None,
                replicate_deletes: true,
            }],
        };

        let xml = to_replication_xml(&config);
        assert_eq!(xml.rules.len(), 1);
        assert_eq!(xml.rules[0].status, "Enabled");
        assert_eq!(
            xml.rules[0].destination.bucket,
            "arn:aws:s3:::backup-bucket"
        );
    }

    #[test]
    fn test_parse_replication_xml() {
        let xml = ReplicationConfigurationXml {
            role: "arn:aws:iam:::role/replication".to_string(),
            rules: vec![ReplicationRuleXml {
                id: Some("test-rule".to_string()),
                priority: None,
                status: "Enabled".to_string(),
                filter: Some(ReplicationFilterXml {
                    prefix: Some("data/".to_string()),
                    tag: None,
                }),
                destination: ReplicationDestinationXml {
                    bucket: "arn:aws:s3:::dest-bucket".to_string(),
                    storage_class: Some("GLACIER".to_string()),
                    account: None,
                },
                delete_marker_replication: Some(DeleteMarkerReplicationXml {
                    status: "Disabled".to_string(),
                }),
            }],
        };

        let config = from_replication_xml(&xml).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].id, "test-rule");
        assert!(config.rules[0].enabled);
        assert_eq!(config.rules[0].prefix, Some("data/".to_string()));
        assert_eq!(config.rules[0].destination_bucket, "dest-bucket");
        assert!(!config.rules[0].replicate_deletes);
    }

    #[test]
    fn test_replication_disabled_rule() {
        let config = ReplicationConfig {
            rules: vec![ReplicationRule {
                id: "disabled-rule".to_string(),
                enabled: false,
                prefix: None,
                destination_bucket: "backup".to_string(),
                destination_storage_class: None,
                replicate_deletes: false,
            }],
        };

        let xml = to_replication_xml(&config);
        assert_eq!(xml.rules[0].status, "Disabled");
    }
}
