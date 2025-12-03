# GPU Acceleration

> Hardware Optimization for Petabyte-Scale Operations

---

## 1. Overview

Portal leverages GPU acceleration for all compute-intensive operations:

| Operation | CPU Speed | GPU Speed | Speedup |
|-----------|-----------|-----------|---------|
| BLAKE3 Hashing | 5 GB/s | 70 GB/s | 14× |
| LZ4 Compression | 2 GB/s | 20 GB/s | 10× |
| ChaCha20 Encryption | 3 GB/s | 25 GB/s | 8× |
| Merkle DAG Build | 1M nodes/s | 50M nodes/s | 50× |
| KZG Commitment | Minutes | Seconds | 60× |
| Chunk Scheduling | 100K/s | 10M/s | 100× |

This document details the GPU architecture, memory management, and optimization strategies.

---

## 2. Target Hardware

### 2.1 Primary Development Target: RTX 5090 FE

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       RTX 5090 FE SPECIFICATIONS                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Architecture:        NVIDIA Blackwell (GB202)                              │
│  CUDA Cores:          ~21,760                                               │
│  Tensor Cores:        5th Generation                                        │
│  RT Cores:            4th Generation                                        │
│                                                                             │
│  MEMORY:                                                                    │
│  VRAM:                32 GB GDDR7                                           │
│  Memory Bandwidth:    ~1.8 TB/s                                             │
│  Memory Bus:          512-bit                                               │
│                                                                             │
│  COMPUTE:                                                                   │
│  FP32 Performance:    ~100 TFLOPS                                           │
│  FP16 Performance:    ~200 TFLOPS                                           │
│  INT8 Performance:    ~400 TOPS                                             │
│                                                                             │
│  INTERFACE:                                                                 │
│  PCIe:                5.0 x16 (~64 GB/s bidirectional)                      │
│  NVLink:              Not available (consumer)                              │
│                                                                             │
│  POWER:                                                                     │
│  TDP:                 ~575W                                                 │
│  Recommended PSU:     1000W+                                                │
│                                                                             │
│  WHAT 32 GB ENABLES:                                                        │
│  • 1-2 PB files processed on single GPU                                     │
│  • Full Merkle DAG resident in VRAM                                         │
│  • Complete KZG workspace                                                   │
│  • Large batch processing                                                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Hardware Tiers

| Tier | Example | VRAM | Max File Size* | Use Case |
|------|---------|------|----------------|----------|
| Entry | RTX 3060 | 12 GB | ~100 TB | Personal use |
| Mid | RTX 4080 | 16 GB | ~200 TB | Power user |
| High | RTX 4090 | 24 GB | ~500 TB | Professional |
| **Target** | **RTX 5090** | **32 GB** | **~2 PB** | **Development** |
| Enterprise | A100 | 80 GB | ~5 PB | Data center |
| Multi-GPU | 4× A100 | 320 GB | ~20 PB | Enterprise |

*Maximum file size for single-GPU processing with all features

### 2.3 Fallback: CPU-Only Mode

Portal gracefully degrades without GPU:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       CPU FALLBACK MODE                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  When GPU unavailable:                                                      │
│  • BLAKE3: Use SIMD-optimized CPU implementation (AVX2/AVX-512)             │
│  • Compression: CPU LZ4/Zstd (still fast)                                   │
│  • Encryption: CPU ChaCha20-Poly1305                                        │
│  • Scheduling: Single-threaded scheduler (limited scale)                    │
│  • Merkle/KZG: CPU computation (much slower)                                │
│                                                                             │
│  Performance in CPU mode:                                                   │
│  • Throughput: 2-5 GB/s (vs 20+ GB/s with GPU)                              │
│  • Max practical file size: ~10 TB                                          │
│  • Still functional, just slower                                            │
│                                                                             │
│  Detection:                                                                 │
│  • Probe for CUDA at startup                                                │
│  • Check compute capability (require 7.0+ for full features)                │
│  • Automatically select best available backend                              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Memory Architecture

