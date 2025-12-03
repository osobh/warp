# Warp Engine

> GPU-Accelerated Data Transfer Engine

---

## 1. Overview

Warp is a high-performance data transfer engine designed for wire-speed operation. It provides two complementary transfer modes:

- **Archive Mode**: Files and directories with deduplication, integrity verification, and resume
- **Stream Mode**: Real-time encrypted data flows with sub-5ms latency

Both modes leverage GPU acceleration for compression and encryption, achieving throughput exceeding 20 GB/s on modern hardware.

---

## 2. Design Goals

| Goal | Target | Mechanism |
|------|--------|-----------|
| Throughput | 20+ GB/s | GPU acceleration |
| Latency (stream) | <5ms | Triple-buffered pipeline |
| Deduplication | >50% typical | Content-defined chunking |
| Integrity | 100% verified | Merkle DAG + BLAKE3 |
| Resume | Any interruption | Chunk-level state |
| Portability | Cross-platform | Rust + CUDA |

---

## 3. Archive Mode

### 3.1 Content-Defined Chunking

Archive mode uses Buzhash rolling hash to find content-defined chunk boundaries:

```
Input Stream: ────────────────────────────────────────────────────────►

Buzhash Window (48 bytes) slides across input:
                    ┌────────────────────┐
                    │ Rolling Hash = H   │
                    └────────────────────┘
                              │
                              ▼
                    If H & MASK == PATTERN:
                         ──► Chunk boundary!

Result: Variable-size chunks (1MB - 16MB, ~4MB average)
```

**Parameters:**
- Window size: 48 bytes
- Mask: 22 bits (0x3FFFFF)
- Minimum chunk: 1 MB
- Maximum chunk: 16 MB
- Average chunk: ~4 MB

**Benefits:**
- Identical content produces identical chunks regardless of position
- Insertions/deletions affect only nearby chunks
- Enables effective deduplication

### 3.2 Archive Format (.warp)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           .WARP FILE FORMAT                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ HEADER (256 bytes)                                                  │   │
│  │ • Magic: "WARP" (0x57415250)                                        │   │
│  │ • Version, flags, compression, encryption                           │   │
│  │ • Offsets to index, file table, data                                │   │
│  │ • Merkle root, chunk count, total size                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ CHUNK INDEX                                                         │   │
│  │ • Array of ChunkEntry (56 bytes each)                               │   │
│  │ • Hash, offset, compressed size, original size, flags               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ FILE TABLE                                                          │   │
│  │ • Path → chunk range mapping                                        │   │
│  │ • Metadata (permissions, timestamps)                                │   │
│  │ • MessagePack encoded                                               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ DATA BLOCKS                                                         │   │
│  │ • Compressed and/or encrypted chunks                                │   │
│  │ • Sequential or interleaved based on access pattern                 │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.3 Archive Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ARCHIVE CREATION PIPELINE                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  STAGE 1: READ                                                              │
│  ─────────────────                                                          │
│  [File System] ──► Directory Walker ──► File Reader ──► Raw Bytes           │
│                                                                             │
│  STAGE 2: CHUNK                                                             │
│  ─────────────────                                                          │
│  Raw Bytes ──► Buzhash Chunker ──► Variable Chunks                          │
│                                                                             │
│  STAGE 3: HASH (GPU)                                                        │
│  ─────────────────                                                          │
│  Chunks ──► GPU BLAKE3 ──► Content IDs (CIDs)                               │
│                                                                             │
│  STAGE 4: DEDUP                                                             │
│  ─────────────────                                                          │
│  CIDs ──► Hash Table Lookup ──► New Chunks Only                             │
│                                                                             │
│  STAGE 5: COMPRESS (GPU)                                                    │
│  ─────────────────                                                          │
│  New Chunks ──► nvCOMP (LZ4/Zstd) ──► Compressed Chunks                     │
│                                                                             │
│  STAGE 6: ENCRYPT (GPU)                                                     │
│  ─────────────────                                                          │
│  Compressed ──► ChaCha20-Poly1305 ──► Encrypted Chunks                      │
│                                                                             │
│  STAGE 7: WRITE                                                             │
│  ─────────────────                                                          │
│  Encrypted ──► Archive Writer ──► .warp File                                │
│                                                                             │
│  All GPU stages run in parallel with triple-buffering.                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.4 Extraction Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ARCHIVE EXTRACTION PIPELINE                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  STAGE 1: READ                                                              │
│  • Memory-map archive file                                                  │
│  • Parse header and index                                                   │
│  • Random access to any chunk                                               │
│                                                                             │
│  STAGE 2: DECRYPT (GPU)                                                     │
│  • ChaCha20-Poly1305 in parallel                                            │
│  • Verify authentication tags                                               │
│                                                                             │
│  STAGE 3: DECOMPRESS (GPU)                                                  │
│  • nvCOMP decompression                                                     │
│  • LZ4 or Zstd based on chunk flags                                         │
│                                                                             │
│  STAGE 4: VERIFY                                                            │
│  • BLAKE3 hash verification                                                 │
│  • Compare to stored CID                                                    │
│                                                                             │
│  STAGE 5: REASSEMBLE                                                        │
│  • Order chunks by file table                                               │
│  • Write to destination                                                     │
│  • Restore metadata                                                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Stream Mode

