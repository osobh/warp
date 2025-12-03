# Development Plan

> 12-Phase Implementation Roadmap

---

## 1. Executive Summary

This document outlines a 48-week development plan for Warp + Portal, progressing from foundational primitives through production deployment.

**Major Milestones:**
- **Week 16**: Warp Engine Complete (20+ GB/s GPU-accelerated transfers)
- **Week 42**: Portal Complete (Full distributed storage fabric)
- **Week 48+**: Production Ready (Hardened, documented, packaged)

---

## 2. Phase Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         DEVELOPMENT TIMELINE                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  WARP ENGINE (Weeks 1-16)                                                   │
│  ═════════════════════════                                                  │
│                                                                             │
│  Phase 1: Foundation Primitives          [Weeks 1-4]                        │
│  Phase 2: Core Transfer Engine           [Weeks 5-8]                        │
│  Phase 3: GPU Acceleration               [Weeks 9-12]                       │
│  Phase 4: Stream Mode                    [Weeks 13-16]                      │
│                                                                             │
│  ══════════════════════════════════════════════════════════════════════     │
│  MILESTONE: Warp Engine Complete - 20+ GB/s GPU transfers                   │
│  ══════════════════════════════════════════════════════════════════════     │
│                                                                             │
│  PORTAL (Weeks 17-42)                                                       │
│  ════════════════════                                                       │
│                                                                             │
│  Phase 5: Portal Core                    [Weeks 17-20]                      │
│  Phase 6: Network Layer                  [Weeks 21-24]                      │
│  Phase 7: Edge Intelligence              [Weeks 25-28]                      │
│  Phase 8: GPU Chunk Scheduler            [Weeks 29-34]                      │
│  Phase 9: Transfer Orchestration         [Weeks 35-38]                      │
│  Phase 10: Auto-Reconciliation           [Weeks 39-42]                      │
│                                                                             │
│  ══════════════════════════════════════════════════════════════════════     │
│  MILESTONE: Portal Complete - Full distributed storage fabric               │
│  ══════════════════════════════════════════════════════════════════════     │
│                                                                             │
│  PRODUCTION (Weeks 43-48+)                                                  │
│  ═════════════════════════                                                  │
│                                                                             │
│  Phase 11: Production Hardening          [Weeks 43-46]                      │
│  Phase 12: Ecosystem & Tools             [Weeks 47-48+]                     │
│                                                                             │
│  ══════════════════════════════════════════════════════════════════════     │
│  MILESTONE: Production Ready                                                │
│  ══════════════════════════════════════════════════════════════════════     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Warp Engine Phases

### Phase 1: Foundation Primitives (Weeks 1-4)

**Goal**: Establish core building blocks for all subsequent development.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Binary format header | 256-byte header with magic, version, offsets |
| Chunk index | Fixed-size entries with hash, offset, size |
| File table | Path-to-chunk-range mapping |
| BLAKE3 wrapper | Parallel hashing with rayon, >5 GB/s CPU |
| Buzhash chunker | Content-defined chunking, configurable params |
| Directory walker | Recursive traversal with metadata |
| CPU compressors | Zstd >500 MB/s, LZ4 >2 GB/s |
| Crypto primitives | ChaCha20-Poly1305, Ed25519, Argon2 |

**Acceptance Criteria**:
- [ ] Header round-trips through serialize/deserialize
- [ ] Chunking is deterministic (same input → same chunks)
- [ ] >90% test coverage on core types
- [ ] Benchmark suite established

**Dependencies**: None (foundation layer)

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Buzhash parameters affect dedup | Medium | Medium | Empirical tuning with real data |

---

### Phase 2: Core Transfer Engine (Weeks 5-8)

**Goal**: Enable complete archive creation and extraction.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Payload analyzer | Entropy analysis, compression recommendations |
| Transfer engine | Pipeline coordination with parallel stages |
| Session management | State persistence, resume support |
| Archive writer | Streaming .warp creation |
| Archive reader | Random-access extraction |
| Merkle tree | Construction, verification, incremental updates |
| QUIC transport | Single-connection transfer protocol |
| Basic CLI | create, extract, verify, send, receive commands |

