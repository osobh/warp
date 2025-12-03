# CUDA Kernel Optimization Guide for warp-gpu

## Overview

This document provides detailed kernel implementation strategies, optimization techniques, and performance tuning for BLAKE3 and ChaCha20-Poly1305 on NVIDIA GPUs.

## 1. BLAKE3 Kernel Implementation

### 1.1 Optimal Kernel Strategy: One Block Per Chunk

**Why this works**:
- BLAKE3 processes data in 1KB chunks
- Each chunk is independent (perfect parallelism)
- 256 threads can process 1KB efficiently
- Registers are plentiful for BLAKE3 state (16 words)

### 1.2 Detailed Kernel Pseudocode

```c
// BLAKE3 kernel - one block per 1KB chunk
__global__ void blake3_hash_chunks(
    const unsigned char *input,      // Input data
    unsigned int *output,             // Output hashes (8 words per chunk)
    unsigned int num_chunks,
    unsigned long long total_size
) {
    // Thread indexing
    const unsigned int chunk_idx = blockIdx.x;
    if (chunk_idx >= num_chunks) return;

    const unsigned int tid = threadIdx.x;      // 0-255
    const unsigned int warp_id = tid / 32;     // 0-7
    const unsigned int lane_id = tid % 32;     // 0-31

    // Shared memory for chunk data (1KB)
    __shared__ unsigned int chunk_words[256];  // 256 * 4 = 1024 bytes

    // Load chunk data cooperatively (coalesced access)
    // Each thread loads 4 bytes
    const unsigned long long chunk_offset = (unsigned long long)chunk_idx * 1024;
    const unsigned long long chunk_end = min(chunk_offset + 1024, total_size);

    for (unsigned int i = tid; i < 256; i += 256) {
        unsigned long long byte_offset = chunk_offset + i * 4;
        unsigned int word = 0;

        // Load 4 bytes with bounds checking
        if (byte_offset < chunk_end) {
            // Unaligned load (handle any address)
            #pragma unroll
            for (int j = 0; j < 4; j++) {
                unsigned long long off = byte_offset + j;
                if (off < chunk_end) {
                    word |= ((unsigned int)input[off]) << (j * 8);
                }
            }
        }
        chunk_words[i] = word;
    }
    __syncthreads();

    // Now process 16 BLAKE3 blocks (each 64 bytes)
    // Each warp processes 2 blocks
    // Total: 8 warps * 2 blocks = 16 blocks

    // For simplicity, we'll process just the first block here
    // Production code would iterate over all 16 blocks

    // BLAKE3 compression function (warp-cooperative)
    if (warp_id == 0) {
        // State in registers (16 words)
        unsigned int state[16];

        // Initialize state
        #pragma unroll
        for (int i = 0; i < 8; i++) {
            state[i] = BLAKE3_IV[i];
        }

        // Chaining value (for first block, use IV)
        state[8] = BLAKE3_IV[0];
        state[9] = BLAKE3_IV[1];
        state[10] = BLAKE3_IV[2];
        state[11] = BLAKE3_IV[3];

        // Counter, block_len, flags
        state[12] = 0;      // Counter (block index within chunk)
        state[13] = 0;      // Reserved
        state[14] = 64;     // Block length
        state[15] = CHUNK_START | CHUNK_END;  // Flags

        // Save original state for finalization
        unsigned int original[16];
        #pragma unroll
        for (int i = 0; i < 16; i++) {
            original[i] = state[i];
        }

        // Message words (first 16 words of chunk)
        unsigned int msg[16];
        if (lane_id < 16) {
            msg[lane_id] = chunk_words[lane_id];
        }

        // Broadcast message words to all lanes via shuffle
        #pragma unroll
        for (int i = 0; i < 16; i++) {
            msg[i] = __shfl_sync(0xFFFFFFFF, msg[i], i);
        }

        // 7 rounds of BLAKE3 mixing
        #pragma unroll
        for (int round = 0; round < 7; round++) {
            // Permute message schedule
            unsigned int m[16];
            #pragma unroll
            for (int i = 0; i < 16; i++) {
                m[i] = msg[MSG_SCHEDULE[round][i]];
            }

            // Round function: 8 G functions (4 column + 4 diagonal)
            // Column mixing
            g(state, 0, 4,  8, 12, m[0], m[1]);
            g(state, 1, 5,  9, 13, m[2], m[3]);
            g(state, 2, 6, 10, 14, m[4], m[5]);
            g(state, 3, 7, 11, 15, m[6], m[7]);

            // Diagonal mixing
            g(state, 0, 5, 10, 15, m[8],  m[9]);
            g(state, 1, 6, 11, 12, m[10], m[11]);
            g(state, 2, 7,  8, 13, m[12], m[13]);
            g(state, 3, 4,  9, 14, m[14], m[15]);
        }

        // Finalize: state = (state + original)[0:8] XOR (state + original)[8:16]
        if (lane_id < 8) {
            unsigned int h = (state[lane_id] + original[lane_id]) ^
                             (state[lane_id + 8] + original[lane_id + 8]);

            // Write to global memory (coalesced)
            output[chunk_idx * 8 + lane_id] = h;
        }
    }
}

// BLAKE3 G mixing function
__device__ __forceinline__ void g(
    unsigned int *state,
    unsigned int a, unsigned int b, unsigned int c, unsigned int d,
    unsigned int mx, unsigned int my
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

// Fast 32-bit rotation
__device__ __forceinline__ unsigned int rotr32(unsigned int x, unsigned int n) {
    return (x >> n) | (x << (32 - n));
}
```

