# Network Layer

> WireGuard Mesh and P2P Networking

---

## 1. Overview

Portal's network layer is built on WireGuard, providing:

- **P2P Mesh**: Direct edge-to-edge communication
- **NAT Traversal**: Automatic hole punching
- **Roaming**: Seamless IP address changes
- **Encryption**: Transport-level security
- **Hub Relay**: Fallback when direct P2P fails

WireGuard eliminates thousands of lines of custom NAT traversal code while providing kernel-level performance.

---

## 2. Why WireGuard?

### 2.1 Problem-Solution Alignment

| Portal's Problem | WireGuard Solution |
|------------------|-------------------|
| NAT traversal | Built-in UDP hole punching |
| Mobile device roaming | Automatic endpoint discovery |
| Edge identity | Cryptokey routing (public keys) |
| Secure transport | ChaCha20-Poly1305 |
| Hub relay | Peer-to-peer with relay fallback |
| Connection management | Stateless, no setup overhead |
| Firewall traversal | UDP-based, works everywhere |

### 2.2 Cryptographic Alignment

WireGuard and Portal use the same cryptographic primitives:

| Primitive | WireGuard | Portal |
|-----------|-----------|--------|
| Key Exchange | Curve25519 | X25519 (same) |
| Encryption | ChaCha20-Poly1305 | ChaCha20-Poly1305 |
| Hashing | BLAKE2s | BLAKE3 (same family) |
| Protocol | Noise | Compatible |

This means: **Same keypair for WireGuard identity and Portal encryption.**

---

## 3. Architecture

### 3.1 Network Stack

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          NETWORK STACK                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  LAYER 4: APPLICATION (Portal)                                              │
│  ═════════════════════════════                                              │
│  Portal messages, chunk transfers, sync protocol                            │
│  Sees: Clean virtual IPs, no NAT complexity                                 │
│                                                                             │
│  LAYER 3: TRANSPORT (QUIC)                                                  │
│  ═════════════════════════                                                  │
│  Multiplexed streams, congestion control                                    │
│  Connects to: Virtual IPs (10.portal.0.X)                                   │
│                                                                             │
│  LAYER 2: WIREGUARD MESH                                                    │
│  ════════════════════════                                                   │
│  Virtual IP addressing, encryption, routing                                 │
│  Handles: NAT, roaming, encryption                                          │
│                                                                             │
│  LAYER 1: PHYSICAL (UDP)                                                    │
│  ═══════════════════════                                                    │
│  Internet, cellular, WiFi                                                   │
│  Reality: NAT, firewalls, changing IPs                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Mesh Topology

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          WIREGUARD MESH                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                              [Hub]                                          │
│                           10.portal.0.1                                     │
│                          /      |      \                                    │
│                         /       |       \                                   │
│                        /        |        \                                  │
│              [Edge A]      [Edge B]      [Edge C]                           │
│            10.portal.0.2  10.portal.0.3  10.portal.0.4                      │
│                   \           |           /                                 │
│                    \          |          /                                  │
│                     \         |         /                                   │
│                      ─────────┴─────────                                    │
│                         Direct P2P                                          │
│                                                                             │
│  • Each edge has virtual IP in 10.portal.0.0/16                             │
│  • All edges can reach all others via virtual IP                            │
│  • Hub provides discovery and relay fallback                                │
│  • Direct P2P when NAT allows                                               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Cryptokey Routing

### 4.1 Concept

