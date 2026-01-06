//! NVMe-oF Capsule (PDU) handling
//!
//! This module defines the NVMe over Fabrics capsule structures for
//! command and response PDUs according to the NVMe over Fabrics specification.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::fmt;

use super::command::{FabricsType, NvmeCommand, NvmeCompletion};
use super::error::{NvmeOfError, NvmeOfResult, NvmeStatus};

/// NVMe-oF PDU type (for TCP transport)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PduType {
    /// ICReq - Initialize Connection Request
    IcReq = 0x00,
    /// ICResp - Initialize Connection Response
    IcResp = 0x01,
    /// H2CTermReq - Host to Controller Terminate Connection Request
    H2CTermReq = 0x02,
    /// C2HTermReq - Controller to Host Terminate Connection Request
    C2HTermReq = 0x03,
    /// CapsuleCmd - Command Capsule
    CapsuleCmd = 0x04,
    /// CapsuleResp - Response Capsule
    CapsuleResp = 0x05,
    /// H2CData - Host to Controller Data
    H2CData = 0x06,
    /// C2HData - Controller to Host Data
    C2HData = 0x07,
    /// R2T - Ready to Transfer
    R2T = 0x09,
}

impl PduType {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::IcReq),
            0x01 => Some(Self::IcResp),
            0x02 => Some(Self::H2CTermReq),
            0x03 => Some(Self::C2HTermReq),
            0x04 => Some(Self::CapsuleCmd),
            0x05 => Some(Self::CapsuleResp),
            0x06 => Some(Self::H2CData),
            0x07 => Some(Self::C2HData),
            0x09 => Some(Self::R2T),
            _ => None,
        }
    }
}

/// NVMe-oF TCP PDU Common Header (8 bytes)
#[derive(Clone)]
pub struct PduHeader {
    /// PDU Type
    pub pdu_type: PduType,
    /// PDU Specific Flags
    pub flags: u8,
    /// Header Length (in 32-bit words)
    pub hlen: u8,
    /// PDU Data Offset (in 32-bit words)
    pub pdo: u8,
    /// Packet Length (entire PDU including header)
    pub plen: u32,
}

impl PduHeader {
    /// Size of PDU common header
    pub const SIZE: usize = 8;

    /// Create a new PDU header
    pub fn new(pdu_type: PduType, hlen: u8, pdo: u8, plen: u32) -> Self {
        Self {
            pdu_type,
            flags: 0,
            hlen,
            pdo,
            plen,
        }
    }

    /// Parse from bytes
    pub fn from_bytes(mut buf: &[u8]) -> NvmeOfResult<Self> {
        if buf.len() < Self::SIZE {
            return Err(NvmeOfError::InvalidCapsule(
                "PDU header too short".to_string(),
            ));
        }

        let pdu_type_raw = buf.get_u8();
        let pdu_type = PduType::from_raw(pdu_type_raw).ok_or_else(|| {
            NvmeOfError::InvalidCapsule(format!("Unknown PDU type: {:#04x}", pdu_type_raw))
        })?;

        Ok(Self {
            pdu_type,
            flags: buf.get_u8(),
            hlen: buf.get_u8(),
            pdo: buf.get_u8(),
            plen: buf.get_u32_le(),
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_u8(self.pdu_type as u8);
        buf.put_u8(self.flags);
        buf.put_u8(self.hlen);
        buf.put_u8(self.pdo);
        buf.put_u32_le(self.plen);
        buf.freeze()
    }

    /// Get header length in bytes
    pub fn header_bytes(&self) -> usize {
        (self.hlen as usize) * 4
    }

    /// Get data offset in bytes
    pub fn data_offset_bytes(&self) -> usize {
        (self.pdo as usize) * 4
    }
}

impl fmt::Debug for PduHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PduHeader")
            .field("pdu_type", &self.pdu_type)
            .field("flags", &format_args!("{:#04x}", self.flags))
            .field("hlen", &self.hlen)
            .field("pdo", &self.pdo)
            .field("plen", &self.plen)
            .finish()
    }
}

/// Initialize Connection Request (ICReq) PDU
#[derive(Clone, Debug)]
pub struct IcReq {
    /// PDU Format Version
    pub pfv: u16,
    /// Maximum Host PDU Data Size
    pub maxhpda: u8,
    /// Digest types supported/requested
    pub digest: u8,
    /// Maximum R2T Outstanding
    pub maxr2t: u32,
}

