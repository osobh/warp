//! Session management for authenticated identities
//!
//! Provides token-based session management with configurable TTL.

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::identity::Identity;

/// Session token - opaque string for client use
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionToken(pub String);

impl SessionToken {
    /// Create from string
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Get the token string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An authenticated session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID
    pub id: String,

    /// The authenticated identity
    pub identity: Identity,

    /// Session token (for client use)
    pub token: SessionToken,

    /// When the session was created
    pub created_at: DateTime<Utc>,

    /// When the session expires
    pub expires_at: DateTime<Utc>,

    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,

    /// Refresh token (if available)
    pub refresh_token: Option<String>,

    /// Session metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl Session {
    /// Check if the session has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if the session is valid (not expired)
    pub fn is_valid(&self) -> bool {
        !self.is_expired()
    }

    /// Time until expiration
    pub fn time_until_expiry(&self) -> Duration {
        self.expires_at - Utc::now()
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }
}

/// Internal session data stored in the manager
#[derive(Debug, Clone)]
struct SessionData {
    session: Session,
    /// Whether this session has been invalidated
    invalidated: bool,
}

/// Session manager - creates and validates sessions
pub struct SessionManager {
    /// Active sessions (token -> session data)
    sessions: DashMap<String, SessionData>,

    /// Session ID to token mapping
    id_to_token: DashMap<String, String>,

    /// Session TTL in seconds
    ttl_seconds: u64,

    /// Signing key for tokens
    signing_key: SigningKey,

    /// Verifying key for tokens
    verifying_key: VerifyingKey,
}

impl SessionManager {
    /// Create a new session manager with the given TTL
    pub fn new(ttl_seconds: u64) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        Self {
            sessions: DashMap::new(),
            id_to_token: DashMap::new(),
            ttl_seconds,
            signing_key,
            verifying_key,
        }
    }

    /// Create a new session for an identity
    pub fn create_session(&self, identity: Identity) -> Result<Session> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires_at = now + Duration::seconds(self.ttl_seconds as i64);

        // Create token payload
        let payload = TokenPayload {
            session_id: session_id.clone(),
            identity_id: identity.id.clone(),
            issued_at: now.timestamp(),
            expires_at: expires_at.timestamp(),
        };

        // Sign the token
        let token = self.sign_token(&payload)?;

        let session = Session {
            id: session_id.clone(),
            identity,
            token: SessionToken::new(token.clone()),
            created_at: now,
            expires_at,
            last_activity: now,
            refresh_token: None,
            metadata: std::collections::HashMap::new(),
        };

        // Store session
        self.sessions.insert(token.clone(), SessionData {
            session: session.clone(),
            invalidated: false,
        });
        self.id_to_token.insert(session_id, token);

        Ok(session)
    }

    /// Get a session by token
    pub fn get_session(&self, token: &str) -> Result<Session> {
        let data = self.sessions.get(token)
            .ok_or_else(|| Error::SessionNotFound("Session not found".to_string()))?;

        if data.invalidated {
            return Err(Error::SessionNotFound("Session has been invalidated".to_string()));
        }

        if data.session.is_expired() {
            return Err(Error::SessionNotFound("Session has expired".to_string()));
        }

        // Verify token signature
        self.verify_token(token)?;

        Ok(data.session.clone())
    }

    /// Check if a session is valid by ID
    pub fn is_valid(&self, session_id: &str) -> bool {
        if let Some(token) = self.id_to_token.get(session_id) {
            if let Some(data) = self.sessions.get(token.value()) {
                return !data.invalidated && !data.session.is_expired();
            }
        }
        false
    }

    /// Invalidate a session by ID
    pub fn invalidate(&self, session_id: &str) -> Result<()> {
        let token = self.id_to_token.get(session_id)
            .ok_or_else(|| Error::SessionNotFound("Session not found".to_string()))?;

        if let Some(mut data) = self.sessions.get_mut(token.value()) {
            data.invalidated = true;
        }

        Ok(())
    }

    /// Invalidate a session by token
    pub fn invalidate_by_token(&self, token: &str) -> Result<()> {
        if let Some(mut data) = self.sessions.get_mut(token) {
            data.invalidated = true;
            Ok(())
        } else {
            Err(Error::SessionNotFound("Session not found".to_string()))
        }
    }

    /// Extend a session's expiry time
    pub fn extend_session(&self, token: &str) -> Result<Session> {
        let mut data = self.sessions.get_mut(token)
            .ok_or_else(|| Error::SessionNotFound("Session not found".to_string()))?;

        if data.invalidated {
            return Err(Error::SessionNotFound("Session has been invalidated".to_string()));
        }

        let now = Utc::now();
        data.session.expires_at = now + Duration::seconds(self.ttl_seconds as i64);
        data.session.last_activity = now;

        Ok(data.session.clone())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired(&self) -> usize {
        let now = Utc::now();
        let mut removed = 0;

        self.sessions.retain(|_, data| {
            let keep = !data.invalidated && data.session.expires_at > now;
            if !keep {
                removed += 1;
            }
            keep
        });

        // Also clean up id_to_token mapping
        self.id_to_token.retain(|_, token| {
            self.sessions.contains_key(token)
        });

        removed
    }

    /// Get the number of active sessions
    pub fn active_session_count(&self) -> usize {
        self.sessions.iter()
            .filter(|entry| !entry.invalidated && !entry.session.is_expired())
            .count()
    }

    /// Sign a token payload
    fn sign_token(&self, payload: &TokenPayload) -> Result<String> {
        let payload_json = serde_json::to_string(payload)
            .map_err(|e| Error::Internal(format!("Failed to serialize token: {}", e)))?;

        let signature = self.signing_key.sign(payload_json.as_bytes());

        // Base64 encode: payload.signature
        let payload_b64 = base64_encode(payload_json.as_bytes());
        let sig_b64 = base64_encode(signature.to_bytes().as_slice());

        Ok(format!("{}.{}", payload_b64, sig_b64))
    }

    /// Verify a token signature
    fn verify_token(&self, token: &str) -> Result<TokenPayload> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 2 {
            return Err(Error::InvalidToken("Invalid token format".to_string()));
        }

        let payload_bytes = base64_decode(parts[0])
            .map_err(|_| Error::InvalidToken("Invalid token encoding".to_string()))?;

        let sig_bytes = base64_decode(parts[1])
            .map_err(|_| Error::InvalidToken("Invalid signature encoding".to_string()))?;

        let signature = ed25519_dalek::Signature::from_slice(&sig_bytes)
            .map_err(|_| Error::InvalidToken("Invalid signature".to_string()))?;

        self.verifying_key.verify(&payload_bytes, &signature)
            .map_err(|_| Error::InvalidToken("Signature verification failed".to_string()))?;

        let payload: TokenPayload = serde_json::from_slice(&payload_bytes)
            .map_err(|_| Error::InvalidToken("Invalid token payload".to_string()))?;

        // Check expiration
        if payload.expires_at < Utc::now().timestamp() {
            return Err(Error::InvalidToken("Token has expired".to_string()));
        }

        Ok(payload)
    }
}