WireGuard's cryptokey routing maps public keys to allowed IPs:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CRYPTOKEY ROUTING                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  WireGuard Configuration (on Edge A):                                       │
│                                                                             │
│  [Interface]                                                                │
│  PrivateKey = <Edge A's private key>                                        │
│  Address = 10.portal.0.2/16                                                 │
│                                                                             │
│  [Peer]  # Hub                                                              │
│  PublicKey = HIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw=                   │
│  AllowedIPs = 10.portal.0.1/32, 10.portal.0.0/24  # Hub + relay range       │
│  Endpoint = hub.portal.example.com:51820                                    │
│                                                                             │
│  [Peer]  # Edge B                                                           │
│  PublicKey = xTIBA5rboUvnH4htodjb60Y7YAf21J7YQMlNGC8HQ14=                   │
│  AllowedIPs = 10.portal.0.3/32                                              │
│  # Endpoint discovered dynamically                                          │
│                                                                             │
│  [Peer]  # Edge C                                                           │
│  PublicKey = TrMvSoP4jYQlY6RIzBgbssQqY3vxI2Pi+y71lOWWXX0=                   │
│  AllowedIPs = 10.portal.0.4/32                                              │
│                                                                             │
│  Result: Packets to 10.portal.0.3 → encrypted to Edge B's public key        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Identity Unification

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    UNIFIED IDENTITY MODEL                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Edge generates ONE keypair:                                                │
│                                                                             │
│  KeyPair:                                                                   │
│    private_key: [u8; 32]                                                    │
│    public_key:  [u8; 32]                                                    │
│                                                                             │
│  This keypair serves as:                                                    │
│  1. WireGuard peer identity                                                 │
│  2. Portal edge identity                                                    │
│  3. Message signing key (converted to Ed25519)                              │
│                                                                             │
│  Hub's edge registry:                                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ public_key           │ virtual_ip    │ last_endpoint    │ status   │   │
│  │ xTIBA5rboUvn...      │ 10.portal.0.3 │ 203.0.113.5:41273│ online   │   │
│  │ TrMvSoP4jYQl...      │ 10.portal.0.4 │ 198.51.100.8:51820│ online  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 5. Automatic Roaming

### 5.1 The Problem

Without WireGuard, IP changes are catastrophic:

```
Traditional approach (painful):
1. Laptop on WiFi: 192.168.1.50
2. User walks to coffee shop
3. New IP: 10.0.0.87
4. All connections break
5. Must re-detect NAT type
6. Re-establish UDP holes
7. Resume interrupted transfers
8. Complex reconnection logic
```

### 5.2 WireGuard Solution

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       AUTOMATIC ROAMING                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Laptop moves: Home WiFi → Coffee Shop → Cellular                           │
│                                                                             │
│  TIME 0: Home WiFi                                                          │
│  • Physical IP: 192.168.1.50                                                │
│  • Virtual IP: 10.portal.0.2 (unchanged)                                    │
│  • Peers know endpoint: <home router public IP>:41273                       │
│                                                                             │
│  TIME 1: Coffee Shop                                                        │
│  • Physical IP: 10.0.0.87 (changed!)                                        │
│  • Virtual IP: 10.portal.0.2 (unchanged)                                    │
│  • Laptop sends packet from new IP                                          │
│  • Peers automatically update endpoint: <coffee shop IP>:52891              │
│  • NO RECONNECTION NEEDED                                                   │
│                                                                             │
│  TIME 2: Cellular                                                           │
│  • Physical IP: Carrier NAT (100.64.x.x)                                    │
│  • Virtual IP: 10.portal.0.2 (unchanged)                                    │
│  • Peers automatically update endpoint                                      │
│  • Transfer continues seamlessly                                            │
│                                                                             │
│  From application perspective: Nothing changed. Same 10.portal.0.2.         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Hub as Coordinator

### 6.1 Responsibilities

| Responsibility | Description |
|----------------|-------------|
| Peer Directory | Maintain edge public keys and virtual IPs |
| Initial Bootstrap | New edge connects to Hub first |
| Endpoint Hints | Provide last-known endpoints to peers |
| Relay Fallback | Route traffic when P2P fails |
| Key Distribution | Share peer keys with relevant edges |

