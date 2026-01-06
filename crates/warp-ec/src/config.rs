//! Erasure coding configuration

use crate::{Error, Result};

/// Configuration for Reed-Solomon erasure coding
///
/// Specifies the number of data shards and parity shards.
/// With `data_shards` = k and `parity_shards` = m:
/// - Original data is split into k shards
/// - m additional parity shards are generated
/// - Data can be recovered from any k of the k+m shards
#[derive(Debug, Clone)]
pub struct ErasureConfig {
    /// Number of data shards (k)
    data_shards: usize,
    /// Number of parity/recovery shards (m)
    parity_shards: usize,
}

impl ErasureConfig {
    /// Create a new erasure coding configuration
    ///
    /// # Arguments
    /// * `data_shards` - Number of data shards (k), must be >= 1
    /// * `parity_shards` - Number of parity shards (m), must be >= 1
    ///
    /// # Constraints
    /// * data_shards + parity_shards <= 32768 (reed-solomon-simd limit)
    ///
    /// # Example
    /// ```
    /// use warp_ec::ErasureConfig;
    ///
    /// // RS(10,4): 10 data shards, 4 parity shards
    /// let config = ErasureConfig::new(10, 4).unwrap();
    /// ```
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        if data_shards == 0 {
            return Err(Error::InvalidConfig(
                "data_shards must be at least 1".into(),
            ));
        }

        if parity_shards == 0 {
            return Err(Error::InvalidConfig(
                "parity_shards must be at least 1".into(),
            ));
        }

        let total = data_shards + parity_shards;
        if total > 32768 {
            return Err(Error::InvalidConfig(format!(
                "total shards ({}) exceeds maximum of 32768",
                total
            )));
        }

        Ok(Self {
            data_shards,
            parity_shards,
        })
    }

    /// Create RS(4,2) configuration - 50% overhead, tolerates 2 failures
    pub fn rs_4_2() -> Self {
        Self {
            data_shards: 4,
            parity_shards: 2,
        }
    }

    /// Create RS(6,3) configuration - 50% overhead, tolerates 3 failures
    pub fn rs_6_3() -> Self {
        Self {
            data_shards: 6,
            parity_shards: 3,
        }
    }

    /// Create RS(10,4) configuration - 40% overhead, tolerates 4 failures
    pub fn rs_10_4() -> Self {
        Self {
            data_shards: 10,
            parity_shards: 4,
        }
    }

    /// Create RS(16,4) configuration - 25% overhead, tolerates 4 failures
    pub fn rs_16_4() -> Self {
        Self {
            data_shards: 16,
            parity_shards: 4,
        }
    }

    /// Get the number of data shards
    pub fn data_shards(&self) -> usize {
        self.data_shards
    }

    /// Get the number of parity shards
    pub fn parity_shards(&self) -> usize {
        self.parity_shards
    }

    /// Get the total number of shards (data + parity)
    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    /// Get the storage overhead as a ratio (parity / data)
    pub fn overhead_ratio(&self) -> f64 {
        self.parity_shards as f64 / self.data_shards as f64
    }

    /// Get the maximum number of shard failures that can be tolerated
    pub fn fault_tolerance(&self) -> usize {
        self.parity_shards
    }

    /// Calculate the required shard size for a given data size
    ///
    /// Each shard will be `ceil(data_size / data_shards)` bytes, rounded up
    /// to the next even number (reed-solomon-simd requirement).
    /// The data may be padded to make it evenly divisible.
    pub fn shard_size_for_data(&self, data_size: usize) -> usize {
        let raw_size = data_size.div_ceil(self.data_shards);
        // Round up to next even number (reed-solomon-simd requires even shard sizes)
        (raw_size + 1) & !1
    }

    /// Calculate the padded data size for even shard division
    pub fn padded_data_size(&self, data_size: usize) -> usize {
        let shard_size = self.shard_size_for_data(data_size);
        shard_size * self.data_shards
    }
}

impl Default for ErasureConfig {
    /// Default configuration: RS(10,4)
    fn default() -> Self {
        Self::rs_10_4()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = ErasureConfig::new(10, 4).unwrap();
        assert_eq!(config.data_shards(), 10);
        assert_eq!(config.parity_shards(), 4);
        assert_eq!(config.total_shards(), 14);
    }

    #[test]
    fn test_config_invalid() {
        assert!(ErasureConfig::new(0, 4).is_err());
        assert!(ErasureConfig::new(10, 0).is_err());
        assert!(ErasureConfig::new(20000, 20000).is_err());
    }

    #[test]
    fn test_preset_configs() {
        let c1 = ErasureConfig::rs_4_2();
        assert_eq!(c1.total_shards(), 6);
        assert_eq!(c1.fault_tolerance(), 2);

        let c2 = ErasureConfig::rs_10_4();
        assert_eq!(c2.total_shards(), 14);
        assert_eq!(c2.fault_tolerance(), 4);
    }

    #[test]
    fn test_overhead_ratio() {
        let config = ErasureConfig::rs_10_4();
        assert!((config.overhead_ratio() - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_shard_size() {
        let config = ErasureConfig::new(10, 4).unwrap();

        // Exact division (100 is already even)
        assert_eq!(config.shard_size_for_data(1000), 100);

        // Needs padding (101 rounded up to 102)
        assert_eq!(config.shard_size_for_data(1001), 102);
        assert_eq!(config.padded_data_size(1001), 1020);
    }
}
