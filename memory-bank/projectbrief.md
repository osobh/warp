# Project Brief

## Project Name
Warp - GPU-Accelerated Bulk Data Transfer Tool

## Overview
Warp is a high-performance data transfer tool designed for moving large datasets between systems with maximum efficiency. It leverages GPU acceleration (nvCOMP), content-defined chunking (Buzhash), adaptive compression, and modern QUIC transport to achieve transfer speeds that significantly outperform traditional tools like rsync, scp, or tar.

The project operates in two primary modes:
- **Archive Mode**: Create and extract .warp archive files with deduplication and Merkle verification
- **Stream Mode**: Direct peer-to-peer transfer with capability negotiation and resume support

## Core Requirements
- [x] Content-defined chunking using Buzhash rolling hash (1MB-16MB chunks)
- [x] BLAKE3 hashing with parallel computation support
- [x] CPU compression (zstd, lz4) with adaptive algorithm selection
- [ ] GPU-accelerated compression via nvCOMP
- [ ] GPU-accelerated encryption via ChaCha20-Poly1305
- [ ] QUIC transport with multiplexed streams (quinn)
- [ ] Merkle tree verification for incremental integrity checking
- [ ] ChaCha20-Poly1305 encryption with Ed25519 signatures
- [ ] Resume support for interrupted transfers
- [ ] Native .warp archive format with O(1) file lookup

## Goals
- **Primary**: Achieve 10GB/s+ transfer speeds on modern hardware with GPU acceleration
- **Secondary**: Provide seamless rsync-like user experience with `warp send` and `warp fetch`
- **Tertiary**: Enable secure, verifiable transfers with cryptographic integrity guarantees

## Scope
- **In Scope**:
  - Archive Mode: Local .warp file creation and extraction
  - Stream Mode: Network transfers between warp nodes
  - GPU acceleration for compression/decompression (NVIDIA via nvCOMP)
  - Cross-platform support (Linux primary, macOS/Windows secondary)
  - CLI interface with send, fetch, listen, plan, probe, info, resume, bench commands

- **Out of Scope**:
  - GUI interface (CLI only for v1.0)
  - Cloud storage integration (S3, GCS, etc.)
  - Multi-GPU support (single GPU for v1.0)
  - AMD/Intel GPU support (NVIDIA only for v1.0)

## Success Criteria
1. Transfer 1TB dataset in under 2 minutes on 100Gbps network with GPU
2. Achieve >95% bandwidth utilization on transfers over 1GB
3. Support resume of interrupted transfers with zero data loss
4. Pass all Merkle verification checks with cryptographic guarantees
5. CPU fallback achieves at least 5GB/s throughput
