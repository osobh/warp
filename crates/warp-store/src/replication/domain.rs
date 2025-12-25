//! Domain registry for distributed storage
//!
//! Manages failure domains (datacenters, regions) and their nodes.
//! Provides health monitoring and failover capabilities.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

/// Domain identifier (unique across the cluster)
pub type DomainId = u64;

/// Node identifier (unique within a domain)
pub type NodeId = u64;

/// Represents a failure domain (datacenter, availability zone, region)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    /// Unique domain identifier
    pub id: DomainId,

    /// Human-readable domain name
    pub name: String,

    /// Geographic region (e.g., "us-west-1", "eu-central-1")
    pub region: Option<String>,

    /// Nodes in this domain
    pub nodes: Vec<NodeInfo>,

    /// WireGuard public key for this domain's gateway
    pub wg_pubkey: Option<[u8; 32]>,

    /// WireGuard endpoint for cross-domain communication
    pub wg_endpoint: Option<SocketAddr>,

    /// Current health status
    pub health: DomainHealth,

    /// When this domain was last seen healthy (not serialized)
    #[serde(skip, default)]
    pub last_healthy: Option<Instant>,
}

impl Domain {
    /// Create a new domain
    pub fn new(id: DomainId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            region: None,
            nodes: Vec::new(),
            wg_pubkey: None,
            wg_endpoint: None,
            health: DomainHealth::Unknown,
            last_healthy: None,
        }
    }

    /// Set the region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set WireGuard configuration
    pub fn with_wireguard(mut self, pubkey: [u8; 32], endpoint: SocketAddr) -> Self {
        self.wg_pubkey = Some(pubkey);
        self.wg_endpoint = Some(endpoint);
        self
    }

    /// Add a node to this domain
    pub fn add_node(&mut self, node: NodeInfo) {
        self.nodes.push(node);
    }

    /// Get healthy nodes in this domain
    pub fn healthy_nodes(&self) -> Vec<&NodeInfo> {
        self.nodes
            .iter()
            .filter(|n| matches!(n.status, NodeStatus::Online))
            .collect()
    }

    /// Check if domain has any healthy nodes
    pub fn is_available(&self) -> bool {
        !self.healthy_nodes().is_empty()
    }

    /// Get total storage capacity across all nodes
    pub fn total_capacity(&self) -> u64 {
        self.nodes.iter().map(|n| n.capacity.total_bytes).sum()
    }

    /// Get available storage capacity across healthy nodes
    pub fn available_capacity(&self) -> u64 {
        self.healthy_nodes()
            .iter()
            .map(|n| n.capacity.available_bytes)
            .sum()
    }
}

/// Information about a storage node within a domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier
    pub node_id: NodeId,

    /// Domain this node belongs to
    pub domain_id: DomainId,

    /// Network address for data transfer
    pub addr: SocketAddr,

    /// WireGuard public key (for authentication)
    pub wg_pubkey: [u8; 32],

    /// Storage capacity information
    pub capacity: StorageCapacity,

    /// Current node status
    pub status: NodeStatus,

    /// Last heartbeat time (not serialized)
    #[serde(skip, default)]
    pub last_heartbeat: Option<Instant>,

    /// Round-trip latency to this node (if measured)
    pub latency_ms: Option<u32>,
}

impl NodeInfo {
    /// Create a new node
    pub fn new(node_id: NodeId, domain_id: DomainId, addr: SocketAddr, wg_pubkey: [u8; 32]) -> Self {
        Self {
            node_id,
            domain_id,
            addr,
            wg_pubkey,
            capacity: StorageCapacity::default(),
            status: NodeStatus::Unknown,
            last_heartbeat: None,
            latency_ms: None,
        }
    }

    /// Check if node is healthy and available
    pub fn is_available(&self) -> bool {
        matches!(self.status, NodeStatus::Online)
    }

    /// Update heartbeat time
    pub fn record_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
        self.status = NodeStatus::Online;
    }

    /// Check if node's heartbeat is stale
    pub fn is_heartbeat_stale(&self, timeout: Duration) -> bool {
        self.last_heartbeat
            .map(|t| t.elapsed() > timeout)
            .unwrap_or(true)
    }
}

/// Storage capacity information
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageCapacity {
    /// Total storage capacity in bytes
    pub total_bytes: u64,

    /// Available (free) capacity in bytes
    pub available_bytes: u64,

    /// Used capacity in bytes
    pub used_bytes: u64,
}

