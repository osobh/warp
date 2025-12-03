# Active Context

## Current Focus
**Portal Core Complete - Ready for Network Layer** - Completed Phase 5 (Portal Core) with zero-knowledge encryption, key hierarchy, and Hub server. Ready to begin Phase 6 (Network Layer).

### Milestone Achieved: Portal Core Complete (2025-12-02)
- **Phase 5**: Portal Core complete (portal-core and portal-hub crates)
  - BIP-39 key hierarchy with HKDF derivation
  - Convergent encryption for content-addressed deduplication
  - Portal lifecycle state machine (Created → Active → Expired → Archived)
  - Access control with ACLs, conditions, and link-based sharing
  - Axum 0.8 HTTP server with Ed25519 authentication

### Confirmed Strategy
- **Approach**: Sequential (complete Warp Engine before Portal) ✓
- **MVP**: Full system through GPU Chunk Scheduler (Phase 8)
- **Team**: Solo development
- **Extensions**: After Phase 12 completion

### Next Priority: Phases 6-10
1. ~~Portal Core (zero-knowledge, key hierarchy, lifecycle)~~ ✓
2. WireGuard P2P mesh networking (Phase 6)
3. Edge Intelligence (health, bandwidth estimation) (Phase 7)
4. GPU Chunk Scheduler (10M chunks in <10ms) (Phase 8)
5. Transfer Orchestration (swarm downloads) (Phase 9)
6. Auto-Reconciliation (self-healing) (Phase 10)

## Recent Accomplishments (2025-12-02)

### portal-core Crate - NEW (Phase 5)
- BIP-39 key hierarchy with HKDF (keys.rs - 863 lines)
- Convergent encryption for deduplication (encryption.rs - 810 lines)
- Portal lifecycle state machine (portal.rs - 771 lines)
- Access control with ACLs (access.rs - 837 lines)
- 74 tests passing

### portal-hub Crate - NEW (Phase 5)
- Axum 0.8 HTTP server (server.rs - 556 lines)
- Ed25519 authentication (auth.rs - 446 lines)
- In-memory storage with DashMap (storage.rs - 839 lines)
- REST API endpoints (routes.rs - 734 lines)
- 71 tests passing

### warp-gpu Crate (Phase 3)
- CUDA context management with cudarc 0.18.1
- GPU BLAKE3 kernel (blake3.rs - 885 lines)
- GPU ChaCha20 kernel (chacha20.rs - 885 lines)
- Pinned memory pool with buffer reuse
- 65 tests passing

### warp-stream Crate (Phase 4)
- Triple-buffer pipeline for <5ms latency
- GPU crypto integration with CPU fallback
- Backpressure handling with flow control
- 61 tests passing

### Test Coverage
- 545 tests passing workspace-wide
- 12-crate workspace
- All files under 900 lines

## Next Steps

### Phase 5: Portal Core - COMPLETE
- [x] Create portal-core crate (74 tests)
- [x] Key hierarchy with BIP-39
- [x] Convergent encryption
- [x] Portal lifecycle state machine
- [x] Access control with ACLs
- [x] Create portal-hub crate (71 tests)

### Phase 6 (Next): Network Layer
- [ ] WireGuard interface management (boringtun)
- [ ] Peer configuration and discovery
- [ ] Virtual IP allocation (10.portal.0.0/16)
- [ ] mDNS discovery for LAN peers
- [ ] Hub relay fallback
- [ ] Roaming support

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
- **GPU Testing on Hardware**: Tests pass but actual GPU hardware needed for performance validation
- **Network Integration Tests**: Need test certificate infrastructure for QUIC
- **Large-scale Benchmarks**: Need multi-TB test dataset generation
- **WireGuard Integration**: Phase 6 - Need boringtun crate integration
