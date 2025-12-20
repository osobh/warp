# warp

> GPU-accelerated bulk data transfer with adaptive compression, deduplication, and Merkle verification

## Features

- **GPU Acceleration**: nvCOMP integration for parallel compression/decompression
- **Ultra-Fast Chunking**: SeqCDC algorithm with SIMD acceleration (30+ GB/s)
- **QUIC Transport**: Modern, multiplexed transport with built-in encryption
- **Sparse Merkle Verification**: O(log n) single-chunk verification with LRU caching
- **Erasure Coding**: Reed-Solomon fault tolerance (survive up to m shard failures)
- **Adaptive Compression**: Per-chunk algorithm selection based on entropy analysis
- **Resume Support**: Interrupted transfers continue from last verified chunk
- **Cross-Platform SIMD**: AVX2/AVX-512 on x86_64, NEON on ARM (Apple Silicon)

## Installation

```bash
cargo install warp-cli
```

## Quick Start

```bash
# Send data to a remote server
warp send ./data server:/archive

# Receive data
warp fetch server:/archive ./local

# Start a listener daemon
warp listen --port 9999

# Analyze transfer before executing
warp plan ./data server:/dest
```

## Examples

Run the included examples to explore warp's capabilities:

```bash
# Basic content-defined chunking
cargo run -p warp-core --example basic_chunking

# Compression and encryption pipeline
cargo run -p warp-core --example compress_encrypt

# Full pipeline (chunk → compress → encrypt → hash)
cargo run -p warp-core --example full_pipeline --release

# Archive creation and extraction
cargo run -p warp-core --example archive_roundtrip

# Parallel BLAKE3 hashing
cargo run -p warp-core --example parallel_hashing --release
```

## Performance

Benchmarks on modern hardware (release build):

| Operation | Throughput | Notes |
|-----------|------------|-------|
| **Chunking (SeqCDC SIMD)** | **31 GB/s** | ARM NEON / AVX-512 |
| Chunking (SeqCDC scalar) | 1-2 GB/s | Fallback |
| Chunking (Buzhash legacy) | 300 MB/s | For compatibility |
| Compression (Zstd) | 4,587 MB/s | |
| Encryption (ChaCha20-Poly1305) | 528 MB/s | |
| Hashing (BLAKE3) | 1,761 MB/s | |
| Full Pipeline | 500+ MB/s | With SeqCDC |

### Chunking Algorithm Comparison

| Algorithm | 10MB | 100MB | vs Buzhash |
|-----------|------|-------|------------|
| Buzhash (legacy) | 300 MiB/s | 300 MiB/s | baseline |
| SeqCDC Scalar | 255 MiB/s | 750 MiB/s | 2.5x |
| **SeqCDC SIMD** | **12.5 GiB/s** | **31 GiB/s** | **100x** |

*Tested on Apple M4 Pro with ARM NEON. x86_64 with AVX-512 achieves similar results.*

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         warp-cli                                │
│                    (User Interface)                             │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────────┐
│                        warp-core                                │
│              (Orchestration & Transfer Engine)                  │
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
└────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────────┐
│                       warp-crypto                               │
│              (ChaCha20, Ed25519, Key Derivation)                │
└─────────────────────────────────────────────────────────────────┘
```

## Erasure Coding

The `warp-ec` crate provides Reed-Solomon erasure coding for fault-tolerant data transfer:

```rust
use warp_ec::{ErasureConfig, ErasureEncoder, ErasureDecoder};

// RS(10,4): 10 data shards + 4 parity shards
// Can survive up to 4 shard failures
let config = ErasureConfig::new(10, 4).unwrap();

// Encode data into shards
let encoder = ErasureEncoder::new(config.clone());
let shards = encoder.encode(&data)?;

// Decode even with missing shards
let mut received: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
received[0] = None;  // Simulate loss
received[5] = None;  // Simulate loss

let decoder = ErasureDecoder::new(config);
let recovered = decoder.decode(&received)?;
```

**Preset Configurations:**

| Config | Overhead | Fault Tolerance | Use Case |
|--------|----------|-----------------|----------|
| RS(4,2) | 50% | 2 failures | Small transfers |
| RS(6,3) | 50% | 3 failures | Medium reliability |
| RS(10,4) | 40% | 4 failures | **Default** |
| RS(16,4) | 25% | 4 failures | Storage efficiency |

## Sparse Merkle Verification

Efficient O(log n) verification for large archives:

```rust
use warp_format::WarpReader;

// Open archive with verification tree
let reader = WarpReader::open_with_verification(path)?;

// Verify single chunk in O(log n) time
let valid = reader.verify_chunk_fast(chunk_index)?;

// Random spot-check (e.g., verify 100 random chunks)
let (passed, total) = reader.verify_random_sample(100)?;
println!("Verified {}/{} chunks", passed, total);
```

## Crates

| Crate | Description |
|-------|-------------|
| `warp-cli` | Command-line interface |
| `warp-core` | Core orchestration and transfer engine |
| `warp-format` | Native `.warp` archive format with sparse Merkle verification |
| `warp-net` | QUIC networking with TLS 1.3 |
| `warp-compress` | Zstd/LZ4 compression |
| `warp-hash` | BLAKE3 hashing with parallelism |
| `warp-crypto` | ChaCha20/Ed25519/Argon2 cryptography |
| `warp-io` | SeqCDC chunking (31 GB/s), file I/O, SIMD acceleration |
| `warp-gpu` | GPU acceleration via nvCOMP |
| `warp-ec` | Reed-Solomon erasure coding for fault tolerance |
| `warp-sched` | Transfer scheduling |
| `warp-orch` | Multi-source orchestration |
| `portal-*` | Zero-knowledge portal system |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
