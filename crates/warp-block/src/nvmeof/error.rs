//! NVMe-oF error types
//!
//! Error types for NVMe-over-Fabrics operations.

use std::io;
use thiserror::Error;

/// Result type for NVMe-oF operations
pub type NvmeOfResult<T> = Result<T, NvmeOfError>;

/// NVMe-oF error types
#[derive(Debug, Error)]
pub enum NvmeOfError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Protocol error
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Invalid NVMe command
    #[error("Invalid command: opcode={opcode:#x}, status={status:#x}")]
    InvalidCommand {
        /// Command opcode
        opcode: u8,
        /// Status code
        status: u16,
    },

    /// Invalid capsule format
    #[error("Invalid capsule: {0}")]
    InvalidCapsule(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Subsystem error
    #[error("Subsystem error: {0}")]
    Subsystem(String),

    /// Namespace error
    #[error("Namespace error: {0}")]
    Namespace(String),

    /// Queue error
    #[error("Queue error: {0}")]
    Queue(String),

    /// Transport error
    #[error("Transport error: {0}")]
    Transport(String),

    /// Authentication error
    #[error("Authentication error: {0}")]
    Auth(String),

    /// NQN (NVMe Qualified Name) error
    #[error("Invalid NQN: {0}")]
    InvalidNqn(String),

    /// Resource exhausted
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Timeout error
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Disconnected
    #[error("Disconnected: {0}")]
    Disconnected(String),

    /// Not supported
    #[error("Not supported: {0}")]
    NotSupported(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Underlying storage error
    #[error("Storage error: {0}")]
    Storage(#[from] crate::error::BlockError),
}

/// NVMe status codes (NVM Express Base Specification)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum NvmeStatus {
    /// Command completed successfully
    Success = 0x0000,

    /// Invalid command opcode
    InvalidOpcode = 0x0001,

    /// Invalid field in command
    InvalidField = 0x0002,

    /// Command ID conflict
    CommandIdConflict = 0x0003,

    /// Data transfer error
    DataTransferError = 0x0004,

    /// Commands aborted due to power loss notification
    PowerLoss = 0x0005,

    /// Internal error
    InternalError = 0x0006,

    /// Command abort requested
    CommandAbortRequested = 0x0007,

    /// Command aborted due to SQ deletion
    SqDeletionAbort = 0x0008,

    /// Command aborted due to failed fused command
    FusedFail = 0x0009,

    /// Command aborted due to missing fused command
    FusedMissing = 0x000A,

    /// Invalid namespace or format
    InvalidNamespaceOrFormat = 0x000B,

    /// Command sequence error
    CommandSequenceError = 0x000C,

    /// Invalid SGL segment descriptor
    InvalidSglSegment = 0x000D,

    /// Invalid number of SGL descriptors
    InvalidSglCount = 0x000E,

    /// Data SGL length invalid
    DataSglLengthInvalid = 0x000F,

    /// Metadata SGL length invalid
    MetadataSglLengthInvalid = 0x0010,

    /// SGL descriptor type invalid
    SglTypeInvalid = 0x0011,

    /// Invalid use of controller memory buffer
    InvalidCmbUse = 0x0012,

    /// PRP offset invalid
    PrpOffsetInvalid = 0x0013,

    /// Atomic write unit exceeded
    AtomicWriteUnitExceeded = 0x0014,

    /// Operation denied
    OperationDenied = 0x0015,

    /// SGL offset invalid
    SglOffsetInvalid = 0x0016,

    /// Host identifier inconsistent format
    HostIdInconsistentFormat = 0x0018,

    /// Keep alive timeout expired
    KeepAliveExpired = 0x0019,

    /// Keep alive timeout invalid
    KeepAliveTimeoutInvalid = 0x001A,

    /// Command aborted due to preempt and abort
    PreemptAbort = 0x001B,

    /// Sanitize failed
    SanitizeFailed = 0x001C,

    /// Sanitize in progress
    SanitizeInProgress = 0x001D,

    /// SGL data block granularity invalid
    SglDataBlockGranularityInvalid = 0x001E,

    /// Command not supported for queue in CMB
    CommandNotSupportedForQueueInCmb = 0x001F,

    /// Namespace is write protected
    NamespaceWriteProtected = 0x0020,

    /// Command interrupted
    CommandInterrupted = 0x0021,

    /// Transient transport error
    TransientTransportError = 0x0022,

    // Fabric-specific status codes (0x1Bxx - 0x1Fxx)
    /// Incompatible format
    IncompatibleFormat = 0x1B80,

    /// Controller busy
    ControllerBusy = 0x1B81,

    /// Connect invalid parameters
    ConnectInvalidParams = 0x1B82,

    /// Connect restart discovery
    ConnectRestartDiscovery = 0x1B83,

    /// Connect invalid host
    ConnectInvalidHost = 0x1B84,

    /// Invalid queue type
    InvalidQueueType = 0x1B90,

    /// Discover restart
    DiscoverRestart = 0x1B91,

    /// Authentication required
    AuthRequired = 0x1B92,
}

impl NvmeStatus {
    /// Create from raw status code
    pub fn from_raw(value: u16) -> Self {
        match value {
            0x0000 => Self::Success,
            0x0001 => Self::InvalidOpcode,
            0x0002 => Self::InvalidField,
            0x0003 => Self::CommandIdConflict,
            0x0004 => Self::DataTransferError,
            0x0005 => Self::PowerLoss,
            0x0006 => Self::InternalError,
            0x0007 => Self::CommandAbortRequested,
            0x0008 => Self::SqDeletionAbort,
            0x0009 => Self::FusedFail,
            0x000A => Self::FusedMissing,
            0x000B => Self::InvalidNamespaceOrFormat,
            0x000C => Self::CommandSequenceError,
            0x000D => Self::InvalidSglSegment,
            0x000E => Self::InvalidSglCount,
            0x000F => Self::DataSglLengthInvalid,
            0x0010 => Self::MetadataSglLengthInvalid,
            0x0011 => Self::SglTypeInvalid,
            0x0012 => Self::InvalidCmbUse,
            0x0013 => Self::PrpOffsetInvalid,
            0x0014 => Self::AtomicWriteUnitExceeded,
            0x0015 => Self::OperationDenied,
            0x0016 => Self::SglOffsetInvalid,
            0x0018 => Self::HostIdInconsistentFormat,
            0x0019 => Self::KeepAliveExpired,
            0x001A => Self::KeepAliveTimeoutInvalid,
            0x001B => Self::PreemptAbort,
            0x001C => Self::SanitizeFailed,
            0x001D => Self::SanitizeInProgress,
            0x001E => Self::SglDataBlockGranularityInvalid,
            0x001F => Self::CommandNotSupportedForQueueInCmb,
            0x0020 => Self::NamespaceWriteProtected,
            0x0021 => Self::CommandInterrupted,
            0x0022 => Self::TransientTransportError,
            0x1B80 => Self::IncompatibleFormat,
            0x1B81 => Self::ControllerBusy,
            0x1B82 => Self::ConnectInvalidParams,
            0x1B83 => Self::ConnectRestartDiscovery,
            0x1B84 => Self::ConnectInvalidHost,
            0x1B90 => Self::InvalidQueueType,
            0x1B91 => Self::DiscoverRestart,
            0x1B92 => Self::AuthRequired,
            _ => Self::InternalError,
        }
    }

    /// Convert to raw status code
    pub fn to_raw(self) -> u16 {
        self as u16
    }

    /// Check if status indicates success
    pub fn is_success(self) -> bool {
        self == Self::Success
    }

    /// Check if status is a fabric-specific error
    pub fn is_fabric_error(self) -> bool {
        (self as u16) >= 0x1B80
    }
}

impl From<NvmeStatus> for NvmeOfError {
    fn from(status: NvmeStatus) -> Self {
        NvmeOfError::InvalidCommand {
            opcode: 0,
            status: status.to_raw(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvme_status_conversion() {
        assert_eq!(NvmeStatus::from_raw(0x0000), NvmeStatus::Success);
        assert_eq!(NvmeStatus::from_raw(0x0001), NvmeStatus::InvalidOpcode);
        assert_eq!(
            NvmeStatus::from_raw(0x1B82),
            NvmeStatus::ConnectInvalidParams
        );

        assert!(NvmeStatus::Success.is_success());
        assert!(!NvmeStatus::InvalidOpcode.is_success());

        assert!(NvmeStatus::ConnectInvalidParams.is_fabric_error());
        assert!(!NvmeStatus::Success.is_fabric_error());
    }
}