impl IcReq {
    /// Size of ICReq specific header (after common header)
    pub const SIZE: usize = 120; // Total header is 128 bytes

    /// Create a new ICReq
    pub fn new() -> Self {
        Self {
            pfv: 0,     // Version 0
            maxhpda: 1, // 512 bytes (minimum)
            digest: 0,  // No digests
            maxr2t: 1,  // Single R2T
        }
    }

    /// Parse from bytes (after common header)
    pub fn from_bytes(mut buf: &[u8]) -> NvmeOfResult<Self> {
        if buf.len() < Self::SIZE {
            return Err(NvmeOfError::InvalidCapsule("ICReq too short".to_string()));
        }

        let pfv = buf.get_u16_le();
        let maxhpda = buf.get_u8();
        let digest = buf.get_u8();
        let maxr2t = buf.get_u32_le();

        Ok(Self {
            pfv,
            maxhpda,
            digest,
            maxr2t,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_u16_le(self.pfv);
        buf.put_u8(self.maxhpda);
        buf.put_u8(self.digest);
        buf.put_u32_le(self.maxr2t);
        // Pad to 120 bytes
        buf.resize(Self::SIZE, 0);
        buf.freeze()
    }
}

impl Default for IcReq {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize Connection Response (ICResp) PDU
#[derive(Clone, Debug)]
pub struct IcResp {
    /// PDU Format Version
    pub pfv: u16,
    /// Controller PDU Data Alignment
    pub cpda: u8,
    /// Digest types enabled
    pub digest: u8,
    /// Maximum Host to Controller Data
    pub maxh2cdata: u32,
}

impl IcResp {
    /// Size of ICResp specific header (after common header)
    pub const SIZE: usize = 120; // Total header is 128 bytes

    /// Create a new ICResp
    pub fn new() -> Self {
        Self {
            pfv: 0,
            cpda: 1,
            digest: 0,
            maxh2cdata: 131072, // 128KB default
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_u16_le(self.pfv);
        buf.put_u8(self.cpda);
        buf.put_u8(self.digest);
        buf.put_u32_le(self.maxh2cdata);
        // Pad to 120 bytes
        buf.resize(Self::SIZE, 0);
        buf.freeze()
    }
}

impl Default for IcResp {
    fn default() -> Self {
        Self::new()
    }
}

/// Command Capsule
#[derive(Clone)]
pub struct CommandCapsule {
    /// NVMe Command (SQE)
    pub command: NvmeCommand,
    /// In-capsule data (if any)
    pub data: Option<Bytes>,
}

impl CommandCapsule {
    /// Minimum size of a command capsule
    pub const MIN_SIZE: usize = NvmeCommand::SIZE;

    /// Create a new command capsule without data
    pub fn new(command: NvmeCommand) -> Self {
        Self {
            command,
            data: None,
        }
    }

    /// Create a command capsule with in-capsule data
    pub fn with_data(command: NvmeCommand, data: Bytes) -> Self {
        Self {
            command,
            data: Some(data),
        }
    }

    /// Parse from bytes (command only, data handled separately)
    pub fn from_bytes(buf: &[u8]) -> NvmeOfResult<Self> {
        if buf.len() < Self::MIN_SIZE {
            return Err(NvmeOfError::InvalidCapsule(
                "Command capsule too short".to_string(),
            ));
        }

        let command = NvmeCommand::from_bytes(&buf[..NvmeCommand::SIZE])
            .ok_or_else(|| NvmeOfError::InvalidCapsule("Invalid NVMe command".to_string()))?;

        let data = if buf.len() > NvmeCommand::SIZE {
            Some(Bytes::copy_from_slice(&buf[NvmeCommand::SIZE..]))
        } else {
            None
        };

        Ok(Self { command, data })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let cmd_bytes = self.command.to_bytes();
        match &self.data {
            Some(data) => {
                let mut buf = BytesMut::with_capacity(cmd_bytes.len() + data.len());
                buf.put(cmd_bytes);
                buf.put(data.clone());
                buf.freeze()
            }
            None => cmd_bytes,
        }
    }

    /// Get the size of this capsule
    pub fn size(&self) -> usize {
        NvmeCommand::SIZE + self.data.as_ref().map(|d| d.len()).unwrap_or(0)
    }
}

impl fmt::Debug for CommandCapsule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandCapsule")
            .field("command", &self.command)
            .field("data_len", &self.data.as_ref().map(|d| d.len()))
            .finish()
    }
}

/// Response Capsule
#[derive(Clone)]
pub struct ResponseCapsule {
    /// NVMe Completion (CQE)
    pub completion: NvmeCompletion,
    /// Response data (for commands that return data in capsule)
    pub data: Option<Bytes>,
}

impl ResponseCapsule {
    /// Minimum size of a response capsule
    pub const MIN_SIZE: usize = NvmeCompletion::SIZE;

    /// Create a new response capsule without data
    pub fn new(completion: NvmeCompletion) -> Self {
        Self {
            completion,
            data: None,
        }
    }

    /// Create a response capsule with data
    pub fn with_data(completion: NvmeCompletion, data: Bytes) -> Self {
        Self {
            completion,
            data: Some(data),
        }
    }

    /// Create a success response
    pub fn success(cid: u16, sq_id: u16, sq_head: u16) -> Self {
        Self::new(NvmeCompletion::success(cid, sq_id, sq_head))
    }

    /// Create an error response
    pub fn error(cid: u16, sq_id: u16, sq_head: u16, status: NvmeStatus) -> Self {
        Self::new(NvmeCompletion::error(cid, sq_id, sq_head, status.to_raw()))
    }

    /// Parse from bytes
    pub fn from_bytes(buf: &[u8]) -> NvmeOfResult<Self> {
        if buf.len() < Self::MIN_SIZE {
            return Err(NvmeOfError::InvalidCapsule(
                "Response capsule too short".to_string(),
            ));
        }

        let completion = NvmeCompletion::from_bytes(&buf[..NvmeCompletion::SIZE])
            .ok_or_else(|| NvmeOfError::InvalidCapsule("Invalid NVMe completion".to_string()))?;

        let data = if buf.len() > NvmeCompletion::SIZE {
            Some(Bytes::copy_from_slice(&buf[NvmeCompletion::SIZE..]))
        } else {
            None
        };

        Ok(Self { completion, data })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let cqe_bytes = self.completion.to_bytes();
        match &self.data {
            Some(data) => {
                let mut buf = BytesMut::with_capacity(cqe_bytes.len() + data.len());
                buf.put(cqe_bytes);
                buf.put(data.clone());
                buf.freeze()
            }
            None => cqe_bytes,
        }
    }

    /// Get the size of this capsule
    pub fn size(&self) -> usize {
        NvmeCompletion::SIZE + self.data.as_ref().map(|d| d.len()).unwrap_or(0)
    }
}

impl fmt::Debug for ResponseCapsule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseCapsule")
            .field("completion", &self.completion)
            .field("data_len", &self.data.as_ref().map(|d| d.len()))
            .finish()
    }
}

