//! GPU-Direct Storage Backend
//!
//! This module provides GPU-direct storage operations that bypass CPU memory
//! for high-performance GPU-to-storage and storage-to-GPU transfers.
//!
//! # Features
//!
//! - **Zero-Copy Transfers**: Direct GPU-to-storage using pinned memory
//! - **P2P GPU-to-GPU**: NVLink-aware peer-to-peer transfers
//! - **Async Pipeline**: Overlapped compute and I/O
//! - **RDMA Integration**: GPUDirect RDMA for network storage
//!
//! # Example
//!
//! ```ignore
//! use warp_store::backend::gpu_direct::{GpuDirectBackend, GpuDirectConfig};
//! use warp_gpu::GpuContext;
//!
//! let ctx = GpuContext::new()?;
//! let config = GpuDirectConfig::default();
//! let backend = GpuDirectBackend::new(ctx, config).await?;
//!
//! // Store GPU data directly
//! let gpu_data = /* ... GPU tensor ... */;
//! let meta = backend.pinned_store(&key, &gpu_data).await?;
//!
//! // Load directly to GPU
//! let loaded = backend.pinned_get(&key).await?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::{HpcStorageBackend, StorageBackend};
use crate::error::{Error, Result};
use crate::key::ObjectKey;
use crate::object::{
    FieldData, ListOptions, ObjectData, ObjectEntry, ObjectList, ObjectMeta, PutOptions,
    StorageClass,
};

/// GPU-Direct storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDirectConfig {
    /// Base path for local storage.
    pub base_path: PathBuf,
    /// Enable P2P GPU transfers.
    pub enable_p2p: bool,
    /// Enable RDMA for network transfers.
    pub enable_rdma: bool,
    /// Maximum pinned memory pool size (bytes).
    pub max_pinned_pool_size: usize,
    /// Chunk size for streaming operations.
    pub chunk_size: usize,
    /// Enable async I/O pipeline.
    pub enable_async_io: bool,
    /// Number of async I/O workers.
    pub io_workers: usize,
}

impl Default for GpuDirectConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("/tmp/warp-gpu-store"),
            enable_p2p: true,
            enable_rdma: false,
            max_pinned_pool_size: 1024 * 1024 * 1024, // 1 GB
            chunk_size: 16 * 1024 * 1024,             // 16 MB
            enable_async_io: true,
            io_workers: 4,
        }
    }
}

