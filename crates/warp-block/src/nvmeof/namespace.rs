//! NVMe-oF Namespace Implementation
//!
//! This module provides the NvmeOfNamespace which maps NVMe namespaces
//! to WARP block volumes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::RwLock;
use tracing::{debug, trace, warn};

use super::command::IdentifyNamespace;
use super::config::NamespaceDefaults;
use super::connection::NamespaceHandler;
use super::error::{NvmeOfError, NvmeOfResult, NvmeStatus};

use crate::volume::VolumeId;

/// Namespace ID (1-based, 0 is broadcast)
pub type NamespaceId = u32;

/// Async volume trait for NVMe-oF operations
#[async_trait]
pub trait AsyncVolume: Send + Sync {
    /// Get volume ID
    fn id(&self) -> VolumeId;

    /// Get volume size in bytes
    fn size_bytes(&self) -> u64;

    /// Get block size
    fn block_size(&self) -> u32;

    /// Read data at offset
    async fn read(&self, offset: u64, length: usize) -> crate::error::BlockResult<Bytes>;

    /// Write data at offset
    async fn write(&self, offset: u64, data: Bytes) -> crate::error::BlockResult<()>;

    /// Flush pending writes
    async fn flush(&self) -> crate::error::BlockResult<()>;

    /// Trim/deallocate region
    async fn trim(&self, offset: u64, length: u64) -> crate::error::BlockResult<()>;
}

/// NVMe-oF Namespace
///
/// Represents an NVMe namespace backed by a WARP block volume.
pub struct NvmeOfNamespace {
    /// Namespace ID
    nsid: NamespaceId,

    /// Backing volume
    volume: Arc<dyn AsyncVolume>,

    /// Block size in bytes
    block_size: u32,

    /// Total size in blocks
    size_blocks: u64,

    /// Namespace GUID (16 bytes)
    nguid: [u8; 16],

    /// EUI-64 identifier
    eui64: [u8; 8],

    /// Read-only flag
    read_only: AtomicBool,

    /// Namespace is attached
    attached: AtomicBool,

    /// Statistics
    stats: RwLock<NamespaceStats>,

    /// Configuration
    config: NamespaceDefaults,
}

impl NvmeOfNamespace {
    /// Create a new namespace backed by a volume
    pub fn new(
        nsid: NamespaceId,
        volume: Arc<dyn AsyncVolume>,
        config: NamespaceDefaults,
    ) -> NvmeOfResult<Self> {
        let volume_size = volume.size_bytes();
        let block_size = config.block_size;

        if volume_size == 0 {
            return Err(NvmeOfError::Namespace("Volume size is zero".to_string()));
        }

        let size_blocks = volume_size / block_size as u64;

        // Generate NGUID from volume ID
        let volume_id = volume.id();
        let mut nguid = [0u8; 16];
        let id_bytes = volume_id.raw().to_le_bytes();
        nguid[..8].copy_from_slice(&id_bytes);

        // Generate EUI-64 from NGUID
        let mut eui64 = [0u8; 8];
        eui64.copy_from_slice(&nguid[..8]);

        debug!(
            "Created namespace {}: size={} blocks, block_size={}",
            nsid, size_blocks, block_size
        );

        Ok(Self {
            nsid,
            volume,
            block_size,
            size_blocks,
            nguid,
            eui64,
            read_only: AtomicBool::new(config.read_only),
            attached: AtomicBool::new(true),
            stats: RwLock::new(NamespaceStats::default()),
            config,
        })
    }

    /// Get namespace ID
    pub fn nsid(&self) -> NamespaceId {
        self.nsid
    }

    /// Get volume reference
    pub fn volume(&self) -> &Arc<dyn AsyncVolume> {
        &self.volume
    }

    /// Get block size
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Get size in blocks
    pub fn size_blocks(&self) -> u64 {
        self.size_blocks
    }

