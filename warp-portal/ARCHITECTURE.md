# System Architecture

> Warp + Portal: Complete System Design

---

## 1. Executive Summary

Warp + Portal is a distributed storage fabric combining:

- **Warp Engine**: GPU-accelerated compression, encryption, and transfer
- **Portal Layer**: Zero-knowledge encryption with ephemeral access control
- **Integrity System**: Three-layer cryptographic verification (BLAKE3 + Merkle DAG + KZG)
- **Network Layer**: WireGuard-based P2P mesh with intelligent routing
- **Intelligence Layer**: GPU-accelerated scheduling with auto-reconciliation

The system handles files from kilobytes to petabytes with consistent performance characteristics, maintaining cryptographic integrity and zero-knowledge guarantees throughout.

---

## 2. Design Principles

### 2.1 Zero Trust
- Hub coordinates but cannot decrypt
- Network transports but cannot inspect
- Every component operates on encrypted data

### 2.2 Performance First
- GPU acceleration for all compute-intensive operations
- Parallel processing at every layer
- Hardware-optimized algorithms

### 2.3 Intelligence by Default
- Automatic routing optimization
- Self-healing on failures
- Predictive pre-positioning

### 2.4 Resilience by Design
- Pre-computed backup paths
- Instant failover (<50ms)
- No single point of failure

### 2.5 Privacy Preserving
- Convergent encryption enables dedup without exposure
- Content addressing without content revelation
- Metadata minimization

---

## 3. System Layers

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│  LAYER 6: APPLICATION                                                       │
│  ═════════════════════                                                      │
│  CLI │ Web Dashboard │ Mobile App │ API                                     │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 5: PORTAL (Zero-Knowledge)                                           │
│  ═════════════════════════════════                                          │
│  Ephemeral Portals │ Lifecycle Policies │ Access Control                    │
│  Re-Entry Flow │ Guest Management │ Key Hierarchy                           │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 4: INTELLIGENCE                                                      │
│  ═════════════════════                                                      │
│  GPU Chunk Scheduler │ Auto-Reconciliation │ Predictive Pre-positioning     │
│  Cost-Aware Routing │ Power-Aware Transfers │ Time-Aware Operations         │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 3: ORCHESTRATION                                                     │
│  ═════════════════════                                                      │
│  Multi-Source Downloads │ Swarm Transfers │ Distributed Uploads             │
│  Edge Registry │ Health Monitoring │ Metrics Collection                     │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 2: INTEGRITY                                                         │
│  ═════════════════════                                                      │
│  BLAKE3 Content Addressing │ Merkle DAG │ KZG Polynomial Commitments        │
│  Convergent Encryption │ Audit Proofs │ Self-Healing                        │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 1: WARP ENGINE                                                       │
│  ═════════════════════                                                      │
│  Content-Defined Chunking │ GPU Compression │ GPU Encryption                │
│  Archive Mode │ Stream Mode │ Triple-Buffer Pipeline                        │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 0: NETWORK                                                           │
│  ═════════════════                                                          │
│  WireGuard Mesh │ QUIC Protocol │ mDNS Discovery │ Virtual IP               │
│  NAT Traversal │ Automatic Roaming │ Hub Relay Fallback                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Component Architecture

### 4.1 Warp Crates (Core Engine)

| Crate | Purpose | Key Functionality |
|-------|---------|-------------------|
| `warp-core` | Common types | Error handling, traits, configuration |
| `warp-crypto` | Cryptography | ChaCha20-Poly1305, BLAKE3, convergent encryption |
| `warp-compress` | Compression | LZ4, Zstd, entropy analysis, GPU nvCOMP |
| `warp-format` | Archive format | Header, index, file table, manifest |
| `warp-hash` | Hashing | BLAKE3 wrapper, parallel hashing |
| `warp-io` | I/O operations | Chunking, directory walking, buffering |
| `warp-gpu` | GPU kernels | CUDA compression, encryption, scheduling |
| `warp-stream` | Streaming | Triple-buffer pipeline, real-time transfer |
| `warp-verify` | Verification | Merkle proofs, integrity checking |
| `warp-cli` | Interface | Command-line tool |

### 4.2 Portal Crates (Distributed Storage)

| Crate | Purpose | Key Functionality |
|-------|---------|-------------------|
| `portal-core` | Portal system | Zero-knowledge model, lifecycle, access control |
| `portal-hub` | Hub server | Coordination, peer directory, relay |
| `warp-edge` | Edge management | Discovery, state tracking, health |
| `warp-sched` | Scheduler | GPU chunk scheduling, cost matrix, failover |
| `warp-net` | Networking | WireGuard integration, mDNS, virtual IP |
| `warp-orch` | Orchestration | Multi-source transfers, swarm coordination |
| `warp-metrics` | Telemetry | Bandwidth estimation, RTT, analytics |
| `warp-integrity` | Integrity | Merkle DAG, KZG commitments, proofs |

### 4.3 Dependency Graph

