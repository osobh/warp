//! Healer metrics collection

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// Healer daemon metrics
pub struct HealerMetrics {
    /// Time when daemon started
    start_time: RwLock<Option<Instant>>,

    /// Total scans performed
    scans_total: AtomicU64,

    /// Successful scans
    scans_successful: AtomicU64,

    /// Failed scans
    scans_failed: AtomicU64,

    /// Total time spent scanning (microseconds)
    scan_time_us: AtomicU64,

    /// Jobs queued
    jobs_queued: AtomicU64,

    /// Successful repairs
    repairs_successful: AtomicU64,

    /// Failed repairs
    repairs_failed: AtomicU64,

    /// Total repair time (microseconds)
    repair_time_us: AtomicU64,

    /// Bytes transferred during repairs
    bytes_transferred: AtomicU64,
}

impl HealerMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self {
            start_time: RwLock::new(None),
            scans_total: AtomicU64::new(0),
            scans_successful: AtomicU64::new(0),
            scans_failed: AtomicU64::new(0),
            scan_time_us: AtomicU64::new(0),
            jobs_queued: AtomicU64::new(0),
            repairs_successful: AtomicU64::new(0),
            repairs_failed: AtomicU64::new(0),
            repair_time_us: AtomicU64::new(0),
            bytes_transferred: AtomicU64::new(0),
        }
    }

    /// Record daemon start
    pub fn record_start(&self) {
        *self.start_time.write() = Some(Instant::now());
    }

    /// Record daemon stop
    pub fn record_stop(&self) {
        *self.start_time.write() = None;
    }

    /// Record scan start
    pub fn record_scan_start(&self) {
        self.scans_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record scan complete
    pub fn record_scan_complete(&self, duration: Duration) {
        self.scans_successful.fetch_add(1, Ordering::Relaxed);
        self.scan_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record scan error
    pub fn record_scan_error(&self) {
        self.scans_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record job queued
    pub fn record_queued(&self) {
        self.jobs_queued.fetch_add(1, Ordering::Relaxed);
    }

    /// Record successful repair
    pub fn record_repair_success(&self, duration: Duration) {
        self.repairs_successful.fetch_add(1, Ordering::Relaxed);
        self.repair_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record failed repair
    pub fn record_repair_failure(&self) {
        self.repairs_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record bytes transferred
    pub fn record_bytes(&self, bytes: u64) {
        self.bytes_transferred.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Get uptime
    pub fn uptime(&self) -> Option<Duration> {
        self.start_time.read().as_ref().map(|t: &Instant| t.elapsed())
    }

    /// Get statistics snapshot
    pub fn stats(&self) -> HealerStats {
        HealerStats {
            uptime_secs: self.uptime().map(|d| d.as_secs()).unwrap_or(0),
            scans_total: self.scans_total.load(Ordering::Relaxed),
            scans_successful: self.scans_successful.load(Ordering::Relaxed),
            scans_failed: self.scans_failed.load(Ordering::Relaxed),
            avg_scan_time_ms: self.avg_scan_time_ms(),
            jobs_queued: self.jobs_queued.load(Ordering::Relaxed),
            repairs_successful: self.repairs_successful.load(Ordering::Relaxed),
            repairs_failed: self.repairs_failed.load(Ordering::Relaxed),
            avg_repair_time_ms: self.avg_repair_time_ms(),
            bytes_transferred: self.bytes_transferred.load(Ordering::Relaxed),
        }
    }

    /// Calculate average scan time in milliseconds
    fn avg_scan_time_ms(&self) -> f64 {
        let successful = self.scans_successful.load(Ordering::Relaxed);
        if successful == 0 {
            return 0.0;
        }
        let total_us = self.scan_time_us.load(Ordering::Relaxed);
        (total_us as f64 / successful as f64) / 1000.0
    }

    /// Calculate average repair time in milliseconds
    fn avg_repair_time_ms(&self) -> f64 {
        let successful = self.repairs_successful.load(Ordering::Relaxed);
        if successful == 0 {
            return 0.0;
        }
        let total_us = self.repair_time_us.load(Ordering::Relaxed);
        (total_us as f64 / successful as f64) / 1000.0
    }
}

impl Default for HealerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics snapshot for the healer
#[derive(Debug, Clone, Default)]
pub struct HealerStats {
    /// Uptime in seconds
    pub uptime_secs: u64,

    /// Total scans performed
    pub scans_total: u64,

    /// Successful scans
    pub scans_successful: u64,

    /// Failed scans
    pub scans_failed: u64,

    /// Average scan time in milliseconds
    pub avg_scan_time_ms: f64,

    /// Total jobs queued
    pub jobs_queued: u64,

    /// Successful repairs
    pub repairs_successful: u64,

    /// Failed repairs
    pub repairs_failed: u64,

    /// Average repair time in milliseconds
    pub avg_repair_time_ms: f64,

    /// Total bytes transferred
    pub bytes_transferred: u64,
}

impl HealerStats {
    /// Calculate repair success rate
    pub fn repair_success_rate(&self) -> f64 {
        let total = self.repairs_successful + self.repairs_failed;
        if total == 0 {
            return 1.0;
        }
        self.repairs_successful as f64 / total as f64
    }

    /// Calculate scan success rate
    pub fn scan_success_rate(&self) -> f64 {
        if self.scans_total == 0 {
            return 1.0;
        }
        self.scans_successful as f64 / self.scans_total as f64
    }

    /// Format as human-readable string
    pub fn summary(&self) -> String {
        format!(
            "Healer Stats:\n\
             - Uptime: {}s\n\
             - Scans: {} total, {:.1}% success, {:.1}ms avg\n\
             - Repairs: {} success, {} failed ({:.1}% success rate)\n\
             - Avg repair time: {:.1}ms\n\
             - Bytes transferred: {}",
            self.uptime_secs,
            self.scans_total,
            self.scan_success_rate() * 100.0,
            self.avg_scan_time_ms,
            self.repairs_successful,
            self.repairs_failed,
            self.repair_success_rate() * 100.0,
            self.avg_repair_time_ms,
            humanize_bytes(self.bytes_transferred),
        )
    }
}

/// Format bytes as human-readable string
fn humanize_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics() {
        let metrics = HealerMetrics::new();

        metrics.record_start();
        metrics.record_scan_start();
        metrics.record_scan_complete(Duration::from_millis(100));
        metrics.record_queued();
        metrics.record_repair_success(Duration::from_millis(500));

        let stats = metrics.stats();
        assert_eq!(stats.scans_total, 1);
        assert_eq!(stats.scans_successful, 1);
        assert_eq!(stats.repairs_successful, 1);
    }

    #[test]
    fn test_humanize_bytes() {
        assert_eq!(humanize_bytes(500), "500 B");
        assert_eq!(humanize_bytes(1500), "1.46 KB");
        assert_eq!(humanize_bytes(1_500_000), "1.43 MB");
        assert_eq!(humanize_bytes(1_500_000_000), "1.40 GB");
    }
}