### 3.1 GPU Memory Layout

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       GPU MEMORY LAYOUT (32 GB)                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ ZONE A: STREAMING I/O (4 GB)                                        │   │
│  │                                                                      │   │
│  │ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                  │   │
│  │ │  Input Buf A │ │  Input Buf B │ │  Input Buf C │   Triple buffer  │   │
│  │ │    512 MB    │ │    512 MB    │ │    512 MB    │                  │   │
│  │ └──────────────┘ └──────────────┘ └──────────────┘                  │   │
│  │                                                                      │   │
│  │ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                  │   │
│  │ │ Output Buf A │ │ Output Buf B │ │ Output Buf C │   Triple buffer  │   │
│  │ │    512 MB    │ │    512 MB    │ │    512 MB    │                  │   │
│  │ └──────────────┘ └──────────────┘ └──────────────┘                  │   │
│  │                                                                      │   │
│  │ ┌──────────────────────────────────────────────────┐                │   │
│  │ │            Staging Area (512 MB)                 │                │   │
│  │ └──────────────────────────────────────────────────┘                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ ZONE B: INTEGRITY (12 GB)                                           │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              Merkle DAG Nodes (8 GB)                       │      │   │
│  │ │              250M nodes @ 32 bytes each                    │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              DAG Hash Table (4 GB)                         │      │   │
│  │ │              cuCollections concurrent map                  │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ ZONE C: KZG (12 GB)                                                 │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              FFT Workspace (6 GB)                          │      │   │
│  │ │              NTT butterflies, twiddle factors              │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              MSM Workspace (4 GB)                          │      │   │
│  │ │              Bucket accumulation, intermediate points      │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              SRS Cache (2 GB)                              │      │   │
│  │ │              Frequently used setup points                  │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ ZONE D: SCHEDULER (3 GB)                                            │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              Chunk State (1.5 GB)                          │      │   │
│  │ │              10M chunks @ 64 bytes each                    │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  │                                                                      │   │
│  │ ┌────────────────────────────────────────────────────────────┐      │   │
│  │ │              Cost Matrix (1.5 GB)                          │      │   │
│  │ │              10M × 100 edges @ 4 bytes each (streaming)    │      │   │
│  │ └────────────────────────────────────────────────────────────┘      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ ZONE E: SCRATCH (1 GB)                                              │   │
│  │                                                                      │   │
│  │ • Temporary computation buffers                                     │   │
│  │ • Proof generation workspace                                        │   │
│  │ • Miscellaneous allocations                                         │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TOTAL: 32 GB                                                               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Memory Transfer Optimization

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MEMORY TRANSFER STRATEGIES                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  PINNED MEMORY:                                                             │
│  • Host memory locked in physical RAM                                       │
│  • Enables DMA transfers (no CPU involvement)                               │
│  • ~12 GB/s per direction on PCIe 4.0                                       │
│  • ~32 GB/s per direction on PCIe 5.0                                       │
│                                                                             │
│  ASYNC TRANSFERS:                                                           │
│  • cudaMemcpyAsync with CUDA streams                                        │
│  • Overlap transfer with computation                                        │
│  • Triple buffering hides transfer latency                                  │
│                                                                             │
│  ZERO-COPY (when beneficial):                                               │
│  • Map host memory into GPU address space                                   │
│  • GPU reads directly from system RAM                                       │
│  • Good for small, random access patterns                                   │
│  • Bad for large sequential transfers                                       │
│                                                                             │
│  UNIFIED MEMORY (fallback):                                                 │
│  • Automatic page migration                                                 │
│  • Simplifies programming                                                   │
│  • Performance penalty for large transfers                                  │
│  • Used only for rarely-accessed data                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. CUDA Kernel Architecture

### 4.1 Kernel Inventory

| Category | Kernel | Purpose | Parallelism |
|----------|--------|---------|-------------|
| **Hashing** | `blake3_hash_chunks` | Chunk content hashing | 1 warp per chunk |
| | `blake3_hash_batch` | Small hash batches | 1 thread per hash |
| **Compression** | `lz4_compress_batch` | LZ4 via nvCOMP | 1 block per chunk |
| | `zstd_compress_batch` | Zstd via nvCOMP | 1 block per chunk |
| | `entropy_analyze` | Compression selection | 1 thread per chunk |
| **Encryption** | `chacha20_encrypt` | Stream cipher | 1 thread per 64B |
| | `poly1305_auth` | Authentication | 1 warp per chunk |
| **Integrity** | `merkle_build_level` | Tree level compute | 1 thread per node |
| | `dag_dedup_check` | Hash table lookup | 1 thread per chunk |
| | `dag_insert_batch` | Hash table insert | 1 thread per chunk |
| | `kzg_fft` | NTT transform | cuFFT |
| | `kzg_msm` | Multi-scalar mult | Custom Pippenger |
| **Scheduler** | `cost_matrix_update` | Cost calculation | 1 thread per pair |
| | `k_best_paths` | Source selection | 1 thread per chunk |
| | `detect_failures` | Timeout check | 1 thread per chunk |
| | `load_balance` | Redistribution | 1 thread per chunk |

