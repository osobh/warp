//! NFSv4.1 COMPOUND operation handling
//!
//! COMPOUND is the primary RPC procedure for NFSv4.1, allowing multiple
//! operations to be batched in a single request.

use bytes::Bytes;

use crate::error::NfsStatus;
use crate::rpc::xdr::{XdrDecoder, XdrEncoder};

/// NFSv4.1 operation codes (RFC 8881)
///
/// These codes identify individual operations within a COMPOUND request.
/// Operations 3-39 are from NFSv4.0, operations 40-58 were added in NFSv4.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Nfs4Op {
    // ===== NFSv4.0 Operations =====
    /// Check access permissions.
    Access = 3,
    /// Close a file.
    Close = 4,
    /// Commit cached data to stable storage.
    Commit = 5,
    /// Create a file or directory.
    Create = 6,
    /// Purge delegations awaiting recovery.
    DelegPurge = 7,
    /// Return a delegation.
    DelegReturn = 8,
    /// Get file attributes.
    GetAttr = 9,
    /// Get current filehandle.
    GetFh = 10,
    /// Create a hard link.
    Link = 11,
    /// Create a byte-range lock.
    Lock = 12,
    /// Test for lock.
    LockT = 13,
    /// Unlock byte-range.
    LockU = 14,
    /// Look up filename.
    Lookup = 15,
    /// Look up parent directory.
    LookupP = 16,
    /// Verify attributes don't match.
    NVerify = 17,
    /// Open a file.
    Open = 18,
    /// Open named attribute directory.
    OpenAttr = 19,
    /// Confirm open (NFSv4.0).
    OpenConfirm = 20,
    /// Reduce open file access.
    OpenDowngrade = 21,
    /// Set current filehandle.
    PutFh = 22,
    /// Set public filehandle.
    PutPubFh = 23,
    /// Set root filehandle.
    PutRootFh = 24,
    /// Read from file.
    Read = 25,
    /// Read directory.
    ReadDir = 26,
    /// Read symbolic link.
    ReadLink = 27,
    /// Remove a file or directory.
    Remove = 28,
    /// Rename a file or directory.
    Rename = 29,
    /// Renew lease (NFSv4.0).
    Renew = 30,
    /// Restore saved filehandle.
    RestoreFh = 31,
    /// Save current filehandle.
    SaveFh = 32,
    /// Get security information.
    SecInfo = 33,
    /// Set file attributes.
    SetAttr = 34,
    /// Set client ID (NFSv4.0).
    SetClientId = 35,
    /// Confirm client ID (NFSv4.0).
    SetClientIdConfirm = 36,
    /// Verify attributes match.
    Verify = 37,
    /// Write to file.
    Write = 38,
    /// Release lock owner state.
    ReleaseLockOwner = 39,

    // ===== NFSv4.1 Operations =====
    /// Control backchannel.
    BackchannelCtl = 40,
    /// Bind connection to session.
    BindConnToSession = 41,
    /// Exchange client identity.
    ExchangeId = 42,
    /// Create a session.
    CreateSession = 43,
    /// Destroy a session.
    DestroySession = 44,
    /// Free stateid.
    FreeStateId = 45,
    /// Get directory delegation.
    GetDirDelegation = 46,
    /// Get device information (pNFS).
    GetDeviceInfo = 47,
    /// Get device list (pNFS).
    GetDeviceList = 48,
    /// Commit layout changes (pNFS).
    LayoutCommit = 49,
    /// Get layout information (pNFS).
    LayoutGet = 50,
    /// Return layout (pNFS).
    LayoutReturn = 51,
    /// Get security info without name.
    SecInfoNoName = 52,
    /// Sequence operation (session slot).
    Sequence = 53,
    /// Set SSV (state protection).
    SetSsv = 54,
    /// Test state IDs.
    TestStateId = 55,
    /// Request delegation.
    WantDelegation = 56,
    /// Destroy client ID.
    DestroyClientId = 57,
    /// Signal reclaim complete.
    ReclaimComplete = 58,

    // ===== Error Marker =====
    /// Illegal/unknown operation.
    Illegal = 10044,
}

