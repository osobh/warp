# Master Implementation Plan: Warp + Portal

**Date**: 2025-12-02
**Scope**: Full 12-phase implementation of Warp Engine and Portal distributed storage fabric
**Reference Docs**: `/warp-portal/*.md`

---

## Strategy Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Approach** | Sequential | Complete Warp Engine (Phases 1-4) before Portal |
| **MVP Scope** | Full Scheduler | Build through GPU Chunk Scheduler (Phase 8) |
| **Team Size** | Solo | Focus on one phase at a time |
| **Extensions** | After Phase 12 | Complete full system before monetization |

---

## Phase Summary

| Phase | Name | Status | Key Deliverable |
|-------|------|--------|-----------------|
| 0 | Planning | COMPLETE | This document |
| 1-2 | Foundation Gaps | COMPLETE | Streaming hash, async chunking |
| 3 | GPU Acceleration | COMPLETE | GPU BLAKE3, ChaCha20 kernels |
| 4 | Stream Mode | COMPLETE | Triple-buffer pipeline |
| **M1** | **Warp Engine Complete** | **COMPLETE** | 2025-12-02 |
| 5 | Portal Core | 3-4 weeks | Zero-knowledge, key hierarchy |
| 6 | Network Layer | 3-4 weeks | WireGuard mesh, mDNS |
| 7 | Edge Intelligence | 2-3 weeks | Health scoring, bandwidth estimation |
| 8 | GPU Scheduler | 4-6 weeks | 10M chunks in <10ms |
| **M2** | **Portal MVP Complete** | | |
| 9 | Orchestration | 3-4 weeks | Swarm downloads |
| 10 | Auto-Reconciliation | 2-3 weeks | Self-healing |
| **M3** | **Portal Complete** | | |
| 11 | Production | 3-4 weeks | Hardening, audit |
| 12 | Ecosystem | 2-4 weeks | Dashboard, packaging |
| **M4** | **Production Ready** | | |

---

## Detailed Phase Plans

### Phase 1-2: Foundation Gaps

**Goal**: Fill gaps in current Warp implementation

| Task | Crate | Description |
|------|-------|-------------|
| Streaming hash | warp-hash | Hash large files without loading into memory |
| Async chunking | warp-io | Tokio-based async chunking |
| Dictionary compression | warp-compress | Pre-trained dictionaries |
| Batch compression API | warp-compress | Process multiple chunks together |

**Acceptance Criteria**:
- [ ] Hash 100GB+ files with bounded memory
- [ ] Async chunking with tokio integration
- [ ] Batch API for GPU pipeline preparation

---

### Phase 3: GPU Acceleration - COMPLETE

**Goal**: 15+ GB/s compression, 20+ GB/s encryption

**New Crate**: `warp-gpu` (implemented)

| Task | File | Status |
|------|------|--------|
| CUDA context | warp-gpu/src/context.rs | COMPLETE |
| Memory pool | warp-gpu/src/memory.rs | COMPLETE |
| GPU BLAKE3 | warp-gpu/src/blake3.rs | COMPLETE (885 lines) |
| GPU ChaCha20 | warp-gpu/src/chacha20.rs | COMPLETE (885 lines) |
| Pinned buffers | warp-gpu/src/pooled.rs | COMPLETE |
| Stream management | warp-gpu/src/stream.rs | COMPLETE |

**Acceptance Criteria**:
- [x] GPU BLAKE3 kernel implemented
- [x] GPU ChaCha20 kernel implemented
- [x] Graceful CPU fallback
- [x] Pinned memory pool
- [x] 65 tests passing
- [ ] Performance validation on hardware (pending)

---

### Phase 4: Stream Mode - COMPLETE

**Goal**: <5ms latency real-time encrypted streaming

**New Crate**: `warp-stream` (implemented)

| Task | File | Status |
|------|------|--------|
| Triple-buffer | warp-stream/src/pipeline.rs | COMPLETE (508 lines) |
| Fixed chunking | warp-io/src/fixed_chunker.rs | COMPLETE (373 lines) |
| Stream encrypt | warp-crypto/src/stream.rs | COMPLETE (466 lines) |
| GPU crypto | warp-stream/src/gpu_crypto.rs | COMPLETE (311 lines) |
| Backpressure | warp-stream/src/flow.rs | COMPLETE (397 lines) |
| Pooled buffers | warp-stream/src/pooled.rs | COMPLETE (348 lines) |
| Statistics | warp-stream/src/stats.rs | COMPLETE (368 lines) |
| Benchmarks | warp-stream/benches/encryption.rs | COMPLETE |

