# Glossary

> Terms and Definitions for Warp + Portal

---

## A

**Access Control**
: Rules determining who can access a portal and what operations they can perform. Levels include Owner, Admin, Write, Read, and List.

**Archive Mode**
: Warp's file-oriented transfer mode that creates .warp archives with content-defined chunking, deduplication, and integrity verification.

**Auto-Reconciliation**
: The system's ability to continuously monitor and optimize transfer performance, including drift detection, reoptimization, and predictive pre-positioning.

---

## B

**Backup Path**
: Pre-computed alternative source for a chunk. When the primary source fails, failover switches to a backup path instantly.

**BLAKE3**
: Cryptographic hash function used for content addressing and integrity verification. Chosen for speed (70+ GB/s on GPU) and security.

**Buzhash**
: Rolling hash algorithm used for content-defined chunking. Finds natural chunk boundaries by looking for specific hash patterns.

---

## C

**ChaCha20-Poly1305**
: Authenticated encryption algorithm used for both transport (WireGuard) and content encryption (Portal). Provides confidentiality and integrity.

**Chunk**
: The fundamental unit of data in Portal. Files are split into variable-size chunks (1-16 MB, ~4 MB average) based on content boundaries.

**Chunk ID (CID)**
: The BLAKE3 hash of a chunk's content. Same content always produces the same CID, enabling deduplication.

**Content Addressing**
: Identifying data by its cryptographic hash rather than its location. Same content = same address, regardless of where it's stored.

**Content-Defined Chunking (CDC)**
: Splitting files into chunks based on content patterns rather than fixed positions. Enables efficient deduplication even when content shifts.

**Convergent Encryption**
: Encryption where the key is derived from the content. Same plaintext → same ciphertext, enabling deduplication of encrypted data.

**Cost Matrix**
: GPU data structure mapping (chunk, edge) pairs to cost scores. Updated each scheduler tick to reflect current network conditions.

**Cryptokey Routing**
: WireGuard's approach where routing decisions are based on peer public keys rather than IP addresses. Packets to a peer are encrypted to that peer's key.

---

## D

**DAG (Directed Acyclic Graph)**
: Graph structure where edges have direction and no cycles exist. Portal's Merkle DAG allows content to be referenced multiple times (deduplication) without duplication.

**Deduplication (Dedup)**
: Eliminating duplicate content by storing each unique chunk only once, regardless of how many files reference it.

**Dispatch Queue**
: Output of the GPU scheduler each tick, containing chunk assignments for the transfer executor to process.

---

## E

**Edge**
: A device participating in the Portal network. Can be a personal device (laptop, phone), server (NAS, cloud VM), or the Hub itself.

**Edge Intelligence**
: System components that track edge state, health, bandwidth, RTT, and constraints to inform scheduling decisions.

**EMA (Exponential Moving Average)**
: Statistical technique used for bandwidth and RTT estimation, balancing responsiveness with stability.

**Ephemeral Portal**
: A portal with a defined lifecycle that automatically expires based on time, usage, or other policies.

---

## F

**Failover**
: Automatic switching from a failed source to a backup source. Portal achieves failover in <50ms using pre-computed backup paths.

**FFT (Fast Fourier Transform)**
: Algorithm used in KZG commitment generation. Portal uses GPU-accelerated NTT (Number Theoretic Transform) variant.

---

## G

**GPU Acceleration**
: Using graphics processing units for compute-intensive operations like hashing, compression, encryption, and scheduling.

**Guest**
: A user accessing a portal they don't own. Guests may have various access levels (Read, Write, etc.) granted by the owner.

---

## H

**Health Score**
: Composite metric (0.0-1.0) representing an edge's reliability, combining success rate, uptime, and consistency.

**Hub**
: Central coordinator in the Portal network. Provides peer discovery, metadata sync, and relay fallback. Cannot access plaintext data (zero-knowledge).

---

## I

**Integrity**
: Guarantee that data has not been modified or corrupted. Portal uses three-layer integrity: BLAKE3, Merkle DAG, and KZG.

---

## K

**K-Best Paths**
: Pre-computed list of top K source edges for each chunk, ordered by cost. Enables instant failover without recomputation.

**KZG (Kate-Zaverucha-Goldberg)**
: Polynomial commitment scheme enabling constant-size proofs regardless of data size. Used for external audits and compact verification.

**Key Hierarchy**
: Structured derivation of multiple keys from a single master seed. Enables separate keys for encryption, signing, authentication, and devices.

---

## L

**Lifecycle Policy**
: Rules governing a portal's active periods. Can be time-based, usage-based, presence-based, or composite.

**Load Balancing**
: Redistributing chunk assignments to prevent any single edge from becoming overloaded while others are underutilized.

---

## M

**Manifest**
: Metadata describing a file's structure: ordered list of chunk CIDs, filename, permissions, etc. Encrypted with user's master key.

**Master Seed**
: 512-bit value derived from recovery phrase. Root of the key hierarchy from which all other keys are derived.

