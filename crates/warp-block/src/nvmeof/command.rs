//! NVMe command structures
//!
//! This module defines NVMe command and completion structures according to
//! the NVM Express Base Specification.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::fmt;

/// NVMe Admin command opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AdminOpcode {
    /// Delete I/O Submission Queue
    DeleteIoSq = 0x00,
    /// Create I/O Submission Queue
    CreateIoSq = 0x01,
    /// Get Log Page
    GetLogPage = 0x02,
    /// Delete I/O Completion Queue
    DeleteIoCq = 0x04,
    /// Create I/O Completion Queue
    CreateIoCq = 0x05,
    /// Identify
    Identify = 0x06,
    /// Abort
    Abort = 0x08,
    /// Set Features
    SetFeatures = 0x09,
    /// Get Features
    GetFeatures = 0x0A,
    /// Async Event Request
    AsyncEventRequest = 0x0C,
    /// Namespace Management
    NamespaceManagement = 0x0D,
    /// Firmware Commit
    FirmwareCommit = 0x10,
    /// Firmware Image Download
    FirmwareImageDownload = 0x11,
    /// Device Self-test
    DeviceSelfTest = 0x14,
    /// Namespace Attachment
    NamespaceAttachment = 0x15,
    /// Keep Alive
    KeepAlive = 0x18,
    /// Directive Send
    DirectiveSend = 0x19,
    /// Directive Receive
    DirectiveReceive = 0x1A,
    /// Virtualization Management
    VirtualizationManagement = 0x1C,
    /// NVMe-MI Send
    NvmeMiSend = 0x1D,
    /// NVMe-MI Receive
    NvmeMiReceive = 0x1E,
    /// Doorbell Buffer Config
    DoorbellBufferConfig = 0x7C,

    // Fabric-specific opcodes (0x7F)
    /// Fabrics command (Property Get/Set, Connect, etc.)
    Fabrics = 0x7F,
}

impl AdminOpcode {
    /// Create from raw opcode
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::DeleteIoSq),
            0x01 => Some(Self::CreateIoSq),
            0x02 => Some(Self::GetLogPage),
            0x04 => Some(Self::DeleteIoCq),
            0x05 => Some(Self::CreateIoCq),
            0x06 => Some(Self::Identify),
            0x08 => Some(Self::Abort),
            0x09 => Some(Self::SetFeatures),
            0x0A => Some(Self::GetFeatures),
            0x0C => Some(Self::AsyncEventRequest),
            0x0D => Some(Self::NamespaceManagement),
            0x10 => Some(Self::FirmwareCommit),
            0x11 => Some(Self::FirmwareImageDownload),
            0x14 => Some(Self::DeviceSelfTest),
            0x15 => Some(Self::NamespaceAttachment),
            0x18 => Some(Self::KeepAlive),
            0x19 => Some(Self::DirectiveSend),
            0x1A => Some(Self::DirectiveReceive),
            0x1C => Some(Self::VirtualizationManagement),
            0x1D => Some(Self::NvmeMiSend),
            0x1E => Some(Self::NvmeMiReceive),
            0x7C => Some(Self::DoorbellBufferConfig),
            0x7F => Some(Self::Fabrics),
            _ => None,
        }
    }
}

/// NVMe I/O command opcodes (NVM Command Set)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IoOpcode {
    /// Flush
    Flush = 0x00,
    /// Write
    Write = 0x01,
    /// Read
    Read = 0x02,
    /// Write Uncorrectable
    WriteUncorrectable = 0x04,
    /// Compare
    Compare = 0x05,
    /// Write Zeroes
    WriteZeroes = 0x08,
    /// Dataset Management (TRIM/Deallocate)
    DatasetManagement = 0x09,
    /// Verify
    Verify = 0x0C,
    /// Reservation Register
    ReservationRegister = 0x0D,
    /// Reservation Report
    ReservationReport = 0x0E,
    /// Reservation Acquire
    ReservationAcquire = 0x11,
    /// Reservation Release
    ReservationRelease = 0x15,
    /// Copy
    Copy = 0x19,
}

impl IoOpcode {
    /// Create from raw opcode
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Flush),
            0x01 => Some(Self::Write),
            0x02 => Some(Self::Read),
            0x04 => Some(Self::WriteUncorrectable),
            0x05 => Some(Self::Compare),
            0x08 => Some(Self::WriteZeroes),
            0x09 => Some(Self::DatasetManagement),
            0x0C => Some(Self::Verify),
            0x0D => Some(Self::ReservationRegister),
            0x0E => Some(Self::ReservationReport),
            0x11 => Some(Self::ReservationAcquire),
            0x15 => Some(Self::ReservationRelease),
            0x19 => Some(Self::Copy),
            _ => None,
        }
    }
}

/// Fabrics command types (fctype)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FabricsType {
    /// Property Set
    PropertySet = 0x00,
    /// Connect
    Connect = 0x01,
    /// Property Get
    PropertyGet = 0x04,
    /// Authentication Send
    AuthenticationSend = 0x05,
    /// Authentication Receive
    AuthenticationReceive = 0x06,
    /// Disconnect
    Disconnect = 0x08,
}

impl FabricsType {
    /// Create from raw type
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::PropertySet),
            0x01 => Some(Self::Connect),
            0x04 => Some(Self::PropertyGet),
            0x05 => Some(Self::AuthenticationSend),
            0x06 => Some(Self::AuthenticationReceive),
            0x08 => Some(Self::Disconnect),
            _ => None,
        }
    }
}