impl StorageCapacity {
    /// Create with specific values
    pub fn new(total: u64, used: u64) -> Self {
        Self {
            total_bytes: total,
            available_bytes: total.saturating_sub(used),
            used_bytes: used,
        }
    }

    /// Usage percentage (0.0 to 1.0)
    pub fn usage_ratio(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f64 / self.total_bytes as f64
        }
    }

    /// Check if storage is nearly full (>90% used)
    pub fn is_nearly_full(&self) -> bool {
        self.usage_ratio() > 0.9
    }
}

/// Node status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Node status is unknown
    Unknown,
    /// Node is online and healthy
    Online,
    /// Node is offline or unreachable
    Offline,
    /// Node is degraded (slow or partial failures)
    Degraded,
    /// Node is being decommissioned
    Draining,
}

/// Domain health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainHealth {
    /// Health status is unknown
    Unknown,
    /// Domain is fully healthy
    Healthy,
    /// Domain is degraded (some nodes down)
    Degraded,
    /// Domain is unhealthy (most nodes down)
    Unhealthy,
    /// Domain is completely offline
    Offline,
}

/// Events broadcast by the domain registry
#[derive(Debug, Clone)]
pub enum DomainEvent {
    /// A new domain was added
    DomainAdded(DomainId),
    /// A domain was removed
    DomainRemoved(DomainId),
    /// Domain health changed
    DomainHealthChanged {
        domain_id: DomainId,
        old: DomainHealth,
        new: DomainHealth,
    },
    /// A node was added to a domain
    NodeAdded {
        domain_id: DomainId,
        node_id: NodeId,
    },
    /// A node was removed from a domain
    NodeRemoved {
        domain_id: DomainId,
        node_id: NodeId,
    },
    /// Node status changed
    NodeStatusChanged {
        domain_id: DomainId,
        node_id: NodeId,
        old: NodeStatus,
        new: NodeStatus,
    },
}

/// Registry of all known domains in the cluster
pub struct DomainRegistry {
    /// All registered domains
    domains: DashMap<DomainId, Domain>,

    /// Local domain ID (this node's domain)
    local_domain_id: DomainId,

    /// Event broadcaster
    events: broadcast::Sender<DomainEvent>,

    /// Heartbeat timeout
    heartbeat_timeout: Duration,
}

