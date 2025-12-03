# Active Context

## Current Focus
**Archive Mode Implementation** - Building the core archive creation and extraction pipeline before network transfer features.

Priority order:
1. Complete warp-format (reader/writer with Merkle verification)
2. Integrate warp-io chunking with warp-compress adaptive selection
3. Build warp-core orchestration to tie components together
4. Add GPU acceleration path (nvCOMP integration)

## Recent Changes
- Established workspace structure with 8 crates
- Implemented Buzhash content-defined chunking (warp-io)
- Completed BLAKE3 hashing with parallel support (warp-hash)
- Built CPU compression with zstd/lz4 (warp-compress)
- Defined .warp file format header (256 bytes)
- Created frame protocol definitions for network layer
- Implemented ChaCha20-Poly1305 encryption primitives

## Next Steps
- [ ] **Fix Merkle tree** - Replace XOR placeholder with BLAKE3 in hash_pair()
- [ ] **Add serialization** - ChunkIndex and FileTable need to_bytes/from_bytes
- [ ] **Implement WarpWriter** - Complete add_file/add_directory with chunking pipeline
- [ ] **Implement WarpReader** - Complete extract_all/extract_file with verification
- [ ] **Add GPU module** - Create warp-compress/src/gpu/nvcomp.rs
- [ ] **QUIC transport** - Implement WarpEndpoint::connect with quinn
- [ ] **Integration test** - End-to-end archive create/extract test

## Active Decisions

### Decision Needed: Chunk Size Strategy
**Context**: Current default is 4MB target, 1MB min, 16MB max.
- Larger chunks: Better compression ratio, less overhead, worse dedup
- Smaller chunks: Better dedup, more overhead, harder to saturate GPU
**Options**:
1. Keep 4MB target (balanced)
2. Increase to 8MB (favor throughput)
3. Make adaptive based on payload size

### Decision Made: Binary Protocol over JSON
**Decided**: Use MessagePack (rmp-serde) for wire protocol
**Rationale**:
- 30-50% smaller than JSON
- Faster serialization
- Still human-debuggable with tools
- Existing serde support

### Decision Made: QUIC over TCP
**Decided**: Use quinn/QUIC for all network transport
**Rationale**:
- Built-in TLS 1.3 encryption
- Multiplexed streams without head-of-line blocking
- 0-RTT connection resume
- Native congestion control

## Important Patterns

### Error Propagation
Each crate defines its own Error enum with thiserror, then warp-core aggregates:
```rust
// In warp-core/src/lib.rs
pub enum Error {
    Io(#[from] std::io::Error),
    Format(#[from] warp_format::Error),
    Network(#[from] warp_net::Error),
    // ...
}
```

### Async Convention
- Use `async fn` for I/O-bound operations
- Use rayon for CPU-bound parallelism (hashing, compression)
- Bridge with `spawn_blocking` when needed

### Testing Pattern
- Unit tests inline in src files
- Integration tests in /tests directory
- Benchmarks with Criterion, report throughput in bytes/sec

## Project Insights

### Performance Discovery
BLAKE3 with rayon achieves 10GB/s+ on modern CPUs when:
- Data is >1MB (amortizes setup)
- Using mmap for file access
- Thread pool warmed up

### Chunking Insight
Buzhash window size of 48 bytes provides good boundary distribution without excessive CPU cost. Smaller windows (32) have more collisions, larger (64) waste cycles.

### Compression Insight
Entropy sampling first 4KB accurately predicts compressibility for most files. High-entropy files (>0.95) should skip compression entirely.

## Blocked Items
- **GPU Testing**: Need NVIDIA GPU with CUDA 12.x and nvCOMP installed
- **Network Integration Tests**: Need test certificate infrastructure for QUIC
- **Large-scale Benchmarks**: Need multi-TB test dataset generation