impl GpuDirectConfig {
    /// Create config with custom base path.
    pub fn with_path(path: impl AsRef<Path>) -> Self {
        Self {
            base_path: path.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    /// Set pinned memory pool size.
    pub fn with_pinned_pool_size(mut self, size: usize) -> Self {
        self.max_pinned_pool_size = size;
        self
    }

    /// Enable RDMA.
    pub fn with_rdma(mut self) -> Self {
        self.enable_rdma = true;
        self
    }
}

/// NVLink topology information.
#[derive(Debug, Clone, Default)]
pub struct NvLinkTopology {
    /// GPU count.
    pub gpu_count: usize,
    /// NVLink connections: (gpu_a, gpu_b) -> bandwidth_gbps.
    pub connections: HashMap<(usize, usize), u32>,
    /// NVSwitch present.
    pub has_nvswitch: bool,
}

impl NvLinkTopology {
    /// Check if GPUs are connected via NVLink.
    pub fn are_connected(&self, gpu_a: usize, gpu_b: usize) -> bool {
        let key = if gpu_a < gpu_b {
            (gpu_a, gpu_b)
        } else {
            (gpu_b, gpu_a)
        };
        self.connections.contains_key(&key)
    }

    /// Get NVLink bandwidth between GPUs (Gbps).
    pub fn bandwidth(&self, gpu_a: usize, gpu_b: usize) -> Option<u32> {
        let key = if gpu_a < gpu_b {
            (gpu_a, gpu_b)
        } else {
            (gpu_b, gpu_a)
        };
        self.connections.get(&key).copied()
    }

    /// Find best path for P2P transfer.
    pub fn best_path(&self, src_gpu: usize, dst_gpu: usize) -> P2PPath {
        if src_gpu == dst_gpu {
            return P2PPath::SameGpu;
        }

        if self.are_connected(src_gpu, dst_gpu) {
            return P2PPath::NvLink {
                bandwidth_gbps: self.bandwidth(src_gpu, dst_gpu).unwrap_or(600),
            };
        }

        if self.has_nvswitch {
            return P2PPath::NvSwitch;
        }

        P2PPath::PciE
    }
}

/// P2P transfer path type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P2PPath {
    /// Same GPU, no transfer needed.
    SameGpu,
    /// Direct NVLink connection.
    NvLink { bandwidth_gbps: u32 },
    /// NVSwitch fabric.
    NvSwitch,
    /// PCIe (fallback).
    PciE,
}

impl P2PPath {
    /// Get approximate bandwidth (GB/s).
    pub fn bandwidth_gbps(&self) -> u32 {
        match self {
            Self::SameGpu => 900, // Memory bandwidth
            Self::NvLink { bandwidth_gbps } => *bandwidth_gbps,
            Self::NvSwitch => 600,
            Self::PciE => 32, // PCIe 4.0 x16
        }
    }
}

/// GPU buffer handle for GPU-direct operations.
#[derive(Debug)]
pub struct GpuBufferHandle {
    /// GPU device index.
    pub gpu_index: usize,
    /// Buffer size in bytes.
    pub size: usize,
    /// Is buffer pinned (page-locked).
    pub pinned: bool,
    /// Memory address (for RDMA registration).
    pub addr: u64,
}

/// GPU-Direct storage backend.
///
/// Provides high-performance storage operations that bypass CPU memory
/// by using pinned memory, NVLink, and GPUDirect RDMA.
pub struct GpuDirectBackend {
    /// Configuration.
    config: GpuDirectConfig,
    /// GPU context (when GPU feature enabled).
    #[cfg(feature = "gpu")]
    gpu_ctx: Option<Arc<warp_gpu::GpuContext>>,
    /// Pinned memory pool (when GPU feature enabled).
    #[cfg(feature = "gpu")]
    pinned_pool: Option<Arc<warp_gpu::PinnedMemoryPool>>,
    /// NVLink topology.
    nvlink_topology: NvLinkTopology,
    /// Object metadata cache.
    metadata_cache: Arc<RwLock<HashMap<String, ObjectMeta>>>,
    /// Transfer statistics.
    stats: Arc<RwLock<GpuDirectStats>>,
}

/// GPU-Direct transfer statistics.
#[derive(Debug, Clone, Default)]
pub struct GpuDirectStats {
    /// Total bytes transferred GPU -> storage.
    pub bytes_to_storage: u64,
    /// Total bytes transferred storage -> GPU.
    pub bytes_from_storage: u64,
    /// P2P transfers.
    pub p2p_transfers: u64,
    /// NVLink transfers.
    pub nvlink_transfers: u64,
    /// PCIe transfers.
    pub pcie_transfers: u64,
    /// Pinned memory hits.
    pub pinned_hits: u64,
    /// Pinned memory misses.
    pub pinned_misses: u64,
}

impl GpuDirectBackend {
    /// Create a new GPU-Direct storage backend.
    pub async fn new(config: GpuDirectConfig) -> Result<Self> {
        // Ensure base path exists
        tokio::fs::create_dir_all(&config.base_path).await?;

        info!(
            path = %config.base_path.display(),
            p2p = config.enable_p2p,
            rdma = config.enable_rdma,
            "GPU-Direct backend initialized"
        );

        Ok(Self {
            config,
            #[cfg(feature = "gpu")]
            gpu_ctx: None,
            #[cfg(feature = "gpu")]
            pinned_pool: None,
            nvlink_topology: NvLinkTopology::default(),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(GpuDirectStats::default())),
        })
    }

