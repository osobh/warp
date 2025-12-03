//! warp-hash: BLAKE3 hashing and Merkle tree
//!
//! High-performance parallel hashing using BLAKE3 with rayon.

#![warn(missing_docs)]

use rayon::prelude::*;

/// Hash output (32 bytes)
pub type Hash = [u8; 32];

/// Hash a single chunk of data
pub fn hash(data: &[u8]) -> Hash {
    *blake3::hash(data).as_bytes()
}

/// Hash multiple chunks in parallel
pub fn hash_chunks_parallel(chunks: &[&[u8]]) -> Vec<Hash> {
    chunks.par_iter().map(|chunk| hash(chunk)).collect()
}

/// Keyed hash (for MAC)
pub fn keyed_hash(key: &[u8; 32], data: &[u8]) -> Hash {
    *blake3::keyed_hash(key, data).as_bytes()
}

/// Derive a key from context and key material
pub fn derive_key(context: &str, key_material: &[u8]) -> Hash {
    blake3::derive_key(context, key_material)
}

/// Incremental hasher
pub struct Hasher {
    inner: blake3::Hasher,
}

impl Hasher {
    /// Create a new hasher
    pub fn new() -> Self {
        Self {
            inner: blake3::Hasher::new(),
        }
    }
    
    /// Create a keyed hasher
    pub fn new_keyed(key: &[u8; 32]) -> Self {
        Self {
            inner: blake3::Hasher::new_keyed(key),
        }
    }
    
    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }
    
    /// Finalize and return hash
    pub fn finalize(&self) -> Hash {
        *self.inner.finalize().as_bytes()
    }
    
    /// Reset hasher for reuse
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hash() {
        let hash1 = hash(b"hello");
        let hash2 = hash(b"hello");
        let hash3 = hash(b"world");
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_incremental() {
        let direct = hash(b"helloworld");
        
        let mut hasher = Hasher::new();
        hasher.update(b"hello");
        hasher.update(b"world");
        let incremental = hasher.finalize();
        
        assert_eq!(direct, incremental);
    }
}
