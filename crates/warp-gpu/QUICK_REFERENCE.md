# warp-gpu Quick Reference

## Performance Targets (RTX 4090)

| Operation | Throughput | Crossover | CPU Speedup |
|-----------|------------|-----------|-------------|
| BLAKE3 | 18 GB/s | 64 KB | 2.6x |
| ChaCha20-Poly1305 | 22 GB/s | 256 KB | 2.2x |

## CUDA Kernel Configurations

### BLAKE3

```c
Grid:  (ceil(data_size / 1024), 1, 1)  // One block per 1KB chunk
Block: (256, 1, 1)                      // 8 warps
Shared Memory: 1 KB per block
Registers: ~60 per thread
Occupancy: 50% (4 blocks/SM)
```

### ChaCha20-Poly1305

```c
Grid:  (ceil(data_size / (64 * 4 * 256)), 1, 1)
Block: (256, 1, 1)
Shared Memory: 0 KB (all registers)
Registers: ~50 per thread
Occupancy: 62.5% (5 blocks/SM)
Thread Coarsening: 4 blocks/thread
```

## Memory Budget (4 streams, 16MB batches)

| Operation | Host Pinned | Device | Total |
|-----------|-------------|--------|-------|
| BLAKE3 | 64 MB | 71 MB | 135 MB |
| ChaCha20 | 128 MB | 128 MB | 256 MB |
| **Combined** | **192 MB** | **200 MB** | **392 MB** |

## Optimal Configuration

```rust
// Stream config
StreamConfig {
    num_streams: 4,
    buffer_size: 16 * 1024 * 1024,  // 16MB
    use_priority: false,
    priority: 0,
}

// Pool config
PoolConfig {
    max_total_memory: 512 * 1024 * 1024,  // 512MB
    size_classes: vec![64*1024, 1024*1024, 16*1024*1024, 64*1024*1024],
    max_buffers_per_class: 4,
    track_statistics: true,
}
```

## Rust API Cheat Sheet

### Memory Pool

```rust
// Create pool
let pool = PinnedMemoryPool::with_defaults(device);

// Acquire buffer
let mut buffer = pool.acquire(16 * 1024 * 1024)?;

// Use buffer
buffer.copy_from_slice(&data)?;

// Release (or drop)
pool.release(buffer);

// Stats
let stats = pool.statistics();
println!("Cache hit rate: {:.1}%",
    100.0 * stats.cache_hits as f64 / stats.allocations as f64);
```

### BLAKE3 Hashing

```rust
// Single hash
let hasher = Blake3Hasher::new(device)?;
let hash = hasher.hash(&data)?;  // [u8; 32]

// Batch hashing
let batch = Blake3Batch::new(device)?;
let hashes = batch.hash_batch(&[&data1, &data2, &data3])?;
```

### ChaCha20-Poly1305

```rust
// Encrypt
let cipher = ChaCha20Poly1305::new(device)?;
let key = [0u8; 32];
let nonce = [0u8; 12];
let ciphertext = cipher.encrypt(&plaintext, &key, &nonce)?;

// Decrypt
let plaintext = cipher.decrypt(&ciphertext, &key, &nonce)?;

// Batch
let batch = EncryptionBatch::new(device)?;
let encrypted = batch.encrypt_batch(&plaintexts, &keys, &nonces)?;
```

### Stream Management

```rust
// Create manager
let manager = StreamManager::with_defaults(device, pool)?;

// Acquire stream (RAII)
let stream_guard = manager.acquire_stream()?;
let stream_id = stream_guard.id();

// Use stream...

// Released on drop

// Synchronize all
manager.synchronize_all()?;
```

### Pipeline Executor

```rust
let executor = PipelineExecutor::new(Arc::new(manager));

let results = executor.execute_batch(&inputs, |stream_id, data| {
    // Process on specific stream
    hash_on_stream(stream_id, data)
})?;
```

## Tuning Parameters

### Batch Size