**Acceptance Criteria**:
- [ ] Create and extract 1 TB archives correctly
- [ ] Merkle verification detects single-bit errors
- [ ] Resume works after interruption at any point
- [ ] CLI is intuitive (user testing)

**Dependencies**: Phase 1 primitives

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Merkle tree memory for large files | Medium | Medium | Streaming construction, disk spill |

---

### Phase 3: GPU Acceleration (Weeks 9-12)

**Goal**: Achieve 20+ GB/s throughput via GPU.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| CUDA context management | Device selection, memory allocation, streams |
| nvCOMP integration | LZ4 and Zstd GPU compression |
| GPU encryption | ChaCha20-Poly1305 CUDA kernels |
| Pinned memory pool | Zero-copy host-device transfers |
| Adaptive backend | Auto CPU/GPU selection based on payload |
| Batch processing | Efficient multi-chunk GPU operations |
| GPU BLAKE3 | Parallel chunk hashing |

**Acceptance Criteria**:
- [ ] GPU compression >15 GB/s
- [ ] GPU encryption >20 GB/s
- [ ] Graceful fallback when GPU unavailable
- [ ] Memory usage bounded (no OOM on large files)

**Dependencies**: Phase 2 transfer engine, NVIDIA GPU (compute 7.0+)

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| nvCOMP version compatibility | Medium | Medium | Pin versions, CI testing |
| GPU memory limits | High | Medium | Streaming, memory pools |

---

### Phase 4: Stream Mode (Weeks 13-16)

**Goal**: Real-time encrypted streaming with <5ms latency.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Triple-buffer pipeline | Overlapped read, process, write |
| Fixed-size chunking | Time-based or size-based for streams |
| Real-time encryption | Streaming ChaCha20-Poly1305 |
| Latency optimization | CUDA stream prioritization, kernel fusion |
| Backpressure handling | Flow control for producer/consumer mismatch |
| Stream CLI | Commands for pipe-based streaming |

**Acceptance Criteria**:
- [ ] <5ms end-to-end latency for 64KB chunks
- [ ] >10 GB/s sustained on 10GbE network
- [ ] No data loss under backpressure
- [ ] Works with Unix pipes (stdin/stdout)

**Dependencies**: Phase 3 GPU acceleration

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Latency variability from GPU | Medium | Medium | Priority streams, dedicated contexts |

---

## 4. Portal Phases

### Phase 5: Portal Core (Weeks 17-20)

**Goal**: Zero-knowledge encryption and portal lifecycle.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Zero-knowledge encryption | Convergent chunk encryption, manifest encryption |
| Key hierarchy | Master seed derivation, per-purpose keys |
| Portal lifecycle | Creation, activation, deactivation, expiration |
| Lifecycle policies | Time-based, download-limited, schedule-based |
| Access control | Owner, explicit access, link-based, re-entry |
| Hub protocol | Registration, authentication, metadata sync |
| Basic Hub server | Single-instance Hub implementation |

**Acceptance Criteria**:
- [ ] Hub cannot decrypt any user data
- [ ] Portals expire per policy
- [ ] Access control properly enforced
- [ ] Recovery phrase restores access

**Dependencies**: Phase 2 archive format, Phase 1 crypto

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Key management complexity | High | Medium | Clear documentation, recovery testing |

---

### Phase 6: Network Layer (Weeks 21-24)

**Goal**: WireGuard-based P2P mesh networking.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| WireGuard interface management | Create, configure, manage wg interfaces |
| Peer configuration | Add/remove peers, update endpoints |
| Virtual IP allocation | Subnet management, IP assignment |
| mDNS discovery | Announce and discover LAN peers |
| Hub as coordinator | Peer directory, endpoint hints, relay |
| Roaming support | Automatic endpoint updates on IP change |

