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
- [Deployment](#deployment)
  - [Single Node Setup](#single-node-setup)
  - [Multi-Node Cluster](#multi-node-cluster)
  - [Shared Storage Access](#shared-storage-access)
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

## Deployment

### Single Node Setup

Deploy WARP on a single server for development or small-scale use.

#### Prerequisites

- **OS**: Linux (Ubuntu 22.04+, RHEL 8+) or macOS 13+
- **Rust**: 1.85+ (for building from source)
- **RAM**: 8GB minimum, 32GB+ recommended
- **Storage**: SSD recommended for metadata, HDD acceptable for data
- **FUSE** (optional): libfuse3-dev (Linux) or macFUSE (macOS)

#### Build from Source

```bash
git clone https://github.com/osobh/warp.git
cd warp
cargo build --release --workspace
```

#### Directory Structure

```bash
sudo mkdir -p /var/lib/warp/{data,meta,logs}
sudo mkdir -p /etc/warp
sudo chown -R $USER:$USER /var/lib/warp /etc/warp
```

#### Configuration

Create `/etc/warp/config.toml`:

```toml
[server]
bind_addr = "0.0.0.0:9000"
data_dir = "/var/lib/warp/data"
meta_dir = "/var/lib/warp/meta"

[storage]
max_object_size = "5TB"
default_versioning = "disabled"
compression = "zstd"
compression_level = 3

[security]
# Generate with: openssl rand -hex 20 | tr '[:lower:]' '[:upper:]'
access_key_id = "YOUR_ACCESS_KEY_ID"
# Generate with: openssl rand -base64 40
secret_access_key = "YOUR_SECRET_ACCESS_KEY"
region = "us-east-1"

[erasure]
enabled = true
data_shards = 10
parity_shards = 4

[logging]
level = "info"
file = "/var/lib/warp/logs/warp.log"
```

#### Start the Server

```bash
# Foreground (for testing)
./target/release/warp-store-api --config /etc/warp/config.toml

# Or with environment variables
WARP_BIND_ADDR=0.0.0.0:9000 \
WARP_DATA_DIR=/var/lib/warp/data \
./target/release/warp-store-api
```

#### Systemd Service

Create `/etc/systemd/system/warp.service`:

```ini
[Unit]
Description=WARP Storage Server
After=network.target

[Service]
Type=simple
User=warp
Group=warp
ExecStart=/usr/local/bin/warp-store-api --config /etc/warp/config.toml
Restart=on-failure
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable warp
sudo systemctl start warp
sudo systemctl status warp
```

#### Verify Installation

```bash
# Check server is running
curl http://localhost:9000/health

# Create a bucket
aws --endpoint-url http://localhost:9000 s3 mb s3://test-bucket

# Upload a file
aws --endpoint-url http://localhost:9000 s3 cp /etc/hosts s3://test-bucket/

# List objects
aws --endpoint-url http://localhost:9000 s3 ls s3://test-bucket/
```

---

### Multi-Node Cluster

Deploy WARP across multiple nodes for high availability and scale.

#### Cluster Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    WARP Cluster Architecture                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌──────────────┐    ┌──────────────┐    ┌──────────────┐     │
│   │ Coordinator  │    │   Storage    │    │   Storage    │     │
│   │   (Leader)   │◄──►│   Node 1     │◄──►│   Node 2     │     │
│   │  Raft + API  │    │  Raft + Data │    │  Raft + Data │     │
│   └──────┬───────┘    └──────────────┘    └──────────────┘     │
│          │                                                       │
│          │ API Requests                                          │
│          ▼                                                       │
│   ┌──────────────┐    ┌──────────────┐    ┌──────────────┐     │
│   │   Gateway    │    │   Compute    │    │   Compute    │     │
│   │  NFS / SMB   │    │  FUSE Mount  │    │  FUSE Mount  │     │
│   └──────────────┘    └──────────────┘    └──────────────┘     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Node Roles

| Role | Purpose | Min Nodes | Services |
|------|---------|-----------|----------|
| **Coordinator** | Raft leader, API endpoint | 1 (3 for HA) | warp-store-api |
| **Storage** | Data storage, Raft follower | 1+ | warp-store-api |
| **Gateway** | Protocol translation | 0+ | warp-nfs, warp-smb |
| **Compute** | Data consumers | 0+ | warp-fs mount |

#### Step 1: Initialize Coordinator

On the coordinator node, create `/etc/warp/config.toml`:

```toml
[server]
bind_addr = "0.0.0.0:9000"
data_dir = "/var/lib/warp/data"
node_name = "coordinator"

[raft]
enabled = true
node_id = 1
bind_addr = "0.0.0.0:9001"
init_cluster = true  # Only on first node!

[cluster]
peers = []
```

Start the coordinator:

```bash
./target/release/warp-store-api --config /etc/warp/config.toml
```

#### Step 2: Join Storage Nodes

On each storage node, create `/etc/warp/config.toml`:

```toml
[server]
bind_addr = "0.0.0.0:9000"
data_dir = "/data/warp"
node_name = "storage-1"

[raft]
enabled = true
node_id = 2  # Unique per node: 2, 3, 4...
bind_addr = "0.0.0.0:9001"
init_cluster = false
join_addr = "coordinator:9001"
```

Start and join:

```bash
./target/release/warp-store-api --config /etc/warp/config.toml
```

Verify cluster status:

```bash
curl http://coordinator:9000/api/v1/cluster/status
```

#### Step 3: Configure Gateways

NFS Gateway (`/etc/warp/nfs.toml`):

```toml
[server]
bind_addr = "0.0.0.0:2049"

[backend]
endpoints = ["coordinator:9000", "storage-1:9000", "storage-2:9000"]

[[exports]]
path = "/"
bucket = "shared-data"
allowed_clients = ["10.0.0.0/8", "192.168.0.0/16"]
read_only = false
```

SMB Gateway (`/etc/warp/smb.toml`):

```toml
[server]
bind_addr = "0.0.0.0:445"

[backend]
endpoints = ["coordinator:9000"]

[[shares]]
name = "data"
bucket = "shared-data"
path = "/"
read_only = false
```

Start gateways:

```bash
./target/release/warp-nfs --config /etc/warp/nfs.toml
./target/release/warp-smb --config /etc/warp/smb.toml
```

#### Step 4: Mount on Compute Nodes

FUSE mount:

```bash
./target/release/warp-fs mount \
  --endpoint coordinator:9000 \
  --bucket shared-data \
  /mnt/warp
```

Or via NFS:

```bash
mount -t nfs4 gateway:/ /mnt/warp
```

Or via `/etc/fstab`:

```
# NFS
gateway:/ /mnt/warp nfs4 defaults,_netdev 0 0
```

---

### Shared Storage Access

WARP provides multiple protocols for accessing shared storage:

#### FUSE Filesystem (warp-fs)

Mount WARP buckets as local filesystems with POSIX semantics.

```bash
# Mount a bucket
./target/release/warp-fs mount \
  --endpoint localhost:9000 \
  --bucket my-bucket \
  --cache-size 1G \
  /mnt/warp

# Mount with options
./target/release/warp-fs mount \
  --endpoint localhost:9000 \
  --bucket my-bucket \
  --read-only \
  --allow-other \
  --cache-size 4G \
  --cache-ttl 300 \
  /mnt/warp

# Unmount
fusermount -u /mnt/warp      # Linux
umount /mnt/warp             # macOS
```

**Performance Tips:**
- Use `--cache-size` for read-heavy workloads
- Use `--direct-io` for large sequential I/O
- Set `--max-background` for parallel operations

#### NFS Gateway (warp-nfs)

Export WARP buckets via NFSv4.1 with pNFS support.

**Server:**

```bash
./target/release/warp-nfs \
  --bind 0.0.0.0:2049 \
  --backend localhost:9000 \
  --export shared-data:/
```

**Client:**

```bash
# Linux
mount -t nfs4 -o vers=4.1 server:/ /mnt/warp

# With tuning
mount -t nfs4 -o vers=4.1,rsize=1048576,wsize=1048576 server:/ /mnt/warp
```

#### SMB Gateway (warp-smb)

Share WARP buckets via SMB3 for Windows and cross-platform access.

**Server:**

```bash
./target/release/warp-smb \
  --bind 0.0.0.0:445 \
  --backend localhost:9000 \
  --share data:shared-data
```

**Clients:**

```bash
# Linux (CIFS)
mount -t cifs -o username=guest //server/data /mnt/warp

# macOS
mount -t smbfs //guest@server/data /mnt/warp

# Windows
net use Z: \\server\data
```

#### Block Device (warp-block)

Export WARP volumes as network block devices via NBD.

**Server:**

```bash
# Create a thin-provisioned volume
./target/release/warp-block volume create \
  --name myvolume \
  --size 100G \
  --pool default

# Start NBD server
./target/release/warp-block serve \
  --bind 0.0.0.0:10809 \
  --volume myvolume
```

**Client:**

```bash
# Load NBD kernel module
sudo modprobe nbd

# Connect to NBD server
sudo nbd-client server 10809 /dev/nbd0

# Create filesystem and mount
sudo mkfs.ext4 /dev/nbd0
sudo mount /dev/nbd0 /mnt/block

# Disconnect
sudo umount /mnt/block
sudo nbd-client -d /dev/nbd0
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