/// NVMe Submission Queue Entry (SQE) - 64 bytes
#[derive(Clone)]
pub struct NvmeCommand {
    /// Command Dword 0 (CDW0): opcode, fuse, psdt, cid
    pub cdw0: u32,
    /// Namespace Identifier
    pub nsid: u32,
    /// Reserved (CDW2-3)
    pub cdw2: u32,
    pub cdw3: u32,
    /// Metadata Pointer
    pub mptr: u64,
    /// Data Pointer (PRP1 or SGL1)
    pub dptr: [u64; 2],
    /// Command Dword 10
    pub cdw10: u32,
    /// Command Dword 11
    pub cdw11: u32,
    /// Command Dword 12
    pub cdw12: u32,
    /// Command Dword 13
    pub cdw13: u32,
    /// Command Dword 14
    pub cdw14: u32,
    /// Command Dword 15
    pub cdw15: u32,
}

impl NvmeCommand {
    /// Size of an NVMe command in bytes
    pub const SIZE: usize = 64;

    /// Create a new empty command
    pub fn new() -> Self {
        Self {
            cdw0: 0,
            nsid: 0,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            dptr: [0, 0],
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Get the command opcode
    pub fn opcode(&self) -> u8 {
        (self.cdw0 & 0xFF) as u8
    }

    /// Set the command opcode
    pub fn set_opcode(&mut self, opcode: u8) {
        self.cdw0 = (self.cdw0 & !0xFF) | (opcode as u32);
    }

    /// Get the fused operation flags (00=normal, 01=fused first, 10=fused second)
    pub fn fuse(&self) -> u8 {
        ((self.cdw0 >> 8) & 0x03) as u8
    }

    /// Get the PRP or SGL for data transfer (PSDT)
    pub fn psdt(&self) -> u8 {
        ((self.cdw0 >> 14) & 0x03) as u8
    }

    /// Get the command identifier
    pub fn cid(&self) -> u16 {
        ((self.cdw0 >> 16) & 0xFFFF) as u16
    }

    /// Set the command identifier
    pub fn set_cid(&mut self, cid: u16) {
        self.cdw0 = (self.cdw0 & 0x0000FFFF) | ((cid as u32) << 16);
    }

    /// Get the fabrics command type (only valid for Fabrics commands)
    pub fn fctype(&self) -> u8 {
        (self.cdw10 & 0xFF) as u8
    }

    /// Check if this is a fabrics command
    pub fn is_fabrics(&self) -> bool {
        self.opcode() == AdminOpcode::Fabrics as u8
    }

    /// Parse from bytes
    pub fn from_bytes(mut buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }

        Some(Self {
            cdw0: buf.get_u32_le(),
            nsid: buf.get_u32_le(),
            cdw2: buf.get_u32_le(),
            cdw3: buf.get_u32_le(),
            mptr: buf.get_u64_le(),
            dptr: [buf.get_u64_le(), buf.get_u64_le()],
            cdw10: buf.get_u32_le(),
            cdw11: buf.get_u32_le(),
            cdw12: buf.get_u32_le(),
            cdw13: buf.get_u32_le(),
            cdw14: buf.get_u32_le(),
            cdw15: buf.get_u32_le(),
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_u32_le(self.cdw0);
        buf.put_u32_le(self.nsid);
        buf.put_u32_le(self.cdw2);
        buf.put_u32_le(self.cdw3);
        buf.put_u64_le(self.mptr);
        buf.put_u64_le(self.dptr[0]);
        buf.put_u64_le(self.dptr[1]);
        buf.put_u32_le(self.cdw10);
        buf.put_u32_le(self.cdw11);
        buf.put_u32_le(self.cdw12);
        buf.put_u32_le(self.cdw13);
        buf.put_u32_le(self.cdw14);
        buf.put_u32_le(self.cdw15);
        buf.freeze()
    }

    // ======== Read/Write command helpers ========

    /// Get the starting LBA for read/write commands
    pub fn slba(&self) -> u64 {
        ((self.cdw11 as u64) << 32) | (self.cdw10 as u64)
    }

    /// Set the starting LBA for read/write commands
    pub fn set_slba(&mut self, lba: u64) {
        self.cdw10 = (lba & 0xFFFFFFFF) as u32;
        self.cdw11 = ((lba >> 32) & 0xFFFFFFFF) as u32;
    }

    /// Get the number of logical blocks (0-based, actual count = nlb + 1)
    pub fn nlb(&self) -> u16 {
        (self.cdw12 & 0xFFFF) as u16
    }

    /// Set the number of logical blocks
    pub fn set_nlb(&mut self, nlb: u16) {
        self.cdw12 = (self.cdw12 & !0xFFFF) | (nlb as u32);
    }

    // ======== Identify command helpers ========

    /// Get the Controller or Namespace Structure (CNS) for Identify command
    pub fn identify_cns(&self) -> u8 {
        (self.cdw10 & 0xFF) as u8
    }

    /// Set the CNS for Identify command
    pub fn set_identify_cns(&mut self, cns: u8) {
        self.cdw10 = (self.cdw10 & !0xFF) | (cns as u32);
    }

    // ======== Get/Set Features helpers ========

    /// Get the Feature Identifier (FID)
    pub fn feature_id(&self) -> u8 {
        (self.cdw10 & 0xFF) as u8
    }

    /// Set the Feature Identifier
    pub fn set_feature_id(&mut self, fid: u8) {
        self.cdw10 = (self.cdw10 & !0xFF) | (fid as u32);
    }
}

impl Default for NvmeCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NvmeCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NvmeCommand")
            .field("opcode", &format_args!("{:#04x}", self.opcode()))
            .field("cid", &self.cid())
            .field("nsid", &self.nsid)
            .field("cdw10", &format_args!("{:#010x}", self.cdw10))
            .field("cdw11", &format_args!("{:#010x}", self.cdw11))
            .field("cdw12", &format_args!("{:#010x}", self.cdw12))
            .finish()
    }
}

/// NVMe Completion Queue Entry (CQE) - 16 bytes
#[derive(Clone)]
pub struct NvmeCompletion {
    /// Command-specific result (DW0)
    pub result: u32,
    /// Reserved (DW1)
    pub rsvd: u32,
    /// SQ Head Pointer
    pub sq_head: u16,
    /// SQ Identifier
    pub sq_id: u16,
    /// Command Identifier
    pub cid: u16,
    /// Status Field (Phase Tag in bit 0)
    pub status: u16,
}

impl NvmeCompletion {
    /// Size of an NVMe completion in bytes
    pub const SIZE: usize = 16;