### 4.1 Triple-Buffer Pipeline

Stream mode achieves low latency through overlapped operations:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        TRIPLE-BUFFER PIPELINE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Time ──────────────────────────────────────────────────────────────────►   │
│                                                                             │
│  Buffer A: [READ 1] [PROC 1] [SEND 1] [READ 4] [PROC 4] [SEND 4] ...       │
│  Buffer B:          [READ 2] [PROC 2] [SEND 2] [READ 5] [PROC 5] ...       │
│  Buffer C:                   [READ 3] [PROC 3] [SEND 3] [READ 6] ...       │
│                                                                             │
│  │─────────│─────────│─────────│─────────│─────────│                        │
│     ~1ms      ~1ms      ~1ms      ~1ms      ~1ms                            │
│                                                                             │
│  While one buffer is being read, another is being processed (GPU),          │
│  and a third is being sent. Maximum parallelism.                            │
│                                                                             │
│  End-to-end latency: ~3ms (one buffer cycle)                                │
│  Throughput: Limited only by slowest stage                                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Stream Chunking

Stream mode uses fixed-size or time-based chunking (not content-defined):

| Strategy | Chunk Size | Use Case |
|----------|------------|----------|
| Fixed Size | 64 KB | Network packets, low latency |
| Fixed Size | 256 KB | General streaming |
| Fixed Size | 1 MB | High throughput |
| Time-Based | 10 ms | Real-time streaming |
| Time-Based | 100 ms | Interactive applications |

### 4.3 Stream Protocol

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        STREAM FRAME FORMAT                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ Frame Header (16 bytes)                                            │    │
│  │ • Sequence number (8 bytes)                                        │    │
│  │ • Payload length (4 bytes)                                         │    │
│  │ • Flags (2 bytes): COMPRESSED, ENCRYPTED, FINAL, etc.              │    │
│  │ • Reserved (2 bytes)                                               │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ Payload (variable)                                                 │    │
│  │ • Compressed and/or encrypted data                                 │    │
│  │ • Size determined by header                                        │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ Authentication Tag (16 bytes)                                      │    │
│  │ • Poly1305 MAC over header + payload                               │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.4 Backpressure Handling

When producer outpaces consumer:

1. **Detection**: Output queue depth exceeds threshold
2. **Throttle**: Reduce read rate to match send rate
3. **Buffer**: Use remaining buffers as temporary storage
4. **Signal**: Inform upstream of slowdown
5. **Resume**: Return to full speed when pressure relieved

No data loss under any condition.

---

## 5. GPU Acceleration

### 5.1 GPU Pipeline Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          GPU MEMORY LAYOUT                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ INPUT BUFFERS (Triple-buffered, Pinned Memory)                      │   │
│  │ • Buffer A: Currently being filled from disk/network                │   │
│  │ • Buffer B: Currently being processed by GPU                        │   │
│  │ • Buffer C: Currently being sent to disk/network                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ GPU WORKSPACE                                                       │   │
│  │ • BLAKE3 hash state arrays                                          │   │
│  │ • nvCOMP compression buffers                                        │   │
│  │ • ChaCha20 state                                                    │   │
│  │ • Intermediate results                                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ OUTPUT BUFFERS (Triple-buffered, Pinned Memory)                     │   │
│  │ • Processed chunks ready for writing                                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 CUDA Kernel Overview

