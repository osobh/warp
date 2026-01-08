# Portal System

> Zero-Knowledge Distributed Storage

---

## 1. Overview

Portal is a zero-knowledge distributed storage system where:

- **Users control their encryption keys** - infrastructure cannot access plaintext
- **Access is ephemeral** - time-limited portals with policy-driven lifecycle
- **Data flows through P2P mesh** - bypassing central infrastructure when possible
- **Intelligence is distributed** - each edge makes local optimization decisions

The Hub coordinates but cannot read. The network routes but cannot inspect.

---

## 2. Core Concepts

### 2.1 Zero-Knowledge Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       ZERO-KNOWLEDGE GUARANTEE                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  WHAT THE HUB SEES:                                                         │
│  • Encrypted blobs (chunks)                                                 │
│  • Encrypted metadata (filenames, structure)                                │
│  • User public keys (for coordination)                                      │
│  • Transfer patterns (which edges communicate)                              │
│                                                                             │
│  WHAT THE HUB CANNOT SEE:                                                   │
│  • File contents                                                            │
│  • Filenames or directory structure                                         │
│  • Relationships between files                                              │
│  • User's encryption keys                                                   │
│                                                                             │
│  EVEN IF HUB IS COMPROMISED:                                                │
│  • Attacker gets encrypted blobs only                                       │
│  • No way to derive decryption keys                                         │
│  • Historical data remains protected                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Portals

