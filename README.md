# warp

> GPU-accelerated bulk data transfer with adaptive compression, deduplication, and Merkle verification

## Features

- **GPU Acceleration**: nvCOMP integration for parallel compression/decompression
- **Content-Defined Chunking**: Buzhash rolling hash for intelligent deduplication
- **QUIC Transport**: Modern, multiplexed transport with built-in encryption
- **Merkle Verification**: Cryptographic integrity with incremental verification
- **Adaptive Compression**: Per-chunk algorithm selection based on entropy analysis
- **Resume Support**: Interrupted transfers continue from last verified chunk

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

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