    /// Get size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.size_blocks * self.block_size as u64
    }

    /// Check if namespace is read-only
    pub fn is_read_only(&self) -> bool {
        self.read_only.load(Ordering::Relaxed)
    }

    /// Set read-only mode
    pub fn set_read_only(&self, read_only: bool) {
        self.read_only.store(read_only, Ordering::Relaxed);
    }

    /// Check if namespace is attached
    pub fn is_attached(&self) -> bool {
        self.attached.load(Ordering::Relaxed)
    }

    /// Attach namespace
    pub fn attach(&self) {
        self.attached.store(true, Ordering::Relaxed);
        debug!("Namespace {} attached", self.nsid);
    }

    /// Detach namespace
    pub fn detach(&self) {
        self.attached.store(false, Ordering::Relaxed);
        debug!("Namespace {} detached", self.nsid);
    }

    /// Get NGUID
    pub fn nguid(&self) -> &[u8; 16] {
        &self.nguid
    }

    /// Get EUI-64
    pub fn eui64(&self) -> &[u8; 8] {
        &self.eui64
    }

    /// Generate Identify Namespace data
    pub fn identify(&self) -> IdentifyNamespace {
        let mut ns = IdentifyNamespace::new(self.size_bytes(), self.block_size);
        ns.nguid = self.nguid;
        ns.eui64 = self.eui64;

        // Enable features based on config
        if self.config.thin_provisioning {
            ns.nsfeat |= 0x01; // Thin provisioning
        }
        if self.config.enable_trim {
            ns.nsfeat |= 0x04; // Deallocate
        }

        ns
    }

    /// Get statistics
    pub fn stats(&self) -> NamespaceStats {
        self.stats.read().clone()
    }

    /// Validate LBA range
    fn validate_lba_range(&self, slba: u64, nlb: u32) -> NvmeOfResult<()> {
        let end_lba = slba.saturating_add(nlb as u64);
        if end_lba > self.size_blocks {
            return Err(NvmeOfError::InvalidCommand {
                opcode: 0,
                status: NvmeStatus::InvalidField.to_raw(),
            });
        }
        Ok(())
    }

    /// Read blocks from namespace
    pub async fn read(&self, slba: u64, nlb: u32) -> NvmeOfResult<Bytes> {
        if !self.is_attached() {
            return Err(NvmeOfError::Namespace("Namespace not attached".to_string()));
        }

        self.validate_lba_range(slba, nlb)?;

        let offset = slba * self.block_size as u64;
        let length = nlb as u64 * self.block_size as u64;

        trace!(
            "Namespace {} read: slba={}, nlb={}, offset={}, length={}",
            self.nsid,
            slba,
            nlb,
            offset,
            length
        );

        let data = self.volume.read(offset, length as usize).await?;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.read_commands += 1;
            stats.bytes_read += data.len() as u64;
        }

        Ok(data)
    }

    /// Write blocks to namespace
    pub async fn write(&self, slba: u64, nlb: u32, data: Bytes) -> NvmeOfResult<()> {
        if !self.is_attached() {
            return Err(NvmeOfError::Namespace("Namespace not attached".to_string()));
        }

        if self.is_read_only() {
            return Err(NvmeOfError::InvalidCommand {
                opcode: 0x01,
                status: NvmeStatus::NamespaceWriteProtected.to_raw(),
            });
        }

        self.validate_lba_range(slba, nlb)?;

        let offset = slba * self.block_size as u64;
        let expected_length = nlb as u64 * self.block_size as u64;

        if data.len() as u64 != expected_length {
            warn!(
                "Write data length mismatch: expected {}, got {}",
                expected_length,
                data.len()
            );
        }

        trace!(
            "Namespace {} write: slba={}, nlb={}, offset={}, length={}",
            self.nsid,
            slba,
            nlb,
            offset,
            data.len()
        );

        self.volume.write(offset, data.clone()).await?;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.write_commands += 1;
            stats.bytes_written += data.len() as u64;
        }

        Ok(())
    }

    /// Flush namespace
    pub async fn flush(&self) -> NvmeOfResult<()> {
        if !self.is_attached() {
            return Err(NvmeOfError::Namespace("Namespace not attached".to_string()));
        }

        trace!("Namespace {} flush", self.nsid);

        self.volume.flush().await?;

        self.stats.write().flush_commands += 1;

        Ok(())
    }

    /// Write zeroes
    pub async fn write_zeroes(&self, slba: u64, nlb: u32) -> NvmeOfResult<()> {
        if !self.is_attached() {
            return Err(NvmeOfError::Namespace("Namespace not attached".to_string()));
        }

        if self.is_read_only() {
            return Err(NvmeOfError::InvalidCommand {
                opcode: 0x08,
                status: NvmeStatus::NamespaceWriteProtected.to_raw(),
            });
        }

        self.validate_lba_range(slba, nlb)?;

        let offset = slba * self.block_size as u64;
        let length = nlb as u64 * self.block_size as u64;

        trace!(
            "Namespace {} write_zeroes: slba={}, nlb={}, offset={}, length={}",
            self.nsid,
            slba,
            nlb,
            offset,
            length
        );

        // Create zero buffer
        let zeros = Bytes::from(vec![0u8; length as usize]);
        self.volume.write(offset, zeros).await?;

        self.stats.write().write_zeroes_commands += 1;

        Ok(())
    }

    /// TRIM/Deallocate
    pub async fn trim(&self, slba: u64, nlb: u32) -> NvmeOfResult<()> {
        if !self.is_attached() {
            return Err(NvmeOfError::Namespace("Namespace not attached".to_string()));
        }

        if self.is_read_only() {
            return Err(NvmeOfError::InvalidCommand {
                opcode: 0x09,
                status: NvmeStatus::NamespaceWriteProtected.to_raw(),
            });
        }

        self.validate_lba_range(slba, nlb)?;

        let offset = slba * self.block_size as u64;
        let length = nlb as u64 * self.block_size as u64;

        trace!(
            "Namespace {} trim: slba={}, nlb={}, offset={}, length={}",
            self.nsid,
            slba,
            nlb,
            offset,
            length
        );

        // Delegate to volume's trim if supported
        self.volume.trim(offset, length).await?;

        self.stats.write().dsm_commands += 1;

        Ok(())
    }
}