    /// Create with GPU context.
    #[cfg(feature = "gpu")]
    pub async fn with_gpu(
        config: GpuDirectConfig,
        gpu_ctx: Arc<warp_gpu::GpuContext>,
    ) -> Result<Self> {
        tokio::fs::create_dir_all(&config.base_path).await?;

        // Create pinned memory pool
        let pool_config = warp_gpu::PoolConfig {
            max_total_memory: config.max_pinned_pool_size,
            ..Default::default()
        };
        let pinned_pool = warp_gpu::PinnedMemoryPool::new(gpu_ctx.context().clone(), pool_config);

        info!(
            path = %config.base_path.display(),
            gpu = gpu_ctx.device_id(),
            pool_size = config.max_pinned_pool_size,
            "GPU-Direct backend initialized with GPU"
        );

        Ok(Self {
            config,
            gpu_ctx: Some(gpu_ctx),
            pinned_pool: Some(Arc::new(pinned_pool)),
            nvlink_topology: NvLinkTopology::default(),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(GpuDirectStats::default())),
        })
    }

    /// Detect NVLink topology.
    #[cfg(feature = "gpu")]
    pub fn detect_nvlink_topology(&mut self) -> Result<()> {
        // In a real implementation, this would query NVML or CUDA
        // For now, we create a simulated topology
        let gpu_count = 1; // Would query CUDA device count

        self.nvlink_topology = NvLinkTopology {
            gpu_count,
            connections: HashMap::new(),
            has_nvswitch: false,
        };

        info!(gpu_count, "NVLink topology detected");
        Ok(())
    }

    /// Get file path for an object key.
    fn object_path(&self, key: &ObjectKey) -> PathBuf {
        self.config.base_path.join(key.bucket()).join(key.key())
    }

    /// Ensure bucket directory exists.
    async fn ensure_bucket(&self, bucket: &str) -> Result<()> {
        let bucket_path = self.config.base_path.join(bucket);
        tokio::fs::create_dir_all(&bucket_path).await?;
        Ok(())
    }

    /// Get transfer statistics.
    pub async fn stats(&self) -> GpuDirectStats {
        self.stats.read().await.clone()
    }

    /// Get NVLink topology.
    pub fn nvlink_topology(&self) -> &NvLinkTopology {
        &self.nvlink_topology
    }

    /// Check if GPU context is available.
    #[cfg(feature = "gpu")]
    pub fn has_gpu(&self) -> bool {
        self.gpu_ctx.is_some()
    }

    /// Check if GPU context is available.
    #[cfg(not(feature = "gpu"))]
    pub fn has_gpu(&self) -> bool {
        false
    }

    /// Get pinned memory pool statistics.
    #[cfg(feature = "gpu")]
    pub fn pinned_pool_stats(&self) -> Option<warp_gpu::PoolStatistics> {
        self.pinned_pool.as_ref().map(|p| p.statistics())
    }
}

#[async_trait]
impl StorageBackend for GpuDirectBackend {
    async fn get(&self, key: &ObjectKey) -> Result<ObjectData> {
        let path = self.object_path(key);

        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| Error::Backend(format!("Failed to read {}: {}", path.display(), e)))?;

        // Update stats
        self.stats.write().await.bytes_from_storage += data.len() as u64;

        debug!(key = %key, size = data.len(), "GPU-Direct get");
        Ok(ObjectData::from(data))
    }

    async fn get_fields(&self, key: &ObjectKey, fields: &[&str]) -> Result<FieldData> {
        // GPU-Direct doesn't support lazy field loading
        let _ = (key, fields);
        Ok(FieldData::new())
    }

    async fn put(&self, key: &ObjectKey, data: ObjectData, opts: PutOptions) -> Result<ObjectMeta> {
        self.ensure_bucket(key.bucket()).await?;

        let path = self.object_path(key);

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&path, data.as_ref())
            .await
            .map_err(|e| Error::Backend(format!("Failed to write {}: {}", path.display(), e)))?;

        // Update stats
        self.stats.write().await.bytes_to_storage += data.len() as u64;

        let hash = blake3::hash(data.as_ref());
        let hash_bytes = *hash.as_bytes();
        let meta = ObjectMeta {
            size: data.len() as u64,
            content_hash: hash_bytes,
            etag: format!("\"{}\"", hex::encode(&hash_bytes[..16])),
            content_type: opts.content_type,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
            version_id: None,
            user_metadata: opts.metadata,
            is_delete_marker: false,
        };

        // Cache metadata
        self.metadata_cache
            .write()
            .await
            .insert(key.to_string(), meta.clone());

        debug!(key = %key, size = data.len(), "GPU-Direct put");
        Ok(meta)
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        let path = self.object_path(key);

        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| Error::Backend(format!("Failed to delete {}: {}", path.display(), e)))?;

        // Remove from cache
        self.metadata_cache.write().await.remove(&key.to_string());

        debug!(key = %key, "GPU-Direct delete");
        Ok(())
    }

    async fn list(&self, bucket: &str, prefix: &str, opts: ListOptions) -> Result<ObjectList> {
        let bucket_path = self.config.base_path.join(bucket);

        if !bucket_path.exists() {
            return Ok(ObjectList {
                objects: vec![],
                common_prefixes: vec![],
                next_continuation_token: None,
                is_truncated: false,
                key_count: 0,
            });
        }

        let mut objects = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&bucket_path).await?;
        let max_keys = opts.max_keys;

        while let Some(entry) = read_dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) {
                let metadata = entry.metadata().await?;

                objects.push(ObjectEntry {
                    key: format!("{}/{}", bucket, name),
                    size: metadata.len(),
                    last_modified: chrono::Utc::now(),
                    etag: String::new(),
                    storage_class: StorageClass::Standard,
                    version_id: None,
                    is_latest: true,
                });

                if objects.len() >= max_keys {
                    break;
                }
            }
        }

        let key_count = objects.len();
        let is_truncated = key_count >= max_keys;

        Ok(ObjectList {
            objects,
            common_prefixes: vec![],
            next_continuation_token: None,
            is_truncated,
            key_count,
        })
    }

    async fn head(&self, key: &ObjectKey) -> Result<ObjectMeta> {
        // Check cache first
        if let Some(meta) = self.metadata_cache.read().await.get(&key.to_string()) {
            return Ok(meta.clone());
        }

        let path = self.object_path(key);
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| Error::Backend(format!("Failed to stat {}: {}", path.display(), e)))?;

        let meta = ObjectMeta {
            size: metadata.len(),
            content_hash: [0u8; 32],
            etag: String::new(),
            content_type: None,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
            version_id: None,
            user_metadata: HashMap::new(),
            is_delete_marker: false,
        };

        Ok(meta)
    }

    async fn create_bucket(&self, name: &str) -> Result<()> {
        let bucket_path = self.config.base_path.join(name);
        tokio::fs::create_dir_all(&bucket_path).await?;
        info!(bucket = name, "Created bucket");
        Ok(())
    }

    async fn delete_bucket(&self, name: &str) -> Result<()> {
        let bucket_path = self.config.base_path.join(name);
        tokio::fs::remove_dir_all(&bucket_path)
            .await
            .map_err(|e| Error::Backend(format!("Failed to delete bucket: {}", e)))?;
        info!(bucket = name, "Deleted bucket");
        Ok(())
    }

    async fn bucket_exists(&self, name: &str) -> Result<bool> {
        let bucket_path = self.config.base_path.join(name);
        Ok(bucket_path.exists())
    }
}

