//! Scrub metrics collection

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// Scrub daemon metrics
pub struct ScrubMetrics {
    /// Time when daemon started
    start_time: RwLock<Option<Instant>>,

    /// Total scrubs performed
    scrubs_total: AtomicU64,

    /// Light scrubs completed
    light_scrubs: AtomicU64,

    /// Deep scrubs completed
    deep_scrubs: AtomicU64,

    /// Total objects scanned
    objects_scanned: AtomicU64,

    /// Total bytes verified
    bytes_verified: AtomicU64,

    /// Checksum errors detected
    checksum_errors: AtomicU64,

    /// Metadata errors detected
    metadata_errors: AtomicU64,

    /// Objects quarantined
    objects_quarantined: AtomicU64,

    /// Repairs triggered
    repairs_triggered: AtomicU64,

    /// Total scrub time (microseconds)
    scrub_time_us: AtomicU64,
}

impl ScrubMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self {
            start_time: RwLock::new(None),
            scrubs_total: AtomicU64::new(0),
            light_scrubs: AtomicU64::new(0),
            deep_scrubs: AtomicU64::new(0),
            objects_scanned: AtomicU64::new(0),
            bytes_verified: AtomicU64::new(0),
            checksum_errors: AtomicU64::new(0),
            metadata_errors: AtomicU64::new(0),
            objects_quarantined: AtomicU64::new(0),
            repairs_triggered: AtomicU64::new(0),
            scrub_time_us: AtomicU64::new(0),
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

    /// Record scrub start
    pub fn record_scrub_start(&self, is_deep: bool) {
        self.scrubs_total.fetch_add(1, Ordering::Relaxed);
        if is_deep {
            self.deep_scrubs.fetch_add(1, Ordering::Relaxed);
        } else {
            self.light_scrubs.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record scrub complete
    pub fn record_scrub_complete(&self, duration: Duration) {
        self.scrub_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record object scanned
    pub fn record_object_scanned(&self, bytes: u64) {
        self.objects_scanned.fetch_add(1, Ordering::Relaxed);
        self.bytes_verified.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record checksum error
    pub fn record_checksum_error(&self) {
        self.checksum_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record metadata error
    pub fn record_metadata_error(&self) {
        self.metadata_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record object quarantined
    pub fn record_quarantine(&self) {
        self.objects_quarantined.fetch_add(1, Ordering::Relaxed);
    }

    /// Record repair triggered
    pub fn record_repair_triggered(&self) {
        self.repairs_triggered.fetch_add(1, Ordering::Relaxed);
    }

    /// Get uptime
    pub fn uptime(&self) -> Option<Duration> {
        self.start_time.read().as_ref().map(|t: &Instant| t.elapsed())
    }

    /// Get statistics snapshot
    pub fn stats(&self) -> ScrubStats {
        ScrubStats {
            uptime_secs: self.uptime().map(|d| d.as_secs()).unwrap_or(0),
            scrubs_total: self.scrubs_total.load(Ordering::Relaxed),
            light_scrubs: self.light_scrubs.load(Ordering::Relaxed),
            deep_scrubs: self.deep_scrubs.load(Ordering::Relaxed),
            objects_scanned: self.objects_scanned.load(Ordering::Relaxed),
            bytes_verified: self.bytes_verified.load(Ordering::Relaxed),
            checksum_errors: self.checksum_errors.load(Ordering::Relaxed),
            metadata_errors: self.metadata_errors.load(Ordering::Relaxed),
            objects_quarantined: self.objects_quarantined.load(Ordering::Relaxed),
            repairs_triggered: self.repairs_triggered.load(Ordering::Relaxed),
            avg_scrub_time_ms: self.avg_scrub_time_ms(),
        }
    }

    /// Calculate average scrub time in milliseconds
    fn avg_scrub_time_ms(&self) -> f64 {
        let total_scrubs = self.scrubs_total.load(Ordering::Relaxed);
        if total_scrubs == 0 {
            return 0.0;
        }
        let total_us = self.scrub_time_us.load(Ordering::Relaxed);
        (total_us as f64 / total_scrubs as f64) / 1000.0
    }
}

impl Default for ScrubMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics snapshot for the scrubber
#[derive(Debug, Clone, Default)]
pub struct ScrubStats {
    /// Uptime in seconds
    pub uptime_secs: u64,

    /// Total scrubs performed
    pub scrubs_total: u64,

    /// Light scrubs completed
    pub light_scrubs: u64,

    /// Deep scrubs completed
    pub deep_scrubs: u64,

    /// Total objects scanned
    pub objects_scanned: u64,

    /// Total bytes verified
    pub bytes_verified: u64,

    /// Checksum errors detected
    pub checksum_errors: u64,

    /// Metadata errors detected
    pub metadata_errors: u64,

    /// Objects quarantined
    pub objects_quarantined: u64,

    /// Repairs triggered
    pub repairs_triggered: u64,

    /// Average scrub time in milliseconds
    pub avg_scrub_time_ms: f64,
}

impl ScrubStats {
    /// Calculate error rate (errors per 1M objects)
    pub fn error_rate_ppm(&self) -> f64 {
        if self.objects_scanned == 0 {
            return 0.0;
        }
        let total_errors = self.checksum_errors + self.metadata_errors;
        (total_errors as f64 / self.objects_scanned as f64) * 1_000_000.0
    }

    /// Format as human-readable string
    pub fn summary(&self) -> String {
        format!(
            "Scrub Stats:\n\
             - Uptime: {}s\n\
             - Scrubs: {} total ({} light, {} deep)\n\
             - Objects scanned: {}\n\
             - Bytes verified: {}\n\
             - Errors: {} checksum, {} metadata\n\
             - Quarantined: {}, Repairs: {}\n\
             - Avg scrub time: {:.1}ms\n\
             - Error rate: {:.2} ppm",
            self.uptime_secs,
            self.scrubs_total,
            self.light_scrubs,
            self.deep_scrubs,
            self.objects_scanned,
            humanize_bytes(self.bytes_verified),
            self.checksum_errors,
            self.metadata_errors,
            self.objects_quarantined,
            self.repairs_triggered,
            self.avg_scrub_time_ms,
            self.error_rate_ppm(),
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
        let metrics = ScrubMetrics::new();

        metrics.record_start();
        metrics.record_scrub_start(false);
        metrics.record_object_scanned(1024);
        metrics.record_scrub_complete(Duration::from_millis(100));

        let stats = metrics.stats();
        assert_eq!(stats.scrubs_total, 1);
        assert_eq!(stats.light_scrubs, 1);
        assert_eq!(stats.objects_scanned, 1);
        assert_eq!(stats.bytes_verified, 1024);
    }

    #[test]
    fn test_humanize_bytes() {
        assert_eq!(humanize_bytes(500), "500 B");
        assert_eq!(humanize_bytes(1500), "1.46 KB");
        assert_eq!(humanize_bytes(1_500_000), "1.43 MB");
        assert_eq!(humanize_bytes(1_500_000_000), "1.40 GB");
    }
}