### 4.2 Kernel Launch Strategy

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    KERNEL LAUNCH CONFIGURATION                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  GENERAL PRINCIPLES:                                                        │
│  • Block size: 256 threads (good occupancy on most GPUs)                    │
│  • Grid size: ceil(work_items / block_size)                                 │
│  • Use shared memory for inter-thread communication                         │
│  • Coalesce memory accesses when possible                                   │
│                                                                             │
│  EXAMPLE: BLAKE3 BATCH HASHING                                              │
│  ────────────────────────────────                                           │
│  // 10,000 chunks to hash                                                   │
│  int num_chunks = 10000;                                                    │
│  int block_size = 256;                                                      │
│  int grid_size = (num_chunks + block_size - 1) / block_size;  // 40        │
│                                                                             │
│  blake3_hash_chunks<<<grid_size, block_size, shared_mem, stream>>>(         │
│      input_chunks,                                                          │
│      chunk_sizes,                                                           │
│      output_cids,                                                           │
│      num_chunks                                                             │
│  );                                                                         │
│                                                                             │
│  STREAM ORGANIZATION:                                                       │
│  • Stream 0: Input transfers (H2D)                                          │
│  • Stream 1: Computation                                                    │
│  • Stream 2: Output transfers (D2H)                                         │
│  • Dependencies via CUDA events                                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 5. nvCOMP Integration

### 5.1 Overview

nvCOMP is NVIDIA's GPU-accelerated compression library:

| Algorithm | Compression Ratio | Throughput | Use Case |
|-----------|-------------------|------------|----------|
| LZ4 | 2-3× | 20+ GB/s | General, fast |
| Zstd | 3-5× | 5+ GB/s | Better ratio |
| Snappy | 2× | 25+ GB/s | Speed priority |
| Deflate | 3-4× | 2+ GB/s | Compatibility |

### 5.2 Batch API Usage

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    nvCOMP BATCH COMPRESSION                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  // Setup batch manager                                                     │
│  nvcompBatchedLZ4CompressGetTempSize(                                       │
│      num_chunks,                                                            │
│      max_chunk_size,                                                        │
│      nvcompBatchedLZ4DefaultOpts,                                           │
│      &temp_bytes                                                            │
│  );                                                                         │
│                                                                             │
│  // Allocate temp buffer                                                    │
│  cudaMalloc(&temp_buffer, temp_bytes);                                      │
│                                                                             │
│  // Compress batch                                                          │
│  nvcompBatchedLZ4CompressAsync(                                             │
│      input_ptrs,          // Array of input chunk pointers                  │
│      input_sizes,         // Array of input sizes                           │
│      max_chunk_size,                                                        │
│      num_chunks,                                                            │
│      temp_buffer,                                                           │
│      temp_bytes,                                                            │
│      output_ptrs,         // Array of output pointers                       │
│      output_sizes,        // Array of output sizes (filled by nvCOMP)       │
│      nvcompBatchedLZ4DefaultOpts,                                           │
│      stream                                                                 │
│  );                                                                         │
│                                                                             │
│  // Batch 10,000 chunks: ~500ms total = 20 GB/s                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Cryptographic Acceleration