/// Token payload structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenPayload {
    session_id: String,
    identity_id: String,
    issued_at: i64,
    expires_at: i64,
}

/// Base64 encode helper
fn base64_encode(data: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = base64::write::EncoderWriter::new(&mut buf, &base64::engine::general_purpose::URL_SAFE_NO_PAD);
        encoder.write_all(data).unwrap();
    }
    String::from_utf8(buf).unwrap()
}

/// Base64 decode helper
fn base64_decode(data: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    #[test]
    fn test_create_session() {
        let manager = SessionManager::new(3600);
        let identity = Identity::user("test-user", "Test User", "local");

        let session = manager.create_session(identity).unwrap();

        assert!(!session.id.is_empty());
        assert!(!session.token.as_str().is_empty());
        assert!(session.is_valid());
    }

    #[test]
    fn test_get_session() {
        let manager = SessionManager::new(3600);
        let identity = Identity::user("test-user", "Test User", "local");

        let session = manager.create_session(identity).unwrap();
        let retrieved = manager.get_session(session.token.as_str()).unwrap();

        assert_eq!(session.id, retrieved.id);
        assert_eq!(session.identity.id, retrieved.identity.id);
    }

    #[test]
    fn test_invalidate_session() {
        let manager = SessionManager::new(3600);
        let identity = Identity::user("test-user", "Test User", "local");

        let session = manager.create_session(identity).unwrap();
        assert!(manager.is_valid(&session.id));

        manager.invalidate(&session.id).unwrap();
        assert!(!manager.is_valid(&session.id));
    }

    #[test]
    fn test_session_expiry() {
        let manager = SessionManager::new(0); // 0 second TTL
        let identity = Identity::user("test-user", "Test User", "local");

        let session = manager.create_session(identity).unwrap();

        // Session should be expired immediately
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(manager.get_session(session.token.as_str()).is_err());
    }

    #[test]
    fn test_extend_session() {
        let manager = SessionManager::new(60);
        let identity = Identity::user("test-user", "Test User", "local");

        let session = manager.create_session(identity).unwrap();
        let original_expiry = session.expires_at;

        std::thread::sleep(std::time::Duration::from_millis(10));

        let extended = manager.extend_session(session.token.as_str()).unwrap();
        assert!(extended.expires_at > original_expiry);
    }

    #[test]
    fn test_cleanup_expired() {
        let manager = SessionManager::new(0); // 0 second TTL
        let identity = Identity::user("test-user", "Test User", "local");

        manager.create_session(identity.clone()).unwrap();
        manager.create_session(identity.clone()).unwrap();
        manager.create_session(identity).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let removed = manager.cleanup_expired();
        assert_eq!(removed, 3);
        assert_eq!(manager.active_session_count(), 0);
    }
}
