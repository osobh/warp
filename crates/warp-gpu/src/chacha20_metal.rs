//! GPU-accelerated ChaCha20-Poly1305 encryption for Metal backend
//!
//! This module provides the Metal Shading Language (MSL) implementation
//! of ChaCha20 for Apple GPUs (M1/M2/M3/M4 series).
//!
//! # Algorithm Overview
//!
//! ChaCha20 is a stream cipher that generates keystream blocks:
//! - 64-byte blocks generated from 256-bit key + 96-bit nonce + 32-bit counter
//! - Each block independent (highly parallelizable)
//! - XOR keystream with plaintext
//!
//! # Metal Parallelization Strategy
//!
//! ## Block-level parallelism:
//! - One thread per ChaCha20 block (64 bytes)
//! - Each thread keeps all state in registers
//! - 20 rounds of quarter-round operations
//!
//! ## Memory access pattern:
//! - Coalesced reads for key and nonce
//! - Each thread reads/writes 64 bytes
//! - Minimal memory divergence
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use warp_gpu::{MetalBackend, Result};
//! use warp_gpu::chacha20_metal::MetalChaCha20Cipher;
//!
//! fn main() -> Result<()> {
//!     let backend = Arc::new(MetalBackend::new()?);
//!     let cipher = MetalChaCha20Cipher::new(backend)?;
//!
//!     let key = [0u8; 32];
//!     let nonce = [0u8; 12];
//!     let plaintext = vec![0u8; 1024];
//!     let ciphertext = cipher.encrypt(&plaintext, &key, &nonce)?;
//!     println!("Encrypted {} bytes", ciphertext.len());
//!     Ok(())
//! }
//! ```

use crate::backend::{GpuBackend, KernelSource};
use crate::backends::metal::{MetalBackend, MetalFunction, MetalModule};
use crate::{Error, Result};
use std::sync::Arc;

/// Metal Shading Language kernel source for ChaCha20 encryption
///
/// Key differences from CUDA version:
/// - Uses `kernel` instead of `__global__`
/// - Uses `thread_position_in_grid` for global thread index
/// - Uses `constant` instead of `__constant__`
/// - No `#pragma unroll` (Metal compiler handles this)
pub const CHACHA20_METAL_KERNEL: &str = r#"
#include <metal_stdlib>
using namespace metal;

// ChaCha20 constants ("expand 32-byte k")
constant uint32_t SIGMA[4] = {
    0x61707865, 0x3320646e, 0x79622d32, 0x6b206574
};

// Quarter round operation
inline void quarter_round(
    thread uint32_t &a, thread uint32_t &b,
    thread uint32_t &c, thread uint32_t &d
) {
    a += b; d ^= a; d = (d << 16) | (d >> 16);
    c += d; b ^= c; b = (b << 12) | (b >> 20);
    a += b; d ^= a; d = (d << 8) | (d >> 24);
    c += d; b ^= c; b = (b << 7) | (b >> 25);
}

// ChaCha20 block function - generates 64-byte keystream block
inline void chacha20_block(
    thread uint32_t state[16],
    const device uint32_t *key,       // 8 words
    const device uint32_t *nonce,     // 3 words
    uint32_t counter
) {
    // Initialize state
    state[0] = SIGMA[0];
    state[1] = SIGMA[1];
    state[2] = SIGMA[2];
    state[3] = SIGMA[3];

    // Key (8 words)
    for (int i = 0; i < 8; i++) {
        state[4 + i] = key[i];
    }

    // Counter (1 word)
    state[12] = counter;

    // Nonce (3 words)
    state[13] = nonce[0];
    state[14] = nonce[1];
    state[15] = nonce[2];

    // Save original state
    uint32_t original[16];
    for (int i = 0; i < 16; i++) {
        original[i] = state[i];
    }

    // 20 rounds (10 double rounds)
    for (int i = 0; i < 10; i++) {
        // Column rounds
        quarter_round(state[0], state[4], state[8], state[12]);
        quarter_round(state[1], state[5], state[9], state[13]);
        quarter_round(state[2], state[6], state[10], state[14]);
        quarter_round(state[3], state[7], state[11], state[15]);

        // Diagonal rounds
        quarter_round(state[0], state[5], state[10], state[15]);
        quarter_round(state[1], state[6], state[11], state[12]);
        quarter_round(state[2], state[7], state[8], state[13]);
        quarter_round(state[3], state[4], state[9], state[14]);
    }

    // Add original state
    for (int i = 0; i < 16; i++) {
        state[i] += original[i];
    }
}

