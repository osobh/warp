//! Error types for IAM operations

use thiserror::Error;

/// IAM error type
#[derive(Debug, Error)]
pub enum Error {
    /// Provider not found
    #[error("Identity provider not found: {0}")]
    ProviderNotFound(String),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Authorization denied
    #[error("Authorization denied: {0}")]
    AuthorizationDenied(String),

    /// Invalid policy document
    #[error("Invalid policy: {0}")]
    InvalidPolicy(String),

    /// Session not found or expired
    #[error("Session not found or expired: {0}")]
    SessionNotFound(String),

    /// Invalid token
    #[error("Invalid token: {0}")]
    InvalidToken(String),

    /// OIDC error
    #[error("OIDC error: {0}")]
    Oidc(String),

    /// LDAP error
    #[error("LDAP error: {0}")]
    Ldap(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for IAM operations
pub type Result<T> = std::result::Result<T, Error>;
