//! Authentication for Portal Hub
//!
//! This module provides Ed25519 signature-based authentication for edge devices.
//! Authentication tokens are signed messages that prove edge ownership.
//!
//! # Authentication Flow
//!
//! 1. Edge registers with Hub (provides public key)
//! 2. For each request, edge signs a token with timestamp
//! 3. Hub verifies signature against registered public key
//! 4. Token must not be expired (within time window)
//!
//! # Security
//!
//! - Tokens expire after 5 minutes to prevent replay attacks
//! - Signatures are verified using Ed25519
//! - Each request must include fresh authentication

use crate::{storage::HubStorage, Error, Result};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

/// Authentication token expiration window (5 minutes)
const TOKEN_EXPIRATION_MINUTES: i64 = 5;

/// Authentication token for edge devices
///
/// Tokens must be signed by the edge's private key and include a timestamp
/// to prevent replay attacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    /// Edge identifier
    pub edge_id: Uuid,
    /// Token expiration timestamp
    pub expires_at: DateTime<Utc>,
    /// Ed25519 signature over the canonical message
    #[serde(with = "signature_serde")]
    pub signature: Signature,
}

impl AuthToken {
    /// Create a new authentication token
    ///
    /// # Arguments
    ///
    /// * `edge_id` - The edge device identifier
    /// * `signing_key` - The edge's Ed25519 signing key
    ///
    /// # Returns
    ///
    /// A new `AuthToken` with signature and expiration
    #[must_use]
    pub fn new(edge_id: Uuid, signing_key: &ed25519_dalek::SigningKey) -> Self {
        let expires_at = Utc::now() + Duration::minutes(TOKEN_EXPIRATION_MINUTES);
        let message = Self::canonical_message(edge_id, expires_at);
        let signature = signing_key.sign(&message);

        Self {
            edge_id,
            expires_at,
            signature,
        }
    }