A **Portal** is an ephemeral access tunnel to shared content:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            PORTAL CONCEPT                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Traditional Sharing:                                                       │
│  • Create link → Link exists forever                                        │
│  • Revoke access → Must manually disable                                    │
│  • Access control → Binary (has link or doesn't)                            │
│                                                                             │
│  Portal Sharing:                                                            │
│  • Create portal → Portal has lifecycle policy                              │
│  • Access expires → Automatically per policy                                │
│  • Access control → Granular (time, count, schedule, presence)              │
│                                                                             │
│  Example Portal:                                                            │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ Portal: "Q3 Financial Report"                                       │   │
│  │ Content: /reports/q3-2024/                                          │   │
│  │ Created: 2024-10-01 09:00                                           │   │
│  │ Policy:                                                              │   │
│  │   • Active: 2024-10-01 to 2024-10-15                                │   │
│  │   • Hours: 09:00-18:00 weekdays                                     │   │
│  │   • Max downloads: 50                                                │   │
│  │   • Requires: Authenticated user                                    │   │
│  │ Access:                                                              │   │
│  │   • finance-team@company.com (full)                                 │   │
│  │   • auditor@external.com (read-only)                                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.3 Edges

An **Edge** is a device participating in the Portal network:

- Personal devices (laptop, phone, desktop)
- Servers (home NAS, cloud VM)
- The Hub itself (special edge)

Each edge has:
- Unique identity (Ed25519 keypair)
- WireGuard configuration
- Local chunk storage
- Metrics and health status

---

## 3. Key Hierarchy

### 3.1 Derivation Path

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          KEY HIERARCHY                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  RECOVERY PHRASE (BIP-39)                                                   │
│  "abandon abandon abandon ... about"                                        │
│  │                                                                          │
│  │ PBKDF2-HMAC-SHA512                                                       │
│  ▼                                                                          │
│  MASTER SEED (512 bits)                                                     │
│  │                                                                          │
│  ├──► HKDF("portal/encryption") ──► MASTER ENCRYPTION KEY                   │
│  │                                   │                                      │
│  │                                   ├──► Manifest encryption               │
│  │                                   └──► Chunk key derivation base         │
│  │                                                                          │
│  ├──► HKDF("portal/signing") ──► MASTER SIGNING KEY (Ed25519)               │
│  │                               │                                          │
│  │                               ├──► Identity attestation                  │
│  │                               └──► Portal creation signatures            │
│  │                                                                          │
│  ├──► HKDF("portal/auth") ──► AUTHENTICATION KEY                            │
│  │                            │                                             │
│  │                            └──► Hub authentication tokens                │
│  │                                                                          │
│  └──► HKDF("portal/device/N") ──► DEVICE SUBKEY                             │
│                                    │                                        │
│                                    └──► Per-device operations               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Convergent Encryption

For deduplication without compromising privacy:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       CONVERGENT ENCRYPTION                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Traditional Encryption:                                                    │
│  • Key is random                                                            │
│  • Same plaintext → different ciphertext each time                          │
│  • Cannot deduplicate (ciphertexts differ)                                  │
│                                                                             │
│  Convergent Encryption:                                                     │
│  • Key derived from content: K = HKDF(master_key, BLAKE3(content))          │
│  • Same plaintext → same ciphertext                                         │
│  • Can deduplicate (identical ciphertexts)                                  │
│  • Still secure (content-derived key unknown to attacker)                   │
│                                                                             │
│  Process:                                                                   │
│  1. chunk_hash = BLAKE3(plaintext_chunk)                                    │
│  2. chunk_key = HKDF(master_key, chunk_hash)                                │
│  3. nonce = chunk_hash[0:12]                                                │
│  4. ciphertext = ChaCha20-Poly1305(chunk_key, nonce, plaintext)             │
│                                                                             │
│  Result:                                                                    │
│  • Same content from same user → identical ciphertext                       │
│  • Different users with same master_key → identical ciphertext              │
│  • Hub can deduplicate without seeing content                               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Portal Lifecycle

### 4.1 States

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        PORTAL LIFECYCLE STATES                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                      ┌──────────┐                                           │
│                      │ CREATED  │                                           │
│                      └────┬─────┘                                           │
│                           │ (schedule reached / manual activation)          │
│                           ▼                                                 │
│                      ┌──────────┐                                           │
│            ┌────────►│  ACTIVE  │◄────────┐                                 │
│            │         └────┬─────┘         │                                 │
│            │              │               │                                 │
│  (schedule │              │               │ (schedule                       │
│   resumes) │              │               │  resumes)                       │
│            │              ▼               │                                 │
│            │         ┌──────────┐         │                                 │
│            └─────────│  PAUSED  │─────────┘                                 │
│                      └────┬─────┘                                           │
│                           │ (expiry / limit reached / manual)               │
│                           ▼                                                 │
│                      ┌──────────┐                                           │
│                      │ EXPIRED  │                                           │
│                      └────┬─────┘                                           │
│                           │ (retention period elapsed)                      │
│                           ▼                                                 │
│                      ┌──────────┐                                           │
│                      │ ARCHIVED │                                           │
│                      └──────────┘                                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Lifecycle Policies

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        LIFECYCLE POLICY TYPES                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  TIME-BASED:                                                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ • Absolute: "Active from Oct 1 to Oct 15"                           │   │
│  │ • Relative: "Active for 7 days after creation"                      │   │
│  │ • Schedule: "Active weekdays 9am-6pm"                               │   │
│  │ • Recurring: "First week of each month"                             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  USAGE-BASED:                                                               │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ • Download count: "Max 100 downloads total"                         │   │
│  │ • Bandwidth: "Max 10 GB transferred"                                │   │
│  │ • Access count: "Max 5 unique accessors"                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  PRESENCE-BASED:                                                            │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ • Owner online: "Active only when owner's edge is online"           │   │
│  │ • Location: "Active when accessor is in office network"             │   │
│  │ • Device: "Active only from registered devices"                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  COMPOSITE:                                                                 │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ • AND: "Weekdays 9-6 AND owner online AND under 100 downloads"      │   │
│  │ • OR: "Owner online OR explicit grant"                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 5. Access Control

### 5.1 Access Levels

| Level | Description | Typical Use |
|-------|-------------|-------------|
| **Owner** | Full control, can modify portal | Creator |
| **Admin** | Can grant/revoke access | Team lead |
| **Write** | Can modify content | Collaborator |
| **Read** | Can download content | Viewer |
| **List** | Can see metadata only | Auditor |

### 5.2 Access Grant Types

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          ACCESS GRANT TYPES                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  IDENTITY-BASED:                                                            │
│  • By public key: "Grant read to Ed25519:abc123..."                         │
│  • By email (resolved to key): "Grant read to alice@example.com"            │
│                                                                             │
│  LINK-BASED:                                                                │
│  • Secret link: "Anyone with link can read"                                 │
│  • Link + password: "Link plus password required"                           │
│  • One-time link: "Link valid for single use"                               │
│                                                                             │
│  GROUP-BASED:                                                               │
│  • Team: "Grant read to engineering-team"                                   │
│  • Domain: "Grant read to *@company.com"                                    │
│                                                                             │
│  CONDITIONAL:                                                               │
│  • Time-limited: "Read access until Oct 15"                                 │
│  • Count-limited: "Read access for 10 downloads"                            │
│  • Network-limited: "Read only from 10.0.0.0/8"                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.3 Re-Entry Flow

When a portal expires but someone needs access:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RE-ENTRY FLOW                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. GUEST REQUESTS ACCESS                                                   │
│     • Guest visits expired portal link                                      │
│     • System shows "Portal expired - Request access?"                       │
│     • Guest submits request with justification                              │
│                                                                             │
│  2. OWNER NOTIFIED                                                          │
│     • Owner receives notification (push, email)                             │
│     • Shows: who's requesting, why, what portal                             │
│                                                                             │
│  3. OWNER DECIDES                                                           │
│     • Approve: Grant temporary access (new mini-portal)                     │
│     • Deny: Request rejected, guest notified                                │
│     • Extend: Reactivate original portal with new policy                    │
│                                                                             │
│  4. ACCESS GRANTED                                                          │
│     • If approved, guest receives time-limited access                       │
│     • Original portal policy unchanged                                      │
│     • Audit log records grant                                               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Hub Architecture

### 6.1 Hub Responsibilities

The Hub is a coordinator, not a custodian:

| Responsibility | What Hub Does | What Hub Cannot Do |
|----------------|---------------|-------------------|
| Peer Directory | Stores edge public keys | Access edge private keys |
| Chunk Storage | Stores encrypted blobs | Decrypt blobs |
| Metadata Sync | Relay encrypted metadata | Read metadata |
| Portal Registry | Track portal existence | Read portal contents |
| Access Coordination | Enforce policies | Bypass encryption |
| Relay Fallback | Route when P2P fails | Inspect traffic |

### 6.2 Hub Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           HUB ARCHITECTURE                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ API GATEWAY                                                         │   │
│  │ • REST/gRPC endpoints                                               │   │
│  │ • Authentication (public key verification)                          │   │
│  │ • Rate limiting                                                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│          ┌───────────────────┼───────────────────┐                          │
│          │                   │                   │                          │
│          ▼                   ▼                   ▼                          │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐                    │
│  │ Edge Service │   │Portal Service│   │ Chunk Store  │                    │
│  │              │   │              │   │              │                    │
│  │ • Registry   │   │ • Lifecycle  │   │ • Blob store │                    │
│  │ • Health     │   │ • Policies   │   │ • Dedup      │                    │
│  │ • WireGuard  │   │ • Access     │   │ • Replication│                    │
│  └──────────────┘   └──────────────┘   └──────────────┘                    │
│          │                   │                   │                          │
│          └───────────────────┼───────────────────┘                          │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ DATABASE                                                            │   │
│  │ • Edge metadata (public keys, virtual IPs)                          │   │
│  │ • Portal metadata (encrypted)                                       │   │
│  │ • Chunk index (CIDs, locations)                                     │   │
│  │ • Audit logs                                                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.3 Hub Deployment Options

| Option | Description | Use Case |
|--------|-------------|----------|
| **Hosted** | Anthropic/third-party runs Hub | Individual users |
| **Self-Hosted** | User runs own Hub | Privacy-conscious users |
| **Enterprise** | Company runs Hub cluster | Organizations |
| **Federated** | Multiple Hubs interconnected | Large deployments |

---

## 7. Edge Architecture

### 7.1 Edge Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           EDGE ARCHITECTURE                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ USER INTERFACE                                                      │   │
│  │ • CLI / GUI / Mobile App                                            │   │
│  │ • File browser integration                                          │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ PORTAL DAEMON                                                       │   │
│  │                                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                  │   │
│  │  │ Key Manager │  │ Sync Engine │  │  Scheduler  │                  │   │
│  │  │             │  │             │  │   (GPU)     │                  │   │
│  │  │ • Master key│  │ • Hub sync  │  │ • Routing   │                  │   │
│  │  │ • Derivation│  │ • P2P sync  │  │ • Failover  │                  │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘                  │   │
│  │                                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                  │   │
│  │  │ Warp Engine │  │ Chunk Cache │  │  WireGuard  │                  │   │
│  │  │             │  │             │  │  Manager    │                  │   │
│  │  │ • Compress  │  │ • Hot cache │  │ • Peers     │                  │   │
│  │  │ • Encrypt   │  │ • LRU evict │  │ • Routing   │                  │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘                  │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ LOCAL STORAGE                                                       │   │
│  │ • Chunk store (encrypted)                                           │   │
│  │ • Metadata cache                                                    │   │
│  │ • State database                                                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 Edge Enrollment

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         EDGE ENROLLMENT FLOW                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. GENERATE IDENTITY                                                       │
│     • Edge generates Ed25519 keypair                                        │
│     • Same keypair used for Portal identity + WireGuard                     │
│                                                                             │
│  2. CONNECT TO HUB                                                          │
│     • Edge connects to Hub (known endpoint)                                 │
│     • Presents public key + enrollment request                              │
│                                                                             │
│  3. USER AUTHENTICATION                                                     │
│     • User proves ownership of master key                                   │
│     • Signs enrollment request                                              │
│                                                                             │
│  4. HUB REGISTRATION                                                        │
│     • Hub assigns virtual IP (10.portal.0.X)                                │
│     • Records edge in database                                              │
│     • Returns peer list (other edges for this user)                         │
│                                                                             │
│  5. WIREGUARD SETUP                                                         │
│     • Edge configures WireGuard interface                                   │
│     • Adds Hub and peer edges as WireGuard peers                            │
│     • Mesh is established                                                   │
│                                                                             │
│  6. SYNC                                                                    │
│     • Edge syncs metadata from Hub                                          │
│     • Downloads chunk index                                                 │
│     • Ready for operation                                                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Data Flow

### 8.1 Upload Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            UPLOAD FLOW                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  USER: "Upload /documents/report.pdf to portal 'Q3 Report'"                 │
│                                                                             │
│  1. CHUNK                                                                   │
│     • Warp chunks file (content-defined)                                    │
│     • Result: [Chunk₀, Chunk₁, ..., Chunkₙ]                                 │
│                                                                             │
│  2. HASH & ENCRYPT                                                          │
│     • For each chunk:                                                       │
│       - CID = BLAKE3(content)                                               │
│       - Key = HKDF(master_key, CID)                                         │
│       - Encrypted = ChaCha20(Key, chunk)                                    │
│                                                                             │
│  3. BUILD MANIFEST                                                          │
│     • Create file node: {path: "report.pdf", chunks: [CID₀, CID₁, ...]}     │
│     • Encrypt manifest with master key                                      │
│                                                                             │
│  4. DEDUP CHECK                                                             │
│     • Query Hub/local cache for existing CIDs                               │
│     • Identify which chunks need upload                                     │
│                                                                             │
│  5. UPLOAD CHUNKS                                                           │
│     • Send new encrypted chunks to Hub                                      │
│     • May also push to other edges (P2P distribution)                       │
│                                                                             │
│  6. UPDATE PORTAL                                                           │
│     • Add file to portal's content DAG                                      │
│     • Update portal's integrity commitment                                  │
│     • Notify other edges of change                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Download Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           DOWNLOAD FLOW                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  USER: "Download report.pdf from portal 'Q3 Report'"                        │
│                                                                             │
│  1. ACCESS CHECK                                                            │
│     • Verify user has access to portal                                      │
│     • Check portal is active (lifecycle policy)                             │
│                                                                             │
│  2. GET MANIFEST                                                            │
│     • Fetch encrypted manifest from Hub                                     │
│     • Decrypt with master key                                               │
│     • Extract chunk list for requested file                                 │
│                                                                             │
│  3. SOURCE SELECTION                                                        │
│     • GPU scheduler determines optimal sources                              │
│     • Options: Hub, other edges (P2P), local cache                          │
│     • Considers: bandwidth, latency, cost, availability                     │
│                                                                             │
│  4. PARALLEL FETCH                                                          │
│     • Request chunks from multiple sources (swarm)                          │
│     • Verify each chunk: BLAKE3(decrypted) == CID                           │
│     • Failover to backup sources if needed                                  │
│                                                                             │
│  5. REASSEMBLE                                                              │
│     • Decrypt chunks                                                        │
│     • Concatenate in order                                                  │
│     • Write to destination                                                  │
│                                                                             │
│  6. VERIFY                                                                  │
│     • Verify Merkle root matches expected                                   │
│     • File is complete and authentic                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Sharing Scenarios

### 9.1 Share with Registered User

```
Alice wants to share "Project Files" with Bob (both have Portal accounts)

1. Alice creates portal with Bob's public key as reader
2. Bob's edge receives notification
3. Bob's edge fetches encrypted manifest
4. Bob can decrypt (his key in access list)
5. Bob downloads files via P2P from Alice or Hub
```

### 9.2 Share via Secret Link

```
Alice wants to share "Vacation Photos" with family (no Portal accounts)

1. Alice creates portal with link-based access
2. Portal generates secret URL: portal://abc123/photos
3. Alice sends link via email/message
4. Recipient clicks link, downloads via browser
5. No account needed, time-limited access
```

### 9.3 Collaborative Workspace

```
Team needs shared workspace for project

1. Lead creates portal with team as admins
2. Team members can add/modify content
3. Changes sync automatically via P2P
4. Conflict resolution via last-write-wins or manual
5. Portal expires when project completes
```

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Mitigation |
|--------|------------|
| Hub compromise | Zero-knowledge - Hub has only encrypted data |
| Network eavesdropping | WireGuard encryption |
| Malicious peer | Content verification via Merkle DAG |
| Key theft | Recovery phrase, device subkeys, revocation |
| Metadata leakage | Encrypted filenames and structure |
| Traffic analysis | WireGuard hides traffic patterns |

### 10.2 Key Security

| Key Type | Storage | Protection |
|----------|---------|------------|
| Recovery phrase | Written, offline | Physical security |
| Master seed | Never stored | Derived when needed |
| Master encryption key | Encrypted keyring | User password |
| Device subkeys | Secure enclave/TPM | Hardware protection |
| Session keys | Memory only | Not persisted |

---

## 11. Summary

Portal provides:

1. **Zero-Knowledge Storage**: Hub cannot access plaintext
2. **Ephemeral Access**: Policy-driven portal lifecycle
3. **Flexible Sharing**: Identity, link, or group-based access
4. **P2P Distribution**: Direct edge-to-edge transfers
5. **Convergent Encryption**: Deduplication without compromise
6. **Re-Entry Flow**: Graceful access request after expiry

The result is a storage system that respects user privacy while enabling powerful sharing and collaboration capabilities.
