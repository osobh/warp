//! Streaming encryption for real-time data processing
//!
//! This module provides stateful encryption that maintains context across
//! multiple chunks, enabling efficient streaming without per-chunk key
//! setup overhead.
//!
//! # Design
//!
//! - Counter-based nonce derivation prevents nonce reuse
//! - Key is reused across chunks (no per-chunk KDF)
//! - Each chunk is independently authenticated (AEAD)
//! - Supports both encryption and decryption streams
//!
//! # Example
//!
//! ```
//! use warp_crypto::stream::StreamCipher;
//!
//! let key = [0u8; 32];
//! let base_nonce = [0u8; 12];
//! let mut cipher = StreamCipher::new(&key, &base_nonce);
//!
//! // Encrypt multiple chunks
//! let chunk1 = cipher.encrypt_chunk(b"Hello, ").unwrap();
//! let chunk2 = cipher.encrypt_chunk(b"World!").unwrap();
//!
//! // Decrypt in order
//! let mut decipher = StreamCipher::new(&key, &base_nonce);
//! let plain1 = decipher.decrypt_chunk(&chunk1).unwrap();
//! let plain2 = decipher.decrypt_chunk(&chunk2).unwrap();
//!
//! assert_eq!(plain1, b"Hello, ");
//! assert_eq!(plain2, b"World!");
//! ```

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305,
};
use chacha20poly1305::aead::generic_array::GenericArray;

use crate::{Error, Result};

/// Streaming cipher for encrypting/decrypting data in chunks
///
/// Maintains internal state (counter) to ensure unique nonces
/// across all chunks in a stream.
pub struct StreamCipher {
    cipher: ChaCha20Poly1305,
    base_nonce: [u8; 12],
    counter: u64,
}

impl StreamCipher {
    /// Create a new streaming cipher
    ///
    /// # Arguments
    /// * `key` - 32-byte encryption key
    /// * `base_nonce` - 12-byte base nonce (counter will be XORed with last 8 bytes)
    pub fn new(key: &[u8; 32], base_nonce: &[u8; 12]) -> Self {
        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(key));
        Self {
            cipher,
            base_nonce: *base_nonce,
            counter: 0,
        }
    }

    /// Get the current chunk counter
    pub fn counter(&self) -> u64 {
        self.counter
    }

    /// Derive nonce for current counter value
    ///
    /// The nonce is derived by XORing the counter (as little-endian bytes)
    /// with the last 8 bytes of the base nonce.
    fn derive_nonce(&self) -> [u8; 12] {
        let mut nonce = self.base_nonce;
        let counter_bytes = self.counter.to_le_bytes();

        // XOR counter with last 8 bytes of nonce
        for i in 0..8 {
            nonce[4 + i] ^= counter_bytes[i];
        }

        nonce
    }

    /// Encrypt a chunk and advance the counter
    ///
    /// # Arguments
    /// * `plaintext` - Data to encrypt
    ///
    /// # Returns
    /// Ciphertext with 16-byte Poly1305 tag appended
    pub fn encrypt_chunk(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = self.derive_nonce();
        let nonce_ga = GenericArray::from_slice(&nonce);

        let ciphertext = self.cipher
            .encrypt(nonce_ga, plaintext)
            .map_err(|e| Error::Encryption(format!("Chunk encryption failed: {}", e)))?;

        self.counter += 1;
        Ok(ciphertext)
    }

    /// Decrypt a chunk and advance the counter
    ///
    /// # Arguments
    /// * `ciphertext` - Encrypted data with tag
    ///
    /// # Returns
    /// Decrypted plaintext
    ///
    /// # Errors
    /// Returns error if authentication fails (tampered data)
    pub fn decrypt_chunk(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 16 {
            return Err(Error::Decryption("Ciphertext too short for tag".into()));
        }

        let nonce = self.derive_nonce();
        let nonce_ga = GenericArray::from_slice(&nonce);

        let plaintext = self.cipher
            .decrypt(nonce_ga, ciphertext)
            .map_err(|e| Error::Decryption(format!("Chunk decryption failed: {}", e)))?;

        self.counter += 1;
        Ok(plaintext)
    }

    /// Reset the counter to a specific value
    ///
    /// Use with caution - resetting to a previously used counter
    /// will result in nonce reuse, which is a security vulnerability.
    pub fn reset_counter(&mut self, counter: u64) {
        self.counter = counter;
    }

    /// Encrypt multiple chunks in sequence
    ///
    /// # Arguments
    /// * `chunks` - Iterator of plaintext chunks
    ///
    /// # Returns
    /// Vector of encrypted chunks
    pub fn encrypt_chunks<'a, I>(&mut self, chunks: I) -> Result<Vec<Vec<u8>>>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        chunks
            .into_iter()
            .map(|chunk| self.encrypt_chunk(chunk))
            .collect()
    }

    /// Decrypt multiple chunks in sequence
    ///
    /// # Arguments
    /// * `chunks` - Iterator of ciphertext chunks
    ///
    /// # Returns
    /// Vector of decrypted chunks
    pub fn decrypt_chunks<'a, I>(&mut self, chunks: I) -> Result<Vec<Vec<u8>>>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        chunks
            .into_iter()
            .map(|chunk| self.decrypt_chunk(chunk))
            .collect()
    }
}

/// Builder for creating StreamCipher instances
pub struct StreamCipherBuilder {
    key: Option<[u8; 32]>,
    nonce: Option<[u8; 12]>,
    initial_counter: u64,
}

