# Technical Context

## Technology Stack

### Core (Warp Engine)
| Category | Technology | Version |
|----------|------------|---------|
| Language | Rust | 1.85+ (Edition 2024) |
| Async Runtime | Tokio | 1.48 (full features) |
| Network Transport | Quinn (QUIC) | 0.11 |
| TLS | rustls | 0.23 |
| CPU Compression | zstd, lz4_flex | 0.13, 0.11 |
| GPU Framework | cudarc | **0.18.1** (CUDA 12.x) |
| GPU Compression | nvCOMP | via cudarc |
| Hashing | BLAKE3 | 1.8 (with rayon, mmap) |
| Encryption | chacha20poly1305 | 0.10 |
| Signatures | ed25519-dalek | 2.1 |
| Key Exchange | x25519-dalek | 2.0 |
| KDF | argon2 | 0.5 |
| Serialization | serde + rmp-serde | 1.0, 1.3 |
| Parallelism | rayon | 1.11 |
| CLI | clap | 4.5 |

### GPU Acceleration (Phase 3-4)
| Category | Technology | Version |
|----------|------------|---------|
| CUDA Bindings | cudarc | 0.18.1 |
| Context Management | CudaContext | cudarc::driver::safe |
| Stream Management | CudaStream | cudarc::driver::safe |
| Memory | PinnedBuffer, CudaSlice | cudarc::driver::safe |
| GPU BLAKE3 | Custom PTX kernel | warp-gpu |
| GPU ChaCha20 | Custom PTX kernel | warp-gpu |

### Portal Extensions (Future)
| Category | Technology | Version | Phase |
|----------|------------|---------|-------|
| VPN | WireGuard (boringtun) | 0.6 | 6 |
| Discovery | mdns-sd | 0.10 | 6 |
| BIP-39 | bip39 | 2.0 | 5 |
| GPU Scheduler | cudarc | 0.18.1 | 8 |
| KZG Commitments | ark-bls12-381 | 0.4 | 5 |
| Polynomial | ark-poly | 0.4 | 5 |
| Web Server | axum | 0.7 | 5, 12 |
| Database | sled or rocksdb | TBD | 5 |

## Development Setup

```bash
# Prerequisites
rustup default nightly  # Rust 1.85+ required
# For GPU support: CUDA toolkit 12.x, nvCOMP library

# Build
cargo build --release

# Build with GPU support
cargo build --release --features gpu

# Run tests
cargo test --all

# Run benchmarks
cargo bench

# Install CLI
cargo install --path crates/warp-cli
```

## Key Dependencies

| Dependency | Version | Purpose |
|------------|---------|---------|
| tokio | 1.48 | Async runtime with full features |
| quinn | 0.11 | QUIC transport implementation |
| rustls | 0.23 | TLS 1.3 for QUIC encryption |
| zstd | 0.13 | High-ratio compression (CPU) |
| lz4_flex | 0.11 | Fast compression (CPU) |
| blake3 | 1.8 | High-speed cryptographic hashing |
| chacha20poly1305 | 0.10 | AEAD encryption |
| ed25519-dalek | 2.1 | Digital signatures |
| x25519-dalek | 2.0 | Key exchange |
| argon2 | 0.5 | Password-based key derivation |
| rayon | 1.11 | Data parallelism |
| clap | 4.5 | CLI argument parsing |
| bytes | 1.7 | Efficient byte buffer handling |
| memmap2 | 0.9 | Memory-mapped file I/O |
| **cudarc** | **0.18.1** | CUDA bindings (GPU acceleration) |
| thiserror | 2.0 | Error type derivation |
| indicatif | 0.17 | Progress bars |
| dashmap | 6.1 | Concurrent hash maps |
| criterion | 0.5 | Benchmarking framework |

## Technical Constraints

1. **Rust Edition 2024**: Using latest language features, requires nightly toolchain
2. **QUIC/TLS**: Quinn requires valid TLS certificates for production use
3. **GPU Support**: Requires NVIDIA GPU with CUDA 12.x and nvCOMP library
4. **Memory**: Large transfers require buffer memory (~256MB recommended)
5. **File Handles**: May need ulimit adjustment for many-file archives
6. **Chunk Size**: Minimum 1MB to amortize overhead, maximum 64MB for GPU memory

## Development Patterns

### Error Handling
```rust
// Each crate defines its own Error enum
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Description: {0}")]
    Custom(String),
}
pub type Result<T> = std::result::Result<T, Error>;
```

### Async Convention
- Use `async fn` for I/O-bound operations
- Use rayon for CPU-bound parallelism (hashing, compression)
- Bridge with `spawn_blocking` when needed

### Testing Pattern
- Unit tests inline in src files
- Integration tests in /tests directory
- Benchmarks with Criterion, report throughput in bytes/sec

### Linting
- Clippy pedantic + nursery
- `unsafe_code` warned
- `missing_docs` warning enabled

## Build Profiles

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true

[profile.bench]
inherits = "release"
debug = true  # For profiling
```