    /// Create a new successful completion
    pub fn success(cid: u16, sq_id: u16, sq_head: u16) -> Self {
        Self {
            result: 0,
            rsvd: 0,
            sq_head,
            sq_id,
            cid,
            status: 0, // Success, phase tag will be set later
        }
    }

    /// Create an error completion
    pub fn error(cid: u16, sq_id: u16, sq_head: u16, status_code: u16) -> Self {
        Self {
            result: 0,
            rsvd: 0,
            sq_head,
            sq_id,
            cid,
            // Status code is in bits 1-15 (bit 0 is phase tag)
            status: status_code << 1,
        }
    }

    /// Get the status code (without phase tag)
    pub fn status_code(&self) -> u16 {
        self.status >> 1
    }

    /// Check if command completed successfully
    pub fn is_success(&self) -> bool {
        self.status_code() == 0
    }

    /// Get the phase tag
    pub fn phase(&self) -> bool {
        (self.status & 1) != 0
    }

    /// Set the phase tag
    pub fn set_phase(&mut self, phase: bool) {
        if phase {
            self.status |= 1;
        } else {
            self.status &= !1;
        }
    }

    /// Parse from bytes
    pub fn from_bytes(mut buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }

        Some(Self {
            result: buf.get_u32_le(),
            rsvd: buf.get_u32_le(),
            sq_head: buf.get_u16_le(),
            sq_id: buf.get_u16_le(),
            cid: buf.get_u16_le(),
            status: buf.get_u16_le(),
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_u32_le(self.result);
        buf.put_u32_le(self.rsvd);
        buf.put_u16_le(self.sq_head);
        buf.put_u16_le(self.sq_id);
        buf.put_u16_le(self.cid);
        buf.put_u16_le(self.status);
        buf.freeze()
    }
}

impl fmt::Debug for NvmeCompletion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NvmeCompletion")
            .field("cid", &self.cid)
            .field("sq_id", &self.sq_id)
            .field("sq_head", &self.sq_head)
            .field("status", &format_args!("{:#06x}", self.status_code()))
            .field("phase", &self.phase())
            .field("result", &format_args!("{:#010x}", self.result))
            .finish()
    }
}

