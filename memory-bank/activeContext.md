# Active Context

## Current Focus
**Phase 10 Complete - Ready for Production Hardening** - Completed Phase 10 (Auto-Reconciliation) with all 6 features. Ready to begin Phase 11 (Production Hardening) or continue Phase 12 (Ecosystem).

### Milestone Achieved: Auto-Reconciliation Complete (2025-12-03)
- **Phase 10**: Auto-Reconciliation complete
  - Drift detection with EMA-based tracking
  - Reoptimization triggers (TriggerGenerator)
  - Incremental rescheduling (request_reopt in scheduler)
  - Predictive pre-positioning (PrepositionManager)
  - Time-aware scheduling (ConstraintEvaluator integration)
  - Cost/power-aware routing (constraint multipliers)

### Confirmed Strategy
- **Approach**: Sequential (complete Warp Engine before Portal) ✓
- **MVP**: Full system through GPU Chunk Scheduler (Phase 8) ✓
- **Team**: Solo development
- **Extensions**: After Phase 12 completion

### Next Priority: Phases 11-12
1. ~~Auto-Reconciliation (self-healing)~~ ✓ (Phase 10)
2. Production Hardening (Phase 11)
   - Error handling audit
   - Security audit
   - Performance profiling
   - Stress testing
   - Documentation
3. Ecosystem & Tools (Phase 12)
   - API documentation (OpenAPI)
   - Integration examples
   - Packaging (deb, rpm, brew, Docker)

## Recent Accomplishments (2025-12-03)

### Phase 10: Auto-Reconciliation - COMPLETE
- **triggers.rs** (warp-orch) - ~725 lines
  - TriggerConfig for configurable thresholds
  - TriggerGenerator monitors progress, health, load
  - Generates ReoptTrigger events for reconciliation loop
  - ~30 tests

- **scheduler.rs** modifications (warp-sched)
  - Added request_reopt() method
  - Pending reopt plan tracking
  - Reopt step execution in tick loop
  - Current assignments tracking for reoptimization

- **preposition.rs** (warp-orch) - ~900 lines
  - PrepositionConfig with rate limiting
  - PrepositionPlanner for demand prediction
  - PrepositionExecutor with concurrent transfer management
  - PrepositionManager coordinates planning/execution
  - ~30 tests

- **cost.rs** modifications (warp-sched)
  - Added set_cost() and invalidate() methods
  - ConstraintEvaluator integration

### Test Coverage
- warp-sched: 249 tests passing
- warp-orch: 271 tests passing
- ~1,500+ tests workspace-wide
- 20-crate workspace
- All files under 900 lines

## Next Steps

### Phase 11: Production Hardening (PARTIAL)
Already completed:
- [x] Logging/tracing (warp-telemetry, 104 tests)
- [x] Configuration management (warp-config, 61 tests)

Remaining:
- [ ] Error handling audit
- [ ] Security audit
- [ ] Performance profiling
- [ ] Stress testing
- [ ] Documentation

### Phase 12: Ecosystem & Tools (PARTIAL)
Already completed:
- [x] CLI polish (stream commands)
- [x] Web dashboard (warp-dashboard, 86 tests)
- [x] API server (warp-api, 55 tests)

Remaining:
- [ ] Mobile companion
- [ ] API documentation (OpenAPI)
- [ ] Integration examples
- [ ] Packaging (deb, rpm, brew, Docker)

## Active Decisions

### Decision Needed: Next Priority
**Options**:
1. **Production Hardening First**: Error handling, security audit, stress testing
2. **Ecosystem First**: API docs, packaging, integration examples
3. **Core Stubs First**: Complete WarpWriter/Reader, network integration

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

## Blocked Items
- **GPU Testing on Hardware**: Tests pass but actual GPU hardware needed for performance validation
- **Network Integration Tests**: Need test certificate infrastructure for QUIC
- **Large-scale Benchmarks**: Need multi-TB test dataset generation

## Known Technical Debt

1. **Merkle hash_pair uses placeholder XOR**
   - Location: `warp-format/src/merkle.rs:76-88`
   - Impact: Merkle verification will be incorrect
   - Fix: Replace XOR with `warp_hash::hash()`

2. **WarpReader/Writer are stubs**
   - Location: `warp-format/src/reader.rs`, `writer.rs`
   - Impact: Cannot create or read .warp archives
   - Fix: Implement full functionality

3. **Chunker window removal is O(n)**
   - Location: `warp-io/src/chunker.rs:74`
   - Fix: Use VecDeque or ring buffer

4. **Integration tests empty**
   - Location: `tests/integration.rs`
   - Fix: Add comprehensive end-to-end tests
