//! Storage metrics and telemetry
//!
//! Provides observability into warp-store operations:
//! - Request latencies (put, get, delete)
//! - Throughput (bytes/sec)
//! - Error rates
//! - Cache hit rates
//! - Shard health statistics
//!
//! # Example
//!
//! ```ignore
//! use warp_store::metrics::{MetricsCollector, MetricsSnapshot};
//!
//! let collector = MetricsCollector::new();
//!
//! // Record operations
//! collector.record_get(1024, Duration::from_micros(50));
//! collector.record_put(4096, Duration::from_millis(2));
//!
//! // Get snapshot
//! let snapshot = collector.snapshot();
//! println!("Avg get latency: {:?}", snapshot.get_latency_avg);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Metrics collector for storage operations
#[derive(Debug)]
pub struct MetricsCollector {
    // Operation counts
    get_count: AtomicU64,
    put_count: AtomicU64,
    delete_count: AtomicU64,
    list_count: AtomicU64,
    head_count: AtomicU64,

    // Error counts
    get_errors: AtomicU64,
    put_errors: AtomicU64,
    delete_errors: AtomicU64,

    // Latency tracking (microseconds, cumulative)
    get_latency_total_us: AtomicU64,
    put_latency_total_us: AtomicU64,
    delete_latency_total_us: AtomicU64,

    // Latency min/max (microseconds)
    get_latency_min_us: AtomicU64,
    get_latency_max_us: AtomicU64,
    put_latency_min_us: AtomicU64,
    put_latency_max_us: AtomicU64,

    // Throughput (bytes)
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,

    // Cache stats
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,

    // Shard stats
    shards_healthy: AtomicU64,
    shards_degraded: AtomicU64,
    shards_missing: AtomicU64,
    shards_repaired: AtomicU64,

    // Erasure coding
    encode_count: AtomicU64,
    decode_count: AtomicU64,
    recovery_count: AtomicU64,

    // Erasure coding - shard level metrics
    shard_sent_count: AtomicU64,
    shard_recv_count: AtomicU64,
    shard_loss_count: AtomicU64,
    decode_success_count: AtomicU64,
    decode_failure_count: AtomicU64,
    recovery_latency_total_us: AtomicU64,

    // Cross-domain
    remote_reads: AtomicU64,
    remote_writes: AtomicU64,
    tunnel_bytes_sent: AtomicU64,
    tunnel_bytes_recv: AtomicU64,

    // Start time
    start_time: Instant,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            get_count: AtomicU64::new(0),
            put_count: AtomicU64::new(0),
            delete_count: AtomicU64::new(0),
            list_count: AtomicU64::new(0),
            head_count: AtomicU64::new(0),
            get_errors: AtomicU64::new(0),
            put_errors: AtomicU64::new(0),
            delete_errors: AtomicU64::new(0),
            get_latency_total_us: AtomicU64::new(0),
            put_latency_total_us: AtomicU64::new(0),
            delete_latency_total_us: AtomicU64::new(0),
            get_latency_min_us: AtomicU64::new(u64::MAX),
            get_latency_max_us: AtomicU64::new(0),
            put_latency_min_us: AtomicU64::new(u64::MAX),
            put_latency_max_us: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            shards_healthy: AtomicU64::new(0),
            shards_degraded: AtomicU64::new(0),
            shards_missing: AtomicU64::new(0),
            shards_repaired: AtomicU64::new(0),
            encode_count: AtomicU64::new(0),
            decode_count: AtomicU64::new(0),
            recovery_count: AtomicU64::new(0),
            shard_sent_count: AtomicU64::new(0),
            shard_recv_count: AtomicU64::new(0),
            shard_loss_count: AtomicU64::new(0),
            decode_success_count: AtomicU64::new(0),
            decode_failure_count: AtomicU64::new(0),
            recovery_latency_total_us: AtomicU64::new(0),
            remote_reads: AtomicU64::new(0),
            remote_writes: AtomicU64::new(0),
            tunnel_bytes_sent: AtomicU64::new(0),
            tunnel_bytes_recv: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record a successful GET operation
    pub fn record_get(&self, bytes: u64, latency: Duration) {
        self.get_count.fetch_add(1, Ordering::Relaxed);
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);

