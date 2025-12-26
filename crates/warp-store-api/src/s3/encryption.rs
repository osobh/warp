//! S3-compatible Bucket Encryption API
//!
//! Implements:
//! - GET /{bucket}?encryption - Get bucket default encryption
//! - PUT /{bucket}?encryption - Set bucket default encryption
//! - DELETE /{bucket}?encryption - Remove bucket default encryption

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use warp_store::backend::StorageBackend;
use warp_store::bucket::{EncryptionAlgorithm, EncryptionConfig};

use crate::error::{ApiError, ApiResult};
use crate::AppState;

// =============================================================================
// XML Types for S3 Bucket Encryption API
// =============================================================================

/// Server-side encryption configuration (S3 XML format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "ServerSideEncryptionConfiguration")]
pub struct ServerSideEncryptionConfigurationXml {
    #[serde(rename = "Rule")]
    pub rules: Vec<ServerSideEncryptionRuleXml>,
}

/// Server-side encryption rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSideEncryptionRuleXml {
    #[serde(rename = "ApplyServerSideEncryptionByDefault")]
    pub apply_server_side_encryption_by_default: ApplyServerSideEncryptionByDefaultXml,

    #[serde(rename = "BucketKeyEnabled", skip_serializing_if = "Option::is_none")]
    pub bucket_key_enabled: Option<bool>,
}

/// Default encryption settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyServerSideEncryptionByDefaultXml {
    /// SSE algorithm: AES256 or aws:kms
    #[serde(rename = "SSEAlgorithm")]
    pub sse_algorithm: String,

    /// KMS key ID (only for aws:kms)
    #[serde(rename = "KMSMasterKeyID", skip_serializing_if = "Option::is_none")]
    pub kms_master_key_id: Option<String>,
}

// =============================================================================
// Encryption Handlers
// =============================================================================

/// Get default encryption for a bucket
///
/// GET /{bucket}?encryption
pub async fn get_encryption<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Get encryption config from state
    let config = state
        .encryption_configs
        .get(&bucket)
        .map(|r| r.value().clone());

    match config {
        Some(config) if config.enabled => {
            let xml_config = to_encryption_xml(&config);
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
            // No encryption configured
            let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
    <Code>ServerSideEncryptionConfigurationNotFoundError</Code>
    <Message>The server side encryption configuration was not found</Message>
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

/// Set default encryption for a bucket
///
/// PUT /{bucket}?encryption
pub async fn put_encryption<B: StorageBackend>(
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

    let xml_config: ServerSideEncryptionConfigurationXml = quick_xml::de::from_str(xml_str)
        .map_err(|e| ApiError::InvalidRequest(format!("Invalid XML: {}", e)))?;

    // Convert to internal config
    let config = from_encryption_xml(&xml_config)?;

    // Store encryption config
    state.encryption_configs.insert(bucket.clone(), config.clone());

    tracing::info!(bucket = %bucket, algorithm = ?config.algorithm, "Bucket encryption configured");

    Ok(StatusCode::OK.into_response())
}

/// Delete default encryption for a bucket
///
/// DELETE /{bucket}?encryption
pub async fn delete_encryption<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    // Verify bucket exists
    let buckets = state.store.list_buckets().await;
    if !buckets.contains(&bucket) {
        return Err(ApiError::NotFound(format!("Bucket '{}' not found", bucket)));
    }

    // Remove encryption config
    state.encryption_configs.remove(&bucket);

    tracing::info!(bucket = %bucket, "Bucket encryption removed");

    Ok(StatusCode::NO_CONTENT.into_response())
}

// =============================================================================
// Conversion Helpers
// =============================================================================

fn to_encryption_xml(config: &EncryptionConfig) -> ServerSideEncryptionConfigurationXml {
    let sse_algorithm = match config.algorithm {
        EncryptionAlgorithm::None => "AES256".to_string(),
        EncryptionAlgorithm::Aes256Gcm => "AES256".to_string(),
        EncryptionAlgorithm::ChaCha20Poly1305 => "AES256".to_string(), // Map to S3-compatible
    };

    ServerSideEncryptionConfigurationXml {
        rules: vec![ServerSideEncryptionRuleXml {
            apply_server_side_encryption_by_default: ApplyServerSideEncryptionByDefaultXml {
                sse_algorithm,
                kms_master_key_id: config.key_id.clone(),
            },
            bucket_key_enabled: Some(true),
        }],
    }
}

fn from_encryption_xml(xml: &ServerSideEncryptionConfigurationXml) -> ApiResult<EncryptionConfig> {
    let rule = xml.rules.first().ok_or_else(|| {
        ApiError::InvalidRequest("At least one encryption rule is required".to_string())
    })?;

    let algorithm = match rule.apply_server_side_encryption_by_default.sse_algorithm.as_str() {
        "AES256" => EncryptionAlgorithm::Aes256Gcm,
        "aws:kms" => {
            // For KMS, we still use AES256 internally but track the key ID
            EncryptionAlgorithm::Aes256Gcm
        }
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "Unsupported SSE algorithm: {}. Must be 'AES256' or 'aws:kms'.",
                other
            )));
        }
    };

    Ok(EncryptionConfig {
        enabled: true,
        algorithm,
        key_id: rule.apply_server_side_encryption_by_default.kms_master_key_id.clone(),
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_xml_aes256() {
        let config = EncryptionConfig {
            enabled: true,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            key_id: None,
        };

        let xml = to_encryption_xml(&config);
        assert_eq!(xml.rules.len(), 1);
        assert_eq!(
            xml.rules[0].apply_server_side_encryption_by_default.sse_algorithm,
            "AES256"
        );
    }

    #[test]
    fn test_encryption_xml_with_kms_key() {
        let config = EncryptionConfig {
            enabled: true,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            key_id: Some("arn:aws:kms:us-east-1:123456789:key/abc".to_string()),
        };

        let xml = to_encryption_xml(&config);
        assert_eq!(
            xml.rules[0].apply_server_side_encryption_by_default.kms_master_key_id,
            Some("arn:aws:kms:us-east-1:123456789:key/abc".to_string())
        );
    }

    #[test]
    fn test_parse_encryption_xml() {
        let xml = ServerSideEncryptionConfigurationXml {
            rules: vec![ServerSideEncryptionRuleXml {
                apply_server_side_encryption_by_default: ApplyServerSideEncryptionByDefaultXml {
                    sse_algorithm: "AES256".to_string(),
                    kms_master_key_id: None,
                },
                bucket_key_enabled: Some(true),
            }],
        };

        let config = from_encryption_xml(&xml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.algorithm, EncryptionAlgorithm::Aes256Gcm);
    }

    #[test]
    fn test_parse_encryption_xml_kms() {
        let xml = ServerSideEncryptionConfigurationXml {
            rules: vec![ServerSideEncryptionRuleXml {
                apply_server_side_encryption_by_default: ApplyServerSideEncryptionByDefaultXml {
                    sse_algorithm: "aws:kms".to_string(),
                    kms_master_key_id: Some("my-key-id".to_string()),
                },
                bucket_key_enabled: None,
            }],
        };

        let config = from_encryption_xml(&xml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.key_id, Some("my-key-id".to_string()));
    }
}
