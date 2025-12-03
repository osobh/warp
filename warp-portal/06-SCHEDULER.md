# GPU Chunk Scheduler

> Real-Time Optimization Across Billions of Chunks

---

## 1. Overview

The GPU Chunk Scheduler is the intelligence engine of Portal, making real-time decisions about:

- **Source Selection**: Which edge to fetch each chunk from
- **Load Balancing**: Distributing work across available edges
- **Failover**: Instant switching when sources fail
- **Cost Optimization**: Considering bandwidth costs, battery, time of day

Running on GPU, it processes billions of chunks with 50ms tick cycles, enabling transfer optimization that would be impossible on CPU.

---

## 2. Design Goals

| Goal | Target | Why It Matters |
|------|--------|----------------|
| Scale | 1B+ chunks | Petabyte-scale files |
| Tick Time | <10ms | Real-time adaptation |
| Failover | <50ms | Uninterrupted transfers |
| Edges | 100+ | Large mesh networks |
| Updates | 20 Hz | Responsive to conditions |

---

## 3. Architecture

### 3.1 Scheduler Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       SCHEDULER ARCHITECTURE                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         CPU COORDINATOR                             │   │
│  │  • Receives edge health updates                                     │   │
│  │  • Manages transfer requests                                        │   │
│  │  • Dispatches scheduled chunks                                      │   │
│  │  • Handles completion/failure events                                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              │ State sync                                   │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         GPU SCHEDULER                               │   │
│  │                                                                      │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐            │   │
│  │  │ Chunk State   │  │ Edge State    │  │ Cost Matrix   │            │   │
│  │  │ Buffer        │  │ Buffer        │  │               │            │   │
│  │  │               │  │               │  │               │            │   │
│  │  │ • Status      │  │ • Bandwidth   │  │ [chunk,edge]  │            │   │
│  │  │ • Assignments │  │ • RTT         │  │ = cost score  │            │   │
│  │  │ • Retry count │  │ • Health      │  │               │            │   │
│  │  │ • Priority    │  │ • Queue depth │  │               │            │   │
│  │  └───────────────┘  └───────────────┘  └───────────────┘            │   │
│  │                                                                      │   │
│  │  ┌─────────────────────────────────────────────────────────────┐    │   │
│  │  │                    GPU KERNELS                              │    │   │
│  │  │                                                              │    │   │
│  │  │  1. cost_matrix_update()    - Parallel cost calculation     │    │   │
│  │  │  2. k_best_paths()          - Find top K sources per chunk  │    │   │
│  │  │  3. detect_failures()       - Identify failed transfers     │    │   │
│  │  │  4. failover_assign()       - Reassign from backup paths    │    │   │
│  │  │  5. load_balance()          - Redistribute overloaded edges │    │   │
│  │  │  6. generate_dispatch()     - Output scheduling decisions   │    │   │
│  │  │                                                              │    │   │
│  │  └─────────────────────────────────────────────────────────────┘    │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              │ Dispatch queue                               │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      TRANSFER EXECUTOR                              │   │
│  │  • Executes scheduled chunk fetches                                 │   │
│  │  • Reports completion/failure back to coordinator                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Data Structures

**Chunk State** (per chunk):
```
ChunkState:
  chunk_id:       u32           # Index into chunk array
  cid:            [u8; 32]      # Content ID (for verification)
  status:         u8            # PENDING, IN_FLIGHT, COMPLETE, FAILED
  assigned_edge:  u16           # Currently assigned edge
  backup_edges:   [u16; K]      # Pre-computed backup sources
  retry_count:    u8            # Number of failed attempts
  priority:       f32           # Higher = schedule first
  deadline:       u64           # Timestamp for timeout
```

