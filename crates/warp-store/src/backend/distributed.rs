//! Distributed storage backend with cross-domain replication
//!
//! Combines erasure coding, WireGuard tunnels, and geo-aware routing
//! for fault-tolerant distributed object storage.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                       Distributed Backend                                │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                          │
//! │   PUT Request                          GET Request                       │
//! │   ├─ Encode to shards (warp-ec)        ├─ Get shard locations           │
//! │   ├─ Plan placement across domains     ├─ GeoRouter: find nearest       │
//! │   ├─ Write local shards                ├─ Parallel fetch via WireGuard  │
//! │   ├─ Replicate via WireGuard           ├─ Decode from shards            │
//! │   └─ Wait for write quorum             └─ Return data                   │
//! │                                                                          │
//! │   ┌─────────────────────────────────────────────────────────────────┐   │
//! │   │                    Component Stack                               │   │
//! │   │                                                                  │   │
//! │   │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │   │
//! │   │  │ GeoRouter    │  │ ShardManager │  │ WireGuardTunnelMgr   │   │   │
//! │   │  │ (routing)    │  │ (placement)  │  │ (cross-domain xfer)  │   │   │
//! │   │  └──────────────┘  └──────────────┘  └──────────────────────┘   │   │
//! │   │                                                                  │   │
//! │   │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │   │
//! │   │  │ DomainReg    │  │ ErasureBack  │  │ ReplicationPolicy    │   │   │
//! │   │  │ (health)     │  │ (encoding)   │  │ (per-bucket config)  │   │   │
//! │   │  └──────────────┘  └──────────────┘  └──────────────────────┘   │   │
//! │   └──────────────────────────────────────────────────────────────────┘  │
//! │                                                                          │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use dashmap::DashMap;
use tracing::{debug, info, warn};

use super::erasure::{ErasureBackend, StoreErasureConfig};
use super::{StorageBackend, HpcStorageBackend, StorageProof};
use crate::error::{Error, Result};
use crate::key::ObjectKey;
use crate::object::{FieldData, ListOptions, ObjectData, ObjectList, ObjectMeta, PutOptions};
use crate::replication::{
    DomainId, DomainRegistry, DistributedShardManager, GeoRouter,
    ReplicationPolicy, ShardHealth, ShardIndex, ShardLocation,
    WireGuardTunnelManager, WireGuardConfig,
};

/// Configuration for distributed backend
#[derive(Debug, Clone)]
pub struct DistributedConfig {
    /// Local domain ID
    pub local_domain_id: DomainId,

    /// Default replication policy for buckets without explicit policy
    pub default_policy: ReplicationPolicy,

    /// WireGuard configuration
    pub wireguard: WireGuardConfig,

    /// Write timeout per shard
    pub write_timeout: Duration,

    /// Read timeout per shard
    pub read_timeout: Duration,

    /// Enable background repair
    pub enable_repair: bool,

    /// Repair check interval
    pub repair_interval: Duration,
}

impl Default for DistributedConfig {
    fn default() -> Self {
        Self {
            local_domain_id: 1,
            default_policy: ReplicationPolicy::default(),
            wireguard: WireGuardConfig::default(),
            write_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            enable_repair: true,
            repair_interval: Duration::from_secs(3600),
        }
    }
}

/// Distributed storage backend
///
/// Provides fault-tolerant object storage across multiple domains
/// using erasure coding and WireGuard tunnels.
pub struct DistributedBackend {
    /// Local erasure backend for shard storage
    local_backend: Arc<ErasureBackend>,

    /// Domain registry
    domain_registry: Arc<DomainRegistry>,

    /// WireGuard tunnel manager
    tunnel_manager: Arc<WireGuardTunnelManager>,

    /// Shard distribution manager
    shard_manager: Arc<DistributedShardManager>,

    /// Geo-aware router
    geo_router: Arc<GeoRouter>,

    /// Per-bucket replication policies
    bucket_policies: DashMap<String, ReplicationPolicy>,

    /// Configuration
    config: DistributedConfig,

    /// Bucket registry
    buckets: DashMap<String, ()>,
}

