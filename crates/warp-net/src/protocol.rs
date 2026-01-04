//! Protocol implementation

use crate::frames::Capabilities;

/// Protocol state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolState {
    /// Initial state
    Initial,
    /// Sent/received HELLO
    HelloExchanged,
    /// Capabilities negotiated
    Negotiated,
    /// Plan sent/received
    Planned,
    /// Transfer in progress
    Transferring,
    /// Verifying integrity
    Verifying,
    /// Completed
    Done,
    /// Error state
    Error,
}

/// Negotiated session parameters
#[derive(Debug, Clone)]
pub struct NegotiatedParams {
    /// Compression algorithm to use
    pub compression: String,
    /// Hash algorithm to use
    pub hash: String,
    /// Chunk size
    pub chunk_size: u32,
    /// Number of parallel streams
    pub parallel_streams: u32,
    /// Use GPU acceleration
    pub use_gpu: bool,
    /// Use encryption
    pub use_encryption: bool,
    /// Use deduplication
    pub use_dedup: bool,
}

impl NegotiatedParams {
    /// Negotiate parameters from two capability sets
    pub fn negotiate(local: &Capabilities, remote: &Capabilities) -> Self {
        // Find common compression
        let compression = local
            .compression
            .iter()
            .find(|c| remote.compression.contains(c))
            .cloned()
            .unwrap_or_else(|| "none".into());

        // Find common hash
        let hash = local
            .hashes
            .iter()
            .find(|h| remote.hashes.contains(h))
            .cloned()
            .unwrap_or_else(|| "blake3".into());

        Self {
            compression,
            hash,
            chunk_size: local.max_chunk_size.min(remote.max_chunk_size),
            parallel_streams: local.max_streams.min(remote.max_streams),
            use_gpu: local.gpu.is_some() && remote.gpu.is_some(),
            use_encryption: local.supports_encryption && remote.supports_encryption,
            use_dedup: local.supports_dedup && remote.supports_dedup,
        }
    }
}