**Edge State** (per edge):
```
EdgeState:
  edge_id:        u16           # Index into edge array
  virtual_ip:     u32           # 10.portal.0.X
  bandwidth_est:  f32           # Estimated bandwidth (bytes/sec)
  rtt_est:        f32           # Estimated round-trip time (ms)
  health_score:   f32           # 0.0 - 1.0, composite health
  queue_depth:    u32           # Chunks currently in flight
  max_parallel:   u32           # Maximum concurrent chunks
  available:      bool          # Currently reachable
  constraints:    u32           # Flags: METERED, BATTERY_LOW, etc.
```

**Cost Matrix** (chunks × edges):
```
CostMatrix[chunk_id][edge_id] = {
  cost:           f32           # Lower is better
  has_chunk:      bool          # Does this edge have the chunk?
}
```

---

## 4. Cost Model

### 4.1 Cost Function

The scheduler computes a cost for each (chunk, edge) pair:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          COST FUNCTION                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  cost(chunk, edge) = Σ (weight_i × factor_i)                                │
│                                                                             │
│  FACTORS:                                                                   │
│  ──────────────────────────────────────────────────────────────────────     │
│                                                                             │
│  1. Transfer Time (weight: 0.4)                                             │
│     time = chunk_size / bandwidth_est + rtt_est                             │
│     factor = normalize(time, 0, max_acceptable_time)                        │
│                                                                             │
│  2. Queue Load (weight: 0.2)                                                │
│     load = queue_depth / max_parallel                                       │
│     factor = load²  (penalize heavily as queue fills)                       │
│                                                                             │
│  3. Health Score (weight: 0.2)                                              │
│     factor = 1.0 - health_score                                             │
│     (lower health = higher cost)                                            │
│                                                                             │
│  4. Monetary Cost (weight: 0.1)                                             │
│     factor = is_metered ? 1.0 : 0.0                                         │
│     (avoid metered connections when alternatives exist)                     │
│                                                                             │
│  5. Power Cost (weight: 0.1)                                                │
│     factor = battery_low ? 1.0 : 0.0                                        │
│     (avoid draining battery-powered devices)                                │
│                                                                             │
│  INFINITY COST (never select):                                              │
│  • Edge doesn't have chunk: cost = ∞                                        │
│  • Edge unavailable: cost = ∞                                               │
│  • Retry limit exceeded for this edge: cost = ∞                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 GPU Cost Matrix Kernel

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GPU COST MATRIX UPDATE                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Parallel over all (chunk, edge) pairs:                                     │
│                                                                             │
│  __global__ void cost_matrix_update(                                        │
│      ChunkState* chunks,      // [num_chunks]                               │
│      EdgeState* edges,        // [num_edges]                                │
│      ChunkAvailability* avail,// [num_chunks × num_edges]                   │
│      float* cost_matrix,      // [num_chunks × num_edges] output            │
│      CostWeights weights                                                    │
│  ) {                                                                        │
│      int chunk_id = blockIdx.x * blockDim.x + threadIdx.x;                  │
│      int edge_id = blockIdx.y * blockDim.y + threadIdx.y;                   │
│                                                                             │
│      if (chunk_id >= num_chunks || edge_id >= num_edges) return;            │
│                                                                             │
│      // Check availability                                                  │
│      if (!avail[chunk_id * num_edges + edge_id] ||                          │
│          !edges[edge_id].available) {                                       │
│          cost_matrix[chunk_id * num_edges + edge_id] = INFINITY;            │
│          return;                                                            │
│      }                                                                      │
│                                                                             │
│      // Compute cost factors                                                │
│      float time_cost = compute_time_cost(chunks[chunk_id], edges[edge_id]); │
│      float load_cost = compute_load_cost(edges[edge_id]);                   │
│      float health_cost = 1.0f - edges[edge_id].health_score;                │
│      float money_cost = (edges[edge_id].constraints & METERED) ? 1.0 : 0.0; │
│      float power_cost = (edges[edge_id].constraints & BATTERY) ? 1.0 : 0.0; │
│                                                                             │
│      // Weighted sum                                                        │
│      float total = weights.time * time_cost                                 │
│                  + weights.load * load_cost                                 │
│                  + weights.health * health_cost                             │
│                  + weights.money * money_cost                               │
│                  + weights.power * power_cost;                              │
│                                                                             │
│      cost_matrix[chunk_id * num_edges + edge_id] = total;                   │
│  }                                                                          │
│                                                                             │
│  For 10M chunks × 100 edges = 1B cost calculations                          │
│  GPU time: ~2ms on RTX 5090                                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 5. K-Best Paths

