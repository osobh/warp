//! NVMe over Fabrics (NVMe-oF) support for WARP
//!
//! This module provides full NVMe-oF Target support, allowing WARP storage
//! to be exposed as NVMe namespaces accessible via standard NVMe-oF tools
//! like `nvme-cli`.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                    NVMe-oF Target                          │
//! ├────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────┐  ┌─────────────────┐                  │
//! │  │   Subsystem 1   │  │   Subsystem 2   │  ...             │
//! │  │  (NQN: warp:s1) │  │  (NQN: warp:s2) │                  │
//! │  ├─────────────────┤  ├─────────────────┤                  │
//! │  │ NS1 │ NS2 │ ... │  │ NS1 │ NS2 │ ... │                  │
//! │  └──┬────┬─────────┘  └─────────────────┘                  │
//! │     │    │                                                 │
//! │     │    └─────────────────┐                               │
//! │     ▼                      ▼                               │
//! │  ┌─────────┐         ┌─────────┐                           │
//! │  │ Volume  │         │ Volume  │  (warp-block volumes)     │
//! │  └─────────┘         └─────────┘                           │
//! └────────────────────────────────────────────────────────────┘
//!                           │
//!           ┌───────────────┼───────────────┐
//!           ▼               ▼               ▼
//!     ┌──────────┐   ┌──────────┐   ┌──────────┐
//!     │   RDMA   │   │   QUIC   │   │   TCP    │
//!     │Transport │   │Transport │   │Transport │
//!     └──────────┘   └──────────┘   └──────────┘
//! ```
//!
//! # Features
//!
//! - **NVMe-oF Target**: Expose WARP volumes as NVMe namespaces
//! - **Multiple Transports**: RDMA (primary), QUIC, TCP (fallback)
//! - **Discovery Service**: RFC 8154 compliant discovery
//! - **Multi-path I/O**: ANA (Asymmetric Namespace Access) support
//! - **Standard Compliance**: NVMe 1.4 and NVMe-oF 1.1 compatible
//!
//! # Example
//!
//! ```rust,no_run
//! use warp_block::nvmeof::{NvmeOfTarget, NvmeOfConfig, SubsystemConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create target
//! let config = NvmeOfConfig::default();
//! let target = NvmeOfTarget::new(config).await?;
//!
//! // Create a subsystem
//! let nqn = target.create_subsystem(SubsystemConfig {
//!     name: "storage".into(),
//!     ..Default::default()
//! })?;
//!
//! // Add a namespace backed by a WARP volume
//! // target.add_namespace(&nqn, &volume_id)?;
//!
//! // Start serving
//! target.run().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Client Access
//!
//! Clients can connect using standard `nvme-cli`:
//!
//! ```bash
//! # Discover available subsystems
//! nvme discover -t tcp -a 192.168.1.100 -s 4420
//!
//! # Connect to a subsystem
//! nvme connect -t tcp -a 192.168.1.100 -s 4420 -n nqn.2024-01.io.warp:storage
//!
//! # Verify connection
//! nvme list
//! ```

#![cfg_attr(not(feature = "nvmeof"), allow(unused))]

pub mod capsule;
pub mod command;
pub mod config;
pub mod error;

#[cfg(feature = "nvmeof")]
pub mod connection;
#[cfg(feature = "nvmeof")]
pub mod discovery;
#[cfg(feature = "nvmeof")]
pub mod namespace;
#[cfg(feature = "nvmeof")]
pub mod queue;
#[cfg(feature = "nvmeof")]
pub mod subsystem;
#[cfg(feature = "nvmeof")]
pub mod target;
#[cfg(feature = "nvmeof")]
pub mod transport;

// Re-exports
pub use capsule::{
    CommandCapsule, ConnectData, ControllerCapabilities, ControllerConfiguration,
    ControllerStatus, IcReq, IcResp, PduHeader, PduType, PropertyOffset, ResponseCapsule,
};
pub use command::{
    AdminOpcode, FabricsType, FeatureId, IdentifyCns, IdentifyController, IdentifyNamespace,
    IoOpcode, NvmeCommand, NvmeCompletion,
};
pub use config::{
    NvmeOfConfig, NvmeOfRdmaConfig, NvmeOfTcpConfig, SubsystemConfig, TransportConfig,
};
pub use error::{NvmeOfError, NvmeOfResult, NvmeStatus};

#[cfg(feature = "nvmeof")]
pub use connection::NvmeOfConnection;
#[cfg(feature = "nvmeof")]
pub use discovery::DiscoveryService;
#[cfg(feature = "nvmeof")]
pub use namespace::{AsyncVolume, NvmeOfNamespace};
#[cfg(feature = "nvmeof")]
pub use queue::{AdminQueue, IoQueue, QueuePair};
#[cfg(feature = "nvmeof")]
pub use subsystem::NvmeOfSubsystem;
#[cfg(feature = "nvmeof")]
pub use target::NvmeOfTarget;

/// NVMe Qualified Name (NQN) prefix for WARP
pub const WARP_NQN_PREFIX: &str = "nqn.2024-01.io.warp:";

/// Generate an NQN for a WARP subsystem
pub fn generate_nqn(name: &str) -> String {
    format!("{}{}", WARP_NQN_PREFIX, name)
}

/// Validate an NQN format
pub fn validate_nqn(nqn: &str) -> bool {
    // NQN format: nqn.YYYY-MM.reverse.domain:name
    // Or: nqn.YYYY-MM.reverse.domain.uuid:uuid
    if !nqn.starts_with("nqn.") {
        return false;
    }

    // Check length (max 223 characters per spec)
    if nqn.len() > 223 {
        return false;
    }

    // Basic format validation
    let parts: Vec<&str> = nqn.splitn(2, ':').collect();
    if parts.len() < 2 {
        // uuid-based NQN might not have colon
        return nqn.len() >= 12; // minimum: nqn.YYYY-MM
    }

    true
}

/// Discovery NQN (well-known)
pub const DISCOVERY_NQN: &str = "nqn.2014-08.org.nvmexpress.discovery";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_nqn() {
        let nqn = generate_nqn("storage");
        assert_eq!(nqn, "nqn.2024-01.io.warp:storage");
    }

    #[test]
    fn test_validate_nqn() {
        assert!(validate_nqn("nqn.2024-01.io.warp:storage"));
        assert!(validate_nqn("nqn.2014-08.org.nvmexpress.discovery"));
        assert!(!validate_nqn("invalid"));
        assert!(!validate_nqn("nq.2024-01.io.warp:storage")); // Wrong prefix
    }
}
