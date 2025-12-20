# Changelog

All notable changes to Warp will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-XX-XX

### Added

#### Core Features
- Content-defined chunking with Buzhash rolling hash
- Zstd and LZ4 compression with automatic selection
- BLAKE3 hashing with parallel processing via rayon
- ChaCha20-Poly1305 AEAD encryption
- Ed25519 digital signatures
- Argon2 password-based key derivation
- Merkle tree integrity verification

#### Archive Format
- Native `.warp` archive format
- Memory-mapped reading for zero-copy access
- Streaming write support
- Directory tree preservation
- File metadata (permissions, timestamps)

#### Networking
- QUIC transport with TLS 1.3
- Multiplexed streams
- Connection resumption
- Protocol frame codec (Hello, Capabilities, Plan, Chunk, Ack, Nack, Verify)

#### Transfer Features
- Multi-source parallel downloads
- Transfer scheduling with bandwidth allocation
- Session persistence and resume
- Progress tracking with ETA
- Automatic retry on failure

#### GPU Acceleration
- nvCOMP integration for parallel compression
- CUDA kernel optimization framework
- Automatic GPU detection and fallback

#### Portal System
- Zero-knowledge encryption
- P2P mesh networking
- Edge node federation
- Hub-based relay

#### Observability
- Structured logging with tracing
- Metrics collection
- Telemetry spans for performance analysis

#### CLI
- `warp send` - Send files to remote
- `warp fetch` - Fetch files from remote
- `warp listen` - Start receiver daemon
- `warp plan` - Analyze transfer before execution

### Security
- Constant-time cryptographic operations
- Secure key derivation
- Input validation
- No unsafe code in critical paths

## [Unreleased]

### Added
- **SeqCDC Algorithm**: High-performance content-defined chunking replacing Buzhash
  - 100x faster than legacy Buzhash (31 GB/s vs 300 MB/s)
  - Monotonic sequence detection for boundary finding
  - Content-based skipping for unfavorable regions
- **SIMD Acceleration**: Platform-optimized vectorization
  - ARM NEON (128-bit): 12-31 GB/s on Apple Silicon
  - x86_64 AVX2 (256-bit): 15-20 GB/s
  - x86_64 AVX-512 (512-bit): 30+ GB/s
  - Automatic runtime detection and dispatch
- **Backward Compatibility**: Legacy `BuzhashChunker` still available
- **Comprehensive Benchmarks**: Criterion benchmarks for all chunking variants
- **Zero-Copy QUIC**: Frame types use `Bytes` instead of `Vec<u8>` for zero-copy
  - Eliminated `.to_vec()` calls in decode paths
  - Updated transport layer for zero-copy chunk handling
- **Sparse Merkle Trees** (`warp-format`): Lazy computation with LRU caching
  - `SparseMerkleTree` for memory-efficient large archives
  - O(log n) single-chunk verification via `MerkleProof`
  - Parallel root computation using rayon
  - Configurable node cache with LRU eviction
- **WarpReader Verification**: Fast integrity checking
  - `open_with_verification()` builds sparse tree from chunk index
  - `verify_chunk_fast()` for O(log n) single-chunk verification
  - `verify_random_sample()` for probabilistic spot-checking
  - Works with both encrypted and unencrypted archives
- **Reed-Solomon Erasure Coding** (`warp-ec` crate): Fault-tolerant data encoding
  - SIMD-optimized via `reed-solomon-simd` (AVX2, SSSE3, NEON)
  - Preset configurations: RS(4,2), RS(6,3), RS(10,4), RS(16,4)
  - `ErasureEncoder` for data-to-shard encoding
  - `ErasureDecoder` for recovery from partial shard sets
  - Shard metadata tracking (data vs parity, indices)
  - Survives up to m shard failures with RS(k,m) encoding

### Changed
- Default chunker now uses SeqCDC instead of Buzhash
- `Chunker` type alias points to `SeqCdcChunker`
- Updated warp-io documentation with new algorithm details
- Frame enum in warp-net uses `Bytes` for chunk data (zero-copy)
- WarpReader struct includes optional `SparseMerkleTree` field

### Planned
- WebSocket transport fallback
- Browser-based transfers
- Cloud storage backends
- Incremental sync
