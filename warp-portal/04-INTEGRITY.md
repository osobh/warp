# Integrity Layer

> Three-Layer Cryptographic Verification at Petabyte Scale

---

## 1. Overview

Portal implements a three-layer integrity architecture designed for:

- **Auditability**: Prove data integrity to external parties
- **Verifiability**: Detect any corruption at any scale
- **Security**: Cryptographic guarantees, not trust assumptions
- **Reliability**: Self-healing with automatic corruption repair
- **Performance**: GPU-accelerated for petabyte-scale operations

The three layers work together, each serving different use cases while sharing a common foundation.

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    THREE-LAYER INTEGRITY ARCHITECTURE                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│    ┌───────────────────────────────────────────────────────────────────┐   │
│    │                     LAYER 3: KZG COMMITMENTS                      │   │
│    │                                                                    │   │
│    │  • 48-byte commitment per file (constant size)                    │   │
│    │  • Constant-time proofs (48 bytes regardless of file size)        │   │
│    │  • Aggregatable across multiple chunks/files                      │   │
│    │  • External audit interface                                       │   │
│    │                                                                    │   │
│    │  USE: Audit, compliance, compact external proofs                  │   │
│    └───────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              │ Computed from                                │
│                              ▼                                              │
│    ┌───────────────────────────────────────────────────────────────────┐   │
│    │                     LAYER 2: MERKLE DAG                           │   │
│    │                                                                    │   │
│    │  • Content-addressed node storage                                 │   │
│    │  • Automatic deduplication (same content = same node)             │   │
│    │  • Cross-file chunk sharing                                       │   │
│    │  • Efficient versioning (share unchanged subtrees)                │   │
│    │  • Virtual tree extraction for proofs                             │   │
│    │                                                                    │   │
│    │  USE: Internal integrity, corruption localization, sync           │   │
│    └───────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              │ Built from                                   │
│                              ▼                                              │
│    ┌───────────────────────────────────────────────────────────────────┐   │
│    │                     LAYER 1: BLAKE3 CONTENT HASHES                │   │
│    │                                                                    │   │
│    │  • ChunkID = BLAKE3(content) for every chunk                      │   │
│    │  • Content addressing (same content = same ID)                    │   │
│    │  • GPU-accelerated (70+ GB/s on RTX 5090)                         │   │
│    │  • Foundation for deduplication                                   │   │
│    │                                                                    │   │
│    │  USE: Day-to-day verification, dedup, transfer integrity          │   │
│    └───────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Layer 1: BLAKE3 Content Addressing

### 3.1 Purpose

Every chunk is identified by its BLAKE3 hash:

```
ChunkID = BLAKE3(chunk_content)   # 32 bytes
```

This provides:
- **Unique identification**: Same content → same ID, always
- **Integrity verification**: Hash mismatch = corruption detected
- **Content addressing**: Fetch chunk by ID, verify on receipt
- **Deduplication key**: Same ID across system = same content

### 3.2 BLAKE3 Characteristics

| Property | Value | Benefit |
|----------|-------|---------|
| Output size | 256 bits (32 bytes) | Compact identifiers |
| Speed (CPU) | 5+ GB/s | Fast verification |
| Speed (GPU) | 70+ GB/s | Petabyte-scale processing |
| Parallelism | Tree-based internal structure | Perfect for GPU |
| Security | 128-bit collision resistance | Cryptographically secure |

### 3.3 GPU Implementation

BLAKE3 is internally a Merkle tree, making it naturally GPU-parallel:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       GPU BLAKE3 PARALLELIZATION                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Input chunk (4 MB):                                                        │
│  [Block₀] [Block₁] [Block₂] ... [Block₄₀₉₅]   (1 KB each)                   │
│      │        │        │            │                                       │
│      ▼        ▼        ▼            ▼                                       │
│   ┌──────────────────────────────────────┐                                  │
│   │     GPU: 4096 parallel leaf hashes   │   ← All blocks hashed together   │
│   └──────────────────────────────────────┘                                  │
│      │        │        │            │                                       │
│      └───┬────┘        └────┬───────┘                                       │
│          │                  │                                               │
│   ┌──────────────────────────────────────┐                                  │
│   │     GPU: 2048 parallel parent hashes │   ← Tree levels in parallel      │
│   └──────────────────────────────────────┘                                  │
│          │                  │                                               │
│         ...                ...                                              │
│          │                  │                                               │
│          └────────┬─────────┘                                               │
│                   │                                                         │
│                [Root]  ← ChunkID                                            │
│                                                                             │
│  Multiple chunks processed simultaneously:                                  │
│  • Batch 1000 chunks → 4M parallel hash operations                          │
│  • RTX 5090: Process entire batch in milliseconds                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.4 Verification Flow

