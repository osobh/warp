# System Patterns

## Architecture Overview

Warp follows a layered architecture with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────────┐
│                         warp-cli                                │
│                    (User Interface Layer)                       │
│  Commands: send, fetch, listen, plan, probe, info, resume, bench│
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────────┐
│                        warp-core                                │
│              (Orchestration & Transfer Engine)                  │
│  Components: TransferEngine, Session, ChunkScheduler, Analyzer  │
└─────────────────────────────────────────────────────────────────┘
        │              │              │              │
┌───────┴───┐  ┌───────┴───┐  ┌───────┴───┐  ┌───────┴───┐
│warp-format│  │ warp-net  │  │warp-compress│ │ warp-hash │
│ (.warp)   │  │  (QUIC)   │  │(zstd/lz4)  │ │ (BLAKE3)  │
└───────────┘  └───────────┘  └────────────┘ └───────────┘
        │              │              │              │
┌───────┴──────────────┴──────────────┴──────────────┴───┐
│                       warp-io                          │
│            (Chunking, File I/O, Buffers)               │
│  Components: Chunker, Walker, MappedFile, BufferPool   │
└────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────────┐
│                       warp-crypto                               │
│              (ChaCha20, Ed25519, Key Derivation)                │
│  Components: encrypt/decrypt, sign/verify, derive_key           │
└─────────────────────────────────────────────────────────────────┘
```

## Design Patterns

### Trait-Based Abstractions
```rust
/// Compression algorithm trait - enables CPU/GPU polymorphism
pub trait Compressor: Send + Sync {
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>>;
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>>;
    fn name(&self) -> &'static str;
}
```
Used by: ZstdCompressor, Lz4Compressor, (future) NvcompCompressor

### Strategy Pattern (Adaptive Compression)
```rust
pub enum Strategy { None, Fast, Balanced, Maximum }

impl Strategy {
    pub fn from_entropy(entropy: f64) -> Self {
        match entropy {
            e if e > 0.95 => Self::None,      // Already compressed
            e if e > 0.70 => Self::Fast,      // LZ4
            e if e > 0.30 => Self::Balanced,  // Zstd-3
            _ => Self::Maximum,               // Zstd-19
        }
    }
}
```

### State Machine (Protocol)
```rust
pub enum ProtocolState {
    Initial,       // Connection established
    HelloExchanged,// HELLO frames exchanged
    Negotiated,    // Capabilities agreed
    Planned,       // Transfer plan accepted
    Transferring,  // Data flowing
    Verifying,     // Merkle verification
    Done,          // Success
    Error,         // Failed
}
```

### Builder Pattern (Configuration)
```rust
pub struct ChunkerConfig {
    pub min_size: usize,     // 1MB default
    pub target_size: usize,  // 4MB default
    pub max_size: usize,     // 16MB default
    pub window_size: usize,  // 48 bytes for Buzhash
}
```

## Component Structure

```
crates/
├── warp-cli/         # CLI entry point
│   └── src/
│       ├── main.rs   # Clap argument parsing
│       └── commands/ # Subcommand implementations
├── warp-core/        # Orchestration layer
│   └── src/
│       ├── engine.rs    # TransferEngine
│       ├── session.rs   # Session state
│       ├── scheduler.rs # ChunkScheduler (priority queue)
│       └── analyzer.rs  # PayloadAnalysis
├── warp-format/      # .warp file format
│   └── src/
│       ├── header.rs     # 256-byte fixed header
│       ├── index.rs      # ChunkEntry (56 bytes each)
│       ├── file_table.rs # Path-to-chunk mapping
│       ├── merkle.rs     # Merkle tree
│       ├── reader.rs     # WarpReader
│       └── writer.rs     # WarpWriter
├── warp-net/         # Network layer
│   └── src/
│       ├── frames.rs     # Wire protocol frames
│       ├── protocol.rs   # State machine
│       ├── transport.rs  # QUIC connection
│       └── listener.rs   # Server listener
├── warp-compress/    # Compression
│   └── src/
│       ├── cpu/          # ZstdCompressor, Lz4Compressor
│       ├── gpu/          # (future) NvcompCompressor
│       └── adaptive.rs   # Entropy-based selection
├── warp-hash/        # Hashing
│   └── src/lib.rs    # BLAKE3, parallel hashing
├── warp-crypto/      # Cryptography
│   └── src/
│       ├── encrypt.rs # ChaCha20-Poly1305
│       ├── sign.rs    # Ed25519
│       └── kdf.rs     # Argon2id
└── warp-io/          # I/O primitives
    └── src/
        ├── chunker.rs # Buzhash CDC
        ├── walker.rs  # Directory traversal
        ├── mmap.rs    # Memory-mapped I/O
        └── pool.rs    # Buffer pool
```

## Data Flow

### Archive Creation Flow
```
Files → Walker → Chunker(Buzhash) → Compressor → Hasher → Index → Writer
          │           │                │            │         │
          ▼           ▼                ▼            ▼         ▼
     FileTable    Chunks(1-16MB)   Compressed    Hashes   ChunkIndex
                                                    │
                                                    ▼
                                              MerkleTree
                                                    │
                                                    ▼
                                              Header.merkle_root
```

### Network Transfer Flow
```
Sender                              Receiver
  │                                    │
  ├──── HELLO ─────────────────────────►
  │◄───────────────────────── HELLO ────┤
  │                                    │
  ├──── CAPABILITIES ──────────────────►
  │◄─────────────────── CAPABILITIES ───┤
  │         (Negotiate params)         │
  │                                    │
  ├──── PLAN (chunk list, sizes) ──────►
  │◄───────────────────── HAVE ─────────┤ (dedupe)
  │◄───────────────────── ACCEPT ───────┤
  │                                    │
  ├──── CHUNK[0..n] (parallel) ─────────►
  │◄───────────────── ACK[batch] ───────┤
  │        ... repeat ...              │
  │                                    │
  ├──── END_OF_DATA ───────────────────►
  │◄───────────────────── VERIFY ───────┤
  │◄───────────────────── DONE ─────────┤
```

## Critical Paths

1. **Chunking Pipeline**: File → mmap → Buzhash → boundaries → chunks
   - Must maintain throughput >5GB/s CPU, >10GB/s GPU

2. **Compression Selection**: Sample entropy → select algorithm → compress
   - Decision must complete in <1ms per chunk

3. **Merkle Verification**: Chunk hashes → tree build → root comparison
   - Incremental verification for partial transfer resume

4. **QUIC Stream Management**: Parallel streams → congestion control → ACKs
   - Target 16 concurrent streams, 64KB-64MB chunks

## Key Technical Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Rolling hash | Buzhash | Fast, good boundary distribution |
| Crypto hash | BLAKE3 | Fastest secure hash, parallelizable |
| Transport | QUIC | 0-RTT resume, multiplexing, TLS built-in |
| Compression | Zstd/LZ4 | Best ratio/speed tradeoffs |
| Encryption | ChaCha20-Poly1305 | AEAD, constant-time, software-friendly |
| Archive format | Custom binary | O(1) lookup, streaming support |
| Chunk size | 1-16MB | Balances dedup ratio vs overhead |
| Serialization | MessagePack | Compact, fast, schema-less |
