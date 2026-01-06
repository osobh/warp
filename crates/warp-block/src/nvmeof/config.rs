//! NVMe-oF configuration types
//!
//! Configuration structures for NVMe-oF Target and transport settings.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

/// Main NVMe-oF configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeOfConfig {
    /// Address to bind to for incoming connections
    pub bind_addr: IpAddr,

    /// Port to listen on (default: 4420)
    pub port: u16,

    /// Advertised address for discovery (if different from bind_addr)
    pub advertised_addr: Option<IpAddr>,

    /// TCP transport configuration
    pub tcp: Option<NvmeOfTcpConfig>,

    /// RDMA transport configuration (requires feature)
    pub rdma: Option<NvmeOfRdmaConfig>,

    /// QUIC transport configuration (requires feature)
    pub quic: Option<NvmeOfQuicConfig>,

    /// Maximum number of connections
    pub max_connections: u32,

    /// Maximum number of queues per connection
    pub max_queues_per_connection: u16,

    /// Maximum queue depth
    pub max_queue_depth: u16,

    /// Keep-alive timeout in milliseconds
    pub keep_alive_timeout_ms: u32,

    /// Enable discovery service
    pub enable_discovery: bool,

    /// Discovery port (if different from main port)
    pub discovery_port: Option<u16>,

    /// Enable inline data in capsules
    pub enable_inline_data: bool,

    /// Maximum inline data size (in bytes)
    pub max_inline_data_size: u32,

    /// Maximum I/O size (in bytes)
    pub max_io_size: u32,

    /// Namespace configuration defaults
    pub namespace_defaults: NamespaceDefaults,
}

impl Default for NvmeOfConfig {
    fn default() -> Self {
        Self {
            bind_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 4420,
            advertised_addr: None,
            tcp: Some(NvmeOfTcpConfig::default()),
            rdma: None,
            quic: None,
            max_connections: 1024,
            max_queues_per_connection: 64,
            max_queue_depth: 128,
            keep_alive_timeout_ms: 120_000, // 2 minutes
            enable_discovery: true,
            discovery_port: None,
            enable_inline_data: true,
            max_inline_data_size: 8192, // 8KB
            max_io_size: 1024 * 1024,   // 1MB
            namespace_defaults: NamespaceDefaults::default(),
        }
    }
}

/// TCP transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeOfTcpConfig {
    /// Enable TCP transport
    pub enabled: bool,

    /// Enable TCP_NODELAY
    pub nodelay: bool,

    /// TCP send buffer size
    pub send_buffer_size: Option<usize>,

    /// TCP receive buffer size
    pub recv_buffer_size: Option<usize>,

    /// Enable header digest (CRC32C)
    pub header_digest: bool,

    /// Enable data digest (CRC32C)
    pub data_digest: bool,

    /// Maximum PDU data size for H2C
    pub maxh2cdata: u32,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Idle timeout
    pub idle_timeout: Duration,
}

impl Default for NvmeOfTcpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            nodelay: true,
            send_buffer_size: Some(256 * 1024), // 256KB
            recv_buffer_size: Some(256 * 1024), // 256KB
            header_digest: false,
            data_digest: false,
            maxh2cdata: 131072, // 128KB
            connect_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(300),
        }
    }
}

/// RDMA transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeOfRdmaConfig {
    /// Enable RDMA transport
    pub enabled: bool,

    /// RDMA device name (e.g., "mlx5_0")
    pub device: Option<String>,

    /// RDMA port number
    pub port: u8,

    /// Number of send work requests
    pub send_wr: u32,

    /// Number of receive work requests
    pub recv_wr: u32,

    /// Maximum scatter/gather entries
    pub max_sge: u32,

    /// Enable inline data
    pub inline_data_size: u32,

    /// Memory region size for data transfer
    pub mr_size: usize,

    /// Number of completion queue entries
    pub cq_size: u32,

    /// Use shared receive queue
    pub use_srq: bool,

    /// SRQ size if using SRQ
    pub srq_size: u32,
}

impl Default for NvmeOfRdmaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device: None, // Auto-detect
            port: 1,
            send_wr: 256,
            recv_wr: 256,
            max_sge: 16,
            inline_data_size: 256,
            mr_size: 4 * 1024 * 1024, // 4MB
            cq_size: 512,
            use_srq: true,
            srq_size: 1024,
        }
    }
}

/// QUIC transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeOfQuicConfig {
    /// Enable QUIC transport
    pub enabled: bool,

    /// TLS certificate path
    pub cert_path: Option<String>,

    /// TLS key path
    pub key_path: Option<String>,

    /// ALPN protocol names
    pub alpn_protocols: Vec<String>,

    /// Maximum idle timeout
    pub max_idle_timeout: Duration,

    /// Initial RTT estimate
    pub initial_rtt: Duration,

    /// Maximum UDP payload size
    pub max_udp_payload_size: u16,

    /// Maximum concurrent bidirectional streams
    pub max_concurrent_streams: u32,
}