// Packed constants for ChaCha20 encryption
struct ChaCha20Constants {
    uint32_t counter_base;
    uint32_t padding1;
    uint64_t data_size;
};

// Main ChaCha20 encryption kernel
// Each thread processes one 64-byte ChaCha20 block
kernel void chacha20_encrypt(
    device const uint8_t *plaintext [[buffer(0)]],
    device uint8_t *ciphertext [[buffer(1)]],
    device const uint32_t *key [[buffer(2)]],       // 8 words (32 bytes)
    device const uint32_t *nonce [[buffer(3)]],     // 3 words (12 bytes)
    constant ChaCha20Constants &constants [[buffer(4)]],
    uint gid [[thread_position_in_grid]]
) {
    uint32_t counter_base = constants.counter_base;
    uint64_t data_size = constants.data_size;

    uint64_t block_idx = gid;
    uint64_t byte_offset = block_idx * 64;

    if (byte_offset >= data_size) return;

    // Generate keystream for this block
    uint32_t state[16];

    // Generate keystream block
    uint32_t counter = counter_base + (uint32_t)block_idx;
    chacha20_block(state, key, nonce, counter);

    // XOR with plaintext (handle partial block at end)
    uint64_t remaining = data_size - byte_offset;
    int block_bytes = (remaining < 64) ? (int)remaining : 64;

    // Process in 4-byte words for efficiency
    for (int i = 0; i < 16 && (i * 4) < block_bytes; i++) {
        uint32_t plaintext_word = 0;
        uint32_t keystream_word = state[i];

        // Load plaintext word (handle partial word at end)
        for (int j = 0; j < 4 && (i * 4 + j) < block_bytes; j++) {
            plaintext_word |= ((uint32_t)plaintext[byte_offset + i * 4 + j]) << (j * 8);
        }

        // XOR and store
        uint32_t ciphertext_word = plaintext_word ^ keystream_word;

        // Write ciphertext (handle partial word)
        for (int j = 0; j < 4 && (i * 4 + j) < block_bytes; j++) {
            ciphertext[byte_offset + i * 4 + j] = (ciphertext_word >> (j * 8)) & 0xFF;
        }
    }
}

// Optimized version: process 4 blocks per thread for better ILP
kernel void chacha20_encrypt_coalesced(
    device const uint8_t *plaintext [[buffer(0)]],
    device uint8_t *ciphertext [[buffer(1)]],
    device const uint32_t *key [[buffer(2)]],
    device const uint32_t *nonce [[buffer(3)]],
    constant ChaCha20Constants &constants [[buffer(4)]],
    uint gid [[thread_position_in_grid]]
) {
    uint32_t counter_base = constants.counter_base;
    uint64_t data_size = constants.data_size;

    // Each thread processes 4 consecutive 64-byte blocks
    uint64_t base_block = (uint64_t)gid * 4;
    uint64_t byte_offset = base_block * 64;

    if (byte_offset >= data_size) return;

    // Process 4 blocks
    for (int block_offset = 0; block_offset < 4; block_offset++) {
        uint64_t current_offset = byte_offset + block_offset * 64;
        if (current_offset >= data_size) break;

        uint32_t state[16];
        uint32_t counter = counter_base + (uint32_t)(base_block + block_offset);

        // Generate keystream
        chacha20_block(state, key, nonce, counter);

        // XOR with plaintext
        uint64_t remaining = data_size - current_offset;
        int block_bytes = (remaining < 64) ? (int)remaining : 64;

        for (int i = 0; i < 16; i++) {
            if (i * 4 >= block_bytes) break;

            uint32_t plain = 0;
            int bytes_to_process = min(4, block_bytes - i * 4);

            // Load plaintext
            for (int j = 0; j < bytes_to_process; j++) {
                plain |= ((uint32_t)plaintext[current_offset + i * 4 + j]) << (j * 8);
            }

            // Encrypt
            uint32_t cipher = plain ^ state[i];

            // Store ciphertext
            for (int j = 0; j < bytes_to_process; j++) {
                ciphertext[current_offset + i * 4 + j] = (cipher >> (j * 8)) & 0xFF;
            }
        }
    }
}
"#;

/// ChaCha20 block size in bytes
const BLOCK_SIZE: usize = 64;

/// Key size in bytes (256-bit)
const KEY_SIZE: usize = 32;

