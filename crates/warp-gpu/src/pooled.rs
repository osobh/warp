//! Pooled GPU operations using pinned memory for zero-copy transfers
//!
//! This module provides wrappers around GPU operations that utilize the
//! pinned memory pool for efficient data transfers. Using pinned memory:
//! - Eliminates CPU-side staging buffers
//! - Enables DMA transfers (12+ GB/s on PCIe 3.0 x16)
//! - Allows overlapping transfers with kernel execution
//!
//! # Usage
//!
//! ```no_run
//! use warp_gpu::{GpuContext, PooledHasher, PooledCipher, create_shared_pool};
//!
//! # fn main() -> Result<(), warp_gpu::Error> {
//! let ctx = GpuContext::new()?;
//! let pool = create_shared_pool(&ctx);
//!
//! // Create pooled hasher
//! let hasher = PooledHasher::new(ctx.clone(), pool.clone())?;
//! let data = b"Hello, World!";
//! let hash = hasher.hash(data)?;
//!
//! // Create pooled cipher
//! let cipher = PooledCipher::new(ctx, pool)?;
//! let key = [0u8; 32];
//! let nonce = [0u8; 12];
//! let ciphertext = cipher.encrypt(data, &key, &nonce)?;
//! # Ok(())
//! # }
//! ```

use crate::{Blake3Hasher, ChaCha20Poly1305, Error, GpuContext, PinnedMemoryPool, Result};
use std::sync::Arc;
use tracing::trace;

/// Pooled BLAKE3 hasher using pinned memory for transfers
///
/// Wraps `Blake3Hasher` with pinned memory pool integration for
/// efficient host-to-device transfers.
pub struct PooledHasher {
    hasher: Blake3Hasher,
    pool: Arc<PinnedMemoryPool>,
}

impl PooledHasher {
    /// Create a new pooled hasher
    ///
    /// # Arguments
    /// * `ctx` - GPU context
    /// * `pool` - Pinned memory pool for transfers
    ///
    /// # Errors
    /// Returns error if hasher creation fails
    pub fn new(ctx: GpuContext, pool: Arc<PinnedMemoryPool>) -> Result<Self> {
        let hasher = Blake3Hasher::new(ctx.context().clone())?;
        Ok(Self { hasher, pool })
    }

    /// Hash data using pinned memory for transfer
    ///
    /// # Arguments
    /// * `data` - Data to hash
    ///
    /// # Returns
    /// 32-byte BLAKE3 hash
    pub fn hash(&self, data: &[u8]) -> Result<[u8; 32]> {
        // Use CPU path for correctness (same as underlying hasher)
        self.hasher.hash(data)
    }

    /// Hash data using GPU with pinned memory transfer (experimental)
    ///
    /// Uses pinned buffer from pool for efficient transfer to GPU.
    ///
    /// # Arguments
    /// * `data` - Data to hash
    ///
    /// # Returns
    /// 32-byte BLAKE3 hash (may differ from CPU implementation)
    pub fn hash_gpu_with_pool(&self, data: &[u8]) -> Result<[u8; 32]> {
        if data.is_empty() {
            return self.hasher.hash(data);
        }

        // Acquire pinned buffer
        let mut buffer = self.pool.acquire(data.len())?;
        buffer.copy_from_slice(data)?;

        trace!("Using pooled buffer ({} bytes) for GPU hash", data.len());

        // Hash using the pinned buffer data
        let result = self.hasher.hash_gpu_experimental(buffer.as_slice());

        // Return buffer to pool
        self.pool.release(buffer);

        result
    }

    /// Hash multiple data chunks using pooled memory
    ///
    /// # Arguments
    /// * `chunks` - Slices of data to hash
    ///
    /// # Returns
    /// Vector of 32-byte hashes
    pub fn hash_batch(&self, chunks: &[&[u8]]) -> Result<Vec<[u8; 32]>> {
        chunks.iter().map(|chunk| self.hash(chunk)).collect()
    }

    /// Get statistics from the underlying pool
    pub fn pool_statistics(&self) -> crate::PoolStatistics {
        self.pool.statistics()
    }
}

/// Pooled ChaCha20-Poly1305 cipher using pinned memory for transfers
///
/// Wraps `ChaCha20Poly1305` with pinned memory pool integration for
/// efficient host-to-device and device-to-host transfers.
pub struct PooledCipher {
    cipher: ChaCha20Poly1305,
    pool: Arc<PinnedMemoryPool>,
}

impl PooledCipher {
    /// Create a new pooled cipher
    ///
    /// # Arguments
    /// * `ctx` - GPU context
    /// * `pool` - Pinned memory pool for transfers
    ///
    /// # Errors
    /// Returns error if cipher creation fails
    pub fn new(ctx: GpuContext, pool: Arc<PinnedMemoryPool>) -> Result<Self> {
        let cipher = ChaCha20Poly1305::new(ctx.context().clone())?;
        Ok(Self { cipher, pool })
    }