| Kernel | Purpose | Parallelism |
|--------|---------|-------------|
| `blake3_hash_batch` | Hash multiple chunks | One warp per chunk |
| `lz4_compress_batch` | LZ4 compression | One block per chunk |
| `zstd_compress_batch` | Zstd compression | One block per chunk |
| `chacha20_encrypt_batch` | ChaCha20 encryption | One thread per 64 bytes |
| `poly1305_auth_batch` | Authentication tags | One warp per chunk |

### 5.3 Memory Transfer Optimization

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       ZERO-COPY DATA PATH                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Traditional:                                                               │
│  [Disk] ──► [User Memory] ──► [Pinned Memory] ──► [GPU] ──► [Pinned] ──►   │
│                                                                             │
│  Optimized (Direct I/O to Pinned):                                          │
│  [Disk] ──────────────────► [Pinned Memory] ──► [GPU] ──► [Pinned] ──►     │
│                                                                             │
│  GPUDirect Storage (future):                                                │
│  [NVMe] ───────────────────────────────────► [GPU] ──────────────────►     │
│                                                                             │
│  Each optimization eliminates one memory copy.                              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Compression Strategy

### 6.1 Adaptive Compression

Warp analyzes each chunk to select optimal compression:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       COMPRESSION DECISION TREE                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  For each chunk:                                                            │
│                                                                             │
│  1. Sample first 4KB, compute entropy                                       │
│                                                                             │
│  2. Entropy > 7.9 bits/byte?                                                │
│     YES ──► Already compressed/random ──► STORE (no compression)            │
│     NO  ──► Continue                                                        │
│                                                                             │
│  3. Entropy > 6.0 bits/byte?                                                │
│     YES ──► Moderate compressibility ──► LZ4 (fast)                         │
│     NO  ──► Continue                                                        │
│                                                                             │
│  4. Entropy < 4.0 bits/byte?                                                │
│     YES ──► Highly compressible ──► Zstd level 6                            │
│     NO  ──► Medium ──► Zstd level 3                                         │
│                                                                             │
│  Result: Each chunk gets optimal algorithm for its content.                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Compression Performance

| Algorithm | Compression Speed | Decompression Speed | Ratio |
|-----------|-------------------|---------------------|-------|
| None | ∞ | ∞ | 1.0x |
| LZ4 (GPU) | 20+ GB/s | 30+ GB/s | 2-3x |
| Zstd L3 (GPU) | 5+ GB/s | 15+ GB/s | 3-5x |
| Zstd L6 (GPU) | 2+ GB/s | 15+ GB/s | 4-6x |

---

## 7. Encryption

### 7.1 ChaCha20-Poly1305

Warp uses ChaCha20-Poly1305 (RFC 8439) for authenticated encryption:

- **Algorithm**: ChaCha20 stream cipher + Poly1305 MAC
- **Key Size**: 256 bits
- **Nonce Size**: 96 bits (12 bytes)
- **Tag Size**: 128 bits (16 bytes)

### 7.2 Nonce Management

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          NONCE GENERATION                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ARCHIVE MODE (Convergent):                                                 │
│  • Nonce = BLAKE3(chunk_content)[0:12]                                      │
│  • Deterministic: same content → same nonce                                 │
│  • Required for deduplication                                               │
│                                                                             │
│  STREAM MODE (Random):                                                      │
│  • Nonce = [Session ID (4 bytes)] || [Counter (8 bytes)]                    │
│  • Counter increments per chunk                                             │
│  • Never reuse (counter + unique session)                                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.3 GPU Encryption Performance

ChaCha20 is highly parallelizable:
- Each 64-byte block independent
- 20 rounds of quarter-round operations
- GPU can process thousands of blocks simultaneously

**RTX 5090 Target**: 25+ GB/s encryption throughput

---

## 8. Transfer Protocol

### 8.1 QUIC Transport

Warp uses QUIC (RFC 9000) for network transfer:

| Feature | Benefit |
|---------|---------|
| Multiplexed streams | Multiple files in parallel |
| Built-in encryption | TLS 1.3 integrated |
| Connection migration | Survives IP changes |
| 0-RTT resumption | Fast reconnection |
| Congestion control | BBR or CUBIC |

### 8.2 Transfer Session

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          TRANSFER SESSION                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  SESSION ESTABLISHMENT:                                                     │
│  1. QUIC connection with mutual TLS authentication                          │
│  2. Exchange capabilities (GPU, compression, bandwidth)                     │
│  3. Agree on transfer parameters                                            │
│                                                                             │
│  TRANSFER:                                                                  │
│  1. Sender transmits manifest (encrypted)                                   │
│  2. Receiver identifies needed chunks (dedup check)                         │
│  3. Sender streams chunks (parallel QUIC streams)                           │
│  4. Receiver verifies and acknowledges                                      │
│                                                                             │
│  COMPLETION:                                                                │
│  1. Receiver verifies Merkle root                                           │
│  2. Final acknowledgment                                                    │
│  3. Session teardown                                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Resume Support

