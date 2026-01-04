//! S3-compatible lifecycle configuration API
//!
//! Implements:
//! - GET /{bucket}?lifecycle - Get bucket lifecycle configuration
//! - PUT /{bucket}?lifecycle - Set bucket lifecycle configuration
//! - DELETE /{bucket}?lifecycle - Delete bucket lifecycle configuration

use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use warp_store::backend::StorageBackend;
use warp_store::bucket::{
    ExpirationAction, LifecycleRule, NoncurrentExpirationAction, NoncurrentTransitionAction,
    TransitionAction,
};
use warp_store::object::StorageClass;

use crate::AppState;
use crate::error::{ApiError, ApiResult};

/// Query parameter to detect lifecycle requests
#[derive(Debug, Deserialize, Default)]
pub struct LifecycleQuery {
    /// Presence of this key indicates a lifecycle request
    pub lifecycle: Option<String>,
}

impl LifecycleQuery {
    /// Check if this is a lifecycle request
    pub fn is_lifecycle_request(&self) -> bool {
        self.lifecycle.is_some()
    }
}

// =============================================================================
// XML Types for S3 Lifecycle API
// =============================================================================

/// Lifecycle configuration (S3 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "LifecycleConfiguration")]
pub struct LifecycleConfigurationXml {
    #[serde(rename = "Rule", default)]
    pub rules: Vec<LifecycleRuleXml>,
}

/// A lifecycle rule (S3 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Rule")]
pub struct LifecycleRuleXml {
    #[serde(rename = "ID")]
    pub id: Option<String>,

    #[serde(rename = "Status")]
    pub status: String,

    #[serde(rename = "Filter")]
    pub filter: Option<FilterXml>,

    #[serde(rename = "Prefix")]
    pub prefix: Option<String>,

    #[serde(rename = "Expiration")]
    pub expiration: Option<ExpirationXml>,

    #[serde(rename = "Transition", default)]
    pub transitions: Vec<TransitionXml>,

    #[serde(rename = "NoncurrentVersionExpiration")]
    pub noncurrent_expiration: Option<NoncurrentExpirationXml>,

    #[serde(rename = "NoncurrentVersionTransition", default)]
    pub noncurrent_transitions: Vec<NoncurrentTransitionXml>,
}

/// Filter for lifecycle rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterXml {
    #[serde(rename = "Prefix")]
    pub prefix: Option<String>,

    #[serde(rename = "Tag")]
    pub tag: Option<TagXml>,

    #[serde(rename = "And")]
    pub and: Option<AndFilterXml>,
}

/// Tag filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagXml {
    #[serde(rename = "Key")]
    pub key: String,

    #[serde(rename = "Value")]
    pub value: String,
}

/// And filter (combines prefix and tags)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndFilterXml {
    #[serde(rename = "Prefix")]
    pub prefix: Option<String>,

    #[serde(rename = "Tag", default)]
    pub tags: Vec<TagXml>,
}

/// Expiration action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpirationXml {
    #[serde(rename = "Days")]
    pub days: Option<u32>,

    #[serde(rename = "Date")]
    pub date: Option<String>,

    #[serde(rename = "ExpiredObjectDeleteMarker")]
    pub expired_object_delete_marker: Option<bool>,
}

/// Transition action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionXml {
    #[serde(rename = "Days")]
    pub days: Option<u32>,

    #[serde(rename = "Date")]
    pub date: Option<String>,

    #[serde(rename = "StorageClass")]
    pub storage_class: String,
}

/// Noncurrent version expiration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoncurrentExpirationXml {
    #[serde(rename = "NoncurrentDays")]
    pub noncurrent_days: u32,

    #[serde(rename = "NewerNoncurrentVersions")]
    pub newer_noncurrent_versions: Option<u32>,
}

/// Noncurrent version transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoncurrentTransitionXml {
    #[serde(rename = "NoncurrentDays")]
    pub noncurrent_days: u32,

    #[serde(rename = "NewerNoncurrentVersions")]
    pub newer_noncurrent_versions: Option<u32>,

    #[serde(rename = "StorageClass")]
    pub storage_class: String,
}

// =============================================================================
// Handlers
// =============================================================================

/// Get bucket lifecycle configuration
pub async fn get_lifecycle<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    // Get lifecycle rules from bucket config
    // TODO: Actually fetch from bucket config via Raft when supported
    let rules: Vec<LifecycleRule> = state.get_lifecycle_rules(&bucket);

    if rules.is_empty() {
        // S3 returns 404 NoSuchLifecycleConfiguration when no config exists
        return Err(ApiError::NotFound(format!(
            "The lifecycle configuration does not exist for bucket: {}",
            bucket
        )));
    }

    // Convert to XML format
    let config = LifecycleConfigurationXml {
        rules: rules.into_iter().map(rule_to_xml).collect(),
    };

    let xml = render_lifecycle_config(&config);

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml")],
        xml,
    )
        .into_response())
}