        let us = latency.as_micros() as u64;
        self.get_latency_total_us.fetch_add(us, Ordering::Relaxed);
        self.update_min(&self.get_latency_min_us, us);
        self.update_max(&self.get_latency_max_us, us);
    }

    /// Record a successful PUT operation
    pub fn record_put(&self, bytes: u64, latency: Duration) {
        self.put_count.fetch_add(1, Ordering::Relaxed);
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);

        let us = latency.as_micros() as u64;
        self.put_latency_total_us.fetch_add(us, Ordering::Relaxed);
        self.update_min(&self.put_latency_min_us, us);
        self.update_max(&self.put_latency_max_us, us);
    }

    /// Record a successful DELETE operation
    pub fn record_delete(&self, latency: Duration) {
        self.delete_count.fetch_add(1, Ordering::Relaxed);
        let us = latency.as_micros() as u64;
        self.delete_latency_total_us.fetch_add(us, Ordering::Relaxed);
    }

    /// Record a LIST operation
    pub fn record_list(&self) {
        self.list_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a HEAD operation
    pub fn record_head(&self) {
        self.head_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a GET error
    pub fn record_get_error(&self) {
        self.get_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a PUT error
    pub fn record_put_error(&self) {
        self.put_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a DELETE error
    pub fn record_delete_error(&self) {
        self.delete_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cache hit
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cache miss
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Update shard health counts
    pub fn update_shard_health(&self, healthy: u64, degraded: u64, missing: u64) {
        self.shards_healthy.store(healthy, Ordering::Relaxed);
        self.shards_degraded.store(degraded, Ordering::Relaxed);
        self.shards_missing.store(missing, Ordering::Relaxed);
    }

    /// Record shards repaired
    pub fn record_shards_repaired(&self, count: u64) {
        self.shards_repaired.fetch_add(count, Ordering::Relaxed);
    }

    /// Record an encode operation
    pub fn record_encode(&self) {
        self.encode_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a decode operation
    pub fn record_decode(&self) {
        self.decode_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a recovery (decode with missing shards)
    pub fn record_recovery(&self) {
        self.recovery_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a shard sent
    pub fn record_shard_sent(&self) {
        self.shard_sent_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record shards sent (batch)
    pub fn record_shards_sent(&self, count: u64) {
        self.shard_sent_count.fetch_add(count, Ordering::Relaxed);
    }

    /// Record a shard received
    pub fn record_shard_received(&self) {
        self.shard_recv_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a shard loss (timeout/missing)
    pub fn record_shard_loss(&self) {
        self.shard_loss_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record shards lost (batch)
    pub fn record_shards_lost(&self, count: u64) {
        self.shard_loss_count.fetch_add(count, Ordering::Relaxed);
    }

    /// Record a successful decode
    pub fn record_decode_success(&self) {
        self.decode_success_count.fetch_add(1, Ordering::Relaxed);
        self.decode_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a failed decode
    pub fn record_decode_failure(&self) {
        self.decode_failure_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record recovery latency (decode with missing shards)
    pub fn record_recovery_latency(&self, latency: Duration) {
        self.recovery_latency_total_us.fetch_add(latency.as_micros() as u64, Ordering::Relaxed);
        self.recovery_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a remote read
    pub fn record_remote_read(&self, bytes: u64) {
        self.remote_reads.fetch_add(1, Ordering::Relaxed);
        self.tunnel_bytes_recv.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record a remote write
    pub fn record_remote_write(&self, bytes: u64) {
        self.remote_writes.fetch_add(1, Ordering::Relaxed);
        self.tunnel_bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Get a snapshot of current metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        let uptime = self.start_time.elapsed();
        let get_count = self.get_count.load(Ordering::Relaxed);
        let put_count = self.put_count.load(Ordering::Relaxed);
        let bytes_read = self.bytes_read.load(Ordering::Relaxed);
        let bytes_written = self.bytes_written.load(Ordering::Relaxed);

        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.cache_misses.load(Ordering::Relaxed);
        let cache_total = cache_hits + cache_misses;

        MetricsSnapshot {
            uptime_secs: uptime.as_secs(),

            // Operation counts
            get_count,
            put_count,
            delete_count: self.delete_count.load(Ordering::Relaxed),
            list_count: self.list_count.load(Ordering::Relaxed),
            head_count: self.head_count.load(Ordering::Relaxed),

            // Error counts
            get_errors: self.get_errors.load(Ordering::Relaxed),
            put_errors: self.put_errors.load(Ordering::Relaxed),
            delete_errors: self.delete_errors.load(Ordering::Relaxed),

            // Latencies
            get_latency_avg_us: if get_count > 0 {
                self.get_latency_total_us.load(Ordering::Relaxed) / get_count
            } else {
                0
            },
            get_latency_min_us: {
                let min = self.get_latency_min_us.load(Ordering::Relaxed);
                if min == u64::MAX { 0 } else { min }
            },
            get_latency_max_us: self.get_latency_max_us.load(Ordering::Relaxed),
            put_latency_avg_us: if put_count > 0 {
                self.put_latency_total_us.load(Ordering::Relaxed) / put_count
            } else {
                0
            },
            put_latency_min_us: {
                let min = self.put_latency_min_us.load(Ordering::Relaxed);
                if min == u64::MAX { 0 } else { min }
            },
            put_latency_max_us: self.put_latency_max_us.load(Ordering::Relaxed),

            // Throughput
            bytes_read,
            bytes_written,
            read_throughput_mbps: if uptime.as_secs() > 0 {
                (bytes_read as f64 / 1_000_000.0) / uptime.as_secs_f64()
            } else {
                0.0
            },
            write_throughput_mbps: if uptime.as_secs() > 0 {
                (bytes_written as f64 / 1_000_000.0) / uptime.as_secs_f64()
            } else {
                0.0
            },

            // Cache
            cache_hits,
            cache_misses,
            cache_hit_rate: if cache_total > 0 {
                cache_hits as f64 / cache_total as f64
            } else {
                0.0
            },

            // Shards
            shards_healthy: self.shards_healthy.load(Ordering::Relaxed),
            shards_degraded: self.shards_degraded.load(Ordering::Relaxed),
            shards_missing: self.shards_missing.load(Ordering::Relaxed),
            shards_repaired: self.shards_repaired.load(Ordering::Relaxed),

            // Erasure coding
            encode_count: self.encode_count.load(Ordering::Relaxed),
            decode_count: self.decode_count.load(Ordering::Relaxed),
            recovery_count: self.recovery_count.load(Ordering::Relaxed),

            // Erasure coding - shard level metrics
            shard_sent_count: self.shard_sent_count.load(Ordering::Relaxed),
            shard_recv_count: self.shard_recv_count.load(Ordering::Relaxed),
            shard_loss_count: self.shard_loss_count.load(Ordering::Relaxed),
            decode_success_count: self.decode_success_count.load(Ordering::Relaxed),
            decode_failure_count: self.decode_failure_count.load(Ordering::Relaxed),
            recovery_latency_avg_us: {
                let recovery_count = self.recovery_count.load(Ordering::Relaxed);
                if recovery_count > 0 {
                    self.recovery_latency_total_us.load(Ordering::Relaxed) / recovery_count
                } else {
                    0
                }
            },

            // Cross-domain
            remote_reads: self.remote_reads.load(Ordering::Relaxed),
            remote_writes: self.remote_writes.load(Ordering::Relaxed),
            tunnel_bytes_sent: self.tunnel_bytes_sent.load(Ordering::Relaxed),
            tunnel_bytes_recv: self.tunnel_bytes_recv.load(Ordering::Relaxed),
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.get_count.store(0, Ordering::Relaxed);
        self.put_count.store(0, Ordering::Relaxed);
        self.delete_count.store(0, Ordering::Relaxed);
        self.list_count.store(0, Ordering::Relaxed);
        self.head_count.store(0, Ordering::Relaxed);
        self.get_errors.store(0, Ordering::Relaxed);
        self.put_errors.store(0, Ordering::Relaxed);
        self.delete_errors.store(0, Ordering::Relaxed);
        self.get_latency_total_us.store(0, Ordering::Relaxed);
        self.put_latency_total_us.store(0, Ordering::Relaxed);
        self.delete_latency_total_us.store(0, Ordering::Relaxed);
        self.get_latency_min_us.store(u64::MAX, Ordering::Relaxed);
        self.get_latency_max_us.store(0, Ordering::Relaxed);
        self.put_latency_min_us.store(u64::MAX, Ordering::Relaxed);
        self.put_latency_max_us.store(0, Ordering::Relaxed);
        self.bytes_read.store(0, Ordering::Relaxed);
        self.bytes_written.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.encode_count.store(0, Ordering::Relaxed);
        self.decode_count.store(0, Ordering::Relaxed);
        self.recovery_count.store(0, Ordering::Relaxed);
        self.shard_sent_count.store(0, Ordering::Relaxed);
        self.shard_recv_count.store(0, Ordering::Relaxed);
        self.shard_loss_count.store(0, Ordering::Relaxed);
        self.decode_success_count.store(0, Ordering::Relaxed);
        self.decode_failure_count.store(0, Ordering::Relaxed);
        self.recovery_latency_total_us.store(0, Ordering::Relaxed);
        self.remote_reads.store(0, Ordering::Relaxed);
        self.remote_writes.store(0, Ordering::Relaxed);
        self.tunnel_bytes_sent.store(0, Ordering::Relaxed);
        self.tunnel_bytes_recv.store(0, Ordering::Relaxed);
    }

    fn update_min(&self, atomic: &AtomicU64, value: u64) {
        let mut current = atomic.load(Ordering::Relaxed);
        while value < current {
            match atomic.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }

    fn update_max(&self, atomic: &AtomicU64, value: u64) {
        let mut current = atomic.load(Ordering::Relaxed);
        while value > current {
            match atomic.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }
}

/// Snapshot of metrics at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Uptime in seconds
    pub uptime_secs: u64,

    // Operation counts
    /// Total GET operations
    pub get_count: u64,
    /// Total PUT operations
    pub put_count: u64,
    /// Total DELETE operations
    pub delete_count: u64,
    /// Total LIST operations
    pub list_count: u64,
    /// Total HEAD operations
    pub head_count: u64,

    // Error counts
    /// GET errors
    pub get_errors: u64,
    /// PUT errors
    pub put_errors: u64,
    /// DELETE errors
    pub delete_errors: u64,

    // Latencies (microseconds)
    /// Average GET latency
    pub get_latency_avg_us: u64,
    /// Minimum GET latency
    pub get_latency_min_us: u64,
    /// Maximum GET latency
    pub get_latency_max_us: u64,
    /// Average PUT latency
    pub put_latency_avg_us: u64,
    /// Minimum PUT latency
    pub put_latency_min_us: u64,
    /// Maximum PUT latency
    pub put_latency_max_us: u64,

    // Throughput
    /// Total bytes read
    pub bytes_read: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// Read throughput in MB/s
    pub read_throughput_mbps: f64,
    /// Write throughput in MB/s
    pub write_throughput_mbps: f64,

    // Cache
    /// Cache hits
    pub cache_hits: u64,
    /// Cache misses
    pub cache_misses: u64,
    /// Cache hit rate (0.0 - 1.0)
    pub cache_hit_rate: f64,

    // Shards
    /// Healthy shards
    pub shards_healthy: u64,
    /// Degraded shards
    pub shards_degraded: u64,
    /// Missing shards
    pub shards_missing: u64,
    /// Total shards repaired
    pub shards_repaired: u64,

    // Erasure coding
    /// Encode operations
    pub encode_count: u64,
    /// Decode operations
    pub decode_count: u64,
    /// Recovery operations (decode with missing shards)
    pub recovery_count: u64,

    // Erasure coding - shard level metrics
    /// Shards sent
    pub shard_sent_count: u64,
    /// Shards received
    pub shard_recv_count: u64,
    /// Shards lost
    pub shard_loss_count: u64,
    /// Successful decode operations
    pub decode_success_count: u64,
    /// Failed decode operations
    pub decode_failure_count: u64,
    /// Average recovery latency in microseconds
    pub recovery_latency_avg_us: u64,

    // Cross-domain
    /// Remote read operations
    pub remote_reads: u64,
    /// Remote write operations
    pub remote_writes: u64,
    /// Bytes sent via tunnel
    pub tunnel_bytes_sent: u64,
    /// Bytes received via tunnel
    pub tunnel_bytes_recv: u64,
}

impl MetricsSnapshot {
    /// Get error rate for GET operations
    pub fn get_error_rate(&self) -> f64 {
        let total = self.get_count + self.get_errors;
        if total > 0 {
            self.get_errors as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Get error rate for PUT operations
    pub fn put_error_rate(&self) -> f64 {
        let total = self.put_count + self.put_errors;
        if total > 0 {
            self.put_errors as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Get overall shard health ratio
    pub fn shard_health_ratio(&self) -> f64 {
        let total = self.shards_healthy + self.shards_degraded + self.shards_missing;
        if total > 0 {
            self.shards_healthy as f64 / total as f64
        } else {
            1.0
        }
    }

    /// Get shard loss rate (0.0 - 1.0)
    /// Returns the ratio of lost shards to total shards sent
    pub fn shard_loss_rate(&self) -> f64 {
        let total = self.shard_sent_count + self.shard_loss_count;
        if total > 0 {
            self.shard_loss_count as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Get decode success rate (0.0 - 1.0)
    /// Returns the ratio of successful decodes to total decode attempts
    pub fn decode_success_rate(&self) -> f64 {
        let total = self.decode_success_count + self.decode_failure_count;
        if total > 0 {
            self.decode_success_count as f64 / total as f64
        } else {
            1.0 // No decode attempts = 100% success (no failures)
        }
    }

    /// Check if the system is healthy
    pub fn is_healthy(&self) -> bool {
        // System is healthy if:
        // - Error rates are below 1%
        // - Shard health is above 90%
        // - No missing shards
        self.get_error_rate() < 0.01
            && self.put_error_rate() < 0.01
            && self.shard_health_ratio() >= 0.9
            && self.shards_missing == 0
    }
}

/// Timer for measuring operation latency
pub struct LatencyTimer {
    start: Instant,
}

impl LatencyTimer {
    /// Start a new timer
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }

    /// Get elapsed duration
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new();

        // Record some operations
        collector.record_get(1024, Duration::from_micros(50));
        collector.record_get(2048, Duration::from_micros(100));
        collector.record_put(4096, Duration::from_millis(2));
        collector.record_delete(Duration::from_micros(30));

        let snapshot = collector.snapshot();

        assert_eq!(snapshot.get_count, 2);
        assert_eq!(snapshot.put_count, 1);
        assert_eq!(snapshot.delete_count, 1);
        assert_eq!(snapshot.bytes_read, 3072);
        assert_eq!(snapshot.bytes_written, 4096);
        assert_eq!(snapshot.get_latency_avg_us, 75); // (50 + 100) / 2
        assert_eq!(snapshot.get_latency_min_us, 50);
        assert_eq!(snapshot.get_latency_max_us, 100);
    }

    #[test]
    fn test_error_tracking() {
        let collector = MetricsCollector::new();

        collector.record_get(1024, Duration::from_micros(50));
        collector.record_get_error();
        collector.record_get_error();

        let snapshot = collector.snapshot();

        assert_eq!(snapshot.get_count, 1);
        assert_eq!(snapshot.get_errors, 2);
        assert!((snapshot.get_error_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_cache_stats() {
        let collector = MetricsCollector::new();

        collector.record_cache_hit();
        collector.record_cache_hit();
        collector.record_cache_hit();
        collector.record_cache_miss();

        let snapshot = collector.snapshot();

        assert_eq!(snapshot.cache_hits, 3);
        assert_eq!(snapshot.cache_misses, 1);
        assert!((snapshot.cache_hit_rate - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_health_check() {
        let collector = MetricsCollector::new();

        // Healthy state
        collector.record_get(1024, Duration::from_micros(50));
        collector.update_shard_health(100, 0, 0);

        let snapshot = collector.snapshot();
        assert!(snapshot.is_healthy());

        // Unhealthy: missing shards
        collector.update_shard_health(90, 5, 5);
        let snapshot = collector.snapshot();
        assert!(!snapshot.is_healthy());
    }

    #[test]
    fn test_reset() {
        let collector = MetricsCollector::new();

        collector.record_get(1024, Duration::from_micros(50));
        collector.record_put(2048, Duration::from_millis(1));

        collector.reset();

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.get_count, 0);
        assert_eq!(snapshot.put_count, 0);
        assert_eq!(snapshot.bytes_read, 0);
    }

    #[test]
    fn test_shard_metrics() {
        let collector = MetricsCollector::new();

        // Record shard operations
        collector.record_shards_sent(10);
        collector.record_shard_received();
        collector.record_shard_received();
        collector.record_shard_received();
        collector.record_shard_received();
        collector.record_shard_loss();
        collector.record_shards_lost(2);

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.shard_sent_count, 10);
        assert_eq!(snapshot.shard_recv_count, 4);
        assert_eq!(snapshot.shard_loss_count, 3);
        // Loss rate = 3 / (10 + 3) = 0.2308
        assert!((snapshot.shard_loss_rate() - 0.2308).abs() < 0.01);
    }

    #[test]
    fn test_decode_metrics() {
        let collector = MetricsCollector::new();

        // Record decode operations
        collector.record_decode_success();
        collector.record_decode_success();
        collector.record_decode_success();
        collector.record_decode_failure();

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.decode_success_count, 3);
        assert_eq!(snapshot.decode_failure_count, 1);
        // Success rate = 3 / 4 = 0.75
        assert!((snapshot.decode_success_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_recovery_latency() {
        let collector = MetricsCollector::new();

        // Record recovery operations with latencies
        collector.record_recovery_latency(Duration::from_micros(100));
        collector.record_recovery_latency(Duration::from_micros(200));
        collector.record_recovery_latency(Duration::from_micros(300));

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.recovery_count, 3);
        // Avg = (100 + 200 + 300) / 3 = 200
        assert_eq!(snapshot.recovery_latency_avg_us, 200);
    }
}
