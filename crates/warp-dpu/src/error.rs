//! Error types for DPU operations
//!
//! Provides comprehensive error handling for DPU device initialization,
//! memory management, and accelerated operations.

/// Result type for DPU operations
pub type Result<T> = std::result::Result<T, Error>;

/// DPU operation errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// DPU device initialization failed
    #[error("Failed to initialize DPU device {device_id}: {message}")]
    DeviceInit {
        /// Device index that failed
        device_id: usize,
        /// Error message
        message: String,
    },

    /// No DPU device found on the system
    #[error("No DPU device found")]
    NoDevice,

    /// DOCA operation failed (BlueField-specific)
    #[error("DOCA operation failed: {0}")]
    DocaOperation(String),

    /// RDMA operation failed
    #[error("RDMA operation failed: {0}")]
    RdmaError(String),

    /// Memory registration failed
    #[error("Failed to register memory: {0}")]
    MemoryRegistration(String),

    /// Memory allocation failed
    #[error("Failed to allocate {requested_bytes} bytes: {reason}")]
    MemoryAllocation {
        /// Bytes requested
        requested_bytes: usize,
        /// Failure reason
        reason: String,
    },

    /// Work queue error
    #[error("Work queue error: {0}")]
    WorkQueue(String),

    /// Crypto acceleration error
    #[error("Crypto acceleration error: {0}")]
    CryptoAccel(String),

    /// Compression acceleration error
    #[error("Compression acceleration error: {0}")]
    CompressAccel(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    DecompressAccel(String),

    /// Hashing error
    #[error("Hashing error: {0}")]
    HashAccel(String),

    /// Erasure coding error
    #[error("Erasure coding error: {0}")]
    ErasureCoding(String),

    /// DPU not available, using CPU fallback
    #[error("DPU not available: {reason}, using CPU fallback")]
    FallbackToCpu {
        /// Reason for fallback
        reason: String,
    },

    /// Operation not supported on this DPU
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Invalid input data
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Timeout waiting for completion
    #[error("Operation timed out after {timeout_ms}ms")]
    Timeout {
        /// Timeout in milliseconds
        timeout_ms: u64,
    },

    /// Buffer size mismatch
    #[error("Buffer size mismatch: expected {expected}, got {actual}")]
    BufferSizeMismatch {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Operation cancelled
    #[error("Operation cancelled")]
    Cancelled,

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Check if this error indicates we should fall back to CPU
    #[must_use]
    pub fn should_fallback(&self) -> bool {
        matches!(
            self,
            Error::NoDevice
                | Error::DeviceInit { .. }
                | Error::FallbackToCpu { .. }
                | Error::NotSupported(_)
        )
    }

    /// Create a fallback error with reason
    #[must_use]
    pub fn fallback(reason: impl Into<String>) -> Self {
        Error::FallbackToCpu {
            reason: reason.into(),
        }
    }

    /// Create a device init error
    #[must_use]
    pub fn device_init(device_id: usize, message: impl Into<String>) -> Self {
        Error::DeviceInit {
            device_id,
            message: message.into(),
        }
    }

    /// Create a memory allocation error
    #[must_use]
    pub fn alloc_failed(requested_bytes: usize, reason: impl Into<String>) -> Self {
        Error::MemoryAllocation {
            requested_bytes,
            reason: reason.into(),
        }
    }
}

/// Error severity for metrics and logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Recoverable with fallback
    Recoverable,
    /// Operation failed but system stable
    Transient,
    /// Requires intervention
    Critical,
}

impl Error {
    /// Get the severity of this error
    #[must_use]
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Error::FallbackToCpu { .. } => ErrorSeverity::Recoverable,
            Error::Timeout { .. } | Error::Cancelled => ErrorSeverity::Transient,
            Error::NoDevice | Error::DeviceInit { .. } => ErrorSeverity::Critical,
            _ => ErrorSeverity::Transient,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::device_init(0, "connection failed");
        assert!(err.to_string().contains("device 0"));
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_should_fallback() {
        assert!(Error::NoDevice.should_fallback());
        assert!(Error::fallback("test").should_fallback());
        assert!(!Error::Timeout { timeout_ms: 1000 }.should_fallback());
    }

    #[test]
    fn test_error_severity() {
        assert_eq!(Error::NoDevice.severity(), ErrorSeverity::Critical);
        assert_eq!(
            Error::fallback("test").severity(),
            ErrorSeverity::Recoverable
        );
    }
}
