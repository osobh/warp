//! Content-blind deduplication using OPRF
//!
//! This module enables storage deduplication without the server seeing content hashes.
//! Uses Oblivious Pseudorandom Functions (OPRF) to derive deterministic dedup tokens
//! that the server cannot link back to the original content.
//!
//! # Architecture
//!
//! ```text
//! Client                          Storage Server
//!   |                                   |
//!   | 1. Chunk data with Chonkers       |
//!   | 2. Compute chunk hash (BLAKE3)    |
//!   | 3. Blind hash: H(chunk)^r         |
//!   |-------- blinded request --------->|
//!   |                                   | 4. Evaluate: (H^r)^k
//!   |<------- evaluation ---------------|
//!   | 5. Unblind: H(chunk)^k            |
//!   | 6. Derive DedupToken              |
//!   |-------- lookup token ------------>|
//!   |                                   | 7. Check DedupIndex
//!   |<------- exists/new ---------------|
//!   | 8. Store chunk if new             |
//! ```
//!
//! # Security Properties
//!
//! - **Content confidentiality**: Server never sees content hashes
//! - **Determinism**: Same content + server key = same token (enables dedup)
//! - **Key isolation**: Different server keys produce different tokens
//! - **Unforgeability**: Client cannot generate tokens without server participation

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::debug;

use warp_oprf::dedup::{
    BlindDedupClient, BlindDedupServer, DedupIndex, DedupReference, DedupToken,
};
use warp_oprf::oprf::{BlindedInput, Evaluation};

use crate::error::{Error, Result};

/// Configuration for blind deduplication
#[derive(Debug, Clone)]
pub struct BlindDedupConfig {
    /// Key ID for the OPRF server (for key rotation tracking)
    pub key_id: String,

    /// Whether to enable batch OPRF operations
    pub batch_enabled: bool,

    /// Maximum batch size for OPRF operations
    pub max_batch_size: usize,

    /// Path to persist dedup index (if using sled)
    pub index_path: Option<PathBuf>,
}

impl Default for BlindDedupConfig {
    fn default() -> Self {
        Self {
            key_id: "dedup-key-v1".to_string(),
            batch_enabled: true,
            max_batch_size: 256,
            index_path: None,
        }
    }
}

impl BlindDedupConfig {
    /// Create a new config with the given key ID
    pub fn new(key_id: impl Into<String>) -> Self {
        Self {
            key_id: key_id.into(),
            ..Default::default()
        }
    }

    /// Set the index path for persistent storage
    pub fn with_index_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.index_path = Some(path.into());
        self
    }

    /// Disable batch processing
    pub fn without_batch(mut self) -> Self {
        self.batch_enabled = false;
        self
    }
}

/// Async trait for OPRF server operations
///
/// This abstraction allows the OPRF server to be:
/// - Embedded in the same process
/// - Running as a separate local service
/// - Remote service accessed over network
#[async_trait]
pub trait BlindDedupService: Send + Sync + 'static {
    /// Get the server's public key
    fn public_key(&self) -> Vec<u8>;

    /// Get the current key ID
    fn key_id(&self) -> &str;

    /// Evaluate a single blinded input
    async fn evaluate(&self, blinded: &BlindedInput) -> Result<Evaluation>;

    /// Evaluate a batch of blinded inputs (more efficient)
    async fn evaluate_batch(&self, blinded: &[BlindedInput]) -> Result<Vec<Evaluation>>;
}

/// Embedded OPRF service (runs in-process)
pub struct EmbeddedDedupService {
    server: BlindDedupServer,
}

impl EmbeddedDedupService {
    /// Create a new embedded dedup service with a random key
    pub fn new(key_id: impl Into<String>) -> Result<Self> {
        let server = BlindDedupServer::new(key_id)
            .map_err(|e| Error::Backend(format!("Failed to create dedup server: {}", e)))?;
        Ok(Self { server })
    }

    /// Create from an existing secret key (for persistence)
    pub fn from_secret_key(secret_key: &[u8], key_id: impl Into<String>) -> Result<Self> {
        let server = BlindDedupServer::from_secret_key(secret_key, key_id)
            .map_err(|e| Error::Backend(format!("Failed to restore dedup server: {}", e)))?;
        Ok(Self { server })
    }

    /// Export the secret key for backup
    pub fn export_secret_key(&self) -> Vec<u8> {
        self.server.export_secret_key()
    }
}

#[async_trait]
impl BlindDedupService for EmbeddedDedupService {
    fn public_key(&self) -> Vec<u8> {
        self.server.public_key()
    }

    fn key_id(&self) -> &str {
        self.server.key_id()
    }

    async fn evaluate(&self, blinded: &BlindedInput) -> Result<Evaluation> {
        // OPRF operations are CPU-bound but fast, run inline
        self.server
            .evaluate(blinded)
            .map_err(|e| Error::Backend(format!("OPRF evaluation failed: {}", e)))
    }

