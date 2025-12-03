# Product Context

## Problem Statement

### Warp Engine Problem
Moving large datasets (100GB to multi-TB) between servers is painfully slow with existing tools:

- **rsync**: Sequential processing, no GPU, limited parallelism, restarts on failure
- **scp/sftp**: No compression intelligence, no deduplication, no resume
- **tar + transfer**: Two-step process, no incremental verification, full restart on corruption
- **Commercial solutions**: Expensive, proprietary, vendor lock-in

### Portal Problem
Distributed file sharing and collaboration is fragmented:

- **Cloud Storage**: Vendor lock-in, privacy concerns, recurring costs
- **NAS/File Servers**: Single point of failure, no geographic distribution
- **P2P Tools**: Complex setup, no zero-knowledge security
- **Traditional VPNs**: Centralized, no content-aware routing

Data scientists, ML engineers, and system administrators waste hours waiting for transfers, often having to restart from scratch when transfers fail at 90% completion. Organizations lack a unified way to share large datasets securely across locations with intelligent routing and automatic failover.

## Target Users

### Warp Engine Users
1. **ML/AI Engineers**: Moving training datasets and model checkpoints between GPU clusters
2. **Data Engineers**: Migrating data warehouses, backing up large databases
3. **System Administrators**: Server migrations, disaster recovery, cross-datacenter replication
4. **Research Scientists**: Sharing large experimental datasets between institutions
5. **DevOps Teams**: Deploying large application artifacts and container images

### Portal Users
1. **Distributed Teams**: Secure file sharing with zero-knowledge encryption
2. **Research Consortiums**: Cross-institutional data sharing with access control
3. **Content Creators**: High-speed delivery of large media files
4. **Enterprises**: Private cloud alternative with geographic redundancy
5. **Infrastructure Operators**: Monetize excess storage/bandwidth capacity

## User Journey

### Archive Mode
1. **Create Archive**: `warp send ./data ./backup.warp`
   - Warp walks directory, analyzes content entropy
   - Chunks data using Buzhash for deduplication
   - Compresses chunks adaptively (zstd for text, lz4 for binaries, none for compressed files)
   - Builds Merkle tree for verification
   - Writes .warp archive with index for O(1) lookup

2. **Transfer Archive**: `warp send ./backup.warp server:/archives/`
   - Probes remote server capabilities (GPU, compression support)
   - Negotiates optimal parameters (chunk size, parallelism, algorithm)
   - Sends HAVE list for deduplication (skip chunks receiver already has)
   - Transfers unique chunks via parallel QUIC streams
   - Verifies Merkle root matches

3. **Extract Archive**: `warp fetch server:/archives/backup.warp ./restored/`
   - Downloads and verifies archive
   - Extracts files with original permissions and timestamps
   - Validates each file against stored hash

## Key Features

### Warp Engine Features
| Feature | Description |
|---------|-------------|
| **Blazing Fast** | GPU acceleration provides 10x+ speedup over CPU-only tools |
| **Smart Compression** | Automatically selects optimal algorithm per chunk based on entropy |
| **Resumable** | Interrupted transfers continue from last verified chunk |
| **Verifiable** | Merkle proofs ensure data integrity at every step |
| **Deduplicated** | Only transfers unique chunks, even across different files |
| **Secure** | Optional ChaCha20 encryption with Ed25519 signatures |
| **Simple CLI** | Familiar rsync-like syntax (`warp send src dest`) |

### Portal Features
| Feature | Description |
|---------|-------------|
| **Zero-Knowledge** | Hub cannot access plaintext data |
| **Ephemeral Portals** | Time-limited access with automatic expiration |
| **P2P Mesh** | WireGuard-based encrypted mesh networking |
| **Swarm Downloads** | BitTorrent-style parallel fetch from multiple sources |
| **GPU Scheduling** | Real-time routing decisions for billions of chunks |
| **Self-Healing** | Automatic failover and reoptimization |
| **Edge Intelligence** | Bandwidth estimation, health scoring |

## User Experience Goals
- Zero-configuration defaults that "just work" optimally
- Progress bar with accurate ETA and throughput display
- Clear error messages with actionable remediation
- Minimal learning curve for rsync/scp users
- Portal creation as simple as sharing a link
- Automatic discovery of local peers

## Business Value
- Reduces transfer time from hours to minutes for large datasets
- Eliminates failed transfer restarts with resume capability
- Provides cryptographic audit trail for compliance
- Zero-knowledge architecture for privacy compliance
- Geographic redundancy without cloud vendor lock-in
- Open source with permissive licensing (MIT/Apache-2.0)

## Portal User Journeys

### Create Portal
1. `portal create ./data --expires 24h`
   - Chunks, compresses, encrypts data
   - Generates shareable portal link
   - Uploads encrypted manifest to Hub

### Access Portal
1. Click portal link or scan QR code
2. Automatic peer discovery and connection
3. Swarm download from multiple sources
4. Local verification and decryption

### Enterprise Deployment
1. Deploy Hub server on-premise
2. Enroll edge devices via QR code
3. Configure lifecycle policies
4. Monitor via web dashboard
