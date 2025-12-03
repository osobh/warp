//! Core types for Portal network
//!
//! This module defines the fundamental types used throughout the Portal networking layer:
//! - Virtual IP addresses in the 10.0.0.0/16 subnet
//! - Peer configuration and metadata
//! - Network events and status tracking
//! - Configuration structures for hub and mDNS

use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::SystemTime;

use crate::{PortalNetError, Result};

/// Virtual IP address in the 10.portal.0.0/16 subnet
///
/// Portal uses the 10.0.0.0/16 private subnet for virtual networking.
/// The hub always uses 10.0.0.1, and peers are assigned addresses
/// in the range 10.0.0.2 to 10.0.255.255.
///
/// # Examples
///
/// ```
/// use portal_net::VirtualIp;
///
/// // Create a virtual IP for host 42
/// let vip = VirtualIp::new(42);
/// assert_eq!(vip.host(), 42);
///
/// // Hub constant
/// assert_eq!(VirtualIp::HUB.host(), 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualIp(Ipv4Addr);

impl VirtualIp {
    /// The network prefix for the Portal subnet (10.0)
    const NETWORK_PREFIX: [u8; 2] = [10, 0];

    /// Hub virtual IP address (10.0.0.1)
    pub const HUB: Self = VirtualIp(Ipv4Addr::new(10, 0, 0, 1));

    /// Creates a new virtual IP address from a 16-bit host identifier
    ///
    /// The host identifier is split into two octets (high byte and low byte)
    /// and combined with the 10.0 network prefix.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_net::VirtualIp;
    ///
    /// let vip = VirtualIp::new(0x0102); // Creates 10.0.1.2
    /// assert_eq!(vip.host(), 0x0102);
    /// ```
    pub fn new(host: u16) -> Self {
        let high = (host >> 8) as u8;
        let low = (host & 0xFF) as u8;
        VirtualIp(Ipv4Addr::new(10, 0, high, low))
    }

    /// Extracts the 16-bit host identifier from the virtual IP
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_net::VirtualIp;
    ///
    /// let vip = VirtualIp::new(0x1234);
    /// assert_eq!(vip.host(), 0x1234);
    /// ```
    pub fn host(&self) -> u16 {
        let octets = self.0.octets();
        ((octets[2] as u16) << 8) | (octets[3] as u16)
    }

    /// Checks if the IP address is in the Portal subnet (10.0.0.0/16)
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_net::VirtualIp;
    ///
    /// let vip = VirtualIp::new(100);
    /// assert!(vip.is_portal_subnet());
    /// ```
    pub fn is_portal_subnet(&self) -> bool {
        let octets = self.0.octets();
        octets[0] == Self::NETWORK_PREFIX[0] && octets[1] == Self::NETWORK_PREFIX[1]
    }

    /// Returns the underlying IPv4 address
    pub fn as_ipv4(&self) -> Ipv4Addr {
        self.0
    }

    /// Attempts to create a VirtualIp from an arbitrary IPv4 address
    ///
    /// Returns an error if the address is not in the 10.0.0.0/16 subnet.
    pub fn from_ipv4(addr: Ipv4Addr) -> Result<Self> {
        let vip = VirtualIp(addr);
        if vip.is_portal_subnet() {
            Ok(vip)
        } else {
            Err(PortalNetError::InvalidVirtualIp(format!(
                "{} is not in the 10.0.0.0/16 subnet",
                addr
            )))
        }
    }
}

impl fmt::Display for VirtualIp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<VirtualIp> for Ipv4Addr {
    fn from(vip: VirtualIp) -> Self {
        vip.0
    }
}

impl From<VirtualIp> for IpAddr {
    fn from(vip: VirtualIp) -> Self {
        IpAddr::V4(vip.0)
    }
}

/// Peer connection status
///
/// Tracks the current connection mode for a peer in the mesh network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerStatus {
    /// Connection status unknown (initial state)
    Unknown,
    /// Peer is online but connection mode not yet determined
    Online,
    /// Peer is offline or unreachable
    Offline,
    /// Direct peer-to-peer connection established (optimal)
    DirectP2P,
    /// Connection relayed through hub (NAT traversal fallback)
    Relayed,
}

impl Default for PeerStatus {
    fn default() -> Self {
        PeerStatus::Unknown
    }
}

