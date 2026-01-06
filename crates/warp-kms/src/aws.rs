//! AWS KMS integration
//!
//! Provides envelope encryption using AWS Key Management Service.

use crate::{DataKey, KeyMetadata, KeyState, KmsError, KmsResult};
use async_trait::async_trait;

/// AWS KMS provider configuration
#[derive(Debug, Clone)]
pub struct AwsKmsConfig {
    /// AWS region
    pub region: String,
    /// Optional endpoint override (for LocalStack or VPC endpoints)
    pub endpoint: Option<String>,
}

impl Default for AwsKmsConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            endpoint: None,
        }
    }
}

/// AWS KMS provider
#[cfg(feature = "aws")]
pub struct AwsKms {
    client: aws_sdk_kms::Client,
}

#[cfg(feature = "aws")]
impl AwsKms {
    /// Create a new AWS KMS provider
    pub async fn new(config: AwsKmsConfig) -> KmsResult<Self> {
        use aws_config::BehaviorVersion;

        let mut aws_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(config.region));

        if let Some(endpoint) = config.endpoint {
            aws_config = aws_config.endpoint_url(endpoint);
        }

        let sdk_config = aws_config.load().await;
        let client = aws_sdk_kms::Client::new(&sdk_config);

        Ok(Self { client })
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl crate::KmsProvider for AwsKms {
    async fn create_key(&self, alias: &str) -> KmsResult<String> {
        let result = self
            .client
            .create_key()
            .description(format!("WARP encryption key: {}", alias))
            .key_usage(aws_sdk_kms::types::KeyUsageType::EncryptDecrypt)
            .key_spec(aws_sdk_kms::types::KeySpec::SymmetricDefault)
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS CreateKey failed: {}", e)))?;

        let key_id = result
            .key_metadata()
            .and_then(|m| m.key_id())
            .ok_or_else(|| KmsError::Provider("No key ID returned".to_string()))?
            .to_string();

        // Create alias for the key
        let alias_name = format!("alias/{}", alias);
        self.client
            .create_alias()
            .alias_name(&alias_name)
            .target_key_id(&key_id)
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS CreateAlias failed: {}", e)))?;

        Ok(key_id)
    }

    async fn generate_data_key(&self, key_id: &str) -> KmsResult<DataKey> {
        let result = self
            .client
            .generate_data_key()
            .key_id(key_id)
            .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS GenerateDataKey failed: {}", e)))?;

        let plaintext = result
            .plaintext()
            .ok_or_else(|| KmsError::Provider("No plaintext returned".to_string()))?
            .as_ref()
            .to_vec();

        let ciphertext = result
            .ciphertext_blob()
            .ok_or_else(|| KmsError::Provider("No ciphertext returned".to_string()))?
            .as_ref()
            .to_vec();

        Ok(DataKey {
            plaintext,
            ciphertext,
            key_id: key_id.to_string(),
        })
    }

    async fn decrypt_data_key(&self, key_id: &str, ciphertext: &[u8]) -> KmsResult<Vec<u8>> {
        let result = self
            .client
            .decrypt()
            .key_id(key_id)
            .ciphertext_blob(aws_sdk_kms::primitives::Blob::new(ciphertext))
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS Decrypt failed: {}", e)))?;

        let plaintext = result
            .plaintext()
            .ok_or_else(|| KmsError::Provider("No plaintext returned".to_string()))?
            .as_ref()
            .to_vec();

        Ok(plaintext)
    }

    async fn encrypt(&self, key_id: &str, plaintext: &[u8]) -> KmsResult<Vec<u8>> {
        let result = self
            .client
            .encrypt()
            .key_id(key_id)
            .plaintext(aws_sdk_kms::primitives::Blob::new(plaintext))
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS Encrypt failed: {}", e)))?;

        let ciphertext = result
            .ciphertext_blob()
            .ok_or_else(|| KmsError::Provider("No ciphertext returned".to_string()))?
            .as_ref()
            .to_vec();

        Ok(ciphertext)
    }

    async fn decrypt(&self, key_id: &str, ciphertext: &[u8]) -> KmsResult<Vec<u8>> {
        self.decrypt_data_key(key_id, ciphertext).await
    }

    async fn rotate_key(&self, key_id: &str) -> KmsResult<String> {
        // Enable automatic key rotation
        self.client
            .enable_key_rotation()
            .key_id(key_id)
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS EnableKeyRotation failed: {}", e)))?;

        Ok(key_id.to_string())
    }

    async fn get_key_metadata(&self, key_id: &str) -> KmsResult<KeyMetadata> {
        let result = self
            .client
            .describe_key()
            .key_id(key_id)
            .send()
            .await
            .map_err(|e| KmsError::Provider(format!("AWS KMS DescribeKey failed: {}", e)))?;

        let metadata = result
            .key_metadata()
            .ok_or_else(|| KmsError::Provider("No metadata returned".to_string()))?;

        let state = match metadata.key_state() {
            Some(aws_sdk_kms::types::KeyState::Enabled) => KeyState::Enabled,
            Some(aws_sdk_kms::types::KeyState::Disabled) => KeyState::Disabled,
            Some(aws_sdk_kms::types::KeyState::PendingDeletion) => KeyState::PendingDeletion,
            _ => KeyState::Disabled,
        };

        let created_at = metadata
            .creation_date()
            .map(|d| {
                chrono::DateTime::from_timestamp(d.secs(), d.subsec_nanos())
                    .unwrap_or_else(chrono::Utc::now)
            })
            .unwrap_or_else(chrono::Utc::now);

        Ok(KeyMetadata {
            key_id: metadata.key_id().unwrap_or_default().to_string(),
            description: metadata.description().map(|s| s.to_string()),
            created_at,
            state,
            rotation_enabled: metadata.key_rotation_enabled(),
        })
    }

    async fn list_keys(&self) -> KmsResult<Vec<String>> {
        let mut keys = Vec::new();
        let mut marker: Option<String> = None;

        loop {
            let mut request = self.client.list_keys();
            if let Some(m) = marker.take() {
                request = request.marker(m);
            }

            let result = request
                .send()
                .await
                .map_err(|e| KmsError::Provider(format!("AWS KMS ListKeys failed: {}", e)))?;

            if let Some(key_list) = result.keys() {
                for key in key_list {
                    if let Some(key_id) = key.key_id() {
                        keys.push(key_id.to_string());
                    }
                }
            }

            if result.truncated() {
                marker = result.next_marker().map(|s| s.to_string());
            } else {
                break;
            }
        }

        Ok(keys)
    }

    async fn schedule_key_deletion(&self, key_id: &str) -> KmsResult<()> {
        self.client
            .schedule_key_deletion()
            .key_id(key_id)
            .pending_window_in_days(7) // Minimum waiting period
            .send()
            .await
            .map_err(|e| {
                KmsError::Provider(format!("AWS KMS ScheduleKeyDeletion failed: {}", e))
            })?;

        Ok(())
    }
}

// Stub implementation when AWS feature is not enabled
#[cfg(not(feature = "aws"))]
pub struct AwsKms {
    #[allow(dead_code)]
    config: AwsKmsConfig,
}

#[cfg(not(feature = "aws"))]
impl AwsKms {
    /// Create a new AWS KMS provider (stub)
    pub async fn new(config: AwsKmsConfig) -> KmsResult<Self> {
        Ok(Self { config })
    }
}

#[cfg(not(feature = "aws"))]
#[async_trait]
impl crate::KmsProvider for AwsKms {
    async fn create_key(&self, _alias: &str) -> KmsResult<String> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn generate_data_key(&self, _key_id: &str) -> KmsResult<DataKey> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn decrypt_data_key(&self, _key_id: &str, _ciphertext: &[u8]) -> KmsResult<Vec<u8>> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn encrypt(&self, _key_id: &str, _plaintext: &[u8]) -> KmsResult<Vec<u8>> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn decrypt(&self, _key_id: &str, _ciphertext: &[u8]) -> KmsResult<Vec<u8>> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn rotate_key(&self, _key_id: &str) -> KmsResult<String> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn get_key_metadata(&self, _key_id: &str) -> KmsResult<KeyMetadata> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn list_keys(&self) -> KmsResult<Vec<String>> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }

