//! S3-compatible notification configuration API
//!
//! Implements:
//! - GET /{bucket}?notification - Get bucket notification configuration
//! - PUT /{bucket}?notification - Set bucket notification configuration
//! - DELETE /{bucket}?notification - Delete bucket notification configuration

use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use warp_store::backend::StorageBackend;
use warp_store::events::{
    FilterRule, FilterRules, HpcChannelConfiguration, KeyFilter, LambdaConfiguration,
    NotificationConfiguration, QueueConfiguration, TopicConfiguration,
};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

// =============================================================================
// XML Types for S3 Notification API
// =============================================================================

/// Notification configuration (S3 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "NotificationConfiguration")]
pub struct NotificationConfigurationXml {
    /// SNS topic configurations for event notifications
    #[serde(rename = "TopicConfiguration", default)]
    pub topic_configurations: Vec<TopicConfigurationXml>,

    /// SQS queue configurations for event notifications
    #[serde(rename = "QueueConfiguration", default)]
    pub queue_configurations: Vec<QueueConfigurationXml>,

    /// Lambda function configurations for event notifications
    #[serde(rename = "CloudFunctionConfiguration", default)]
    pub lambda_configurations: Vec<LambdaConfigurationXml>,

    /// HPC-Channels configuration (WARP-specific extension)
    #[serde(rename = "HpcChannelConfiguration", default)]
    pub hpc_channel_configurations: Vec<HpcChannelConfigurationXml>,
}

/// Topic configuration (S3 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicConfigurationXml {
    #[serde(rename = "Id")]
    pub id: Option<String>,

    #[serde(rename = "Topic")]
    pub topic: String,

    #[serde(rename = "Event", default)]
    pub events: Vec<String>,

    #[serde(rename = "Filter")]
    pub filter: Option<FilterXml>,
}

/// Queue configuration (S3 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfigurationXml {
    #[serde(rename = "Id")]
    pub id: Option<String>,

    #[serde(rename = "Queue")]
    pub queue: String,

    #[serde(rename = "Event", default)]
    pub events: Vec<String>,

    #[serde(rename = "Filter")]
    pub filter: Option<FilterXml>,
}

/// Lambda configuration (S3 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LambdaConfigurationXml {
    #[serde(rename = "Id")]
    pub id: Option<String>,

    #[serde(rename = "CloudFunction")]
    pub cloud_function: String,

    #[serde(rename = "Event", default)]
    pub events: Vec<String>,

    #[serde(rename = "Filter")]
    pub filter: Option<FilterXml>,
}

/// HPC-Channels configuration (WARP-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HpcChannelConfigurationXml {
    #[serde(rename = "Id")]
    pub id: Option<String>,

    #[serde(rename = "ChannelId")]
    pub channel_id: String,

    #[serde(rename = "Event", default)]
    pub events: Vec<String>,

    #[serde(rename = "Filter")]
    pub filter: Option<FilterXml>,
}

/// Filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterXml {
    #[serde(rename = "S3Key")]
    pub s3_key: Option<S3KeyFilterXml>,
}

/// S3 key filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3KeyFilterXml {
    #[serde(rename = "FilterRule", default)]
    pub filter_rules: Vec<FilterRuleXml>,
}

/// Filter rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRuleXml {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Value")]
    pub value: String,
}

// =============================================================================
// Handlers
// =============================================================================

/// Get bucket notification configuration
pub async fn get_notifications<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    let config = state.get_notification_config(&bucket);

    // Unlike lifecycle, S3 returns empty config instead of 404
    let config = config.unwrap_or_default();

    // Convert to XML format
    let xml_config = config_to_xml(&config);
    let xml = render_notification_config(&xml_config);

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml")],
        xml,
    )
        .into_response())
}

/// Set bucket notification configuration
pub async fn put_notifications<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    // Parse XML body
    let body_str =
        std::str::from_utf8(&body).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    let xml_config: NotificationConfigurationXml =
        quick_xml::de::from_str(body_str).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    // Convert to internal format
    let config = xml_to_config(xml_config);

    // Store configuration
    state.set_notification_config(&bucket, config);

    Ok(StatusCode::OK.into_response())
}

/// Delete bucket notification configuration
pub async fn delete_notifications<B: StorageBackend>(
    State(state): State<AppState<B>>,
    Path(bucket): Path<String>,
) -> ApiResult<Response> {
    state.set_notification_config(&bucket, NotificationConfiguration::default());
    Ok(StatusCode::NO_CONTENT.into_response())
}

// =============================================================================
// Conversion Functions
// =============================================================================