**Acceptance Criteria**:
- [x] Triple-buffer pipeline implemented
- [x] Fixed-size chunking for streaming
- [x] Streaming encryption with nonce derivation
- [x] GPU crypto integration with CPU fallback
- [x] Backpressure handling
- [x] 61 tests passing
- [ ] Performance validation on hardware (pending)

---

### Phase 5: Portal Core

**Goal**: Zero-knowledge encryption and portal lifecycle

**New Crate**: `portal-core`

| Task | File | Description |
|------|------|-------------|
| Key hierarchy | portal-core/src/keys.rs | Master seed → derived keys |
| BIP-39 | portal-core/src/recovery.rs | 24-word mnemonic |
| Convergent enc | portal-core/src/convergent.rs | Content-addressed encryption |
| Portal struct | portal-core/src/portal.rs | Portal state and metadata |
| Lifecycle | portal-core/src/lifecycle.rs | Create, activate, expire |
| Access control | portal-core/src/access.rs | Owner, guest, link-based |

**New Crate**: `portal-hub`

| Task | File | Description |
|------|------|-------------|
| Hub server | portal-hub/src/server.rs | axum-based API |
| Registration | portal-hub/src/registration.rs | Edge enrollment |
| Auth | portal-hub/src/auth.rs | Token-based auth |
| Metadata sync | portal-hub/src/sync.rs | Portal metadata storage |

**Acceptance Criteria**:
- [ ] Hub cannot decrypt user data
- [ ] Portals expire per policy
- [ ] Access control enforced
- [ ] Recovery phrase works

---

### Phase 6: Network Layer

**Goal**: WireGuard P2P mesh

| Task | File | Description |
|------|------|-------------|
| WG interface | warp-net/src/wireguard.rs | boringtun integration |
| Peer config | warp-net/src/peers.rs | Add/remove peers |
| Virtual IP | warp-net/src/vip.rs | 10.portal.0.0/16 subnet |
| mDNS | warp-net/src/mdns.rs | LAN discovery |
| Hub relay | portal-hub/src/relay.rs | Fallback relay |
| Roaming | warp-net/src/roaming.rs | Auto endpoint updates |

**Acceptance Criteria**:
- [ ] Edges connect via WireGuard
- [ ] LAN peers discovered
- [ ] Seamless roaming
- [ ] Hub relay fallback

---

### Phase 7: Edge Intelligence

**Goal**: Track edge state and health

**New Crate**: `warp-edge`

| Task | File | Description |
|------|------|-------------|
| Edge registry | warp-edge/src/registry.rs | Track all edges |
| Chunk map | warp-edge/src/chunks.rs | Which edges have what |
| Bandwidth | warp-edge/src/bandwidth.rs | EMA estimation |
| RTT | warp-edge/src/rtt.rs | SRTT/RTTVAR |
| Health score | warp-edge/src/health.rs | Composite score |
| Constraints | warp-edge/src/constraints.rs | Metered, battery |

**Acceptance Criteria**:
- [ ] Bandwidth converges in 5 samples
- [ ] Health predicts failures
- [ ] Constraints respected

---

### Phase 8: GPU Chunk Scheduler

**Goal**: Real-time scheduling of billions of chunks

**New Crate**: `warp-sched`

| Task | File | Description |
|------|------|-------------|
| State buffers | warp-sched/src/buffers.rs | GPU data structures |
| Cost matrix | warp-sched/src/cost.cu | CUDA cost calculation |
| K-best paths | warp-sched/src/kbest.cu | Top K sources |
| Failover | warp-sched/src/failover.cu | Detect, switch |
| Load balance | warp-sched/src/balance.cu | Redistribute |
| Dispatch | warp-sched/src/dispatch.rs | CPU-readable output |
| Tick loop | warp-sched/src/tick.rs | 50ms cycle |