```
Receive chunk from network:
1. Compute: actual_cid = BLAKE3(received_data)
2. Compare: actual_cid == expected_cid?
3. If match: Chunk is authentic and uncorrupted
4. If mismatch: Reject, request from different source
```

---

## 4. Layer 2: Merkle DAG

### 4.1 Why DAG Over Tree?

| Aspect | Merkle Tree | Merkle DAG |
|--------|-------------|------------|
| Structure | Strict binary hierarchy | Flexible graph |
| Duplicate content | Stored multiple times | Stored once, referenced many |
| Cross-file sharing | Not possible | Natural |
| Versioning | Full copy per version | Share unchanged subtrees |
| Updates | Rebuild affected paths | Append new nodes only |
| Memory (1 PB, 50% dedup) | 16 GB | 8.5 GB |

For a storage system with deduplication, versioning, and multi-file operations, DAG is superior.

### 4.2 DAG Structure

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         MERKLE DAG STRUCTURE                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                    [User Root]                                              │
│                   /     |     \                                             │
│                  /      |      \                                            │
│          [Portal 1] [Portal 2] [Portal 3]                                   │
│              |          |          |                                        │
│         ┌────┴────┐     │     ┌────┴────┐                                   │
│         │         │     │     │         │                                   │
│     [Dir A]   [Dir B]  ...  [File X] [File Y]                               │
│      /   \       |                     |                                    │
│ [File1] [File2] [File3]               ...                                   │
│    |       |       |                                                        │
│   ...     ...     ...                                                       │
│    │       │       │                                                        │
│    └───────┼───────┘                                                        │
│            │                                                                │
│     ┌──────┼──────┬──────┐                                                  │
│     │      │      │      │                                                  │
│  [Chunk] [Chunk] [Chunk] [Chunk]                                            │
│     A       B       C       A     ← Same chunk A, single storage            │
│     │                       │                                               │
│     └───────────────────────┘     ← Multiple references, one node           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.3 Node Types

**Chunk Node** (Leaf):
```
ChunkNode:
  type:     CHUNK
  cid:      [u8; 32]      # BLAKE3(content)
  size:     u32           # Chunk size in bytes
```

**File Node** (Internal):
```
FileNode:
  type:     FILE
  children: Vec<(offset, cid)>   # Ordered chunk references
  size:     u64                  # Total file size
  metadata: EncryptedBlob        # Name, permissions, etc.
```

**Directory Node** (Internal):
```
DirectoryNode:
  type:     DIRECTORY
  children: Vec<(name, cid)>     # Named entries
  metadata: EncryptedBlob        # Permissions, etc.
```

**Portal Node** (Root):
```
PortalNode:
  type:        PORTAL
  portal_id:   UUID
  root:        cid               # Content root node
  policy:      EncryptedBlob     # Lifecycle policy
  kzg_commit:  [u8; 48]          # KZG commitment
  merkle_root: [u8; 32]          # Virtual tree root
```

### 4.4 Content Addressing Creates DAG Automatically

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                   CONTENT ADDRESSING → DAG                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  When storing any node:                                                     │
│                                                                             │
│  1. Compute: node_id = BLAKE3(serialize(node))                              │
│                                                                             │
│  2. Check: Does node_id exist in store?                                     │
│     • YES → Return existing reference (DAG edge created)                    │
│     • NO  → Store new node, return reference                                │
│                                                                             │
│  Result: Identical content automatically shares storage.                    │
│                                                                             │
│  Example:                                                                   │
│  • File A contains chunk with content "Hello World"                         │
│  • File B also contains "Hello World"                                       │
│  • Both reference same chunk node (one storage, two references)             │
│                                                                             │
│  This is exactly how Git and IPFS work.                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.5 Virtual Tree Extraction