| Size | GPU Util | Memory | Latency | Recommendation |
|------|----------|--------|---------|----------------|
| < 1 MB | Low | Low | Low | Use CPU |
| 1-4 MB | Medium | Low | Low | OK for small jobs |
| 4-32 MB | High | Medium | Medium | **Optimal** |
| > 64 MB | High | High | High | Memory pressure |

**Formula**: `optimal = min(32 MB, GPU_memory * 0.1)`

### Stream Count

| Streams | Overlap | Memory | Recommendation |
|---------|---------|--------|----------------|
| 1 | None | Low | **Don't use** |
| 2 | Partial | Low | Insufficient |
| 3-4 | Full | Medium | **Optimal** |
| > 8 | Diminishing | High | Overhead |

**Formula**: `optimal = max(3, ceil(max_latency / min_latency))`

### Thread Block Size

| Size | Occupancy | Warps/Block | Recommendation |
|------|-----------|-------------|----------------|
| 128 | 25-50% | 4 | Too small |
| 256 | 50-75% | 8 | **Optimal** |
| 512 | 50-100% | 16 | Register pressure |
| 1024 | 25-100% | 32 | Rarely useful |

**Rule**: Use 256 for most kernels (8 warps = good balance)

## Performance Debugging

### Low Throughput

**Symptom**: < 50% of expected throughput

**Checklist**:
1. Check batch size (< 1MB → CPU faster)
2. Verify stream overlap (only 1 stream → no overlap)
3. Profile with Nsight Compute (memory bound? compute bound?)
4. Check PCIe bandwidth (lspci, nvidia-smi)
5. Verify no CPU throttling (power management)

### High Latency

**Symptom**: First operation takes 10x longer

**Solution**: Warm up GPU before timing
```rust
// Warm-up
hasher.hash(&dummy_data)?;

// Now measure
let start = Instant::now();
let hash = hasher.hash(&real_data)?;
let elapsed = start.elapsed();
```

### Memory Errors

**Symptom**: `out of memory` errors

**Checklist**:
1. Reduce batch size (16MB → 8MB)
2. Reduce stream count (4 → 3)
3. Check GPU memory usage (nvidia-smi)
4. Free unused buffers (pool.clear())
5. Verify no memory leaks

## Common Pitfalls

### Pitfall 1: Forgetting to Synchronize

```rust
// BAD: No sync before reading result
let hash = hasher.hash(&data)?;
// Kernel may still be running!

// GOOD: Synchronize
hasher.hash(&data)?;
device.synchronize()?;
// Now result is ready
```

### Pitfall 2: Using GPU for Small Data

```rust
// BAD: Transfer overhead dominates
for chunk in small_chunks {  // 1KB each
    hasher.hash(chunk)?;  // GPU slower than CPU!
}

// GOOD: Batch or use CPU
if total_size < 64 * 1024 {
    blake3::hash(&data)  // CPU
} else {
    hasher.hash(&data)?  // GPU
}
```

### Pitfall 3: Not Reusing Buffers

```rust
// BAD: Allocate every time
for data in batches {
    let buffer = PinnedBuffer::new(size)?;  // Slow!
    buffer.copy_from_slice(data)?;
    // ...
}

// GOOD: Reuse from pool
for data in batches {
    let mut buffer = pool.acquire(size)?;  // Fast (cache hit)
    buffer.copy_from_slice(data)?;
    // ...
    pool.release(buffer);
}
```

### Pitfall 4: Blocking Streams

```rust
// BAD: Synchronize each stream individually
for i in 0..num_batches {
    let stream = manager.acquire_stream()?;
    process_on_stream(stream, batch[i])?;
    stream.synchronize()?;  // Blocks! No overlap!
}

// GOOD: Launch all, then sync
let handles: Vec<_> = (0..num_batches).map(|i| {
    let stream = manager.acquire_stream()?;
    std::thread::spawn(move || {
        process_on_stream(stream, batch[i])
    })
}).collect();

// Sync at end
manager.synchronize_all()?;
```

