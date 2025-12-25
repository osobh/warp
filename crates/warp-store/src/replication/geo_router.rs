//! Geo-aware routing for distributed reads
//!
//! Provides optimized read paths based on:
//! - Geographic proximity (latency measurements)
//! - Domain health status
//! - Read preference configuration
//! - Parallel shard fetching from multiple domains

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::{DomainId, DomainRegistry, ReadPreference, ShardDistributionInfo, ShardHealth, ShardIndex};
use crate::error::{Error, Result};

/// Latency statistics for a domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Minimum observed latency
    pub min_ms: u32,

    /// Maximum observed latency
    pub max_ms: u32,

    /// Average latency
    pub avg_ms: u32,

    /// Number of samples
    pub sample_count: u64,

    /// Last measurement time (not serialized)
    #[serde(skip, default)]
    pub last_measured: Option<Instant>,

    /// Exponential moving average (for adaptive routing)
    pub ema_ms: f64,
}

impl Default for LatencyStats {
    fn default() -> Self {
        Self {
            min_ms: u32::MAX,
            max_ms: 0,
            avg_ms: 0,
            sample_count: 0,
            last_measured: None,
            ema_ms: 0.0,
        }
    }
}

impl LatencyStats {
    /// EMA smoothing factor (0.1 = slow adaptation, 0.9 = fast adaptation)
    const EMA_ALPHA: f64 = 0.2;

    /// Record a new latency measurement
    pub fn record(&mut self, latency_ms: u32) {
        self.min_ms = self.min_ms.min(latency_ms);
        self.max_ms = self.max_ms.max(latency_ms);

        // Update running average
        let old_total = self.avg_ms as u64 * self.sample_count;
        self.sample_count += 1;
        self.avg_ms = ((old_total + latency_ms as u64) / self.sample_count) as u32;

        // Update EMA
        if self.sample_count == 1 {
            self.ema_ms = latency_ms as f64;
        } else {
            self.ema_ms = Self::EMA_ALPHA * latency_ms as f64 + (1.0 - Self::EMA_ALPHA) * self.ema_ms;
        }

        self.last_measured = Some(Instant::now());
    }

    /// Check if stats are stale (no recent measurements)
    pub fn is_stale(&self, max_age: Duration) -> bool {
        self.last_measured
            .map(|t| t.elapsed() > max_age)
            .unwrap_or(true)
    }
}

/// Selection of domains for reading shards
#[derive(Debug, Clone)]
pub struct ShardReadPlan {
    /// Mapping of shard index to list of domains (in preference order)
    pub shard_sources: HashMap<ShardIndex, Vec<DomainId>>,

    /// Primary domain for this read
    pub primary_domain: Option<DomainId>,

    /// Total shards needed to reconstruct object
    pub shards_needed: usize,

    /// Whether parallel fetching is recommended
    pub parallel_fetch: bool,
}

impl ShardReadPlan {
    /// Get the preferred domain for a specific shard
    pub fn preferred_domain(&self, shard_index: ShardIndex) -> Option<DomainId> {
        self.shard_sources.get(&shard_index).and_then(|v| v.first()).copied()
    }

    /// Get all domains that will be used
    pub fn all_domains(&self) -> Vec<DomainId> {
        let mut domains: Vec<_> = self
            .shard_sources
            .values()
            .filter_map(|v| v.first())
            .copied()
            .collect();
        domains.sort();
        domains.dedup();
        domains
    }
}

/// Geo-aware router for optimized distributed reads
pub struct GeoRouter {
    /// Latency measurements per domain
    latency: DashMap<DomainId, LatencyStats>,

    /// Domain registry reference
    domain_registry: Arc<DomainRegistry>,

    /// Local domain ID
    local_domain_id: DomainId,

    /// Round-robin counter for RoundRobin preference
    round_robin_counter: std::sync::atomic::AtomicU64,

