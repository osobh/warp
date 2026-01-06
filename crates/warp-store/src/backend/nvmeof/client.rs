//! NVMe-oF Client
//!
//! Client for connecting to NVMe-oF targets.

use std::sync::Arc;

use bytes::Bytes;
use tracing::{debug, trace};

use super::config::{NvmeOfBackendConfig, TransportPreference};
use super::error::NvmeOfBackendResult;
use super::pool::{NvmeOfConnectionPool, PooledConnection};

/// NVMe-oF client for initiator operations
pub struct NvmeOfClient {
    /// Configuration
    config: NvmeOfBackendConfig,

    /// Connection pool
    pool: Arc<NvmeOfConnectionPool>,
}

impl NvmeOfClient {
    /// Create a new client
    pub async fn new(config: NvmeOfBackendConfig) -> NvmeOfBackendResult<Self> {
        let pool = Arc::new(NvmeOfConnectionPool::new(
            config.pool.clone(),
            config.transport_preference.clone(),
        ));

        // Add all configured targets to the pool
        for target in &config.targets {
            pool.add_target(target.clone())?;
        }

        debug!(
            "NVMe-oF client created with {} targets",
            config.targets.len()
        );

        Ok(Self { config, pool })
    }

    /// Get connection pool
    pub fn pool(&self) -> &Arc<NvmeOfConnectionPool> {
        &self.pool
    }

    /// Get a connection to a target
    pub async fn get_connection(&self, nqn: &str) -> NvmeOfBackendResult<Arc<PooledConnection>> {
        self.pool.get_connection(nqn).await
    }

    /// Read blocks from a namespace
    pub async fn read(
        &self,
        target_nqn: &str,
        namespace_id: u32,
        start_lba: u64,
        block_count: u32,
    ) -> NvmeOfBackendResult<Bytes> {
        let conn = self.get_connection(target_nqn).await?;
        conn.begin_command();

        // In real implementation, this would:
        // 1. Build NVMe Read command
        // 2. Send via transport
        // 3. Receive data
        // For now, return placeholder

        let data_size = block_count as usize * self.config.block_size as usize;
        let data = Bytes::from(vec![0u8; data_size]);

        conn.end_command();
        self.pool.return_connection(conn);

        trace!(
            "Read {} blocks from LBA {} on {}/ns{}",
            block_count,
            start_lba,
            target_nqn,
            namespace_id
        );

        Ok(data)
    }

    /// Write blocks to a namespace
    pub async fn write(
        &self,
        target_nqn: &str,
        namespace_id: u32,
        start_lba: u64,
        data: Bytes,
    ) -> NvmeOfBackendResult<()> {
        let conn = self.get_connection(target_nqn).await?;
        conn.begin_command();

        // In real implementation, this would:
        // 1. Build NVMe Write command
        // 2. Send via transport with data
        // 3. Wait for completion

        let block_count =
            (data.len() + self.config.block_size as usize - 1) / self.config.block_size as usize;

        conn.end_command();
        self.pool.return_connection(conn);

        trace!(
            "Wrote {} blocks to LBA {} on {}/ns{}",
            block_count,
            start_lba,
            target_nqn,
            namespace_id
        );

        Ok(())
    }

    /// Flush a namespace
    pub async fn flush(&self, target_nqn: &str, namespace_id: u32) -> NvmeOfBackendResult<()> {
        let conn = self.get_connection(target_nqn).await?;
        conn.begin_command();

        // Send flush command

        conn.end_command();
        self.pool.return_connection(conn);

        trace!("Flushed {}/ns{}", target_nqn, namespace_id);
        Ok(())
    }

    /// TRIM/Deallocate blocks
    pub async fn trim(
        &self,
        target_nqn: &str,
        namespace_id: u32,
        start_lba: u64,
        block_count: u32,
    ) -> NvmeOfBackendResult<()> {
        let conn = self.get_connection(target_nqn).await?;
        conn.begin_command();

        // Send Dataset Management (TRIM) command

        conn.end_command();
        self.pool.return_connection(conn);

        trace!(
            "Trimmed {} blocks from LBA {} on {}/ns{}",
            block_count,
            start_lba,
            target_nqn,
            namespace_id
        );

        Ok(())
    }

    /// Discover targets
    pub async fn discover(&self, address: &str) -> NvmeOfBackendResult<Vec<DiscoveredTarget>> {
        // In real implementation, this would:
        // 1. Connect to discovery service
        // 2. Get log page 0x70
        // 3. Parse discovery entries

        debug!("Discovering targets at {}", address);
        Ok(Vec::new())
    }

    /// Get namespace information
    pub async fn get_namespace_info(
        &self,
        target_nqn: &str,
        namespace_id: u32,
    ) -> NvmeOfBackendResult<NamespaceInfo> {
        let conn = self.get_connection(target_nqn).await?;
        conn.begin_command();

        // Send Identify Namespace command

        conn.end_command();
        self.pool.return_connection(conn);

        // Return placeholder info
        Ok(NamespaceInfo {
            namespace_id,
            size_blocks: 0,
            block_size: self.config.block_size,
            capacity_blocks: 0,
            utilization_blocks: 0,
        })
    }

    /// Get target list
    pub fn targets(&self) -> Vec<String> {
        self.config.targets.iter().map(|t| t.nqn.clone()).collect()
    }
}

/// Discovered NVMe-oF target
#[derive(Debug, Clone)]
pub struct DiscoveredTarget {
    /// Target NQN
    pub nqn: String,

    /// Transport type
    pub transport: TransportPreference,

    /// Address
    pub address: String,

    /// Port
    pub port: u16,

    /// Subsystem type (discovery or NVM)
    pub subsystem_type: u8,
}

/// Namespace information
#[derive(Debug, Clone)]
pub struct NamespaceInfo {
    /// Namespace ID
    pub namespace_id: u32,

    /// Size in blocks
    pub size_blocks: u64,

    /// Block size
    pub block_size: u32,

    /// Capacity in blocks
    pub capacity_blocks: u64,

    /// Current utilization in blocks
    pub utilization_blocks: u64,
}

impl NamespaceInfo {
    /// Get size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.size_blocks * self.block_size as u64
    }

    /// Get utilization percentage
    pub fn utilization_percent(&self) -> f64 {
        if self.capacity_blocks == 0 {
            return 0.0;
        }
        (self.utilization_blocks as f64 / self.capacity_blocks as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::NvmeOfTargetConfig;
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let config = NvmeOfBackendConfig {
            targets: vec![NvmeOfTargetConfig {
                nqn: "nqn.2024-01.io.warp:test".to_string(),
                addresses: vec!["127.0.0.1:4420".parse().unwrap()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let client = NvmeOfClient::new(config).await.unwrap();
        assert_eq!(client.targets().len(), 1);
    }
}