    /// Verify the token signature and expiration
    ///
    /// # Arguments
    ///
    /// * `public_key` - The edge's public key for verification
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidSignature` if verification fails
    pub fn verify(&self, public_key: &VerifyingKey) -> Result<()> {
        // Check expiration
        if Utc::now() >= self.expires_at {
            debug!(edge_id = %self.edge_id, expires_at = %self.expires_at, "Auth token expired");
            return Err(Error::AuthFailed);
        }

        // Verify signature
        let message = Self::canonical_message(self.edge_id, self.expires_at);
        public_key.verify(&message, &self.signature).map_err(|e| {
            debug!(edge_id = %self.edge_id, error = %e, "Signature verification failed");
            Error::InvalidSignature
        })?;

        Ok(())
    }

    /// Generate canonical message for signing
    fn canonical_message(edge_id: Uuid, expires_at: DateTime<Utc>) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(edge_id.as_bytes());
        message.extend_from_slice(&expires_at.timestamp().to_le_bytes());
        message
    }
}

/// Extracted authenticated edge from request
///
/// This is used as an Axum extractor to automatically authenticate requests.
#[derive(Debug, Clone)]
pub struct AuthenticatedEdge {
    /// The authenticated edge identifier
    pub edge_id: Uuid,
    /// The edge's public key
    pub public_key: VerifyingKey,
}

impl AuthenticatedEdge {
    /// Extract authenticated edge from request parts
    ///
    /// # Errors
    ///
    /// Returns `AuthError` if authentication fails
    pub fn from_parts(parts: &Parts, storage: &Arc<HubStorage>) -> std::result::Result<Self, AuthError> {
        // Extract Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .ok_or(AuthError::MissingToken)?
            .to_str()
            .map_err(|e| {
                debug!("Invalid Authorization header encoding: {}", e);
                AuthError::InvalidToken
            })?;

        // Parse "Bearer <base64-encoded-token>"
        let token_str = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?;

        // Decode base64 token
        let token_bytes = base64_decode(token_str).map_err(|e| {
            debug!("Failed to decode base64 token: {}", e);
            AuthError::InvalidToken
        })?;

        // Deserialize token
        let token: AuthToken = serde_json::from_slice(&token_bytes).map_err(|e| {
            debug!("Failed to deserialize auth token: {}", e);
            AuthError::InvalidToken
        })?;

        // Get edge from storage
        let edge = storage.get_edge(&token.edge_id).map_err(|e| {
            warn!(edge_id = %token.edge_id, error = %e, "Edge lookup failed during auth");
            AuthError::EdgeNotFound
        })?;

        // Verify token
        token.verify(&edge.public_key)?;

        // Update last seen
        let _ = storage.update_edge_last_seen(&token.edge_id);

        Ok(Self {
            edge_id: token.edge_id,
            public_key: edge.public_key,
        })
    }
}

impl<S> FromRequestParts<S> for AuthenticatedEdge
where
    Arc<HubStorage>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl std::future::Future<Output = std::result::Result<Self, Self::Rejection>> + Send {
        let storage = <Arc<HubStorage> as FromRef<S>>::from_ref(state);
        async move { Self::from_parts(parts, &storage) }
    }
}

/// Authentication errors
#[derive(Debug)]
pub enum AuthError {
    /// Missing authentication token
    MissingToken,
    /// Invalid token format
    InvalidToken,
    /// Edge not found
    EdgeNotFound,
    /// Hub error
    HubError(Error),
}

impl From<Error> for AuthError {
    fn from(err: Error) -> Self {
        match err {
            Error::EdgeNotFound(_) => Self::EdgeNotFound,
            _ => Self::HubError(err),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::MissingToken => (StatusCode::UNAUTHORIZED, "Missing authorization token"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid authorization token"),
            Self::EdgeNotFound => (StatusCode::UNAUTHORIZED, "Edge not registered"),
            Self::HubError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Authentication error"),
        };

        (status, message).into_response()
    }
}

/// Base64 decode helper
fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s)
}

/// Base64 encode helper
fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Serialize auth token to Authorization header value
///
/// # Errors
///
/// Returns error if serialization fails
pub fn serialize_auth_token(token: &AuthToken) -> Result<String> {
    let token_json =
        serde_json::to_vec(token).map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(format!("Bearer {}", base64_encode(&token_json)))
}

// Signature serialization helpers
mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(signature: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&signature.to_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let array: [u8; 64] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid signature length"))?;
        Ok(Signature::from_bytes(&array))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn create_test_signing_key() -> (SigningKey, VerifyingKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    }

    #[test]
    fn test_auth_token_creation() {
        let (signing_key, _) = create_test_signing_key();
        let edge_id = Uuid::new_v4();

        let token = AuthToken::new(edge_id, &signing_key);
        assert_eq!(token.edge_id, edge_id);
        assert!(token.expires_at > Utc::now());
    }

    #[test]
    fn test_auth_token_verify_success() {
        let (signing_key, verifying_key) = create_test_signing_key();
        let edge_id = Uuid::new_v4();

        let token = AuthToken::new(edge_id, &signing_key);
        let result = token.verify(&verifying_key);
        assert!(result.is_ok());
    }

    #[test]
    fn test_auth_token_verify_wrong_key() {
        let (signing_key, _) = create_test_signing_key();
        let (_, wrong_key) = create_test_signing_key();
        let edge_id = Uuid::new_v4();

        let token = AuthToken::new(edge_id, &signing_key);
        let result = token.verify(&wrong_key);
        assert!(matches!(result, Err(Error::InvalidSignature)));
    }

    #[test]
    fn test_auth_token_expired() {
        let (signing_key, verifying_key) = create_test_signing_key();
        let edge_id = Uuid::new_v4();

        // Create token with past expiration
        let expires_at = Utc::now() - Duration::minutes(1);
        let message = AuthToken::canonical_message(edge_id, expires_at);
        let signature = signing_key.sign(&message);

        let token = AuthToken {
            edge_id,
            expires_at,
            signature,
        };

        let result = token.verify(&verifying_key);
        assert!(matches!(result, Err(Error::AuthFailed)));
    }

    #[test]
    fn test_canonical_message() {
        let edge_id = Uuid::new_v4();
        let expires_at = Utc::now();

        let msg1 = AuthToken::canonical_message(edge_id, expires_at);
        let msg2 = AuthToken::canonical_message(edge_id, expires_at);

        // Same inputs should produce same message
        assert_eq!(msg1, msg2);

        // Different edge_id should produce different message
        let different_id = Uuid::new_v4();
        let msg3 = AuthToken::canonical_message(different_id, expires_at);
        assert_ne!(msg1, msg3);
    }

    #[test]
    fn test_serialize_auth_token() {
        let (signing_key, _) = create_test_signing_key();
        let edge_id = Uuid::new_v4();
        let token = AuthToken::new(edge_id, &signing_key);

        let header = serialize_auth_token(&token).expect("Failed to serialize");
        assert!(header.starts_with("Bearer "));

        // Verify we can decode it back
        let token_str = header.strip_prefix("Bearer ").unwrap();
        let token_bytes = base64_decode(token_str).unwrap();
        let decoded: AuthToken = serde_json::from_slice(&token_bytes).unwrap();
        assert_eq!(decoded.edge_id, edge_id);
    }

    #[test]
    fn test_base64_encode_decode() {
        let data = b"Hello, World!";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_auth_error_into_response() {
        let err = AuthError::MissingToken;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let err = AuthError::InvalidToken;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let err = AuthError::EdgeNotFound;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let err = AuthError::HubError(Error::Storage("test".into()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_token_expiration_window() {
        let (signing_key, verifying_key) = create_test_signing_key();
        let edge_id = Uuid::new_v4();

        let token = AuthToken::new(edge_id, &signing_key);
        let expected_expiration = Utc::now() + Duration::minutes(TOKEN_EXPIRATION_MINUTES);

        // Should be within a second of expected
        let diff = (token.expires_at - expected_expiration).num_seconds().abs();
        assert!(diff <= 1);

        // Should verify successfully
        assert!(token.verify(&verifying_key).is_ok());
    }

    #[test]
    fn test_signature_serialization() {
        let (signing_key, _) = create_test_signing_key();
        let edge_id = Uuid::new_v4();
        let token = AuthToken::new(edge_id, &signing_key);

        // Serialize to JSON
        let json = serde_json::to_string(&token).expect("Failed to serialize");

        // Deserialize back
        let decoded: AuthToken = serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(decoded.edge_id, token.edge_id);
        assert_eq!(decoded.expires_at, token.expires_at);
        assert_eq!(decoded.signature.to_bytes(), token.signature.to_bytes());
    }

    #[test]
    fn test_token_tampering_detection() {
        let (signing_key, verifying_key) = create_test_signing_key();
        let edge_id = Uuid::new_v4();
        let mut token = AuthToken::new(edge_id, &signing_key);

        // Tamper with edge_id
        token.edge_id = Uuid::new_v4();

        // Should fail verification
        let result = token.verify(&verifying_key);
        assert!(matches!(result, Err(Error::InvalidSignature)));
    }

    #[test]
    fn test_authenticated_edge_creation() {
        let (_, verifying_key) = create_test_signing_key();
        let edge_id = Uuid::new_v4();

        let auth_edge = AuthenticatedEdge {
            edge_id,
            public_key: verifying_key,
        };

        assert_eq!(auth_edge.edge_id, edge_id);
        assert_eq!(auth_edge.public_key, verifying_key);
    }

    // Note: Extractor tests are covered by integration tests in server.rs
    // Testing extractors in isolation requires more complex setup with Axum's state system
}
