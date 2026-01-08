# Implementation Plan: Archive Mode Foundation

**Date**: 2025-12-02
**Research Reference**: `.ai/research/001_archive_mode_foundation.md`
**Objective**: Enable local .warp archive creation and reading (Archive Mode) as foundation for network transfer.

---

## Overview

Archive Mode allows creating and extracting .warp files locally without network transfer. This is the foundation for all transfer operations since even network transfers ultimately work with the .warp format.

### Success Criteria
1. `warp-format` can create valid .warp archives from directories
2. `warp-format` can extract .warp archives back to filesystem
3. Merkle tree verification works end-to-end
4. Compression/encryption can be enabled per-archive
5. Basic CLI commands work: `warp send ./dir /local/archive.warp`

---

## Phase 1: Core Infrastructure

**Goal**: Fix foundational issues and implement serialization

### Task 1.1: Fix Merkle Tree Hash Function
- **File**: `crates/warp-format/src/merkle.rs`
- **Change**: Replace placeholder XOR with BLAKE3 from warp-hash
- **Implementation**:
  ```rust
  fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
      let mut combined = [0u8; 64];
      combined[..32].copy_from_slice(left);
      combined[32..].copy_from_slice(right);
      warp_hash::hash(&combined)
  }
  ```

### Task 1.2: Add warp-hash Dependency to warp-format
- **File**: `crates/warp-format/Cargo.toml`
- **Change**: Add `warp-hash.workspace = true` to dependencies

### Task 1.3: Implement ChunkIndex Serialization
- **File**: `crates/warp-format/src/index.rs`
- **Change**: Add binary serialization (fixed-size entries)
- **Implementation**: `ChunkEntry::to_bytes()` / `from_bytes()`, `ChunkIndex::to_bytes()` / `from_bytes()`

### Task 1.4: Implement FileTable Serialization
- **File**: `crates/warp-format/src/file_table.rs`
- **Change**: Add MessagePack serialization via rmp-serde

---

## Phase 2: WarpWriter Implementation

**Goal**: Enable creating .warp archives

### Task 2.1: Implement WarpWriter Internal State
- **File**: `crates/warp-format/src/writer.rs`
- **Change**: Add proper internal state (file, header, index, file_table, chunk_hashes)

### Task 2.2: Implement WarpWriter::create()
- **File**: `crates/warp-format/src/writer.rs`
- **Change**: Initialize writer, reserve header space

### Task 2.3: Implement add_chunk() Method
- Write chunk data to file
- Add entry to ChunkIndex
- Track hash for Merkle tree

### Task 2.4: Implement add_file() Method
- Chunk file using warp-io Chunker
- Compress each chunk
- Add to archive via add_chunk()

### Task 2.5: Implement add_directory() Method
- Walk directory using warp-io Walker
- Call add_file for each file

### Task 2.6: Implement finish() Method
- Build Merkle tree from chunk hashes
- Write index and file_table
- Update and write header

---

## Phase 3: WarpReader Implementation

**Goal**: Enable reading and extracting .warp archives

### Task 3.1: Implement WarpReader State
- Memory-mapped archive file
- Parsed header, index, file_table

### Task 3.2: Implement WarpReader::open()
- Parse header, validate magic bytes
- Load index and file_table from mmap

### Task 3.3: Implement Chunk Access Methods
- `chunk_count()`, `chunk_entry()`, `chunk_data()`

### Task 3.4: Implement extract_file()
- Find file entry, decompress chunks, write to dest

### Task 3.5: Implement extract_all()
- Iterate file_table, create dirs, extract each

### Task 3.6: Implement Merkle Verification
- Rehash all chunks, rebuild tree, compare root

---

## Phase 4: CLI Integration

**Goal**: Wire up warp-cli send/fetch for local archive mode

### Task 4.1: Update warp-cli Dependencies
### Task 4.2: Implement Local Archive Detection
### Task 4.3: Implement Local Send (Archive Creation)
### Task 4.4: Implement Local Fetch (Archive Extraction)
### Task 4.5: Implement Plan Command

---

## Phase 5: GPU Acceleration Foundation

**Goal**: Add nvCOMP integration for GPU compression

### Task 5.1: Add GPU Feature Flag
### Task 5.2: Create GPU Module Structure
### Task 5.3: Implement GPU Detection
### Task 5.4: Implement nvCOMP LZ4 Wrapper
### Task 5.5: Implement Batch Compression
### Task 5.6: Integrate GPU into Pipeline

---

## Phase 6: Network Layer (QUIC)

**Goal**: Implement QUIC transport with quinn

### Task 6.1: TLS Certificate Generation
### Task 6.2: Implement WarpEndpoint
### Task 6.3: Implement WarpConnection
### Task 6.4: Implement WarpListener
### Task 6.5: Implement Frame Handlers
### Task 6.6: Implement Handshake Protocol
### Task 6.7: Implement Chunk Transfer

---

## Phase 7: Transfer Engine Integration

**Goal**: Wire everything together in warp-core

### Task 7.1: Implement Real Payload Analyzer
### Task 7.2: Implement TransferEngine::send
### Task 7.3: Implement TransferEngine::fetch
### Task 7.4: Implement Session Persistence
### Task 7.5: Implement Resume Logic

---

## Phase 8: Integration Testing

**Goal**: Comprehensive test coverage

### Task 8.1: Archive Roundtrip Tests
### Task 8.2: Compression Tests
### Task 8.3: Network Transfer Tests
### Task 8.4: Benchmark Suite

---

## Risk Assessment

### High Risk
- **GPU Integration**: Requires CUDA setup, may fail on CI
  - Mitigation: Feature-gate, CPU fallback
- **QUIC Complexity**: quinn API is complex
  - Mitigation: Start with simple single-stream

### Medium Risk
- **Large File Handling**: Memory pressure with big files
  - Mitigation: Streaming design, buffer pools

### Low Risk
- **Serialization Format**: MessagePack is well-tested
- **Compression Libraries**: zstd/lz4 are stable

---

## Implementation Notes

### Context Management
- Each phase should fit in 1-2 sessions
- Save progress after each task
- If context > 40%, create continuation

### Testing Approach
- Write tests alongside implementation
- Use proptest for roundtrip tests
- Integration tests after each phase

### Commit Strategy
- Commit after each task completes
- Tag after each phase: `v0.1.0-phase1`, etc.
