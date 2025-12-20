//! Reed-Solomon encoder implementation

use crate::{ErasureConfig, Error, Result};
use reed_solomon_simd::ReedSolomonEncoder;

/// Encoder for Reed-Solomon erasure coding
///
/// Takes original data and produces data shards plus parity shards.
pub struct ErasureEncoder {
    config: ErasureConfig,
}

impl ErasureEncoder {
    /// Create a new encoder with the given configuration
    pub fn new(config: ErasureConfig) -> Self {
        Self { config }
    }

    /// Encode data into shards
    ///
    /// # Arguments
    /// * `data` - Original data to encode
    ///
    /// # Returns
    /// A vector of shards: first `data_shards` are data, remaining are parity.
    ///
    /// # Note
    /// If data length is not evenly divisible by `data_shards`, it will be
    /// padded with zeros. The caller should track the original data length.
    pub fn encode(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        if data.is_empty() {
            return Err(Error::InvalidDataSize("Data cannot be empty".into()));
        }

        let shard_size = self.config.shard_size_for_data(data.len());
        let padded_size = self.config.padded_data_size(data.len());

        // Pad data if necessary
        let padded_data = if data.len() == padded_size {
            data.to_vec()
        } else {
            let mut padded = data.to_vec();
            padded.resize(padded_size, 0);
            padded
        };

        // Create the encoder
        let mut encoder = ReedSolomonEncoder::new(
            self.config.data_shards(),
            self.config.parity_shards(),
            shard_size,
        )
        .map_err(|e| Error::EncodingError(format!("Failed to create encoder: {}", e)))?;

        // Add original shards
        for chunk in padded_data.chunks(shard_size) {
            encoder
                .add_original_shard(chunk)
                .map_err(|e| Error::EncodingError(format!("Failed to add shard: {}", e)))?;
        }

        // Encode and get recovery shards
        let result = encoder
            .encode()
            .map_err(|e| Error::EncodingError(format!("Encoding failed: {}", e)))?;

        // Collect original shards (data shards)
        let mut shards: Vec<Vec<u8>> = padded_data
            .chunks(shard_size)
            .map(|s| s.to_vec())
            .collect();

        // Add recovery shards (parity shards)
        for recovery in result.recovery_iter() {
            shards.push(recovery.to_vec());
        }

        Ok(shards)
    }

    /// Encode data and return shards with metadata
    ///
    /// This is a higher-level API that includes shard type information.
    pub fn encode_with_metadata(&self, data: &[u8]) -> Result<Vec<crate::Shard>> {
        let shards = self.encode(data)?;
        let data_count = self.config.data_shards();

        Ok(shards
            .into_iter()
            .enumerate()
            .map(|(i, data)| {
                if i < data_count {
                    crate::Shard::data(i as u16, data)
                } else {
                    crate::Shard::parity((i - data_count) as u16, data)
                }
            })
            .collect())
    }

    /// Get the configuration
    pub fn config(&self) -> &ErasureConfig {
        &self.config
    }

    /// Calculate the total encoded size for given data
    pub fn encoded_size(&self, data_len: usize) -> usize {
        let shard_size = self.config.shard_size_for_data(data_len);
        shard_size * self.config.total_shards()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_basic() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let encoder = ErasureEncoder::new(config);

        let data: Vec<u8> = (0..256).map(|i| i as u8).collect();
        let shards = encoder.encode(&data).unwrap();

        assert_eq!(shards.len(), 6); // 4 data + 2 parity
        assert_eq!(shards[0].len(), 64); // 256 / 4 = 64 bytes per shard
    }

    #[test]
    fn test_encode_with_padding() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let encoder = ErasureEncoder::new(config);

        // 100 bytes, not divisible by 4
        let data: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let shards = encoder.encode(&data).unwrap();

        assert_eq!(shards.len(), 6);
        // Shard size should be ceil(100/4) = 25, rounded up to 26 (even)
        assert_eq!(shards[0].len(), 26);
    }

    #[test]
    fn test_encode_empty_fails() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let encoder = ErasureEncoder::new(config);

        let result = encoder.encode(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_with_metadata() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let encoder = ErasureEncoder::new(config);

        let data: Vec<u8> = (0..=255).collect();
        let shards = encoder.encode_with_metadata(&data).unwrap();

        assert_eq!(shards.len(), 6);

        // First 4 should be data shards
        for i in 0..4 {
            assert!(shards[i].is_data());
            assert_eq!(shards[i].id.index, i as u16);
        }

        // Last 2 should be parity shards
        for i in 0..2 {
            assert!(shards[4 + i].is_parity());
            assert_eq!(shards[4 + i].id.index, i as u16);
        }
    }

    #[test]
    fn test_encoded_size() {
        let config = ErasureConfig::new(10, 4).unwrap();
        let encoder = ErasureEncoder::new(config);

        // 1000 bytes -> 100 bytes per shard -> 14 shards = 1400 bytes
        assert_eq!(encoder.encoded_size(1000), 1400);

        // 1001 bytes -> 102 bytes per shard (even) -> 14 shards = 1428 bytes
        assert_eq!(encoder.encoded_size(1001), 1428);
    }
}
