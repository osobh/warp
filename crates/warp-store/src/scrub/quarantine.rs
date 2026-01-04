//! Quarantine management for corrupted blocks

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::replication::ShardKey;

/// Reason for quarantining a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuarantineReason {
    /// Checksum mismatch
    ChecksumMismatch,
    /// Metadata corruption
    MetadataCorruption,
    /// Read error
    ReadError,
    /// Erasure coding verification failed
    ErasureVerificationFailed,
    /// Multiple failures on same block
    RepeatedFailures,
    /// Suspected bitrot
    Bitrot,
    /// Manual quarantine by admin
    ManualQuarantine,
}

impl QuarantineReason {
    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::ChecksumMismatch => "Checksum verification failed",
            Self::MetadataCorruption => "Metadata is corrupted or invalid",
            Self::ReadError => "Failed to read data from storage",
            Self::ErasureVerificationFailed => "Erasure coding parity check failed",
            Self::RepeatedFailures => "Multiple failures detected on this block",
            Self::Bitrot => "Silent data corruption detected",
            Self::ManualQuarantine => "Manually quarantined by administrator",
        }
    }

    /// Check if this reason suggests the data is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::ChecksumMismatch | Self::Bitrot | Self::ErasureVerificationFailed => true,
            Self::MetadataCorruption | Self::ReadError | Self::RepeatedFailures => false,
            Self::ManualQuarantine => true, // Assume recoverable unless proven otherwise
        }
    }
}

/// A quarantined block
#[derive(Debug, Clone)]
pub struct QuarantinedBlock {
    /// The shard key
    pub shard_key: ShardKey,

    /// Reason for quarantine
    pub reason: QuarantineReason,

    /// When the block was quarantined
    pub quarantined_at: SystemTime,

    /// Expected checksum (if available)
    pub expected_checksum: Option<[u8; 32]>,

    /// Actual checksum computed (if available)
    pub actual_checksum: Option<[u8; 32]>,

    /// Number of repair attempts
    pub repair_attempts: u32,

    /// Last repair attempt time
    pub last_repair_attempt: Option<SystemTime>,

    /// Whether repair was requested
    pub repair_requested: bool,

    /// Additional diagnostic info
    pub diagnostic_info: Option<String>,
}

impl QuarantinedBlock {
    /// Create a new quarantined block
    pub fn new(shard_key: ShardKey, reason: QuarantineReason) -> Self {
        Self {
            shard_key,
            reason,
            quarantined_at: SystemTime::now(),
            expected_checksum: None,
            actual_checksum: None,
            repair_attempts: 0,
            last_repair_attempt: None,
            repair_requested: false,
            diagnostic_info: None,
        }
    }

    /// Create with checksum info
    pub fn with_checksums(
        shard_key: ShardKey,
        reason: QuarantineReason,
        expected: [u8; 32],
        actual: [u8; 32],
    ) -> Self {
        Self {
            expected_checksum: Some(expected),
            actual_checksum: Some(actual),
            ..Self::new(shard_key, reason)
        }
    }

    /// Add diagnostic info
    pub fn with_diagnostic(mut self, info: String) -> Self {
        self.diagnostic_info = Some(info);
        self
    }

    /// Record a repair attempt
    pub fn record_repair_attempt(&mut self) {
        self.repair_attempts += 1;
        self.last_repair_attempt = Some(SystemTime::now());
    }

    /// Check if the block has exceeded max repair attempts
    pub fn exceeded_max_repairs(&self, max: u32) -> bool {
        self.repair_attempts >= max
    }

    /// Get age of quarantine
    pub fn age(&self) -> Duration {
        self.quarantined_at.elapsed().unwrap_or(Duration::ZERO)
    }
}

/// Manager for quarantined blocks
pub struct QuarantineManager {
    /// Quarantined blocks by shard key
    blocks: DashMap<ShardKey, QuarantinedBlock>,

    /// Statistics
    stats: RwLock<QuarantineStats>,

    /// Maximum repair attempts before giving up
    max_repair_attempts: u32,

    /// Retention period for quarantined blocks
    retention_period: Duration,
}

/// Statistics for quarantine management
#[derive(Debug, Clone, Default)]
pub struct QuarantineStats {
    /// Total blocks quarantined
    pub total_quarantined: u64,
    /// Blocks successfully repaired
    pub total_repaired: u64,
    /// Blocks permanently failed
    pub total_failed: u64,
    /// Blocks by reason
    pub by_reason: HashMap<QuarantineReason, u64>,
}

impl QuarantineManager {
    /// Create a new quarantine manager
    pub fn new() -> Self {
        Self {
            blocks: DashMap::new(),
            stats: RwLock::new(QuarantineStats::default()),
            max_repair_attempts: 3,
            retention_period: Duration::from_secs(30 * 24 * 3600), // 30 days
        }
    }