### 1.3 BLAKE3 Optimization Strategies

#### Strategy 1: Warp Shuffle for Message Distribution

Instead of loading from shared memory in each round, use warp shuffle:

```c
// Load message once
unsigned int my_msg[16];
if (lane_id < 16) {
    #pragma unroll
    for (int i = 0; i < 16; i++) {
        my_msg[i] = chunk_words[i];
    }
}

// Broadcast to all lanes
#pragma unroll
for (int i = 0; i < 16; i++) {
    msg[i] = __shfl_sync(0xFFFFFFFF, my_msg[i], i);
}
```

**Benefit**: Reduces shared memory bank conflicts, uses faster shuffle units.

#### Strategy 2: Vectorized Loads

Use `uint4` for loading 16-byte chunks (requires 16-byte alignment):

```c
if (is_aligned(input, 16)) {
    const uint4 *input_vec = (const uint4 *)input;
    uint4 *chunk_vec = (uint4 *)chunk_words;

    for (unsigned int i = tid; i < 64; i += 256) {
        chunk_vec[i] = input_vec[chunk_idx * 64 + i];
    }
}
```

**Benefit**: 4x fewer loads, better memory throughput.

#### Strategy 3: Thread Coarsening for Small Chunks

For very small inputs, process multiple chunks per block:

```c
const unsigned int chunks_per_block = 4;
const unsigned int chunk_start = blockIdx.x * chunks_per_block;

for (unsigned int c = 0; c < chunks_per_block; c++) {
    unsigned int chunk_idx = chunk_start + c;
    if (chunk_idx >= num_chunks) break;

    // Process chunk...
}
```

**Benefit**: Amortizes kernel launch overhead.

#### Strategy 4: Tree Merging with Persistent Threads

Keep threads alive across merge rounds:

```c
__global__ void blake3_merge_persistent(
    unsigned int *hashes,
    unsigned int num_chunks
) {
    const unsigned int tid = blockIdx.x * blockDim.x + threadIdx.x;
    const unsigned int total_threads = gridDim.x * blockDim.x;

    for (unsigned int level = 0; level < log2(num_chunks); level++) {
        unsigned int stride = 1 << level;
        unsigned int num_pairs = num_chunks / (2 * stride);

        for (unsigned int pair = tid; pair < num_pairs; pair += total_threads) {
            unsigned int left_idx = pair * 2 * stride;
            unsigned int right_idx = left_idx + stride;

            // Merge left and right hashes
            // (In BLAKE3, this is a parent node compression)
            unsigned int parent[8];
            blake3_compress_parent(&hashes[left_idx * 8],
                                   &hashes[right_idx * 8],
                                   parent);

            #pragma unroll
            for (int i = 0; i < 8; i++) {
                hashes[left_idx * 8 + i] = parent[i];
            }
        }

        __syncthreads();  // Wait for all threads to finish level
    }
}
```

**Benefit**: Eliminates kernel launch overhead between merge rounds.

### 1.4 BLAKE3 Occupancy Analysis

**Register usage**:
- State: 16 words
- Original state: 16 words
- Message: 16 words
- Temporaries: ~10 words
- **Total**: ~60 registers/thread