/// Fabrics Connect command data
#[derive(Clone, Debug)]
pub struct ConnectData {
    /// Host Identifier (16 bytes)
    pub hostid: [u8; 16],
    /// Controller ID (0xFFFF for dynamic allocation)
    pub cntlid: u16,
    /// Reserved
    pub reserved: [u8; 238],
    /// Subsystem NQN (256 bytes, null-terminated)
    pub subnqn: [u8; 256],
    /// Host NQN (256 bytes, null-terminated)
    pub hostnqn: [u8; 256],
    /// Reserved (256 bytes)
    pub reserved2: [u8; 256],
}

impl ConnectData {
    /// Size of connect data
    pub const SIZE: usize = 1024;

    /// Create new connect data
    pub fn new(hostid: [u8; 16], subnqn: &str, hostnqn: &str) -> Self {
        let mut data = Self::default();
        data.hostid = hostid;
        data.cntlid = 0xFFFF; // Dynamic allocation

        // Copy NQNs
        let subnqn_bytes = subnqn.as_bytes();
        let hostnqn_bytes = hostnqn.as_bytes();

        let subnqn_len = subnqn_bytes.len().min(255);
        let hostnqn_len = hostnqn_bytes.len().min(255);

        data.subnqn[..subnqn_len].copy_from_slice(&subnqn_bytes[..subnqn_len]);
        data.hostnqn[..hostnqn_len].copy_from_slice(&hostnqn_bytes[..hostnqn_len]);

        data
    }