    async fn evaluate_batch(&self, blinded: &[BlindedInput]) -> Result<Vec<Evaluation>> {
        self.server
            .evaluate_batch(blinded)
            .map_err(|e| Error::Backend(format!("OPRF batch evaluation failed: {}", e)))
    }
}

/// Sled-backed persistent dedup index
pub struct SledDedupIndex {
    tree: sled::Tree,
}

impl SledDedupIndex {
    /// Open or create a persistent dedup index at the given path
    pub fn new(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let db =
            sled::open(path).map_err(|e| Error::Backend(format!("Failed to open sled: {}", e)))?;
        let tree = db
            .open_tree("dedup_index")
            .map_err(|e| Error::Backend(format!("Failed to open tree: {}", e)))?;
        Ok(Self { tree })
    }

    /// Create an in-memory dedup index (for testing)
    pub fn in_memory() -> Result<Self> {
        let config = sled::Config::new().temporary(true);
        let db = config
            .open()
            .map_err(|e| Error::Backend(format!("Failed to create temp sled: {}", e)))?;
        let tree = db
            .open_tree("dedup_index")
            .map_err(|e| Error::Backend(format!("Failed to open tree: {}", e)))?;
        Ok(Self { tree })
    }

    /// Get the number of entries in the index
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }
}