**Acceptance Criteria**:
- [ ] Edges connect via WireGuard mesh
- [ ] LAN peers discovered and preferred
- [ ] Seamless roaming on IP change
- [ ] Hub relay works when P2P fails

**Dependencies**: Phase 5 Portal core, WireGuard (kernel or userspace)

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| WireGuard platform availability | Medium | Low | Support kernel + userspace modes |

---

### Phase 7: Edge Intelligence (Weeks 25-28)

**Goal**: Track edge state, health, and capabilities.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Edge registry | Track all known edges, capabilities, state |
| Chunk availability | Track which edges have which chunks |
| Bandwidth estimation | EMA-based bandwidth tracking per edge |
| RTT estimation | TCP-style SRTT/RTTVAR estimation |
| Health scoring | Composite score from success rate, uptime |
| Constraint tracking | Metered connections, battery, bandwidth limits |
| Metrics collection | Aggregate statistics, transfer history |

**Acceptance Criteria**:
- [ ] Bandwidth estimates converge within 5 samples
- [ ] Health scores accurately predict failures
- [ ] Constraints respected (no metered abuse)
- [ ] Metrics available via API

**Dependencies**: Phase 6 network layer

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Estimation accuracy | Medium | Medium | Adaptive algorithms, confidence intervals |

---

### Phase 8: GPU Chunk Scheduler (Weeks 29-34)

**Goal**: Real-time scheduling of billions of chunks.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| GPU state buffers | Chunk info, edge state, cost matrix |
| Cost matrix kernel | Parallel cost calculation for all pairs |
| K-best paths kernel | Find top K sources per chunk |
| Failover kernel | Detect failures, switch to backup paths |
| Load balancing kernel | Redistribute from overloaded edges |
| Dispatch queue | CPU-readable scheduling decisions |
| Tick loop | 50ms scheduling cycle |

**Acceptance Criteria**:
- [ ] Schedule 10M chunks in <10ms GPU time
- [ ] Failover to backup in <50ms
- [ ] Load balancing prevents saturation
- [ ] Scales to 1B+ chunks with multi-GPU

**Dependencies**: Phase 7 edge intelligence, Phase 3 GPU infrastructure

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| GPU memory limits | High | Medium | Multi-GPU partitioning, streaming |
| Scheduler complexity | High | Medium | Add buffer time to this phase |

---

### Phase 9: Transfer Orchestration (Weeks 35-38)

**Goal**: Multi-source swarm downloads.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Transfer orchestrator | Coordinate multi-source downloads |
| Swarm download | BitTorrent-style parallel fetch |
| Distributed upload | Parallel push to multiple edges |
| Connection pool | Manage QUIC connections to edges |
| Progress tracking | Real-time progress, ETA, per-edge stats |
| Failure handling | Detect failures, trigger scheduler failover |

**Acceptance Criteria**:
- [ ] Download from 5+ sources simultaneously
- [ ] Swarm achieves >90% aggregate bandwidth
- [ ] Failures handled automatically
- [ ] Accurate progress reporting

**Dependencies**: Phase 8 GPU scheduler, Phase 7 edge registry

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Coordination overhead | Medium | Medium | Batch operations, efficient protocols |

---

### Phase 10: Auto-Reconciliation (Weeks 39-42)

**Goal**: Self-healing and predictive optimization.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Drift detection | Monitor actual vs expected performance |
| Reoptimization triggers | Detect when reschedule is beneficial |
| Incremental rescheduling | Reschedule without disruption |
| Predictive pre-positioning | Anticipate access, pre-fetch content |
| Time-aware scheduling | Optimize for time-of-day patterns |
| Cost-aware routing | Factor in metered connections |
| Power-aware transfers | Respect battery constraints |

**Acceptance Criteria**:
- [ ] Reoptimization improves transfer time >10%
- [ ] Pre-positioning reduces latency measurably
- [ ] Metered connections used only when necessary
- [ ] Battery devices not drained

