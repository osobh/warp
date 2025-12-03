//! Frames implementation

use crate::Result;

use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};

/// Frame type identifiers
pub mod frame_type {
    // Handshake
    pub const HELLO: u8 = 0x01;
    pub const CAPABILITIES: u8 = 0x02;
    pub const PLAN: u8 = 0x03;
    pub const ACCEPT: u8 = 0x04;
    
    // Deduplication
    pub const HAVE: u8 = 0x05;
    pub const WANT: u8 = 0x06;
    
    // Data transfer
    pub const METADATA: u8 = 0x10;
    pub const CHUNK: u8 = 0x11;
    pub const CHUNK_BATCH: u8 = 0x12;
    pub const END_OF_DATA: u8 = 0x13;
    
    // Acknowledgment
    pub const ACK: u8 = 0x20;
    pub const NACK: u8 = 0x21;
    
    // Control
    pub const DONE: u8 = 0x30;
    pub const VERIFY: u8 = 0x31;
    pub const ERROR: u8 = 0x40;
    pub const CANCEL: u8 = 0x41;
    pub const PAUSE: u8 = 0x42;
}

/// Frame header (8 bytes)
#[derive(Debug, Clone, Copy)]
pub struct FrameHeader {
    /// Frame type
    pub frame_type: u8,
    /// Flags
    pub flags: u8,
    /// Stream ID
    pub stream_id: u16,
    /// Payload length
    pub length: u32,
}

impl FrameHeader {
    /// Header size in bytes
    pub const SIZE: usize = 8;
    
    /// Encode header to bytes
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(self.frame_type);
        buf.put_u8(self.flags);
        buf.put_u16_le(self.stream_id);
        buf.put_u32_le(self.length);
    }
    
    /// Decode header from bytes
    pub fn decode(buf: &mut impl Buf) -> Result<Self> {
        if buf.remaining() < Self::SIZE {
            return Err(crate::Error::Protocol("Incomplete header".into()));
        }
        
        Ok(Self {
            frame_type: buf.get_u8(),
            flags: buf.get_u8(),
            stream_id: buf.get_u16_le(),
            length: buf.get_u32_le(),
        })
    }
}

/// Node capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    /// Node identifier
    pub node_id: String,
    /// Hostname
    pub hostname: String,
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// GPU info (if available)
    pub gpu: Option<GpuInfo>,
    /// Supported compression algorithms
    pub compression: Vec<String>,
    /// Supported hash algorithms
    pub hashes: Vec<String>,
    /// Maximum chunk size
    pub max_chunk_size: u32,
    /// Maximum concurrent streams
    pub max_streams: u32,
    /// Deduplication support
    pub supports_dedup: bool,
    /// Encryption support
    pub supports_encryption: bool,
}

/// GPU information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// GPU name
    pub name: String,
    /// GPU memory in bytes
    pub memory: u64,
    /// nvCOMP support
    pub nvcomp: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            hostname: std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("HOST"))
                .unwrap_or_else(|_| "unknown".into()),
            cpu_cores: std::thread::available_parallelism()
                .map(|p| p.get() as u32)
                .unwrap_or(1),
            gpu: None,
            compression: vec!["zstd".into(), "lz4".into()],
            hashes: vec!["blake3".into()],
            max_chunk_size: 64 * 1024 * 1024, // 64MB
            max_streams: 16,
            supports_dedup: true,
            supports_encryption: true,
        }
    }
}