    /// Maximum age before latency stats are considered stale
    max_latency_age: Duration,
}

impl GeoRouter {
    /// Create a new geo router
    pub fn new(domain_registry: Arc<DomainRegistry>, local_domain_id: DomainId) -> Self {
        Self {
            latency: DashMap::new(),
            domain_registry,
            local_domain_id,
            round_robin_counter: std::sync::atomic::AtomicU64::new(0),
            max_latency_age: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Record a latency measurement for a domain
    pub fn record_latency(&self, domain_id: DomainId, latency_ms: u32) {
        self.latency
            .entry(domain_id)
            .or_insert_with(LatencyStats::default)
            .record(latency_ms);
    }

    /// Get latency stats for a domain
    pub fn get_latency(&self, domain_id: DomainId) -> Option<LatencyStats> {
        self.latency.get(&domain_id).map(|r| r.clone())
    }

    /// Get estimated latency to a domain (using EMA)
    pub fn estimated_latency_ms(&self, domain_id: DomainId) -> Option<u32> {
        self.latency.get(&domain_id).map(|s| s.ema_ms as u32)
    }

    /// Plan read operations for an object's shards
    pub async fn plan_read(
        &self,
        distribution: &ShardDistributionInfo,
        preference: ReadPreference,
        primary_domain: Option<DomainId>,
    ) -> Result<ShardReadPlan> {
        let mut shard_sources: HashMap<ShardIndex, Vec<DomainId>> = HashMap::new();
        let parallel_fetch = matches!(preference, ReadPreference::Any | ReadPreference::Nearest);

        for shard_idx in 0..distribution.total_shards {
            let idx = shard_idx as ShardIndex;

            // Get healthy locations for this shard
            let locations = distribution.locations.get(&idx).map(|locs| {
                locs.iter()
                    .filter(|l| l.health == ShardHealth::Healthy)
                    .map(|l| l.domain_id)
                    .collect::<Vec<_>>()
            });

            if let Some(mut domains) = locations {
                if domains.is_empty() {
                    continue;
                }

                // Sort domains based on preference
                match preference {
                    ReadPreference::Nearest => {
                        self.sort_by_latency(&mut domains);
                    }
                    ReadPreference::Primary => {
                        if let Some(primary) = primary_domain {
                            // Move primary to front if present
                            if let Some(pos) = domains.iter().position(|&d| d == primary) {
                                domains.swap(0, pos);
                            }
                        }
                    }
                    ReadPreference::Any => {
                        // Keep as-is (arbitrary order)
                    }
                    ReadPreference::RoundRobin => {
                        let counter = self
                            .round_robin_counter
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let offset = counter as usize % domains.len();
                        domains.rotate_left(offset);
                    }
                }

                shard_sources.insert(idx, domains);
            }
        }

        // Verify we have enough shards
        if shard_sources.len() < distribution.data_shards {
            return Err(Error::Replication(format!(
                "Insufficient healthy shards: have {} but need {}",
                shard_sources.len(),
                distribution.data_shards
            )));
        }

        debug!(
            bucket = distribution.bucket,
            key = distribution.key,
            preference = ?preference,
            shard_count = shard_sources.len(),
            "Planned shard read"
        );

        Ok(ShardReadPlan {
            shard_sources,
            primary_domain,
            shards_needed: distribution.data_shards,
            parallel_fetch,
        })
    }

    /// Sort domains by latency (lowest first)
    fn sort_by_latency(&self, domains: &mut [DomainId]) {
        domains.sort_by(|a, b| {
            let lat_a = self.estimated_latency_ms(*a).unwrap_or(u32::MAX);
            let lat_b = self.estimated_latency_ms(*b).unwrap_or(u32::MAX);

            // Prefer local domain when latencies are similar
            if *a == self.local_domain_id && lat_a <= lat_b + 10 {
                std::cmp::Ordering::Less
            } else if *b == self.local_domain_id && lat_b <= lat_a + 10 {
                std::cmp::Ordering::Greater
            } else {
                lat_a.cmp(&lat_b)
            }
        });
    }

    /// Find the nearest healthy domain for a specific shard
    pub fn nearest_domain_for_shard(
        &self,
        distribution: &ShardDistributionInfo,
        shard_index: ShardIndex,
    ) -> Option<DomainId> {
        let locations = distribution.locations.get(&shard_index)?;

        let mut healthy_domains: Vec<_> = locations
            .iter()
            .filter(|l| l.health == ShardHealth::Healthy)
            .map(|l| l.domain_id)
            .collect();

        if healthy_domains.is_empty() {
            return None;
        }

        self.sort_by_latency(&mut healthy_domains);
        healthy_domains.first().copied()
    }

    /// Get domains ranked by latency (for general routing)
    pub fn ranked_domains(&self) -> Vec<(DomainId, u32)> {
        let mut ranked: Vec<_> = self
            .latency
            .iter()
            .filter(|e| !e.is_stale(self.max_latency_age))
            .map(|e| (*e.key(), e.ema_ms as u32))
            .collect();

        ranked.sort_by_key(|&(_, lat)| lat);
        ranked
    }

    /// Probe latency to a domain
    pub async fn probe_domain(&self, domain_id: DomainId) -> Result<u32> {
        let start = Instant::now();

        // Check if domain is healthy
        if !self.domain_registry.is_healthy(domain_id).await {
            return Err(Error::Replication(format!(
                "Domain {} is not healthy",
                domain_id
            )));
        }

        // In a real implementation, this would send a ping packet
        // For now, we simulate with the domain registry check
        let latency_ms = start.elapsed().as_millis() as u32;

        self.record_latency(domain_id, latency_ms);

        Ok(latency_ms)
    }

    /// Probe all known domains
    pub async fn probe_all_domains(&self) {
        let domain_ids = self.domain_registry.domain_ids();

        for domain_id in domain_ids {
            if let Err(e) = self.probe_domain(domain_id).await {
                warn!(domain_id = domain_id, error = %e, "Failed to probe domain");
            }
        }
    }

    /// Get overall latency statistics
    pub fn stats(&self) -> GeoRouterStats {
        let domains = self.latency.len();
        let stale_count = self
            .latency
            .iter()
            .filter(|e| e.is_stale(self.max_latency_age))
            .count();

        let (min_lat, max_lat, total_lat) =
            self.latency.iter().fold((u32::MAX, 0u32, 0u64), |acc, e| {
                let lat = e.ema_ms as u32;
                (acc.0.min(lat), acc.1.max(lat), acc.2 + lat as u64)
            });

        GeoRouterStats {
            domains_tracked: domains,
            stale_entries: stale_count,
            min_latency_ms: if domains > 0 { min_lat } else { 0 },
            max_latency_ms: max_lat,
            avg_latency_ms: if domains > 0 {
                (total_lat / domains as u64) as u32
            } else {
                0
            },
        }
    }
}

/// Statistics about geo router state
#[derive(Debug, Clone)]
pub struct GeoRouterStats {
    /// Number of domains being tracked
    pub domains_tracked: usize,

    /// Number of stale entries
    pub stale_entries: usize,

    /// Minimum latency across all domains
    pub min_latency_ms: u32,

    /// Maximum latency across all domains
    pub max_latency_ms: u32,

    /// Average latency across all domains
    pub avg_latency_ms: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication::ErasurePolicy;

    fn create_test_registry() -> Arc<DomainRegistry> {
        Arc::new(DomainRegistry::new(1))
    }

    #[test]
    fn test_latency_stats() {
        let mut stats = LatencyStats::default();

        stats.record(100);
        assert_eq!(stats.min_ms, 100);
        assert_eq!(stats.max_ms, 100);
        assert_eq!(stats.avg_ms, 100);
        assert!((stats.ema_ms - 100.0).abs() < 0.01);

        stats.record(200);
        assert_eq!(stats.min_ms, 100);
        assert_eq!(stats.max_ms, 200);
        assert_eq!(stats.avg_ms, 150);

        stats.record(50);
        assert_eq!(stats.min_ms, 50);
        assert_eq!(stats.sample_count, 3);
    }

    #[tokio::test]
    async fn test_geo_router_basic() {
        let registry = create_test_registry();
        let router = GeoRouter::new(registry, 1);

        // Record latencies
        router.record_latency(1, 10);
        router.record_latency(2, 50);
        router.record_latency(3, 30);

        // Check estimated latencies
        assert_eq!(router.estimated_latency_ms(1), Some(10));
        assert_eq!(router.estimated_latency_ms(2), Some(50));
        assert_eq!(router.estimated_latency_ms(3), Some(30));

        // Get ranked domains
        let ranked = router.ranked_domains();
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].0, 1); // Lowest latency
        assert_eq!(ranked[1].0, 3);
        assert_eq!(ranked[2].0, 2); // Highest latency
    }

