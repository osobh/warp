//! Reed-Solomon decoder implementation

use crate::{ErasureConfig, Error, Result};
use reed_solomon_simd::ReedSolomonDecoder;

/// Decoder for Reed-Solomon erasure coding
///
/// Reconstructs original data from available shards, even when some are missing.
pub struct ErasureDecoder {
    config: ErasureConfig,
}

impl ErasureDecoder {
    /// Create a new decoder with the given configuration
    pub fn new(config: ErasureConfig) -> Self {
        Self { config }
    }

    /// Decode shards back to original data
    ///
    /// # Arguments
    /// * `shards` - Vector of optional shards. Must have `total_shards` elements.
    ///   Use `None` for missing shards.
    ///
    /// # Returns
    /// The reconstructed original data.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Fewer than `data_shards` shards are available
    /// - Shard sizes don't match
    /// - Wrong number of shards provided
    pub fn decode(&self, shards: &[Option<Vec<u8>>]) -> Result<Vec<u8>> {
        let total = self.config.total_shards();
        let data_count = self.config.data_shards();

        // Validate shard count
        if shards.len() != total {
            return Err(Error::WrongShardCount {
                expected: total,
                actual: shards.len(),
            });
        }

        // Count available shards and determine shard size
        let mut shard_size = 0;
        let mut available_count = 0;

        for shard in shards.iter().flatten() {
            if shard_size == 0 {
                shard_size = shard.len();
            } else if shard.len() != shard_size {
                return Err(Error::ShardSizeMismatch {
                    expected: shard_size,
                    actual: shard.len(),
                });
            }
            available_count += 1;
        }

        if shard_size == 0 {
            return Err(Error::TooManyMissing {
                needed: data_count,
                available: 0,
            });
        }

        // Check if we have enough shards
        if available_count < data_count {
            return Err(Error::TooManyMissing {
                needed: data_count,
                available: available_count,
            });
        }

        // If all data shards are present, just concatenate them
        let all_data_present = shards[..data_count].iter().all(|s| s.is_some());
        if all_data_present {
            let mut result = Vec::with_capacity(shard_size * data_count);
            for shard in &shards[..data_count] {
                result.extend_from_slice(shard.as_ref().unwrap());
            }
            return Ok(result);
        }

        // Need to reconstruct - use the decoder
        let mut decoder = ReedSolomonDecoder::new(
            data_count,
            self.config.parity_shards(),
            shard_size,
        )
        .map_err(|e| Error::EncodingError(format!("Failed to create decoder: {}", e)))?;

        // Add original shards (data shards that are present)
        for (i, shard) in shards[..data_count].iter().enumerate() {
            if let Some(data) = shard {
                decoder
                    .add_original_shard(i, data)
                    .map_err(|e| Error::EncodingError(format!("Failed to add original shard: {}", e)))?;
            }
        }

        // Add recovery shards (parity shards that are present)
        for (i, shard) in shards[data_count..].iter().enumerate() {
            if let Some(data) = shard {
                decoder
                    .add_recovery_shard(i, data)
                    .map_err(|e| Error::EncodingError(format!("Failed to add recovery shard: {}", e)))?;
            }
        }

        // Decode
        let restored = decoder
            .decode()
            .map_err(|e| Error::EncodingError(format!("Decoding failed: {}", e)))?;

        // Reconstruct full data
        let mut result = vec![0u8; shard_size * data_count];

        // Copy present original shards
        for (i, shard) in shards[..data_count].iter().enumerate() {
            if let Some(data) = shard {
                let start = i * shard_size;
                result[start..start + shard_size].copy_from_slice(data);
            }
        }

        // Copy restored shards
        for (index, data) in restored.restored_original_iter() {
            let start = index * shard_size;
            result[start..start + shard_size].copy_from_slice(data);
        }

        Ok(result)
    }

    /// Decode with original data length (removes padding)
    ///
    /// # Arguments
    /// * `shards` - Vector of optional shards
    /// * `original_len` - Original data length before encoding
    ///
    /// # Returns
    /// The reconstructed data truncated to original length
    pub fn decode_exact(&self, shards: &[Option<Vec<u8>>], original_len: usize) -> Result<Vec<u8>> {
        let mut data = self.decode(shards)?;
        data.truncate(original_len);
        Ok(data)
    }

    /// Get the configuration
    pub fn config(&self) -> &ErasureConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErasureEncoder;

    fn encode_and_decode(
        config: ErasureConfig,
        data: &[u8],
        missing_indices: &[usize],
    ) -> Result<Vec<u8>> {
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config);

        let shards = encoder.encode(data)?;
        let mut received: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

        for &idx in missing_indices {
            received[idx] = None;
        }

        decoder.decode(&received)
    }

    #[test]
    fn test_decode_no_loss() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let data: Vec<u8> = (0..=255).collect();

        let recovered = encode_and_decode(config, &data, &[]).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_decode_one_data_loss() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let data: Vec<u8> = (0..=255).collect();

        let recovered = encode_and_decode(config, &data, &[0]).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_decode_one_parity_loss() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let data: Vec<u8> = (0..=255).collect();

        let recovered = encode_and_decode(config, &data, &[4]).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_decode_max_loss() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let data: Vec<u8> = (0..=255).collect();

        // Lose 2 shards (maximum for 2 parity)
        let recovered = encode_and_decode(config, &data, &[1, 5]).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_decode_too_many_missing() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let data: Vec<u8> = (0..=255).collect();

        // Lose 3 shards (more than 2 parity)
        let result = encode_and_decode(config, &data, &[0, 1, 2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_wrong_shard_count() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let decoder = ErasureDecoder::new(config);

        let shards: Vec<Option<Vec<u8>>> = vec![Some(vec![1, 2, 3])]; // Only 1 shard

        let result = decoder.decode(&shards);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_exact() {
        let config = ErasureConfig::new(4, 2).unwrap();
        let encoder = ErasureEncoder::new(config.clone());
        let decoder = ErasureDecoder::new(config);

        // 100 bytes - will be padded to 104
        let data: Vec<u8> = (0..100).collect();
        let shards = encoder.encode(&data).unwrap();

        let received: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        let recovered = decoder.decode_exact(&received, 100).unwrap();

        assert_eq!(recovered, data);
    }

    #[test]
    fn test_decode_various_losses() {
        let config = ErasureConfig::new(10, 4).unwrap();
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

        // Test various combinations of 4 missing shards
        let loss_patterns = vec![
            vec![0, 1, 2, 3],     // All first 4 data shards
            vec![10, 11, 12, 13], // All parity shards
            vec![0, 5, 10, 12],   // Mixed
            vec![9, 10, 11, 13],  // Last data + parity
        ];

        for pattern in loss_patterns {
            let recovered = encode_and_decode(config.clone(), &data, &pattern).unwrap();
            assert_eq!(
                recovered, data,
                "Failed with loss pattern: {:?}",
                pattern
            );
        }
    }
}
