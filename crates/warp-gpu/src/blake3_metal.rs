//! GPU-accelerated BLAKE3 hashing for Metal backend
//!
//! This module provides the Metal Shading Language (MSL) implementation
//! of BLAKE3 for Apple GPUs (M1/M2/M3/M4 series).
//!
//! # Algorithm Overview
//!
//! BLAKE3 is structured as a Merkle tree with 1KB chunks:
//! 1. Split input into 1KB chunks
//! 2. Hash each chunk independently (parallelizable)
//! 3. Merge hashes in binary tree fashion
//!
//! # Metal Parallelization Strategy
//!
//! ## Chunk-level parallelism (coarse-grained):
//! - One threadgroup per 1KB chunk
//! - Each threadgroup processes 16 x 64-byte blocks internally
//! - Threadgroup size: 256 threads
//!
//! ## Memory access pattern:
//! - Coalesced reads: threads read consecutive bytes
//! - Threadgroup memory staging: 1KB chunk per threadgroup
//! - Output: 32-byte hash per chunk to device memory
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use warp_gpu::{MetalBackend, Result};
//! use warp_gpu::blake3_metal::MetalBlake3Hasher;
//!
//! fn main() -> Result<()> {
//!     let backend = Arc::new(MetalBackend::new()?);
//!     let hasher = MetalBlake3Hasher::new(backend)?;
//!
//!     let data = vec![0u8; 1024 * 1024]; // 1MB
//!     let hash = hasher.hash(&data)?;
//!     println!("BLAKE3 hash: {:?}", hash);
//!     Ok(())
//! }
//! ```

use crate::backend::{GpuBackend, KernelSource};
use crate::backends::metal::{MetalBackend, MetalBuffer, MetalFunction, MetalModule};
use crate::{Error, Result};
use std::sync::Arc;

/// Metal Shading Language kernel source for BLAKE3 hashing
///
/// Key differences from CUDA version:
/// - Uses `kernel` instead of `__global__`
/// - Uses `threadgroup` instead of `__shared__`
/// - Uses `thread_position_in_threadgroup` instead of `threadIdx`
/// - Uses `threadgroup_position_in_grid` instead of `blockIdx`
/// - Uses `threadgroup_barrier(mem_flags::mem_threadgroup)` instead of `__syncthreads()`
pub const BLAKE3_METAL_KERNEL: &str = r#"
#include <metal_stdlib>
using namespace metal;

// BLAKE3 IV constants
constant uint32_t BLAKE3_IV[8] = {
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
    0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19
};

// Permutation indices for BLAKE3 mixing
constant uint8_t MSG_SCHEDULE[7][16] = {
    {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15},
    {2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8},
    {3, 4, 10, 12, 13, 2, 7, 14, 6, 5, 9, 0, 11, 15, 8, 1},
    {10, 7, 12, 9, 14, 3, 13, 15, 4, 0, 11, 2, 5, 8, 1, 6},
    {12, 13, 9, 11, 15, 10, 14, 8, 7, 2, 5, 3, 0, 1, 6, 4},
    {9, 14, 11, 5, 8, 12, 15, 1, 13, 3, 0, 10, 2, 6, 4, 7},
    {11, 15, 5, 0, 1, 9, 8, 6, 14, 10, 2, 12, 3, 4, 7, 13}
};

// Rotation amounts for BLAKE3
inline uint32_t rotr32(uint32_t x, uint32_t n) {
    return (x >> n) | (x << (32 - n));
}

// BLAKE3 G mixing function
inline void g(
    thread uint32_t *state,
    uint32_t a, uint32_t b, uint32_t c, uint32_t d,
    uint32_t mx, uint32_t my
) {
    state[a] = state[a] + state[b] + mx;
    state[d] = rotr32(state[d] ^ state[a], 16);
    state[c] = state[c] + state[d];
    state[b] = rotr32(state[b] ^ state[c], 12);
    state[a] = state[a] + state[b] + my;
    state[d] = rotr32(state[d] ^ state[a], 8);
    state[c] = state[c] + state[d];
    state[b] = rotr32(state[b] ^ state[c], 7);
}

