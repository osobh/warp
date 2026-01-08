# Portal Extensions: Access, Federation & Monetization

> Discussion Document - Advanced Portal Features

---

## 1. Executive Summary

Portal is designed as a **private-first, monetization-optional** platform. Every feature works in fully private mode, with the ability to progressively enable paid services when desired.

**Core Principle**: Deploy everything for yourself first. When ready, flip a switch to monetize your infrastructure.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     PROGRESSIVE EXPOSURE MODEL                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   PRIVATE                      HYBRID                        PUBLIC         │
│   (Self-only)                  (Selective)                   (Open)         │
│                                                                             │
│   ┌─────────┐                  ┌─────────┐                  ┌─────────┐    │
│   │ ██████  │      ──────►     │ ██░░██  │      ──────►     │ ░░░░░░  │    │
│   │ ██████  │    Enable        │ ██░░██  │    Enable        │ ░░░░░░  │    │
│   │ ██████  │    wormholes     │ ██░░██  │    public        │ ░░░░░░  │    │
│   └─────────┘                  └─────────┘    listing       └─────────┘    │
│                                                                             │
│   • All features work         • Partner access             • Public hub     │
│   • Zero external access      • Selective sharing          • Toll operator  │
│   • No monetization           • Start monetizing           • Full economy   │
│                                                                             │
│   Your choice. Your timeline. Your rules.                                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

This document covers:
1. **Access Tiers**: Free, Paid, and Proof-gated portal access
2. **Wormhole Services**: Routing, caching, and storage across networks
3. **Storage Tiers**: Edge cache, mid-term, and long-term placement
4. **Capability Monetization**: Routes, storage, compute, devices
5. **QR Integration**: Physical-digital bridge

---

## 2. Private-First Architecture

### 2.1 Everything Works Privately

Every Portal feature is fully functional in private mode:

| Feature | Private Mode | What Changes When Public |
|---------|--------------|--------------------------|
| File storage | ✓ Full functionality | Others can pay to cache on you |
| Portals | ✓ Share with your devices | Accept external payments |
| Wormholes | ✓ Between YOUR hubs | Route through partner hubs |
| GPU Scheduler | ✓ Optimizes your transfers | Factors in toll costs |
| Edge Cache | ✓ Cache on your edges | Cache on partner edges |
| Long-term Storage | ✓ Store on your nodes | Store on partner nodes |

### 2.2 Deployment Progression

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      DEPLOYMENT JOURNEY                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  STAGE 1: Personal (Day 1)                                                  │
│  ─────────────────────────                                                  │
│  • Install Portal on your devices                                           │
│  • All your devices form a private mesh                                     │
│  • Sync, backup, stream between YOUR devices                                │
│  • Zero external dependencies                                               │
│                                                                             │
│  STAGE 2: Team (When Ready)                                                 │
│  ──────────────────────────                                                 │
│  • Invite team members to your hub                                          │
│  • Create portals for collaboration                                         │
│  • Still fully private to your organization                                 │
│                                                                             │
│  STAGE 3: Partners (When Needed)                                            │
│  ───────────────────────────────                                            │
│  • Enable wormholes to specific partner hubs                                │
│  • Negotiate terms (free, paid, reciprocal)                                 │
│  • Access partner infrastructure for routing/storage                        │
│  • Partners can access yours (if you allow)                                 │
│                                                                             │
│  STAGE 4: Public (When Desired)                                             │
│  ──────────────────────────────                                             │
│  • List in Public Hub Directory                                             │
│  • Set pricing for routing, storage, compute                                │
│  • Earn from infrastructure you already have                                │
│  • Become part of the global fabric                                         │
│                                                                             │
│  Each stage is optional. Stay at any level forever.                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.3 Configuration Example

```yaml
# hub_config.yaml

hub:
  id: "hub_7f3a9c..."
  name: "My Hub"
  
  # Start private, enable features when ready
  mode: private | hybrid | public
  
  # Wormhole settings (only active in hybrid/public mode)
  wormholes:
    enabled: false  # Flip to true when ready
    
    # What you OFFER to partners
    offer:
      routing: true
      edge_cache: true
      mid_term_storage: true
      long_term_storage: false  # Not ready to offer this yet
      compute: false            # Future capability
      
    # What you ACCEPT from partners
    accept:
      inbound_routing: true
      cache_requests: true
      storage_requests: true
      
    # Pricing (ignored in private mode)
    pricing:
      routing_per_gb: 0.01
      edge_cache_per_gb_day: 0.05
      mid_term_per_gb_month: 0.02
      long_term_per_gb_month: 0.005
      
  # Public directory listing (only in public mode)
  public_listing:
    enabled: false
    display_name: "FastPath Tokyo"
    description: "High-speed node in Tokyo datacenter"
    contact: "ops@example.com"
```