For Merkle proofs, we extract a virtual tree from the DAG:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      VIRTUAL TREE FROM DAG                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  DAG Storage:                                                               │
│                                                                             │
│  Nodes: { A, B, C, D, E }  (5 unique chunks)                                │
│  File1: [A, B, C, A, D]    (references A twice)                             │
│                                                                             │
│  Virtual Tree for File1:                                                    │
│                                                                             │
│                    [Root₁]                                                  │
│                   /       \                                                 │
│               [H01]       [H234]                                            │
│               /   \       /    \                                            │
│             [A]  [B]   [H23]   [D]                                          │
│                        /    \                                               │
│                      [C]   [A]    ← A appears twice in tree                 │
│                                                                             │
│  • Tree is computed, not stored                                             │
│  • Standard Merkle proofs work on virtual tree                              │
│  • DAG benefits (dedup) + Tree benefits (proofs)                            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.6 GPU DAG Operations

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       GPU DAG IMPLEMENTATION                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  GPU DATA STRUCTURES:                                                       │
│                                                                             │
│  1. Hash Table (cuCollections):                                             │
│     Key: [u8; 32] (Node CID)                                                │
│     Value: u32 (Index into node array)                                      │
│     Operations: O(1) insert, lookup, contains                               │
│                                                                             │
│  2. Node Array:                                                             │
│     Compact storage of all unique nodes                                     │
│     Indexed by hash table                                                   │
│                                                                             │
│  3. Edge List:                                                              │
│     (parent_idx, child_idx, position)                                       │
│     Enables forward and reverse traversal                                   │
│                                                                             │
│  CONSTRUCTION PIPELINE:                                                     │
│                                                                             │
│  Stage 1: Hash chunks (GPU BLAKE3)                                          │
│  Stage 2: Batch dedup check (GPU hash table lookup)                         │
│  Stage 3: Compact new nodes (GPU prefix sum)                                │
│  Stage 4: Batch insert (GPU hash table insert)                              │
│  Stage 5: Build file structure (create internal nodes)                      │
│                                                                             │
│  All stages run on GPU with minimal CPU involvement.                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 5. Layer 3: KZG Polynomial Commitments

### 5.1 Why KZG?

KZG provides capabilities impossible with Merkle alone:

| Feature | Merkle | KZG |
|---------|--------|-----|
| Commitment size | 32 bytes (root) | 48 bytes |
| Single proof size | O(log n) ≈ 900 bytes | 48 bytes (constant) |
| Batch proof size | O(k × log n) | 48 bytes (constant!) |
| Aggregation | Not possible | Natural |
| Verification time | O(k × log n) hashes | O(1) pairings |

For external audits of petabyte archives, KZG is transformative.

### 5.2 How KZG Works

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          KZG OVERVIEW                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  SETUP (one-time):                                                          │
│  • Structured Reference String (SRS) from trusted ceremony                  │
│  • Use existing (Ethereum, Filecoin) or run private ceremony                │
│                                                                             │
│  COMMIT:                                                                    │
│  • Interpret chunk CIDs as polynomial coefficients                          │
│  • p(x) where p(i) encodes chunk[i]'s CID                                   │
│  • Commitment C = [p(s)]₁  (elliptic curve point, 48 bytes)                 │
│                                                                             │
│  PROVE:                                                                     │
│  • For chunk at position i with CID v:                                      │
│  • Compute quotient q(x) = (p(x) - v) / (x - i)                             │
│  • Proof π = [q(s)]₁  (48 bytes)                                            │
│                                                                             │
│  VERIFY:                                                                    │
│  • Check pairing equation:                                                  │
│  • e(C - [v]₁, [1]₂) = e(π, [s - i]₂)                                       │
│  • Two pairings, constant time regardless of file size                      │
│                                                                             │
│  AGGREGATE:                                                                 │
│  • Multiple proofs → single 48-byte aggregated proof                        │
│  • Verify 1 chunk or 1 million: same 48-byte proof, same verification       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.3 Audit Scenario Comparison

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    AUDIT: 10 PB ARCHIVE, 10,000 RANDOM SAMPLES               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  WITH MERKLE ONLY:                                                          │
│  • Generate 10,000 proofs: 300 ms (GPU)                                     │
│  • Proof data: 10,000 × 900 bytes = 8.7 MB                                  │
│  • Auditor verifies: 10,000 × 28 hashes = 280,000 hashes                    │
│  • Auditor time: ~300 ms                                                    │
│                                                                             │
│  WITH KZG:                                                                  │
│  • Generate aggregated proof: 300 ms (GPU)                                  │
│  • Proof data: 48 bytes (!)                                                 │
│  • Auditor verifies: 1 pairing check                                        │
│  • Auditor time: ~5 ms                                                      │
│                                                                             │
│  KZG ADVANTAGE:                                                             │
│  • Proof size: 180,000× smaller                                             │
│  • Verification: 60× faster                                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.4 GPU KZG Implementation

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       GPU KZG OPERATIONS                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  COMMITMENT (1 PB file, 250M chunks):                                       │
│                                                                             │
│  Step 1: FFT                                                                │
│  • Convert chunk CIDs to polynomial evaluation form                         │
│  • GPU cuFFT: 250M-point NTT                                                │
│  • Time: ~2 seconds                                                         │
│                                                                             │
│  Step 2: Multi-Scalar Multiplication (MSM)                                  │
│  • Compute C = Σ cᵢ × Gᵢ for all coefficients                               │
│  • GPU parallel Pippenger algorithm                                         │
│  • Time: ~20 seconds                                                        │
│                                                                             │
│  Total commitment time: ~22 seconds for 1 PB                                │
│                                                                             │
│  PROOF GENERATION:                                                          │
│                                                                             │
│  • Single proof: ~5 ms                                                      │
│  • Batch 1000 proofs: ~50 ms (parallelized)                                 │
│  • Aggregate into single proof: ~10 ms                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.5 Trusted Setup Options