**Occupancy calculation** (RTX 4090, SM 8.9):
- Max registers/SM: 65,536
- Registers/block: 256 threads × 60 = 15,360
- Max blocks/SM: 65,536 / 15,360 = 4 blocks
- But scheduler allows up to 16 blocks/SM
- **Actual**: 4 blocks/SM (register-limited)

**Achieved occupancy**: 4 blocks × 256 threads / 2048 max = 50%

**Tuning**: Reduce register usage to increase occupancy:
- Use shared memory for message words (trade-off: slower access)
- Spill less-used variables to local memory
- Target: 48 registers/thread → 5 blocks/SM → 62.5% occupancy

## 2. ChaCha20 Kernel Implementation

### 2.1 Optimal Kernel Strategy: Thread Coarsening

**Why thread coarsening works**:
- ChaCha20 blocks are independent
- 20 rounds = high arithmetic intensity
- Register pressure is moderate (16 words state)
- Processing 4 blocks/thread improves ILP

### 2.2 Detailed Kernel Pseudocode

```c
// ChaCha20 kernel - each thread processes 4 blocks
__global__ void chacha20_encrypt(
    const unsigned char *plaintext,
    unsigned char *ciphertext,
    const unsigned int *key,        // 8 words (32 bytes)
    const unsigned int *nonce,      // 3 words (12 bytes)
    unsigned int counter_base,
    unsigned long long data_size
) {
    const unsigned long long thread_idx = blockIdx.x * blockDim.x + threadIdx.x;
    const unsigned long long base_block = thread_idx * 4;  // 4 blocks per thread
    const unsigned long long byte_offset = base_block * 64;

    if (byte_offset >= data_size) return;

    // Load key and nonce once (broadcast via L1 cache)
    unsigned int key_local[8];
    unsigned int nonce_local[3];

    #pragma unroll
    for (int i = 0; i < 8; i++) {
        key_local[i] = key[i];
    }

    #pragma unroll
    for (int i = 0; i < 3; i++) {
        nonce_local[i] = nonce[i];
    }

    // Process 4 blocks (unrolled for ILP)
    #pragma unroll
    for (int block_off = 0; block_off < 4; block_off++) {
        unsigned long long offset = byte_offset + block_off * 64;
        if (offset >= data_size) break;

        // ChaCha20 state (16 words, register-resident)
        unsigned int state[16];

        // Initialize state
        state[0] = 0x61707865;  // "expa"
        state[1] = 0x3320646e;  // "nd 3"
        state[2] = 0x79622d32;  // "2-by"
        state[3] = 0x6b206574;  // "te k"

        #pragma unroll
        for (int i = 0; i < 8; i++) {
            state[4 + i] = key_local[i];
        }

        state[12] = counter_base + (unsigned int)(base_block + block_off);

        #pragma unroll
        for (int i = 0; i < 3; i++) {
            state[13 + i] = nonce_local[i];
        }

        // Save original state for addition later
        unsigned int original[16];
        #pragma unroll
        for (int i = 0; i < 16; i++) {
            original[i] = state[i];
        }

        // 20 rounds (10 double rounds)
        #pragma unroll 10
        for (int i = 0; i < 10; i++) {
            // Column rounds
            QUARTERROUND(state[0], state[4], state[8],  state[12]);
            QUARTERROUND(state[1], state[5], state[9],  state[13]);
            QUARTERROUND(state[2], state[6], state[10], state[14]);
            QUARTERROUND(state[3], state[7], state[11], state[15]);

            // Diagonal rounds
            QUARTERROUND(state[0], state[5], state[10], state[15]);
            QUARTERROUND(state[1], state[6], state[11], state[12]);
            QUARTERROUND(state[2], state[7], state[8],  state[13]);
            QUARTERROUND(state[3], state[4], state[9],  state[14]);
        }

        // Add original state
        #pragma unroll
        for (int i = 0; i < 16; i++) {
            state[i] += original[i];
        }

        // XOR with plaintext (coalesced reads/writes)
        unsigned long long remaining = data_size - offset;
        unsigned int block_bytes = (remaining < 64) ? remaining : 64;

        #pragma unroll
        for (int i = 0; i < 16; i++) {
            unsigned int word_offset = i * 4;
            if (word_offset >= block_bytes) break;

            // Load plaintext word
            unsigned int plain = 0;
            unsigned int bytes_to_process = min(4U, block_bytes - word_offset);

            #pragma unroll
            for (unsigned int j = 0; j < 4; j++) {
                if (j < bytes_to_process) {
                    plain |= ((unsigned int)plaintext[offset + word_offset + j]) << (j * 8);
                }
            }

            // Encrypt
            unsigned int cipher = plain ^ state[i];

            // Store ciphertext word
            #pragma unroll
            for (unsigned int j = 0; j < 4; j++) {
                if (j < bytes_to_process) {
                    ciphertext[offset + word_offset + j] = (cipher >> (j * 8)) & 0xFF;
                }
            }
        }
    }
}

// ChaCha20 quarter round
#define QUARTERROUND(a, b, c, d) \
    do { \
        a += b; d ^= a; d = (d << 16) | (d >> 16); \
        c += d; b ^= c; b = (b << 12) | (b >> 20); \
        a += b; d ^= a; d = (d << 8) | (d >> 24); \
        c += d; b ^= c; b = (b << 7) | (b >> 25); \
    } while (0)
```