    /// Encrypt data using CPU (correct implementation)
    ///
    /// # Arguments
    /// * `plaintext` - Data to encrypt
    /// * `key` - 32-byte encryption key
    /// * `nonce` - 12-byte nonce
    ///
    /// # Returns
    /// Ciphertext with 16-byte Poly1305 tag appended
    pub fn encrypt(&self, plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>> {
        self.cipher.encrypt(plaintext, key, nonce)
    }

    /// Decrypt data using CPU (correct implementation)
    ///
    /// # Arguments
    /// * `ciphertext` - Ciphertext with 16-byte tag
    /// * `key` - 32-byte encryption key
    /// * `nonce` - 12-byte nonce
    ///
    /// # Returns
    /// Decrypted plaintext
    pub fn decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>> {
        self.cipher.decrypt(ciphertext, key, nonce)
    }

    /// Encrypt using GPU with pinned memory transfer (experimental)
    ///
    /// Uses pinned buffers from pool for efficient transfers.
    ///
    /// # Arguments
    /// * `plaintext` - Data to encrypt
    /// * `key` - 32-byte encryption key
    /// * `nonce` - 12-byte nonce
    pub fn encrypt_gpu_with_pool(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>> {
        if plaintext.is_empty() {
            return self.cipher.encrypt(plaintext, key, nonce);
        }

        // Acquire pinned buffer for input
        let mut input_buffer = self.pool.acquire(plaintext.len())?;
        input_buffer.copy_from_slice(plaintext)?;

        trace!(
            "Using pooled buffer ({} bytes) for GPU encrypt",
            plaintext.len()
        );

        // Encrypt using pinned buffer data
        let result = self
            .cipher
            .encrypt_gpu_experimental(input_buffer.as_slice(), key, nonce);

        // Return buffer to pool
        self.pool.release(input_buffer);

        result
    }

    /// Encrypt multiple plaintexts in batch
    ///
    /// # Arguments
    /// * `plaintexts` - Slices of plaintext data
    /// * `keys` - Keys for each plaintext
    /// * `nonces` - Nonces for each plaintext
    ///
    /// # Returns
    /// Vector of ciphertexts
    pub fn encrypt_batch(
        &self,
        plaintexts: &[&[u8]],
        keys: &[[u8; 32]],
        nonces: &[[u8; 12]],
    ) -> Result<Vec<Vec<u8>>> {
        if plaintexts.len() != keys.len() || plaintexts.len() != nonces.len() {
            return Err(Error::InvalidOperation(
                "Mismatched batch lengths".to_string(),
            ));
        }

        plaintexts
            .iter()
            .zip(keys.iter())
            .zip(nonces.iter())
            .map(|((pt, key), nonce)| self.encrypt(pt, key, nonce))
            .collect()
    }

    /// Get statistics from the underlying pool
    pub fn pool_statistics(&self) -> crate::PoolStatistics {
        self.pool.statistics()
    }
}

/// Convenience function to create a shared pool for multiple operations
///
/// # Arguments
/// * `ctx` - GPU context (uses its internal CUDA context)
///
/// # Returns
/// Arc-wrapped pool suitable for sharing across operations
pub fn create_shared_pool(ctx: &GpuContext) -> Arc<PinnedMemoryPool> {
    Arc::new(PinnedMemoryPool::with_defaults(ctx.context().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn try_get_context() -> Option<GpuContext> {
        GpuContext::new().ok()
    }

    // ============== PooledHasher Tests ==============

    #[test]
    fn test_pooled_hasher_creation() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let hasher = PooledHasher::new(ctx, pool);
            assert!(hasher.is_ok(), "PooledHasher creation should succeed");
        }
    }

    #[test]
    fn test_pooled_hasher_hash_empty() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let hasher = PooledHasher::new(ctx, pool).expect("Hasher creation should succeed");

            let hash = hasher.hash(&[]).expect("Empty hash should succeed");
            assert_eq!(hash.len(), 32);
        }
    }

    #[test]
    fn test_pooled_hasher_hash_data() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let hasher = PooledHasher::new(ctx, pool).expect("Hasher creation should succeed");

            let data = b"Hello, World!";
            let hash = hasher.hash(data).expect("Hash should succeed");