/// Convert internal config to XML format
fn config_to_xml(config: &NotificationConfiguration) -> NotificationConfigurationXml {
    NotificationConfigurationXml {
        topic_configurations: config
            .topic_configurations
            .iter()
            .map(|c| TopicConfigurationXml {
                id: Some(c.id.clone()),
                topic: c.topic_arn.clone(),
                events: c.events.clone(),
                filter: c.filter.as_ref().map(filter_to_xml),
            })
            .collect(),
        queue_configurations: config
            .queue_configurations
            .iter()
            .map(|c| QueueConfigurationXml {
                id: Some(c.id.clone()),
                queue: c.queue_arn.clone(),
                events: c.events.clone(),
                filter: c.filter.as_ref().map(filter_to_xml),
            })
            .collect(),
        lambda_configurations: config
            .lambda_function_configurations
            .iter()
            .map(|c| LambdaConfigurationXml {
                id: Some(c.id.clone()),
                cloud_function: c.lambda_function_arn.clone(),
                events: c.events.clone(),
                filter: c.filter.as_ref().map(filter_to_xml),
            })
            .collect(),
        hpc_channel_configurations: config
            .hpc_channel_configurations
            .iter()
            .map(|c| HpcChannelConfigurationXml {
                id: Some(c.id.clone()),
                channel_id: c.channel_id.clone(),
                events: c.events.clone(),
                filter: c.filter.as_ref().map(filter_to_xml),
            })
            .collect(),
    }
}

/// Convert filter rules to XML
fn filter_to_xml(filter: &FilterRules) -> FilterXml {
    FilterXml {
        s3_key: Some(S3KeyFilterXml {
            filter_rules: filter
                .key
                .filter_rules
                .iter()
                .map(|r| FilterRuleXml {
                    name: r.name.clone(),
                    value: r.value.clone(),
                })
                .collect(),
        }),
    }
}

/// Convert XML config to internal format
fn xml_to_config(xml: NotificationConfigurationXml) -> NotificationConfiguration {
    NotificationConfiguration {
        topic_configurations: xml
            .topic_configurations
            .into_iter()
            .map(|c| TopicConfiguration {
                id: c.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                topic_arn: c.topic,
                events: c.events,
                filter: c.filter.and_then(xml_to_filter),
            })
            .collect(),
        queue_configurations: xml
            .queue_configurations
            .into_iter()
            .map(|c| QueueConfiguration {
                id: c.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                queue_arn: c.queue,
                events: c.events,
                filter: c.filter.and_then(xml_to_filter),
            })
            .collect(),
        lambda_function_configurations: xml
            .lambda_configurations
            .into_iter()
            .map(|c| LambdaConfiguration {
                id: c.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                lambda_function_arn: c.cloud_function,
                events: c.events,
                filter: c.filter.and_then(xml_to_filter),
            })
            .collect(),
        hpc_channel_configurations: xml
            .hpc_channel_configurations
            .into_iter()
            .map(|c| HpcChannelConfiguration {
                id: c.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                channel_id: c.channel_id,
                events: c.events,
                filter: c.filter.and_then(xml_to_filter),
            })
            .collect(),
        // Webhooks not yet supported via XML configuration
        webhook_configurations: Vec::new(),
    }
}

/// Convert XML filter to internal format
fn xml_to_filter(xml: FilterXml) -> Option<FilterRules> {
    xml.s3_key.map(|s3_key| FilterRules {
        key: KeyFilter {
            filter_rules: s3_key
                .filter_rules
                .into_iter()
                .map(|r| FilterRule {
                    name: r.name,
                    value: r.value,
                })
                .collect(),
        },
    })
}

/// Render notification configuration to XML string
fn render_notification_config(config: &NotificationConfigurationXml) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push_str("\n<NotificationConfiguration>");

    // Topic configurations
    for topic in &config.topic_configurations {
        xml.push_str("\n  <TopicConfiguration>");
        if let Some(ref id) = topic.id {
            xml.push_str(&format!("\n    <Id>{}</Id>", id));
        }
        xml.push_str(&format!("\n    <Topic>{}</Topic>", topic.topic));
        for event in &topic.events {
            xml.push_str(&format!("\n    <Event>{}</Event>", event));
        }
        if let Some(ref filter) = topic.filter {
            render_filter(&mut xml, filter);
        }
        xml.push_str("\n  </TopicConfiguration>");
    }

    // Queue configurations
    for queue in &config.queue_configurations {
        xml.push_str("\n  <QueueConfiguration>");
        if let Some(ref id) = queue.id {
            xml.push_str(&format!("\n    <Id>{}</Id>", id));
        }
        xml.push_str(&format!("\n    <Queue>{}</Queue>", queue.queue));
        for event in &queue.events {
            xml.push_str(&format!("\n    <Event>{}</Event>", event));
        }
        if let Some(ref filter) = queue.filter {
            render_filter(&mut xml, filter);
        }
        xml.push_str("\n  </QueueConfiguration>");
    }

    // Lambda configurations
    for lambda in &config.lambda_configurations {
        xml.push_str("\n  <CloudFunctionConfiguration>");
        if let Some(ref id) = lambda.id {
            xml.push_str(&format!("\n    <Id>{}</Id>", id));
        }
        xml.push_str(&format!(
            "\n    <CloudFunction>{}</CloudFunction>",
            lambda.cloud_function
        ));
        for event in &lambda.events {
            xml.push_str(&format!("\n    <Event>{}</Event>", event));
        }
        if let Some(ref filter) = lambda.filter {
            render_filter(&mut xml, filter);
        }
        xml.push_str("\n  </CloudFunctionConfiguration>");
    }

    // HPC Channel configurations (WARP extension)
    for channel in &config.hpc_channel_configurations {
        xml.push_str("\n  <HpcChannelConfiguration>");
        if let Some(ref id) = channel.id {
            xml.push_str(&format!("\n    <Id>{}</Id>", id));
        }
        xml.push_str(&format!(
            "\n    <ChannelId>{}</ChannelId>",
            channel.channel_id
        ));
        for event in &channel.events {
            xml.push_str(&format!("\n    <Event>{}</Event>", event));
        }
        if let Some(ref filter) = channel.filter {
            render_filter(&mut xml, filter);
        }
        xml.push_str("\n  </HpcChannelConfiguration>");
    }

    xml.push_str("\n</NotificationConfiguration>");
    xml
}

