//! AWS KMS integration
//!
//! This module is a placeholder for AWS KMS integration.
//! Enable with `--features aws` once aws-sdk-kms bindings are implemented.

use crate::{DataKey, KeyMetadata, KmsError, KmsResult, MasterKey};
use async_trait::async_trait;

/// AWS KMS provider configuration
#[derive(Debug, Clone)]
pub struct AwsKmsConfig {
    /// AWS region
    pub region: String,
    /// Optional endpoint override (for LocalStack)
    pub endpoint: Option<String>,
}

/// AWS KMS provider
pub struct AwsKms {
    #[allow(dead_code)]
    config: AwsKmsConfig,
}

impl AwsKms {
    /// Create a new AWS KMS provider
    ///
    /// # Errors
    /// Returns error if AWS credentials are not configured
    pub async fn new(config: AwsKmsConfig) -> KmsResult<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl crate::KmsProvider for AwsKms {
    async fn create_key(&self, _algorithm: crate::KeyAlgorithm) -> KmsResult<MasterKey> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }

    async fn get_key(&self, _key_id: &str) -> KmsResult<Option<MasterKey>> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }

    async fn generate_data_key(&self, _master_key_id: &str) -> KmsResult<DataKey> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }

    async fn decrypt_data_key(
        &self,
        _master_key_id: &str,
        _encrypted_key: &[u8],
    ) -> KmsResult<Vec<u8>> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }

    async fn list_keys(&self) -> KmsResult<Vec<KeyMetadata>> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }

    async fn rotate_key(&self, _key_id: &str) -> KmsResult<MasterKey> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }

    async fn delete_key(&self, _key_id: &str) -> KmsResult<()> {
        Err(KmsError::NotSupported(
            "AWS KMS provider not yet implemented".to_string(),
        ))
    }
}