/// Identify Controller data structure (4096 bytes)
#[derive(Clone)]
pub struct IdentifyController {
    /// PCI Vendor ID
    pub vid: u16,
    /// PCI Subsystem Vendor ID
    pub ssvid: u16,
    /// Serial Number (20 bytes, ASCII)
    pub sn: [u8; 20],
    /// Model Number (40 bytes, ASCII)
    pub mn: [u8; 40],
    /// Firmware Revision (8 bytes, ASCII)
    pub fr: [u8; 8],
    /// Recommended Arbitration Burst
    pub rab: u8,
    /// IEEE OUI Identifier (3 bytes)
    pub ieee: [u8; 3],
    /// Controller Multi-Path I/O and Namespace Sharing
    pub cmic: u8,
    /// Maximum Data Transfer Size (in units of minimum memory page size)
    pub mdts: u8,
    /// Controller ID
    pub cntlid: u16,
    /// Version
    pub ver: u32,
    /// RTD3 Resume Latency
    pub rtd3r: u32,
    /// RTD3 Entry Latency
    pub rtd3e: u32,
    /// Optional Async Events Supported
    pub oaes: u32,
    /// Controller Attributes
    pub ctratt: u32,
    /// Read Recovery Levels Supported
    pub rrls: u16,
    /// Reserved
    pub reserved1: [u8; 9],
    /// Controller Type
    pub cntrltype: u8,
    /// FRU GUID
    pub fguid: [u8; 16],
    /// Command Retry Delay Times
    pub crdt: [u16; 3],
    /// Reserved
    pub reserved2: [u8; 106],
    /// Reserved (NVMe-MI)
    pub reserved3: [u8; 13],
    /// NVMe Management Interface
    pub nvmsr: u8,
    /// VPD Write Cycle Information
    pub vwci: u8,
    /// Management Endpoint Capabilities
    pub mec: u8,
    /// Optional Admin Command Support
    pub oacs: u16,
    /// Abort Command Limit
    pub acl: u8,
    /// Asynchronous Event Request Limit
    pub aerl: u8,
    /// Firmware Updates
    pub frmw: u8,
    /// Log Page Attributes
    pub lpa: u8,
    /// Error Log Page Entries
    pub elpe: u8,
    /// Number of Power States Support
    pub npss: u8,
    /// Admin Vendor Specific Command Configuration
    pub avscc: u8,
    /// Autonomous Power State Transition Attributes
    pub apsta: u8,
    /// Warning Composite Temperature Threshold
    pub wctemp: u16,
    /// Critical Composite Temperature Threshold
    pub cctemp: u16,
    /// Maximum Time for Firmware Activation
    pub mtfa: u16,
    /// Host Memory Buffer Preferred Size
    pub hmpre: u32,
    /// Host Memory Buffer Minimum Size
    pub hmmin: u32,
    /// Total NVM Capacity (16 bytes)
    pub tnvmcap: [u8; 16],
    /// Unallocated NVM Capacity (16 bytes)
    pub unvmcap: [u8; 16],
    /// Replay Protected Memory Block Support
    pub rpmbs: u32,
    /// Extended Device Self-test Time
    pub edstt: u16,
    /// Device Self-test Options
    pub dsto: u8,
    /// Firmware Update Granularity
    pub fwug: u8,
    /// Keep Alive Support
    pub kas: u16,
    /// Host Controlled Thermal Management Attributes
    pub hctma: u16,
    /// Minimum Thermal Management Temperature
    pub mntmt: u16,
    /// Maximum Thermal Management Temperature
    pub mxtmt: u16,
    /// Sanitize Capabilities
    pub sanicap: u32,
    /// Host Memory Buffer Minimum Descriptor Entry Size
    pub hmminds: u32,
    /// Host Memory Maximum Descriptors Entries
    pub hmmaxd: u16,
    /// NVM Set Identifier Maximum
    pub nsetidmax: u16,
    /// Endurance Group Identifier Maximum
    pub endgidmax: u16,
    /// ANA Transition Time
    pub anatt: u8,
    /// Asymmetric Namespace Access Capabilities
    pub anacap: u8,
    /// ANA Group Identifier Maximum
    pub anagrpmax: u32,
    /// Number of ANA Group Identifiers
    pub nanagrpid: u32,
    /// Persistent Event Log Size
    pub pels: u32,
    /// Reserved
    pub reserved4: [u8; 156],
    /// Submission Queue Entry Size
    pub sqes: u8,
    /// Completion Queue Entry Size
    pub cqes: u8,
    /// Maximum Outstanding Commands
    pub maxcmd: u16,
    /// Number of Namespaces
    pub nn: u32,
    /// Optional NVM Command Support
    pub oncs: u16,
    /// Fused Operation Support
    pub fuses: u16,
    /// Format NVM Attributes
    pub fna: u8,
    /// Volatile Write Cache
    pub vwc: u8,
    /// Atomic Write Unit Normal
    pub awun: u16,
    /// Atomic Write Unit Power Fail
    pub awupf: u16,
    /// NVM Vendor Specific Command Configuration
    pub nvscc: u8,
    /// Namespace Write Protection Capabilities
    pub nwpc: u8,
    /// Atomic Compare & Write Unit
    pub acwu: u16,
    /// Reserved
    pub reserved5: u16,
    /// SGL Support
    pub sgls: u32,
    /// Maximum Number of Allowed Namespaces
    pub mnan: u32,
    /// Reserved
    pub reserved6: [u8; 224],
    /// NVM Subsystem NVMe Qualified Name (256 bytes)
    pub subnqn: [u8; 256],
    /// Reserved
    pub reserved7: [u8; 768],
    /// I/O Queue Command Capsule Supported Size
    pub ioccsz: u32,
    /// I/O Queue Response Capsule Supported Size
    pub iorcsz: u32,
    /// In Capsule Data Offset
    pub icdoff: u16,
    /// Fabrics Controller Attributes
    pub fcatt: u8,
    /// Maximum SGL Data Block Descriptors
    pub msdbd: u8,
    /// Optional Fabric Commands Support
    pub ofcs: u16,
    /// Reserved
    pub reserved8: [u8; 242],
    /// Power State Descriptors (32 * 32 bytes = 1024 bytes)
    pub psd: [u8; 1024],
    /// Vendor Specific (1024 bytes)
    pub vs: [u8; 1024],
}

impl IdentifyController {
    /// Size of Identify Controller data
    pub const SIZE: usize = 4096;

    /// Create a new Identify Controller with WARP defaults
    pub fn warp_defaults(cntlid: u16, nn: u32, nqn: &str) -> Self {
        let mut ctrl = Self::default();

        // Vendor info
        ctrl.vid = 0x1DE5; // WARP vendor ID (fictional)
        ctrl.ssvid = 0x1DE5;

        // Serial number
        let sn = b"WARP-NVMEOF-0001    ";
        ctrl.sn.copy_from_slice(sn);

        // Model number
        let mn = b"WARP NVMe-oF Target                     ";
        ctrl.mn.copy_from_slice(mn);

        // Firmware revision
        let fr = b"1.0.0   ";
        ctrl.fr.copy_from_slice(fr);

        ctrl.cntlid = cntlid;
        ctrl.ver = 0x00010400; // NVMe 1.4
        ctrl.nn = nn;

        // Controller capabilities
        ctrl.mdts = 5; // 128KB max transfer (2^5 * 4KB pages)
        ctrl.cmic = 0x06; // Multi-path capable, SR-IOV
        ctrl.ctratt = 0;
        ctrl.cntrltype = 1; // I/O controller

        // Queue capabilities
        ctrl.sqes = 0x66; // 64 bytes min/max
        ctrl.cqes = 0x44; // 16 bytes min/max

        // NVMe-oF specific
        ctrl.ioccsz = 16; // 16 DWORDs = 64 bytes (minimum capsule size)
        ctrl.iorcsz = 4; // 4 DWORDs = 16 bytes (completion size)
        ctrl.icdoff = 0; // In-capsule data offset
        ctrl.fcatt = 0; // Fabrics controller attributes
        ctrl.msdbd = 1; // Max SGL block descriptors
        ctrl.ofcs = 0x0001; // Supports disconnect command

        // Features
        ctrl.oacs = 0x0006; // Format NVM, Firmware Download
        ctrl.oncs = 0x0047; // Compare, Write Uncorrectable, DSM, Write Zeroes
        ctrl.vwc = 0x01; // Volatile write cache present
        ctrl.sgls = 0x00000001; // SGL supported for data transfer

        // Keep-alive support (10 second granularity)
        ctrl.kas = 10;

        // NQN
        let nqn_bytes = nqn.as_bytes();
        let copy_len = nqn_bytes.len().min(256);
        ctrl.subnqn[..copy_len].copy_from_slice(&nqn_bytes[..copy_len]);

        ctrl
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);