---

## 3. Access Tiers

### 3.1 Overview

Portal access supports three gate types, all functional in private mode:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ACCESS TIER MODEL                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         PORTAL GATE                                 │   │
│  │                                                                      │   │
│  │   Incoming Request ──►  ┌──────────────┐  ──► Access Granted        │   │
│  │                         │  Gate Check  │                            │   │
│  │                         └──────────────┘                            │   │
│  │                               │                                      │   │
│  │              ┌────────────────┼────────────────┐                    │   │
│  │              │                │                │                    │   │
│  │              ▼                ▼                ▼                    │   │
│  │        ┌──────────┐    ┌──────────┐    ┌──────────┐                 │   │
│  │        │   FREE   │    │   PAID   │    │  PROOF   │                 │   │
│  │        │          │    │          │    │          │                 │   │
│  │        │ Time-    │    │ Payment  │    │ Upload   │                 │   │
│  │        │ limited  │    │ required │    │ required │                 │   │
│  │        └──────────┘    └──────────┘    └──────────┘                 │   │
│  │                                                                      │   │
│  │        Can be combined: Paid + Proof, Member discounts, etc.        │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  PRIVATE MODE: Gates still work - just between your own devices/users      │
│  PUBLIC MODE:  Gates can accept external payments and proofs               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Tier 1: Free Access

Standard time-limited ephemeral portal:

| Property | Description |
|----------|-------------|
| URL | Unique, unguessable link |
| Duration | Owner-defined expiration |
| Access | Anyone with link |
| Cost | Free |

```
https://portal.example/p/7f3a9c2b...
```

**Use Cases**:
- Quick file sharing
- Temporary collaboration
- Public releases

---

### 3.3 Tier 2: Paid Access

Monetized portal requiring payment before content access:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         PAID PORTAL MODEL                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  OWNER CONFIGURES:                                                          │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ Amount                                                              │   │
│  │ ├── Fixed price: $5.00                                              │   │
│  │ ├── Per-GB: $0.10/GB                                                │   │
│  │ └── Dynamic: Market-based pricing                                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ Cadence                                                             │   │
│  │ ├── One-time: Pay once, access forever (within duration)            │   │
│  │ ├── Per-download: Pay each time                                     │   │
│  │ ├── Subscription: $X/month for access                               │   │
│  │ └── Streaming: Pay-per-minute/chunk (real-time content)             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ Duration                                                            │   │
│  │ ├── Time-bound: Access for 24 hours after payment                   │   │
│  │ ├── Permanent: Access while portal exists                           │   │
│  │ └── Consumption: Access until X downloads/GB used                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  PAYMENT RAILS:                                                             │
│  ├── Crypto: Lightning, Stablecoins                                        │
│  ├── Traditional: Stripe, PayPal                                           │
│  └── Portal Credits: Internal currency                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Configuration**:
```yaml
paid_portal:
  amount:
    type: fixed | per_gb | dynamic
    value: 5.00
    currency: USD | BTC | PORTAL_CREDITS
  
  cadence: one_time | per_download | subscription | streaming
  
  duration:
    type: time_bound | permanent | consumption
    value: 24h | null | 10GB
  
  payment_rails:
    - lightning
    - stripe
```

---

### 3.4 Tier 3: Proof-Gated Access

Access unlocked by uploading required content:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       PROOF-GATED PORTAL MODEL                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  PROOF TYPES:                                                               │
│                                                                             │
│  ┌─────────────────┐                                                       │
│  │ EXACT CONTENT   │  "Upload file with hash X"                            │
│  │ CID: 7f3a9c...  │  Verifiable by CID match                              │
│  └─────────────────┘                                                       │
│                                                                             │
│  ┌─────────────────┐                                                       │
│  │ SCHEMA MATCH    │  "Upload any file matching schema Y"                  │
│  │ Schema: {...}   │  JSON schema, file type, size constraints             │
│  └─────────────────┘                                                       │
│                                                                             │
│  ┌─────────────────┐                                                       │
│  │ ZK PROOF        │  "Prove you possess X without revealing X"            │
│  │ Verifier: ...   │  ZK-SNARK/STARK verification                          │
│  └─────────────────┘                                                       │
│                                                                             │
│  ┌─────────────────┐                                                       │
│  │ SIGNED PROOF    │  "Upload document signed by authority Z"              │
│  │ Signer: 0x...   │  Signature verification                               │
│  └─────────────────┘                                                       │
│                                                                             │
│  PROOF COUNT:                                                               │
│  ├── Single: 1 proof required                                              │
│  ├── Multiple: N proofs required (1 of each type)                          │
│  └── Any-of: M of N possible proofs                                        │
│                                                                             │
│  PROOF DESTINATION:                                                         │
│  ├── Owner Only: Proof goes directly to portal owner                       │
│  ├── Shared Pool: Proof added to collective resource                       │
│  └── Burn: Proof verified but not stored (ZK scenarios)                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Use Cases**:
- Research consortiums (contribute to access)
- KYC/compliance verification
- Collaborative projects (prove contribution)
- Gated communities (prove membership)