impl fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerStatus::Unknown => write!(f, "unknown"),
            PeerStatus::Online => write!(f, "online"),
            PeerStatus::Offline => write!(f, "offline"),
            PeerStatus::DirectP2P => write!(f, "direct-p2p"),
            PeerStatus::Relayed => write!(f, "relayed"),
        }
    }
}

/// Peer configuration
///
/// Contains the essential configuration needed to establish a WireGuard
/// connection with a peer in the mesh network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerConfig {
    /// WireGuard public key (X25519)
    pub public_key: [u8; 32],

    /// Virtual IP address assigned to this peer
    pub virtual_ip: VirtualIp,

    /// Optional known endpoint (IP:port) for the peer
    pub endpoint: Option<SocketAddr>,

    /// Persistent keepalive interval in seconds (0 to disable)
    pub keepalive: u16,

    /// List of IP addresses allowed from this peer (routing table)
    pub allowed_ips: Vec<IpAddr>,
}

impl PeerConfig {
    /// Creates a new peer configuration with default settings
    pub fn new(public_key: [u8; 32], virtual_ip: VirtualIp) -> Self {
        PeerConfig {
            public_key,
            virtual_ip,
            endpoint: None,
            keepalive: 25, // Default 25 second keepalive
            allowed_ips: vec![virtual_ip.into()],
        }
    }

    /// Returns the public key as a hex string
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key)
    }
}

/// Peer metadata with statistics
///
/// Extends PeerConfig with runtime state including connection statistics
/// and status tracking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerMetadata {
    /// Base peer configuration
    pub config: PeerConfig,

    /// Timestamp of last successful WireGuard handshake
    pub last_handshake: Option<SystemTime>,

    /// Total bytes transmitted to this peer
    pub tx_bytes: u64,

    /// Total bytes received from this peer
    pub rx_bytes: u64,

    /// Current connection status
    pub status: PeerStatus,
}

impl PeerMetadata {
    /// Creates new peer metadata from configuration
    pub fn new(config: PeerConfig) -> Self {
        PeerMetadata {
            config,
            last_handshake: None,
            tx_bytes: 0,
            rx_bytes: 0,
            status: PeerStatus::Unknown,
        }
    }

    /// Checks if the peer is considered active (recent handshake)
    ///
    /// A peer is active if a handshake occurred within the last 180 seconds.
    pub fn is_active(&self) -> bool {
        if let Some(last) = self.last_handshake {
            if let Ok(duration) = SystemTime::now().duration_since(last) {
                return duration.as_secs() < 180;
            }
        }
        false
    }

    /// Updates connection statistics
    pub fn update_stats(&mut self, tx_bytes: u64, rx_bytes: u64) {
        self.tx_bytes = tx_bytes;
        self.rx_bytes = rx_bytes;
    }

    /// Updates the last handshake timestamp
    pub fn update_handshake(&mut self) {
        self.last_handshake = Some(SystemTime::now());
    }
}

impl Default for PeerMetadata {
    fn default() -> Self {
        PeerMetadata {
            config: PeerConfig {
                public_key: [0u8; 32],
                virtual_ip: VirtualIp::HUB,
                endpoint: None,
                keepalive: 25,
                allowed_ips: vec![],
            },
            last_handshake: None,
            tx_bytes: 0,
            rx_bytes: 0,
            status: PeerStatus::Unknown,
        }
    }
}

/// Network events
///
/// Events emitted by the network layer to notify about topology changes,
/// peer lifecycle, and connection status updates.
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A new peer has joined the network
    PeerJoined {
        public_key: [u8; 32],
        virtual_ip: VirtualIp,
    },

    /// A peer has left the network
    PeerLeft { public_key: [u8; 32] },

    /// A peer's endpoint has been updated
    EndpointUpdated {
        public_key: [u8; 32],
        endpoint: SocketAddr,
    },

    /// A peer was discovered on the local network via mDNS
    LanPeerDiscovered {
        public_key: [u8; 32],
        local_addr: SocketAddr,
    },

    /// A peer's connection mode has changed (e.g., relayed -> direct P2P)
    ConnectionModeChanged {
        public_key: [u8; 32],
        status: PeerStatus,
    },
}

impl NetworkEvent {
    /// Returns the public key associated with this event
    pub fn public_key(&self) -> &[u8; 32] {
        match self {
            NetworkEvent::PeerJoined { public_key, .. }
            | NetworkEvent::PeerLeft { public_key }
            | NetworkEvent::EndpointUpdated { public_key, .. }
            | NetworkEvent::LanPeerDiscovered { public_key, .. }
            | NetworkEvent::ConnectionModeChanged { public_key, .. } => public_key,
        }
    }
}