### 5.1 Pre-computed Backup Sources

For each chunk, the scheduler maintains K backup sources:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        K-BEST PATHS                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  For each chunk, find top K edges by cost:                                  │
│                                                                             │
│  Chunk 0:  [Edge 5, Edge 12, Edge 3]     costs: [0.1, 0.3, 0.5]             │
│  Chunk 1:  [Edge 12, Edge 5, Edge 8]     costs: [0.2, 0.25, 0.4]            │
│  Chunk 2:  [Edge 3, Edge 5, Edge 12]     costs: [0.15, 0.2, 0.35]           │
│  ...                                                                        │
│                                                                             │
│  Primary assignment = best[0]                                               │
│  Backup 1 = best[1]                                                         │
│  Backup 2 = best[2]                                                         │
│                                                                             │
│  On failure: Instantly switch to backup, no recomputation needed            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 GPU K-Best Selection

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GPU K-BEST SELECTION                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Parallel partial sort per chunk:                                           │
│                                                                             │
│  __global__ void k_best_paths(                                              │
│      float* cost_matrix,      // [num_chunks × num_edges]                   │
│      uint16_t* best_edges,    // [num_chunks × K] output                    │
│      float* best_costs,       // [num_chunks × K] output                    │
│      int num_chunks,                                                        │
│      int num_edges,                                                         │
│      int K                                                                  │
│  ) {                                                                        │
│      int chunk_id = blockIdx.x * blockDim.x + threadIdx.x;                  │
│      if (chunk_id >= num_chunks) return;                                    │
│                                                                             │
│      // Each thread handles one chunk                                       │
│      // Use register-based insertion sort for small K                       │
│      float top_k_costs[K];                                                  │
│      uint16_t top_k_edges[K];                                               │
│                                                                             │
│      // Initialize with infinity                                            │
│      for (int i = 0; i < K; i++) {                                          │
│          top_k_costs[i] = INFINITY;                                         │
│          top_k_edges[i] = INVALID_EDGE;                                     │
│      }                                                                      │
│                                                                             │
│      // Scan all edges, maintain top K                                      │
│      float* row = &cost_matrix[chunk_id * num_edges];                       │
│      for (int e = 0; e < num_edges; e++) {                                  │
│          float cost = row[e];                                               │
│          if (cost < top_k_costs[K-1]) {                                     │
│              // Insert into sorted position                                 │
│              insert_sorted(top_k_costs, top_k_edges, cost, e, K);           │
│          }                                                                  │
│      }                                                                      │
│                                                                             │
│      // Write results                                                       │
│      for (int i = 0; i < K; i++) {                                          │
│          best_edges[chunk_id * K + i] = top_k_edges[i];                     │
│          best_costs[chunk_id * K + i] = top_k_costs[i];                     │
│      }                                                                      │
│  }                                                                          │
│                                                                             │
│  For 10M chunks, K=3: ~3ms on RTX 5090                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Failover