**Dependencies**: Phase 9 orchestration, Phase 8 scheduler

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Prediction accuracy | Medium | Medium | Conservative predictions, fallback |

---

## 5. Production Phases

### Phase 11: Production Hardening (Weeks 43-46)

**Goal**: Production-ready reliability and security.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| Error handling | Comprehensive error types, recovery strategies |
| Logging and tracing | Structured logging, distributed tracing |
| Configuration management | File-based config, env vars, defaults |
| Security audit | External review of crypto implementation |
| Performance profiling | Identify and eliminate bottlenecks |
| Stress testing | Multi-day transfers, millions of files, chaos |
| Documentation | Architecture docs, API reference, deployment |

**Acceptance Criteria**:
- [ ] No panics under any tested condition
- [ ] Graceful degradation on resource exhaustion
- [ ] Security audit findings addressed
- [ ] Documentation complete and accurate

**Dependencies**: All previous phases

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Security vulnerabilities | High | Medium | Early auditor engagement |
| Documentation effort | Medium | Medium | Document as you go |

---

### Phase 12: Ecosystem & Tools (Weeks 47-48+)

**Goal**: User-facing tools and packaging.

**Deliverables**:
| Component | Description |
|-----------|-------------|
| CLI polish | User-friendly commands, shell completions |
| Web dashboard | Transfer monitoring, edge status, metrics |
| Mobile companion | iOS/Android app for enrollment, status |
| API documentation | OpenAPI spec, SDK documentation |
| Integration examples | Example integrations with common tools |
| Packaging | deb, rpm, brew, Docker images |

**Acceptance Criteria**:
- [ ] CLI intuitive for new users
- [ ] Dashboard provides real-time visibility
- [ ] Mobile app enables basic management
- [ ] Packages available for major platforms

**Dependencies**: Phase 11 production-ready core

**Risks**:
| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Scope creep | Medium | High | Strict MVP definition |

---

## 6. Crate Structure

### 6.1 Warp Crates

| Crate | Phase | Purpose |
|-------|-------|---------|
| `warp-core` | 1 | Common types, traits, error handling |
| `warp-crypto` | 1, 5 | ChaCha20, BLAKE3, convergent encryption |
| `warp-compress` | 1, 3 | CPU and GPU compression |
| `warp-hash` | 1, 3 | BLAKE3 CPU and GPU |
| `warp-io` | 1 | Chunking, directory walking |
| `warp-format` | 1-2 | Archive format, header, index |
| `warp-verify` | 2 | Merkle tree, proofs |
| `warp-gpu` | 3 | CUDA kernels, nvCOMP |
| `warp-stream` | 4 | Triple-buffer pipeline |
| `warp-net` | 6 | WireGuard, mDNS |
| `warp-proto` | 2, 6 | QUIC transport, protocol |
| `warp-cli` | 2, 12 | Command-line interface |

### 6.2 Portal Crates

| Crate | Phase | Purpose |
|-------|-------|---------|
| `portal-core` | 5 | Zero-knowledge model, lifecycle |
| `portal-hub` | 5 | Hub server implementation |
| `warp-edge` | 7 | Edge state, discovery, health |
| `warp-sched` | 8 | GPU chunk scheduler |
| `warp-orch` | 9 | Multi-source orchestration |
| `warp-metrics` | 7 | Bandwidth/RTT estimation |
| `warp-integrity` | 2, 5 | Merkle DAG, KZG commitments |
| `portal-cli` | 12 | Portal-specific CLI |

---

## 7. Success Metrics

### 7.1 Performance Targets

| Metric | Target | Phase |
|--------|--------|-------|
| GPU compression throughput | >15 GB/s | 3 |
| GPU encryption throughput | >20 GB/s | 3 |
| Stream mode latency | <5 ms | 4 |
| Scheduler tick time | <10 ms | 8 |
| Failover time | <50 ms | 8 |
| Multi-source efficiency | >90% aggregate | 9 |

