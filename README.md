# warp

> Works alone but better with friends and batteries

GPU-accelerated HPC storage and transfer with adaptive compression, deduplication, erasure coding, and S3-compatible API.

[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

---

## Table of Contents

- [Features](#features)
- [Performance](#performance)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [HPC-AI Integration](#hpc-ai-integration)
- [Crate Catalog](#crate-catalog)
- [Feature Flags](#feature-flags)
- [Code Examples](#code-examples)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)

---

## Features

### Standalone (batteries included)

WARP runs fully standalone with no external dependencies:

- **SeqCDC Chunking** - SIMD-accelerated content-defined chunking at 31 GB/s
- **QUIC Transport** - Modern multiplexed transport with built-in TLS 1.3
- **ChaCha20-Poly1305** - AEAD encryption for data at rest and in transit
- **Zstd/LZ4 Compression** - Adaptive per-chunk compression based on entropy
- **BLAKE3 Hashing** - Parallel cryptographic hashing at 1.7 GB/s
- **Sparse Merkle Verification** - O(log n) single-chunk verification
- **Ephemeral URLs** - Ed25519-signed time-limited access tokens
- **S3-Compatible API** - Drop-in replacement for AWS S3 core operations
- **CLI Tools** - Full-featured command-line interface

### Enhanced (with friends)

Link with other HPC-AI projects for additional capabilities:

- **Portal Mesh** - Zero-knowledge P2P distributed storage
- **Parcode Lazy-Loading** - O(1) field-level access without full deserialization
- **Erasure Coding** - RS(10,4) Reed-Solomon fault tolerance
- **GPU Acceleration** - nvCOMP integration for parallel compression
- **Raft Consensus** - Strong consistency for distributed metadata
- **Multi-Tier Transport** - Auto-select optimal transport via hpc-channels

---

## Performance

Benchmarks on modern hardware (Apple M4 Pro / x86_64 with AVX-512):

| Operation | Throughput | Notes |
|-----------|------------|-------|
| **SeqCDC SIMD** | **31 GB/s** | ARM NEON / AVX-512 |
| **Erasure Decode** | **37.5 GiB/s** | RS(10,4) |
| **Local Get** | **22.6 GiB/s** | In-memory backend |
| **Distributed Get** | **3.5 GiB/s** | Cross-node with erasure |
| Compression (Zstd) | 4,587 MB/s | Level 3 |
| Encryption | 528 MB/s | ChaCha20-Poly1305 |
| Hashing | 1,761 MB/s | BLAKE3 parallel |
| Full Pipeline | 500+ MB/s | Chunk + compress + encrypt |

### Chunking Algorithm Comparison

| Algorithm | 10 MB | 100 MB | vs Buzhash |
|-----------|-------|--------|------------|
| Buzhash (legacy) | 300 MiB/s | 300 MiB/s | baseline |
| SeqCDC Scalar | 255 MiB/s | 750 MiB/s | 2.5x |
| **SeqCDC SIMD** | **12.5 GiB/s** | **31 GiB/s** | **100x** |

---

## Quick Start

### Installation

```bash
cargo install warp-cli
```

### Transfer Files

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

### Run the API Server

```bash
# Start S3-compatible API server
cargo run -p warp-store-api -- --bind 0.0.0.0:9000
```

### Examples

```bash
# Basic content-defined chunking
cargo run -p warp-core --example basic_chunking

# Compression and encryption pipeline
cargo run -p warp-core --example compress_encrypt

# Full pipeline (chunk -> compress -> encrypt -> hash)
cargo run -p warp-core --example full_pipeline --release

# Archive creation and extraction
cargo run -p warp-core --example archive_roundtrip

# Parallel BLAKE3 hashing
cargo run -p warp-core --example parallel_hashing --release
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                              WARP                                    │
│              "Works alone but better with friends"                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─ User Interfaces ────────────────────────────────────────────┐   │
│  │  warp-cli           │  warp-store-api (S3 + Native HPC)      │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                │                                     │
│  ┌─ Storage Layer ─────────────┴─────────────────────────────────┐  │
│  │  warp-store  │  Buckets, Versioning, Ephemeral Tokens         │  │
│  │  Backends: Local │ Portal │ Erasure │ Distributed             │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                │                                     │
│  ┌─ Core Primitives ───────────┴─────────────────────────────────┐  │
│  │  warp-io (SeqCDC 31 GB/s)  │  warp-ec (RS encoding)           │  │
│  │  warp-compress (Zstd/LZ4)  │  warp-crypto (ChaCha20/Ed25519)  │  │
│  │  warp-hash (BLAKE3)        │  warp-format (.warp archives)    │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                │                                     │
│  ┌─ Transport Layer ───────────┴─────────────────────────────────┐  │
│  │  Tier 0: In-process (<1µs)  │  Tier 2: RDMA (1-50µs)          │  │
│  │  Tier 1: Unix socket (2-10µs)│ Tier 3: WireGuard (50µs+)      │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌─ Portal System (Zero-Knowledge P2P) ─────────────────────────┐   │
│  │  portal-core  │  portal-hub  │  portal-net  │  portal-store   │  │
│  │  Convergent encryption, Merkle proofs, WireGuard mesh         │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

---

## HPC-AI Integration

WARP is part of the HPC-AI ecosystem. Here's the integration status with all projects:

| # | Project | Status | Integration | Benefit |
|---|---------|--------|-------------|---------|
| 01 | **hpc-channels** | Integrated | Transport tier selection | Auto-select optimal transport (Tier 0-3) |
| 02 | **parcode** | Feature-gated | Lazy-loading backend | O(1) field access without full deser |
| 03 | **rmpi** | Planned | CollectiveRead/Write | Type-safe distributed I/O |
| 04 | **slai** | Planned | Brain-Link scheduling | GPU-aware data placement |
| 05 | **warp** | Core | Data primitives | Chunking, EC, compression |
| 06 | **stratoswarm** | Planned | Mesh coordination | P2P replication |
| 07 | - | Reserved | - | - |
| 08 | **rustytorch** | Planned | Model storage | Checkpoint management |
| 09 | **vortex** | Planned | API gateway | S3 compatibility layer |
| 10 | **nebula** | Planned | RDMA + WireGuard | Cross-domain transport |
| 11 | **horizon** | Planned | UI dashboard | Management console |
| 12 | - | Reserved | - | - |

**Legend:** Integrated | Feature-gated | Planned

---

## Crate Catalog

WARP consists of 33 crates organized by function:

### Core

| Crate | Description |
|-------|-------------|
| `warp-core` | Orchestration and transfer engine |
| `warp-io` | SeqCDC chunking (31 GB/s), SIMD I/O, file operations |
| `warp-format` | Native `.warp` archive format with sparse Merkle verification |
| `warp-config` | Configuration management and validation |

### Cryptography & Encoding

| Crate | Description |
|-------|-------------|
| `warp-crypto` | ChaCha20-Poly1305, Ed25519, Argon2 key derivation |
| `warp-hash` | BLAKE3 parallel cryptographic hashing |
| `warp-compress` | Zstd/LZ4 adaptive compression |
| `warp-ec` | Reed-Solomon erasure coding (RS(4,2) to RS(16,4)) |
| `warp-oprf` | OPRF privacy-preserving protocols for blind dedup |
| `warp-kms` | Key management service integration |

### Networking

| Crate | Description |
|-------|-------------|
| `warp-net` | QUIC transport with TLS 1.3, zero-copy |
| `warp-edge` | Edge transport with metrics and monitoring |
| `warp-ipc` | Inter-process communication primitives |

### Storage

| Crate | Description |
|-------|-------------|
| `warp-store` | S3-compatible object storage with HPC extensions |
| `warp-store-api` | REST API server (S3 + Native HPC endpoints) |
| `warp-chonkers` | Versioned content-defined deduplication |

### Orchestration

| Crate | Description |
|-------|-------------|
| `warp-orch` | Multi-source download orchestration |
| `warp-sched` | Transfer scheduling and prioritization |
| `warp-stream` | Triple-buffer streaming pipeline |

### Portal System (Zero-Knowledge P2P)

| Crate | Description |
|-------|-------------|
| `portal-core` | Convergent encryption, Merkle proofs |
| `portal-hub` | P2P coordination hub |
| `portal-net` | WireGuard mesh networking |

### Acceleration

| Crate | Description |
|-------|-------------|
| `warp-gpu` | CUDA/Metal GPU acceleration |
| `warp-dpu` | DPU offload (BlueField, Pensando, Intel IPU) |
| `warp-neural` | WaLLoC neural compression with ONNX Runtime |

### CLI & Operations

| Crate | Description |
|-------|-------------|
| `warp-cli` | Command-line interface |
| `warp-api` | REST API framework |
| `warp-dashboard` | Real-time monitoring dashboard |
| `warp-telemetry` | Metrics and tracing collection |
| `warp-iam` | Identity and access management |

---

## Feature Flags

Configure WARP capabilities via Cargo features:

```toml
[dependencies]
warp-store = { version = "0.1", features = ["full"] }
```

| Feature | Description |
|---------|-------------|
| `local` | Local filesystem backend (default) |
| `portal` | P2P mesh storage via portal system |
| `parcode` | Lazy-loading integration with hpc-ai/02-parcode |
| `erasure` | Reed-Solomon erasure coding |
| `gpu` | GPU acceleration via nvCOMP |
| `raft` | Distributed consensus for metadata |
| `full` | All features enabled |

---

## Code Examples

### Ephemeral URL Generation

Create time-limited, scoped access tokens:

```rust
use warp_store::{Store, StoreConfig, ObjectKey, AccessScope, Permissions};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::new(StoreConfig::default()).await?;

    // Simple: Create a read-only token for a specific object
    let key = ObjectKey::new("my-bucket", "data/file.bin")?;
    let token = store.create_ephemeral_url(&key, Duration::from_secs(3600))?;
    println!("Access URL: /api/v1/access/{}/data/file.bin", token.encode());

    // Advanced: Create a token with custom scope and permissions
    let token = store.create_ephemeral_url_with_options(
        AccessScope::Prefix {
            bucket: "data".into(),
            prefix: "public/".into()
        },
        Permissions::READ,  // or Permissions::READ | Permissions::WRITE
        Duration::from_secs(3600),
        None,  // IP restrictions
        None,  // Rate limiting
    )?;

    Ok(())
}
```

### Erasure Coding

Fault-tolerant data encoding with Reed-Solomon:

```rust
use warp_ec::{ErasureConfig, ErasureEncoder, ErasureDecoder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // RS(10,4): 10 data shards + 4 parity shards
    // Can survive up to 4 shard failures
    let config = ErasureConfig::new(10, 4)?;

    // Data length must be divisible by data_shards (10)
    let data: Vec<u8> = (0..10240).map(|i| (i % 256) as u8).collect();

    // Encode data into 14 shards (10 data + 4 parity)
    let encoder = ErasureEncoder::new(config.clone());
    let shards = encoder.encode(&data)?;

    // Simulate losing 4 shards (the maximum we can tolerate)
    let mut received: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
    received[2] = None;   // data shard
    received[5] = None;   // data shard
    received[8] = None;   // data shard
    received[11] = None;  // parity shard

    // Still recoverable!
    let decoder = ErasureDecoder::new(config);
    let recovered = decoder.decode(&received)?;
    assert_eq!(recovered, data);

    Ok(())
}
```

### Transport Tier Selection

Automatic optimal transport selection:

```rust
use warp_store::transport::{StorageTransport, TransportConfig, PeerLocation, Tier};

fn main() {
    let config = TransportConfig::auto_detect()
        .with_zone("us-east-1a")
        .with_region("us-east-1");

    let transport = StorageTransport::new(config);

    // Same process -> Tier 0 (<1µs)
    // Same machine -> Tier 1 (2-10µs)
    // Same datacenter -> Tier 2 (1-50µs, RDMA)
    // Cross-site -> Tier 3 (50µs+, WireGuard)

    let peer = PeerLocation::network(
        "peer-1".into(),
        "10.0.0.1:9000".parse().unwrap(),
        Some("us-east-1a".into()),
    );

    let tier = peer.optimal_tier(&transport.local_peer());
    println!("Using {:?} for peer communication", tier);
}
```

### S3-Compatible API

```rust
use warp_store_api::{ApiServer, ApiConfig};
use warp_store::{Store, StoreConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let store = Store::new(StoreConfig::default()).await?;

    let config = ApiConfig {
        bind_addr: "0.0.0.0:9000".parse()?,
        enable_s3: true,
        enable_native: true,
        access_key_id: Some("AKIAIOSFODNN7EXAMPLE".into()),
        secret_access_key: Some("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into()),
        region: "us-east-1".into(),
    };

    let server = ApiServer::new(store, config).await;
    server.run().await?;

    Ok(())
}
```

---

## Roadmap

### Completed

- [x] Core storage engine (warp-store)
- [x] S3-compatible API (warp-store-api)
- [x] Ephemeral URL system with Ed25519 signatures
- [x] Local + Erasure + Distributed backends
- [x] Multi-tier transport layer
- [x] Metrics collection and health checks
- [x] hpc-channels integration
- [x] SeqCDC SIMD chunking (31 GB/s)
- [x] Sparse Merkle verification
- [x] Chonkers versioned deduplication
- [x] OPRF privacy-preserving blind deduplication
- [x] WaLLoC neural compression (ONNX Runtime)
- [x] DPU offload framework (BlueField, Pensando, Intel IPU)
- [x] Portal mesh P2P storage
- [x] Triple-buffer streaming pipeline

### In Progress

- [ ] Raft consensus for distributed metadata
- [ ] Parcode lazy-loading backend
- [ ] Production hardening and security audit

### Planned

- [ ] rmpi collective read/write operations
- [ ] GPU-direct storage (GPUDirect RDMA)
- [ ] WireGuard cross-domain tunnels
- [ ] Horizon dashboard integration
- [ ] slai Brain-Link scheduling integration

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

### Development

```bash
# Run all tests
cargo test --workspace

# Run benchmarks
cargo bench -p warp-io
cargo bench -p warp-ec

# Build with all features
cargo build --workspace --all-features

# Generate documentation
cargo doc --workspace --no-deps --open
```

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