/// Render filter to XML
fn render_filter(xml: &mut String, filter: &FilterXml) {
    xml.push_str("\n    <Filter>");
    if let Some(ref s3_key) = filter.s3_key {
        xml.push_str("\n      <S3Key>");
        for rule in &s3_key.filter_rules {
            xml.push_str("\n        <FilterRule>");
            xml.push_str(&format!("\n          <Name>{}</Name>", rule.name));
            xml.push_str(&format!("\n          <Value>{}</Value>", rule.value));
            xml.push_str("\n        </FilterRule>");
        }
        xml.push_str("\n      </S3Key>");
    }
    xml.push_str("\n    </Filter>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_xml_parse() {
        let xml = r#"
        <NotificationConfiguration>
            <TopicConfiguration>
                <Id>image-uploads</Id>
                <Topic>arn:aws:sns:us-east-1:123456789012:image-notifications</Topic>
                <Event>s3:ObjectCreated:*</Event>
                <Filter>
                    <S3Key>
                        <FilterRule>
                            <Name>prefix</Name>
                            <Value>images/</Value>
                        </FilterRule>
                        <FilterRule>
                            <Name>suffix</Name>
                            <Value>.jpg</Value>
                        </FilterRule>
                    </S3Key>
                </Filter>
            </TopicConfiguration>
        </NotificationConfiguration>
        "#;

        let config: NotificationConfigurationXml = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(config.topic_configurations.len(), 1);
        assert_eq!(
            config.topic_configurations[0].id,
            Some("image-uploads".to_string())
        );
        assert!(config.topic_configurations[0].filter.is_some());
    }

    #[test]
    fn test_hpc_channel_config() {
        let xml = r#"
        <NotificationConfiguration>
            <HpcChannelConfiguration>
                <Id>ml-training-events</Id>
                <ChannelId>hpc.storage.events</ChannelId>
                <Event>s3:ObjectCreated:*</Event>
                <Event>s3:ObjectRemoved:*</Event>
                <Filter>
                    <S3Key>
                        <FilterRule>
                            <Name>prefix</Name>
                            <Value>checkpoints/</Value>
                        </FilterRule>
                    </S3Key>
                </Filter>
            </HpcChannelConfiguration>
        </NotificationConfiguration>
        "#;

        let xml_config: NotificationConfigurationXml = quick_xml::de::from_str(xml).unwrap();
        let config = xml_to_config(xml_config);

        assert_eq!(config.hpc_channel_configurations.len(), 1);
        assert_eq!(
            config.hpc_channel_configurations[0].channel_id,
            "hpc.storage.events"
        );
        assert_eq!(config.hpc_channel_configurations[0].events.len(), 2);
    }

    #[test]
    fn test_config_roundtrip() {
        let config = NotificationConfiguration {
            topic_configurations: vec![TopicConfiguration {
                id: "test-topic".to_string(),
                topic_arn: "arn:aws:sns:us-east-1:123:topic".to_string(),
                events: vec!["s3:ObjectCreated:*".to_string()],
                filter: Some(FilterRules {
                    key: KeyFilter {
                        filter_rules: vec![FilterRule {
                            name: "prefix".to_string(),
                            value: "logs/".to_string(),
                        }],
                    },
                }),
            }],
            ..Default::default()
        };

        let xml_config = config_to_xml(&config);
        let xml = render_notification_config(&xml_config);

        assert!(xml.contains("test-topic"));
        assert!(xml.contains("s3:ObjectCreated:*"));
        assert!(xml.contains("logs/"));
    }
}