        buf.put_u16_le(self.vid);
        buf.put_u16_le(self.ssvid);
        buf.put_slice(&self.sn);
        buf.put_slice(&self.mn);
        buf.put_slice(&self.fr);
        buf.put_u8(self.rab);
        buf.put_slice(&self.ieee);
        buf.put_u8(self.cmic);
        buf.put_u8(self.mdts);
        buf.put_u16_le(self.cntlid);
        buf.put_u32_le(self.ver);
        buf.put_u32_le(self.rtd3r);
        buf.put_u32_le(self.rtd3e);
        buf.put_u32_le(self.oaes);
        buf.put_u32_le(self.ctratt);
        buf.put_u16_le(self.rrls);
        buf.put_slice(&self.reserved1);
        buf.put_u8(self.cntrltype);
        buf.put_slice(&self.fguid);
        for crdt in &self.crdt {
            buf.put_u16_le(*crdt);
        }
        buf.put_slice(&self.reserved2);
        buf.put_slice(&self.reserved3);
        buf.put_u8(self.nvmsr);
        buf.put_u8(self.vwci);
        buf.put_u8(self.mec);
        buf.put_u16_le(self.oacs);
        buf.put_u8(self.acl);
        buf.put_u8(self.aerl);
        buf.put_u8(self.frmw);
        buf.put_u8(self.lpa);
        buf.put_u8(self.elpe);
        buf.put_u8(self.npss);
        buf.put_u8(self.avscc);
        buf.put_u8(self.apsta);
        buf.put_u16_le(self.wctemp);
        buf.put_u16_le(self.cctemp);
        buf.put_u16_le(self.mtfa);
        buf.put_u32_le(self.hmpre);
        buf.put_u32_le(self.hmmin);
        buf.put_slice(&self.tnvmcap);
        buf.put_slice(&self.unvmcap);
        buf.put_u32_le(self.rpmbs);
        buf.put_u16_le(self.edstt);
        buf.put_u8(self.dsto);
        buf.put_u8(self.fwug);
        buf.put_u16_le(self.kas);
        buf.put_u16_le(self.hctma);
        buf.put_u16_le(self.mntmt);
        buf.put_u16_le(self.mxtmt);
        buf.put_u32_le(self.sanicap);
        buf.put_u32_le(self.hmminds);
        buf.put_u16_le(self.hmmaxd);
        buf.put_u16_le(self.nsetidmax);
        buf.put_u16_le(self.endgidmax);
        buf.put_u8(self.anatt);
        buf.put_u8(self.anacap);
        buf.put_u32_le(self.anagrpmax);
        buf.put_u32_le(self.nanagrpid);
        buf.put_u32_le(self.pels);
        buf.put_slice(&self.reserved4);
        buf.put_u8(self.sqes);
        buf.put_u8(self.cqes);
        buf.put_u16_le(self.maxcmd);
        buf.put_u32_le(self.nn);
        buf.put_u16_le(self.oncs);
        buf.put_u16_le(self.fuses);
        buf.put_u8(self.fna);
        buf.put_u8(self.vwc);
        buf.put_u16_le(self.awun);
        buf.put_u16_le(self.awupf);
        buf.put_u8(self.nvscc);
        buf.put_u8(self.nwpc);
        buf.put_u16_le(self.acwu);
        buf.put_u16_le(self.reserved5);
        buf.put_u32_le(self.sgls);
        buf.put_u32_le(self.mnan);
        buf.put_slice(&self.reserved6);
        buf.put_slice(&self.subnqn);
        buf.put_slice(&self.reserved7);
        buf.put_u32_le(self.ioccsz);
        buf.put_u32_le(self.iorcsz);
        buf.put_u16_le(self.icdoff);
        buf.put_u8(self.fcatt);
        buf.put_u8(self.msdbd);
        buf.put_u16_le(self.ofcs);
        buf.put_slice(&self.reserved8);
        buf.put_slice(&self.psd);
        buf.put_slice(&self.vs);

        buf.freeze()
    }
}