impl TryFrom<u32> for Nfs4Op {
    type Error = NfsStatus;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            3 => Ok(Self::Access),
            4 => Ok(Self::Close),
            5 => Ok(Self::Commit),
            6 => Ok(Self::Create),
            7 => Ok(Self::DelegPurge),
            8 => Ok(Self::DelegReturn),
            9 => Ok(Self::GetAttr),
            10 => Ok(Self::GetFh),
            11 => Ok(Self::Link),
            12 => Ok(Self::Lock),
            13 => Ok(Self::LockT),
            14 => Ok(Self::LockU),
            15 => Ok(Self::Lookup),
            16 => Ok(Self::LookupP),
            17 => Ok(Self::NVerify),
            18 => Ok(Self::Open),
            19 => Ok(Self::OpenAttr),
            20 => Ok(Self::OpenConfirm),
            21 => Ok(Self::OpenDowngrade),
            22 => Ok(Self::PutFh),
            23 => Ok(Self::PutPubFh),
            24 => Ok(Self::PutRootFh),
            25 => Ok(Self::Read),
            26 => Ok(Self::ReadDir),
            27 => Ok(Self::ReadLink),
            28 => Ok(Self::Remove),
            29 => Ok(Self::Rename),
            30 => Ok(Self::Renew),
            31 => Ok(Self::RestoreFh),
            32 => Ok(Self::SaveFh),
            33 => Ok(Self::SecInfo),
            34 => Ok(Self::SetAttr),
            35 => Ok(Self::SetClientId),
            36 => Ok(Self::SetClientIdConfirm),
            37 => Ok(Self::Verify),
            38 => Ok(Self::Write),
            39 => Ok(Self::ReleaseLockOwner),
            40 => Ok(Self::BackchannelCtl),
            41 => Ok(Self::BindConnToSession),
            42 => Ok(Self::ExchangeId),
            43 => Ok(Self::CreateSession),
            44 => Ok(Self::DestroySession),
            45 => Ok(Self::FreeStateId),
            46 => Ok(Self::GetDirDelegation),
            47 => Ok(Self::GetDeviceInfo),
            48 => Ok(Self::GetDeviceList),
            49 => Ok(Self::LayoutCommit),
            50 => Ok(Self::LayoutGet),
            51 => Ok(Self::LayoutReturn),
            52 => Ok(Self::SecInfoNoName),
            53 => Ok(Self::Sequence),
            54 => Ok(Self::SetSsv),
            55 => Ok(Self::TestStateId),
            56 => Ok(Self::WantDelegation),
            57 => Ok(Self::DestroyClientId),
            58 => Ok(Self::ReclaimComplete),
            10044 => Ok(Self::Illegal),
            _ => Err(NfsStatus::OpIllegal),
        }
    }
}

/// COMPOUND request arguments
#[derive(Debug, Clone)]
pub struct CompoundArgs {
    /// Minor version
    pub minor_version: u32,
    /// Tag (for debugging)
    pub tag: String,
    /// Operations
    pub ops: Vec<Nfs4OpArgs>,
}

impl CompoundArgs {
    /// Decode from XDR
    pub fn decode(dec: &mut XdrDecoder) -> std::io::Result<Self> {
        let tag = dec.decode_string()?;
        let minor_version = dec.decode_u32()?;
        let op_count = dec.decode_u32()? as usize;

        let mut ops = Vec::with_capacity(op_count);
        for _ in 0..op_count {
            ops.push(Nfs4OpArgs::decode(dec)?);
        }

        Ok(Self {
            minor_version,
            tag,
            ops,
        })
    }
}

/// Individual operation arguments
#[derive(Debug, Clone)]
pub struct Nfs4OpArgs {
    /// Operation code
    pub op: Nfs4Op,
    /// Operation-specific arguments (raw XDR)
    pub args: Bytes,
}

impl Nfs4OpArgs {
    /// Decode from XDR
    pub fn decode(dec: &mut XdrDecoder) -> std::io::Result<Self> {
        let op_code = dec.decode_u32()?;
        let op = Nfs4Op::try_from(op_code).unwrap_or(Nfs4Op::Illegal);

        // For now, we don't parse individual op args - that's done during processing
        Ok(Self {
            op,
            args: Bytes::new(),
        })
    }
}