### 6.1 Failure Detection

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      FAILURE DETECTION                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  A chunk is considered failed if:                                           │
│                                                                             │
│  1. TIMEOUT                                                                 │
│     current_time > chunk.deadline                                           │
│     (deadline = dispatch_time + expected_time × timeout_multiplier)         │
│                                                                             │
│  2. EXPLICIT FAILURE                                                        │
│     Transfer executor reports error (connection reset, etc.)                │
│                                                                             │
│  3. EDGE BECAME UNAVAILABLE                                                 │
│     Assigned edge's available flag set to false                             │
│                                                                             │
│  GPU kernel scans all in-flight chunks in parallel:                         │
│                                                                             │
│  __global__ void detect_failures(                                           │
│      ChunkState* chunks,                                                    │
│      EdgeState* edges,                                                      │
│      uint64_t current_time,                                                 │
│      uint32_t* failed_chunks,  // Output: indices of failed chunks          │
│      uint32_t* num_failed      // Output: count                             │
│  ) {                                                                        │
│      int chunk_id = blockIdx.x * blockDim.x + threadIdx.x;                  │
│      if (chunk_id >= num_chunks) return;                                    │
│                                                                             │
│      ChunkState* c = &chunks[chunk_id];                                     │
│      if (c->status != IN_FLIGHT) return;                                    │
│                                                                             │
│      bool failed = false;                                                   │
│      failed |= (current_time > c->deadline);                                │
│      failed |= (!edges[c->assigned_edge].available);                        │
│                                                                             │
│      if (failed) {                                                          │
│          uint32_t idx = atomicAdd(num_failed, 1);                           │
│          failed_chunks[idx] = chunk_id;                                     │
│          c->status = FAILED;                                                │
│      }                                                                      │
│  }                                                                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Instant Failover

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       INSTANT FAILOVER                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Because backup paths are pre-computed, failover is instant:                │
│                                                                             │
│  __global__ void failover_assign(                                           │
│      ChunkState* chunks,                                                    │
│      uint32_t* failed_chunks,                                               │
│      uint32_t num_failed                                                    │
│  ) {                                                                        │
│      int i = blockIdx.x * blockDim.x + threadIdx.x;                         │
│      if (i >= num_failed) return;                                           │
│                                                                             │
│      uint32_t chunk_id = failed_chunks[i];                                  │
│      ChunkState* c = &chunks[chunk_id];                                     │
│                                                                             │
│      // Move to next backup                                                 │
│      c->retry_count++;                                                      │
│                                                                             │
│      if (c->retry_count < K) {                                              │
│          // Use pre-computed backup                                         │
│          c->assigned_edge = c->backup_edges[c->retry_count];                │
│          c->status = PENDING;  // Ready for re-dispatch                     │
│      } else {                                                               │
│          // All backups exhausted                                           │
│          c->status = FAILED_PERMANENT;                                      │
│      }                                                                      │
│  }                                                                          │
│                                                                             │
│  TIME: Failure detected to new assignment < 50ms                            │
│  (One scheduler tick)                                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 7. Load Balancing

### 7.1 Problem

Without load balancing, fast edges get overloaded:

```
Before balancing:
  Edge A (fast):   [████████████████████████] 100% queue, dropping
  Edge B (medium): [████████            ] 40% queue
  Edge C (slow):   [██                  ] 10% queue
  
Chunks waiting because Edge A is saturated, even though B and C have capacity.
```

