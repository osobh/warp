//! NVMe-oF Discovery Service
//!
//! This module implements the NVMe-oF Discovery service according to
//! the NVMe over Fabrics specification.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{BufMut, Bytes, BytesMut};
use parking_lot::RwLock;
use tracing::{debug, info};

use super::config::{AddressFamily, DiscoveryLogEntry, SubsystemType, TransportType};
use super::subsystem::SubsystemManager;

/// Discovery log page ID
pub const DISCOVERY_LOG_PAGE: u8 = 0x70;

/// Discovery service for NVMe-oF
pub struct DiscoveryService {
    /// Subsystem manager reference
    subsystems: Arc<SubsystemManager>,

    /// Listen addresses for discovery
    listen_addrs: RwLock<Vec<DiscoveryAddress>>,
}

/// Address advertised in discovery log
#[derive(Debug, Clone)]
pub struct DiscoveryAddress {
    /// Transport type
    pub transport: TransportType,

    /// Address family
    pub adrfam: AddressFamily,

    /// Address string
    pub traddr: String,

    /// Service ID (port)
    pub trsvcid: String,
}

impl DiscoveryService {
    /// Create a new discovery service
    pub fn new(subsystems: Arc<SubsystemManager>) -> Self {
        Self {
            subsystems,
            listen_addrs: RwLock::new(Vec::new()),
        }
    }

    /// Add a listen address
    pub fn add_listen_addr(&self, transport: TransportType, addr: SocketAddr) {
        let adrfam = if addr.is_ipv4() {
            AddressFamily::Ipv4
        } else {
            AddressFamily::Ipv6
        };

        let discovery_addr = DiscoveryAddress {
            transport,
            adrfam,
            traddr: addr.ip().to_string(),
            trsvcid: addr.port().to_string(),
        };

        self.listen_addrs.write().push(discovery_addr);
        info!("Added discovery address: {}:{}", addr.ip(), addr.port());
    }

    /// Remove all listen addresses
    pub fn clear_listen_addrs(&self) {
        self.listen_addrs.write().clear();
    }

    /// Generate discovery log entries
    pub fn generate_log_entries(&self) -> Vec<DiscoveryLogEntry> {
        let mut entries = Vec::new();
        let listen_addrs = self.listen_addrs.read();

        // Add entry for each subsystem on each listen address
        for subsystem in self.subsystems.list_io_subsystems() {
            let nqn = subsystem.nqn();

            for (port_id, addr) in listen_addrs.iter().enumerate() {
                let entry = DiscoveryLogEntry {
                    trtype: addr.transport,
                    adrfam: addr.adrfam,
                    subtype: SubsystemType::Nvm,
                    treq: 0, // No secure channel required
                    portid: port_id as u16,
                    cntlid: 0xFFFF, // Dynamic allocation
                    asqsz: 128,     // Admin SQ size
                    trsvcid: addr.trsvcid.clone(),
                    traddr: addr.traddr.clone(),
                    subnqn: nqn.to_string(),
                };

                entries.push(entry);
            }
        }

        // Add discovery subsystem entry on each address
        for (port_id, addr) in listen_addrs.iter().enumerate() {
            let entry = DiscoveryLogEntry {
                trtype: addr.transport,
                adrfam: addr.adrfam,
                subtype: SubsystemType::Discovery,
                treq: 0,
                portid: port_id as u16,
                cntlid: 0xFFFF,
                asqsz: 128,
                trsvcid: addr.trsvcid.clone(),
                traddr: addr.traddr.clone(),
                subnqn: super::DISCOVERY_NQN.to_string(),
            };

            entries.push(entry);
        }

        debug!("Generated {} discovery log entries", entries.len());
        entries
    }

    /// Generate discovery log page data
    pub fn generate_log_page(&self) -> Bytes {
        let entries = self.generate_log_entries();

        // Discovery log page header (8 bytes)
        // + entries (1024 bytes each according to spec, but we use compact format)
        let entry_size = 1024;
        let total_size = 8 + entries.len() * entry_size;

        let mut buf = BytesMut::with_capacity(total_size);

        // Log page header
        buf.put_u64_le(entries.len() as u64); // Generation Counter + Number of Records

        // Write each entry
        for entry in &entries {
            self.write_log_entry(&mut buf, entry);
        }

        buf.freeze()
    }