/// COMPOUND response
#[derive(Debug, Clone)]
pub struct CompoundRes {
    /// Status of last operation
    pub status: NfsStatus,
    /// Tag (echoed from request)
    pub tag: String,
    /// Operation results
    pub results: Vec<Nfs4OpRes>,
}

impl CompoundRes {
    /// Create a new response
    pub fn new(tag: String) -> Self {
        Self {
            status: NfsStatus::Ok,
            tag,
            results: Vec::new(),
        }
    }

    /// Add an operation result
    pub fn add_result(&mut self, result: Nfs4OpRes) {
        if result.status != NfsStatus::Ok {
            self.status = result.status;
        }
        self.results.push(result);
    }

    /// Encode to XDR
    pub fn encode(&self, enc: &mut XdrEncoder) {
        enc.encode_u32(self.status as u32);
        enc.encode_string(&self.tag);
        enc.encode_u32(self.results.len() as u32);
        for result in &self.results {
            result.encode(enc);
        }
    }
}

/// Individual operation result
#[derive(Debug, Clone)]
pub struct Nfs4OpRes {
    /// Operation code
    pub op: Nfs4Op,
    /// Status
    pub status: NfsStatus,
    /// Operation-specific result (raw XDR)
    pub result: Bytes,
}

impl Nfs4OpRes {
    /// Create a successful result
    pub fn ok(op: Nfs4Op, result: Bytes) -> Self {
        Self {
            op,
            status: NfsStatus::Ok,
            result,
        }
    }

    /// Create an error result
    pub fn error(op: Nfs4Op, status: NfsStatus) -> Self {
        Self {
            op,
            status,
            result: Bytes::new(),
        }
    }

    /// Encode to XDR
    pub fn encode(&self, enc: &mut XdrEncoder) {
        enc.encode_u32(self.op as u32);
        enc.encode_u32(self.status as u32);
        if self.status == NfsStatus::Ok {
            // Write result data directly
            // The result is already XDR-encoded
        }
    }
}

/// COMPOUND execution context
#[derive(Debug)]
pub struct CompoundContext {
    /// Current filehandle
    pub current_fh: Option<super::Nfs4FileHandle>,
    /// Saved filehandle
    pub saved_fh: Option<super::Nfs4FileHandle>,
    /// Session ID (if bound)
    pub session_id: Option<[u8; 16]>,
    /// Sequence slot
    pub slot_id: Option<u32>,
}

impl CompoundContext {
    /// Create a new context
    pub fn new() -> Self {
        Self {
            current_fh: None,
            saved_fh: None,
            session_id: None,
            slot_id: None,
        }
    }

    /// Check if current filehandle is set
    pub fn has_current_fh(&self) -> bool {
        self.current_fh.is_some()
    }

    /// Get current filehandle or return error
    pub fn require_current_fh(&self) -> Result<&super::Nfs4FileHandle, NfsStatus> {
        self.current_fh.as_ref().ok_or(NfsStatus::NoFileHandle)
    }
}

impl Default for CompoundContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_code_conversion() {
        assert_eq!(Nfs4Op::try_from(9).unwrap(), Nfs4Op::GetAttr);
        assert_eq!(Nfs4Op::try_from(53).unwrap(), Nfs4Op::Sequence);
        assert!(Nfs4Op::try_from(999).is_err());
    }

    #[test]
    fn test_compound_context() {
        let ctx = CompoundContext::new();
        assert!(!ctx.has_current_fh());
        assert!(ctx.require_current_fh().is_err());
    }

    #[test]
    fn test_compound_res() {
        let mut res = CompoundRes::new("test".to_string());
        res.add_result(Nfs4OpRes::ok(Nfs4Op::GetAttr, Bytes::new()));
        assert_eq!(res.status, NfsStatus::Ok);

        res.add_result(Nfs4OpRes::error(Nfs4Op::Read, NfsStatus::Access));
        assert_eq!(res.status, NfsStatus::Access);
    }
}