impl DistributedBackend {
    /// Create a new distributed backend
    pub async fn new(
        root: impl AsRef<Path>,
        config: DistributedConfig,
    ) -> Result<Self> {
        // Create erasure config from default policy
        let erasure_config = StoreErasureConfig::new(
            config.default_policy.erasure.data_shards,
            config.default_policy.erasure.parity_shards,
        )?;

        let local_backend = Arc::new(
            ErasureBackend::new(root, erasure_config).await?
        );

        let domain_registry = Arc::new(DomainRegistry::new(config.local_domain_id));
        let tunnel_manager = Arc::new(WireGuardTunnelManager::new(config.wireguard.clone()));
        let shard_manager = Arc::new(DistributedShardManager::new(
            config.local_domain_id,
            config.default_policy.clone(),
        ));
        let geo_router = Arc::new(GeoRouter::new(
            domain_registry.clone(),
            config.local_domain_id,
        ));

        info!(
            local_domain = config.local_domain_id,
            "Initialized distributed backend"
        );

        Ok(Self {
            local_backend,
            domain_registry,
            tunnel_manager,
            shard_manager,
            geo_router,
            bucket_policies: DashMap::new(),
            config,
            buckets: DashMap::new(),
        })
    }

    /// Get the domain registry
    pub fn domain_registry(&self) -> &Arc<DomainRegistry> {
        &self.domain_registry
    }

    /// Get the tunnel manager
    pub fn tunnel_manager(&self) -> &Arc<WireGuardTunnelManager> {
        &self.tunnel_manager
    }

    /// Get the shard manager
    pub fn shard_manager(&self) -> &Arc<DistributedShardManager> {
        &self.shard_manager
    }

    /// Get the geo router
    pub fn geo_router(&self) -> &Arc<GeoRouter> {
        &self.geo_router
    }

    /// Set replication policy for a bucket
    pub fn set_bucket_policy(&self, bucket: &str, policy: ReplicationPolicy) {
        self.bucket_policies.insert(bucket.to_string(), policy);
    }

    /// Get replication policy for a bucket
    pub fn get_bucket_policy(&self, bucket: &str) -> ReplicationPolicy {
        self.bucket_policies
            .get(bucket)
            .map(|p| p.clone())
            .unwrap_or_else(|| self.config.default_policy.clone())
    }

    /// Start the distributed backend (including WireGuard listener)
    pub async fn start(&self) -> Result<()> {
        self.tunnel_manager.start().await?;
        info!("Distributed backend started");
        Ok(())
    }

    /// Stop the distributed backend
    pub async fn stop(&self) {
        self.tunnel_manager.stop().await;
        info!("Distributed backend stopped");
    }

    /// Distributed PUT operation
    async fn distributed_put(
        &self,
        key: &ObjectKey,
        data: ObjectData,
        opts: PutOptions,
    ) -> Result<ObjectMeta> {
        let policy = self.get_bucket_policy(key.bucket());

        // Encode data to shards
        let (shards, meta) = self.local_backend.encode_to_shards(
            data.as_ref(),
            opts.content_type.clone(),
        )?;

        // Get available domains
        let available_domains = self.domain_registry.domain_ids();
        if available_domains.is_empty() {
            // Fallback to local-only storage
            return self.local_backend.put(key, data, opts).await;
        }

        // Plan shard placement
        let placement = self.shard_manager.plan_placement(
            key.bucket(),
            key.key(),
            &policy,
            &available_domains,
        )?;

        // Write shards to domains
        let mut acks = 0;
        let mut shard_locations: HashMap<ShardIndex, ShardLocation> = HashMap::new();
        let local_domain = self.config.local_domain_id;

        for (shard_idx, shard_data) in shards.iter().enumerate() {
            let target_domain = placement.get(&(shard_idx as ShardIndex))
                .copied()
                .unwrap_or(local_domain);

            if target_domain == local_domain {
                // Store locally
                self.local_backend.put_shard(key, shard_idx, shard_data).await?;
                shard_locations.insert(shard_idx as ShardIndex, ShardLocation {
                    domain_id: local_domain,
                    node_id: "local".to_string(),
                    path: format!("{}/{}", key.bucket(), key.key()),
                    last_verified: None,
                    health: ShardHealth::Healthy,
                });
                acks += 1;
            } else {
                // Send to remote domain via WireGuard
                if let Err(e) = self.send_shard_to_domain(
                    target_domain,
                    key,
                    shard_idx,
                    shard_data,
                ).await {
                    warn!(
                        domain = target_domain,
                        shard = shard_idx,
                        error = %e,
                        "Failed to send shard to remote domain"
                    );
                } else {
                    shard_locations.insert(shard_idx as ShardIndex, ShardLocation {
                        domain_id: target_domain,
                        node_id: "remote".to_string(),
                        path: format!("{}/{}", key.bucket(), key.key()),
                        last_verified: None,
                        health: ShardHealth::Healthy,
                    });
                    acks += 1;
                }
            }
        }

        // Check write quorum
        if acks < policy.write_quorum {
            return Err(Error::InsufficientReplicas {
                available: acks,
                required: policy.write_quorum,
            });
        }

        // Store metadata locally
        self.local_backend.store_shard_meta(key, &meta).await?;

        // Register shard distribution
        self.shard_manager.register_distribution(
            key.bucket(),
            key.key(),
            &policy.erasure,
            shard_locations,
        ).await?;

        debug!(
            key = %key,
            shards = shards.len(),
            acks = acks,
            "Stored object with distributed replication"
        );

        Ok(ObjectMeta {
            size: meta.original_size,
            content_hash: meta.content_hash,
            etag: format!("\"{}\"", hex_encode(&meta.content_hash[..16])),
            content_type: opts.content_type,
            created_at: Utc::now(),
            modified_at: Utc::now(),
            version_id: None,
            user_metadata: opts.metadata,
            is_delete_marker: false,
        })
    }