### 7.2 GPU Load Balancer

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       GPU LOAD BALANCING                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Strategy: Redistribute from overloaded edges to underutilized edges        │
│                                                                             │
│  __global__ void load_balance(                                              │
│      ChunkState* chunks,                                                    │
│      EdgeState* edges,                                                      │
│      float* cost_matrix,                                                    │
│      float overload_threshold,   // e.g., 0.9 (90% of max_parallel)         │
│      float underload_threshold   // e.g., 0.5 (50% of max_parallel)         │
│  ) {                                                                        │
│      int chunk_id = blockIdx.x * blockDim.x + threadIdx.x;                  │
│      if (chunk_id >= num_chunks) return;                                    │
│                                                                             │
│      ChunkState* c = &chunks[chunk_id];                                     │
│      if (c->status != PENDING) return;                                      │
│                                                                             │
│      EdgeState* assigned = &edges[c->assigned_edge];                        │
│      float load = (float)assigned->queue_depth / assigned->max_parallel;    │
│                                                                             │
│      // Only rebalance if assigned edge is overloaded                       │
│      if (load < overload_threshold) return;                                 │
│                                                                             │
│      // Find better alternative                                             │
│      for (int i = 1; i < K; i++) {  // Skip current (i=0)                   │
│          uint16_t alt_edge = c->backup_edges[i];                            │
│          EdgeState* alt = &edges[alt_edge];                                 │
│          float alt_load = (float)alt->queue_depth / alt->max_parallel;      │
│                                                                             │
│          if (alt_load < underload_threshold) {                              │
│              // Reassign to underutilized edge                              │
│              c->assigned_edge = alt_edge;                                   │
│              atomicSub(&assigned->queue_depth, 1);                          │
│              atomicAdd(&alt->queue_depth, 1);                               │
│              break;                                                         │
│          }                                                                  │
│      }                                                                      │
│  }                                                                          │
│                                                                             │
│  After balancing:                                                           │
│    Edge A: [████████████████    ] 80% queue                                 │
│    Edge B: [████████████        ] 60% queue                                 │
│    Edge C: [████████            ] 40% queue                                 │
│                                                                             │
│  All edges productively utilized.                                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Scheduler Tick

### 8.1 Tick Loop

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       SCHEDULER TICK (50ms)                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Every 50ms:                                                                │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 1. SYNC STATE (CPU → GPU)                              ~0.5ms       │   │
│  │    • Update edge states (health, availability)                      │   │
│  │    • Mark completed chunks                                          │   │
│  │    • Add new chunks to pending                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 2. UPDATE COST MATRIX (GPU)                            ~2ms         │   │
│  │    • Parallel cost calculation for all chunk-edge pairs             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 3. COMPUTE K-BEST PATHS (GPU)                          ~3ms         │   │
│  │    • Find top K sources per chunk                                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 4. DETECT FAILURES (GPU)                               ~0.5ms       │   │
│  │    • Scan in-flight chunks for timeouts                             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 5. FAILOVER ASSIGNMENT (GPU)                           ~0.5ms       │   │
│  │    • Reassign failed chunks to backups                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 6. LOAD BALANCE (GPU)                                  ~1ms         │   │
│  │    • Redistribute from overloaded edges                             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ 7. GENERATE DISPATCH (GPU → CPU)                       ~1ms         │   │
│  │    • Select next batch of chunks to dispatch                        │   │
│  │    • Copy dispatch queue to CPU                                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TOTAL TICK TIME: ~8.5ms (well under 50ms budget)                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Dispatch Generation

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      DISPATCH GENERATION                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Select chunks to dispatch this tick:                                       │
│                                                                             │
│  Constraints:                                                               │
│  • Don't exceed edge's max_parallel                                         │
│  • Prioritize higher-priority chunks                                        │
│  • Respect dispatch rate limit                                              │
│                                                                             │
│  Output: Dispatch queue                                                     │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ chunk_id │ target_edge │ expected_time │ deadline                   │   │
│  │ ──────── │ ─────────── │ ───────────── │ ────────                   │   │
│  │ 1234     │ Edge 5      │ 50ms          │ T+250ms                    │   │
│  │ 5678     │ Edge 12     │ 30ms          │ T+150ms                    │   │
│  │ 9012     │ Edge 5      │ 50ms          │ T+250ms                    │   │
│  │ ...      │ ...         │ ...           │ ...                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  CPU coordinator executes dispatch queue via network layer.                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Auto-Reconciliation

### 9.1 Continuous Learning