    #[tokio::test]
    async fn test_plan_read_nearest() {
        let registry = create_test_registry();
        let router = GeoRouter::new(registry, 1);

        // Record latencies (domain 2 is nearest)
        router.record_latency(1, 100);
        router.record_latency(2, 10);
        router.record_latency(3, 50);

        // Create distribution with shards across domains
        let policy = ErasurePolicy::rs_4_2();
        let mut dist = ShardDistributionInfo::new("bucket", "key", &policy);

        for i in 0..6 {
            let domain_id = (i % 3) + 1;
            dist.add_location(
                i as ShardIndex,
                super::super::shards::ShardLocation {
                    domain_id,
                    node_id: "node1".to_string(),
                    path: format!("/data/shard_{}", i),
                    last_verified: Some(Instant::now()),
                    health: ShardHealth::Healthy,
                },
            );
        }

        let plan = router
            .plan_read(&dist, ReadPreference::Nearest, None)
            .await
            .unwrap();

        assert_eq!(plan.shards_needed, 4);
        assert!(plan.parallel_fetch);

        // Shards on domain 2 should prefer domain 2
        if let Some(domains) = plan.shard_sources.get(&1) {
            assert_eq!(domains[0], 2); // Shard 1 is on domain 2, should prefer it
        }
    }