| Option | Description | Trust Model |
|--------|-------------|-------------|
| **Ethereum Ceremony** | 140,000+ participants | 1-of-N honest |
| **Filecoin Powers of Tau** | Larger SRS | 1-of-N honest |
| **Private Ceremony** | Organization runs own | Internal trust |
| **HSM-based** | Hardware generates and destroys | Trust hardware |

Recommendation: Use existing public ceremony for general use, private for enterprise.

---

## 6. Unified Integrity Pipeline

### 6.1 Ingest Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     INTEGRITY INGEST PIPELINE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Input: Stream of file data                                                 │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ STAGE 1: CHUNKING                                                   │   │
│  │ Content-defined chunking → Variable-size chunks                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ STAGE 2: BLAKE3 HASHING (GPU)                              [LAYER 1]│   │
│  │ Parallel chunk hashing → Content IDs                                │   │
│  │ Throughput: 70+ GB/s on RTX 5090                                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ STAGE 3: DAG CONSTRUCTION (GPU)                            [LAYER 2]│   │
│  │ Dedup check → Insert new nodes → Build file structure               │   │
│  │ Uses GPU hash table for O(1) dedup                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ STAGE 4: KZG COMMITMENT (GPU)                              [LAYER 3]│   │
│  │ FFT → MSM → 48-byte commitment                                      │   │
│  │ Time: ~22 seconds for 1 PB (after all chunks processed)             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  Output: IntegrityRecord                                                    │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ merkle_root:     [u8; 32]   # Virtual tree root from DAG            │   │
│  │ kzg_commitment:  [u8; 48]   # KZG commitment                        │   │
│  │ chunk_count:     u64                                                │   │
│  │ signature:       [u8; 64]   # Ed25519 over above                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Time Budget (1 PB file)

| Stage | Time | Notes |
|-------|------|-------|
| Read from storage | ~3 hours | 100 GB/s parallel NVMe |
| BLAKE3 hashing | ~5.5 hours | 70 GB/s GPU (overlapped with read) |
| DAG construction | ~4 seconds | GPU hash table operations |
| KZG commitment | ~22 seconds | FFT + MSM |
| **Total** | **~5.5 hours** | Dominated by hashing |

Adding Merkle DAG and KZG costs <1% overhead vs BLAKE3 alone.

---

## 7. Use Case Matrix

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    WHICH LAYER FOR WHICH USE CASE                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  USE CASE                          BLAKE3    DAG    KZG                     │
│  ───────────────────────────────────────────────────────                    │
│                                                                             │
│  TRANSFER                                                                   │
│  Verify chunk integrity              ✓                                      │
│  Multi-source swarm download         ✓        ✓                             │
│  Resume interrupted transfer         ✓                                      │
│                                                                             │
│  STORAGE                                                                    │
│  Detect corruption                   ✓        ✓                             │
│  Locate corrupted chunks                      ✓                             │
│  Self-healing coordination                    ✓                             │
│  Deduplication                       ✓        ✓                             │
│                                                                             │
│  VERSIONING                                                                 │
│  Efficient diff between versions              ✓                             │
│  Share unchanged content                      ✓                             │
│  Cross-file deduplication                     ✓                             │
│                                                                             │
│  AUDIT                                                                      │
│  Internal integrity check            ✓        ✓                             │
│  External auditor verification                         ✓                    │
│  Compliance reporting                                  ✓                    │
│  Proof of storage                                      ✓                    │
│  Random sampling audit                        ✓        ✓                    │
│                                                                             │
│  SHARING                                                                    │
│  Prove file exists                                     ✓                    │
│  Compact proof to third party                          ✓                    │
│  Non-repudiation                                       ✓                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Self-Healing