impl Default for IdentifyController {
    fn default() -> Self {
        Self {
            vid: 0,
            ssvid: 0,
            sn: [0; 20],
            mn: [0; 40],
            fr: [0; 8],
            rab: 0,
            ieee: [0; 3],
            cmic: 0,
            mdts: 0,
            cntlid: 0,
            ver: 0,
            rtd3r: 0,
            rtd3e: 0,
            oaes: 0,
            ctratt: 0,
            rrls: 0,
            reserved1: [0; 9],
            cntrltype: 0,
            fguid: [0; 16],
            crdt: [0; 3],
            reserved2: [0; 106],
            reserved3: [0; 13],
            nvmsr: 0,
            vwci: 0,
            mec: 0,
            oacs: 0,
            acl: 0,
            aerl: 0,
            frmw: 0,
            lpa: 0,
            elpe: 0,
            npss: 0,
            avscc: 0,
            apsta: 0,
            wctemp: 0,
            cctemp: 0,
            mtfa: 0,
            hmpre: 0,
            hmmin: 0,
            tnvmcap: [0; 16],
            unvmcap: [0; 16],
            rpmbs: 0,
            edstt: 0,
            dsto: 0,
            fwug: 0,
            kas: 0,
            hctma: 0,
            mntmt: 0,
            mxtmt: 0,
            sanicap: 0,
            hmminds: 0,
            hmmaxd: 0,
            nsetidmax: 0,
            endgidmax: 0,
            anatt: 0,
            anacap: 0,
            anagrpmax: 0,
            nanagrpid: 0,
            pels: 0,
            reserved4: [0; 156],
            sqes: 0,
            cqes: 0,
            maxcmd: 0,
            nn: 0,
            oncs: 0,
            fuses: 0,
            fna: 0,
            vwc: 0,
            awun: 0,
            awupf: 0,
            nvscc: 0,
            nwpc: 0,
            acwu: 0,
            reserved5: 0,
            sgls: 0,
            mnan: 0,
            reserved6: [0; 224],
            subnqn: [0; 256],
            reserved7: [0; 768],
            ioccsz: 0,
            iorcsz: 0,
            icdoff: 0,
            fcatt: 0,
            msdbd: 0,
            ofcs: 0,
            reserved8: [0; 242],
            psd: [0; 1024],
            vs: [0; 1024],
        }
    }
}

/// Identify Namespace data structure (4096 bytes)
#[derive(Clone)]
pub struct IdentifyNamespace {
    /// Namespace Size (in logical blocks)
    pub nsze: u64,
    /// Namespace Capacity
    pub ncap: u64,
    /// Namespace Utilization
    pub nuse: u64,
    /// Namespace Features
    pub nsfeat: u8,
    /// Number of LBA Formats
    pub nlbaf: u8,
    /// Formatted LBA Size
    pub flbas: u8,
    /// Metadata Capabilities
    pub mc: u8,
    /// End-to-end Data Protection Capabilities
    pub dpc: u8,
    /// End-to-end Data Protection Type Settings
    pub dps: u8,
    /// Namespace Multi-path I/O and Namespace Sharing
    pub nmic: u8,
    /// Reservation Capabilities
    pub rescap: u8,
    /// Format Progress Indicator
    pub fpi: u8,
    /// Deallocate Logical Block Features
    pub dlfeat: u8,
    /// Namespace Atomic Write Unit Normal
    pub nawun: u16,
    /// Namespace Atomic Write Unit Power Fail
    pub nawupf: u16,
    /// Namespace Atomic Compare & Write Unit
    pub nacwu: u16,
    /// Namespace Atomic Boundary Size Normal
    pub nabsn: u16,
    /// Namespace Atomic Boundary Offset
    pub nabo: u16,
    /// Namespace Atomic Boundary Size Power Fail
    pub nabspf: u16,
    /// Namespace Optimal I/O Boundary
    pub noiob: u16,
    /// NVM Capacity (16 bytes)
    pub nvmcap: [u8; 16],
    /// Namespace Preferred Write Granularity
    pub npwg: u16,
    /// Namespace Preferred Write Alignment
    pub npwa: u16,
    /// Namespace Preferred Deallocate Granularity
    pub npdg: u16,
    /// Namespace Preferred Deallocate Alignment
    pub npda: u16,
    /// Namespace Optimal Write Size
    pub nows: u16,
    /// Reserved
    pub reserved1: [u8; 18],
    /// ANA Group Identifier
    pub anagrpid: u32,
    /// Reserved
    pub reserved2: [u8; 3],
    /// Namespace Attributes
    pub nsattr: u8,
    /// NVM Set Identifier
    pub nvmsetid: u16,
    /// Endurance Group Identifier
    pub endgid: u16,
    /// Namespace GUID (16 bytes)
    pub nguid: [u8; 16],
    /// IEEE Extended Unique Identifier (8 bytes)
    pub eui64: [u8; 8],
    /// LBA Format Support (16 entries, 4 bytes each)
    pub lbaf: [[u8; 4]; 16],
    /// Reserved
    pub reserved3: [u8; 192],
    /// Vendor Specific (3712 bytes)
    pub vs: [u8; 3712],
}

impl IdentifyNamespace {
    /// Size of Identify Namespace data
    pub const SIZE: usize = 4096;

    /// Create a new namespace identity with given size
    pub fn new(size_bytes: u64, block_size: u32) -> Self {
        let num_blocks = size_bytes / block_size as u64;

        let mut ns = Self::default();
        ns.nsze = num_blocks;
        ns.ncap = num_blocks;
        ns.nuse = num_blocks;

        // Namespace features
        ns.nsfeat = 0x04; // Deallocate supported
        ns.nlbaf = 0; // 1 LBA format (0-indexed)
        ns.flbas = 0; // Using LBA format 0

        // LBA Format 0: block_size bytes, no metadata
        let lba_ds = (block_size.trailing_zeros() as u8) & 0xFF; // log2(block_size)
        ns.lbaf[0] = [0, 0, lba_ds, 0]; // [ms, lbads, rp]

        // Thin provisioning / deallocate
        ns.dlfeat = 0x01; // Read zeroes on deallocated blocks

        ns
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::SIZE);

