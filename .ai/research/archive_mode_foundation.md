# Research: Archive Mode Foundation

**Date**: 2025-12-02
**Objective**: Analyze existing implementation across all 8 crates to identify gaps and integration points for Archive Mode functionality.

---

## 1. Overview

### Current Implementation Summary
The warp project is a GPU-accelerated bulk data transfer tool with 8 Rust crates implementing a layered architecture. The codebase is approximately 50% implemented with solid foundational structures but many stub implementations requiring completion.

### Architecture Layers
```
warp-cli      → User interface (CLI commands)
warp-core     → Orchestration engine
warp-format   → .warp file format (archive container)
warp-net      → QUIC transport layer
warp-compress → Compression (CPU/GPU)
warp-hash     → BLAKE3 hashing
warp-crypto   → Encryption/signatures
warp-io       → File I/O, chunking, buffers
```

### Key Components Status Matrix

| Crate | Structures | Core Logic | Integration | Tests |
|-------|-----------|------------|-------------|-------|
| warp-crypto | COMPLETE | COMPLETE | Needs integration | Basic |
| warp-hash | COMPLETE | COMPLETE | Ready | Basic |
| warp-io | COMPLETE | ~70% | Ready | Basic |
| warp-compress | COMPLETE | COMPLETE (CPU) | Ready | Basic |
| warp-format | COMPLETE | ~30% (stubs) | Blocked | None |
| warp-net | ~70% | ~20% (stubs) | Blocked | None |
| warp-core | ~60% | ~10% (stubs) | Blocked | None |
| warp-cli | COMPLETE | ~20% (stubs) | Blocked | None |

---

## 2. Relevant Files Analysis

### warp-crypto (COMPLETE - Ready for Integration)
- `crates/warp-crypto/src/lib.rs`: Module exports, Error enum with thiserror
- `crates/warp-crypto/src/encrypt.rs`: Key struct with zeroize, ChaCha20-Poly1305 AEAD
- `crates/warp-crypto/src/sign.rs`: Ed25519 sign/verify wrappers
- `crates/warp-crypto/src/kdf.rs`: Argon2id password derivation

**Gap**: No X25519 key exchange implementation (declared but not used)

### warp-hash (COMPLETE - Ready)
- `crates/warp-hash/src/lib.rs`: hash(), hash_chunks_parallel(), keyed_hash(), derive_key(), Hasher struct

**Gap**: None - fully functional

### warp-io (MOSTLY COMPLETE)
- `crates/warp-io/src/chunker.rs`: Buzhash CDC with configurable min/target/max
- `crates/warp-io/src/walker.rs`: Directory traversal with metadata
- `crates/warp-io/src/mmap.rs`: Memory-mapped file I/O
- `crates/warp-io/src/pool.rs`: Buffer pooling

**Issues**:
- `chunker.rs:74` - window.remove(0) is O(n), should use VecDeque

### warp-compress (CPU COMPLETE, GPU STUB)
- `crates/warp-compress/src/lib.rs`: Compressor trait definition
- `crates/warp-compress/src/cpu/zstd.rs`: ZstdCompressor (levels 1-22)
- `crates/warp-compress/src/cpu/lz4.rs`: Lz4Compressor
- `crates/warp-compress/src/adaptive.rs`: Entropy-based algorithm selection

**Gap**: No GPU module (feature-gated, requires nvCOMP)

### warp-format (CRITICAL GAPS)
- `crates/warp-format/src/header.rs`: 256-byte header, flags, enums - COMPLETE
- `crates/warp-format/src/file_table.rs`: FileEntry, FileTable - NO SERIALIZATION
- `crates/warp-format/src/index.rs`: ChunkEntry (56 bytes), ChunkIndex - NO SERIALIZATION
- `crates/warp-format/src/merkle.rs`: MerkleTree - BROKEN (hash_pair uses XOR!)
- `crates/warp-format/src/reader.rs`: WarpReader - STUB
- `crates/warp-format/src/writer.rs`: WarpWriter - STUB

### warp-net (CRITICAL GAPS)
- `crates/warp-net/src/frames.rs`: Frame types, Capabilities - COMPLETE
- `crates/warp-net/src/protocol.rs`: State machine, NegotiatedParams - COMPLETE
- `crates/warp-net/src/transport.rs`: WarpConnection, WarpEndpoint - STUBS
- `crates/warp-net/src/listener.rs`: WarpListener - STUB

### warp-core (CRITICAL GAPS)
- `crates/warp-core/src/engine.rs`: TransferConfig, TransferEngine - STUBS
- `crates/warp-core/src/scheduler.rs`: ChunkScheduler with BinaryHeap - COMPLETE
- `crates/warp-core/src/analyzer.rs`: PayloadAnalysis - STUB
- `crates/warp-core/src/session.rs`: Session, SessionState - NO PERSISTENCE

### warp-cli (STUBS)
- `crates/warp-cli/src/main.rs`: CLI structure with clap - COMPLETE
- `crates/warp-cli/src/commands/*.rs`: ALL STUBS