impl Default for NvmeOfQuicConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: None,
            key_path: None,
            alpn_protocols: vec!["nvme-tcp".to_string()],
            max_idle_timeout: Duration::from_secs(60),
            initial_rtt: Duration::from_millis(100),
            max_udp_payload_size: 1452,
            max_concurrent_streams: 256,
        }
    }
}

/// Generic transport configuration (for initiator)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Transport type
    pub transport_type: TransportType,

    /// Host address
    pub host: String,

    /// Port
    pub port: u16,

    /// Transport-specific options
    pub options: TransportOptions,
}

/// Transport type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// TCP transport
    Tcp,
    /// RDMA transport
    Rdma,
    /// QUIC transport
    Quic,
}

impl TransportType {
    /// Get the default port for this transport
    pub fn default_port(&self) -> u16 {
        match self {
            Self::Tcp | Self::Rdma => 4420,
            Self::Quic => 4421,
        }
    }

    /// Get the transport string (for nvme-cli compatibility)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Rdma => "rdma",
            Self::Quic => "quic",
        }
    }
}

/// Transport-specific options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportOptions {
    /// Header digest
    pub hdr_digest: bool,

    /// Data digest
    pub data_digest: bool,

    /// Connection timeout in milliseconds
    pub timeout_ms: Option<u32>,

    /// Additional key-value options
    pub extra: std::collections::HashMap<String, String>,
}

/// Subsystem configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsystemConfig {
    /// Subsystem name (will be appended to NQN prefix)
    pub name: String,

    /// Custom NQN (overrides name-based generation)
    pub nqn: Option<String>,

    /// Serial number
    pub serial: Option<String>,

    /// Model number
    pub model: Option<String>,

    /// Allow any host to connect
    pub allow_any_host: bool,

    /// Allowed host NQNs (if allow_any_host is false)
    pub allowed_hosts: Vec<String>,

    /// Maximum number of namespaces
    pub max_namespaces: u32,

    /// Asymmetric Namespace Access (ANA) group
    pub ana_group: Option<u32>,
}

impl Default for SubsystemConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            nqn: None,
            serial: None,
            model: None,
            allow_any_host: true,
            allowed_hosts: Vec::new(),
            max_namespaces: 256,
            ana_group: None,
        }
    }
}

/// Namespace configuration defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceDefaults {
    /// Block size in bytes
    pub block_size: u32,

    /// Enable thin provisioning
    pub thin_provisioning: bool,

    /// Enable TRIM/deallocate
    pub enable_trim: bool,

    /// Enable write cache
    pub write_cache: bool,

    /// Read-only by default
    pub read_only: bool,
}

impl Default for NamespaceDefaults {
    fn default() -> Self {
        Self {
            block_size: 4096,
            thin_provisioning: true,
            enable_trim: true,
            write_cache: true,
            read_only: false,
        }
    }
}

/// Discovery log entry for subsystem advertisement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryLogEntry {
    /// Transport type
    pub trtype: TransportType,

    /// Address family
    pub adrfam: AddressFamily,

    /// Subsystem type
    pub subtype: SubsystemType,

    /// Transport requirements
    pub treq: u8,

    /// NVMe port ID
    pub portid: u16,

    /// Controller ID
    pub cntlid: u16,

    /// Admin max SQ size
    pub asqsz: u16,

    /// Transport service identifier (port as string)
    pub trsvcid: String,

    /// Transport address
    pub traddr: String,

    /// Subsystem NQN
    pub subnqn: String,
}

/// Address family
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AddressFamily {
    /// IPv4
    Ipv4 = 1,
    /// IPv6
    Ipv6 = 2,
    /// InfiniBand
    Ib = 3,
    /// Fibre Channel
    Fc = 4,
    /// Intra-host
    Loop = 254,
}

/// Subsystem type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum SubsystemType {
    /// Discovery subsystem (not for I/O)
    Discovery = 1,
    /// NVM subsystem (for I/O)
    Nvm = 2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NvmeOfConfig::default();
        assert_eq!(config.port, 4420);
        assert!(config.tcp.is_some());
        assert!(config.enable_discovery);
    }

    #[test]
    fn test_transport_type() {
        assert_eq!(TransportType::Tcp.default_port(), 4420);
        assert_eq!(TransportType::Rdma.as_str(), "rdma");
    }

    #[test]
    fn test_subsystem_config() {
        let config = SubsystemConfig::default();
        assert!(config.allow_any_host);
        assert_eq!(config.max_namespaces, 256);
    }

    #[test]
    fn test_serialization() {
        let config = NvmeOfConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: NvmeOfConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.port, config.port);
    }
}
