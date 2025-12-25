//! WARP IPC Types and Commands for Horizon Integration
//!
//! This crate provides shared types for Inter-Process Communication
//! between the WARP backend and the Horizon Tauri frontend.
//!
//! # Overview
//!
//! The IPC layer enables Horizon to:
//! - Query transfer state and metrics
//! - Control active transfers (pause, resume, cancel)
//! - Subscribe to real-time events
//! - Access edge topology information
//! - View scheduler metrics
//!
//! # Usage
//!
//! ```rust
//! use warp_ipc::{commands::*, events::*};
//!
//! // Create a command
//! let cmd = IpcCommand::GetTransfers;
//!
//! // Create an event
//! let event = IpcEvent::TransferProgress {
//!     transfer_id: "abc-123".into(),
//!     bytes_transferred: 1_000_000,
//!     total_bytes: 10_000_000,
//!     speed_bps: 100_500_000,
//!     progress_percent: 10.0,
//!     eta_seconds: Some(90),
//! };
//! ```

#![warn(missing_docs)]

pub mod commands;
pub mod events;
pub mod types;

pub use commands::*;
pub use events::*;
pub use types::*;

use thiserror::Error;

/// IPC error types
#[derive(Debug, Error)]
pub enum IpcError {
    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Command not found
    #[error("Unknown command: {0}")]
    UnknownCommand(String),

    /// Invalid parameters
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    /// Transfer not found
    #[error("Transfer not found: {0}")]
    TransferNotFound(String),

    /// Edge not found
    #[error("Edge not found: {0}")]
    EdgeNotFound(String),

    /// Operation not permitted
    #[error("Operation not permitted: {0}")]
    NotPermitted(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// IPC Result type
pub type IpcResult<T> = Result<T, IpcError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = IpcError::TransferNotFound("abc-123".to_string());
        assert_eq!(err.to_string(), "Transfer not found: abc-123");
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err = IpcError::from(json_err);
        assert!(matches!(err, IpcError::Serialization(_)));
    }
}