/// Set bucket lifecycle configuration
pub async fn put_lifecycle<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    // Parse XML body
    let body_str =
        std::str::from_utf8(&body).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    let config: LifecycleConfigurationXml =
        quick_xml::de::from_str(body_str).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    // Convert to internal format
    let rules: Vec<LifecycleRule> = config
        .rules
        .into_iter()
        .map(|r| xml_to_rule(r))
        .collect::<Result<Vec<_>, _>>()?;

    // Store lifecycle configuration
    state.set_lifecycle_rules(&bucket, rules);

    Ok(StatusCode::OK.into_response())
}

/// Delete bucket lifecycle configuration
pub async fn delete_lifecycle<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    state.set_lifecycle_rules(&bucket, Vec::new());
    Ok(StatusCode::NO_CONTENT.into_response())
}

// =============================================================================
// Conversion Functions
// =============================================================================

/// Convert internal LifecycleRule to XML format
fn rule_to_xml(rule: LifecycleRule) -> LifecycleRuleXml {
    let filter = if rule.prefix.is_some() || !rule.tags.is_empty() {
        if rule.tags.is_empty() {
            Some(FilterXml {
                prefix: rule.prefix.clone(),
                tag: None,
                and: None,
            })
        } else if rule.prefix.is_none() && rule.tags.len() == 1 {
            let (k, v) = &rule.tags[0];
            Some(FilterXml {
                prefix: None,
                tag: Some(TagXml {
                    key: k.clone(),
                    value: v.clone(),
                }),
                and: None,
            })
        } else {
            Some(FilterXml {
                prefix: None,
                tag: None,
                and: Some(AndFilterXml {
                    prefix: rule.prefix.clone(),
                    tags: rule
                        .tags
                        .iter()
                        .map(|(k, v)| TagXml {
                            key: k.clone(),
                            value: v.clone(),
                        })
                        .collect(),
                }),
            })
        }
    } else {
        None
    };

    LifecycleRuleXml {
        id: Some(rule.id),
        status: if rule.enabled {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        },
        filter,
        prefix: None, // Use Filter instead of deprecated Prefix
        expiration: rule.expiration.map(|e| ExpirationXml {
            days: e.days,
            date: e
                .date
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
            expired_object_delete_marker: Some(e.expired_object_delete_marker),
        }),
        transitions: rule
            .transitions
            .iter()
            .map(|t| TransitionXml {
                days: Some(t.days),
                date: None,
                storage_class: storage_class_to_string(t.storage_class),
            })
            .collect(),
        noncurrent_expiration: rule.noncurrent_expiration.map(|e| NoncurrentExpirationXml {
            noncurrent_days: e.noncurrent_days,
            newer_noncurrent_versions: e.newer_noncurrent_versions,
        }),
        noncurrent_transitions: rule
            .noncurrent_transitions
            .iter()
            .map(|t| NoncurrentTransitionXml {
                noncurrent_days: t.noncurrent_days,
                newer_noncurrent_versions: t.newer_noncurrent_versions,
                storage_class: storage_class_to_string(t.storage_class),
            })
            .collect(),
    }
}

/// Convert XML format to internal LifecycleRule
fn xml_to_rule(xml: LifecycleRuleXml) -> Result<LifecycleRule, ApiError> {
    // Extract prefix and tags from filter
    let (prefix, tags) = if let Some(filter) = xml.filter {
        if let Some(and) = filter.and {
            (
                and.prefix,
                and.tags
                    .into_iter()
                    .map(|t| (t.key, t.value))
                    .collect::<Vec<_>>(),
            )
        } else if let Some(tag) = filter.tag {
            (None, vec![(tag.key, tag.value)])
        } else {
            (filter.prefix, Vec::new())
        }
    } else {
        (xml.prefix, Vec::new())
    };

    let expiration = xml.expiration.map(|e| ExpirationAction {
        days: e.days,
        date: e.date.and_then(|d| {
            chrono::DateTime::parse_from_rfc3339(&d)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        }),
        expired_object_delete_marker: e.expired_object_delete_marker.unwrap_or(false),
    });

    let transitions = xml
        .transitions
        .into_iter()
        .filter_map(|t| {
            t.days.map(|days| TransitionAction {
                days,
                storage_class: string_to_storage_class(&t.storage_class),
            })
        })
        .collect();

    let noncurrent_expiration = xml
        .noncurrent_expiration
        .map(|e| NoncurrentExpirationAction {
            noncurrent_days: e.noncurrent_days,
            newer_noncurrent_versions: e.newer_noncurrent_versions,
        });

    let noncurrent_transitions = xml
        .noncurrent_transitions
        .into_iter()
        .map(|t| NoncurrentTransitionAction {
            noncurrent_days: t.noncurrent_days,
            newer_noncurrent_versions: t.newer_noncurrent_versions,
            storage_class: string_to_storage_class(&t.storage_class),
        })
        .collect();

    Ok(LifecycleRule {
        id: xml.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        enabled: xml.status == "Enabled",
        prefix,
        tags,
        expiration,
        transitions,
        noncurrent_expiration,
        noncurrent_transitions,
    })
}