### 8.1 Corruption Detection

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      CORRUPTION DETECTION FLOW                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  BACKGROUND SCAN:                                                           │
│  • Periodically read and hash stored chunks                                 │
│  • Compare to expected CID                                                  │
│  • GPU-accelerated for speed                                                │
│                                                                             │
│  ON CORRUPTION DETECTED:                                                    │
│  1. Mark chunk as corrupted in local database                               │
│  2. Query DAG: Which files reference this chunk?                            │
│  3. Query network: Which edges have valid copy?                             │
│  4. Initiate repair: Fetch from healthy source                              │
│  5. Verify repaired chunk                                                   │
│  6. Update local storage                                                    │
│                                                                             │
│  LOCALIZATION (using Merkle):                                               │
│  • Binary search via virtual tree                                           │
│  • O(log n) probes to find corrupted subtree                                │
│  • For 1 PB: 28 checks instead of 250M                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Repair Coordination

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       REPAIR COORDINATION                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Edge A detects corruption in chunk X:                                      │
│                                                                             │
│  1. A queries Hub: "Who has chunk X?"                                       │
│     Response: [Edge B, Edge C, Hub]                                         │
│                                                                             │
│  2. A selects source (prefer P2P, low latency)                              │
│     Selected: Edge B (same LAN)                                             │
│                                                                             │
│  3. A requests chunk from B                                                 │
│     B sends encrypted chunk                                                 │
│                                                                             │
│  4. A verifies: BLAKE3(decrypted) == X?                                     │
│     YES → Store, repair complete                                            │
│     NO  → Try next source (C, then Hub)                                     │
│                                                                             │
│  5. A reports repair to Hub (for health tracking)                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Scale Characteristics

### 9.1 Storage Requirements

| File Size | Chunks | DAG Nodes | DAG Storage | KZG Aux |
|-----------|--------|-----------|-------------|---------|
| 1 TB | 250K | 300K | 25 MB | 8 MB |
| 10 TB | 2.5M | 3M | 250 MB | 80 MB |
| 100 TB | 25M | 30M | 2.5 GB | 800 MB |
| 1 PB | 250M | 300M | 16 GB | 8 GB |
| 10 PB | 2.5B | 3B | 160 GB | 80 GB |

*Assumes 50% deduplication in DAG

### 9.2 GPU Memory (RTX 5090, 32 GB)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    RTX 5090 MEMORY ALLOCATION                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  For 1 PB file processing:                                                  │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ DAG Storage:           8.5 GB    (nodes + edges, 50% dedup)        │    │
│  │ DAG Hash Table:        4.0 GB    (cuCollections)                   │    │
│  │ BLAKE3 Pipeline:       4.0 GB    (triple buffer + state)           │    │
│  │ KZG Workspace:        12.0 GB    (FFT + MSM)                       │    │
│  │ Proof Generation:      2.0 GB    (batch proofs)                    │    │
│  │ Scratch:               1.5 GB    (misc)                            │    │
│  │ ─────────────────────────────────────────────────────────────────  │    │
│  │ TOTAL:                32.0 GB                                      │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  SINGLE RTX 5090 HANDLES ~1-2 PB FILES                                      │
│                                                                             │
│  For larger: Multi-GPU or streaming to system RAM                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. Summary

The three-layer integrity architecture provides:

| Layer | Purpose | Key Benefit |
|-------|---------|-------------|
| **BLAKE3** | Content addressing | Fast verification, dedup foundation |
| **Merkle DAG** | Structural integrity | Dedup, versioning, corruption localization |
| **KZG** | External proofs | Constant-size proofs, aggregation |

Together they enable:
- **Petabyte-scale** integrity verification
- **GPU-accelerated** processing (70+ GB/s)
- **External audit** capability with compact proofs
- **Self-healing** through DAG-coordinated repair
- **Efficient versioning** via structural sharing

This is enterprise-grade integrity for a personal storage system.