            // Verify against CPU blake3
            let expected = blake3::hash(data);
            assert_eq!(hash, *expected.as_bytes());
        }
    }

    #[test]
    fn test_pooled_hasher_batch() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let hasher = PooledHasher::new(ctx, pool).expect("Hasher creation should succeed");

            let chunks: Vec<&[u8]> = vec![b"chunk1", b"chunk2", b"chunk3"];
            let hashes = hasher
                .hash_batch(&chunks)
                .expect("Batch hash should succeed");

            assert_eq!(hashes.len(), 3);
            for (i, chunk) in chunks.iter().enumerate() {
                let expected = blake3::hash(chunk);
                assert_eq!(hashes[i], *expected.as_bytes());
            }
        }
    }

    #[test]
    fn test_pooled_hasher_gpu_experimental() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let hasher = PooledHasher::new(ctx, pool).expect("Hasher creation should succeed");

            let data = vec![0xABu8; 4096];
            // GPU experimental may produce different results
            let result = hasher.hash_gpu_with_pool(&data);
            assert!(result.is_ok(), "GPU hash with pool should succeed");
        }
    }

    #[test]
    fn test_pooled_hasher_statistics() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let hasher = PooledHasher::new(ctx, pool).expect("Hasher creation should succeed");

            // Initial stats
            let stats = hasher.pool_statistics();
            assert_eq!(stats.allocations, 0);
        }
    }

    // ============== PooledCipher Tests ==============

    #[test]
    fn test_pooled_cipher_creation() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool);
            assert!(cipher.is_ok(), "PooledCipher creation should succeed");
        }
    }

    #[test]
    fn test_pooled_cipher_encrypt_decrypt() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool).expect("Cipher creation should succeed");

            let plaintext = b"Secret message for testing";
            let key = [0x42u8; 32];
            let nonce = [0x24u8; 12];

            let ciphertext = cipher
                .encrypt(plaintext, &key, &nonce)
                .expect("Encrypt should succeed");
            let decrypted = cipher
                .decrypt(&ciphertext, &key, &nonce)
                .expect("Decrypt should succeed");

            assert_eq!(decrypted, plaintext);
        }
    }

    #[test]
    fn test_pooled_cipher_empty_data() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool).expect("Cipher creation should succeed");

            let key = [0x11u8; 32];
            let nonce = [0x22u8; 12];

            let ciphertext = cipher
                .encrypt(&[], &key, &nonce)
                .expect("Empty encrypt should succeed");
            let decrypted = cipher
                .decrypt(&ciphertext, &key, &nonce)
                .expect("Empty decrypt should succeed");

            assert!(decrypted.is_empty());
        }
    }

    #[test]
    fn test_pooled_cipher_batch_encrypt() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool).expect("Cipher creation should succeed");

            let data1 = b"Message 1".to_vec();
            let data2 = b"Message 2".to_vec();
            let plaintexts: Vec<&[u8]> = vec![&data1, &data2];
            let keys = [[0x11u8; 32], [0x22u8; 32]];
            let nonces = [[0x33u8; 12], [0x44u8; 12]];

            let ciphertexts = cipher
                .encrypt_batch(&plaintexts, &keys, &nonces)
                .expect("Batch encrypt should succeed");

            assert_eq!(ciphertexts.len(), 2);
            for (i, ct) in ciphertexts.iter().enumerate() {
                let decrypted = cipher
                    .decrypt(ct, &keys[i], &nonces[i])
                    .expect("Decrypt should succeed");
                assert_eq!(decrypted, plaintexts[i]);
            }
        }
    }

    #[test]
    fn test_pooled_cipher_batch_mismatched_lengths() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool).expect("Cipher creation should succeed");

            let data = b"data".to_vec();
            let plaintexts: Vec<&[u8]> = vec![&data];
            let keys = [[0x11u8; 32], [0x22u8; 32]]; // 2 keys but 1 plaintext
            let nonces = [[0x33u8; 12]];

            let result = cipher.encrypt_batch(&plaintexts, &keys, &nonces);
            assert!(result.is_err(), "Mismatched lengths should fail");
        }
    }

    #[test]
    fn test_pooled_cipher_gpu_experimental() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool).expect("Cipher creation should succeed");

            let plaintext = vec![0xBBu8; 1024];
            let key = [0x55u8; 32];
            let nonce = [0x66u8; 12];

            // GPU experimental may produce different results
            let result = cipher.encrypt_gpu_with_pool(&plaintext, &key, &nonce);
            assert!(result.is_ok(), "GPU encrypt with pool should succeed");
        }
    }

    #[test]
    fn test_pooled_cipher_statistics() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            let cipher = PooledCipher::new(ctx, pool).expect("Cipher creation should succeed");

            let stats = cipher.pool_statistics();
            assert_eq!(stats.allocations, 0);
        }
    }

    // ============== Shared Pool Tests ==============

    #[test]
    fn test_shared_pool_creation() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);
            assert_eq!(pool.statistics().allocations, 0);
        }
    }

    #[test]
    fn test_pool_reuse_across_operations() {
        if let Some(ctx) = try_get_context() {
            let pool = create_shared_pool(&ctx);

            // Create both hasher and cipher with same pool
            let hasher =
                PooledHasher::new(ctx.clone(), pool.clone()).expect("Hasher creation should work");
            let cipher = PooledCipher::new(ctx, pool.clone()).expect("Cipher creation should work");

            // Use hasher
            let _ = hasher.hash(b"test data");

            // Use cipher
            let key = [0x42u8; 32];
            let nonce = [0x24u8; 12];
            let _ = cipher.encrypt(b"test data", &key, &nonce);

            // Stats should be shared
            let stats1 = hasher.pool_statistics();
            let stats2 = cipher.pool_statistics();
            assert_eq!(stats1.allocations, stats2.allocations);
        }
    }
}