/// Convert StorageClass to S3 string
fn storage_class_to_string(class: StorageClass) -> String {
    match class {
        StorageClass::Standard => "STANDARD".to_string(),
        StorageClass::InfrequentAccess => "STANDARD_IA".to_string(),
        StorageClass::Archive => "GLACIER".to_string(),
        StorageClass::GpuPinned => "GPU_PINNED".to_string(), // WARP-specific
    }
}

/// Convert S3 string to StorageClass
fn string_to_storage_class(s: &str) -> StorageClass {
    match s.to_uppercase().as_str() {
        "STANDARD" => StorageClass::Standard,
        "STANDARD_IA" | "ONEZONE_IA" | "INTELLIGENT_TIERING" => StorageClass::InfrequentAccess,
        "GLACIER" | "GLACIER_IR" | "DEEP_ARCHIVE" => StorageClass::Archive,
        "GPU_PINNED" => StorageClass::GpuPinned,
        _ => StorageClass::Standard,
    }
}

/// Render lifecycle configuration to XML string
fn render_lifecycle_config(config: &LifecycleConfigurationXml) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push_str("\n<LifecycleConfiguration>");

    for rule in &config.rules {
        xml.push_str("\n  <Rule>");

        if let Some(ref id) = rule.id {
            xml.push_str(&format!("\n    <ID>{}</ID>", id));
        }

        xml.push_str(&format!("\n    <Status>{}</Status>", rule.status));

        // Filter
        if let Some(ref filter) = rule.filter {
            xml.push_str("\n    <Filter>");
            if let Some(ref prefix) = filter.prefix {
                xml.push_str(&format!("\n      <Prefix>{}</Prefix>", prefix));
            }
            if let Some(ref tag) = filter.tag {
                xml.push_str("\n      <Tag>");
                xml.push_str(&format!("\n        <Key>{}</Key>", tag.key));
                xml.push_str(&format!("\n        <Value>{}</Value>", tag.value));
                xml.push_str("\n      </Tag>");
            }
            if let Some(ref and) = filter.and {
                xml.push_str("\n      <And>");
                if let Some(ref prefix) = and.prefix {
                    xml.push_str(&format!("\n        <Prefix>{}</Prefix>", prefix));
                }
                for tag in &and.tags {
                    xml.push_str("\n        <Tag>");
                    xml.push_str(&format!("\n          <Key>{}</Key>", tag.key));
                    xml.push_str(&format!("\n          <Value>{}</Value>", tag.value));
                    xml.push_str("\n        </Tag>");
                }
                xml.push_str("\n      </And>");
            }
            xml.push_str("\n    </Filter>");
        }

        // Expiration
        if let Some(ref exp) = rule.expiration {
            xml.push_str("\n    <Expiration>");
            if let Some(days) = exp.days {
                xml.push_str(&format!("\n      <Days>{}</Days>", days));
            }
            if let Some(ref date) = exp.date {
                xml.push_str(&format!("\n      <Date>{}</Date>", date));
            }
            if let Some(delete_marker) = exp.expired_object_delete_marker {
                xml.push_str(&format!(
                    "\n      <ExpiredObjectDeleteMarker>{}</ExpiredObjectDeleteMarker>",
                    delete_marker
                ));
            }
            xml.push_str("\n    </Expiration>");
        }

        // Transitions
        for transition in &rule.transitions {
            xml.push_str("\n    <Transition>");
            if let Some(days) = transition.days {
                xml.push_str(&format!("\n      <Days>{}</Days>", days));
            }
            if let Some(ref date) = transition.date {
                xml.push_str(&format!("\n      <Date>{}</Date>", date));
            }
            xml.push_str(&format!(
                "\n      <StorageClass>{}</StorageClass>",
                transition.storage_class
            ));
            xml.push_str("\n    </Transition>");
        }

        // Noncurrent version expiration
        if let Some(ref exp) = rule.noncurrent_expiration {
            xml.push_str("\n    <NoncurrentVersionExpiration>");
            xml.push_str(&format!(
                "\n      <NoncurrentDays>{}</NoncurrentDays>",
                exp.noncurrent_days
            ));
            if let Some(keep) = exp.newer_noncurrent_versions {
                xml.push_str(&format!(
                    "\n      <NewerNoncurrentVersions>{}</NewerNoncurrentVersions>",
                    keep
                ));
            }
            xml.push_str("\n    </NoncurrentVersionExpiration>");
        }

        // Noncurrent version transitions
        for transition in &rule.noncurrent_transitions {
            xml.push_str("\n    <NoncurrentVersionTransition>");
            xml.push_str(&format!(
                "\n      <NoncurrentDays>{}</NoncurrentDays>",
                transition.noncurrent_days
            ));
            if let Some(keep) = transition.newer_noncurrent_versions {
                xml.push_str(&format!(
                    "\n      <NewerNoncurrentVersions>{}</NewerNoncurrentVersions>",
                    keep
                ));
            }
            xml.push_str(&format!(
                "\n      <StorageClass>{}</StorageClass>",
                transition.storage_class
            ));
            xml.push_str("\n    </NoncurrentVersionTransition>");
        }

        xml.push_str("\n  </Rule>");
    }

    xml.push_str("\n</LifecycleConfiguration>");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_class_conversion() {
        assert_eq!(storage_class_to_string(StorageClass::Standard), "STANDARD");
        assert_eq!(
            storage_class_to_string(StorageClass::InfrequentAccess),
            "STANDARD_IA"
        );
        assert_eq!(storage_class_to_string(StorageClass::Archive), "GLACIER");
        assert_eq!(
            storage_class_to_string(StorageClass::GpuPinned),
            "GPU_PINNED"
        );
    }

    #[test]
    fn test_string_to_storage_class() {
        assert_eq!(string_to_storage_class("STANDARD"), StorageClass::Standard);
        assert_eq!(
            string_to_storage_class("STANDARD_IA"),
            StorageClass::InfrequentAccess
        );
        assert_eq!(
            string_to_storage_class("ONEZONE_IA"),
            StorageClass::InfrequentAccess
        );
        assert_eq!(string_to_storage_class("GLACIER"), StorageClass::Archive);
        assert_eq!(
            string_to_storage_class("DEEP_ARCHIVE"),
            StorageClass::Archive
        );
        assert_eq!(
            string_to_storage_class("GPU_PINNED"),
            StorageClass::GpuPinned
        );
        assert_eq!(string_to_storage_class("UNKNOWN"), StorageClass::Standard);
    }

    #[test]
    fn test_lifecycle_rule_xml_parse() {
        let xml = r#"
        <LifecycleConfiguration>
            <Rule>
                <ID>ExpireOldLogs</ID>
                <Status>Enabled</Status>
                <Filter>
                    <Prefix>logs/</Prefix>
                </Filter>
                <Expiration>
                    <Days>90</Days>
                </Expiration>
            </Rule>
        </LifecycleConfiguration>
        "#;

        let config: LifecycleConfigurationXml = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].id, Some("ExpireOldLogs".to_string()));
        assert_eq!(config.rules[0].status, "Enabled");
        assert!(config.rules[0].filter.is_some());
        assert_eq!(
            config.rules[0].filter.as_ref().unwrap().prefix,
            Some("logs/".to_string())
        );
        assert!(config.rules[0].expiration.is_some());
        assert_eq!(config.rules[0].expiration.as_ref().unwrap().days, Some(90));
    }

    #[test]
    fn test_lifecycle_rule_conversion() {
        let xml_rule = LifecycleRuleXml {
            id: Some("test-rule".to_string()),
            status: "Enabled".to_string(),
            filter: Some(FilterXml {
                prefix: Some("data/".to_string()),
                tag: None,
                and: None,
            }),
            prefix: None,
            expiration: Some(ExpirationXml {
                days: Some(30),
                date: None,
                expired_object_delete_marker: Some(false),
            }),
            transitions: vec![TransitionXml {
                days: Some(7),
                date: None,
                storage_class: "STANDARD_IA".to_string(),
            }],
            noncurrent_expiration: None,
            noncurrent_transitions: Vec::new(),
        };

        let rule = xml_to_rule(xml_rule).unwrap();
        assert_eq!(rule.id, "test-rule");
        assert!(rule.enabled);
        assert_eq!(rule.prefix, Some("data/".to_string()));
        assert!(rule.expiration.is_some());
        assert_eq!(rule.expiration.as_ref().unwrap().days, Some(30));
        assert_eq!(rule.transitions.len(), 1);
        assert_eq!(rule.transitions[0].days, 7);
        assert_eq!(
            rule.transitions[0].storage_class,
            StorageClass::InfrequentAccess
        );
    }
}