    /// Distributed GET operation
    async fn distributed_get(&self, key: &ObjectKey) -> Result<ObjectData> {
        let policy = self.get_bucket_policy(key.bucket());

        // Get shard metadata
        let meta = self.local_backend.get_shard_meta(key).await.map_err(|_| {
            Error::ObjectNotFound {
                bucket: key.bucket().to_string(),
                key: key.key().to_string(),
            }
        })?;

        let total_shards = meta.data_shards + meta.parity_shards;

        // Try to get shard distribution info
        let distribution = self.shard_manager.get_distribution(key.bucket(), key.key());

        // Plan read based on geo-router
        let mut shards: Vec<Option<Vec<u8>>> = vec![None; total_shards];
        let mut fetched = 0;

        if let Some(dist) = distribution {
            let dist_info = dist.read().await;

            // Get read plan from geo-router
            let read_plan = self.geo_router.plan_read(
                &dist_info,
                policy.read_preference,
                policy.placement.primary_domain,
            ).await?;

            // Fetch shards in parallel (simplified - fetch needed shards)
            for shard_idx in 0..total_shards {
                if fetched >= meta.data_shards {
                    break; // We have enough shards
                }

                let idx = shard_idx as ShardIndex;
                if let Some(domain) = read_plan.preferred_domain(idx) {
                    if domain == self.config.local_domain_id {
                        // Fetch locally
                        if let Ok(Some(shard_data)) = self.local_backend.get_shard(key, shard_idx).await {
                            shards[shard_idx] = Some(shard_data);
                            fetched += 1;
                        }
                    } else {
                        // Fetch from remote domain via WireGuard
                        if let Ok(shard_data) = self.fetch_shard_from_domain(domain, key, shard_idx).await {
                            shards[shard_idx] = Some(shard_data);
                            fetched += 1;
                        }
                    }
                }
            }
        } else {
            // No distribution info - try local shards only
            for shard_idx in 0..total_shards {
                if fetched >= meta.data_shards {
                    break;
                }

                if let Ok(Some(shard_data)) = self.local_backend.get_shard(key, shard_idx).await {
                    shards[shard_idx] = Some(shard_data);
                    fetched += 1;
                }
            }
        }

        // Decode shards
        let original = self.local_backend.decode_from_shards(&shards, meta.original_size)?;

        debug!(
            key = %key,
            shards_fetched = fetched,
            "Retrieved object from distributed storage"
        );

        Ok(ObjectData::from(original))
    }