    /// Write a single discovery log entry
    fn write_log_entry(&self, buf: &mut BytesMut, entry: &DiscoveryLogEntry) {
        let start = buf.len();

        // Transport type (1 byte)
        buf.put_u8(entry.trtype as u8);

        // Address family (1 byte)
        buf.put_u8(entry.adrfam as u8);

        // Subsystem type (1 byte)
        buf.put_u8(entry.subtype as u8);

        // Transport requirements (1 byte)
        buf.put_u8(entry.treq);

        // Port ID (2 bytes)
        buf.put_u16_le(entry.portid);

        // Controller ID (2 bytes)
        buf.put_u16_le(entry.cntlid);

        // Admin SQ size (2 bytes)
        buf.put_u16_le(entry.asqsz);

        // Reserved (22 bytes)
        buf.put_bytes(0, 22);

        // Transport service ID (32 bytes, null-padded)
        let trsvcid_bytes = entry.trsvcid.as_bytes();
        let trsvcid_len = trsvcid_bytes.len().min(32);
        buf.put_slice(&trsvcid_bytes[..trsvcid_len]);
        buf.put_bytes(0, 32 - trsvcid_len);

        // Reserved (192 bytes)
        buf.put_bytes(0, 192);

        // Transport address (256 bytes, null-padded)
        let traddr_bytes = entry.traddr.as_bytes();
        let traddr_len = traddr_bytes.len().min(256);
        buf.put_slice(&traddr_bytes[..traddr_len]);
        buf.put_bytes(0, 256 - traddr_len);

        // Transport specific address (256 bytes, reserved)
        buf.put_bytes(0, 256);

        // Subsystem NQN (256 bytes, null-padded)
        let subnqn_bytes = entry.subnqn.as_bytes();
        let subnqn_len = subnqn_bytes.len().min(256);
        buf.put_slice(&subnqn_bytes[..subnqn_len]);
        buf.put_bytes(0, 256 - subnqn_len);

        // Pad to 1024 bytes
        let current = buf.len() - start;
        if current < 1024 {
            buf.put_bytes(0, 1024 - current);
        }
    }

    /// Get the number of entries in the discovery log
    pub fn entry_count(&self) -> usize {
        let listen_addrs = self.listen_addrs.read();
        let io_subsystems = self.subsystems.list_io_subsystems().len();

        // Each IO subsystem advertised on each address, plus discovery on each address
        (io_subsystems + 1) * listen_addrs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_discovery_service_creation() {
        let subsystems = Arc::new(SubsystemManager::new());
        let discovery = DiscoveryService::new(subsystems);

        assert_eq!(discovery.entry_count(), 0);
    }

    #[test]
    fn test_add_listen_addr() {
        let subsystems = Arc::new(SubsystemManager::new());
        let discovery = DiscoveryService::new(subsystems.clone());

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 4420);
        discovery.add_listen_addr(TransportType::Tcp, addr);

        // Should have discovery subsystem on 1 address
        assert_eq!(discovery.entry_count(), 1);
    }

    #[test]
    fn test_generate_log_entries() {
        use super::super::config::SubsystemConfig;

        let subsystems = Arc::new(SubsystemManager::new());

        // Create an I/O subsystem
        let config = SubsystemConfig {
            name: "test".to_string(),
            ..Default::default()
        };
        subsystems.create(config).unwrap();

        let discovery = DiscoveryService::new(subsystems);

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 4420);
        discovery.add_listen_addr(TransportType::Tcp, addr);

        let entries = discovery.generate_log_entries();

        // Should have 2 entries: 1 I/O subsystem + 1 discovery
        assert_eq!(entries.len(), 2);

        // Verify I/O subsystem entry
        let io_entry = entries
            .iter()
            .find(|e| e.subtype == SubsystemType::Nvm)
            .unwrap();
        assert_eq!(io_entry.subnqn, "nqn.2024-01.io.warp:test");
        assert_eq!(io_entry.traddr, "192.168.1.100");
        assert_eq!(io_entry.trsvcid, "4420");

        // Verify discovery entry
        let disc_entry = entries
            .iter()
            .find(|e| e.subtype == SubsystemType::Discovery)
            .unwrap();
        assert_eq!(disc_entry.subnqn, super::super::DISCOVERY_NQN);
    }

    #[test]
    fn test_generate_log_page() {
        let subsystems = Arc::new(SubsystemManager::new());
        let discovery = DiscoveryService::new(subsystems);

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 4420);
        discovery.add_listen_addr(TransportType::Tcp, addr);

        let log_page = discovery.generate_log_page();

        // Header (8 bytes) + 1 entry (1024 bytes)
        assert_eq!(log_page.len(), 8 + 1024);
    }
}