---

## 3. Code Flow Analysis (Archive Mode Send)

### Target Flow (Not Yet Implemented)
```
1. CLI Entry: warp-cli/src/commands/send.rs:execute()
   └── Parse source/dest, create TransferConfig

2. Session Creation: warp-core/src/session.rs:Session::new()
   └── Generate session ID, set state to Created

3. Analysis: warp-core/src/analyzer.rs:analyze_payload()
   └── Walk directory (warp-io), sample files, estimate entropy
   └── Choose compression strategy (warp-compress/adaptive.rs)

4. Archive Creation (LOCAL - Archive Mode):
   └── warp-format/src/writer.rs:WarpWriter::create()
   └── For each file:
       └── warp-io/src/chunker.rs:Chunker::chunk()
       └── warp-hash/src/lib.rs:hash() per chunk
       └── warp-compress/src/lib.rs:Compressor::compress()
       └── (optional) warp-crypto/src/encrypt.rs:encrypt()
       └── Write chunk data, build index
   └── Build Merkle tree (warp-format/src/merkle.rs)
   └── Write header, file_table, index
   └── warp-format/src/writer.rs:WarpWriter::finish()

5. Network Transfer (if remote destination):
   └── warp-net connection/negotiation (future phase)
```

### Current Blockers for Archive Mode
1. **WarpWriter** cannot write chunks (stub)
2. **ChunkIndex/FileTable** have no serialization
3. **MerkleTree::hash_pair()** uses placeholder XOR, not BLAKE3
4. **No integration** between warp-io chunker and warp-format writer

---

## 4. Dependencies & Testing

### External Dependencies
| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1.48 | Async runtime |
| quinn | 0.11 | QUIC transport |
| zstd | 0.13 | Zstd compression |
| lz4_flex | 0.11 | LZ4 compression |
| blake3 | 1.8 | Hashing |
| chacha20poly1305 | 0.10 | AEAD encryption |
| rmp-serde | 1.3 | MessagePack serialization |
| memmap2 | 0.9 | Memory mapping |

### Test Coverage
- **warp-crypto**: 2 tests (roundtrip, sign/verify)
- **warp-hash**: 2 tests (hash, incremental)
- **warp-io**: 2 tests (chunker, walker)
- **warp-compress**: 2 tests (zstd, lz4)
- **warp-format**: 1 test (header roundtrip)
- **warp-net**: 0 tests
- **warp-core**: 0 tests
- **warp-cli**: 0 tests
- **Integration**: 1 placeholder test

---

## 5. Critical Integration Points

### Point 1: Chunker → Writer
- **Location**: warp-io/chunker.rs → warp-format/writer.rs
- **Gap**: Writer cannot consume chunks
- **Solution**: Add `add_chunk(&mut self, data: &[u8], hash: Hash)` method

### Point 2: Hash → Merkle
- **Location**: warp-hash → warp-format/merkle.rs
- **Gap**: Merkle uses placeholder hash
- **Solution**: Import warp_hash::hash and use in hash_pair()

### Point 3: Compress → Chunk Storage
- **Location**: warp-compress → warp-format/index.rs
- **Gap**: No way to mark chunks as compressed
- **Solution**: ChunkEntry.flags already has bit 0x01, need setter

### Point 4: Index → Mmap
- **Location**: warp-format/index.rs → warp-io/mmap.rs
- **Gap**: No memory-mapped index access
- **Solution**: Add Index::from_mmap(slice: &[u8]) method

---

## 6. Recommendations

### Immediate Priority (Phase 1)
1. **Fix MerkleTree hash_pair()** - Use warp_hash::hash
2. **Add serialization to FileTable** - MessagePack via rmp-serde
3. **Add serialization to ChunkIndex** - Binary format, same layout as memory
4. **Implement WarpWriter core** - Chunk writing, index building
5. **Implement WarpReader core** - Header reading, chunk extraction

### Suggested Approach
1. Start with warp-format since it's the core container format
2. Work bottom-up: merkle fix → index serialization → writer impl
3. Add integration test: create archive → read back → verify

### Files Needing Modification (Phase 1)
| File | Changes | Priority |
|------|---------|----------|
| warp-format/src/merkle.rs | Use warp_hash | P0 |
| warp-format/src/index.rs | Add to_bytes/from_bytes | P0 |
| warp-format/src/file_table.rs | Add serialization | P0 |
| warp-format/src/writer.rs | Full implementation | P0 |
| warp-format/src/reader.rs | Full implementation | P1 |
| warp-format/Cargo.toml | Add warp-hash dep | P0 |

---

## Context Notes for Future Sessions

### Keep (Critical)
- 8 crate architecture with dependency DAG
- warp-format is the critical blocker
- Merkle hash_pair() is broken (uses XOR)
- Error patterns: thiserror + per-crate Result

### Drop (Completed Details)
- Specific line numbers (may shift)
- Full dependency list (in Cargo.toml)