/// mDNS configuration
///
/// Settings for local network peer discovery using multicast DNS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MdnsConfig {
    /// Enable mDNS discovery
    pub enabled: bool,

    /// Service name for mDNS announcements
    pub service_name: String,

    /// How often to announce our presence (seconds)
    pub announce_interval_secs: u64,

    /// How often to scan for peers (seconds)
    pub scan_interval_secs: u64,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        MdnsConfig {
            enabled: true,
            service_name: "_portal._udp.local".to_string(),
            announce_interval_secs: 60,
            scan_interval_secs: 30,
        }
    }
}

/// Hub network configuration
///
/// Configuration for connecting to the Portal hub, which provides
/// peer coordination and acts as a relay for NAT traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubNetConfig {
    /// Hub's WireGuard public key
    pub public_key: [u8; 32],

    /// Hub's network endpoint
    pub endpoint: SocketAddr,

    /// Hub's virtual IP (always 10.0.0.1)
    pub virtual_ip: VirtualIp,

    /// Heartbeat interval for hub connection (seconds)
    pub heartbeat_secs: u64,
}

impl HubNetConfig {
    /// Creates a new hub configuration
    pub fn new(public_key: [u8; 32], endpoint: SocketAddr) -> Self {
        HubNetConfig {
            public_key,
            endpoint,
            virtual_ip: VirtualIp::HUB,
            heartbeat_secs: 30,
        }
    }
}

/// Network configuration
///
/// Complete configuration for the Portal network layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Local WireGuard listen port
    pub listen_port: u16,

    /// Our virtual IP address (assigned by hub if None)
    pub virtual_ip: Option<VirtualIp>,

    /// Hub configuration
    pub hub: HubNetConfig,

    /// mDNS discovery configuration
    pub mdns: MdnsConfig,
}