## Nsight Compute Key Metrics

| Metric | Target | Meaning |
|--------|--------|---------|
| Achieved Occupancy | > 60% | GPU utilization |
| Memory Throughput | > 80% of peak | Memory efficiency |
| Compute Utilization | > 70% | ALU usage |
| L1 Cache Hit Rate | > 80% | Data reuse |
| Warp Execution Efficiency | > 90% | Branch divergence |

## File Locations

```
crates/warp-gpu/
├── src/
│   ├── lib.rs           - Public API
│   ├── error.rs         - Error types
│   ├── context.rs       - GPU device management
│   ├── memory.rs        - Pinned memory pool
│   ├── buffer.rs        - GPU/host buffers
│   ├── traits.rs        - GpuHasher/GpuCipher traits
│   ├── blake3.rs        - BLAKE3 hasher + kernel
│   ├── chacha20.rs      - ChaCha20-Poly1305 + kernel
│   └── stream.rs        - Multi-stream manager
├── DESIGN.md            - Architecture deep-dive
├── README.md            - User documentation
├── IMPLEMENTATION_SUMMARY.md - Implementation guide
├── KERNEL_OPTIMIZATION_GUIDE.md - CUDA optimization
└── QUICK_REFERENCE.md   - This file
```

## Build Commands

```bash
# Standard build
cargo build --release --features cuda-12060

# Run tests
cargo test --features cuda-12060 -- --test-threads=1

# Benchmark
cargo bench --features cuda-12060

# Profile with Nsight Compute
ncu -o profile --set full cargo bench blake3_gpu

# Check memory usage
nvidia-smi
```

## Environment Variables

```bash
export CUDA_PATH=/usr/local/cuda-12.6
export PATH=$CUDA_PATH/bin:$PATH
export LD_LIBRARY_PATH=$CUDA_PATH/lib64:$LD_LIBRARY_PATH

# For debugging
export CUDA_LAUNCH_BLOCKING=1  # Synchronous kernels
export CUDA_DEVICE_ORDER=PCI_BUS_ID  # Stable device IDs
```

## GPU Selection

```rust
// List devices
for i in 0..num_devices {
    match GpuContext::with_device(i) {
        Ok(ctx) => {
            println!("Device {}: {}", i, ctx.device_name()?);
            println!("  Memory: {} MB", ctx.total_memory() / 1024 / 1024);
        }
        Err(e) => println!("Device {} unavailable: {}", i, e),
    }
}

// Select best device
let ctx = GpuContext::with_device(select_fastest_gpu()?)?;
```

## Theoretical Limits

### RTX 4090

```
Compute Capability: 8.9 (Ada Lovelace)
SMs: 128
CUDA Cores: 16,384
Tensor Cores: 512
Memory: 24 GB GDDR6X
Memory Bandwidth: 1008 GB/s
PCIe: 4.0 x16 = 32 GB/s bidirectional
FP32 Peak: 82.6 TFLOPS

Bottlenecks:
- BLAKE3: PCIe (data movement)
- ChaCha20: PCIe (data movement)
- LZ4: Memory bandwidth (1008 GB/s)
```

### RTX 3070

```
Compute Capability: 8.6 (Ampere)
SMs: 46
CUDA Cores: 5,888
Memory: 8 GB GDDR6
Memory Bandwidth: 448 GB/s
PCIe: 4.0 x16 = 32 GB/s bidirectional
FP32 Peak: 20.3 TFLOPS

Bottlenecks:
- All ops: PCIe (limited by 32 GB/s)
```

## Further Reading

- [DESIGN.md](DESIGN.md) - Full architecture
- [KERNEL_OPTIMIZATION_GUIDE.md](KERNEL_OPTIMIZATION_GUIDE.md) - Kernel details
- [CUDA Best Practices](https://docs.nvidia.com/cuda/cuda-c-best-practices-guide/)
- [cudarc Docs](https://docs.rs/cudarc/)