    async fn schedule_key_deletion(&self, _key_id: &str) -> KmsResult<()> {
        Err(KmsError::NotSupported(
            "AWS KMS requires the 'aws' feature".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KmsProvider;

    #[tokio::test]
    async fn test_aws_kms_config_default() {
        let config = AwsKmsConfig::default();
        assert_eq!(config.region, "us-east-1");
        assert!(config.endpoint.is_none());
    }

    #[tokio::test]
    async fn test_aws_kms_config_custom() {
        let config = AwsKmsConfig {
            region: "eu-west-1".to_string(),
            endpoint: Some("http://localhost:4566".to_string()),
        };
        assert_eq!(config.region, "eu-west-1");
        assert_eq!(config.endpoint.unwrap(), "http://localhost:4566");
    }

    #[tokio::test]
    #[cfg(not(feature = "aws"))]
    async fn test_aws_kms_stub_returns_error() {
        let config = AwsKmsConfig::default();
        let kms = AwsKms::new(config).await.unwrap();

        let result = kms.create_key("test").await;
        assert!(matches!(result, Err(KmsError::NotSupported(_))));

        let result = kms.generate_data_key("test").await;
        assert!(matches!(result, Err(KmsError::NotSupported(_))));

        let result = kms.list_keys().await;
        assert!(matches!(result, Err(KmsError::NotSupported(_))));
    }
}