### 7.2 Scale Targets

| Metric | Target | Phase |
|--------|--------|-------|
| Single transfer size | >100 TB | 2 |
| File count | >10 million | 2 |
| Chunk count | >1 billion | 8 |
| Edge count | >100 | 7 |
| Concurrent transfers | >1000 | 9 |

### 7.3 Reliability Targets

| Metric | Target | Phase |
|--------|--------|-------|
| Transfer success rate | >99.9% | 11 |
| Data integrity | 100% | 2 |
| Failover success rate | >99% | 8 |
| Roaming continuity | >99% | 6 |

### 7.4 Security Targets

| Metric | Target | Phase |
|--------|--------|-------|
| Zero-knowledge compliance | 100% | 5 |
| Key derivation strength | >128 bits | 5 |
| Forward secrecy | Yes | 6 |
| Critical vulnerabilities | 0 | 11 |

---

## 8. Resource Requirements

### 8.1 Development Environment

| Resource | Specification |
|----------|---------------|
| Development machines | 32+ GB RAM, NVIDIA GPU (RTX 3080+) |
| CI/CD infrastructure | GPU-enabled runners |
| Test network | Isolated 10 GbE for benchmarking |
| Cloud resources | GPU instances for multi-node testing |

### 8.2 Recommended Team

| Role | Count | Focus |
|------|-------|-------|
| Systems Engineer | 2 | Warp engine, GPU kernels |
| Network Engineer | 1 | WireGuard, QUIC, P2P mesh |
| Security Engineer | 1 | Crypto, zero-knowledge, audit |
| Full-Stack Engineer | 1 | Hub, CLI, web dashboard |
| DevOps Engineer | 1 | CI/CD, packaging, deployment |

### 8.3 External Resources

| Resource | Timing |
|----------|--------|
| Security audit firm | Phase 5, 11 |
| Technical writer | Phase 11-12 |
| UX designer | Phase 12 |

---

## 9. Risk Summary

### 9.1 Technical Risks

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| GPU memory limits | High | Medium | Multi-GPU, streaming |
| Scheduler complexity | High | Medium | Buffer time in Phase 8 |
| WireGuard platform issues | Medium | Low | Kernel + userspace support |
| nvCOMP compatibility | Medium | Medium | Pin versions, CI testing |

### 9.2 Schedule Risks

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Phase 8 underestimated | High | Medium | Add 2-week buffer |
| Security audit delays | Medium | Medium | Engage auditors early |
| Integration issues | Medium | Medium | Continuous integration |

### 9.3 External Risks

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Dependency vulnerabilities | Medium | Medium | Automated scanning |
| Hardware availability | Low | Low | Cloud GPU fallback |
| Regulatory requirements | Low | Low | Consult legal early |

---

## 10. Timeline Summary

| Month | Phase | Milestone |
|-------|-------|-----------|
| 1-2 | 1-2 | Archive format working |
| 2-3 | 3 | GPU acceleration integrated |
| 3-4 | 4 | **Warp Engine Complete** |
| 4-5 | 5 | Zero-knowledge Hub working |
| 5-6 | 6 | WireGuard mesh operational |
| 6-7 | 7 | Edge intelligence active |
| 7-9 | 8 | GPU scheduler operational |
| 9-10 | 9 | Swarm downloads working |
| 10-11 | 10 | **Portal Complete** |
| 11-12 | 11-12 | **Production Ready** |

---

## 11. Next Steps

1. **Immediate**: Set up development environment with RTX 5090
2. **Week 1**: Begin Phase 1 - binary format header
3. **Week 2**: BLAKE3 wrapper and Buzhash chunker
4. **Week 3**: Directory walker and CPU compressors
5. **Week 4**: Crypto primitives, Phase 1 integration testing

The foundation phase sets the trajectory for the entire project. Getting the primitives right enables everything that follows.
