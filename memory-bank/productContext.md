# Product Context

## Problem Statement
Moving large datasets (100GB to multi-TB) between servers is painfully slow with existing tools:

- **rsync**: Sequential processing, no GPU, limited parallelism, restarts on failure
- **scp/sftp**: No compression intelligence, no deduplication, no resume
- **tar + transfer**: Two-step process, no incremental verification, full restart on corruption
- **Commercial solutions**: Expensive, proprietary, vendor lock-in

Data scientists, ML engineers, and system administrators waste hours waiting for transfers, often having to restart from scratch when transfers fail at 90% completion.

## Target Users
1. **ML/AI Engineers**: Moving training datasets and model checkpoints between GPU clusters
2. **Data Engineers**: Migrating data warehouses, backing up large databases
3. **System Administrators**: Server migrations, disaster recovery, cross-datacenter replication
4. **Research Scientists**: Sharing large experimental datasets between institutions
5. **DevOps Teams**: Deploying large application artifacts and container images

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
| Feature | Description |
|---------|-------------|
| **Blazing Fast** | GPU acceleration provides 10x+ speedup over CPU-only tools |
| **Smart Compression** | Automatically selects optimal algorithm per chunk based on entropy |
| **Resumable** | Interrupted transfers continue from last verified chunk |
| **Verifiable** | Merkle proofs ensure data integrity at every step |
| **Deduplicated** | Only transfers unique chunks, even across different files |
| **Secure** | Optional ChaCha20 encryption with Ed25519 signatures |
| **Simple CLI** | Familiar rsync-like syntax (`warp send src dest`) |

## User Experience Goals
- Zero-configuration defaults that "just work" optimally
- Progress bar with accurate ETA and throughput display
- Clear error messages with actionable remediation
- Minimal learning curve for rsync/scp users

## Business Value
- Reduces transfer time from hours to minutes for large datasets
- Eliminates failed transfer restarts with resume capability
- Provides cryptographic audit trail for compliance
- Open source with permissive licensing (MIT/Apache-2.0)