    /// Get subsystem NQN as string
    pub fn subnqn_str(&self) -> &str {
        let len = self.subnqn.iter().position(|&b| b == 0).unwrap_or(256);
        std::str::from_utf8(&self.subnqn[..len]).unwrap_or("")
    }

    /// Get host NQN as string
    pub fn hostnqn_str(&self) -> &str {
        let len = self.hostnqn.iter().position(|&b| b == 0).unwrap_or(256);
        std::str::from_utf8(&self.hostnqn[..len]).unwrap_or("")
    }

    /// Parse from bytes
    pub fn from_bytes(mut buf: &[u8]) -> NvmeOfResult<Self> {
        if buf.len() < Self::SIZE {
            return Err(NvmeOfError::InvalidCapsule(
                "Connect data too short".to_string(),
            ));
        }

        let mut hostid = [0u8; 16];
        buf.copy_to_slice(&mut hostid);
        let cntlid = buf.get_u16_le();

        let mut reserved = [0u8; 238];
        buf.copy_to_slice(&mut reserved);

        let mut subnqn = [0u8; 256];
        buf.copy_to_slice(&mut subnqn);

        let mut hostnqn = [0u8; 256];
        buf.copy_to_slice(&mut hostnqn);

        let mut reserved2 = [0u8; 256];
        buf.copy_to_slice(&mut reserved2);

        Ok(Self {
            hostid,
            cntlid,
            reserved,
            subnqn,
            hostnqn,
            reserved2,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_slice(&self.hostid);
        buf.put_u16_le(self.cntlid);
        buf.put_slice(&self.reserved);
        buf.put_slice(&self.subnqn);
        buf.put_slice(&self.hostnqn);
        buf.put_slice(&self.reserved2);
        buf.freeze()
    }
}

impl Default for ConnectData {
    fn default() -> Self {
        Self {
            hostid: [0; 16],
            cntlid: 0xFFFF,
            reserved: [0; 238],
            subnqn: [0; 256],
            hostnqn: [0; 256],
            reserved2: [0; 256],
        }
    }
}

/// Fabrics Property Get/Set values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PropertyOffset {
    /// Controller Capabilities (8 bytes)
    Cap = 0x00,
    /// Version (4 bytes)
    Vs = 0x08,
    /// Controller Configuration (4 bytes)
    Cc = 0x14,
    /// Controller Status (4 bytes)
    Csts = 0x1C,
    /// NVM Subsystem Reset (4 bytes)
    Nssr = 0x20,
}

impl PropertyOffset {
    /// Create from raw offset
    pub fn from_raw(value: u32) -> Option<Self> {
        match value {
            0x00 => Some(Self::Cap),
            0x08 => Some(Self::Vs),
            0x14 => Some(Self::Cc),
            0x1C => Some(Self::Csts),
            0x20 => Some(Self::Nssr),
            _ => None,
        }
    }

    /// Get the size of this property
    pub fn size(&self) -> usize {
        match self {
            Self::Cap => 8,
            Self::Vs => 4,
            Self::Cc => 4,
            Self::Csts => 4,
            Self::Nssr => 4,
        }
    }
}

/// Controller Capabilities Register (CAP)
#[derive(Clone, Copy, Debug)]
pub struct ControllerCapabilities {
    /// Maximum Queue Entries Supported (0-15)
    pub mqes: u16,
    /// Contiguous Queues Required (16)
    pub cqr: bool,
    /// Arbitration Mechanism Supported (17-18)
    pub ams: u8,
    /// Timeout (24-31) in 500ms units
    pub to: u8,
    /// Doorbell Stride (32-35)
    pub dstrd: u8,
    /// NVM Subsystem Reset Supported (36)
    pub nssrs: bool,
    /// Command Sets Supported (37-44)
    pub css: u8,
    /// Boot Partition Support (45)
    pub bps: bool,
    /// Controller Power Scope (46-47)
    pub cps: u8,
    /// Memory Page Size Minimum (48-51)
    pub mpsmin: u8,
    /// Memory Page Size Maximum (52-55)
    pub mpsmax: u8,
    /// Persistent Memory Region Supported (56)
    pub pmrs: bool,
    /// Controller Memory Buffer Supported (57)
    pub cmbs: bool,
    /// NVM Subsystem Shutdown Supported (58)
    pub nsss: bool,
    /// Controller Ready Modes Supported (59-60)
    pub crms: u8,
}

impl ControllerCapabilities {
    /// Create default capabilities for WARP
    pub fn warp_defaults() -> Self {
        Self {
            mqes: 1023,   // 1024 entries max
            cqr: false,   // Non-contiguous queues OK
            ams: 0,       // Round-robin only
            to: 40,       // 20 seconds timeout
            dstrd: 0,     // 4-byte stride
            nssrs: false, // No NVM subsystem reset
            css: 0x01,    // NVM command set
            bps: false,   // No boot partition
            cps: 0,       // Not reported
            mpsmin: 0,    // 4KB min page
            mpsmax: 4,    // 64KB max page
            pmrs: false,  // No PMR
            cmbs: false,  // No CMB
            nsss: false,  // No subsystem shutdown
            crms: 0,      // Default
        }
    }