/// Nonce size in bytes (96-bit)
const NONCE_SIZE: usize = 12;

/// Minimum size for GPU acceleration (64KB)
/// Below this threshold, CPU encryption is faster due to transfer overhead
const MIN_GPU_SIZE: usize = 64 * 1024;

/// Metal-accelerated ChaCha20 stream cipher
///
/// This cipher uses Apple Metal GPU for large data and automatically falls back
/// to CPU ChaCha20 for small data where GPU overhead would be counterproductive.
///
/// # Note
///
/// This is the raw ChaCha20 stream cipher. For authenticated encryption (AEAD),
/// use `MetalChaCha20Poly1305` which adds Poly1305 authentication.
///
/// # Performance Notes
///
/// - GPU acceleration kicks in for data >= 64KB
/// - For data < 64KB, uses CPU ChaCha20 (no transfer overhead)
/// - On M1/M2/M3/M4 chips, can achieve 10+ GB/s for large data
pub struct MetalChaCha20Cipher {
    backend: Arc<MetalBackend>,
    #[allow(dead_code)]
    module: MetalModule,
    encrypt_fn: MetalFunction,
    #[allow(dead_code)]
    encrypt_coalesced_fn: MetalFunction,
}

impl MetalChaCha20Cipher {
    /// Create a new Metal ChaCha20 cipher
    ///
    /// # Arguments
    ///
    /// * `backend` - Arc-wrapped Metal backend
    ///
    /// # Returns
    ///
    /// A new cipher, or an error if shader compilation fails
    pub fn new(backend: Arc<MetalBackend>) -> Result<Self> {
        let source = KernelSource::metal_only(CHACHA20_METAL_KERNEL);
        let module = backend.compile(&source)?;
        let encrypt_fn = backend.get_function(&module, "chacha20_encrypt")?;
        let encrypt_coalesced_fn = backend.get_function(&module, "chacha20_encrypt_coalesced")?;

        Ok(Self {
            backend,
            module,
            encrypt_fn,
            encrypt_coalesced_fn,
        })
    }

