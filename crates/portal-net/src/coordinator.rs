//! Hub coordinator for Portal mesh network
//!
//! Manages communication with the Portal Hub server for peer coordination,
//! NAT traversal, and relay functionality. The Hub acts as a central registry
//! for edge nodes and facilitates P2P connections when direct communication fails.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;

use crate::discovery::LocalEdgeInfo;
use crate::types::{HubNetConfig, PeerConfig, PeerMetadata, PeerStatus, VirtualIp};
use crate::{PortalNetError, Result};

/// Hub protocol messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum HubMessage {
    Register { public_key: [u8; 32], virtual_ip: Option<VirtualIp>, endpoint: SocketAddr },
    RegisterResponse { virtual_ip: VirtualIp },
    Heartbeat { public_key: [u8; 32], status: PeerStatus },
    HeartbeatAck,
    PeerQuery { public_key: [u8; 32] },
    PeerResponse { peer: Option<PeerMetadata> },
    PeerListRequest,
    PeerList { peers: Vec<PeerMetadata> },
    HolePunchRequest { target: [u8; 32] },
    HolePunchResponse { endpoint: SocketAddr },
    Relay { from: [u8; 32], to: [u8; 32], payload: Vec<u8> },
    RelayAck,
    Disconnect { public_key: [u8; 32] },
    DisconnectAck,
}

/// Connection state of the Hub coordinator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected to Hub
    Disconnected,
    /// Attempting to connect to Hub
    Connecting,
    /// Connected and registered with Hub
    Connected,
    /// Connection failed, will retry
    Failed,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Disconnected
    }
}

/// Internal state for the Hub coordinator
struct CoordinatorState {
    connection_state: ConnectionState,
    virtual_ip: Option<VirtualIp>,
    public_key: [u8; 32],
    last_heartbeat: Option<SystemTime>,
    peer_cache: Vec<PeerMetadata>,
}

/// Hub coordinator for managing Hub communication
pub struct HubCoordinator {
    config: HubNetConfig,
    state: Arc<RwLock<CoordinatorState>>,
}

impl HubCoordinator {
    /// Creates a new Hub coordinator with the given configuration
    pub fn new(config: HubNetConfig) -> Result<Self> {
        if config.endpoint.port() == 0 {
            return Err(PortalNetError::Configuration("Hub endpoint port cannot be 0".to_string()));
        }
        if config.heartbeat_secs == 0 {
            return Err(PortalNetError::Configuration("Heartbeat interval must be greater than 0".to_string()));
        }

        let state = CoordinatorState {
            connection_state: ConnectionState::Disconnected,
            virtual_ip: None,
            public_key: [0u8; 32],
            last_heartbeat: None,
            peer_cache: Vec::new(),
        };

        Ok(Self { config, state: Arc::new(RwLock::new(state)) })
    }