---

### 3.5 Composite Gates

Gates can be combined:

```yaml
# Example: Members pay $5, non-members pay $50
gates:
  - type: conditional
    if:
      type: zk_proof
      verifier: "membership"
    then:
      type: paid
      amount: 5.00
    else:
      type: paid
      amount: 50.00

# Example: Contribute research OR pay $100
gates:
  - type: proof
    requirement: research_paper
  - type: paid
    amount: 100.00
logic: any_of(1)
```

---

## 4. Wormhole Services

### 4.1 What Wormholes Are

Wormholes are **trust tunnels** between hubs that enable multiple services:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      WORMHOLE SERVICE SPECTRUM                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  A wormhole between Hub A and Hub B enables:                                │
│                                                                             │
│  1. ROUTING         - Faster paths through partner infrastructure           │
│  2. CONTENT ACCESS  - Access partner's shared portals                       │
│  3. EDGE CACHE      - Temporarily place your data on partner edges          │
│  4. MID-TERM STORAGE- Store your data on partner for days/weeks             │
│  5. LONG-TERM STORAGE- Archive your data on partner infrastructure          │
│  6. FUTURE: COMPUTE - Use partner's CPU/GPU resources                       │
│  7. FUTURE: DEVICES - Access partner's connected devices                    │
│                                                                             │
│  Each service can be enabled/disabled and priced independently              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Wormhole Use Case 1: Routing

Faster paths to YOUR infrastructure through partner networks:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         ROUTING WORMHOLE                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  WITHOUT WORMHOLE:                                                          │
│                                                                             │
│    You (NYC) ─────── Public Internet ─────── Your Server (Tokyo)            │
│                      (congested, 800ms)                                     │
│                                                                             │
│  WITH ROUTING WORMHOLE:                                                     │
│                                                                             │
│    You (NYC) ═══► Partner (SF) ═══► Partner (HK) ═══► Your Server (Tokyo)   │
│                   (optimized path, 200ms)                                   │
│                                                                             │
│  • Your data stays on YOUR server                                           │
│  • Partners provide TRANSIT only                                            │
│  • Pay per-GB transferred                                                   │
│  • No storage on partner infrastructure                                     │
│                                                                             │
│  PRICING EXAMPLE:                                                           │
│  ├── Regular (no wormhole): Free, best-effort                               │
│  ├── Optimized (local wormhole): $0.01/GB                                   │
│  └── Premium (multi-hop wormhole): $0.05/GB                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.3 Wormhole Use Case 2: Content Access

Access partner's shared content through wormhole:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       CONTENT ACCESS WORMHOLE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Partner has content they want to share with you:                           │
│                                                                             │
│    Your Hub ═══► Wormhole ═══► Partner Hub                                  │
│                                    │                                        │
│                                    ▼                                        │
│                              ┌──────────┐                                   │
│                              │ Partner's│                                   │
│                              │  Portal  │                                   │
│                              │ (shared) │                                   │
│                              └──────────┘                                   │
│                                                                             │
│  • Partner controls access (free/paid/proof gated)                          │
│  • You access their data through the wormhole                               │
│  • Use cases: Data marketplace, consortium, partnership                     │
│                                                                             │
│  RECIPROCAL: They can also access YOUR shared portals                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.4 Trust Model