    #[tokio::test]
    async fn test_plan_read_primary() {
        let registry = create_test_registry();
        let router = GeoRouter::new(registry, 1);

        let policy = ErasurePolicy::rs_4_2();
        let mut dist = ShardDistributionInfo::new("bucket", "key", &policy);

        // All shards available on all domains
        for i in 0..6 {
            for domain_id in 1..=3 {
                dist.add_location(
                    i as ShardIndex,
                    super::super::shards::ShardLocation {
                        domain_id,
                        node_id: "node1".to_string(),
                        path: format!("/data/shard_{}", i),
                        last_verified: Some(Instant::now()),
                        health: ShardHealth::Healthy,
                    },
                );
            }
        }

        let plan = router
            .plan_read(&dist, ReadPreference::Primary, Some(3))
            .await
            .unwrap();

        // All shards should prefer domain 3 (primary)
        for domains in plan.shard_sources.values() {
            assert_eq!(domains[0], 3);
        }
    }

    #[test]
    fn test_geo_router_stats() {
        let registry = create_test_registry();
        let router = GeoRouter::new(registry, 1);

        router.record_latency(1, 10);
        router.record_latency(2, 20);
        router.record_latency(3, 30);

        let stats = router.stats();
        assert_eq!(stats.domains_tracked, 3);
        assert_eq!(stats.stale_entries, 0);
        assert_eq!(stats.min_latency_ms, 10);
        assert_eq!(stats.max_latency_ms, 30);
        assert_eq!(stats.avg_latency_ms, 20);
    }
}