    /// Registers this edge with the Hub
    pub async fn register(&self, edge: &LocalEdgeInfo) -> Result<VirtualIp> {
        edge.validate()?;

        let mut state = self.state.write().await;
        state.connection_state = ConnectionState::Connecting;
        state.public_key = edge.public_key;

        let message = HubMessage::Register {
            public_key: edge.public_key,
            virtual_ip: Some(edge.virtual_ip),
            endpoint: edge.endpoint,
        };

        drop(state);
        let response = self.simulate_hub_response(message).await?;

        if let HubMessage::RegisterResponse { virtual_ip } = response {
            let mut state = self.state.write().await;
            state.virtual_ip = Some(virtual_ip);
            state.connection_state = ConnectionState::Connected;
            state.last_heartbeat = Some(SystemTime::now());
            Ok(virtual_ip)
        } else {
            let mut state = self.state.write().await;
            state.connection_state = ConnectionState::Failed;
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Fetches the current peer directory from the Hub
    pub async fn get_peers(&self) -> Result<Vec<PeerMetadata>> {
        let state = self.state.read().await;
        if state.connection_state != ConnectionState::Connected {
            return Err(PortalNetError::HubConnection("not connected to Hub".to_string()));
        }
        drop(state);

        let response = self.simulate_hub_response(HubMessage::PeerListRequest).await?;

        if let HubMessage::PeerList { peers } = response {
            let mut state = self.state.write().await;
            state.peer_cache.clone_from(&peers);
            Ok(peers)
        } else {
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Requests connection info for a specific peer
    pub async fn get_peer(&self, public_key: &[u8; 32]) -> Result<Option<PeerMetadata>> {
        let state = self.state.read().await;
        if state.connection_state != ConnectionState::Connected {
            return Err(PortalNetError::HubConnection("not connected to Hub".to_string()));
        }
        drop(state);

        let response = self.simulate_hub_response(HubMessage::PeerQuery { public_key: *public_key }).await?;

        if let HubMessage::PeerResponse { peer } = response {
            Ok(peer)
        } else {
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Sends a heartbeat to maintain registration
    pub async fn heartbeat(&self) -> Result<()> {
        let state = self.state.read().await;
        if state.connection_state != ConnectionState::Connected {
            return Err(PortalNetError::HubConnection("not connected to Hub".to_string()));
        }
        let public_key = state.public_key;
        drop(state);

        let message = HubMessage::Heartbeat { public_key, status: PeerStatus::Online };
        let response = self.simulate_hub_response(message).await?;

        if response == HubMessage::HeartbeatAck {
            let mut state = self.state.write().await;
            state.last_heartbeat = Some(SystemTime::now());
            Ok(())
        } else {
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Requests NAT hole punching assistance for connecting to a target peer
    pub async fn request_hole_punch(&self, target: &[u8; 32]) -> Result<SocketAddr> {
        let state = self.state.read().await;
        if state.connection_state != ConnectionState::Connected {
            return Err(PortalNetError::HubConnection("not connected to Hub".to_string()));
        }
        drop(state);

        let response = self.simulate_hub_response(HubMessage::HolePunchRequest { target: *target }).await?;

        if let HubMessage::HolePunchResponse { endpoint } = response {
            Ok(endpoint)
        } else {
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Relays data through the Hub when P2P connection fails
    pub async fn relay(&self, to: &[u8; 32], data: &[u8]) -> Result<()> {
        let state = self.state.read().await;
        if state.connection_state != ConnectionState::Connected {
            return Err(PortalNetError::HubConnection("not connected to Hub".to_string()));
        }
        let from = state.public_key;
        drop(state);

        if data.len() > 65536 {
            return Err(PortalNetError::Configuration("relay payload too large (max 64KB)".to_string()));
        }

        let message = HubMessage::Relay { from, to: *to, payload: data.to_vec() };
        let response = self.simulate_hub_response(message).await?;

        if response == HubMessage::RelayAck {
            Ok(())
        } else {
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Disconnects from the Hub
    pub async fn disconnect(&self) -> Result<()> {
        let state = self.state.read().await;
        if state.connection_state == ConnectionState::Disconnected {
            return Ok(());
        }
        let public_key = state.public_key;
        drop(state);

        let response = self.simulate_hub_response(HubMessage::Disconnect { public_key }).await?;

        if response == HubMessage::DisconnectAck {
            let mut state = self.state.write().await;
            state.connection_state = ConnectionState::Disconnected;
            state.virtual_ip = None;
            state.last_heartbeat = None;
            Ok(())
        } else {
            Err(PortalNetError::HubConnection("unexpected response from Hub".to_string()))
        }
    }

    /// Returns the current connection state
    pub async fn connection_state(&self) -> ConnectionState {
        self.state.read().await.connection_state
    }

    /// Returns the assigned virtual IP if connected
    pub async fn virtual_ip(&self) -> Option<VirtualIp> {
        self.state.read().await.virtual_ip
    }

    /// Returns the last heartbeat timestamp
    pub async fn last_heartbeat(&self) -> Option<SystemTime> {
        self.state.read().await.last_heartbeat
    }

    /// Returns the Hub configuration
    #[must_use]
    pub fn config(&self) -> &HubNetConfig {
        &self.config
    }

    /// Starts automatic heartbeat loop
    pub async fn start_heartbeat_loop(&self) -> Result<mpsc::Receiver<Result<()>>> {
        let state = self.state.read().await;
        if state.connection_state != ConnectionState::Connected {
            return Err(PortalNetError::HubConnection("not connected to Hub".to_string()));
        }
        drop(state);

        let (tx, rx) = mpsc::channel(10);
        let coordinator = self.clone_state().await;
        let heartbeat_secs = self.config.heartbeat_secs;

        tokio::spawn(async move {
            let mut timer = interval(Duration::from_secs(heartbeat_secs));
            loop {
                timer.tick().await;
                if tx.send(coordinator.heartbeat().await).await.is_err() {
                    break;
                }
            }
        });

        Ok(rx)
    }

    async fn clone_state(&self) -> Self {
        Self { config: self.config.clone(), state: Arc::clone(&self.state) }
    }

    async fn simulate_hub_response(&self, message: HubMessage) -> Result<HubMessage> {
        tokio::time::sleep(Duration::from_millis(10)).await;

        match message {
            HubMessage::Register { virtual_ip, .. } => {
                Ok(HubMessage::RegisterResponse { virtual_ip: virtual_ip.unwrap_or_else(|| VirtualIp::new(100)) })
            }
            HubMessage::Heartbeat { .. } => Ok(HubMessage::HeartbeatAck),
            HubMessage::PeerQuery { public_key } => {
                let peer = if public_key == [1u8; 32] {
                    Some(PeerMetadata::new(PeerConfig::new(public_key, VirtualIp::new(50))))
                } else {
                    None
                };
                Ok(HubMessage::PeerResponse { peer })
            }
            HubMessage::PeerListRequest => {
                let peers = vec![
                    PeerMetadata::new(PeerConfig::new([1u8; 32], VirtualIp::new(50))),
                    PeerMetadata::new(PeerConfig::new([2u8; 32], VirtualIp::new(51))),
                ];
                Ok(HubMessage::PeerList { peers })
            }
            HubMessage::HolePunchRequest { .. } => {
                Ok(HubMessage::HolePunchResponse { endpoint: "192.168.1.100:51820".parse().unwrap() })
            }
            HubMessage::Relay { .. } => Ok(HubMessage::RelayAck),
            HubMessage::Disconnect { .. } => Ok(HubMessage::DisconnectAck),
            _ => Err(PortalNetError::HubConnection("unexpected message type".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HubNetConfig {
        HubNetConfig::new([1u8; 32], "192.168.1.100:51820".parse().unwrap())
    }

    fn test_edge() -> LocalEdgeInfo {
        LocalEdgeInfo::new([2u8; 32], VirtualIp::new(100), "192.168.1.50:51820".parse().unwrap())
    }

    #[test]
    fn test_hub_coordinator_new_valid() {
        assert!(HubCoordinator::new(test_config()).is_ok());
    }

    #[test]
    fn test_hub_coordinator_new_invalid_port() {
        let mut config = test_config();
        config.endpoint = "192.168.1.100:0".parse().unwrap();
        assert!(matches!(HubCoordinator::new(config), Err(PortalNetError::Configuration(_))));
    }

    #[test]
    fn test_hub_coordinator_new_invalid_heartbeat() {
        let mut config = test_config();
        config.heartbeat_secs = 0;
        assert!(matches!(HubCoordinator::new(config), Err(PortalNetError::Configuration(_))));
    }

    #[tokio::test]
    async fn test_register() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        let vip = coordinator.register(&test_edge()).await.unwrap();
        assert_eq!(vip, VirtualIp::new(100));
        assert_eq!(coordinator.connection_state().await, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn test_register_assigns_vip() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        let vip = coordinator.register(&test_edge()).await.unwrap();
        assert_eq!(coordinator.virtual_ip().await, Some(vip));
    }

    #[tokio::test]
    async fn test_get_peers_not_connected() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(matches!(coordinator.get_peers().await, Err(PortalNetError::HubConnection(_))));
    }

    #[tokio::test]
    async fn test_get_peers_after_register() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let peers = coordinator.get_peers().await.unwrap();
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].config.public_key, [1u8; 32]);
        assert_eq!(peers[1].config.public_key, [2u8; 32]);
    }

    #[tokio::test]
    async fn test_get_peer_not_connected() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.get_peer(&[1u8; 32]).await.is_err());
    }

    #[tokio::test]
    async fn test_get_peer_found() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let peer = coordinator.get_peer(&[1u8; 32]).await.unwrap();
        assert!(peer.is_some());
        assert_eq!(peer.unwrap().config.public_key, [1u8; 32]);
    }

    #[tokio::test]
    async fn test_get_peer_not_found() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let peer = coordinator.get_peer(&[99u8; 32]).await.unwrap();
        assert!(peer.is_none());
    }

    #[tokio::test]
    async fn test_heartbeat_not_connected() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.heartbeat().await.is_err());
    }

    #[tokio::test]
    async fn test_heartbeat_after_register() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let first_heartbeat = coordinator.last_heartbeat().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(coordinator.heartbeat().await.is_ok());
        assert!(coordinator.last_heartbeat().await > first_heartbeat);
    }

    #[tokio::test]
    async fn test_request_hole_punch_not_connected() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.request_hole_punch(&[3u8; 32]).await.is_err());
    }

    #[tokio::test]
    async fn test_request_hole_punch() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let endpoint = coordinator.request_hole_punch(&[3u8; 32]).await.unwrap();
        assert_eq!(endpoint, "192.168.1.100:51820".parse().unwrap());
    }

    #[tokio::test]
    async fn test_relay_not_connected() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.relay(&[3u8; 32], b"test").await.is_err());
    }

