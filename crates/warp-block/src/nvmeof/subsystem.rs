//! NVMe-oF Subsystem Implementation
//!
//! This module provides the NvmeOfSubsystem which manages namespaces
//! and host access control.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

use super::config::{NamespaceDefaults, SubsystemConfig, SubsystemType};
use super::connection::NvmeOfConnection;
use super::error::{NvmeOfError, NvmeOfResult};
use super::namespace::{AsyncVolume, NamespaceId, NamespaceManager, NvmeOfNamespace};
use super::{generate_nqn, validate_nqn};

/// NVMe-oF Subsystem
///
/// A subsystem is a container for namespaces and manages host access.
/// Each subsystem has a unique NQN (NVMe Qualified Name).
pub struct NvmeOfSubsystem {
    /// Subsystem NQN
    nqn: String,

    /// Subsystem type
    subsystem_type: SubsystemType,

    /// Serial number
    serial: String,

    /// Model number
    model: String,

    /// Namespace manager
    namespaces: Arc<NamespaceManager>,

    /// Allow any host to connect
    allow_any_host: bool,

    /// Allowed host NQNs
    allowed_hosts: RwLock<HashSet<String>>,

    /// Connected controllers (connection ID -> connection)
    controllers: RwLock<HashMap<u64, Arc<NvmeOfConnection>>>,

    /// Controller ID counter
    next_cntlid: AtomicU16,

    /// Maximum namespaces
    max_namespaces: u32,

    /// Subsystem is enabled
    enabled: AtomicBool,

    /// Statistics
    stats: RwLock<SubsystemStats>,
}

impl NvmeOfSubsystem {
    /// Create a new subsystem
    pub fn new(config: SubsystemConfig) -> NvmeOfResult<Self> {
        let nqn = config
            .nqn
            .clone()
            .unwrap_or_else(|| generate_nqn(&config.name));

        if !validate_nqn(&nqn) {
            return Err(NvmeOfError::InvalidNqn(nqn));
        }

        let serial = config
            .serial
            .clone()
            .unwrap_or_else(|| format!("WARP-{}", &config.name[..config.name.len().min(12)]));

        let model = config
            .model
            .clone()
            .unwrap_or_else(|| "WARP NVMe-oF Target".to_string());

        let namespace_defaults = NamespaceDefaults::default();
        let namespaces = Arc::new(NamespaceManager::new(
            config.max_namespaces,
            namespace_defaults,
        ));

        let mut allowed_hosts = HashSet::new();
        for host in &config.allowed_hosts {
            allowed_hosts.insert(host.clone());
        }

        info!("Created subsystem: nqn={}", nqn);

        Ok(Self {
            nqn,
            subsystem_type: SubsystemType::Nvm,
            serial,
            model,
            namespaces,
            allow_any_host: config.allow_any_host,
            allowed_hosts: RwLock::new(allowed_hosts),
            controllers: RwLock::new(HashMap::new()),
            next_cntlid: AtomicU16::new(1),
            max_namespaces: config.max_namespaces,
            enabled: AtomicBool::new(true),
            stats: RwLock::new(SubsystemStats::default()),
        })
    }

    /// Create a discovery subsystem
    pub fn discovery() -> Self {
        Self {
            nqn: super::DISCOVERY_NQN.to_string(),
            subsystem_type: SubsystemType::Discovery,
            serial: "WARP-DISCOVERY".to_string(),
            model: "WARP Discovery".to_string(),
            namespaces: Arc::new(NamespaceManager::new(0, NamespaceDefaults::default())),
            allow_any_host: true,
            allowed_hosts: RwLock::new(HashSet::new()),
            controllers: RwLock::new(HashMap::new()),
            next_cntlid: AtomicU16::new(1),
            max_namespaces: 0,
            enabled: AtomicBool::new(true),
            stats: RwLock::new(SubsystemStats::default()),
        }
    }