```
                           portal-cli
                               │
                               ▼
                          portal-core
                               │
                               ▼
                           warp-orch
                         /    │    \
                        /     │     \
                       ▼      ▼      ▼
             warp-sched  warp-edge  warp-net
                  │          │          │
                  ▼          ▼          ▼
               warp-gpu  warp-metrics  warp-proto
                  │          │          │
                  └────┬─────┴────┬─────┘
                       │          │
                       ▼          ▼
              warp-integrity   warp-core
                       │
                       ▼
                  warp-crypto
```

---

## 5. Data Model

### 5.1 Content Addressing

All content is addressed by its cryptographic hash:

```
ChunkID = BLAKE3(chunk_content)     # 32 bytes
NodeID  = BLAKE3(node_definition)   # 32 bytes  
FileID  = BLAKE3(file_root_node)    # 32 bytes
```

Same content always produces the same ID, enabling:
- Automatic deduplication
- Content verification
- Distributed caching

### 5.2 Merkle DAG Structure

```
                    [User Root]
                   /     |     \
                  /      |      \
          [Portal 1] [Portal 2] [Portal 3]
              |          |          |
         ┌────┴────┐     │     ┌────┴────┐
         │         │     │     │         │
     [Dir A]   [Dir B]  ...  [File X] [File Y]
      /   \       |                     |
 [File1] [File2] [File3]               ...
    |       |       |
   ...     ...     ...
    │       │       │
┌───┴───┬───┴───┬───┘
│       │       │
[Chunk] [Chunk] [Chunk] [Chunk] [Chunk]
   A       B       C       A       D
   │                       │
   └───────────────────────┘
   Same chunk, single storage, multiple references
```

### 5.3 Node Types

**Chunk Node** (Leaf):
- Content hash (CID)
- Size
- Encryption metadata

**File Node** (Internal):
- Ordered list of chunk references
- Total size
- File metadata (encrypted)

**Directory Node** (Internal):
- Named entries (name → node CID)
- Directory metadata (encrypted)

**Portal Node** (Root):
- Portal configuration
- Root content node reference
- Lifecycle policy
- KZG commitment

---

## 6. Security Model

### 6.1 Encryption Layers

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          ENCRYPTION LAYERS                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 1: TRANSPORT (WireGuard)                                             │
│  • ChaCha20-Poly1305 between edges                                          │
│  • Curve25519 key exchange                                                  │
│  • Perfect forward secrecy                                                  │
│  • Protects: All traffic in transit                                         │
│                                                                             │
│  LAYER 2: CONTENT (Portal - Convergent)                                     │
│  • ChaCha20-Poly1305 per chunk                                              │
│  • Key derived from content hash (enables dedup)                            │
│  • Protects: Data at rest, enables dedup                                    │
│                                                                             │
│  LAYER 3: METADATA (Portal - Master Key)                                    │
│  • ChaCha20-Poly1305 for manifests                                          │
│  • Key derived from user's master seed                                      │
│  • Protects: Filenames, structure, relationships                            │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  WHO CAN SEE WHAT:                                                          │
│                                                                             │
│  Network observers     → Nothing (WireGuard)                                │
│  Hub                   → Encrypted blobs only                               │
│  Edges without key     → Encrypted blobs only                               │
│  Authorized edges      → Full plaintext access                              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Key Hierarchy

```
Recovery Phrase (24 words, BIP-39)
        │
        ▼
   Master Seed (256 bits)
        │
        ├──────────────────────────────────────────┐
        │                                          │
        ▼                                          ▼
  Encryption Key                             Signing Key
  (ChaCha20-Poly1305)                       (Ed25519)
        │                                          │
        ├───────────┬───────────┐                  │
        │           │           │                  │
        ▼           ▼           ▼                  ▼
   Manifest    Auth Token   Device Key      Identity
     Key          Key         Subkey        Signature
```

### 6.3 Cryptographic Primitives

| Primitive | Algorithm | Purpose |
|-----------|-----------|---------|
| Symmetric Encryption | ChaCha20-Poly1305 | Content and metadata encryption |
| Hashing | BLAKE3 | Content addressing, integrity |
| Key Exchange | X25519 | WireGuard, key agreement |
| Signatures | Ed25519 | Authentication, non-repudiation |
| Key Derivation | Argon2id | Password to key |
| Key Derivation | HKDF-SHA256 | Key hierarchy expansion |
| Polynomial Commitment | KZG (BLS12-381) | Compact proofs |

---

## 7. Network Architecture

### 7.1 WireGuard Mesh

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          WIREGUARD MESH                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                              [Hub]                                          │
│                           10.portal.0.1                                     │
│                          /      |      \                                    │
│                         /       |       \                                   │
│                        /        |        \                                  │
│              [Edge A]      [Edge B]      [Edge C]                           │
│            10.portal.0.2  10.portal.0.3  10.portal.0.4                      │
│                   \           |           /                                 │
│                    \          |          /                                  │
│                     \         |         /                                   │
│                      ─────────┼─────────                                    │
│                               │                                             │
│                          P2P Direct                                         │
│                        (when possible)                                      │
│                                                                             │
│  • Every edge has virtual IP in 10.portal.0.0/16                            │
│  • Hub provides peer discovery and relay fallback                           │
│  • Direct P2P preferred when NAT allows                                     │
│  • Automatic roaming on IP change                                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 Connection Flow