impl StreamCipherBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            key: None,
            nonce: None,
            initial_counter: 0,
        }
    }

    /// Set the encryption key
    pub fn key(mut self, key: [u8; 32]) -> Self {
        self.key = Some(key);
        self
    }

    /// Set the base nonce
    pub fn nonce(mut self, nonce: [u8; 12]) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set the initial counter value
    pub fn initial_counter(mut self, counter: u64) -> Self {
        self.initial_counter = counter;
        self
    }

    /// Build the StreamCipher
    ///
    /// # Errors
    /// Returns error if key or nonce is not set
    pub fn build(self) -> Result<StreamCipher> {
        let key = self.key.ok_or_else(|| Error::InvalidKey("Key not set".into()))?;
        let nonce = self.nonce.ok_or_else(|| Error::InvalidKey("Nonce not set".into()))?;

        let mut cipher = StreamCipher::new(&key, &nonce);
        cipher.counter = self.initial_counter;

        Ok(cipher)
    }
}

impl Default for StreamCipherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_cipher_creation() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let cipher = StreamCipher::new(&key, &nonce);

        assert_eq!(cipher.counter(), 0);
    }

    #[test]
    fn test_encrypt_decrypt_single_chunk() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        let plaintext = b"Hello, World!";
        let ciphertext = encryptor.encrypt_chunk(plaintext).unwrap();
        let decrypted = decryptor.decrypt_chunk(&ciphertext).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_multiple_chunks() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        let chunks = [b"First chunk".as_slice(), b"Second chunk", b"Third chunk"];

        for chunk in &chunks {
            let ciphertext = encryptor.encrypt_chunk(chunk).unwrap();
            let decrypted = decryptor.decrypt_chunk(&ciphertext).unwrap();
            assert_eq!(&decrypted, chunk);
        }

        assert_eq!(encryptor.counter(), 3);
        assert_eq!(decryptor.counter(), 3);
    }

    #[test]
    fn test_counter_increments() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let mut cipher = StreamCipher::new(&key, &nonce);

        assert_eq!(cipher.counter(), 0);

        cipher.encrypt_chunk(b"test").unwrap();
        assert_eq!(cipher.counter(), 1);

        cipher.encrypt_chunk(b"test").unwrap();
        assert_eq!(cipher.counter(), 2);
    }

    #[test]
    fn test_different_nonces_per_chunk() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let mut cipher = StreamCipher::new(&key, &nonce);

        // Same plaintext should produce different ciphertexts
        let ct1 = cipher.encrypt_chunk(b"same").unwrap();
        let ct2 = cipher.encrypt_chunk(b"same").unwrap();

        assert_ne!(ct1, ct2);
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        let mut ciphertext = encryptor.encrypt_chunk(b"secret").unwrap();

        // Tamper with ciphertext
        ciphertext[0] ^= 0xFF;

        let result = decryptor.decrypt_chunk(&ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = [0x42u8; 32];
        let key2 = [0x43u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key1, &nonce);
        let mut decryptor = StreamCipher::new(&key2, &nonce);

        let ciphertext = encryptor.encrypt_chunk(b"secret").unwrap();
        let result = decryptor.decrypt_chunk(&ciphertext);

        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_counter_fails() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        // Encrypt two chunks
        let _ = encryptor.encrypt_chunk(b"first").unwrap();
        let ct2 = encryptor.encrypt_chunk(b"second").unwrap();

        // Try to decrypt second chunk with counter at 0 (should fail)
        let result = decryptor.decrypt_chunk(&ct2);
        assert!(result.is_err());
    }

    #[test]
    fn test_reset_counter() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let mut cipher = StreamCipher::new(&key, &nonce);

        cipher.encrypt_chunk(b"test").unwrap();
        assert_eq!(cipher.counter(), 1);

        cipher.reset_counter(100);
        assert_eq!(cipher.counter(), 100);
    }

    #[test]
    fn test_encrypt_chunks_batch() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        let chunks: Vec<&[u8]> = vec![b"one", b"two", b"three"];
        let encrypted = encryptor.encrypt_chunks(chunks.iter().copied()).unwrap();

        assert_eq!(encrypted.len(), 3);

        let decrypted = decryptor.decrypt_chunks(encrypted.iter().map(|c| c.as_slice())).unwrap();

        assert_eq!(decrypted[0], b"one");
        assert_eq!(decrypted[1], b"two");
        assert_eq!(decrypted[2], b"three");
    }

    #[test]
    fn test_empty_chunk() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        let ciphertext = encryptor.encrypt_chunk(b"").unwrap();
        let decrypted = decryptor.decrypt_chunk(&ciphertext).unwrap();

        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_large_chunk() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let mut encryptor = StreamCipher::new(&key, &nonce);
        let mut decryptor = StreamCipher::new(&key, &nonce);

        let large_data = vec![0xABu8; 1024 * 1024]; // 1MB
        let ciphertext = encryptor.encrypt_chunk(&large_data).unwrap();
        let decrypted = decryptor.decrypt_chunk(&ciphertext).unwrap();

        assert_eq!(decrypted, large_data);
    }

    #[test]
    fn test_builder_pattern() {
        let cipher = StreamCipherBuilder::new()
            .key([0x42u8; 32])
            .nonce([0x24u8; 12])
            .initial_counter(10)
            .build()
            .unwrap();

        assert_eq!(cipher.counter(), 10);
    }

    #[test]
    fn test_builder_missing_key() {
        let result = StreamCipherBuilder::new()
            .nonce([0x24u8; 12])
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_builder_missing_nonce() {
        let result = StreamCipherBuilder::new()
            .key([0x42u8; 32])
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_nonce_derivation_uniqueness() {
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];

        let cipher1 = StreamCipher::new(&key, &nonce);
        let mut cipher2 = StreamCipher::new(&key, &nonce);
        cipher2.counter = 1;

        // Different counters should produce different derived nonces
        let nonce1 = cipher1.derive_nonce();
        let nonce2 = cipher2.derive_nonce();

        assert_ne!(nonce1, nonce2);
    }
}