    /// Convert to 64-bit register value
    pub fn to_u64(&self) -> u64 {
        let mut val: u64 = 0;
        val |= self.mqes as u64;
        val |= (self.cqr as u64) << 16;
        val |= (self.ams as u64 & 0x03) << 17;
        val |= (self.to as u64) << 24;
        val |= (self.dstrd as u64 & 0x0F) << 32;
        val |= (self.nssrs as u64) << 36;
        val |= (self.css as u64) << 37;
        val |= (self.bps as u64) << 45;
        val |= (self.cps as u64 & 0x03) << 46;
        val |= (self.mpsmin as u64 & 0x0F) << 48;
        val |= (self.mpsmax as u64 & 0x0F) << 52;
        val |= (self.pmrs as u64) << 56;
        val |= (self.cmbs as u64) << 57;
        val |= (self.nsss as u64) << 58;
        val |= (self.crms as u64 & 0x03) << 59;
        val
    }
}

/// Controller Configuration Register (CC)
#[derive(Clone, Copy, Debug, Default)]
pub struct ControllerConfiguration {
    /// Enable (0)
    pub en: bool,
    /// I/O Command Set Selected (4-6)
    pub css: u8,
    /// Memory Page Size (7-10)
    pub mps: u8,
    /// Arbitration Mechanism Selected (11-13)
    pub ams: u8,
    /// Shutdown Notification (14-15)
    pub shn: u8,
    /// I/O Submission Queue Entry Size (16-19)
    pub iosqes: u8,
    /// I/O Completion Queue Entry Size (20-23)
    pub iocqes: u8,
    /// Controller Ready Independent of Media Enable (24)
    pub crime: bool,
}

impl ControllerConfiguration {
    /// Parse from 32-bit register value
    pub fn from_u32(val: u32) -> Self {
        Self {
            en: (val & 0x01) != 0,
            css: ((val >> 4) & 0x07) as u8,
            mps: ((val >> 7) & 0x0F) as u8,
            ams: ((val >> 11) & 0x07) as u8,
            shn: ((val >> 14) & 0x03) as u8,
            iosqes: ((val >> 16) & 0x0F) as u8,
            iocqes: ((val >> 20) & 0x0F) as u8,
            crime: ((val >> 24) & 0x01) != 0,
        }
    }

    /// Convert to 32-bit register value
    pub fn to_u32(&self) -> u32 {
        let mut val: u32 = 0;
        val |= self.en as u32;
        val |= (self.css as u32 & 0x07) << 4;
        val |= (self.mps as u32 & 0x0F) << 7;
        val |= (self.ams as u32 & 0x07) << 11;
        val |= (self.shn as u32 & 0x03) << 14;
        val |= (self.iosqes as u32 & 0x0F) << 16;
        val |= (self.iocqes as u32 & 0x0F) << 20;
        val |= (self.crime as u32) << 24;
        val
    }
}

/// Controller Status Register (CSTS)
#[derive(Clone, Copy, Debug, Default)]
pub struct ControllerStatus {
    /// Ready (0)
    pub rdy: bool,
    /// Controller Fatal Status (1)
    pub cfs: bool,
    /// Shutdown Status (2-3)
    pub shst: u8,
    /// NVM Subsystem Reset Occurred (4)
    pub nssro: bool,
    /// Processing Paused (5)
    pub pp: bool,
    /// Shutdown Type (6)
    pub st: bool,
}

impl ControllerStatus {
    /// Create ready status
    pub fn ready() -> Self {
        Self {
            rdy: true,
            cfs: false,
            shst: 0,
            nssro: false,
            pp: false,
            st: false,
        }
    }