### 6.2 Enrollment Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ENROLLMENT FLOW                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. NEW EDGE GENERATES KEYPAIR                                              │
│     Edge locally generates X25519 keypair                                   │
│     Same key for WireGuard + Portal identity                                │
│                                                                             │
│  2. EDGE CONNECTS TO HUB                                                    │
│     Edge → Hub: "I'm new, here's my public key: xTIBA5r..."                 │
│     Connection via Hub's known public endpoint                              │
│                                                                             │
│  3. HUB ASSIGNS VIRTUAL IP                                                  │
│     Hub allocates: 10.portal.0.5                                            │
│     Hub stores: (public_key, virtual_ip, endpoint)                          │
│                                                                             │
│  4. HUB RETURNS PEER LIST                                                   │
│     Hub → Edge: "You're 10.portal.0.5. Here are your peers:"                │
│     [                                                                       │
│       { pubkey: "abc...", virtual_ip: "10.portal.0.2" },                    │
│       { pubkey: "def...", virtual_ip: "10.portal.0.3" },                    │
│       ...                                                                   │
│     ]                                                                       │
│                                                                             │
│  5. EDGE CONFIGURES WIREGUARD                                               │
│     Edge creates wg-portal interface                                        │
│     Adds Hub and all peers                                                  │
│                                                                             │
│  6. HUB BROADCASTS TO EXISTING PEERS                                        │
│     Hub → All: "New peer 10.portal.0.5 with key xTIBA5r..."                 │
│     Existing edges add new peer to their config                             │
│                                                                             │
│  7. MESH ESTABLISHED                                                        │
│     New edge can reach all others via virtual IP                            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.3 Relay Fallback

When direct P2P fails (both behind symmetric NAT):

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        RELAY FALLBACK                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Direct P2P attempt fails:                                                  │
│  • Edge A: Symmetric NAT                                                    │
│  • Edge B: Symmetric NAT                                                    │
│  • Cannot establish direct UDP path                                         │
│                                                                             │
│  Hub relay activates:                                                       │
│                                                                             │
│  Edge A ──WireGuard──► Hub ──WireGuard──► Edge B                            │
│                                                                             │
│  How it works:                                                              │
│  • Hub has AllowedIPs = 0.0.0.0/0 for edges needing relay                   │
│  • Traffic to unreachable peer routes through Hub                           │
│  • Hub forwards (still encrypted end-to-end by WireGuard)                   │
│  • No special relay code needed - just WireGuard routing                    │
│                                                                             │
│  Performance:                                                               │
│  • Additional latency (2 hops vs 1)                                         │
│  • Hub bandwidth consumption                                                │
│  • Still encrypted, still works                                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 7. LAN Optimization

### 7.1 mDNS Discovery

Edges on the same LAN should transfer directly, bypassing WAN entirely:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        LAN OPTIMIZATION                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  mDNS Service:                                                              │
│  • Service name: _portal._udp.local                                         │
│  • Announced by each edge on local network                                  │
│  • Contains: public key, virtual IP, WireGuard port                         │
│                                                                             │
│  Discovery flow:                                                            │
│  1. Edge A announces: "I'm xTIBA5r... at 192.168.1.50:51820"                │
│  2. Edge B (same LAN) receives announcement                                 │
│  3. Edge B recognizes public key from Hub's peer list                       │
│  4. Edge B updates WireGuard: Edge A endpoint = 192.168.1.50:51820          │
│  5. Direct LAN transfer at full speed!                                      │
│                                                                             │
│  Benefits:                                                                  │
│  • Full LAN speed (1 Gbps, 10 Gbps)                                         │
│  • No WAN bandwidth used                                                    │
│  • No Hub involvement                                                       │
│  • Lower latency                                                            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Connection Simplification

### 8.1 Before WireGuard (Custom Solution)

```
// ~2000-3000 lines of code

async fn connect_to_peer(peer_id: &PeerId) -> Result<Connection> {
    // Detect our NAT type
    let our_nat = stun_detect(&stun_servers).await?;
    
    // Get peer's NAT type from signaling server
    let peer_nat = signaling.get_nat_type(peer_id).await?;
    
    // Determine connection strategy
    match (our_nat, peer_nat) {
        (Symmetric, Symmetric) => {
            // Both symmetric NAT - must use relay
            return connect_via_relay(peer_id).await;
        }
        _ => {
            // Try hole punching
            let candidates = gather_ice_candidates().await?;
            signaling.send_candidates(peer_id, &candidates).await?;
            
            let peer_candidates = signaling.receive_candidates(peer_id).await?;
            
            for (our, their) in candidate_pairs(&candidates, &peer_candidates) {
                match attempt_hole_punch(our, their).await {
                    Ok(conn) => return Ok(conn),
                    Err(_) => continue,
                }
            }
            
            // All hole punch attempts failed, fall back to relay
            connect_via_relay(peer_id).await
        }
    }
}

// Plus: reconnection logic, keepalives, endpoint tracking, etc.
```