### 9.1 State Tracking

Every transfer maintains resumable state:

```
TransferState:
  session_id:        UUID
  direction:         SEND | RECEIVE
  manifest_hash:     [u8; 32]
  total_chunks:      u64
  completed_chunks:  BitVec
  in_flight:         HashMap<ChunkID, Timestamp>
  last_activity:     Timestamp
```

### 9.2 Resume Protocol

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          RESUME FLOW                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. RECONNECT                                                               │
│     • Establish new QUIC connection                                         │
│     • Present session ID                                                    │
│                                                                             │
│  2. STATE EXCHANGE                                                          │
│     • Sender provides manifest hash                                         │
│     • Receiver provides completed chunk bitmap                              │
│                                                                             │
│  3. VERIFICATION                                                            │
│     • Compare manifest hashes (must match)                                  │
│     • Verify receiver's claimed chunks (spot check)                         │
│                                                                             │
│  4. CONTINUE                                                                │
│     • Resume from incomplete chunks                                         │
│     • Skip already-transferred chunks                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. Performance Benchmarks

### 10.1 Target Performance (RTX 5090)

| Operation | Target | Notes |
|-----------|--------|-------|
| BLAKE3 Hashing | 70 GB/s | Parallel chunk hashing |
| LZ4 Compression | 20 GB/s | nvCOMP batch |
| Zstd Compression | 5 GB/s | nvCOMP batch, level 3 |
| ChaCha20 Encryption | 25 GB/s | Parallel blocks |
| Archive Create | 15 GB/s | End-to-end |
| Archive Extract | 20 GB/s | End-to-end |
| Stream Latency | <5 ms | End-to-end |

### 10.2 Comparison

| Tool | Throughput | GPU | Integrity | Resume |
|------|------------|-----|-----------|--------|
| **Warp** | 15+ GB/s | ✓ | Merkle+KZG | ✓ |
| tar + pigz | 2 GB/s | ✗ | CRC32 | ✗ |
| rsync | 500 MB/s | ✗ | MD5 | ✓ |
| rclone | 1 GB/s | ✗ | MD5 | ✓ |

---

## 11. CLI Reference

### 11.1 Archive Commands

```bash
# Create archive
warp archive create <INPUT> -o <OUTPUT.warp> [OPTIONS]
  --compression <auto|none|lz4|zstd>
  --encryption <KEY>
  --threads <N>
  --gpu <DEVICE>

# Extract archive
warp archive extract <INPUT.warp> -o <OUTPUT_DIR> [OPTIONS]
  --verify           # Verify integrity before extract
  --encryption <KEY>

# List contents
warp archive list <INPUT.warp>

# Verify integrity
warp archive verify <INPUT.warp>
```

### 11.2 Stream Commands

```bash
# Encrypt stream
warp stream encrypt --key <KEY> [OPTIONS]
  --compression <none|lz4|zstd>
  --chunk-size <BYTES>

# Decrypt stream
warp stream decrypt --key <KEY>

# Example: Encrypted backup over network
tar cf - /data | warp stream encrypt --key $KEY | ssh remote "cat > backup.enc"
```

### 11.3 Transfer Commands

```bash
# Send to remote
warp send <PATH> --to <HOST:PORT> [OPTIONS]
  --key <KEY>
  --resume <SESSION_ID>

# Receive from remote
warp receive --from <HOST:PORT> -o <OUTPUT> [OPTIONS]
  --key <KEY>
```

---

## 12. Summary

Warp provides:

1. **GPU-Accelerated Processing**: 20+ GB/s compression and encryption
2. **Content-Defined Chunking**: Intelligent deduplication
3. **Two Transfer Modes**: Archive (files) and Stream (real-time)
4. **Adaptive Compression**: Per-chunk algorithm selection
5. **Authenticated Encryption**: ChaCha20-Poly1305
6. **QUIC Transport**: Modern, multiplexed, resumable
7. **Full Resume Support**: Any interruption recoverable

Warp is the performance foundation for Portal's distributed storage fabric.