/// Namespace statistics
#[derive(Debug, Clone, Default)]
pub struct NamespaceStats {
    /// Read commands processed
    pub read_commands: u64,

    /// Write commands processed
    pub write_commands: u64,

    /// Flush commands processed
    pub flush_commands: u64,

    /// Write zeroes commands processed
    pub write_zeroes_commands: u64,

    /// Dataset management commands processed
    pub dsm_commands: u64,

    /// Total bytes read
    pub bytes_read: u64,

    /// Total bytes written
    pub bytes_written: u64,
}

/// Namespace manager for a subsystem
pub struct NamespaceManager {
    /// Namespaces by NSID
    namespaces: RwLock<std::collections::HashMap<NamespaceId, Arc<NvmeOfNamespace>>>,

    /// Maximum namespaces
    max_namespaces: u32,

    /// Configuration defaults
    defaults: NamespaceDefaults,
}

impl NamespaceManager {
    /// Create a new namespace manager
    pub fn new(max_namespaces: u32, defaults: NamespaceDefaults) -> Self {
        Self {
            namespaces: RwLock::new(std::collections::HashMap::new()),
            max_namespaces,
            defaults,
        }
    }

    /// Add a namespace
    pub fn add(
        &self,
        nsid: NamespaceId,
        volume: Arc<dyn AsyncVolume>,
    ) -> NvmeOfResult<Arc<NvmeOfNamespace>> {
        if nsid == 0 {
            return Err(NvmeOfError::Namespace(
                "Namespace ID 0 is reserved".to_string(),
            ));
        }

        let mut namespaces = self.namespaces.write();

        if namespaces.len() >= self.max_namespaces as usize {
            return Err(NvmeOfError::ResourceExhausted(
                "Maximum namespaces reached".to_string(),
            ));
        }

        if namespaces.contains_key(&nsid) {
            return Err(NvmeOfError::Namespace(format!(
                "Namespace {} already exists",
                nsid
            )));
        }

        let namespace = Arc::new(NvmeOfNamespace::new(nsid, volume, self.defaults.clone())?);
        namespaces.insert(nsid, namespace.clone());

        debug!("Added namespace {} to manager", nsid);
        Ok(namespace)
    }