**mDNS (Multicast DNS)**
: Protocol for discovering services on local networks. Portal uses mDNS to find edges on the same LAN for direct transfer.

**Merkle DAG**
: Content-addressed directed acyclic graph where nodes are identified by their hash. Enables deduplication, versioning, and efficient sync.

**Merkle Tree**
: Binary tree where each non-leaf node is the hash of its children. Enables efficient verification and corruption localization.

**MSM (Multi-Scalar Multiplication)**
: Core operation in KZG commitment: computing Σ sᵢ × Gᵢ for many scalars and elliptic curve points. GPU-parallelizable.

---

## N

**NAT Traversal**
: Techniques for establishing direct connections between devices behind Network Address Translation. WireGuard handles this automatically.

**nvCOMP**
: NVIDIA's GPU-accelerated compression library supporting LZ4, Zstd, and other algorithms at high throughput.

**NTT (Number Theoretic Transform)**
: FFT variant operating over finite fields, used in KZG polynomial evaluation.

---

## O

**Owner**
: The user who created a portal and has full control over it, including access grants and lifecycle management.

---

## P

**P2P (Peer-to-Peer)**
: Direct communication between edges without routing through the Hub. Preferred when NAT conditions allow.

**Pinned Memory**
: Host memory locked in physical RAM, enabling DMA transfers between CPU and GPU without page faults.

**Portal**
: An ephemeral access tunnel to shared content with policy-driven lifecycle and access control.

**Pre-positioning**
: Proactively copying chunks to edges where they're likely to be needed, based on predicted access patterns.

---

## Q

**QUIC**
: Modern transport protocol (RFC 9000) with multiplexing, built-in encryption, and connection migration. Used for Portal transfers.

---

## R

**Recovery Phrase**
: 24-word BIP-39 mnemonic that encodes the master seed. Only way to recover access if all devices are lost.

**Re-Entry Flow**
: Process for requesting access to an expired portal. Guest submits request, owner approves/denies, temporary access may be granted.

**Roaming**
: Continuing transfers seamlessly when a device's IP address changes. WireGuard handles this automatically via endpoint updates.

---

## S

**Scheduler**
: GPU component that makes real-time decisions about chunk routing, running in 50ms tick cycles.

**SRS (Structured Reference String)**
: Pre-computed data required for KZG operations, generated via trusted setup ceremony.

**Stream Mode**
: Warp's real-time transfer mode with <5ms latency, using fixed-size chunks and triple-buffered pipeline.

**Swarm Download**
: Fetching chunks from multiple sources simultaneously, similar to BitTorrent. Maximizes aggregate bandwidth.

---

## T

**Tick**
: One cycle of the scheduler loop (50ms). Each tick updates costs, detects failures, and generates new dispatch decisions.

**Triple Buffer**
: Pipeline technique using three buffers to overlap read, process, and write operations for maximum throughput.

**Trusted Setup**
: Ceremony generating KZG's SRS where participants contribute randomness and destroy their secrets. Security relies on at least one honest participant.

---

## V

**Virtual IP**
: IP address in Portal's internal address space (10.portal.0.0/16). Each edge receives a virtual IP for WireGuard routing.

**Virtual Tree**
: Merkle tree computed on-demand from the Merkle DAG for a specific file. Enables standard Merkle proofs while storing as DAG.

---

## W

**Warp**
: GPU-accelerated data transfer engine providing 20+ GB/s throughput for both archive and streaming modes.

**WireGuard**
: Modern VPN protocol used for Portal's network layer. Provides encryption, NAT traversal, and roaming.

---

## Z

**Zero-Knowledge**
: Property where the Hub and network infrastructure cannot access plaintext data. Only users with keys can decrypt.

**Zstd (Zstandard)**
: Compression algorithm offering better ratios than LZ4 at lower speeds. Portal uses GPU-accelerated Zstd via nvCOMP.

---

## Acronyms

| Acronym | Expansion |
|---------|-----------|
| BIP-39 | Bitcoin Improvement Proposal 39 (mnemonic standard) |
| CDC | Content-Defined Chunking |
| CID | Content Identifier (chunk hash) |
| CUDA | Compute Unified Device Architecture (NVIDIA) |
| DAG | Directed Acyclic Graph |
| DMA | Direct Memory Access |
| EMA | Exponential Moving Average |
| FFT | Fast Fourier Transform |
| HKDF | HMAC-based Key Derivation Function |
| KZG | Kate-Zaverucha-Goldberg (commitment scheme) |
| LZ4 | Lempel-Ziv 4 (compression algorithm) |
| MAC | Message Authentication Code |
| mDNS | Multicast DNS |
| MSM | Multi-Scalar Multiplication |
| NAT | Network Address Translation |
| NTT | Number Theoretic Transform |
| P2P | Peer-to-Peer |
| QUIC | Quick UDP Internet Connections |
| RTT | Round-Trip Time |
| SRTT | Smoothed Round-Trip Time |
| SRS | Structured Reference String |
| TDP | Thermal Design Power |
| VRAM | Video Random Access Memory |
| Zstd | Zstandard (compression algorithm) |
