//! LZ4 compression

use crate::{Compressor, Error, Result};

/// LZ4 compressor
pub struct Lz4Compressor;

impl Lz4Compressor {
    /// Create a new LZ4 compressor
    pub fn new() -> Self {
        Self
    }
}

impl Default for Lz4Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compressor for Lz4Compressor {
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        Ok(lz4_flex::compress_prepend_size(input))
    }
    
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        lz4_flex::decompress_size_prepended(input)
            .map_err(|e| Error::Decompression(e.to_string()))
    }
    
    fn name(&self) -> &'static str {
        "lz4"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_roundtrip() {
        let compressor = Lz4Compressor::new();
        let data = b"hello world hello world hello world";
        
        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();
        
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }
}