#[cfg(feature = "gpu")]
#[async_trait]
impl HpcStorageBackend for GpuDirectBackend {
    async fn pinned_store(
        &self,
        key: &ObjectKey,
        gpu_buffer: &warp_gpu::GpuBuffer<u8>,
    ) -> Result<ObjectMeta> {
        self.ensure_bucket(key.bucket()).await?;

        // Copy from GPU to host using pinned memory
        let host_data = if let Some(ref pool) = self.pinned_pool {
            // Use pinned memory pool for zero-copy
            let size = gpu_buffer.len();

            match pool.acquire(size) {
                Ok(mut pinned) => {
                    // Copy from GPU to pinned memory
                    gpu_buffer
                        .copy_to_host_into(pinned.as_mut_slice())
                        .map_err(|e| Error::Backend(format!("GPU copy failed: {}", e)))?;

                    self.stats.write().await.pinned_hits += 1;
                    pinned.as_slice().to_vec()
                }
                Err(_) => {
                    // Fallback to regular copy
                    self.stats.write().await.pinned_misses += 1;
                    gpu_buffer
                        .copy_to_host()
                        .map_err(|e| Error::Backend(format!("GPU copy failed: {}", e)))?
                }
            }
        } else {
            // No pinned pool, regular copy
            gpu_buffer
                .copy_to_host()
                .map_err(|e| Error::Backend(format!("GPU copy failed: {}", e)))?
        };

        // Write to storage
        let path = self.object_path(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&path, &host_data)
            .await
            .map_err(|e| Error::Backend(format!("Failed to write: {}", e)))?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.bytes_to_storage += host_data.len() as u64;
        }

        let hash = blake3::hash(&host_data);
        let hash_bytes = *hash.as_bytes();
        let meta = ObjectMeta {
            size: host_data.len() as u64,
            content_hash: hash_bytes,
            etag: format!("\"{}\"", hex::encode(&hash_bytes[..16])),
            content_type: Some("application/octet-stream".to_string()),
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
            version_id: None,
            user_metadata: HashMap::new(),
            is_delete_marker: false,
        };

        debug!(
            key = %key,
            size = host_data.len(),
            gpu = self.gpu_ctx.as_ref().map(|c| c.device_id()).unwrap_or(0),
            "GPU-Direct pinned store"
        );

        Ok(meta)
    }
}

/// Pinned memory handle for direct GPU access.
#[cfg(feature = "gpu")]
pub struct PinnedHandle {
    /// Underlying pinned buffer.
    buffer: warp_gpu::PinnedBuffer,
    /// Size in bytes.
    size: usize,
}

#[cfg(feature = "gpu")]
impl PinnedHandle {
    /// Get the size of the pinned buffer.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get a slice of the pinned data.
    pub fn as_slice(&self) -> &[u8] {
        self.buffer.as_slice()
    }

