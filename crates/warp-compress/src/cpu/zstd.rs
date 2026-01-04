//! Zstandard compression

use crate::{Compressor, Error, Result};

/// Zstd compressor
pub struct ZstdCompressor {
    level: i32,
}

impl ZstdCompressor {
    /// Create a new Zstd compressor
    ///
    /// Level range: 1-22 (default: 3)
    pub fn new(level: i32) -> Result<Self> {
        if !(1..=22).contains(&level) {
            return Err(Error::InvalidLevel(level));
        }
        Ok(Self { level })
    }
}

impl Default for ZstdCompressor {
    fn default() -> Self {
        Self { level: 3 }
    }
}

impl Compressor for ZstdCompressor {
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        zstd::bulk::compress(input, self.level).map_err(|e| Error::Compression(e.to_string()))
    }

    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        zstd::bulk::decompress(input, 1024 * 1024 * 64) // 64MB max
            .map_err(|e| Error::Decompression(e.to_string()))
    }

    fn name(&self) -> &'static str {
        "zstd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let compressor = ZstdCompressor::default();
        let data = b"hello world hello world hello world";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(data.as_slice(), decompressed.as_slice());
        assert!(compressed.len() < data.len());
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: Zstd compress/decompress roundtrip
        #[test]
        fn zstd_roundtrip(data in prop::collection::vec(any::<u8>(), 1..8192)) {
            let compressor = ZstdCompressor::default();

            let compressed = compressor.compress(&data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            prop_assert_eq!(data, decompressed);
        }

        /// Property: Zstd with varying compression levels
        #[test]
        fn zstd_roundtrip_levels(
            data in prop::collection::vec(any::<u8>(), 1..4096),
            level in 1i32..=22,
        ) {
            let compressor = ZstdCompressor::new(level).unwrap();

            let compressed = compressor.compress(&data).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();

            prop_assert_eq!(data, decompressed);
        }
    }
}