    /// Remove a namespace
    pub fn remove(&self, nsid: NamespaceId) -> NvmeOfResult<()> {
        let mut namespaces = self.namespaces.write();

        if namespaces.remove(&nsid).is_none() {
            return Err(NvmeOfError::Namespace(format!(
                "Namespace {} not found",
                nsid
            )));
        }

        debug!("Removed namespace {} from manager", nsid);
        Ok(())
    }

    /// Get a namespace by ID
    pub fn get(&self, nsid: NamespaceId) -> Option<Arc<NvmeOfNamespace>> {
        self.namespaces.read().get(&nsid).cloned()
    }

    /// Get all namespace IDs
    pub fn list(&self) -> Vec<NamespaceId> {
        self.namespaces.read().keys().copied().collect()
    }

    /// Get count of namespaces
    pub fn count(&self) -> usize {
        self.namespaces.read().len()
    }
}

/// Namespace handler implementation that delegates to namespace manager
pub struct NamespaceHandlerImpl {
    manager: Arc<NamespaceManager>,
}

impl NamespaceHandlerImpl {
    /// Create a new namespace handler
    pub fn new(manager: Arc<NamespaceManager>) -> Self {
        Self { manager }
    }

    fn get_namespace(&self, nsid: u32) -> NvmeOfResult<Arc<NvmeOfNamespace>> {
        self.manager
            .get(nsid)
            .ok_or_else(|| NvmeOfError::InvalidCommand {
                opcode: 0,
                status: NvmeStatus::InvalidNamespaceOrFormat.to_raw(),
            })
    }
}

#[async_trait]
impl NamespaceHandler for NamespaceHandlerImpl {
    async fn read(&self, nsid: u32, slba: u64, nlb: u32) -> NvmeOfResult<Bytes> {
        let ns = self.get_namespace(nsid)?;
        ns.read(slba, nlb).await
    }

    async fn write(&self, nsid: u32, slba: u64, nlb: u32, data: Bytes) -> NvmeOfResult<()> {
        let ns = self.get_namespace(nsid)?;
        ns.write(slba, nlb, data).await
    }

    async fn flush(&self, nsid: u32) -> NvmeOfResult<()> {
        let ns = self.get_namespace(nsid)?;
        ns.flush().await
    }

    async fn write_zeroes(&self, nsid: u32, slba: u64, nlb: u32) -> NvmeOfResult<()> {
        let ns = self.get_namespace(nsid)?;
        ns.write_zeroes(slba, nlb).await
    }

    async fn trim(&self, nsid: u32, slba: u64, nlb: u32) -> NvmeOfResult<()> {
        let ns = self.get_namespace(nsid)?;
        ns.trim(slba, nlb).await
    }

    async fn size(&self, nsid: u32) -> NvmeOfResult<u64> {
        let ns = self.get_namespace(nsid)?;
        Ok(ns.size_blocks())
    }

    async fn block_size(&self, nsid: u32) -> NvmeOfResult<u32> {
        let ns = self.get_namespace(nsid)?;
        Ok(ns.block_size())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require mock Volume implementation
    // These are placeholder tests for basic structures

    #[test]
    fn test_namespace_stats() {
        let stats = NamespaceStats::default();
        assert_eq!(stats.read_commands, 0);
        assert_eq!(stats.bytes_written, 0);
    }

    #[test]
    fn test_namespace_manager_creation() {
        let manager = NamespaceManager::new(256, NamespaceDefaults::default());
        assert_eq!(manager.count(), 0);
        assert!(manager.list().is_empty());
    }
}