### 2.3 ChaCha20 Optimization Strategies

#### Strategy 1: Vectorized Memory Access

Use `uint4` for 16-byte loads/stores:

```c
// Load plaintext
const uint4 *plain_vec = (const uint4 *)(plaintext + offset);
uint4 *cipher_vec = (uint4 *)(ciphertext + offset);

// Process in 16-byte chunks
for (int i = 0; i < 4; i++) {  // 4 × 16 = 64 bytes
    uint4 plain_chunk = plain_vec[i];
    uint4 state_chunk = make_uint4(state[i*4], state[i*4+1], state[i*4+2], state[i*4+3]);

    cipher_vec[i] = plain_chunk ^ state_chunk;  // Vector XOR
}
```

**Benefit**: 4x fewer memory operations, better coalescing.

#### Strategy 2: Persistent Kernel with Circular Buffer

Keep threads alive and process multiple batches:

```c
__global__ void chacha20_persistent(
    const unsigned char **plaintext_batches,
    unsigned char **ciphertext_batches,
    unsigned int num_batches,
    ...
) {
    // Each thread processes blocks from all batches
    for (unsigned int batch = 0; batch < num_batches; batch++) {
        // Process blocks from this batch
        ...
    }
}
```

**Benefit**: Eliminates kernel launch overhead for multi-batch workloads.

#### Strategy 3: Shared Memory for Key/Nonce

Load key/nonce to shared memory once per block:

```c
__shared__ unsigned int shared_key[8];
__shared__ unsigned int shared_nonce[3];

// First warp loads key/nonce
if (threadIdx.x < 8) {
    shared_key[threadIdx.x] = key[threadIdx.x];
}
if (threadIdx.x < 3) {
    shared_nonce[threadIdx.x] = nonce[threadIdx.x];
}
__syncthreads();

// All threads read from shared memory
for (int i = 0; i < 8; i++) {
    key_local[i] = shared_key[i];
}
```

**Benefit**: Reduces global memory traffic (but unlikely to help with L1 caching).

#### Strategy 4: SIMD-Within-Register (SWAR)

Process multiple blocks in SIMD fashion using register tricks:

```c
// Pack two 16-bit counters in one 32-bit register
unsigned int dual_counter = (counter1 << 16) | counter0;

// Dual quarter-round (experimental)
// Requires careful bit manipulation
```

**Benefit**: Theoretical 2x throughput (very complex, rarely worth it).

### 2.4 ChaCha20 Occupancy Analysis

**Register usage**:
- State: 16 words
- Original state: 16 words
- Key: 8 words
- Nonce: 3 words
- Loop variables: ~5 words
- **Total**: ~50 registers/thread

**Occupancy calculation** (RTX 4090):
- Max registers/SM: 65,536
- Registers/block: 256 threads × 50 = 12,800
- Max blocks/SM: 65,536 / 12,800 = 5 blocks
- **Actual**: 5 blocks/SM (register-limited)

**Achieved occupancy**: 5 blocks × 256 threads / 2048 max = 62.5%

**Tuning**: Already near-optimal. Could try:
- Reduce to 256 threads/block → 6 blocks/SM → 75% occupancy
- But loses ILP from thread coarsening

## 3. Memory Access Optimization

### 3.1 Coalesced Access Pattern

**Good** (all threads access consecutive addresses):
```c
// Global thread ID
unsigned int tid = blockIdx.x * blockDim.x + threadIdx.x;

// Each thread accesses consecutive bytes
unsigned int data = input[tid * 4];
```

