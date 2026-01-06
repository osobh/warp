//! NVMe-oF Backend Configuration

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

/// NVMe-oF backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeOfBackendConfig {
    /// Target configurations
    pub targets: Vec<NvmeOfTargetConfig>,

    /// Transport preference order
    pub transport_preference: Vec<TransportPreference>,

    /// Connection pool configuration
    pub pool: ConnectionPoolConfig,

    /// Enable automatic failover
    pub enable_failover: bool,

    /// Metadata backend configuration
    pub metadata_backend: MetadataBackendConfig,

    /// Block allocation strategy
    pub allocation_strategy: AllocationStrategy,

    /// Default block size
    pub block_size: u32,

    /// Maximum object size
    pub max_object_size: u64,

    /// Retry configuration
    pub retry: RetryConfig,

    /// Enable read-ahead
    pub enable_readahead: bool,

    /// Read-ahead size in blocks
    pub readahead_blocks: u32,
}

impl Default for NvmeOfBackendConfig {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            transport_preference: vec![
                TransportPreference::Rdma,
                TransportPreference::Quic,
                TransportPreference::Tcp,
            ],
            pool: ConnectionPoolConfig::default(),
            enable_failover: true,
            metadata_backend: MetadataBackendConfig::default(),
            allocation_strategy: AllocationStrategy::default(),
            block_size: 4096,
            max_object_size: 5 * 1024 * 1024 * 1024, // 5GB
            retry: RetryConfig::default(),
            enable_readahead: true,
            readahead_blocks: 32,
        }
    }
}

/// Configuration for a single NVMe-oF target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeOfTargetConfig {
    /// Target NQN (NVMe Qualified Name)
    pub nqn: String,

    /// Target addresses (for multi-path)
    pub addresses: Vec<SocketAddr>,

    /// Host NQN to use when connecting
    pub host_nqn: Option<String>,

    /// Namespace ID to use (0 for all)
    pub namespace_id: Option<u32>,

    /// Priority (lower = higher priority)
    pub priority: u8,

    /// Maximum I/O queue depth
    pub queue_depth: u16,

    /// Number of I/O queues
    pub num_io_queues: u16,

    /// Keep-alive timeout in milliseconds
    pub keep_alive_ms: u32,

    /// Enable reconnect on disconnect
    pub reconnect: bool,

    /// Reconnect delay in milliseconds
    pub reconnect_delay_ms: u32,
}

impl Default for NvmeOfTargetConfig {
    fn default() -> Self {
        Self {
            nqn: String::new(),
            addresses: Vec::new(),
            host_nqn: None,
            namespace_id: None,
            priority: 0,
            queue_depth: 128,
            num_io_queues: 4,
            keep_alive_ms: 120_000,
            reconnect: true,
            reconnect_delay_ms: 5000,
        }
    }
}

/// Transport preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportPreference {
    /// RDMA transport (preferred for datacenter)
    Rdma,
    /// QUIC transport (good for WAN)
    Quic,
    /// TCP transport (fallback)
    Tcp,
}

/// Connection pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Minimum connections per target
    pub min_connections: u32,

    /// Maximum connections per target
    pub max_connections: u32,

    /// Connection acquire timeout
    pub acquire_timeout: Duration,

    /// Idle connection timeout
    pub idle_timeout: Duration,

    /// Health check interval
    pub health_check_interval: Duration,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 1,
            max_connections: 16,
            acquire_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(300),
            health_check_interval: Duration::from_secs(30),
        }
    }
}

/// Metadata backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataBackendConfig {
    /// Backend type
    pub backend_type: MetadataBackendType,

    /// Path for file-based backend
    pub path: Option<String>,

    /// Cache size
    pub cache_size: usize,

    /// Enable write-ahead logging
    pub enable_wal: bool,
}

impl Default for MetadataBackendConfig {
    fn default() -> Self {
        Self {
            backend_type: MetadataBackendType::InMemory,
            path: None,
            cache_size: 10000,
            enable_wal: false,
        }
    }
}

/// Metadata backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetadataBackendType {
    /// In-memory (volatile)
    InMemory,
    /// Sled embedded database
    Sled,
    /// On the NVMe target itself
    OnTarget,
}

/// Block allocation strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationStrategy {
    /// First-fit allocation
    FirstFit,
    /// Best-fit allocation
    BestFit,
    /// Round-robin across namespaces
    RoundRobin,
    /// Striped across namespaces
    Striped,
}

impl Default for AllocationStrategy {
    fn default() -> Self {
        Self::FirstFit
    }
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum retries
    pub max_retries: u32,

    /// Initial backoff
    pub initial_backoff: Duration,

    /// Maximum backoff
    pub max_backoff: Duration,

    /// Backoff multiplier
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NvmeOfBackendConfig::default();
        assert!(config.targets.is_empty());
        assert_eq!(config.transport_preference.len(), 3);
        assert!(config.enable_failover);
    }

    #[test]
    fn test_serialization() {
        let config = NvmeOfBackendConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: NvmeOfBackendConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.block_size, config.block_size);
    }
}