Wormholes are established hub-to-hub:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      WORMHOLE ESTABLISHMENT                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  STEP 1: Discovery                                                          │
│  • Public Hub Directory                                                     │
│  • Direct introduction                                                      │
│  • QR code exchange                                                         │
│                                                                             │
│  STEP 2: Negotiation                                                        │
│  Hub A ──────────────────────────────────────────────────────► Hub B        │
│         { "request": "wormhole",                                            │
│           "direction": "bidirectional",                                     │
│           "services": ["routing", "edge_cache"],                            │
│           "terms": { pricing... } }                                         │
│                                                                             │
│  Hub A ◄────────────────────────────────────────────────────── Hub B        │
│         { "response": "accept",                                             │
│           "counter_terms": { ... },                                         │
│           "wormhole_id": "wh_7f3a9c..." }                                   │
│                                                                             │
│  STEP 3: Key Exchange                                                       │
│  • Exchange WireGuard keys                                                  │
│  • Establish encrypted tunnel                                               │
│                                                                             │
│  STEP 4: Active Wormhole                                                    │
│  • Routing tables updated                                                   │
│  • Services available                                                       │
│  • Usage tracked for billing                                                │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.5 Reciprocity Options

```
BIDIRECTIONAL (Common):
  Hub A ◄════════════════════════════► Hub B
  Both can use each other's services

ONE-WAY GRANT:
  Hub A ════════════════════════════► Hub B
  Only A can use B's services (B pays A, or free grant)

ASYMMETRIC:
  Hub A ◄════════════════════════════► Hub B
  Different terms in each direction
  (A pays $10/mo to B, B pays $50/mo to A)
```

---

## 5. Storage Tiers

### 5.1 Storage Service Spectrum

When wormholes are active, you can place YOUR data on PARTNER infrastructure:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      STORAGE TIER SPECTRUM                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  EPHEMERAL ◄───────────────────────────────────────────────► PERMANENT      │
│                                                                             │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐       │
│  │  EDGE CACHE  │ │   MID-TERM   │ │  LONG-TERM   │ │   ARCHIVE    │       │
│  │              │ │   STORAGE    │ │   STORAGE    │ │              │       │
│  ├──────────────┤ ├──────────────┤ ├──────────────┤ ├──────────────┤       │
│  │              │ │              │ │              │ │              │       │
│  │  ≤24 hours   │ │  Days-Weeks  │ │   Months     │ │    Years     │       │
│  │  Auto-evict  │ │  Scheduled   │ │  Contracted  │ │  Guaranteed  │       │
│  │  Hot data    │ │  Warm data   │ │  Cold data   │ │  Archival    │       │
│  │              │ │              │ │              │ │              │       │
│  │ $0.05/GB/day │ │ $0.02/GB/mo  │ │$0.005/GB/mo  │ │$0.002/GB/mo  │       │
│  │              │ │              │ │              │ │              │       │
│  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘       │
│                                                                             │
│  ALL TIERS:                                                                 │
│  • Your data, encrypted with YOUR keys                                      │
│  • Partner cannot read content (zero-knowledge)                             │
│  • Sandboxed from partner's data                                            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Edge Cache (≤24 Hours)