impl NetworkConfig {
    /// Creates a new network configuration
    pub fn new(listen_port: u16, hub: HubNetConfig) -> Self {
        NetworkConfig {
            listen_port,
            virtual_ip: None,
            hub,
            mdns: MdnsConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            listen_port: 51820,
            virtual_ip: None,
            hub: HubNetConfig {
                public_key: [0u8; 32],
                endpoint: "127.0.0.1:51820".parse().unwrap(),
                virtual_ip: VirtualIp::HUB,
                heartbeat_secs: 30,
            },
            mdns: MdnsConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_ip_new() {
        // Test basic creation
        let vip = VirtualIp::new(42);
        assert_eq!(vip.as_ipv4(), Ipv4Addr::new(10, 0, 0, 42));

        // Test with larger host ID
        let vip = VirtualIp::new(0x1234);
        assert_eq!(vip.as_ipv4(), Ipv4Addr::new(10, 0, 0x12, 0x34));

        // Test edge cases
        let vip = VirtualIp::new(0);
        assert_eq!(vip.as_ipv4(), Ipv4Addr::new(10, 0, 0, 0));

        let vip = VirtualIp::new(0xFFFF);
        assert_eq!(vip.as_ipv4(), Ipv4Addr::new(10, 0, 255, 255));
    }

    #[test]
    fn test_virtual_ip_host() {
        // Test extraction
        let vip = VirtualIp::new(42);
        assert_eq!(vip.host(), 42);

        let vip = VirtualIp::new(0x1234);
        assert_eq!(vip.host(), 0x1234);

        // Test roundtrip
        for host in [0, 1, 100, 256, 1000, 0xFFFF] {
            let vip = VirtualIp::new(host);
            assert_eq!(vip.host(), host);
        }
    }

    #[test]
    fn test_virtual_ip_hub_constant() {
        assert_eq!(VirtualIp::HUB.as_ipv4(), Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(VirtualIp::HUB.host(), 1);
    }

    #[test]
    fn test_virtual_ip_subnet_check() {
        // Valid Portal subnet IPs
        assert!(VirtualIp::new(1).is_portal_subnet());
        assert!(VirtualIp::new(100).is_portal_subnet());
        assert!(VirtualIp::new(0xFFFF).is_portal_subnet());
        assert!(VirtualIp::HUB.is_portal_subnet());

        // Test from_ipv4
        assert!(VirtualIp::from_ipv4(Ipv4Addr::new(10, 0, 1, 1)).is_ok());
        assert!(VirtualIp::from_ipv4(Ipv4Addr::new(10, 0, 255, 255)).is_ok());

        // Invalid IPs (not in 10.0.0.0/16)
        assert!(VirtualIp::from_ipv4(Ipv4Addr::new(10, 1, 0, 1)).is_err());
        assert!(VirtualIp::from_ipv4(Ipv4Addr::new(192, 168, 1, 1)).is_err());
        assert!(VirtualIp::from_ipv4(Ipv4Addr::new(172, 16, 0, 1)).is_err());
    }

    #[test]
    fn test_virtual_ip_serialize() {
        let vip = VirtualIp::new(0x1234);

        // JSON serialization
        let json = serde_json::to_string(&vip).unwrap();
        let deserialized: VirtualIp = serde_json::from_str(&json).unwrap();
        assert_eq!(vip, deserialized);

        // MessagePack serialization
        let msgpack = rmp_serde::to_vec(&vip).unwrap();
        let deserialized: VirtualIp = rmp_serde::from_slice(&msgpack).unwrap();
        assert_eq!(vip, deserialized);

        // Test with HUB constant
        let json = serde_json::to_string(&VirtualIp::HUB).unwrap();
        let deserialized: VirtualIp = serde_json::from_str(&json).unwrap();
        assert_eq!(VirtualIp::HUB, deserialized);
    }

    #[test]
    fn test_peer_status_serialize() {
        let statuses = vec![
            PeerStatus::Unknown,
            PeerStatus::Online,
            PeerStatus::Offline,
            PeerStatus::DirectP2P,
            PeerStatus::Relayed,
        ];

        for status in statuses {
            // JSON roundtrip
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: PeerStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);

            // MessagePack roundtrip
            let msgpack = rmp_serde::to_vec(&status).unwrap();
            let deserialized: PeerStatus = rmp_serde::from_slice(&msgpack).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_peer_config_serialize() {
        let public_key = [42u8; 32];
        let vip = VirtualIp::new(100);
        let endpoint = "192.168.1.100:51820".parse().unwrap();

        let config = PeerConfig {
            public_key,
            virtual_ip: vip,
            endpoint: Some(endpoint),
            keepalive: 25,
            allowed_ips: vec![vip.into()],
        };

        // JSON roundtrip
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: PeerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.public_key, deserialized.public_key);
        assert_eq!(config.virtual_ip, deserialized.virtual_ip);
        assert_eq!(config.endpoint, deserialized.endpoint);
        assert_eq!(config.keepalive, deserialized.keepalive);
        assert_eq!(config.allowed_ips.len(), deserialized.allowed_ips.len());

        // MessagePack roundtrip
        let msgpack = rmp_serde::to_vec(&config).unwrap();
        let deserialized: PeerConfig = rmp_serde::from_slice(&msgpack).unwrap();
        assert_eq!(config.public_key, deserialized.public_key);
        assert_eq!(config.virtual_ip, deserialized.virtual_ip);
    }

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();

        assert_eq!(config.listen_port, 51820);
        assert!(config.virtual_ip.is_none());
        assert_eq!(config.hub.virtual_ip, VirtualIp::HUB);
        assert_eq!(config.hub.heartbeat_secs, 30);
        assert!(config.mdns.enabled);
        assert_eq!(config.mdns.service_name, "_portal._udp.local");
        assert_eq!(config.mdns.announce_interval_secs, 60);
        assert_eq!(config.mdns.scan_interval_secs, 30);
    }

    #[test]
    fn test_virtual_ip_display() {
        let vip = VirtualIp::new(0x1234);
        assert_eq!(format!("{}", vip), "10.0.18.52");

        assert_eq!(format!("{}", VirtualIp::HUB), "10.0.0.1");

        let vip = VirtualIp::new(100);
        assert_eq!(format!("{}", vip), "10.0.0.100");
    }

    #[test]
    fn test_peer_metadata_default() {
        let metadata = PeerMetadata::default();

        assert_eq!(metadata.config.public_key, [0u8; 32]);
        assert_eq!(metadata.config.virtual_ip, VirtualIp::HUB);
        assert!(metadata.last_handshake.is_none());
        assert_eq!(metadata.tx_bytes, 0);
        assert_eq!(metadata.rx_bytes, 0);
        assert_eq!(metadata.status, PeerStatus::Unknown);
    }

    #[test]
    fn test_peer_config_new() {
        let public_key = [1u8; 32];
        let vip = VirtualIp::new(50);

        let config = PeerConfig::new(public_key, vip);

        assert_eq!(config.public_key, public_key);
        assert_eq!(config.virtual_ip, vip);
        assert!(config.endpoint.is_none());
        assert_eq!(config.keepalive, 25);
        assert_eq!(config.allowed_ips.len(), 1);
        assert_eq!(config.allowed_ips[0], IpAddr::from(vip));
    }

    #[test]
    fn test_peer_config_public_key_hex() {
        let public_key = [0xAB; 32];
        let config = PeerConfig::new(public_key, VirtualIp::new(1));

        let hex = config.public_key_hex();
        assert_eq!(hex.len(), 64); // 32 bytes = 64 hex chars
        assert!(hex.chars().all(|c| "0123456789abcdef".contains(c)));
    }

    #[test]
    fn test_peer_metadata_new() {
        let public_key = [2u8; 32];
        let vip = VirtualIp::new(75);
        let config = PeerConfig::new(public_key, vip);

        let metadata = PeerMetadata::new(config.clone());

        assert_eq!(metadata.config.public_key, config.public_key);
        assert_eq!(metadata.config.virtual_ip, config.virtual_ip);
        assert!(metadata.last_handshake.is_none());
        assert_eq!(metadata.tx_bytes, 0);
        assert_eq!(metadata.rx_bytes, 0);
        assert_eq!(metadata.status, PeerStatus::Unknown);
    }

    #[test]
    fn test_peer_metadata_is_active() {
        let config = PeerConfig::new([3u8; 32], VirtualIp::new(10));
        let mut metadata = PeerMetadata::new(config);

        // Initially not active
        assert!(!metadata.is_active());

        // Update handshake
        metadata.update_handshake();
        assert!(metadata.is_active());

        // Test with old timestamp (simulate 200 seconds ago)
        use std::time::Duration;
        if let Some(old_time) = SystemTime::now().checked_sub(Duration::from_secs(200)) {
            metadata.last_handshake = Some(old_time);
            assert!(!metadata.is_active());
        }
    }

    #[test]
    fn test_peer_metadata_update_stats() {
        let config = PeerConfig::new([4u8; 32], VirtualIp::new(20));
        let mut metadata = PeerMetadata::new(config);

        metadata.update_stats(1024, 2048);
        assert_eq!(metadata.tx_bytes, 1024);
        assert_eq!(metadata.rx_bytes, 2048);

        metadata.update_stats(4096, 8192);
        assert_eq!(metadata.tx_bytes, 4096);
        assert_eq!(metadata.rx_bytes, 8192);
    }

    #[test]
    fn test_peer_status_default() {
        assert_eq!(PeerStatus::default(), PeerStatus::Unknown);
    }

    #[test]
    fn test_peer_status_display() {
        assert_eq!(format!("{}", PeerStatus::Unknown), "unknown");
        assert_eq!(format!("{}", PeerStatus::Online), "online");
        assert_eq!(format!("{}", PeerStatus::Offline), "offline");
        assert_eq!(format!("{}", PeerStatus::DirectP2P), "direct-p2p");
        assert_eq!(format!("{}", PeerStatus::Relayed), "relayed");
    }

    #[test]
    fn test_network_event_public_key() {
        let pk = [5u8; 32];
        let vip = VirtualIp::new(30);

        let event1 = NetworkEvent::PeerJoined {
            public_key: pk,
            virtual_ip: vip,
        };
        assert_eq!(event1.public_key(), &pk);

        let event2 = NetworkEvent::PeerLeft { public_key: pk };
        assert_eq!(event2.public_key(), &pk);

        let event3 = NetworkEvent::EndpointUpdated {
            public_key: pk,
            endpoint: "192.168.1.1:51820".parse().unwrap(),
        };
        assert_eq!(event3.public_key(), &pk);

        let event4 = NetworkEvent::LanPeerDiscovered {
            public_key: pk,
            local_addr: "192.168.1.2:51820".parse().unwrap(),
        };
        assert_eq!(event4.public_key(), &pk);

        let event5 = NetworkEvent::ConnectionModeChanged {
            public_key: pk,
            status: PeerStatus::DirectP2P,
        };
        assert_eq!(event5.public_key(), &pk);
    }

    #[test]
    fn test_mdns_config_default() {
        let config = MdnsConfig::default();

        assert!(config.enabled);
        assert_eq!(config.service_name, "_portal._udp.local");
        assert_eq!(config.announce_interval_secs, 60);
        assert_eq!(config.scan_interval_secs, 30);
    }

    #[test]
    fn test_hub_net_config_new() {
        let public_key = [6u8; 32];
        let endpoint = "192.168.1.100:51820".parse().unwrap();

        let config = HubNetConfig::new(public_key, endpoint);

        assert_eq!(config.public_key, public_key);
        assert_eq!(config.endpoint, endpoint);
        assert_eq!(config.virtual_ip, VirtualIp::HUB);
        assert_eq!(config.heartbeat_secs, 30);
    }

    #[test]
    fn test_network_config_new() {
        let public_key = [7u8; 32];
        let endpoint = "192.168.1.200:51820".parse().unwrap();
        let hub = HubNetConfig::new(public_key, endpoint);

        let config = NetworkConfig::new(12345, hub.clone());

        assert_eq!(config.listen_port, 12345);
        assert!(config.virtual_ip.is_none());
        assert_eq!(config.hub.public_key, hub.public_key);
        assert_eq!(config.hub.endpoint, hub.endpoint);
        assert!(config.mdns.enabled);
    }

    #[test]
    fn test_virtual_ip_conversions() {
        let vip = VirtualIp::new(0x0A0B);

        // Test Into<Ipv4Addr>
        let ipv4: Ipv4Addr = vip.into();
        assert_eq!(ipv4, Ipv4Addr::new(10, 0, 0x0A, 0x0B));

        // Test Into<IpAddr>
        let ip: IpAddr = vip.into();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0x0A, 0x0B)));
    }

    #[test]
    fn test_network_config_serialization() {
        let config = NetworkConfig::default();

        // JSON roundtrip
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: NetworkConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.listen_port, deserialized.listen_port);
        assert_eq!(config.hub.heartbeat_secs, deserialized.hub.heartbeat_secs);

        // MessagePack roundtrip
        let msgpack = rmp_serde::to_vec(&config).unwrap();
        let deserialized: NetworkConfig = rmp_serde::from_slice(&msgpack).unwrap();
        assert_eq!(config.listen_port, deserialized.listen_port);
    }

    #[test]
    fn test_hub_net_config_serialization() {
        let public_key = [8u8; 32];
        let endpoint = "192.168.1.1:51820".parse().unwrap();
        let config = HubNetConfig::new(public_key, endpoint);

        // JSON roundtrip
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: HubNetConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.public_key, deserialized.public_key);
        assert_eq!(config.endpoint, deserialized.endpoint);
        assert_eq!(config.virtual_ip, deserialized.virtual_ip);

        // MessagePack roundtrip
        let msgpack = rmp_serde::to_vec(&config).unwrap();
        let deserialized: HubNetConfig = rmp_serde::from_slice(&msgpack).unwrap();
        assert_eq!(config.public_key, deserialized.public_key);
    }

    #[test]
    fn test_mdns_config_serialization() {
        let config = MdnsConfig::default();

        // JSON roundtrip
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MdnsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.enabled, deserialized.enabled);
        assert_eq!(config.service_name, deserialized.service_name);

        // MessagePack roundtrip
        let msgpack = rmp_serde::to_vec(&config).unwrap();
        let deserialized: MdnsConfig = rmp_serde::from_slice(&msgpack).unwrap();
        assert_eq!(config.enabled, deserialized.enabled);
    }

    #[test]
    fn test_virtual_ip_edge_cases() {
        // Test minimum and maximum host values
        let min_vip = VirtualIp::new(u16::MIN);
        assert_eq!(min_vip.host(), 0);
        assert_eq!(min_vip.as_ipv4(), Ipv4Addr::new(10, 0, 0, 0));

        let max_vip = VirtualIp::new(u16::MAX);
        assert_eq!(max_vip.host(), 65535);
        assert_eq!(max_vip.as_ipv4(), Ipv4Addr::new(10, 0, 255, 255));
    }

    #[test]
    fn test_peer_config_with_multiple_allowed_ips() {
        let public_key = [9u8; 32];
        let vip = VirtualIp::new(40);
        let mut config = PeerConfig::new(public_key, vip);

        // Add additional allowed IPs
        config
            .allowed_ips
            .push(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0)));
        config
            .allowed_ips
            .push(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 0)));

        assert_eq!(config.allowed_ips.len(), 3);

        // Test serialization with multiple IPs
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: PeerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.allowed_ips.len(), 3);
    }
}