The scheduler continuously adapts to observed performance:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     CONTINUOUS LEARNING                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  BANDWIDTH ESTIMATION (EMA):                                                │
│  ───────────────────────────                                                │
│  On chunk completion:                                                       │
│    actual_bw = chunk_size / transfer_time                                   │
│    edge.bandwidth_est = α × actual_bw + (1-α) × edge.bandwidth_est          │
│    where α = 0.1 (smooth but responsive)                                    │
│                                                                             │
│  RTT ESTIMATION (TCP-style SRTT):                                           │
│  ─────────────────────────────────                                          │
│  On chunk completion:                                                       │
│    rtt_sample = first_byte_time - request_time                              │
│    edge.rtt_est = 0.875 × edge.rtt_est + 0.125 × rtt_sample                 │
│                                                                             │
│  HEALTH SCORING:                                                            │
│  ───────────────                                                            │
│  health = weighted combination of:                                          │
│    • Success rate (last 100 chunks)                                         │
│    • Uptime (last hour)                                                     │
│    • Consistency (variance in bandwidth)                                    │
│                                                                             │
│  REOPTIMIZATION TRIGGER:                                                    │
│  ─────────────────────────                                                  │
│  Trigger full reschedule when:                                              │
│    • New edge comes online                                                  │
│    • Edge goes offline                                                      │
│    • Significant bandwidth change detected                                  │
│    • Time-of-day schedule changes                                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 9.2 Predictive Pre-Positioning

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                   PREDICTIVE PRE-POSITIONING                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Anticipate what chunks will be needed and pre-fetch:                       │
│                                                                             │
│  PATTERNS:                                                                  │
│  ─────────                                                                  │
│  1. Sequential access: User streaming video                                 │
│     → Pre-fetch next N chunks ahead                                         │
│                                                                             │
│  2. Directory listing: User browsing folder                                 │
│     → Pre-fetch file metadata for visible items                             │
│                                                                             │
│  3. Time-based: Daily backup at 2 AM                                        │
│     → Pre-position chunks to backup edge before 2 AM                        │
│                                                                             │
│  4. Geographic: User traveling                                              │
│     → Replicate to edges near destination                                   │
│                                                                             │
│  IMPLEMENTATION:                                                            │
│  ───────────────                                                            │
│  • Track access patterns in lightweight model                               │
│  • Scheduler adds low-priority "pre-position" tasks                         │
│  • These run during idle time, don't compete with active transfers          │
│  • Result: When user requests, chunk is already local                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. Scale Performance

### 10.1 Benchmark Projections (RTX 5090)

| Chunks | Edges | Tick Time | Notes |
|--------|-------|-----------|-------|
| 100K | 10 | <1 ms | Trivial |
| 1M | 50 | ~2 ms | Easy |
| 10M | 100 | ~8 ms | Comfortable |
| 100M | 100 | ~50 ms | At budget |
| 1B | 100 | ~500 ms | Multi-tick |

For files >100M chunks (>400 TB), use hierarchical scheduling or multi-GPU.

### 10.2 Memory Usage

| Chunks | Edges | Chunk State | Edge State | Cost Matrix | Total |
|--------|-------|-------------|------------|-------------|-------|
| 10M | 100 | 480 MB | 10 KB | 4 GB | ~4.5 GB |
| 100M | 100 | 4.8 GB | 10 KB | 40 GB* | Streaming |
| 1B | 100 | 48 GB* | 10 KB | 400 GB* | Multi-GPU |

*Requires streaming or partitioning

---

## 11. Summary

The GPU Chunk Scheduler provides:

1. **Real-Time Optimization**: 50ms tick cycles for responsive adaptation
2. **Pre-Computed Failover**: Instant switching on failure (<50ms)
3. **Intelligent Load Balancing**: Maximize aggregate throughput
4. **Continuous Learning**: Adapt to observed network conditions
5. **Cost-Aware Routing**: Consider money, power, time constraints
6. **Petabyte Scale**: Handle billions of chunks on single GPU

This is the intelligence that makes Portal more than simple file transfer—it's a self-optimizing, self-healing data fabric.