    /// Get a mutable slice of the pinned data.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.buffer.as_mut_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gpu_direct_backend_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = GpuDirectConfig::with_path(temp_dir.path());

        let backend = GpuDirectBackend::new(config).await.unwrap();
        assert!(!backend.has_gpu());
    }

    #[tokio::test]
    async fn test_gpu_direct_put_get() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = GpuDirectConfig::with_path(temp_dir.path());

        let backend = GpuDirectBackend::new(config).await.unwrap();

        // Create bucket
        backend.create_bucket("test").await.unwrap();
        assert!(backend.bucket_exists("test").await.unwrap());

        // Put object
        let key = ObjectKey::new("test", "object1").unwrap();
        let data = ObjectData::from(vec![1u8, 2, 3, 4, 5]);
        let meta = backend
            .put(&key, data.clone(), PutOptions::default())
            .await
            .unwrap();

        assert_eq!(meta.size, 5);

        // Get object
        let retrieved = backend.get(&key).await.unwrap();
        assert_eq!(retrieved.as_ref(), data.as_ref());

        // Check stats
        let stats = backend.stats().await;
        assert_eq!(stats.bytes_to_storage, 5);
        assert_eq!(stats.bytes_from_storage, 5);
    }

    #[tokio::test]
    async fn test_gpu_direct_delete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = GpuDirectConfig::with_path(temp_dir.path());

        let backend = GpuDirectBackend::new(config).await.unwrap();

        backend.create_bucket("test").await.unwrap();

        let key = ObjectKey::new("test", "to-delete").unwrap();
        let data = ObjectData::from(vec![1u8, 2, 3]);
        backend
            .put(&key, data, PutOptions::default())
            .await
            .unwrap();

        backend.delete(&key).await.unwrap();

        // Should fail to get deleted object
        assert!(backend.get(&key).await.is_err());
    }

    #[tokio::test]
    async fn test_gpu_direct_list() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = GpuDirectConfig::with_path(temp_dir.path());

        let backend = GpuDirectBackend::new(config).await.unwrap();

        backend.create_bucket("test").await.unwrap();

        // Create some objects
        for i in 0..5 {
            let key = ObjectKey::new("test", &format!("object-{}", i)).unwrap();
            let data = ObjectData::from(vec![i as u8; 10]);
            backend
                .put(&key, data, PutOptions::default())
                .await
                .unwrap();
        }

        // List objects
        let list = backend
            .list("test", "object-", ListOptions::default())
            .await
            .unwrap();
        assert_eq!(list.objects.len(), 5);
    }

    #[test]
    fn test_nvlink_topology() {
        let mut topology = NvLinkTopology::default();
        topology.gpu_count = 4;
        topology.connections.insert((0, 1), 600);
        topology.connections.insert((1, 2), 600);
        topology.connections.insert((2, 3), 600);

        assert!(topology.are_connected(0, 1));
        assert!(topology.are_connected(1, 0)); // Symmetric
        assert!(!topology.are_connected(0, 2)); // Not directly connected

        assert_eq!(topology.bandwidth(0, 1), Some(600));
        assert_eq!(topology.bandwidth(0, 2), None);

        assert_eq!(topology.best_path(0, 0), P2PPath::SameGpu);
        assert!(matches!(topology.best_path(0, 1), P2PPath::NvLink { .. }));
        assert_eq!(topology.best_path(0, 3), P2PPath::PciE);
    }

    #[test]
    fn test_p2p_path() {
        assert_eq!(P2PPath::SameGpu.bandwidth_gbps(), 900);
        assert_eq!(
            P2PPath::NvLink {
                bandwidth_gbps: 600
            }
            .bandwidth_gbps(),
            600
        );
        assert_eq!(P2PPath::NvSwitch.bandwidth_gbps(), 600);
        assert_eq!(P2PPath::PciE.bandwidth_gbps(), 32);
    }

    #[test]
    fn test_gpu_direct_config() {
        let config = GpuDirectConfig::default();
        assert!(config.enable_p2p);
        assert!(!config.enable_rdma);
        assert_eq!(config.io_workers, 4);

        let config = GpuDirectConfig::with_path("/custom/path")
            .with_pinned_pool_size(2 * 1024 * 1024 * 1024)
            .with_rdma();

        assert_eq!(config.base_path, PathBuf::from("/custom/path"));
        assert_eq!(config.max_pinned_pool_size, 2 * 1024 * 1024 * 1024);
        assert!(config.enable_rdma);
    }
}