Temporary hot-data placement for regional performance:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          EDGE CACHE                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  YOUR HUB (Tokyo)              PARTNER HUB (Frankfurt)                      │
│  ┌─────────────────┐           ┌─────────────────┐                          │
│  │                 │  Cache    │                 │                          │
│  │  ┌───────────┐  │ ────────► │  ┌───────────┐  │                          │
│  │  │ Your File │  │  24hrs    │  │  Cached   │  │                          │
│  │  │  500 GB   │  │  max      │  │   Copy    │  │◄──── European users      │
│  │  └───────────┘  │           │  └───────────┘  │      fetch locally!      │
│  │                 │           │                 │                          │
│  └─────────────────┘           └─────────────────┘                          │
│                                                                             │
│  PROPERTIES:                                                                │
│  • Maximum 24 hours (hard limit, auto-evicted)                              │
│  • Encrypted with your keys (partner can't read)                            │
│  • Sandboxed from partner's storage                                         │
│  • Pay per GB per day                                                       │
│                                                                             │
│  USE CASES:                                                                 │
│  • CDN-like distribution for events                                         │
│  • Spike traffic handling                                                   │
│  • Video streaming to distant regions                                       │
│  • Software release distribution                                            │
│                                                                             │
│  PRICING EXAMPLE: $0.05/GB/day                                              │
│  500 GB cached for 24 hours = $25                                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.3 Mid-Term Storage (Days to Weeks)

Extended placement for projects and campaigns:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       MID-TERM STORAGE                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  PROPERTIES:                                                                │
│  • Days to weeks duration                                                   │
│  • Scheduled start/end dates                                                │
│  • Encrypted with your keys                                                 │
│  • Optional redundancy                                                      │
│                                                                             │
│  USE CASES:                                                                 │
│  • Marketing campaign assets                                                │
│  • Project collaboration (2-week sprint)                                    │
│  • Seasonal content                                                         │
│  • Conference/event materials                                               │
│                                                                             │
│  PRICING EXAMPLE: $0.02/GB/month (prorated)                                 │
│  1 TB stored for 2 weeks = $10                                              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.4 Long-Term Storage (Months+)

Geo-distributed backup and cold storage:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      LONG-TERM STORAGE                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Your Hub ────► Partner Hub A (US-West)                                     │
│           ────► Partner Hub B (EU-Central)                                  │
│           ────► Partner Hub C (APAC)                                        │
│                                                                             │
│  PROPERTIES:                                                                │
│  • Months to years duration                                                 │
│  • Contract-based commitment                                                │
│  • Durability guarantees (99.999%)                                          │
│  • Recovery SLAs                                                            │
│  • Geographic distribution                                                  │
│                                                                             │
│  USE CASES:                                                                 │
│  • Disaster recovery                                                        │
│  • Compliance copies                                                        │
│  • Cold archive                                                             │
│  • Geographic redundancy                                                    │
│                                                                             │
│  PRICING EXAMPLE: $0.005/GB/month                                           │
│  10 TB stored for 1 year = $600                                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.5 Storage Tier Comparison

| Aspect | Edge Cache | Mid-Term | Long-Term |
|--------|------------|----------|-----------|
| **Duration** | ≤24 hours | Days-Weeks | Months-Years |
| **Eviction** | Auto (hard limit) | Scheduled | Manual/Policy |
| **Redundancy** | None (ephemeral) | Optional | Required |
| **SLA** | Best-effort | 99% | 99.9%+ |
| **Encryption** | Always | Always | Always |
| **Use Case** | CDN, spikes | Projects | Backup |
| **Price/GB/mo** | ~$1.50 | $0.02 | $0.005 |
| **Commitment** | None | Soft | Contract |

### 5.6 Intelligent Tier Selection

GPU Scheduler automatically optimizes:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    AUTOMATIC TIER SELECTION                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  REQUEST: User in Frankfurt needs your Tokyo file                           │
│                                                                             │
│  SCHEDULER EVALUATES:                                                       │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ Option A: Direct fetch from Tokyo                                    │  │
│  │ Route: Tokyo → Internet → Frankfurt                                  │  │
│  │ Latency: 800ms, Cost: $0                                             │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ Option B: Routing wormhole                                           │  │
│  │ Route: Tokyo ══► Singapore ══► Frankfurt                             │  │
│  │ Latency: 200ms, Cost: $0.02                                          │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ Option C: Edge cache hit (already cached!)                           │  │
│  │ Route: Frankfurt cache → User                                        │  │
│  │ Latency: 5ms, Cost: $0                                               │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ Option D: Mid-term storage hit                                       │  │
│  │ Route: EU storage → User                                             │  │
│  │ Latency: 20ms, Cost: $0                                              │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  DECISION: Use cache if exists, else route + auto-cache for next time      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Network Tiers

### 6.1 Hub Operating Modes

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         NETWORK TIERS                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ TIER: PRIVATE                                                       │   │
│  ├─────────────────────────────────────────────────────────────────────┤   │
│  │                                                                      │   │
│  │  • Only my devices                                                   │   │
│  │  • Only my routes                                                    │   │
│  │  • No external sharing                                               │   │
│  │  • No wormholes                                                      │   │
│  │  • Zero monetization                                                 │   │
│  │                                                                      │   │
│  │  Perfect for: Personal use, maximum privacy                          │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ TIER: HYBRID                                                        │   │
│  ├─────────────────────────────────────────────────────────────────────┤   │
│  │                                                                      │   │
│  │  • My devices + invited users                                        │   │
│  │  • Wormholes to SELECTED partners                                    │   │
│  │  • Can use partner services (pay)                                    │   │
│  │  • Can offer services to partners (earn)                             │   │
│  │  • NOT listed publicly                                               │   │
│  │                                                                      │   │
│  │  Perfect for: Teams, organizations, trusted partnerships            │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ TIER: PUBLIC                                                        │   │
│  ├─────────────────────────────────────────────────────────────────────┤   │
│  │                                                                      │   │
│  │  • Listed in Public Hub Directory                                    │   │
│  │  • Accepts wormholes from any hub                                    │   │
│  │  • Full monetization enabled                                         │   │
│  │  • Acts as infrastructure provider                                   │   │
│  │  • Reputation tracked publicly                                       │   │
│  │                                                                      │   │
│  │  Perfect for: Commercial operators, infrastructure providers         │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Public Hub Directory

Registry of public hubs for discovery:

```yaml
# Example public hub listing
hub_listing:
  id: "hub_7f3a9c..."
  name: "FastPath Singapore"
  operator: "FastPath Networks Inc."
  
  location:
    region: "asia-southeast"
    country: "SG"
    coordinates: [1.3521, 103.8198]
  
  capacity:
    bandwidth: 100 Gbps
    storage: 500 TB
    edges: 50
  
  services:
    routing: true
    edge_cache: true
    mid_term_storage: true
    long_term_storage: true
    compute: false  # Coming soon
  
  pricing:
    routing_per_gb: 0.01
    edge_cache_per_gb_day: 0.05
    mid_term_per_gb_month: 0.02
    long_term_per_gb_month: 0.005
  
  reputation:
    uptime_30d: 99.97%
    avg_latency: 12ms
    transfers_completed: 1_234_567
    rating: 4.8/5.0
    verified: true
```

---

## 7. Capability Monetization

### 7.1 The Monetization Stack

Portal enables monetization of infrastructure you already have:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MONETIZATION STACK                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  TODAY:                                                                     │
│  ──────                                                                     │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ ROUTES                                                             │    │
│  │ Charge for traffic passing through your hub                        │    │
│  │ Pricing: Per-GB transferred                                        │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ STORAGE                                                            │    │
│  │ Rent out disk space (edge cache, mid-term, long-term)              │    │
│  │ Pricing: Per-GB per time unit                                      │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ CONTENT                                                            │    │
│  │ Sell access to your portals (paid gates)                           │    │
│  │ Pricing: Your terms (one-time, subscription, etc.)                 │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ═══════════════════════════════════════════════════════════════════════   │
│                                                                             │
│  FUTURE:                                                                    │
│  ───────                                                                    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ CPU COMPUTE                                                        │    │
│  │ Rent out processing power for tasks                                │    │
│  │ Pricing: Per-CPU-hour or per-job                                   │    │
│  │ Use cases: Transcoding, data processing, batch jobs                │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ GPU COMPUTE                                                        │    │
│  │ Rent out GPU for acceleration                                      │    │
│  │ Pricing: Per-GPU-hour or per-operation                             │    │
│  │ Use cases: ML inference, rendering, encoding                       │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │ EXTERNAL DEVICES                                                   │    │
│  │ Share access to connected hardware                                 │    │
│  │ Pricing: Per-use or per-time                                       │    │
│  │ Use cases: Sensors, cameras, printers, specialized equipment       │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 Route Pricing

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      ROUTE PRICING MODEL                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  When operating as a toll hub, you set pricing tiers:                       │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ SINGLE USE                                                          │   │
│  │ • Pay per transfer                                                   │   │
│  │ • Example: $0.01/GB                                                  │   │
│  │ • Good for: Occasional users                                         │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ SUBSCRIPTION                                                        │   │
│  │ • Flat monthly fee for unlimited routing                            │   │
│  │ • Example: $25/month                                                 │   │
│  │ • Good for: Regular users                                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ BULK PACKS                                                          │   │
│  │ • Pre-purchase hours or GB at discount                              │   │
│  │ • Example: 200 hours for $1000 ($5/hour)                            │   │
│  │ • Hours don't expire                                                 │   │
│  │ • Good for: Heavy/predictable usage                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  EXAMPLE USER DECISION:                                                     │
│                                                                             │
│  "I need to transfer 500 GB from NYC to Tokyo"                              │
│                                                                             │
│  ├── Regular (no wormhole): Free, 4 hours, 95% reliability                  │
│  ├── Single use wormhole: $5, 45 min, 99.9% reliability                     │
│  └── Subscription (if I do this often): $25/mo unlimited                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.3 Future: Compute Monetization

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    COMPUTE MONETIZATION (Future)                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  CPU COMPUTE:                                                               │
│  ────────────                                                               │
│                                                                             │
│  • Sandboxed execution environment (WASM, containers)                       │
│  • Jobs submitted through wormhole                                          │
│  • Results returned through wormhole                                        │
│  • Billing: Per-CPU-second or per-job                                       │
│                                                                             │
│  Use cases:                                                                 │
│  • Video transcoding near edge cache                                        │
│  • Data processing at storage location                                      │
│  • Batch jobs distributed across hubs                                       │
│                                                                             │
│  GPU COMPUTE:                                                               │
│  ────────────                                                               │
│                                                                             │
│  • Access partner's GPU through wormhole                                    │
│  • Run inference, encoding, processing                                      │
│  • Billing: Per-GPU-second or per-operation                                 │
│                                                                             │
│  Use cases:                                                                 │
│  • ML inference at edge                                                     │
│  • Real-time video processing                                               │
│  • Rendering near content storage                                           │
│                                                                             │
│  EXAMPLE:                                                                   │
│  ─────────                                                                  │
│                                                                             │
│  You upload 100 GB video to partner's edge cache                            │
│  You rent partner's GPU for 2 hours                                         │
│  GPU transcodes video at edge                                               │
│  Result: Transcoded video ready to serve, never left partner's hub          │
│                                                                             │
│  Cost: Edge cache ($5) + GPU ($20) = $25                                    │
│  Benefit: 10x faster than download→transcode→upload                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 7.4 Future: Device Sharing

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    DEVICE SHARING (Future)                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Share access to connected devices through wormholes:                       │
│                                                                             │
│  DEVICE TYPES:                                                              │
│  • Sensors (weather, environmental, industrial)                             │
│  • Cameras (security, monitoring, specialized)                              │
│  • Printers (3D printers, large format)                                     │
│  • Lab equipment (microscopes, spectrometers)                               │
│  • IoT devices (smart home, automation)                                     │
│                                                                             │
│  ACCESS MODEL:                                                              │
│  • Device registered on hub                                                 │
│  • Access granted through wormhole                                          │
│  • Sandboxed control interface                                              │
│  • Billing: Per-use, per-minute, or subscription                            │
│                                                                             │
│  EXAMPLE:                                                                   │
│  ─────────                                                                  │
│                                                                             │
│  University has specialized microscope                                      │
│  Researcher at another institution needs access                             │
│  Wormhole grants controlled access to microscope                            │
│  Researcher operates remotely, pays per-session                             │
│                                                                             │
│  This is the "Airbnb for scientific equipment" vision                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. QR Code Integration

### 8.1 Overview

QR codes bridge physical and digital worlds:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       QR CODE INTEGRATION                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────┐                                                           │
│  │ ▄▄▄▄▄ ▄▄▄▄ │                                                           │
│  │ █   █ █ ▄▄ │      Scan this QR code                                     │
│  │ █   █ ██▄█ │      ─────────────────                                     │
│  │ █   █ █▄▄▄ │                                                            │
│  │ ▀▀▀▀▀ ▀▀▀▀ │      URL: portal://hub.example/action/payload              │
│  └─────────────┘                                                           │
│                                                                             │
│  QR CODE → HUB URL → WEBHOOK → OPERATION                                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 QR-Triggered Operations

| Action | URL Pattern | Description |
|--------|-------------|-------------|
| Portal Access | `portal://hub/portal/{id}` | Access a shared portal |
| Edge Enrollment | `portal://hub/enroll/{invite}` | Add device to network |
| Payment | `portal://hub/pay/{invoice}` | Process payment |
| Proof Submission | `portal://hub/proof/{req_id}` | Upload required proof |
| Webhook | `portal://hub/webhook/{hook_id}` | Trigger external action |
| Wormhole Request | `portal://hub/wormhole/{offer_id}` | Establish partnership |

### 8.3 Webhook Configuration

```yaml
webhook_config:
  id: "wh_7f3a9c..."
  name: "Conference Check-in"
  enabled: true
  
  trigger:
    type: qr_scan
    qr_url: "portal://hub.example/webhook/wh_7f3a9c..."
  
  action:
    url: "https://api.myapp.com/webhooks/portal"
    method: POST
    headers:
      Authorization: "Bearer ${secret.webhook_token}"
    payload:
      event: "qr_scanned"
      scanner_id: "${scanner.edge_id}"
      timestamp: "${scan.timestamp}"
  
  response_actions:
    on_success:
      - type: grant_portal_access
        portal_id: "${response.portal_id}"
      - type: show_message
        message: "Welcome! Access granted."
```

### 8.4 Physical World Use Cases

| Scenario | QR Action | Result |
|----------|-----------|--------|
| Product Packaging | Portal access | Access digital manual, register warranty |
| Event Ticket | Time-limited portal | Access event materials during event |
| Conference Badge | Wormhole request | Quick partnership between attendees |
| Payment Request | Payment flow | Pay and receive access |
| Device Pairing | Enrollment | Add new device to your network |

---

## 9. Architecture Integration

### 9.1 New Components

| Component | Layer | Purpose |
|-----------|-------|---------|
| Access Gate Engine | Portal | Evaluates free/paid/proof gates |
| Payment Gateway | Application | Processes payments (crypto/fiat) |
| Proof Verifier | Application | Validates proof submissions |
| Wormhole Manager | Portal | Manages cross-network connections |
| Storage Tier Manager | Portal | Handles edge cache, mid/long-term |
| Hub Directory Client | Portal | Queries public hub registry |
| QR Code Handler | Application | Parses URLs, triggers actions |
| Webhook Engine | Portal | Manages webhook configs, dispatches |
| Billing Tracker | Portal | Usage metering & settlement |
| Capability Registry | Portal | Tracks offered services (future: compute, devices) |

### 9.2 How It Fits

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         EXTENDED ARCHITECTURE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      APPLICATION LAYER                              │   │
│  │  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐           │   │
│  │  │  Payment  │ │   Proof   │ │    QR     │ │  Webhook  │           │   │
│  │  │  Gateway  │ │  Verifier │ │  Handler  │ │  Engine   │           │   │
│  │  └───────────┘ └───────────┘ └───────────┘ └───────────┘           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                        │
│  ┌─────────────────────────────────┼───────────────────────────────────┐   │
│  │                         PORTAL LAYER                                │   │
│  │                                 │                                    │   │
│  │  ┌────────────────┐  ┌─────────▼─────────┐  ┌────────────────┐      │   │
│  │  │ Access Gate    │  │ Wormhole Manager  │  │ Storage Tier   │      │   │
│  │  │ Engine         │  │                   │  │ Manager        │      │   │
│  │  │                │  │ • Routing         │  │                │      │   │
│  │  │ • Free/Paid/   │  │ • Content access  │  │ • Edge cache   │      │   │
│  │  │   Proof gates  │  │ • Storage tiers   │  │ • Mid-term     │      │   │
│  │  │ • Composites   │  │ • Future: compute │  │ • Long-term    │      │   │
│  │  └────────────────┘  └───────────────────┘  └────────────────┘      │   │
│  │                                                                      │   │
│  │  ┌────────────────┐  ┌───────────────────┐  ┌────────────────┐      │   │
│  │  │ Hub Directory  │  │ Billing Tracker   │  │ Capability     │      │   │
│  │  │ Client         │  │                   │  │ Registry       │      │   │
│  │  └────────────────┘  └───────────────────┘  └────────────────┘      │   │
│  │                                                                      │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                    │                                        │
│  ┌─────────────────────────────────┼───────────────────────────────────┐   │
│  │                      INTELLIGENCE LAYER                             │   │
│  │                                 │                                    │   │
│  │  ┌─────────────────────────────▼─────────────────────────────────┐  │   │
│  │  │              GPU CHUNK SCHEDULER (Extended)                    │  │   │
│  │  │  • Wormhole-aware routing                                      │  │   │
│  │  │  • Storage tier selection                                      │  │   │
│  │  │  • Cost optimization (tolls, storage, future: compute)         │  │   │
│  │  └───────────────────────────────────────────────────────────────┘  │   │
│  │                                                                      │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. Implementation Priority

### Suggested Phasing

| Phase | Features | Dependencies |
|-------|----------|--------------|
| **A** | Free portals, QR basic | Core Portal |
| **B** | Paid gates (single payment type) | Payment integration |
| **C** | Proof gates (content match) | Proof verifier |
| **D** | Wormhole routing (hub-to-hub) | Network layer |
| **E** | Edge cache (≤24hr) | Wormhole foundation |
| **F** | Public Hub Directory | Wormhole + cache |
| **G** | Mid-term + Long-term storage | Storage tier manager |
| **H** | Route pricing tiers | Billing tracker |
| **I** | ZK proofs, composite gates | Advanced crypto |
| **J** | Toll model, settlement | Full economics |
| **K** | Compute monetization | Sandboxed execution |
| **L** | Device sharing | Device registry |

---

## 11. Summary

Portal is a **private-first, progressively-exposable** platform:

1. **Deploy privately** - Everything works for just you
2. **Enable wormholes** - Connect to partners when ready
3. **Monetize infrastructure** - Earn from routes, storage, compute
4. **Go public** - Become part of the global fabric (optional)

**Current monetization**:
- Routes (transit through your hub)
- Storage (cache, mid-term, long-term)
- Content (paid/proof-gated portals)

**Future monetization**:
- CPU compute
- GPU acceleration
- External devices

**The vision**: Your infrastructure, your rules, your revenue.

*This document is for discussion - feedback welcome on all aspects.*