### 8.2 After WireGuard

```
// ~50 lines of code

async fn connect_to_peer(peer: &WireGuardPeer) -> Result<QuicConnection> {
    // Connect to peer's virtual IP
    // WireGuard handles: NAT, hole punching, roaming, encryption
    let addr = SocketAddr::new(peer.virtual_ip.into(), PORTAL_PORT);
    QuicClient::connect(addr).await
}

// That's it. WireGuard does the rest.
```

---

## 9. Configuration

### 9.1 Portal Network Configuration

```toml
[network]
# WireGuard interface name
interface = "wg-portal"

# Listen port (0 = auto-select)
listen_port = 0

# Virtual IP subnet
subnet = "10.portal.0.0/16"

# Persistent keepalive for NAT traversal
keepalive_interval = 25

[network.hub]
# Hub's public key
public_key = "HIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw="

# Hub's endpoint (always known)
endpoint = "hub.portal.example.com:51820"

# Hub's virtual IP
virtual_ip = "10.portal.0.1"

[network.lan]
# Enable mDNS discovery
mdns_enabled = true

# mDNS service name
mdns_service = "_portal._udp.local"

# LAN scan interval
scan_interval_secs = 30

[network.performance]
# Prefer kernel WireGuard (faster)
prefer_kernel = true

# Fallback to userspace if kernel unavailable
userspace_fallback = true

# MTU (0 = auto-detect)
mtu = 0
```

---

## 10. Platform Support

### 10.1 WireGuard Availability

| Platform | Kernel Module | Userspace |
|----------|---------------|-----------|
| Linux 5.6+ | ✓ Built-in | wireguard-go |
| Linux <5.6 | ✓ DKMS module | wireguard-go |
| Windows | ✗ | wireguard-go (WireGuard for Windows) |
| macOS | ✗ | wireguard-go |
| iOS | ✗ | NetworkExtension |
| Android | ✓ (some kernels) | wireguard-go |

### 10.2 Performance Comparison

| Mode | Throughput | CPU Usage | Notes |
|------|------------|-----------|-------|
| Kernel | 10+ Gbps | Low | Best performance |
| Userspace (Go) | 1-2 Gbps | Higher | Good fallback |
| Userspace (Rust) | 2-4 Gbps | Medium | boringtun |

Portal uses kernel mode when available, falls back to userspace.

---

## 11. Security Properties

### 11.1 WireGuard Security

| Property | Guarantee |
|----------|-----------|
| Encryption | ChaCha20-Poly1305 per packet |
| Key Exchange | Curve25519 ECDH |
| Perfect Forward Secrecy | New key per handshake |
| Identity Hiding | Peers identified by public key only |
| Replay Protection | Built-in |
| DoS Resistance | Cookie mechanism |

### 11.2 Two-Layer Encryption

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      TWO-LAYER ENCRYPTION                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Layer 1: WireGuard (Transport)                                             │
│  • Encrypts all traffic between edges                                       │
│  • Per-edge keys (X25519)                                                   │
│  • Protects: Traffic in transit                                             │
│                                                                             │
│  Layer 2: Portal (Application)                                              │
│  • Encrypts file contents with user's master key                            │
│  • Per-user keys (derived from master seed)                                 │
│  • Protects: Data at rest, zero-knowledge guarantee                         │
│                                                                             │
│  Data flow:                                                                 │
│  Plaintext → [Portal encrypt] → [WireGuard encrypt] → Network               │
│                                                                             │
│  Why both?                                                                  │
│  • WireGuard protects traffic between edges                                 │
│  • Portal protects data even if Hub is compromised                          │
│  • Defense in depth                                                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 12. Summary

The WireGuard-based network layer provides:

1. **Simplified P2P**: NAT traversal, roaming handled automatically
2. **Mesh Topology**: Any edge can reach any other
3. **Unified Identity**: Same keypair for network and application
4. **LAN Optimization**: mDNS discovery for direct local transfer
5. **Hub Relay**: Fallback when direct P2P impossible
6. **Two-Layer Security**: Transport + application encryption

This eliminates thousands of lines of custom networking code while providing kernel-level performance and battle-tested security.