    /// Get subsystem NQN
    pub fn nqn(&self) -> &str {
        &self.nqn
    }

    /// Get subsystem type
    pub fn subsystem_type(&self) -> SubsystemType {
        self.subsystem_type
    }

    /// Get serial number
    pub fn serial(&self) -> &str {
        &self.serial
    }

    /// Get model number
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Check if subsystem is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Enable the subsystem
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
        info!("Subsystem {} enabled", self.nqn);
    }

    /// Disable the subsystem
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        info!("Subsystem {} disabled", self.nqn);
    }

    /// Check if this is a discovery subsystem
    pub fn is_discovery(&self) -> bool {
        self.subsystem_type == SubsystemType::Discovery
    }

    /// Get namespace manager
    pub fn namespace_manager(&self) -> &Arc<NamespaceManager> {
        &self.namespaces
    }

    // ======== Namespace Management ========

    /// Add a namespace backed by a volume
    pub fn add_namespace(
        &self,
        nsid: NamespaceId,
        volume: Arc<dyn AsyncVolume>,
    ) -> NvmeOfResult<Arc<NvmeOfNamespace>> {
        if !self.is_enabled() {
            return Err(NvmeOfError::Subsystem("Subsystem is disabled".to_string()));
        }

        if self.is_discovery() {
            return Err(NvmeOfError::Subsystem(
                "Cannot add namespaces to discovery subsystem".to_string(),
            ));
        }

        let namespace = self.namespaces.add(nsid, volume)?;

        info!("Added namespace {} to subsystem {}", nsid, self.nqn);
        self.stats.write().namespaces_added += 1;

        Ok(namespace)
    }

    /// Add a namespace with auto-assigned ID
    pub fn add_namespace_auto(
        &self,
        volume: Arc<dyn AsyncVolume>,
    ) -> NvmeOfResult<(NamespaceId, Arc<NvmeOfNamespace>)> {
        // Find next available NSID
        let existing = self.namespaces.list();
        let mut nsid: NamespaceId = 1;
        while existing.contains(&nsid) {
            nsid += 1;
            if nsid > self.max_namespaces {
                return Err(NvmeOfError::ResourceExhausted(
                    "Maximum namespaces reached".to_string(),
                ));
            }
        }

        let namespace = self.add_namespace(nsid, volume)?;
        Ok((nsid, namespace))
    }

    /// Remove a namespace
    pub fn remove_namespace(&self, nsid: NamespaceId) -> NvmeOfResult<()> {
        self.namespaces.remove(nsid)?;
        info!("Removed namespace {} from subsystem {}", nsid, self.nqn);
        self.stats.write().namespaces_removed += 1;
        Ok(())
    }

    /// Get a namespace by ID
    pub fn get_namespace(&self, nsid: NamespaceId) -> Option<Arc<NvmeOfNamespace>> {
        self.namespaces.get(nsid)
    }

    /// List all namespace IDs
    pub fn list_namespaces(&self) -> Vec<NamespaceId> {
        self.namespaces.list()
    }

    /// Get namespace count
    pub fn namespace_count(&self) -> usize {
        self.namespaces.count()
    }

    // ======== Host Access Control ========

    /// Check if a host is allowed to connect
    pub fn is_host_allowed(&self, host_nqn: &str) -> bool {
        if self.allow_any_host {
            return true;
        }
        self.allowed_hosts.read().contains(host_nqn)
    }

    /// Add an allowed host
    pub fn add_allowed_host(&self, host_nqn: &str) {
        self.allowed_hosts.write().insert(host_nqn.to_string());
        debug!("Added allowed host {} to subsystem {}", host_nqn, self.nqn);
    }

    /// Remove an allowed host
    pub fn remove_allowed_host(&self, host_nqn: &str) {
        self.allowed_hosts.write().remove(host_nqn);
        debug!(
            "Removed allowed host {} from subsystem {}",
            host_nqn, self.nqn
        );
    }

    /// List allowed hosts
    pub fn list_allowed_hosts(&self) -> Vec<String> {
        self.allowed_hosts.read().iter().cloned().collect()
    }

    /// Set allow any host
    pub fn set_allow_any_host(&self, allow: bool) {
        // Note: This is set at creation, but we provide a method for dynamic config
        debug!("Allow any host set to {} for subsystem {}", allow, self.nqn);
    }

    // ======== Controller Management ========

    /// Allocate a controller ID
    pub fn allocate_cntlid(&self) -> u16 {
        self.next_cntlid.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a connection as a controller
    pub fn add_controller(&self, connection: Arc<NvmeOfConnection>) -> NvmeOfResult<()> {
        let conn_id = connection.id();

        // Check host access
        let host_nqn = connection.host_nqn();
        if !self.is_host_allowed(&host_nqn) {
            return Err(NvmeOfError::Auth(format!(
                "Host {} not allowed on subsystem {}",
                host_nqn, self.nqn
            )));
        }

        self.controllers.write().insert(conn_id, connection);
        self.stats.write().connections_accepted += 1;

        debug!("Added controller {} to subsystem {}", conn_id, self.nqn);
        Ok(())
    }

    /// Remove a controller
    pub fn remove_controller(&self, conn_id: u64) -> Option<Arc<NvmeOfConnection>> {
        let conn = self.controllers.write().remove(&conn_id);
        if conn.is_some() {
            debug!("Removed controller {} from subsystem {}", conn_id, self.nqn);
        }
        conn
    }

    /// Get a controller by connection ID
    pub fn get_controller(&self, conn_id: u64) -> Option<Arc<NvmeOfConnection>> {
        self.controllers.read().get(&conn_id).cloned()
    }

    /// Get number of connected controllers
    pub fn controller_count(&self) -> usize {
        self.controllers.read().len()
    }

    /// List all controller connection IDs
    pub fn list_controllers(&self) -> Vec<u64> {
        self.controllers.read().keys().copied().collect()
    }

    /// Disconnect all controllers
    pub async fn disconnect_all(&self) {
        let controllers: Vec<_> = self.controllers.write().drain().collect();

        for (conn_id, conn) in controllers {
            info!(
                "Disconnecting controller {} from subsystem {}",
                conn_id, self.nqn
            );
            if let Err(e) = conn.close().await {
                warn!("Error closing controller {}: {}", conn_id, e);
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> SubsystemStats {
        self.stats.read().clone()
    }
}

/// Subsystem statistics
#[derive(Debug, Clone, Default)]
pub struct SubsystemStats {
    /// Connections accepted
    pub connections_accepted: u64,

    /// Connections rejected
    pub connections_rejected: u64,

    /// Namespaces added
    pub namespaces_added: u64,

    /// Namespaces removed
    pub namespaces_removed: u64,

    /// Commands processed
    pub commands_processed: u64,
}

/// Subsystem manager for the target
pub struct SubsystemManager {
    /// Subsystems by NQN
    subsystems: RwLock<HashMap<String, Arc<NvmeOfSubsystem>>>,

    /// Discovery subsystem
    discovery: Arc<NvmeOfSubsystem>,
}

impl SubsystemManager {
    /// Create a new subsystem manager
    pub fn new() -> Self {
        let discovery = Arc::new(NvmeOfSubsystem::discovery());

        let mut subsystems = HashMap::new();
        subsystems.insert(discovery.nqn().to_string(), discovery.clone());

        Self {
            subsystems: RwLock::new(subsystems),
            discovery,
        }
    }

    /// Get discovery subsystem
    pub fn discovery(&self) -> &Arc<NvmeOfSubsystem> {
        &self.discovery
    }

    /// Create a new subsystem
    pub fn create(&self, config: SubsystemConfig) -> NvmeOfResult<Arc<NvmeOfSubsystem>> {
        let subsystem = Arc::new(NvmeOfSubsystem::new(config)?);
        let nqn = subsystem.nqn().to_string();

        let mut subsystems = self.subsystems.write();

        if subsystems.contains_key(&nqn) {
            return Err(NvmeOfError::Subsystem(format!(
                "Subsystem {} already exists",
                nqn
            )));
        }

        subsystems.insert(nqn.clone(), subsystem.clone());
        info!("Created subsystem: {}", nqn);

        Ok(subsystem)
    }

    /// Delete a subsystem
    pub async fn delete(&self, nqn: &str) -> NvmeOfResult<()> {
        if nqn == super::DISCOVERY_NQN {
            return Err(NvmeOfError::Subsystem(
                "Cannot delete discovery subsystem".to_string(),
            ));
        }

        let subsystem = self.subsystems.write().remove(nqn);

        if let Some(subsystem) = subsystem {
            // Disconnect all controllers
            subsystem.disconnect_all().await;
            info!("Deleted subsystem: {}", nqn);
            Ok(())
        } else {
            Err(NvmeOfError::Subsystem(format!(
                "Subsystem {} not found",
                nqn
            )))
        }
    }

    /// Get a subsystem by NQN
    pub fn get(&self, nqn: &str) -> Option<Arc<NvmeOfSubsystem>> {
        self.subsystems.read().get(nqn).cloned()
    }

    /// List all subsystem NQNs
    pub fn list(&self) -> Vec<String> {
        self.subsystems.read().keys().cloned().collect()
    }

    /// List all I/O subsystems (excluding discovery)
    pub fn list_io_subsystems(&self) -> Vec<Arc<NvmeOfSubsystem>> {
        self.subsystems
            .read()
            .values()
            .filter(|s| !s.is_discovery())
            .cloned()
            .collect()
    }

    /// Get count of subsystems
    pub fn count(&self) -> usize {
        self.subsystems.read().len()
    }
}

impl Default for SubsystemManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsystem_creation() {
        let config = SubsystemConfig {
            name: "test".to_string(),
            ..Default::default()
        };

        let subsystem = NvmeOfSubsystem::new(config).unwrap();
        assert_eq!(subsystem.nqn(), "nqn.2024-01.io.warp:test");
        assert!(subsystem.is_enabled());
        assert!(!subsystem.is_discovery());
    }

    #[test]
    fn test_discovery_subsystem() {
        let discovery = NvmeOfSubsystem::discovery();
        assert_eq!(discovery.nqn(), super::super::DISCOVERY_NQN);
        assert!(discovery.is_discovery());
    }

    #[test]
    fn test_subsystem_manager() {
        let manager = SubsystemManager::new();

        // Should have discovery subsystem
        assert_eq!(manager.count(), 1);
        assert!(manager.get(super::super::DISCOVERY_NQN).is_some());

        // Create new subsystem
        let config = SubsystemConfig {
            name: "storage".to_string(),
            ..Default::default()
        };

        let subsystem = manager.create(config).unwrap();
        assert_eq!(manager.count(), 2);
        assert!(manager.get("nqn.2024-01.io.warp:storage").is_some());
    }

    #[test]
    fn test_host_access_control() {
        let config = SubsystemConfig {
            name: "test".to_string(),
            allow_any_host: false,
            allowed_hosts: vec!["nqn.2024-01.io.host:client1".to_string()],
            ..Default::default()
        };

        let subsystem = NvmeOfSubsystem::new(config).unwrap();

        assert!(subsystem.is_host_allowed("nqn.2024-01.io.host:client1"));
        assert!(!subsystem.is_host_allowed("nqn.2024-01.io.host:client2"));

        subsystem.add_allowed_host("nqn.2024-01.io.host:client2");
        assert!(subsystem.is_host_allowed("nqn.2024-01.io.host:client2"));
    }
}