    /// Send a shard to a remote domain via WireGuard
    async fn send_shard_to_domain(
        &self,
        domain_id: DomainId,
        key: &ObjectKey,
        shard_index: usize,
        data: &[u8],
    ) -> Result<()> {
        // Get domain info for WireGuard endpoint
        let domain = self.domain_registry.get_domain(domain_id)
            .ok_or_else(|| Error::DomainNotFound(domain_id))?;

        let (pubkey, endpoint) = match (domain.wg_pubkey, domain.wg_endpoint) {
            (Some(pk), Some(ep)) => (pk, ep),
            _ => return Err(Error::WireGuard("Domain has no WireGuard config".to_string())),
        };

        // Get or establish tunnel
        let tunnel = self.tunnel_manager
            .get_or_connect(domain_id, pubkey, endpoint)
            .await?;

        // Serialize and send shard data
        // In a real implementation, this would use a proper protocol
        let message = format!("PUT:{}:{}:{}:", key.bucket(), key.key(), shard_index);
        let mut payload = message.into_bytes();
        payload.extend_from_slice(data);

        let tunnel_guard = tunnel.read().await;
        tunnel_guard.send(Bytes::from(payload)).await?;

        debug!(
            domain = domain_id,
            shard = shard_index,
            "Sent shard to remote domain"
        );

        Ok(())
    }

    /// Fetch a shard from a remote domain via WireGuard
    async fn fetch_shard_from_domain(
        &self,
        domain_id: DomainId,
        key: &ObjectKey,
        shard_index: usize,
    ) -> Result<Vec<u8>> {
        // Get domain info for WireGuard endpoint
        let domain = self.domain_registry.get_domain(domain_id)
            .ok_or_else(|| Error::DomainNotFound(domain_id))?;

        let (pubkey, endpoint) = match (domain.wg_pubkey, domain.wg_endpoint) {
            (Some(pk), Some(ep)) => (pk, ep),
            _ => return Err(Error::WireGuard("Domain has no WireGuard config".to_string())),
        };

        // Get or establish tunnel
        let tunnel = self.tunnel_manager
            .get_or_connect(domain_id, pubkey, endpoint)
            .await?;

        // Send request (simplified protocol)
        let request = format!("GET:{}:{}:{}", key.bucket(), key.key(), shard_index);
        let tunnel_guard = tunnel.read().await;
        tunnel_guard.send(Bytes::from(request)).await?;

        // Wait for response
        let response = tunnel_guard.recv().await?;

        debug!(
            domain = domain_id,
            shard = shard_index,
            size = response.len(),
            "Fetched shard from remote domain"
        );

        Ok(response.to_vec())
    }

    /// Run background repair check
    pub async fn run_repair_check(&self) -> Result<usize> {
        let candidates = self.shard_manager.find_repair_candidates().await;
        let mut repaired_total = 0;

        for (bucket, key, _missing_shards) in candidates {
            let obj_key = ObjectKey::new(&bucket, &key)?;

            match self.local_backend.repair_shards(&obj_key).await {
                Ok(count) => {
                    repaired_total += count;
                    info!(
                        bucket = bucket,
                        key = key,
                        repaired = count,
                        "Repaired shards"
                    );
                }
                Err(e) => {
                    warn!(
                        bucket = bucket,
                        key = key,
                        error = %e,
                        "Failed to repair shards"
                    );
                }
            }
        }

        Ok(repaired_total)
    }

    /// Get distributed backend statistics
    pub async fn stats(&self) -> DistributedStats {
        let shard_stats = self.shard_manager.stats().await;
        let geo_stats = self.geo_router.stats();

        DistributedStats {
            local_domain_id: self.config.local_domain_id,
            total_domains: self.domain_registry.domain_ids().len(),
            active_tunnels: self.tunnel_manager.tunnel_count(),
            tracked_objects: shard_stats.total_objects,
            total_shards: shard_stats.total_shards,
            healthy_shards: shard_stats.healthy_shards,
            degraded_objects: shard_stats.degraded_objects,
            avg_latency_ms: geo_stats.avg_latency_ms,
        }
    }
}

/// Distributed backend statistics
#[derive(Debug, Clone)]
pub struct DistributedStats {
    /// Local domain ID
    pub local_domain_id: DomainId,
    /// Total number of known domains
    pub total_domains: usize,
    /// Active WireGuard tunnels
    pub active_tunnels: usize,
    /// Objects being tracked
    pub tracked_objects: usize,
    /// Total shards across all objects
    pub total_shards: usize,
    /// Healthy shards count
    pub healthy_shards: usize,
    /// Objects needing repair
    pub degraded_objects: usize,
    /// Average cross-domain latency
    pub avg_latency_ms: u32,
}

// Hex encoding helper
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[async_trait]
impl StorageBackend for DistributedBackend {
    async fn get(&self, key: &ObjectKey) -> Result<ObjectData> {
        self.distributed_get(key).await
    }