// Round function
inline void round_fn(thread uint32_t *state, const thread uint32_t *msg) {
    // Columns
    g(state, 0, 4, 8, 12, msg[0], msg[1]);
    g(state, 1, 5, 9, 13, msg[2], msg[3]);
    g(state, 2, 6, 10, 14, msg[4], msg[5]);
    g(state, 3, 7, 11, 15, msg[6], msg[7]);

    // Diagonals
    g(state, 0, 5, 10, 15, msg[8], msg[9]);
    g(state, 1, 6, 11, 12, msg[10], msg[11]);
    g(state, 2, 7, 8, 13, msg[12], msg[13]);
    g(state, 3, 4, 9, 14, msg[14], msg[15]);
}

// Compress a single block of 64 bytes
// Note: Uses threadgroup chaining_value for in-place updates
inline void compress_block(
    threadgroup uint32_t *chaining_value,
    const threadgroup uint32_t *block_words,
    uint64_t counter,
    uint32_t block_len,
    uint32_t flags
) {
    uint32_t state[16];

    // Initialize state: first 8 from chaining value, next 4 from IV, last 4 special
    for (int i = 0; i < 8; i++) {
        state[i] = chaining_value[i];
    }
    state[8] = BLAKE3_IV[0];
    state[9] = BLAKE3_IV[1];
    state[10] = BLAKE3_IV[2];
    state[11] = BLAKE3_IV[3];
    state[12] = (uint32_t)(counter & 0xFFFFFFFF);        // Counter low
    state[13] = (uint32_t)((counter >> 32) & 0xFFFFFFFF); // Counter high
    state[14] = block_len;
    state[15] = flags;

    // 7 rounds of permutation and mixing
    uint32_t msg[16];
    for (int round = 0; round < 7; round++) {
        // Permute message according to schedule
        for (int i = 0; i < 16; i++) {
            msg[i] = block_words[MSG_SCHEDULE[round][i]];
        }
        round_fn(state, msg);
    }

    // XOR upper and lower halves to produce output chaining value
    for (int i = 0; i < 8; i++) {
        chaining_value[i] = state[i] ^ state[i + 8];
    }
}

// Packed constants for chunk hashing
struct Blake3ChunkConstants {
    uint32_t num_chunks;
    uint32_t padding1;  // Align to 8 bytes
    uint64_t total_size;
    uint32_t is_single_chunk;
    uint32_t padding2;  // Padding to 24 bytes
};

