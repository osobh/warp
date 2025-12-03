//! Statistics tracking for streaming pipeline
//!
//! This module provides real-time statistics collection for monitoring
//! pipeline performance, latency, and throughput.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Statistics for a single pipeline stage
#[derive(Debug, Default)]
pub struct StageStats {
    /// Total chunks processed
    chunks_processed: AtomicU64,
    /// Total bytes processed
    bytes_processed: AtomicU64,
    /// Sum of latencies in microseconds (for averaging)
    total_latency_us: AtomicU64,
    /// Maximum latency observed in microseconds
    max_latency_us: AtomicU64,
    /// Minimum latency observed in microseconds
    min_latency_us: AtomicU64,
    /// Number of timeouts
    timeouts: AtomicU64,
}

impl StageStats {
    /// Create new stage statistics
    pub fn new() -> Self {
        Self {
            min_latency_us: AtomicU64::new(u64::MAX),
            ..Default::default()
        }
    }

    /// Record a chunk processing event
    pub fn record(&self, bytes: usize, latency: Duration) {
        let latency_us = latency.as_micros() as u64;

        self.chunks_processed.fetch_add(1, Ordering::Relaxed);
        self.bytes_processed.fetch_add(bytes as u64, Ordering::Relaxed);
        self.total_latency_us.fetch_add(latency_us, Ordering::Relaxed);

        // Update max latency
        self.max_latency_us.fetch_max(latency_us, Ordering::Relaxed);

        // Update min latency
        self.min_latency_us.fetch_min(latency_us, Ordering::Relaxed);
    }

    /// Record a timeout event
    pub fn record_timeout(&self) {
        self.timeouts.fetch_add(1, Ordering::Relaxed);
    }

    /// Get number of chunks processed
    pub fn chunks(&self) -> u64 {
        self.chunks_processed.load(Ordering::Relaxed)
    }

    /// Get bytes processed
    pub fn bytes(&self) -> u64 {
        self.bytes_processed.load(Ordering::Relaxed)
    }

    /// Get average latency
    pub fn avg_latency(&self) -> Duration {
        let chunks = self.chunks();
        if chunks == 0 {
            return Duration::ZERO;
        }
        let total_us = self.total_latency_us.load(Ordering::Relaxed);
        Duration::from_micros(total_us / chunks)
    }

    /// Get maximum latency
    pub fn max_latency(&self) -> Duration {
        Duration::from_micros(self.max_latency_us.load(Ordering::Relaxed))
    }

    /// Get minimum latency
    pub fn min_latency(&self) -> Duration {
        let min = self.min_latency_us.load(Ordering::Relaxed);
        if min == u64::MAX {
            Duration::ZERO
        } else {
            Duration::from_micros(min)
        }
    }

    /// Get number of timeouts
    pub fn timeouts(&self) -> u64 {
        self.timeouts.load(Ordering::Relaxed)
    }
}

/// Aggregated pipeline statistics
#[derive(Debug)]
pub struct PipelineStats {
    /// Start time of the pipeline
    start_time: Instant,
    /// Input stage stats (reading/chunking)
    pub input: StageStats,
    /// Process stage stats (encrypt/compress)
    pub process: StageStats,
    /// Output stage stats (writing)
    pub output: StageStats,
    /// Backpressure events
    backpressure_events: AtomicU64,
    /// Total chunks completed end-to-end
    completed_chunks: AtomicU64,
}

impl Default for PipelineStats {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineStats {
    /// Create new pipeline statistics
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            input: StageStats::new(),
            process: StageStats::new(),
            output: StageStats::new(),
            backpressure_events: AtomicU64::new(0),
            completed_chunks: AtomicU64::new(0),
        }
    }

    /// Record a backpressure event
    pub fn record_backpressure(&self) {
        self.backpressure_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a completed chunk (passed through all stages)
    pub fn record_completed(&self) {
        self.completed_chunks.fetch_add(1, Ordering::Relaxed);
    }

    /// Get elapsed time since pipeline start
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get total bytes processed
    pub fn total_bytes(&self) -> u64 {
        self.output.bytes()
    }

    /// Get throughput in bytes per second
    pub fn throughput_bps(&self) -> f64 {
        let elapsed = self.elapsed().as_secs_f64();
        if elapsed == 0.0 {
            return 0.0;
        }
        self.total_bytes() as f64 / elapsed
    }

    /// Get throughput in GB/s
    pub fn throughput_gbps(&self) -> f64 {
        self.throughput_bps() / (1024.0 * 1024.0 * 1024.0)
    }

    /// Get number of backpressure events
    pub fn backpressure_count(&self) -> u64 {
        self.backpressure_events.load(Ordering::Relaxed)
    }

    /// Get number of completed chunks
    pub fn completed_count(&self) -> u64 {
        self.completed_chunks.load(Ordering::Relaxed)
    }

    /// Get end-to-end average latency (all stages combined)
    pub fn end_to_end_latency(&self) -> Duration {
        self.input.avg_latency() + self.process.avg_latency() + self.output.avg_latency()
    }

    /// Check if latency target is being met
    pub fn meets_latency_target(&self, target: Duration) -> bool {
        self.end_to_end_latency() <= target
    }

    /// Generate a summary report
    pub fn summary(&self) -> StatsSummary {
        StatsSummary {
            elapsed: self.elapsed(),
            total_bytes: self.total_bytes(),
            throughput_gbps: self.throughput_gbps(),
            completed_chunks: self.completed_count(),
            input_avg_latency: self.input.avg_latency(),
            process_avg_latency: self.process.avg_latency(),
            output_avg_latency: self.output.avg_latency(),
            end_to_end_latency: self.end_to_end_latency(),
            backpressure_events: self.backpressure_count(),
            timeouts: self.input.timeouts() + self.process.timeouts() + self.output.timeouts(),
        }
    }
}

