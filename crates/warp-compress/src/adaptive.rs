//! Adaptive compression selection

use crate::{Compressor, Lz4Compressor, ZstdCompressor};

/// Entropy threshold for compression selection
pub const ENTROPY_THRESHOLD_HIGH: f64 = 0.95;
/// Entropy threshold for highly compressible data
pub const ENTROPY_THRESHOLD_LOW: f64 = 0.3;

/// Compression strategy based on data characteristics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// No compression (already compressed data)
    None,
    /// Fast compression (LZ4)
    Fast,
    /// Balanced compression (Zstd level 3)
    Balanced,
    /// Maximum compression (Zstd level 19)
    Maximum,
}

impl Strategy {
    /// Select strategy based on entropy
    pub fn from_entropy(entropy: f64) -> Self {
        if entropy > ENTROPY_THRESHOLD_HIGH {
            Self::None
        } else if entropy > 0.7 {
            Self::Fast
        } else if entropy > ENTROPY_THRESHOLD_LOW {
            Self::Balanced
        } else {
            Self::Maximum
        }
    }
    
    /// Create compressor for this strategy
    pub fn compressor(&self) -> Option<Box<dyn Compressor>> {
        match self {
            Self::None => None,
            Self::Fast => Some(Box::new(Lz4Compressor::new())),
            Self::Balanced => Some(Box::new(ZstdCompressor::new(3).unwrap())),
            Self::Maximum => Some(Box::new(ZstdCompressor::new(19).unwrap())),
        }
    }
}

/// Calculate entropy of data (0.0 = compressible, 1.0 = random)
pub fn calculate_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    
    let mut freq = [0u64; 256];
    for &byte in data {
        freq[byte as usize] += 1;
    }
    
    let len = data.len() as f64;
    let mut entropy = 0.0;
    
    for &count in &freq {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }
    
    entropy / 8.0 // Normalize to 0-1
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_entropy_zeros() {
        let data = vec![0u8; 1000];
        let entropy = calculate_entropy(&data);
        assert!(entropy < 0.1);
    }
    
    #[test]
    fn test_entropy_random() {
        let data: Vec<u8> = (0..=255).cycle().take(1000).collect();
        let entropy = calculate_entropy(&data);
        assert!(entropy > 0.9);
    }
}