        buf.put_u64_le(self.nsze);
        buf.put_u64_le(self.ncap);
        buf.put_u64_le(self.nuse);
        buf.put_u8(self.nsfeat);
        buf.put_u8(self.nlbaf);
        buf.put_u8(self.flbas);
        buf.put_u8(self.mc);
        buf.put_u8(self.dpc);
        buf.put_u8(self.dps);
        buf.put_u8(self.nmic);
        buf.put_u8(self.rescap);
        buf.put_u8(self.fpi);
        buf.put_u8(self.dlfeat);
        buf.put_u16_le(self.nawun);
        buf.put_u16_le(self.nawupf);
        buf.put_u16_le(self.nacwu);
        buf.put_u16_le(self.nabsn);
        buf.put_u16_le(self.nabo);
        buf.put_u16_le(self.nabspf);
        buf.put_u16_le(self.noiob);
        buf.put_slice(&self.nvmcap);
        buf.put_u16_le(self.npwg);
        buf.put_u16_le(self.npwa);
        buf.put_u16_le(self.npdg);
        buf.put_u16_le(self.npda);
        buf.put_u16_le(self.nows);
        buf.put_slice(&self.reserved1);
        buf.put_u32_le(self.anagrpid);
        buf.put_slice(&self.reserved2);
        buf.put_u8(self.nsattr);
        buf.put_u16_le(self.nvmsetid);
        buf.put_u16_le(self.endgid);
        buf.put_slice(&self.nguid);
        buf.put_slice(&self.eui64);
        for lbaf in &self.lbaf {
            buf.put_slice(lbaf);
        }
        buf.put_slice(&self.reserved3);
        buf.put_slice(&self.vs);

        buf.freeze()
    }
}

impl Default for IdentifyNamespace {
    fn default() -> Self {
        Self {
            nsze: 0,
            ncap: 0,
            nuse: 0,
            nsfeat: 0,
            nlbaf: 0,
            flbas: 0,
            mc: 0,
            dpc: 0,
            dps: 0,
            nmic: 0,
            rescap: 0,
            fpi: 0,
            dlfeat: 0,
            nawun: 0,
            nawupf: 0,
            nacwu: 0,
            nabsn: 0,
            nabo: 0,
            nabspf: 0,
            noiob: 0,
            nvmcap: [0; 16],
            npwg: 0,
            npwa: 0,
            npdg: 0,
            npda: 0,
            nows: 0,
            reserved1: [0; 18],
            anagrpid: 0,
            reserved2: [0; 3],
            nsattr: 0,
            nvmsetid: 0,
            endgid: 0,
            nguid: [0; 16],
            eui64: [0; 8],
            lbaf: [[0; 4]; 16],
            reserved3: [0; 192],
            vs: [0; 3712],
        }
    }
}

/// CNS (Controller or Namespace Structure) values for Identify command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IdentifyCns {
    /// Identify Namespace (NSID specified)
    Namespace = 0x00,
    /// Identify Controller
    Controller = 0x01,
    /// Active Namespace ID list
    ActiveNamespaceList = 0x02,
    /// Namespace Identification Descriptor list
    NamespaceIdDescriptorList = 0x03,
    /// NVM Set List
    NvmSetList = 0x04,
    /// I/O Command Set specific Identify Namespace
    IoCommandSetNamespace = 0x05,
    /// I/O Command Set specific Identify Controller
    IoCommandSetController = 0x06,
    /// I/O Command Set specific Active Namespace ID list
    IoCommandSetActiveNamespaceList = 0x07,
    /// Allocated Namespace ID list
    AllocatedNamespaceList = 0x10,
    /// Identify Namespace for allocated NSID
    AllocatedNamespace = 0x11,
    /// Controller list attached to NSID
    AttachedControllerList = 0x12,
    /// Controller list in subsystem
    ControllerList = 0x13,
    /// Primary Controller Capabilities
    PrimaryControllerCapabilities = 0x14,
    /// Secondary Controller list
    SecondaryControllerList = 0x15,
    /// Namespace Granularity List
    NamespaceGranularityList = 0x16,
    /// UUID List
    UuidList = 0x17,
}

impl IdentifyCns {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Namespace),
            0x01 => Some(Self::Controller),
            0x02 => Some(Self::ActiveNamespaceList),
            0x03 => Some(Self::NamespaceIdDescriptorList),
            0x04 => Some(Self::NvmSetList),
            0x05 => Some(Self::IoCommandSetNamespace),
            0x06 => Some(Self::IoCommandSetController),
            0x07 => Some(Self::IoCommandSetActiveNamespaceList),
            0x10 => Some(Self::AllocatedNamespaceList),
            0x11 => Some(Self::AllocatedNamespace),
            0x12 => Some(Self::AttachedControllerList),
            0x13 => Some(Self::ControllerList),
            0x14 => Some(Self::PrimaryControllerCapabilities),
            0x15 => Some(Self::SecondaryControllerList),
            0x16 => Some(Self::NamespaceGranularityList),
            0x17 => Some(Self::UuidList),
            _ => None,
        }
    }
}