    /// Create with custom settings
    pub fn with_settings(max_repair_attempts: u32, retention_period: Duration) -> Self {
        Self {
            blocks: DashMap::new(),
            stats: RwLock::new(QuarantineStats::default()),
            max_repair_attempts,
            retention_period,
        }
    }

    /// Quarantine a block
    pub fn quarantine(&self, block: QuarantinedBlock) {
        let reason = block.reason;
        let key = block.shard_key.clone();

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.total_quarantined += 1;
            *stats.by_reason.entry(reason).or_insert(0) += 1;
        }

        self.blocks.insert(key, block);
    }

    /// Remove a block from quarantine (after successful repair)
    pub fn release(&self, key: &ShardKey) -> Option<QuarantinedBlock> {
        if let Some((_, block)) = self.blocks.remove(key) {
            self.stats.write().total_repaired += 1;
            Some(block)
        } else {
            None
        }
    }

    /// Mark a block as permanently failed
    pub fn mark_failed(&self, key: &ShardKey) -> Option<QuarantinedBlock> {
        if let Some((_, block)) = self.blocks.remove(key) {
            self.stats.write().total_failed += 1;
            Some(block)
        } else {
            None
        }
    }

    /// Check if a shard is quarantined
    pub fn is_quarantined(&self, key: &ShardKey) -> bool {
        self.blocks.contains_key(key)
    }

    /// Get a quarantined block
    pub fn get(&self, key: &ShardKey) -> Option<QuarantinedBlock> {
        self.blocks.get(key).map(|r| r.clone())
    }

    /// Get all quarantined blocks
    pub fn all(&self) -> Vec<QuarantinedBlock> {
        self.blocks.iter().map(|r| r.value().clone()).collect()
    }

    /// Get blocks needing repair
    pub fn pending_repairs(&self) -> Vec<QuarantinedBlock> {
        self.blocks
            .iter()
            .filter(|r| {
                let block = r.value();
                block.reason.is_recoverable()
                    && !block.repair_requested
                    && !block.exceeded_max_repairs(self.max_repair_attempts)
            })
            .map(|r| r.value().clone())
            .collect()
    }

    /// Mark repair as requested
    pub fn mark_repair_requested(&self, key: &ShardKey) {
        if let Some(mut block) = self.blocks.get_mut(key) {
            block.repair_requested = true;
        }
    }

    /// Record a repair attempt
    pub fn record_repair_attempt(&self, key: &ShardKey) {
        if let Some(mut block) = self.blocks.get_mut(key) {
            block.record_repair_attempt();
        }
    }

    /// Clean up old entries
    pub fn cleanup_expired(&self) -> usize {
        let now = SystemTime::now();
        let mut removed = 0;

        self.blocks.retain(|_, block| {
            let age = block.quarantined_at.elapsed().unwrap_or(Duration::ZERO);
            if age > self.retention_period {
                removed += 1;
                false
            } else {
                true
            }
        });

        removed
    }

    /// Get current count
    pub fn count(&self) -> usize {
        self.blocks.len()
    }

    /// Get statistics
    pub fn stats(&self) -> QuarantineStats {
        self.stats.read().clone()
    }

    /// Get summary string
    pub fn summary(&self) -> String {
        let stats = self.stats.read();
        format!(
            "Quarantine: {} active, {} repaired, {} failed, {} total",
            self.blocks.len(),
            stats.total_repaired,
            stats.total_failed,
            stats.total_quarantined
        )
    }
}

impl Default for QuarantineManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quarantine_basic() {
        let manager = QuarantineManager::new();
        let key = ShardKey::new("bucket", "key", 0);

        let block = QuarantinedBlock::new(key.clone(), QuarantineReason::ChecksumMismatch);
        manager.quarantine(block);

        assert!(manager.is_quarantined(&key));
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_quarantine_release() {
        let manager = QuarantineManager::new();
        let key = ShardKey::new("bucket", "key", 0);

        manager.quarantine(QuarantinedBlock::new(key.clone(), QuarantineReason::Bitrot));
        assert!(manager.is_quarantined(&key));

        manager.release(&key);
        assert!(!manager.is_quarantined(&key));

        let stats = manager.stats();
        assert_eq!(stats.total_repaired, 1);
    }

    #[test]
    fn test_pending_repairs() {
        let manager = QuarantineManager::new();

        // Recoverable
        let key1 = ShardKey::new("bucket", "key1", 0);
        manager.quarantine(QuarantinedBlock::new(
            key1,
            QuarantineReason::ChecksumMismatch,
        ));

        // Not recoverable
        let key2 = ShardKey::new("bucket", "key2", 0);
        manager.quarantine(QuarantinedBlock::new(key2, QuarantineReason::ReadError));

        let pending = manager.pending_repairs();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].reason, QuarantineReason::ChecksumMismatch);
    }

    #[test]
    fn test_reason_description() {
        assert!(QuarantineReason::ChecksumMismatch.is_recoverable());
        assert!(!QuarantineReason::ReadError.is_recoverable());
    }
}