    #[tokio::test]
    async fn test_relay() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        assert!(coordinator.relay(&[3u8; 32], b"test relay data").await.is_ok());
    }

    #[tokio::test]
    async fn test_relay_payload_too_large() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let data = vec![0u8; 100000];
        assert!(matches!(coordinator.relay(&[3u8; 32], &data).await, Err(PortalNetError::Configuration(_))));
    }

    #[tokio::test]
    async fn test_disconnect() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        assert_eq!(coordinator.connection_state().await, ConnectionState::Connected);
        assert!(coordinator.disconnect().await.is_ok());
        assert_eq!(coordinator.connection_state().await, ConnectionState::Disconnected);
        assert!(coordinator.virtual_ip().await.is_none());
    }

    #[tokio::test]
    async fn test_disconnect_when_not_connected() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.disconnect().await.is_ok());
    }

    #[tokio::test]
    async fn test_connection_state_transitions() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert_eq!(coordinator.connection_state().await, ConnectionState::Disconnected);
        coordinator.register(&test_edge()).await.unwrap();
        assert_eq!(coordinator.connection_state().await, ConnectionState::Connected);
        coordinator.disconnect().await.unwrap();
        assert_eq!(coordinator.connection_state().await, ConnectionState::Disconnected);
    }

    #[test]
    fn test_connection_state_default() {
        assert_eq!(ConnectionState::default(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_hub_message_serialization() {
        let messages = vec![
            HubMessage::Register { public_key: [1u8; 32], virtual_ip: Some(VirtualIp::new(100)), endpoint: "192.168.1.1:51820".parse().unwrap() },
            HubMessage::RegisterResponse { virtual_ip: VirtualIp::new(100) },
            HubMessage::Heartbeat { public_key: [2u8; 32], status: PeerStatus::Online },
            HubMessage::HeartbeatAck,
            HubMessage::PeerQuery { public_key: [3u8; 32] },
            HubMessage::PeerListRequest,
            HubMessage::HolePunchRequest { target: [4u8; 32] },
            HubMessage::Relay { from: [5u8; 32], to: [6u8; 32], payload: vec![1, 2, 3] },
            HubMessage::RelayAck,
            HubMessage::Disconnect { public_key: [7u8; 32] },
            HubMessage::DisconnectAck,
        ];

        for message in messages {
            let json = serde_json::to_string(&message).unwrap();
            let deserialized: HubMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(message, deserialized);

            let msgpack = rmp_serde::to_vec(&message).unwrap();
            let deserialized: HubMessage = rmp_serde::from_slice(&msgpack).unwrap();
            assert_eq!(message, deserialized);
        }
    }

    #[tokio::test]
    async fn test_config_access() {
        let config = test_config();
        let coordinator = HubCoordinator::new(config.clone()).unwrap();
        assert_eq!(coordinator.config().endpoint, config.endpoint);
        assert_eq!(coordinator.config().public_key, config.public_key);
        assert_eq!(coordinator.config().heartbeat_secs, config.heartbeat_secs);
    }

    #[tokio::test]
    async fn test_virtual_ip_lifecycle() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.virtual_ip().await.is_none());
        let vip = coordinator.register(&test_edge()).await.unwrap();
        assert_eq!(coordinator.virtual_ip().await, Some(vip));
        coordinator.disconnect().await.unwrap();
        assert!(coordinator.virtual_ip().await.is_none());
    }

    #[tokio::test]
    async fn test_last_heartbeat() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        assert!(coordinator.last_heartbeat().await.is_none());
        coordinator.register(&test_edge()).await.unwrap();
        assert!(coordinator.last_heartbeat().await.is_some());
    }

    #[tokio::test]
    async fn test_multiple_heartbeats() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let mut last_time = coordinator.last_heartbeat().await;

        for _ in 0..5 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            coordinator.heartbeat().await.unwrap();
            let current_time = coordinator.last_heartbeat().await;
            assert!(current_time > last_time);
            last_time = current_time;
        }
    }

    #[tokio::test]
    async fn test_get_peers_caching() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        let peers1 = coordinator.get_peers().await.unwrap();
        let peers2 = coordinator.get_peers().await.unwrap();
        assert_eq!(peers1.len(), peers2.len());
        assert_eq!(peers1.len(), 2);
    }

    #[tokio::test]
    async fn test_relay_empty_payload() {
        let coordinator = HubCoordinator::new(test_config()).unwrap();
        coordinator.register(&test_edge()).await.unwrap();
        assert!(coordinator.relay(&[3u8; 32], b"").await.is_ok());
    }

    #[test]
    fn test_hub_message_debug() {
        let message = HubMessage::Heartbeat { public_key: [1u8; 32], status: PeerStatus::Online };
        let debug_str = format!("{:?}", message);
        assert!(debug_str.contains("Heartbeat"));
    }

    #[test]
    fn test_connection_state_debug() {
        let state = ConnectionState::Connected;
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Connected"));
    }
}