**Bad** (strided or random access):
```c
// Strided access (wastes 31/32 of bandwidth)
unsigned int data = input[tid * 128];

// Random access (can't coalesce at all)
unsigned int data = input[random_indices[tid]];
```

### 3.2 Shared Memory Bank Conflicts

**Bank conflict-free** (sequential access):
```c
__shared__ unsigned int data[256];

// Each thread accesses different bank
data[threadIdx.x] = input[threadIdx.x];
```

**Bank conflicts** (same bank access):
```c
__shared__ unsigned int data[256];

// All threads in warp access bank 0 (32-way conflict)
unsigned int value = data[0];
```

**Solution**: Use padding or restructure access pattern.

### 3.3 L1/L2 Cache Utilization

**L1 cache**: 128 KB per SM (configurable, can prefer shared memory)
**L2 cache**: 40-120 MB shared across all SMs

**Optimization**:
- Reuse data within same warp (L1 hit)
- Access data from nearby blocks (L2 hit)
- Avoid cache thrashing (working set > cache size)

## 4. Kernel Launch Tuning

### 4.1 Optimal Block Size

**BLAKE3**: 256 threads (8 warps)
- Balances occupancy with register usage
- Sufficient parallelism for 1KB chunk

**ChaCha20**: 256 threads
- Same reasoning
- Multiple blocks per thread (coarsening)

### 4.2 Grid Size

**BLAKE3**:
```c
unsigned int num_chunks = (data_size + 1023) / 1024;
dim3 grid(num_chunks, 1, 1);
dim3 block(256, 1, 1);
```

**ChaCha20** (with 4 blocks/thread):
```c
unsigned int num_chacha_blocks = (data_size + 63) / 64;
unsigned int num_threads = (num_chacha_blocks + 3) / 4;
dim3 grid((num_threads + 255) / 256, 1, 1);
dim3 block(256, 1, 1);
```

### 4.3 Stream Concurrency

Launch kernels on different streams for overlap:

```c
for (int stream = 0; stream < num_streams; stream++) {
    // H2D transfer (async)
    cudaMemcpyAsync(d_input[stream], h_input[stream], size,
                    cudaMemcpyHostToDevice, streams[stream]);

    // Kernel launch
    blake3_hash<<<grid, block, 0, streams[stream]>>>(
        d_input[stream], d_output[stream], num_chunks, size
    );

    // D2H transfer (async)
    cudaMemcpyAsync(h_output[stream], d_output[stream], output_size,
                    cudaMemcpyDeviceToHost, streams[stream]);
}
```

## 5. Profiling and Debugging

### 5.1 Nsight Compute Metrics

Key metrics to monitor:

1. **Achieved Occupancy**: Target >60%
2. **Memory Throughput**: Compare to theoretical bandwidth
3. **Compute Utilization**: FLOP/s vs peak
4. **Warp Execution Efficiency**: >90% (minimize divergence)
5. **L1/L2 Cache Hit Rate**: >80% for hashes

### 5.2 Common Issues

**Low occupancy**:
- Reduce register usage (spill to shared memory)
- Reduce shared memory usage (increase L1 cache)
- Increase block size (but watch register pressure)

**Low memory throughput**:
- Check for uncoalesced access (stride analysis)
- Reduce bank conflicts in shared memory
- Use vectorized loads (`uint4`)

**Low compute utilization**:
- Increase arithmetic intensity (more FLOPs per byte)
- Reduce divergence (avoid `if` statements)
- Unroll loops (`#pragma unroll`)

## 6. Production Checklist

- [ ] CUDA error checking after every kernel launch
- [ ] Synchronize before checking results
- [ ] Handle partial blocks at input end
- [ ] Validate alignment for vectorized loads
- [ ] Profile with Nsight Compute
- [ ] Test on multiple GPU architectures (Volta, Turing, Ampere, Hopper)
- [ ] Benchmark against CPU baseline
- [ ] Measure PCIe transfer overhead
- [ ] Implement graceful degradation (CPU fallback)
- [ ] Add unit tests for correctness

## 7. References

- [CUDA Best Practices Guide](https://docs.nvidia.com/cuda/cuda-c-best-practices-guide/)
- [BLAKE3 Reference Implementation](https://github.com/BLAKE3-team/BLAKE3)
- [ChaCha20 RFC 8439](https://datatracker.ietf.org/doc/html/rfc8439)
- [Nsight Compute Documentation](https://docs.nvidia.com/nsight-compute/)