/// Feature identifiers for Get/Set Features commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeatureId {
    /// Arbitration
    Arbitration = 0x01,
    /// Power Management
    PowerManagement = 0x02,
    /// LBA Range Type
    LbaRangeType = 0x03,
    /// Temperature Threshold
    TemperatureThreshold = 0x04,
    /// Error Recovery
    ErrorRecovery = 0x05,
    /// Volatile Write Cache
    VolatileWriteCache = 0x06,
    /// Number of Queues
    NumberOfQueues = 0x07,
    /// Interrupt Coalescing
    InterruptCoalescing = 0x08,
    /// Interrupt Vector Configuration
    InterruptVectorConfig = 0x09,
    /// Write Atomicity Normal
    WriteAtomicityNormal = 0x0A,
    /// Asynchronous Event Configuration
    AsyncEventConfig = 0x0B,
    /// Autonomous Power State Transition
    AutoPowerStateTransition = 0x0C,
    /// Host Memory Buffer
    HostMemoryBuffer = 0x0D,
    /// Timestamp
    Timestamp = 0x0E,
    /// Keep Alive Timer
    KeepAliveTimer = 0x0F,
    /// Host Controlled Thermal Management
    HostControlledThermalManagement = 0x10,
    /// Non-Operational Power State Config
    NonOperationalPowerStateConfig = 0x11,
    /// Read Recovery Level Config
    ReadRecoveryLevelConfig = 0x12,
    /// Predictable Latency Mode Config
    PredictableLatencyModeConfig = 0x13,
    /// Predictable Latency Mode Window
    PredictableLatencyModeWindow = 0x14,
    /// LBA Status Information Report Interval
    LbaStatusInfoReportInterval = 0x15,
    /// Host Behavior Support
    HostBehaviorSupport = 0x16,
    /// Sanitize Config
    SanitizeConfig = 0x17,
    /// Endurance Group Event Configuration
    EnduranceGroupEventConfig = 0x18,
}

impl FeatureId {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Arbitration),
            0x02 => Some(Self::PowerManagement),
            0x03 => Some(Self::LbaRangeType),
            0x04 => Some(Self::TemperatureThreshold),
            0x05 => Some(Self::ErrorRecovery),
            0x06 => Some(Self::VolatileWriteCache),
            0x07 => Some(Self::NumberOfQueues),
            0x08 => Some(Self::InterruptCoalescing),
            0x09 => Some(Self::InterruptVectorConfig),
            0x0A => Some(Self::WriteAtomicityNormal),
            0x0B => Some(Self::AsyncEventConfig),
            0x0C => Some(Self::AutoPowerStateTransition),
            0x0D => Some(Self::HostMemoryBuffer),
            0x0E => Some(Self::Timestamp),
            0x0F => Some(Self::KeepAliveTimer),
            0x10 => Some(Self::HostControlledThermalManagement),
            0x11 => Some(Self::NonOperationalPowerStateConfig),
            0x12 => Some(Self::ReadRecoveryLevelConfig),
            0x13 => Some(Self::PredictableLatencyModeConfig),
            0x14 => Some(Self::PredictableLatencyModeWindow),
            0x15 => Some(Self::LbaStatusInfoReportInterval),
            0x16 => Some(Self::HostBehaviorSupport),
            0x17 => Some(Self::SanitizeConfig),
            0x18 => Some(Self::EnduranceGroupEventConfig),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let mut cmd = NvmeCommand::new();
        cmd.set_opcode(AdminOpcode::Identify as u8);
        cmd.set_cid(42);
        cmd.nsid = 1;
        cmd.set_identify_cns(IdentifyCns::Controller as u8);

        let bytes = cmd.to_bytes();
        assert_eq!(bytes.len(), NvmeCommand::SIZE);

        let parsed = NvmeCommand::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.opcode(), AdminOpcode::Identify as u8);
        assert_eq!(parsed.cid(), 42);
        assert_eq!(parsed.nsid, 1);
        assert_eq!(parsed.identify_cns(), IdentifyCns::Controller as u8);
    }

    #[test]
    fn test_completion_serialization() {
        let cqe = NvmeCompletion::success(42, 1, 10);

        let bytes = cqe.to_bytes();
        assert_eq!(bytes.len(), NvmeCompletion::SIZE);

        let parsed = NvmeCompletion::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.cid, 42);
        assert_eq!(parsed.sq_id, 1);
        assert_eq!(parsed.sq_head, 10);
        assert!(parsed.is_success());
    }

    #[test]
    fn test_identify_controller() {
        let ctrl = IdentifyController::warp_defaults(1, 4, "nqn.2024-01.io.warp:test");
        let bytes = ctrl.to_bytes();
        assert_eq!(bytes.len(), IdentifyController::SIZE);
    }

    #[test]
    fn test_identify_namespace() {
        let ns = IdentifyNamespace::new(1024 * 1024 * 1024, 4096); // 1GB, 4KB blocks
        assert_eq!(ns.nsze, 262144); // 1GB / 4KB = 262144 blocks

        let bytes = ns.to_bytes();
        assert_eq!(bytes.len(), IdentifyNamespace::SIZE);
    }

    #[test]
    fn test_read_write_lba() {
        let mut cmd = NvmeCommand::new();
        cmd.set_opcode(IoOpcode::Read as u8);
        cmd.set_slba(0x123456789ABCDEF0);
        cmd.set_nlb(255); // 256 blocks (0-based)

        assert_eq!(cmd.slba(), 0x123456789ABCDEF0);
        assert_eq!(cmd.nlb(), 255);
    }
}