**Acceptance Criteria**:
- [ ] Schedule 10M chunks in <10ms GPU
- [ ] Failover <50ms
- [ ] Load balancing prevents saturation
- [ ] Scales to 1B+ with multi-GPU

---

### Phase 9: Transfer Orchestration

**Goal**: Multi-source swarm downloads

**New Crate**: `warp-orch`

| Task | File | Description |
|------|------|-------------|
| Orchestrator | warp-orch/src/orchestrator.rs | Coordinate transfers |
| Swarm | warp-orch/src/swarm.rs | BitTorrent-style fetch |
| Upload | warp-orch/src/upload.rs | Multi-edge push |
| Pool | warp-orch/src/pool.rs | QUIC connections |
| Progress | warp-orch/src/progress.rs | Real-time stats |
| Failure | warp-orch/src/failure.rs | Scheduler failover |

**Acceptance Criteria**:
- [ ] 5+ simultaneous sources
- [ ] >90% aggregate bandwidth
- [ ] Automatic failure handling

---

### Phase 10: Auto-Reconciliation

**Goal**: Self-healing and predictive optimization

| Task | File | Description |
|------|------|-------------|
| Drift | warp-orch/src/drift.rs | Monitor vs expected |
| Triggers | warp-sched/src/triggers.rs | When to reschedule |
| Incremental | warp-sched/src/incremental.rs | Non-disruptive |
| Pre-position | warp-orch/src/preposition.rs | Anticipate access |
| Time-aware | warp-sched/src/time.rs | Time-of-day |
| Cost-aware | warp-sched/src/cost_aware.rs | Metered, battery |

**Acceptance Criteria**:
- [ ] Reoptimization improves >10%
- [ ] Pre-positioning reduces latency
- [ ] Metered connections respected

---

### Phase 11: Production Hardening

| Task | Description |
|------|-------------|
| Error handling | Comprehensive types, recovery |
| Logging | Structured, distributed tracing |
| Configuration | File-based, env vars |
| Security audit | External crypto review |
| Performance | Profiling, bottleneck elimination |
| Stress testing | Multi-day, chaos testing |
| Documentation | Architecture, API |

---

### Phase 12: Ecosystem & Tools

| Task | Description |
|------|-------------|
| CLI polish | Shell completions |
| Web dashboard | Monitoring UI |
| Mobile companion | iOS/Android |
| API documentation | OpenAPI |
| Integration examples | Common tools |
| Packaging | deb, rpm, brew, Docker |

---

## Crate Dependencies

```
portal-cli
├── portal-core
│   ├── warp-crypto
│   ├── warp-hash
│   └── warp-format
├── portal-hub
│   ├── portal-core
│   ├── warp-net
│   └── axum
├── warp-edge
│   ├── warp-net
│   └── warp-metrics
├── warp-sched
│   ├── warp-edge
│   └── warp-gpu
├── warp-orch
│   ├── warp-sched
│   ├── warp-edge
│   └── warp-net
└── warp-cli
    ├── warp-core
    └── warp-format
```

---

## Risk Assessment

### High Risk
| Risk | Impact | Mitigation |
|------|--------|------------|
| GPU memory limits | High | Multi-GPU, streaming |
| Scheduler complexity | High | Buffer time in Phase 8 |

### Medium Risk
| Risk | Impact | Mitigation |
|------|--------|------------|
| WireGuard platform issues | Medium | Kernel + userspace |
| nvCOMP compatibility | Medium | Pin versions |
| Security vulnerabilities | Medium | Early audit engagement |

### Low Risk
| Risk | Impact | Mitigation |
|------|--------|------------|
| Dependency vulnerabilities | Low | Automated scanning |
| Hardware availability | Low | Cloud GPU fallback |

---

## Success Metrics

| Metric | Target | Phase |
|--------|--------|-------|
| GPU compression | >15 GB/s | 3 |
| GPU encryption | >20 GB/s | 3 |
| Stream latency | <5 ms | 4 |
| Scheduler tick | <10 ms | 8 |
| Failover | <50 ms | 8 |
| Swarm efficiency | >90% | 9 |
| Transfer success | >99.9% | 11 |
| Zero-knowledge | 100% | 5 |