    async fn get_fields(&self, _key: &ObjectKey, _fields: &[&str]) -> Result<FieldData> {
        // Distributed backend doesn't support field-level access
        Ok(FieldData::new())
    }

    async fn put(&self, key: &ObjectKey, data: ObjectData, opts: PutOptions) -> Result<ObjectMeta> {
        self.distributed_put(key, data, opts).await
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        // Delete from local backend
        self.local_backend.delete(key).await?;

        // Remove from shard manager
        self.shard_manager.remove_distribution(key.bucket(), key.key());

        debug!(key = %key, "Deleted distributed object");
        Ok(())
    }

    async fn list(&self, bucket: &str, prefix: &str, opts: ListOptions) -> Result<ObjectList> {
        // Delegate to local backend
        self.local_backend.list(bucket, prefix, opts).await
    }

    async fn head(&self, key: &ObjectKey) -> Result<ObjectMeta> {
        self.local_backend.head(key).await
    }

    async fn create_bucket(&self, name: &str) -> Result<()> {
        self.local_backend.create_bucket(name).await?;
        self.buckets.insert(name.to_string(), ());
        Ok(())
    }

    async fn delete_bucket(&self, name: &str) -> Result<()> {
        self.local_backend.delete_bucket(name).await?;
        self.buckets.remove(name);
        self.bucket_policies.remove(name);
        Ok(())
    }

    async fn bucket_exists(&self, name: &str) -> Result<bool> {
        self.local_backend.bucket_exists(name).await
    }
}

#[async_trait]
impl HpcStorageBackend for DistributedBackend {
    #[cfg(feature = "gpu")]
    async fn pinned_store(
        &self,
        key: &ObjectKey,
        gpu_buffer: &warp_gpu::GpuBuffer<u8>,
    ) -> Result<ObjectMeta> {
        // Copy from GPU to host memory first, then use distributed put
        let data = gpu_buffer.copy_to_host()
            .map_err(|e| Error::Backend(format!("GPU copy failed: {}", e)))?;
        self.put(key, ObjectData::from(data), PutOptions::default()).await
    }

    async fn verified_get(&self, key: &ObjectKey) -> Result<(ObjectData, StorageProof)> {
        let data = self.get(key).await?;
        let meta = self.local_backend.get_shard_meta(key).await?;

        let proof = StorageProof {
            root: meta.content_hash,
            path: vec![],
            leaf_index: 0,
        };

        Ok((data, proof))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_distributed_backend_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = DistributedConfig::default();

        let backend = DistributedBackend::new(temp_dir.path(), config).await.unwrap();

        // Create bucket
        backend.create_bucket("test").await.unwrap();
        assert!(backend.bucket_exists("test").await.unwrap());

        // Put object (will use local-only mode since no remote domains)
        let key = ObjectKey::new("test", "hello.txt").unwrap();
        let data = b"Hello, distributed storage!".to_vec();
        let meta = backend.put(
            &key,
            ObjectData::from(data.clone()),
            PutOptions::default(),
        ).await.unwrap();

        assert_eq!(meta.size, data.len() as u64);

        // Get object
        let retrieved = backend.get(&key).await.unwrap();
        assert_eq!(retrieved.as_ref(), data.as_slice());

        // Delete
        backend.delete(&key).await.unwrap();
        assert!(backend.get(&key).await.is_err());
    }

    #[tokio::test]
    async fn test_bucket_policy() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = DistributedConfig::default();

        let backend = DistributedBackend::new(temp_dir.path(), config).await.unwrap();

        // Set custom policy
        let policy = ReplicationPolicy::high_durability();
        backend.set_bucket_policy("important-data", policy.clone());

        let retrieved = backend.get_bucket_policy("important-data");
        assert_eq!(retrieved.write_quorum, policy.write_quorum);
        assert_eq!(retrieved.placement.min_domain_spread, 3);
    }

    #[tokio::test]
    async fn test_stats() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = DistributedConfig::default();

        let backend = DistributedBackend::new(temp_dir.path(), config).await.unwrap();

        let stats = backend.stats().await;
        assert_eq!(stats.local_domain_id, 1);
        assert_eq!(stats.active_tunnels, 0);
        assert_eq!(stats.tracked_objects, 0);
    }
}