/// Summary of pipeline statistics
#[derive(Debug, Clone)]
pub struct StatsSummary {
    /// Total elapsed time
    pub elapsed: Duration,
    /// Total bytes processed
    pub total_bytes: u64,
    /// Throughput in GB/s
    pub throughput_gbps: f64,
    /// Number of completed chunks
    pub completed_chunks: u64,
    /// Average input stage latency
    pub input_avg_latency: Duration,
    /// Average process stage latency
    pub process_avg_latency: Duration,
    /// Average output stage latency
    pub output_avg_latency: Duration,
    /// End-to-end latency
    pub end_to_end_latency: Duration,
    /// Number of backpressure events
    pub backpressure_events: u64,
    /// Number of timeouts across all stages
    pub timeouts: u64,
}

impl std::fmt::Display for StatsSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Pipeline Statistics:")?;
        writeln!(f, "  Elapsed: {:?}", self.elapsed)?;
        writeln!(f, "  Total bytes: {} ({:.2} GB)",
                 self.total_bytes, self.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0))?;
        writeln!(f, "  Throughput: {:.2} GB/s", self.throughput_gbps)?;
        writeln!(f, "  Completed chunks: {}", self.completed_chunks)?;
        writeln!(f, "  Latency (avg):")?;
        writeln!(f, "    Input:   {:?}", self.input_avg_latency)?;
        writeln!(f, "    Process: {:?}", self.process_avg_latency)?;
        writeln!(f, "    Output:  {:?}", self.output_avg_latency)?;
        writeln!(f, "    End-to-end: {:?}", self.end_to_end_latency)?;
        writeln!(f, "  Backpressure events: {}", self.backpressure_events)?;
        writeln!(f, "  Timeouts: {}", self.timeouts)?;
        Ok(())
    }
}

/// Shared statistics handle for use across async tasks
pub type SharedStats = Arc<PipelineStats>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_stats_record() {
        let stats = StageStats::new();

        stats.record(1024, Duration::from_micros(100));
        stats.record(1024, Duration::from_micros(200));

        assert_eq!(stats.chunks(), 2);
        assert_eq!(stats.bytes(), 2048);
        assert_eq!(stats.avg_latency(), Duration::from_micros(150));
        assert_eq!(stats.max_latency(), Duration::from_micros(200));
        assert_eq!(stats.min_latency(), Duration::from_micros(100));
    }

    #[test]
    fn test_stage_stats_timeout() {
        let stats = StageStats::new();

        stats.record_timeout();
        stats.record_timeout();

        assert_eq!(stats.timeouts(), 2);
    }

    #[test]
    fn test_stage_stats_empty() {
        let stats = StageStats::new();

        assert_eq!(stats.chunks(), 0);
        assert_eq!(stats.avg_latency(), Duration::ZERO);
        assert_eq!(stats.min_latency(), Duration::ZERO);
    }

    #[test]
    fn test_pipeline_stats_new() {
        let stats = PipelineStats::new();

        assert_eq!(stats.total_bytes(), 0);
        assert_eq!(stats.completed_count(), 0);
        assert_eq!(stats.backpressure_count(), 0);
    }

    #[test]
    fn test_pipeline_stats_recording() {
        let stats = PipelineStats::new();

        stats.input.record(1024, Duration::from_micros(50));
        stats.process.record(1024, Duration::from_micros(100));
        stats.output.record(1024, Duration::from_micros(50));
        stats.record_completed();

        assert_eq!(stats.completed_count(), 1);
        assert_eq!(stats.total_bytes(), 1024);
    }

    #[test]
    fn test_pipeline_stats_backpressure() {
        let stats = PipelineStats::new();

        stats.record_backpressure();
        stats.record_backpressure();

        assert_eq!(stats.backpressure_count(), 2);
    }

    #[test]
    fn test_pipeline_stats_end_to_end_latency() {
        let stats = PipelineStats::new();

        stats.input.record(1024, Duration::from_millis(1));
        stats.process.record(1024, Duration::from_millis(2));
        stats.output.record(1024, Duration::from_millis(1));

        assert_eq!(stats.end_to_end_latency(), Duration::from_millis(4));
    }

    #[test]
    fn test_pipeline_stats_meets_latency_target() {
        let stats = PipelineStats::new();

        stats.input.record(1024, Duration::from_millis(1));
        stats.process.record(1024, Duration::from_millis(2));
        stats.output.record(1024, Duration::from_millis(1));

        assert!(stats.meets_latency_target(Duration::from_millis(5)));
        assert!(!stats.meets_latency_target(Duration::from_millis(3)));
    }

    #[test]
    fn test_stats_summary() {
        let stats = PipelineStats::new();

        stats.input.record(1024, Duration::from_micros(100));
        stats.process.record(1024, Duration::from_micros(200));
        stats.output.record(1024, Duration::from_micros(100));
        stats.record_completed();

        let summary = stats.summary();

        assert_eq!(summary.completed_chunks, 1);
        assert_eq!(summary.total_bytes, 1024);
        assert_eq!(summary.end_to_end_latency, Duration::from_micros(400));
    }

    #[test]
    fn test_shared_stats() {
        let stats: SharedStats = Arc::new(PipelineStats::new());

        let stats_clone = Arc::clone(&stats);
        stats_clone.input.record(1024, Duration::from_micros(100));

        assert_eq!(stats.input.chunks(), 1);
    }
}