#[async_trait]
impl DedupIndex for SledDedupIndex {
    async fn lookup(&self, token: &DedupToken) -> warp_oprf::error::Result<Option<DedupReference>> {
        let tree = self.tree.clone();
        let token_bytes = token.as_bytes().to_vec();

        tokio::task::spawn_blocking(move || match tree.get(&token_bytes) {
            Ok(Some(value)) => {
                let reference: DedupReference = rmp_serde::from_slice(&value)
                    .map_err(|e| warp_oprf::error::OprfError::Internal(e.to_string()))?;
                Ok(Some(reference))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(warp_oprf::error::OprfError::Internal(e.to_string())),
        })
        .await
        .map_err(|e| warp_oprf::error::OprfError::Internal(e.to_string()))?
    }

    async fn store(
        &self,
        token: &DedupToken,
        reference: DedupReference,
    ) -> warp_oprf::error::Result<()> {
        let tree = self.tree.clone();
        let token_bytes = token.as_bytes().to_vec();
        let value = rmp_serde::to_vec(&reference)
            .map_err(|e| warp_oprf::error::OprfError::Internal(e.to_string()))?;

        tokio::task::spawn_blocking(move || {
            tree.insert(&token_bytes, value)
                .map_err(|e| warp_oprf::error::OprfError::Internal(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| warp_oprf::error::OprfError::Internal(e.to_string()))?
    }

    async fn remove(&self, token: &DedupToken) -> warp_oprf::error::Result<bool> {
        let tree = self.tree.clone();
        let token_bytes = token.as_bytes().to_vec();

        tokio::task::spawn_blocking(move || match tree.remove(&token_bytes) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(warp_oprf::error::OprfError::Internal(e.to_string())),
        })
        .await
        .map_err(|e| warp_oprf::error::OprfError::Internal(e.to_string()))?
    }
}

/// Context for blind deduplication operations in ChonkersBackend
pub(crate) struct BlindDedupContext {
    /// OPRF service handle
    pub service: Arc<dyn BlindDedupService>,

    /// Client for blinding operations
    pub client: BlindDedupClient,

    /// Dedup token index
    pub index: Arc<dyn DedupIndex + Send + Sync>,

    /// Configuration
    pub config: BlindDedupConfig,
}

impl BlindDedupContext {
    /// Create a new blind dedup context
    pub fn new(
        service: Arc<dyn BlindDedupService>,
        index: Arc<dyn DedupIndex + Send + Sync>,
        config: BlindDedupConfig,
    ) -> Result<Self> {
        let client = BlindDedupClient::new(&service.public_key())
            .map_err(|e| Error::Backend(format!("Failed to create dedup client: {}", e)))?;

        Ok(Self {
            service,
            client,
            index,
            config,
        })
    }

    /// Blind a chunk hash and get a dedup token
    pub async fn get_dedup_token(&self, chunk_hash: &[u8]) -> Result<DedupToken> {
        // Step 1: Blind the hash
        let (blinded, state) = self
            .client
            .blind_hash(chunk_hash)
            .map_err(|e| Error::Backend(format!("Failed to blind hash: {}", e)))?;

        // Step 2: Get server evaluation
        let evaluation = self.service.evaluate(&blinded).await?;

        // Step 3: Finalize to get token
        let token = self
            .client
            .finalize(state, &evaluation)
            .map_err(|e| Error::Backend(format!("Failed to finalize OPRF: {}", e)))?;

        Ok(token)
    }

    /// Check if a chunk exists in the dedup index
    pub async fn lookup(&self, token: &DedupToken) -> Result<Option<DedupReference>> {
        self.index
            .lookup(token)
            .await
            .map_err(|e| Error::Backend(format!("Dedup lookup failed: {}", e)))
    }

    /// Store a dedup reference
    pub async fn store(&self, token: &DedupToken, reference: DedupReference) -> Result<()> {
        self.index
            .store(token, reference)
            .await
            .map_err(|e| Error::Backend(format!("Failed to store dedup reference: {}", e)))
    }

    /// Process a batch of chunk hashes
    pub async fn get_dedup_tokens_batch(&self, chunk_hashes: &[&[u8]]) -> Result<Vec<DedupToken>> {
        // Step 1: Blind all hashes
        let mut blinded_inputs = Vec::with_capacity(chunk_hashes.len());
        let mut states = Vec::with_capacity(chunk_hashes.len());

        for hash in chunk_hashes {
            let (blinded, state) = self
                .client
                .blind_hash(hash)
                .map_err(|e| Error::Backend(format!("Failed to blind hash: {}", e)))?;
            blinded_inputs.push(blinded);
            states.push(state);
        }

        // Step 2: Batch evaluate
        let evaluations = self.service.evaluate_batch(&blinded_inputs).await?;

        // Step 3: Finalize all
        let mut tokens = Vec::with_capacity(chunk_hashes.len());
        for (evaluation, state) in evaluations.iter().zip(states.into_iter()) {
            let token = self
                .client
                .finalize(state, evaluation)
                .map_err(|e| Error::Backend(format!("Failed to finalize OPRF: {}", e)))?;
            tokens.push(token);
        }

        Ok(tokens)
    }
}

/// Statistics for blind deduplication
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlindDedupStats {
    /// Current key ID
    pub key_id: String,

    /// Number of dedup hits
    pub dedup_hits: u64,

    /// Number of new chunks stored
    pub new_chunks: u64,

    /// Total bytes saved from deduplication
    pub bytes_saved: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp_oprf::dedup::MemoryDedupIndex;

    #[tokio::test]
    async fn test_embedded_service() {
        let service = EmbeddedDedupService::new("test-key").unwrap();
        assert!(!service.public_key().is_empty());
        assert_eq!(service.key_id(), "test-key");
    }

    #[tokio::test]
    async fn test_service_persistence() {
        let service1 = EmbeddedDedupService::new("test-key").unwrap();
        let sk = service1.export_secret_key();
        let pk1 = service1.public_key();

        let service2 = EmbeddedDedupService::from_secret_key(&sk, "test-key").unwrap();
        let pk2 = service2.public_key();

        assert_eq!(pk1, pk2);
    }

    #[tokio::test]
    async fn test_sled_index() {
        let index = SledDedupIndex::in_memory().unwrap();
        let token = DedupToken::from_bytes([0x42; 32]);
        let reference = DedupReference::new("bucket/key", 1024, "key-v1");

        // Store
        index.store(&token, reference.clone()).await.unwrap();
        assert_eq!(index.len(), 1);

        // Lookup
        let found = index.lookup(&token).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().object_key, "bucket/key");

        // Remove
        let removed = index.remove(&token).await.unwrap();
        assert!(removed);
        assert!(index.is_empty());
    }

    #[tokio::test]
    async fn test_blind_dedup_context() {
        let service = Arc::new(EmbeddedDedupService::new("test-key").unwrap());
        let index = Arc::new(MemoryDedupIndex::new());
        let config = BlindDedupConfig::new("test-key");

        let ctx = BlindDedupContext::new(service, index, config).unwrap();

        // Get token for some content
        let content_hash = warp_hash::hash(b"test content");
        let token1 = ctx.get_dedup_token(&content_hash).await.unwrap();

        // Same content should produce same token
        let token2 = ctx.get_dedup_token(&content_hash).await.unwrap();
        assert_eq!(token1, token2);

        // Different content should produce different token
        let other_hash = warp_hash::hash(b"other content");
        let token3 = ctx.get_dedup_token(&other_hash).await.unwrap();
        assert_ne!(token1, token3);
    }

    #[tokio::test]
    async fn test_batch_tokens() {
        let service = Arc::new(EmbeddedDedupService::new("test-key").unwrap());
        let index = Arc::new(MemoryDedupIndex::new());
        let config = BlindDedupConfig::new("test-key");

        let ctx = BlindDedupContext::new(service, index, config).unwrap();

        let hash1 = warp_hash::hash(b"content 1");
        let hash2 = warp_hash::hash(b"content 2");
        let hash3 = warp_hash::hash(b"content 3");

        let hashes: Vec<&[u8]> = vec![&hash1, &hash2, &hash3];
        let tokens = ctx.get_dedup_tokens_batch(&hashes).await.unwrap();

        assert_eq!(tokens.len(), 3);

        // Verify determinism - individual tokens should match batch
        let single_token = ctx.get_dedup_token(&hash1).await.unwrap();
        assert_eq!(tokens[0], single_token);
    }
}