    /// Convert to 32-bit register value
    pub fn to_u32(&self) -> u32 {
        let mut val: u32 = 0;
        val |= self.rdy as u32;
        val |= (self.cfs as u32) << 1;
        val |= (self.shst as u32 & 0x03) << 2;
        val |= (self.nssro as u32) << 4;
        val |= (self.pp as u32) << 5;
        val |= (self.st as u32) << 6;
        val
    }
}

/// Fabrics Property Get command helper
pub fn parse_property_get(cmd: &NvmeCommand) -> NvmeOfResult<(u32, bool)> {
    if !cmd.is_fabrics() || cmd.fctype() != FabricsType::PropertyGet as u8 {
        return Err(NvmeOfError::InvalidCommand {
            opcode: cmd.opcode(),
            status: NvmeStatus::InvalidOpcode.to_raw(),
        });
    }

    // CDW10: offset
    let offset = cmd.cdw10;
    // CDW11 bit 0: size (0 = 4 bytes, 1 = 8 bytes)
    let size_8byte = (cmd.cdw11 & 0x01) != 0;

    Ok((offset, size_8byte))
}

/// Fabrics Property Set command helper
pub fn parse_property_set(cmd: &NvmeCommand) -> NvmeOfResult<(u32, u64)> {
    if !cmd.is_fabrics() || cmd.fctype() != FabricsType::PropertySet as u8 {
        return Err(NvmeOfError::InvalidCommand {
            opcode: cmd.opcode(),
            status: NvmeStatus::InvalidOpcode.to_raw(),
        });
    }

    // CDW10: offset
    let offset = cmd.cdw10;
    // CDW11-12: value (depending on size)
    let value = ((cmd.cdw12 as u64) << 32) | (cmd.cdw11 as u64);

    Ok((offset, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdu_header_serialization() {
        let header = PduHeader::new(PduType::CapsuleCmd, 32, 16, 1024);
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), PduHeader::SIZE);

        let parsed = PduHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.pdu_type, PduType::CapsuleCmd);
        assert_eq!(parsed.hlen, 32);
        assert_eq!(parsed.pdo, 16);
        assert_eq!(parsed.plen, 1024);
    }

    #[test]
    fn test_command_capsule() {
        let mut cmd = NvmeCommand::new();
        cmd.set_opcode(0x02); // Read
        cmd.set_cid(42);
        cmd.nsid = 1;

        let capsule = CommandCapsule::new(cmd);
        let bytes = capsule.to_bytes();
        assert_eq!(bytes.len(), NvmeCommand::SIZE);

        let parsed = CommandCapsule::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.command.opcode(), 0x02);
        assert_eq!(parsed.command.cid(), 42);
    }

    #[test]
    fn test_response_capsule() {
        let capsule = ResponseCapsule::success(42, 1, 10);
        let bytes = capsule.to_bytes();
        assert_eq!(bytes.len(), NvmeCompletion::SIZE);

        let parsed = ResponseCapsule::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.completion.cid, 42);
        assert!(parsed.completion.is_success());
    }

    #[test]
    fn test_connect_data() {
        let hostid = [1u8; 16];
        let data = ConnectData::new(
            hostid,
            "nqn.2024-01.io.warp:test",
            "nqn.2024-01.io.warp:host",
        );

        assert_eq!(data.subnqn_str(), "nqn.2024-01.io.warp:test");
        assert_eq!(data.hostnqn_str(), "nqn.2024-01.io.warp:host");

        let bytes = data.to_bytes();
        assert_eq!(bytes.len(), ConnectData::SIZE);

        let parsed = ConnectData::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.hostid, hostid);
        assert_eq!(parsed.subnqn_str(), data.subnqn_str());
    }

    #[test]
    fn test_controller_capabilities() {
        let cap = ControllerCapabilities::warp_defaults();
        let val = cap.to_u64();

        // Verify MQES is in lower 16 bits
        assert_eq!((val & 0xFFFF) as u16, 1023);
    }

    #[test]
    fn test_controller_configuration() {
        let mut cc = ControllerConfiguration::default();
        cc.en = true;
        cc.iosqes = 6; // 64 bytes
        cc.iocqes = 4; // 16 bytes

        let val = cc.to_u32();
        let parsed = ControllerConfiguration::from_u32(val);

        assert!(parsed.en);
        assert_eq!(parsed.iosqes, 6);
        assert_eq!(parsed.iocqes, 4);
    }
}