### 6.1 ChaCha20-Poly1305 on GPU

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GPU ChaCha20 IMPLEMENTATION                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ChaCha20 is highly GPU-friendly:                                           │
│  • Each 64-byte block is independent                                        │
│  • Quarter-round is pure arithmetic (add, XOR, rotate)                      │
│  • No table lookups (unlike AES)                                            │
│  • Perfect for SIMT execution                                               │
│                                                                             │
│  PARALLELIZATION:                                                           │
│                                                                             │
│  4 MB chunk = 65,536 blocks of 64 bytes                                     │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ Thread 0:     Block 0      → 20 rounds → Encrypted Block 0        │    │
│  │ Thread 1:     Block 1      → 20 rounds → Encrypted Block 1        │    │
│  │ Thread 2:     Block 2      → 20 rounds → Encrypted Block 2        │    │
│  │ ...                                                                │    │
│  │ Thread 65535: Block 65535  → 20 rounds → Encrypted Block 65535    │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  All 65,536 blocks processed in parallel!                                   │
│                                                                             │
│  With 1000 chunks in batch:                                                 │
│  • 65M blocks total                                                         │
│  • RTX 5090: ~160ms                                                         │
│  • Throughput: ~25 GB/s                                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.2 BLAKE3 on GPU

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GPU BLAKE3 IMPLEMENTATION                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  BLAKE3 internal structure is a Merkle tree:                                │
│                                                                             │
│  Input: 4 MB chunk                                                          │
│  Leaf size: 1 KB                                                            │
│  Leaves: 4096                                                               │
│                                                                             │
│  Level 0: 4096 parallel leaf hashes                                         │
│  Level 1: 2048 parallel parent hashes                                       │
│  Level 2: 1024 parallel parent hashes                                       │
│  ...                                                                        │
│  Level 12: 1 root hash                                                      │
│                                                                             │
│  GPU EXECUTION:                                                             │
│                                                                             │
│  // Level 0: All leaves in parallel                                         │
│  __global__ void blake3_leaves(                                             │
│      uint8_t* input,                                                        │
│      uint32_t* chaining_values,  // Output                                  │
│      int num_leaves                                                         │
│  ) {                                                                        │
│      int leaf_id = blockIdx.x * blockDim.x + threadIdx.x;                   │
│      if (leaf_id >= num_leaves) return;                                     │
│                                                                             │
│      uint8_t* leaf_data = input + leaf_id * 1024;                           │
│      compress_leaf(leaf_data, &chaining_values[leaf_id * 8]);               │
│  }                                                                          │
│                                                                             │
│  // Subsequent levels: Parallel parent compression                          │
│  __global__ void blake3_parents(                                            │
│      uint32_t* input_cvs,                                                   │
│      uint32_t* output_cvs,                                                  │
│      int num_parents                                                        │
│  ) {                                                                        │
│      int parent_id = blockIdx.x * blockDim.x + threadIdx.x;                 │
│      if (parent_id >= num_parents) return;                                  │
│                                                                             │
│      uint32_t* left = &input_cvs[parent_id * 2 * 8];                        │
│      uint32_t* right = &input_cvs[(parent_id * 2 + 1) * 8];                 │
│      compress_parent(left, right, &output_cvs[parent_id * 8]);              │
│  }                                                                          │
│                                                                             │
│  PERFORMANCE: 70+ GB/s on RTX 5090                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 7. KZG Acceleration

### 7.1 FFT on GPU

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GPU FFT FOR KZG                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  KZG requires Number Theoretic Transform (NTT):                             │
│  • FFT over finite field (not complex numbers)                              │
│  • Same structure as FFT, different arithmetic                              │
│                                                                             │
│  OPTIONS:                                                                   │
│                                                                             │
│  1. cuFFT (with modifications)                                              │
│     • Optimized radix-2/4/8 butterflies                                     │
│     • Need custom kernel for field arithmetic                               │
│                                                                             │
│  2. Custom NTT kernel                                                       │
│     • Full control over field operations                                    │
│     • Better for BLS12-381 specific optimizations                           │
│                                                                             │
│  3. Icicle library (Ingonyama)                                              │
│     • GPU-accelerated cryptography library                                  │
│     • Native NTT support                                                    │
│     • Recommended approach                                                  │
│                                                                             │
│  250M-point NTT: ~2 seconds on RTX 5090                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 Multi-Scalar Multiplication (MSM)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GPU MSM FOR KZG                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  MSM: Compute Σ sᵢ × Gᵢ for many scalars and points                         │
│                                                                             │
│  PIPPENGER'S ALGORITHM:                                                     │
│                                                                             │
│  1. WINDOWING                                                               │
│     • Split each scalar into w-bit windows                                  │
│     • Window size w ≈ log₂(n) / 2                                           │
│                                                                             │
│  2. BUCKET ACCUMULATION (GPU-parallel)                                      │
│     • 2^w buckets per window                                                │
│     • Each scalar contributes to one bucket per window                      │
│     • Parallel: Each thread handles subset of scalars                       │
│                                                                             │
│  3. BUCKET REDUCTION (GPU-parallel)                                         │
│     • Reduce buckets within each window                                     │
│     • Parallel prefix sum pattern                                           │
│                                                                             │
│  4. WINDOW COMBINATION                                                      │
│     • Combine windows with appropriate shifts                               │
│     • Sequential (small compared to bucket ops)                             │
│                                                                             │
│  250M-point MSM: ~20 seconds on RTX 5090                                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Multi-GPU Scaling

### 8.1 When Single GPU Isn't Enough

For files > 2 PB, single RTX 5090 runs out of memory:

| File Size | Chunks | Required VRAM | GPUs Needed |
|-----------|--------|---------------|-------------|
| 2 PB | 500M | 32 GB | 1 × RTX 5090 |
| 5 PB | 1.25B | 80 GB | 1 × A100 or 3 × RTX 5090 |
| 10 PB | 2.5B | 160 GB | 2 × A100 or 5 × RTX 5090 |
| 20 PB | 5B | 320 GB | 4 × A100 or 10 × RTX 5090 |

