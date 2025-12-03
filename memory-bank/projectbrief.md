# Project Brief

## Project Name
Warp + Portal - GPU-Accelerated Distributed Storage Fabric

## Overview
Warp + Portal is a comprehensive distributed storage and transfer platform combining:

**Warp Engine**: A GPU-accelerated data transfer engine achieving 20+ GB/s throughput with content-defined chunking, adaptive compression, and Merkle verification.

**Portal**: A distributed zero-knowledge storage fabric with ephemeral portals, WireGuard P2P mesh networking, GPU-accelerated chunk scheduling, and swarm downloads from multiple sources.

The project operates in multiple modes:
- **Archive Mode**: Create and extract .warp archive files with deduplication and Merkle verification
- **Stream Mode**: Real-time encrypted streaming with <5ms latency
- **Portal Mode**: Ephemeral access tunnels with lifecycle policies and access control
- **Swarm Mode**: Multi-source parallel downloads from distributed edges

## Core Requirements

### Warp Engine (Phases 1-4)
- [x] Content-defined chunking using Buzhash rolling hash (1MB-16MB chunks)
- [x] BLAKE3 hashing with parallel computation support
- [x] CPU compression (zstd, lz4) with adaptive algorithm selection
- [x] ChaCha20-Poly1305 encryption with Ed25519 signatures
- [x] QUIC transport with multiplexed streams (quinn)
- [x] Merkle tree verification for incremental integrity checking
- [x] Resume support for interrupted transfers
- [x] Native .warp archive format with O(1) file lookup
- [ ] GPU-accelerated compression (>15 GB/s via nvCOMP)
- [ ] GPU-accelerated encryption (>20 GB/s via CUDA)
- [ ] Stream Mode (<5ms latency, triple-buffer pipeline)

### Portal System (Phases 5-10)
- [ ] Zero-knowledge encryption (convergent, content-addressed)
- [ ] Key hierarchy (master seed → per-purpose keys, BIP-39 recovery)
- [ ] Portal lifecycle (create, activate, deactivate, expire)
- [ ] Access control (owner, explicit, link-based, re-entry)
- [ ] WireGuard P2P mesh with mDNS discovery
- [ ] Edge intelligence (health scoring, bandwidth estimation)
- [ ] GPU chunk scheduler (10M chunks in <10ms, <50ms failover)
- [ ] Swarm downloads (multi-source, >90% aggregate bandwidth)
- [ ] Auto-reconciliation (self-healing, predictive pre-positioning)
- [ ] Hub server (peer directory, relay, zero-knowledge)

### Production (Phases 11-12)
- [ ] Production hardening (error handling, security audit)
- [ ] Ecosystem tools (web dashboard, mobile companion)
- [ ] Packaging (deb, rpm, brew, Docker)

## Goals
- **Primary**: 20+ GB/s GPU-accelerated transfers with zero-knowledge security
- **Secondary**: Distributed storage fabric with real-time chunk scheduling
- **Tertiary**: Production-ready platform with ecosystem tools

## Scope
- **In Scope**:
  - Warp Engine: Archive, Stream, and Transfer modes
  - Portal: Zero-knowledge portals, lifecycle, access control
  - Network: WireGuard mesh, mDNS, Hub coordination
  - Scheduling: GPU chunk scheduler, swarm orchestration
  - CLI: Complete command set for all operations
  - Cross-platform: Linux primary, macOS/Windows secondary

- **Out of Scope (v1.0)**:
  - Monetization features (paid gates, wormholes, storage tiers)
  - Multi-GPU scheduling
  - AMD/Intel GPU support
  - GUI interface

- **Post-v1.0 Extensions**:
  - Access tiers (free, paid, proof-gated)
  - Wormhole services (routing, storage tiers)
  - Capability monetization (routes, storage, compute)
  - QR code integration

## Success Criteria
1. GPU compression throughput >15 GB/s
2. GPU encryption throughput >20 GB/s
3. Stream mode latency <5ms for 64KB chunks
4. Schedule 10M chunks in <10ms GPU time
5. Failover to backup paths in <50ms
6. Multi-source downloads achieve >90% aggregate bandwidth
7. Transfer success rate >99.9%
8. Zero-knowledge compliance 100% (Hub cannot decrypt)

## Development Strategy
- **Approach**: Sequential (Warp Engine first, then Portal)
- **Team**: Solo development
- **MVP**: Full system through GPU Chunk Scheduler (Phase 8)
- **Extensions**: After Phase 12 completion