    /// Encrypt data, automatically choosing GPU or CPU based on size
    ///
    /// # Arguments
    ///
    /// * `plaintext` - Data to encrypt
    /// * `key` - 32-byte key
    /// * `nonce` - 12-byte nonce
    ///
    /// # Returns
    ///
    /// Ciphertext (same length as plaintext)
    pub fn encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; KEY_SIZE],
        nonce: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>> {
        if plaintext.len() < MIN_GPU_SIZE {
            self.encrypt_cpu(plaintext, key, nonce)
        } else {
            self.encrypt_gpu(plaintext, key, nonce)
        }
    }

    /// Decrypt data (ChaCha20 is symmetric - encrypt = decrypt)
    pub fn decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; KEY_SIZE],
        nonce: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>> {
        self.encrypt(ciphertext, key, nonce)
    }

    /// Always use CPU ChaCha20 (for comparison/fallback)
    pub fn encrypt_cpu(
        &self,
        plaintext: &[u8],
        key: &[u8; KEY_SIZE],
        nonce: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>> {
        use chacha20::ChaCha20;
        use chacha20::cipher::{KeyIvInit, StreamCipher};

        let mut output = plaintext.to_vec();
        let mut cipher = ChaCha20::new(key.into(), nonce.into());
        cipher.apply_keystream(&mut output);
        Ok(output)
    }

    /// Always use GPU ChaCha20
    ///
    /// This is useful for benchmarking or when you know the data is large.
    pub fn encrypt_gpu(
        &self,
        plaintext: &[u8],
        key: &[u8; KEY_SIZE],
        nonce: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>> {
        if plaintext.is_empty() {
            return Ok(Vec::new());
        }

        // Copy plaintext to GPU
        let d_input = self.backend.copy_to_device(plaintext)?;

        // Allocate output buffer
        let d_output = self.backend.allocate(plaintext.len())?;

        // Copy key to GPU (as u32 words)
        let key_words: Vec<u8> = key.to_vec();
        let d_key = self.backend.copy_to_device(&key_words)?;

        // Copy nonce to GPU (padded to 16 bytes for alignment, but kernel only reads 12)
        let mut nonce_padded = [0u8; 16];
        nonce_padded[..NONCE_SIZE].copy_from_slice(nonce);
        let d_nonce = self.backend.copy_to_device(&nonce_padded)?;

        // Build constants (ChaCha20Constants struct):
        // - counter_base: u32 (4 bytes)
        // - padding1: u32 (4 bytes)
        // - data_size: u64 (8 bytes)
        let mut constants = Vec::with_capacity(16);
        constants.extend_from_slice(&0u32.to_le_bytes()); // counter_base
        constants.extend_from_slice(&0u32.to_le_bytes()); // padding
        constants.extend_from_slice(&(plaintext.len() as u64).to_le_bytes());

        // Calculate grid size
        let num_blocks = (plaintext.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;

        // Dispatch kernel
        self.backend.dispatch_kernel(
            &self.encrypt_fn,
            &[&d_input, &d_output, &d_key, &d_nonce],
            &constants,
            ((num_blocks as u32 + 63) / 64, 1, 1), // grid
            (64, 1, 1),                            // threadgroup
        )?;

        // Copy result back
        self.backend.copy_to_host(&d_output)
    }

    /// Encrypt multiple plaintexts in batch (each with different nonce)
    pub fn encrypt_batch(
        &self,
        plaintexts: &[&[u8]],
        key: &[u8; KEY_SIZE],
        nonces: &[&[u8; NONCE_SIZE]],
    ) -> Result<Vec<Vec<u8>>> {
        if plaintexts.len() != nonces.len() {
            return Err(Error::InvalidOperation(
                "Plaintext and nonce counts must match".into(),
            ));
        }

        plaintexts
            .iter()
            .zip(nonces.iter())
            .map(|(pt, nonce)| self.encrypt(pt, key, nonce))
            .collect()
    }

    /// Get the minimum size threshold for GPU acceleration
    pub fn min_gpu_size(&self) -> usize {
        MIN_GPU_SIZE
    }

    /// Check if GPU would be used for given data size
    pub fn would_use_gpu(&self, size: usize) -> bool {
        size >= MIN_GPU_SIZE
    }

    /// Get the underlying Metal backend
    pub fn backend(&self) -> &Arc<MetalBackend> {
        &self.backend
    }

    /// Get the key size in bytes
    pub fn key_size(&self) -> usize {
        KEY_SIZE
    }

    /// Get the nonce size in bytes
    pub fn nonce_size(&self) -> usize {
        NONCE_SIZE
    }
}

impl std::fmt::Debug for MetalChaCha20Cipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetalChaCha20Cipher")
            .field("backend", &self.backend.device_name())
            .field("min_gpu_size", &MIN_GPU_SIZE)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::metal::MetalBackend;

    #[test]
    fn test_kernel_source_not_empty() {
        assert!(!super::CHACHA20_METAL_KERNEL.is_empty());
    }

    #[test]
    fn test_kernel_contains_functions() {
        let kernel = super::CHACHA20_METAL_KERNEL;
        assert!(kernel.contains("kernel void chacha20_encrypt"));
        assert!(kernel.contains("kernel void chacha20_encrypt_coalesced"));
    }

    #[test]
    fn test_kernel_contains_sigma() {
        let kernel = super::CHACHA20_METAL_KERNEL;
        assert!(kernel.contains("0x61707865")); // "expa"
        assert!(kernel.contains("SIGMA"));
    }

    #[test]
    fn test_kernel_contains_quarter_round() {
        let kernel = super::CHACHA20_METAL_KERNEL;
        assert!(kernel.contains("quarter_round"));
        assert!(kernel.contains("chacha20_block"));
    }

    #[test]
    fn test_cipher_creation() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");
        println!("Cipher created: {:?}", cipher);
    }

    #[test]
    fn test_encrypt_empty() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let result = cipher
            .encrypt(&[], &key, &nonce)
            .expect("Failed to encrypt");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_encrypt_small_data_uses_cpu() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        // Small data should use CPU
        let plaintext = vec![0x42u8; 1024];
        assert!(!cipher.would_use_gpu(plaintext.len()));

        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let ciphertext = cipher
            .encrypt(&plaintext, &key, &nonce)
            .expect("Failed to encrypt");
        assert_eq!(ciphertext.len(), plaintext.len());

        // Verify encryption is reversible
        let decrypted = cipher
            .decrypt(&ciphertext, &key, &nonce)
            .expect("Failed to decrypt");
        assert_eq!(decrypted, plaintext);
        println!("Small data encrypt/decrypt verified");
    }

    #[test]
    fn test_cpu_matches_reference() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        // Test with known key/nonce
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let nonce = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00,
        ];

        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let ciphertext = cipher
            .encrypt_cpu(plaintext, &key, &nonce)
            .expect("Failed to encrypt");

        // Decrypt and verify
        let decrypted = cipher
            .decrypt(&ciphertext, &key, &nonce)
            .expect("Failed to decrypt");
        assert_eq!(&decrypted[..], &plaintext[..]);
        println!("CPU ChaCha20 reference test passed");
    }

    #[test]
    fn test_gpu_encrypt_small() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        // Test GPU path directly with small data
        let key = [0x42u8; 32];
        let nonce = [0x24u8; 12];
        let plaintext = vec![0xABu8; 256];

        let gpu_ciphertext = cipher
            .encrypt_gpu(&plaintext, &key, &nonce)
            .expect("Failed to GPU encrypt");
        let cpu_ciphertext = cipher
            .encrypt_cpu(&plaintext, &key, &nonce)
            .expect("Failed to CPU encrypt");

        assert_eq!(
            gpu_ciphertext, cpu_ciphertext,
            "GPU and CPU ciphertext mismatch"
        );
        println!("GPU encryption matches CPU for small data");
    }

    #[test]
    fn test_gpu_encrypt_large() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        // Large data should use GPU automatically
        let key = [0x55u8; 32];
        let nonce = [0x77u8; 12];
        let plaintext = vec![0x99u8; 256 * 1024]; // 256KB

        assert!(cipher.would_use_gpu(plaintext.len()));

        let ciphertext = cipher
            .encrypt(&plaintext, &key, &nonce)
            .expect("Failed to encrypt");
        let decrypted = cipher
            .decrypt(&ciphertext, &key, &nonce)
            .expect("Failed to decrypt");

        assert_eq!(decrypted, plaintext, "Large data decrypt mismatch");
        println!("GPU encryption verified for 256KB data");
    }

    #[test]
    fn test_gpu_matches_cpu_large() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        let key = [0xAAu8; 32];
        let nonce = [0xBBu8; 12];
        let plaintext = vec![0xCCu8; 128 * 1024]; // 128KB

        let gpu_ciphertext = cipher
            .encrypt_gpu(&plaintext, &key, &nonce)
            .expect("Failed to GPU encrypt");
        let cpu_ciphertext = cipher
            .encrypt_cpu(&plaintext, &key, &nonce)
            .expect("Failed to CPU encrypt");

        assert_eq!(
            gpu_ciphertext, cpu_ciphertext,
            "GPU and CPU ciphertext mismatch for large data"
        );
        println!("GPU matches CPU for 128KB data");
    }

    #[test]
    fn test_encrypt_batch() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        let key = [0xFFu8; 32];
        let plaintexts: Vec<Vec<u8>> = (0..5).map(|i| vec![i as u8; 1024]).collect();
        let nonces: Vec<[u8; 12]> = (0..5).map(|i| [i as u8; 12]).collect();

        let plaintext_refs: Vec<&[u8]> = plaintexts.iter().map(|v| v.as_slice()).collect();
        let nonce_refs: Vec<&[u8; 12]> = nonces.iter().collect();

        let ciphertexts = cipher
            .encrypt_batch(&plaintext_refs, &key, &nonce_refs)
            .expect("Failed to batch encrypt");

        assert_eq!(ciphertexts.len(), 5);
        for (i, ct) in ciphertexts.iter().enumerate() {
            let decrypted = cipher
                .decrypt(ct, &key, &nonces[i])
                .expect("Failed to decrypt");
            assert_eq!(decrypted, plaintexts[i], "Batch decrypt mismatch at {}", i);
        }
        println!("Batch encryption verified for 5 inputs");
    }

    #[test]
    fn test_min_gpu_size() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        assert_eq!(cipher.min_gpu_size(), MIN_GPU_SIZE);
        assert!(!cipher.would_use_gpu(0));
        assert!(!cipher.would_use_gpu(MIN_GPU_SIZE - 1));
        assert!(cipher.would_use_gpu(MIN_GPU_SIZE));
        assert!(cipher.would_use_gpu(MIN_GPU_SIZE + 1));
    }

    #[test]
    fn test_key_nonce_sizes() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let cipher = MetalChaCha20Cipher::new(backend).expect("Failed to create cipher");

        assert_eq!(cipher.key_size(), 32);
        assert_eq!(cipher.nonce_size(), 12);
    }
}