impl DomainRegistry {
    /// Create a new domain registry
    pub fn new(local_domain_id: DomainId) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            domains: DashMap::new(),
            local_domain_id,
            events,
            heartbeat_timeout: Duration::from_secs(30),
        }
    }

    /// Set heartbeat timeout
    pub fn with_heartbeat_timeout(mut self, timeout: Duration) -> Self {
        self.heartbeat_timeout = timeout;
        self
    }

    /// Get the local domain ID
    pub fn local_domain_id(&self) -> DomainId {
        self.local_domain_id
    }

    /// Subscribe to domain events
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.events.subscribe()
    }

    /// Register a new domain
    pub fn register_domain(&self, domain: Domain) -> Result<()> {
        let domain_id = domain.id;

        if self.domains.contains_key(&domain_id) {
            return Err(Error::DomainAlreadyExists(domain_id));
        }

        info!(domain_id, name = %domain.name, "Registering domain");
        self.domains.insert(domain_id, domain);

        let _ = self.events.send(DomainEvent::DomainAdded(domain_id));
        Ok(())
    }

    /// Remove a domain
    pub fn remove_domain(&self, domain_id: DomainId) -> Result<Domain> {
        self.domains
            .remove(&domain_id)
            .map(|(_, d)| {
                info!(domain_id, "Removed domain");
                let _ = self.events.send(DomainEvent::DomainRemoved(domain_id));
                d
            })
            .ok_or_else(|| Error::DomainNotFound(domain_id))
    }

    /// Get a domain by ID
    pub fn get_domain(&self, domain_id: DomainId) -> Option<Domain> {
        self.domains.get(&domain_id).map(|d| d.clone())
    }

    /// Get the local domain
    pub fn local_domain(&self) -> Option<Domain> {
        self.get_domain(self.local_domain_id)
    }

    /// List all domain IDs
    pub fn domain_ids(&self) -> Vec<DomainId> {
        self.domains.iter().map(|r| *r.key()).collect()
    }

    /// List all domains
    pub fn list_domains(&self) -> Vec<Domain> {
        self.domains.iter().map(|r| r.value().clone()).collect()
    }

    /// Get healthy domains
    pub fn healthy_domains(&self) -> Vec<Domain> {
        self.domains
            .iter()
            .filter(|r| matches!(r.health, DomainHealth::Healthy | DomainHealth::Degraded))
            .map(|r| r.value().clone())
            .collect()
    }

    /// Check if a specific domain is healthy
    pub async fn is_healthy(&self, domain_id: DomainId) -> bool {
        self.domains
            .get(&domain_id)
            .map(|d| matches!(d.health, DomainHealth::Healthy | DomainHealth::Degraded))
            .unwrap_or(false)
    }

    /// Get domains sorted by proximity to local domain
    ///
    /// Returns domains in order: local first, then by region, then others
    pub fn domains_by_proximity(&self) -> Vec<Domain> {
        let local = self.local_domain();
        let local_region = local.as_ref().and_then(|d| d.region.clone());

        let mut domains: Vec<Domain> = self.list_domains();

        domains.sort_by(|a, b| {
            // Local domain always first
            if a.id == self.local_domain_id {
                return std::cmp::Ordering::Less;
            }
            if b.id == self.local_domain_id {
                return std::cmp::Ordering::Greater;
            }

            // Same region domains next
            let a_same_region = local_region
                .as_ref()
                .map(|lr| a.region.as_ref() == Some(lr))
                .unwrap_or(false);
            let b_same_region = local_region
                .as_ref()
                .map(|lr| b.region.as_ref() == Some(lr))
                .unwrap_or(false);

            match (a_same_region, b_same_region) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.id.cmp(&b.id),
            }
        });

        domains
    }

    /// Add a node to a domain
    pub fn add_node(&self, domain_id: DomainId, node: NodeInfo) -> Result<()> {
        let mut domain = self
            .domains
            .get_mut(&domain_id)
            .ok_or_else(|| Error::DomainNotFound(domain_id))?;

        let node_id = node.node_id;
        domain.add_node(node);

        debug!(domain_id, node_id, "Added node to domain");
        let _ = self.events.send(DomainEvent::NodeAdded {
            domain_id,
            node_id,
        });

        Ok(())
    }

    /// Remove a node from a domain
    pub fn remove_node(&self, domain_id: DomainId, node_id: NodeId) -> Result<()> {
        let mut domain = self
            .domains
            .get_mut(&domain_id)
            .ok_or_else(|| Error::DomainNotFound(domain_id))?;

        let initial_len = domain.nodes.len();
        domain.nodes.retain(|n| n.node_id != node_id);

        if domain.nodes.len() == initial_len {
            return Err(Error::NodeNotFound(node_id));
        }

        debug!(domain_id, node_id, "Removed node from domain");
        let _ = self.events.send(DomainEvent::NodeRemoved {
            domain_id,
            node_id,
        });

        Ok(())
    }

    /// Update node status
    pub fn update_node_status(
        &self,
        domain_id: DomainId,
        node_id: NodeId,
        status: NodeStatus,
    ) -> Result<()> {
        let mut domain = self
            .domains
            .get_mut(&domain_id)
            .ok_or_else(|| Error::DomainNotFound(domain_id))?;

        let node = domain
            .nodes
            .iter_mut()
            .find(|n| n.node_id == node_id)
            .ok_or_else(|| Error::NodeNotFound(node_id))?;

        let old_status = node.status;
        node.status = status;

        if status == NodeStatus::Online {
            node.record_heartbeat();
        }

        if old_status != status {
            debug!(domain_id, node_id, ?old_status, ?status, "Node status changed");
            let _ = self.events.send(DomainEvent::NodeStatusChanged {
                domain_id,
                node_id,
                old: old_status,
                new: status,
            });
        }

        Ok(())
    }

    /// Update domain health based on node statuses
    pub fn update_domain_health(&self, domain_id: DomainId) -> Result<DomainHealth> {
        let mut domain = self
            .domains
            .get_mut(&domain_id)
            .ok_or_else(|| Error::DomainNotFound(domain_id))?;

        let total_nodes = domain.nodes.len();
        let healthy_nodes = domain.healthy_nodes().len();

        let old_health = domain.health;
        let new_health = if total_nodes == 0 {
            DomainHealth::Unknown
        } else if healthy_nodes == total_nodes {
            DomainHealth::Healthy
        } else if healthy_nodes == 0 {
            DomainHealth::Offline
        } else if healthy_nodes * 2 >= total_nodes {
            DomainHealth::Degraded
        } else {
            DomainHealth::Unhealthy
        };

        domain.health = new_health;

        if new_health == DomainHealth::Healthy {
            domain.last_healthy = Some(Instant::now());
        }

        if old_health != new_health {
            warn!(domain_id, ?old_health, ?new_health, "Domain health changed");
            let _ = self.events.send(DomainEvent::DomainHealthChanged {
                domain_id,
                old: old_health,
                new: new_health,
            });
        }

        Ok(new_health)
    }

    /// Check for stale heartbeats and mark nodes offline
    pub fn check_heartbeats(&self) {
        for mut entry in self.domains.iter_mut() {
            let domain_id = *entry.key();
            for node in entry.nodes.iter_mut() {
                if node.is_heartbeat_stale(self.heartbeat_timeout)
                    && node.status == NodeStatus::Online
                {
                    let old_status = node.status;
                    node.status = NodeStatus::Offline;

                    warn!(
                        domain_id,
                        node_id = node.node_id,
                        "Node heartbeat stale, marking offline"
                    );

                    let _ = self.events.send(DomainEvent::NodeStatusChanged {
                        domain_id,
                        node_id: node.node_id,
                        old: old_status,
                        new: NodeStatus::Offline,
                    });
                }
            }
        }
    }

    /// Get total cluster capacity
    pub fn total_capacity(&self) -> u64 {
        self.domains.iter().map(|d| d.total_capacity()).sum()
    }

    /// Get available cluster capacity
    pub fn available_capacity(&self) -> u64 {
        self.domains.iter().map(|d| d.available_capacity()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_creation() {
        let domain = Domain::new(1, "us-west")
            .with_region("us-west-1");

        assert_eq!(domain.id, 1);
        assert_eq!(domain.name, "us-west");
        assert_eq!(domain.region, Some("us-west-1".to_string()));
        assert!(domain.nodes.is_empty());
    }

    #[test]
    fn test_node_heartbeat() {
        let mut node = NodeInfo::new(
            1,
            1,
            "127.0.0.1:8080".parse().unwrap(),
            [0u8; 32],
        );

        assert!(node.is_heartbeat_stale(Duration::from_secs(1)));

        node.record_heartbeat();
        assert!(!node.is_heartbeat_stale(Duration::from_secs(1)));
        assert_eq!(node.status, NodeStatus::Online);
    }

    #[test]
    fn test_storage_capacity() {
        let cap = StorageCapacity::new(1000, 250);
        assert_eq!(cap.total_bytes, 1000);
        assert_eq!(cap.used_bytes, 250);
        assert_eq!(cap.available_bytes, 750);
        assert!((cap.usage_ratio() - 0.25).abs() < f64::EPSILON);
        assert!(!cap.is_nearly_full());

        let full = StorageCapacity::new(1000, 950);
        assert!(full.is_nearly_full());
    }

    #[test]
    fn test_domain_registry() {
        let registry = DomainRegistry::new(1);

        // Register domains
        let domain1 = Domain::new(1, "local").with_region("us-west");
        let domain2 = Domain::new(2, "remote").with_region("eu-central");

        registry.register_domain(domain1).unwrap();
        registry.register_domain(domain2).unwrap();

        assert_eq!(registry.domain_ids().len(), 2);
        assert_eq!(registry.local_domain_id(), 1);

        // Check proximity ordering
        let by_prox = registry.domains_by_proximity();
        assert_eq!(by_prox[0].id, 1); // Local first
    }

    #[test]
    fn test_node_management() {
        let registry = DomainRegistry::new(1);
        registry.register_domain(Domain::new(1, "test")).unwrap();

        let node = NodeInfo::new(
            1,
            1,
            "127.0.0.1:8080".parse().unwrap(),
            [0u8; 32],
        );
        registry.add_node(1, node).unwrap();

        // Update status
        registry.update_node_status(1, 1, NodeStatus::Online).unwrap();

        let domain = registry.get_domain(1).unwrap();
        assert_eq!(domain.healthy_nodes().len(), 1);

        // Update domain health
        let health = registry.update_domain_health(1).unwrap();
        assert_eq!(health, DomainHealth::Healthy);
    }
}