### 8.2 Multi-GPU Strategy

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MULTI-GPU PARTITIONING                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  STRATEGY: Partition by chunk range                                         │
│                                                                             │
│  10 PB file = 2.5B chunks                                                   │
│  4 GPUs: Each handles 625M chunks                                           │
│                                                                             │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐                        │
│  │  GPU 0   │ │  GPU 1   │ │  GPU 2   │ │  GPU 3   │                        │
│  │          │ │          │ │          │ │          │                        │
│  │ Chunks   │ │ Chunks   │ │ Chunks   │ │ Chunks   │                        │
│  │ 0-624M   │ │ 625M-1.25B│ │1.25B-1.875B│ │1.875B-2.5B│                    │
│  │          │ │          │ │          │ │          │                        │
│  │ DAG      │ │ DAG      │ │ DAG      │ │ DAG      │                        │
│  │ Partition│ │ Partition│ │ Partition│ │ Partition│                        │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘                        │
│       │            │            │            │                              │
│       └────────────┴─────┬──────┴────────────┘                              │
│                          │                                                  │
│                    ┌─────▼─────┐                                            │
│                    │   CPU     │                                            │
│                    │ Aggregator│                                            │
│                    │           │                                            │
│                    │ • Merge   │                                            │
│                    │   results │                                            │
│                    │ • Final   │                                            │
│                    │   KZG     │                                            │
│                    └───────────┘                                            │
│                                                                             │
│  COMMUNICATION:                                                             │
│  • NVLink (if available): 600+ GB/s                                         │
│  • PCIe (fallback): 64 GB/s per GPU                                         │
│  • Most computation is embarrassingly parallel                              │
│  • Cross-GPU communication only for final aggregation                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Performance Benchmarks

### 9.1 RTX 5090 Projected Performance

| Operation | Input Size | Time | Throughput |
|-----------|------------|------|------------|
| BLAKE3 Hash | 1 TB | 14.3 s | 70 GB/s |
| BLAKE3 Hash | 1 PB | 4.0 hr | 70 GB/s |
| LZ4 Compress | 1 TB | 50 s | 20 GB/s |
| Zstd Compress | 1 TB | 200 s | 5 GB/s |
| ChaCha20 Encrypt | 1 TB | 40 s | 25 GB/s |
| Merkle DAG Build | 250M nodes | 4 s | 62.5M nodes/s |
| KZG Commitment | 250M chunks | 22 s | 11M chunks/s |
| Scheduler Tick | 10M chunks | 8 ms | 1.25B pairs/s |

### 9.2 End-to-End Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    1 PB ARCHIVE CREATION                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  PHASE 1: Chunking + Hashing                                                │
│  • Read: 1 PB @ 100 GB/s = 2.8 hours (parallel NVMe)                        │
│  • Hash: 1 PB @ 70 GB/s = 4.0 hours (GPU)                                   │
│  • Overlapped: ~4.5 hours                                                   │
│                                                                             │
│  PHASE 2: DAG Construction                                                  │
│  • Build: 250M nodes = 4 seconds                                            │
│                                                                             │
│  PHASE 3: Compression                                                       │
│  • LZ4: 1 PB @ 20 GB/s = 14 hours                                           │
│  • (overlapped with Phase 1)                                                │
│                                                                             │
│  PHASE 4: Encryption                                                        │
│  • ChaCha20: 1 PB @ 25 GB/s = 11 hours                                      │
│  • (overlapped with Phase 1, 3)                                             │
│                                                                             │
│  PHASE 5: KZG Commitment                                                    │
│  • Commit: 22 seconds                                                       │
│                                                                             │
│  PHASE 6: Write Output                                                      │
│  • Write: compressed size @ NVMe speed                                      │
│  • (overlapped)                                                             │
│                                                                             │
│  TOTAL: ~5-6 hours for complete archive with all integrity layers           │
│                                                                             │
│  For comparison:                                                            │
│  • tar + gzip: 50+ hours                                                    │
│  • rsync: 100+ hours                                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. Summary

GPU acceleration enables Portal to:

1. **Process at Wire Speed**: 20+ GB/s throughput
2. **Scale to Petabytes**: Single GPU handles 1-2 PB files
3. **Real-Time Intelligence**: Scheduler processes 10M chunks in <10ms
4. **Cryptographic Strength**: Full integrity (BLAKE3 + Merkle + KZG) with minimal overhead
5. **Graceful Degradation**: Falls back to CPU when GPU unavailable

The RTX 5090's 32 GB VRAM is the sweet spot for development, enabling full-featured processing of multi-petabyte datasets on consumer hardware.