1. **Bootstrap**: New edge connects to Hub (known endpoint)
2. **Registration**: Edge presents public key, receives virtual IP
3. **Peer Discovery**: Hub provides list of relevant peers
4. **Direct Connection**: Edges attempt P2P via WireGuard
5. **Fallback**: If P2P fails, traffic routes through Hub
6. **LAN Optimization**: mDNS discovers local peers for direct LAN transfer

---

## 8. Intelligence Layer

### 8.1 GPU Chunk Scheduler

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          SCHEDULER TICK (50ms)                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  INPUT STATE:                                                               │
│  • Chunk status (pending, in-flight, complete, failed)                      │
│  • Edge status (bandwidth, RTT, health, queue depth)                        │
│  • Transfer progress and deadlines                                          │
│                                                                             │
│  GPU KERNELS:                                                               │
│  1. Cost Matrix Update    - Parallel cost calculation per chunk-edge pair   │
│  2. K-Best Paths          - Find top K sources per chunk                    │
│  3. Failover Detection    - Identify failed transfers                       │
│  4. Load Balancing        - Redistribute from overloaded edges              │
│  5. Dispatch Generation   - Output next batch of chunk assignments          │
│                                                                             │
│  OUTPUT:                                                                    │
│  • Dispatch queue (which chunks to fetch from which edges)                  │
│  • Failover events (reassignments needed)                                   │
│  • Updated ETA                                                              │
│                                                                             │
│  PERFORMANCE:                                                               │
│  • 10M chunks scheduled in <10ms GPU time                                   │
│  • 50ms tick provides real-time adaptation                                  │
│  • Pre-computed backup paths enable instant failover                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Auto-Reconciliation

The system continuously monitors and optimizes:

- **Drift Detection**: Actual vs expected performance
- **Reoptimization**: Reschedule remaining chunks when beneficial
- **Predictive Pre-positioning**: Anticipate access, pre-fetch content
- **Time-Aware**: Optimize for network conditions by time of day
- **Cost-Aware**: Factor in metered connections, data caps
- **Power-Aware**: Respect battery constraints on mobile

---

## 9. Scale Characteristics

### 9.1 File Scale

| File Size | Chunks (4MB) | DAG Nodes | GPU Memory |
|-----------|--------------|-----------|------------|
| 1 TB | 250K | 300K | 200 MB |
| 10 TB | 2.5M | 3M | 2 GB |
| 100 TB | 25M | 30M | 8 GB |
| 1 PB | 250M | 300M | 16 GB |
| 10 PB | 2.5B | 3B | 160 GB* |

*Requires multi-GPU or streaming

### 9.2 Performance Targets

| Operation | Target | Hardware |
|-----------|--------|----------|
| Chunk hashing | 70+ GB/s | RTX 5090 |
| Compression | 20+ GB/s | RTX 5090 + nvCOMP |
| Encryption | 25+ GB/s | RTX 5090 |
| Merkle DAG build | <5s for 1 PB | RTX 5090 |
| KZG commitment | <30s for 1 PB | RTX 5090 |
| Scheduler tick | <10ms for 10M chunks | RTX 5090 |
| Failover | <50ms | Any |

### 9.3 Network Scale

| Metric | Target |
|--------|--------|
| Edges per Hub | 10,000+ |
| Concurrent transfers | 1,000+ |
| P2P connections per edge | 100+ |
| Swarm sources per download | 20+ |

---

## 10. Deployment Topology

### 10.1 Single User

```
[Laptop] ←──WireGuard──→ [Hub] ←──WireGuard──→ [Desktop]
                           ↑
                           │
                    [Cloud Storage]
```

### 10.2 Team Deployment

```
                         [Hub Cluster]
                        /      |      \
                       /       |       \
            [Office A]    [Office B]    [Remote Workers]
            /   |   \        |   \            |
         [PC] [PC] [NAS]   [PC] [NAS]    [Laptop] [Phone]
```

### 10.3 Enterprise

```
              [Primary Hub] ←──────→ [Backup Hub]
                    │                      │
        ┌───────────┼───────────┐          │
        │           │           │          │
   [Region A]  [Region B]  [Region C]      │
        │           │           │          │
        └───────────┴───────────┴──────────┘
                    │
            [Central Archive]
```

---

## 11. Summary

Warp + Portal provides:

1. **Wire-Speed Transfer**: GPU-accelerated compression and encryption at 20+ GB/s
2. **Zero-Knowledge Storage**: Infrastructure cannot access plaintext
3. **Cryptographic Integrity**: Three-layer verification with PB-scale proofs
4. **Intelligent Networking**: P2P mesh with automatic optimization
5. **Self-Healing**: Pre-computed failover, instant recovery
6. **Flexible Access**: Ephemeral portals with policy-driven lifecycle

The architecture is designed for scale (petabytes), performance (GPU-accelerated), and security (zero-knowledge) while remaining practical for individual users and small teams.