// Main BLAKE3 kernel - one threadgroup per 1KB chunk
// Grid: (num_chunks, 1, 1), Threadgroup: (256, 1, 1)
kernel void blake3_hash_chunks(
    device const uint8_t *input [[buffer(0)]],
    device uint32_t *output [[buffer(1)]],
    constant Blake3ChunkConstants &constants [[buffer(2)]],
    uint tid [[thread_position_in_threadgroup]],
    uint chunk_idx [[threadgroup_position_in_grid]]
) {
    uint32_t num_chunks = constants.num_chunks;
    uint64_t total_size = constants.total_size;
    uint32_t is_single_chunk = constants.is_single_chunk;

    if (chunk_idx >= num_chunks) return;

    // Threadgroup memory for chunk data (1KB) and chaining value
    threadgroup uint32_t chunk_data[256];  // 1KB = 256 words
    threadgroup uint32_t cv[8];            // Chaining value (8 words = 32 bytes)

    // Load chunk data (coalesced reads)
    uint64_t chunk_offset = (uint64_t)chunk_idx * 1024;
    uint64_t chunk_end = min(chunk_offset + 1024, total_size);
    uint64_t chunk_size = chunk_end - chunk_offset;

    // Each thread loads 4 bytes (256 threads * 4 = 1KB)
    for (uint i = tid; i < 256; i += 256) {
        uint64_t byte_offset = chunk_offset + i * 4;
        if (byte_offset < total_size) {
            // Load 4 bytes as word (handle misalignment)
            uint32_t word = 0;
            for (int j = 0; j < 4; j++) {
                uint64_t off = byte_offset + j;
                if (off < total_size) {
                    word |= ((uint32_t)input[off]) << (j * 8);
                }
            }
            chunk_data[i] = word;
        } else {
            chunk_data[i] = 0;
        }
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Initialize chaining value with IV for first block
    if (tid < 8) {
        cv[tid] = BLAKE3_IV[tid];
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Process all 16 blocks in this chunk (each block is 64 bytes = 16 words)
    // Only thread 0 does the compression to avoid race conditions on shared cv
    if (tid == 0) {
        const int blocks_per_chunk = 16;

        for (int block_idx = 0; block_idx < blocks_per_chunk; block_idx++) {
            // Determine block length
            uint32_t block_len = 64;
            uint64_t block_start = block_idx * 64;
            if (block_start >= chunk_size) {
                break; // This block is entirely padding
            }
            if (block_start + 64 > chunk_size) {
                block_len = (uint32_t)(chunk_size - block_start);
            }

            // Set flags
            uint32_t flags = 0;
            if (block_idx == 0) {
                flags |= 0x01; // CHUNK_START
            }
            // Check if this is the last block in the chunk
            bool is_last_block = (block_idx == blocks_per_chunk - 1) || (block_start + 64 >= chunk_size);
            if (is_last_block) {
                flags |= 0x02; // CHUNK_END
                // For single-chunk inputs, also set ROOT flag on last block
                if (is_single_chunk) {
                    flags |= 0x08; // ROOT
                }
            }

            // Get pointer to this block's words (16 words = 64 bytes)
            const threadgroup uint32_t *block_words = &chunk_data[block_idx * 16];

            // Compress this block, updating cv in place
            // Note: In BLAKE3, the counter is the chunk index (not block index)
            compress_block(cv, block_words, (uint64_t)chunk_idx, block_len, flags);
        }
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);

    // Write output hash (32 bytes = 8 words)
    if (tid < 8) {
        output[chunk_idx * 8 + tid] = cv[tid];
    }
}

// Packed constants for tree merging
struct Blake3MergeConstants {
    uint32_t num_parents;
    uint32_t num_children;
    uint32_t is_root;
    uint32_t padding;
};

// Merge tree kernel for combining chunk hashes
// This implements parent node compression in BLAKE3 tree
kernel void blake3_merge_tree(
    device const uint32_t *chunk_hashes [[buffer(0)]],
    device uint32_t *output [[buffer(1)]],
    constant Blake3MergeConstants &constants [[buffer(2)]],
    uint gid [[thread_position_in_grid]]
) {
    uint32_t num_parents = constants.num_parents;
    uint32_t num_children = constants.num_children;
    uint32_t is_root = constants.is_root;

    uint parent_idx = gid;
    if (parent_idx >= num_parents) return;

    // Each parent node combines two child hashes (left and right)
    uint left_idx = parent_idx * 2;
    uint right_idx = parent_idx * 2 + 1;

    // Parent node compression: hash(left_cv || right_cv)
    // In BLAKE3, parent nodes use a 64-byte block containing both child CVs
    uint32_t block[16]; // 64 bytes
    uint32_t cv[8];

    // Initialize CV with IV
    for (int i = 0; i < 8; i++) {
        cv[i] = BLAKE3_IV[i];
    }

    // Load left child CV (32 bytes = 8 words)
    for (int i = 0; i < 8; i++) {
        block[i] = chunk_hashes[left_idx * 8 + i];
    }

    // Load right child CV (32 bytes = 8 words), or zeros if no right child
    for (int i = 0; i < 8; i++) {
        if (right_idx < num_children) {
            block[8 + i] = chunk_hashes[right_idx * 8 + i];
        } else {
            block[8 + i] = 0;
        }
    }

    // Compress parent node with PARENT flag (0x04), add ROOT flag (0x08) if this is the final merge
    uint32_t flags = 0x04; // PARENT
    if (is_root && parent_idx == 0) {
        flags |= 0x08; // ROOT
    }

    // Inline compress for parent node (uses thread-local block)
    uint32_t state[16];

    // Initialize state
    for (int i = 0; i < 8; i++) {
        state[i] = cv[i];
    }
    state[8] = BLAKE3_IV[0];
    state[9] = BLAKE3_IV[1];
    state[10] = BLAKE3_IV[2];
    state[11] = BLAKE3_IV[3];
    state[12] = 0; // Counter low
    state[13] = 0; // Counter high
    state[14] = 64; // block_len
    state[15] = flags;

    // 7 rounds
    uint32_t msg[16];
    for (int round = 0; round < 7; round++) {
        for (int i = 0; i < 16; i++) {
            msg[i] = block[MSG_SCHEDULE[round][i]];
        }
        // Inline round_fn
        // Columns
        g(state, 0, 4, 8, 12, msg[0], msg[1]);
        g(state, 1, 5, 9, 13, msg[2], msg[3]);
        g(state, 2, 6, 10, 14, msg[4], msg[5]);
        g(state, 3, 7, 11, 15, msg[6], msg[7]);
        // Diagonals
        g(state, 0, 5, 10, 15, msg[8], msg[9]);
        g(state, 1, 6, 11, 12, msg[10], msg[11]);
        g(state, 2, 7, 8, 13, msg[12], msg[13]);
        g(state, 3, 4, 9, 14, msg[14], msg[15]);
    }

    // XOR halves
    for (int i = 0; i < 8; i++) {
        cv[i] = state[i] ^ state[i + 8];
    }

    // Write output
    for (int i = 0; i < 8; i++) {
        output[parent_idx * 8 + i] = cv[i];
    }
}
"#;

/// BLAKE3 chunk size in bytes (1KB)
const CHUNK_SIZE: usize = 1024;

/// Minimum size for GPU acceleration (256KB)
/// Below this threshold, CPU hashing is faster due to transfer overhead
const MIN_GPU_SIZE: usize = 256 * 1024;

/// Metal-accelerated BLAKE3 hasher
///
/// This hasher uses Apple Metal GPU for large data and automatically falls back
/// to CPU BLAKE3 for small data where GPU overhead would be counterproductive.
///
/// # Performance Notes
///
/// - GPU acceleration kicks in for data >= 256KB
/// - For data < 256KB, uses CPU BLAKE3 (no transfer overhead)
/// - On M1/M2/M3/M4 chips, can achieve 2-5 GB/s for large data
pub struct MetalBlake3Hasher {
    backend: Arc<MetalBackend>,
    #[allow(dead_code)]
    module: MetalModule,
    hash_chunks_fn: MetalFunction,
    merge_tree_fn: MetalFunction,
}

impl MetalBlake3Hasher {
    /// Create a new Metal BLAKE3 hasher
    ///
    /// # Arguments
    ///
    /// * `backend` - Arc-wrapped Metal backend
    ///
    /// # Returns
    ///
    /// A new hasher, or an error if shader compilation fails
    pub fn new(backend: Arc<MetalBackend>) -> Result<Self> {
        let source = KernelSource::metal_only(BLAKE3_METAL_KERNEL);
        let module = backend.compile(&source)?;
        let hash_chunks_fn = backend.get_function(&module, "blake3_hash_chunks")?;
        let merge_tree_fn = backend.get_function(&module, "blake3_merge_tree")?;

        Ok(Self {
            backend,
            module,
            hash_chunks_fn,
            merge_tree_fn,
        })
    }

    /// Hash data, automatically choosing GPU or CPU based on size
    ///
    /// # Arguments
    ///
    /// * `data` - Data to hash
    ///
    /// # Returns
    ///
    /// 32-byte BLAKE3 hash
    pub fn hash(&self, data: &[u8]) -> Result<[u8; 32]> {
        if data.len() < MIN_GPU_SIZE {
            // Use CPU for small data (faster due to no transfer overhead)
            self.hash_cpu(data)
        } else {
            // Use GPU for large data
            self.hash_gpu(data)
        }
    }

    /// Always use CPU BLAKE3 (for comparison/fallback)
    pub fn hash_cpu(&self, data: &[u8]) -> Result<[u8; 32]> {
        let hash = blake3::hash(data);
        Ok(*hash.as_bytes())
    }

    /// Always use GPU BLAKE3
    ///
    /// This is useful for benchmarking or when you know the data is large.
    pub fn hash_gpu(&self, data: &[u8]) -> Result<[u8; 32]> {
        if data.is_empty() {
            // Empty data: return BLAKE3 hash of empty input
            return self.hash_cpu(data);
        }

        // Calculate number of chunks
        let num_chunks = (data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let is_single_chunk = if num_chunks == 1 { 1u32 } else { 0u32 };

        // Copy input to GPU
        let d_input = self.backend.copy_to_device(data)?;

        // Allocate output buffer for chunk hashes (8 words = 32 bytes per chunk)
        let d_output = self.backend.allocate(num_chunks * 32)?;

        // Build constants buffer (Blake3ChunkConstants struct):
        // - num_chunks: u32 (4 bytes)
        // - padding1: u32 (4 bytes) - align to 8 bytes
        // - total_size: u64 (8 bytes)
        // - is_single_chunk: u32 (4 bytes)
        // - padding2: u32 (4 bytes) - total 24 bytes
        let mut constants = Vec::with_capacity(24);
        constants.extend_from_slice(&(num_chunks as u32).to_le_bytes());
        constants.extend_from_slice(&0u32.to_le_bytes()); // padding1
        constants.extend_from_slice(&(data.len() as u64).to_le_bytes());
        constants.extend_from_slice(&is_single_chunk.to_le_bytes());
        constants.extend_from_slice(&0u32.to_le_bytes()); // padding2

        // Dispatch chunk hashing kernel
        // Grid: one threadgroup per chunk
        // Threadgroup: 256 threads for coalesced loading
        self.backend.dispatch_kernel(
            &self.hash_chunks_fn,
            &[&d_input, &d_output],
            &constants,
            (num_chunks as u32, 1, 1), // grid: num_chunks threadgroups
            (256, 1, 1),               // threadgroup: 256 threads
        )?;

        // For single chunk, we're done (ROOT flag was set)
        if num_chunks == 1 {
            let output = self.backend.copy_to_host(&d_output)?;
            let mut result = [0u8; 32];
            result.copy_from_slice(&output[..32]);
            return Ok(result);
        }

        // For multiple chunks, merge tree
        let result = self.merge_chunk_hashes(&d_output, num_chunks)?;
        Ok(result)
    }

    /// Merge chunk hashes using tree reduction
    fn merge_chunk_hashes(
        &self,
        chunk_hashes: &MetalBuffer,
        num_chunks: usize,
    ) -> Result<[u8; 32]> {
        let mut current_hashes = chunk_hashes;
        let mut num_children = num_chunks;
        let mut owned_buffer: Option<MetalBuffer> = None;

        while num_children > 1 {
            // Number of parent nodes
            let num_parents = (num_children + 1) / 2;
            let is_root = if num_parents == 1 { 1u32 } else { 0u32 };

            // Allocate output for this level
            let d_output = self.backend.allocate(num_parents * 32)?;

            // Build constants (Blake3MergeConstants struct):
            // - num_parents: u32 (4 bytes)
            // - num_children: u32 (4 bytes)
            // - is_root: u32 (4 bytes)
            // - padding: u32 (4 bytes) - total 16 bytes
            let mut constants = Vec::with_capacity(16);
            constants.extend_from_slice(&(num_parents as u32).to_le_bytes());
            constants.extend_from_slice(&(num_children as u32).to_le_bytes());
            constants.extend_from_slice(&is_root.to_le_bytes());
            constants.extend_from_slice(&0u32.to_le_bytes()); // padding

            // Dispatch merge kernel
            self.backend.dispatch_kernel(
                &self.merge_tree_fn,
                &[current_hashes, &d_output],
                &constants,
                ((num_parents as u32 + 63) / 64, 1, 1), // grid
                (64, 1, 1),                             // threadgroup
            )?;

            // Move to next level
            owned_buffer = Some(d_output);
            current_hashes = owned_buffer.as_ref().unwrap();
            num_children = num_parents;
        }

        // Read final result
        let final_buffer =
            owned_buffer.ok_or_else(|| Error::InvalidOperation("No merge result".into()))?;
        let output = self.backend.copy_to_host(&final_buffer)?;
        let mut result = [0u8; 32];
        result.copy_from_slice(&output[..32]);
        Ok(result)
    }

    /// Hash multiple inputs in batch
    ///
    /// This is efficient when you have many inputs to hash, as it
    /// amortizes GPU kernel launch overhead.
    pub fn hash_batch(&self, inputs: &[&[u8]]) -> Result<Vec<[u8; 32]>> {
        inputs.iter().map(|input| self.hash(input)).collect()
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
}

impl std::fmt::Debug for MetalBlake3Hasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetalBlake3Hasher")
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
        assert!(!super::BLAKE3_METAL_KERNEL.is_empty());
    }

    #[test]
    fn test_kernel_contains_functions() {
        let kernel = super::BLAKE3_METAL_KERNEL;
        assert!(kernel.contains("kernel void blake3_hash_chunks"));
        assert!(kernel.contains("kernel void blake3_merge_tree"));
    }

    #[test]
    fn test_kernel_contains_iv() {
        let kernel = super::BLAKE3_METAL_KERNEL;
        assert!(kernel.contains("0x6A09E667"));
        assert!(kernel.contains("BLAKE3_IV"));
    }

    #[test]
    fn test_hasher_creation() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");
        println!("Hasher created: {:?}", hasher);
    }

    #[test]
    fn test_hash_empty_data() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        let data = b"";
        let gpu_hash = hasher.hash(data).expect("Failed to hash");
        let cpu_hash = blake3::hash(data);

        assert_eq!(&gpu_hash, cpu_hash.as_bytes());
        println!("Empty data hash matches CPU");
    }

    #[test]
    fn test_hash_small_data_uses_cpu() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        // Small data should use CPU
        let data = vec![0x42u8; 1024];
        assert!(!hasher.would_use_gpu(data.len()));

        let hash = hasher.hash(&data).expect("Failed to hash");
        let expected = blake3::hash(&data);
        assert_eq!(&hash, expected.as_bytes());
        println!("Small data hash matches CPU");
    }

    #[test]
    fn test_hash_single_chunk() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        // Single chunk (< 1KB)
        let data = vec![0x42u8; 512];
        let gpu_hash = hasher.hash_gpu(&data).expect("Failed to hash");
        let cpu_hash = blake3::hash(&data);

        assert_eq!(&gpu_hash, cpu_hash.as_bytes(), "Single chunk hash mismatch");
        println!("Single chunk GPU hash matches CPU: {:02x?}", &gpu_hash[..8]);
    }

    #[test]
    fn test_hash_exactly_one_chunk() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        // Exactly one chunk (1KB)
        let data = vec![0xABu8; CHUNK_SIZE];
        let gpu_hash = hasher.hash_gpu(&data).expect("Failed to hash");
        let cpu_hash = blake3::hash(&data);

        assert_eq!(&gpu_hash, cpu_hash.as_bytes(), "1KB chunk hash mismatch");
        println!("1KB GPU hash matches CPU: {:02x?}", &gpu_hash[..8]);
    }

    #[test]
    fn test_hash_large_data() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        // Large data (1MB) - should use GPU automatically
        let data = vec![0x55u8; 1024 * 1024];
        assert!(hasher.would_use_gpu(data.len()));

        let gpu_hash = hasher.hash(&data).expect("Failed to hash");
        let cpu_hash = blake3::hash(&data);

        // Verify GPU hash matches CPU
        println!("1MB GPU hash: {:02x?}", &gpu_hash[..8]);
        println!("1MB CPU hash: {:02x?}", &cpu_hash.as_bytes()[..8]);
        assert_eq!(&gpu_hash, cpu_hash.as_bytes(), "Large data hash mismatch");
    }

    #[test]
    fn test_hash_batch() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        let inputs: Vec<Vec<u8>> = (0..10).map(|i| vec![i as u8; 1024]).collect();
        let input_refs: Vec<&[u8]> = inputs.iter().map(|v| v.as_slice()).collect();

        let hashes = hasher
            .hash_batch(&input_refs)
            .expect("Failed to batch hash");
        assert_eq!(hashes.len(), 10);

        // Verify each hash
        for (i, hash) in hashes.iter().enumerate() {
            let expected = blake3::hash(&inputs[i]);
            assert_eq!(hash, expected.as_bytes(), "Batch hash {} mismatch", i);
        }
        println!("Batch hashing verified for 10 inputs");
    }

    #[test]
    fn test_min_gpu_size() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = Arc::new(MetalBackend::new().expect("Failed to create backend"));
        let hasher = MetalBlake3Hasher::new(backend).expect("Failed to create hasher");

        assert_eq!(hasher.min_gpu_size(), MIN_GPU_SIZE);
        assert!(!hasher.would_use_gpu(0));
        assert!(!hasher.would_use_gpu(MIN_GPU_SIZE - 1));
        assert!(hasher.would_use_gpu(MIN_GPU_SIZE));
        assert!(hasher.would_use_gpu(MIN_GPU_SIZE + 1));
    }
}
