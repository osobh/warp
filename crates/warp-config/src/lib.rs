//! Configuration Management for Portal Distributed Storage
//!
//! This crate provides:
//! - File-based configuration (TOML)
//! - Environment variable overrides
//! - Configuration validation
//! - Hot-reload support

pub mod config;
pub mod validate;

pub use config::*;
pub use validate::*;

use thiserror::Error;

/// Configuration error types
#[derive(Debug, Error)]
pub enum ConfigError {
    /// File I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error
    #[error("Parse error: {0}")]
    Parse(String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid value
    #[error("Invalid value for {field}: {message}")]
    InvalidValue {
        /// Field name
        field: String,
        /// Error message
        message: String,
    },

    /// Environment variable error
    #[error("Environment variable error: {0}")]
    EnvVar(String),
}

/// Result type for configuration operations
pub type Result<T> = std::result::Result<T, ConfigError>;
