# Warp + Portal

> **GPU-Accelerated Distributed Storage Fabric**

A next-generation platform combining wire-speed data transfer, zero-knowledge encryption, intelligent P2P mesh networking, and cryptographic integrity proofs at petabyte scale.

---

## What Is This?

**Warp** is a GPU-accelerated data transfer engine achieving 20+ GB/s throughput through hardware-accelerated compression and encryption.

**Portal** extends Warp into a complete distributed storage fabric with:
- **Zero-Knowledge Encryption**: Even the infrastructure cannot read your data
- **Ephemeral Access Tunnels**: Time-limited, policy-driven sharing
- **Intelligent P2P Mesh**: WireGuard-based networking with automatic routing
- **Cryptographic Integrity**: Three-layer verification (BLAKE3 + Merkle DAG + KZG)
- **GPU-Accelerated Scheduling**: Real-time optimization across billions of chunks

Together, they deliver a system where data flows like water—always finding the optimal path, automatically routing around failures, and maintaining end-to-end encryption that even the infrastructure cannot break.

---

## Key Capabilities

| Capability | Implementation | Performance |
|------------|----------------|-------------|
| Transfer Speed | GPU compression + encryption | 20+ GB/s |
| Encryption | Zero-knowledge, convergent | ChaCha20-Poly1305 |
| Integrity | BLAKE3 + Merkle DAG + KZG | PB-scale proofs |
| Network | WireGuard P2P mesh | Automatic NAT traversal |
| Routing | GPU-accelerated scheduler | 50ms optimization cycles |
| Failover | Pre-computed backup paths | <50ms recovery |
| Scale | Tested architecture | 1 PB+ single file |

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              PORTAL                                          │
│         Zero-Knowledge │ Ephemeral Portals │ Access Control                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                           INTELLIGENCE                                       │
│        GPU Scheduler │ Auto-Reconciliation │ Predictive Routing              │
├─────────────────────────────────────────────────────────────────────────────┤
│                            INTEGRITY                                         │
│           BLAKE3 Content Addressing │ Merkle DAG │ KZG Proofs                │
├─────────────────────────────────────────────────────────────────────────────┤
│                              WARP                                            │
│       Archive Mode │ Stream Mode │ GPU Compression │ GPU Encryption          │
├─────────────────────────────────────────────────────────────────────────────┤
│                             NETWORK                                          │
│          WireGuard Mesh │ QUIC Protocol │ mDNS Discovery                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/01-ARCHITECTURE.md) | System design and component overview |
| [Warp Engine](docs/02-WARP-ENGINE.md) | GPU-accelerated transfer engine |
| [Portal System](docs/03-PORTAL-SYSTEM.md) | Zero-knowledge distributed storage |
| [Integrity Layer](docs/04-INTEGRITY.md) | Three-layer cryptographic integrity |
| [Network Layer](docs/05-NETWORK.md) | WireGuard mesh and P2P networking |
| [GPU Scheduler](docs/06-SCHEDULER.md) | Real-time chunk scheduling |
| [GPU Acceleration](docs/07-GPU-ACCELERATION.md) | Hardware optimization details |
| [Development Plan](docs/08-DEVELOPMENT-PLAN.md) | Phased implementation roadmap |
| [Glossary](docs/09-GLOSSARY.md) | Terms and definitions |

---

## Hardware Requirements

### Minimum
- CPU: 8 cores, AVX2 support
- RAM: 32 GB
- Storage: NVMe SSD
- Network: 1 Gbps

### Recommended (Full Performance)
- CPU: 16+ cores
- RAM: 64+ GB
- GPU: NVIDIA RTX 3080+ (12+ GB VRAM)
- Storage: NVMe RAID
- Network: 10+ Gbps

### Development Target
- GPU: NVIDIA RTX 5090 FE (32 GB GDDR7)
- Enables: Single-GPU processing up to 2 PB files

---

## Project Structure

```
warp-project/
├── Cargo.toml              # Workspace definition
├── README.md               # This file
├── docs/                   # Documentation
│   ├── 01-ARCHITECTURE.md
│   ├── 02-WARP-ENGINE.md
│   ├── 03-PORTAL-SYSTEM.md
│   ├── 04-INTEGRITY.md
│   ├── 05-NETWORK.md
│   ├── 06-SCHEDULER.md
│   ├── 07-GPU-ACCELERATION.md
│   ├── 08-DEVELOPMENT-PLAN.md
│   └── 09-GLOSSARY.md
├── crates/                 # Rust crates
│   ├── warp-core/          # Common types and traits
│   ├── warp-crypto/        # Encryption and hashing
│   ├── warp-compress/      # Compression backends
│   ├── warp-format/        # Archive format
│   ├── warp-hash/          # BLAKE3 integration
│   ├── warp-io/            # Chunking and I/O
│   ├── warp-net/           # Network protocol
│   └── warp-cli/           # Command-line interface
└── tests/                  # Integration tests
```

---

## Quick Start

```bash
# Clone the repository
git clone https://github.com/your-org/warp-project
cd warp-project

# Build (requires Rust 2024 edition)
cargo build --release

# Run tests
cargo test

# Create an archive
warp archive create ./my-data -o backup.warp

# Extract an archive
warp archive extract backup.warp -o ./restored

# Stream mode (pipe-based)
tar cf - ./data | warp stream encrypt --key KEY | network-send
network-recv | warp stream decrypt --key KEY | tar xf -
```

---

## Unique Value Proposition

What makes Warp + Portal different from existing solutions:

| Feature | Warp + Portal | Dropbox/GDrive | IPFS | BitTorrent |
|---------|---------------|----------------|------|------------|
| Zero-knowledge | ✓ | ✗ | Partial | ✗ |
| GPU acceleration | ✓ | ✗ | ✗ | ✗ |
| Ephemeral sharing | ✓ | ✗ | ✗ | ✗ |
| P2P mesh | ✓ | ✗ | ✓ | ✓ |
| Intelligent routing | ✓ | ✗ | ✗ | ✗ |
| Cryptographic proofs | ✓ (KZG) | ✗ | ✗ | ✗ |
| PB-scale integrity | ✓ | ✗ | Partial | ✗ |

---

## Philosophy

**Your data. Your keys. Your rules.**

Portal is built on the principle that infrastructure should serve users, not surveil them. The Hub coordinates but cannot read. The network routes but cannot inspect. The system optimizes but cannot compromise.

This is a personal, private CDN with the sophistication of Google's B4 WAN or Akamai's routing—but for individuals and small teams, with zero-knowledge encryption.

---

## License

[To be determined]

---

## Contributing

[Contribution guidelines to be added]
