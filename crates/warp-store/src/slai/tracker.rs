//! Access pattern tracking for SLAI-driven placement

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Access pattern type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessPattern {
    /// Sequential reads (batch data)
    Sequential,
    /// Random reads (inference, lookups)
    Random,
    /// Repeated reads (model weights)
    Repeated,
    /// Write-once read-many (checkpoints)
    WriteOnceReadMany,
    /// Write-heavy (logging, metrics)
    WriteHeavy,
    /// Unknown pattern
    Unknown,
}

impl Default for AccessPattern {
    /// Returns the default access pattern (Unknown)
    fn default() -> Self {
        Self::Unknown
    }
}

/// Access operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessOp {
    /// Read operation
    Read,
    /// Write operation
    Write,
    /// Delete operation
    Delete,
    /// List operation
    List,
}

/// Single access record
#[derive(Debug, Clone)]
struct AccessRecord {
    /// Object key
    key: String,
    /// Operation type
    op: AccessOp,
    /// Access timestamp
    timestamp: Instant,
    /// Size in bytes (if applicable)
    size: Option<u64>,
    /// Latency in microseconds
    latency_us: Option<u64>,
}

/// Statistics for a single object
#[derive(Debug, Clone, Default)]
pub struct ObjectStats {
    /// Read count
    pub reads: u64,
    /// Write count
    pub writes: u64,
    /// Total bytes read
    pub bytes_read: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// Average read latency (microseconds)
    pub avg_read_latency_us: u64,
    /// Average write latency (microseconds)
    pub avg_write_latency_us: u64,
    /// First access time
    pub first_access: Option<Instant>,
    /// Last access time
    pub last_access: Option<Instant>,
    /// Detected access pattern
    pub pattern: AccessPattern,
}

/// Global access statistics
#[derive(Debug, Clone, Default)]
pub struct AccessStats {
    /// Total reads
    pub total_reads: u64,
    /// Total writes
    pub total_writes: u64,
    /// Total bytes transferred
    pub total_bytes: u64,
    /// Unique objects accessed
    pub unique_objects: usize,
    /// Hot objects (frequently accessed)
    pub hot_objects: usize,
    /// Cold objects (rarely accessed)
    pub cold_objects: usize,
    /// Pattern distribution
    pub pattern_counts: HashMap<AccessPattern, usize>,
}

/// Access tracker for learning data access patterns
pub struct AccessTracker {
    /// Per-object statistics
    object_stats: DashMap<String, ObjectStats>,
    /// Recent access history
    access_history: RwLock<VecDeque<AccessRecord>>,
    /// Global statistics
    global_stats: RwLock<AccessStats>,
    /// Hot object threshold (accesses per minute)
    hot_threshold: u64,
    /// Maximum history size
    max_history: usize,
    /// Pattern detection window (records to analyze)
    pattern_window: usize,
}

impl AccessTracker {
    /// Create a new access tracker
    pub fn new() -> Self {
        Self {
            object_stats: DashMap::new(),
            access_history: RwLock::new(VecDeque::with_capacity(10000)),
            global_stats: RwLock::new(AccessStats::default()),
            hot_threshold: 10, // 10 accesses per minute = hot
            max_history: 10000,
            pattern_window: 100,
        }
    }

    /// Set hot object threshold
    pub fn with_hot_threshold(mut self, threshold: u64) -> Self {
        self.hot_threshold = threshold;
        self
    }

    /// Record an access
    pub fn record(&self, key: &str, op: AccessOp, size: Option<u64>, latency_us: Option<u64>) {
        let now = Instant::now();

        // Record in history
        {
            let mut history = self.access_history.write();
            history.push_back(AccessRecord {
                key: key.to_string(),
                op,
                timestamp: now,
                size,
                latency_us,
            });
            if history.len() > self.max_history {
                history.pop_front();
            }
        }

        // Update object stats
        self.object_stats
            .entry(key.to_string())
            .and_modify(|stats| {
                match op {
                    AccessOp::Read => {
                        stats.reads += 1;
                        if let Some(s) = size {
                            stats.bytes_read += s;
                        }
                        if let Some(lat) = latency_us {
                            let total = stats.avg_read_latency_us * (stats.reads - 1) + lat;
                            stats.avg_read_latency_us = total / stats.reads;
                        }
                    }
                    AccessOp::Write => {
                        stats.writes += 1;
                        if let Some(s) = size {
                            stats.bytes_written += s;
                        }
                        if let Some(lat) = latency_us {
                            let total = stats.avg_write_latency_us * (stats.writes - 1) + lat;
                            stats.avg_write_latency_us = total / stats.writes;
                        }
                    }
                    _ => {}
                }
                stats.last_access = Some(now);
            })
            .or_insert_with(|| {
                let mut stats = ObjectStats::default();
                match op {
                    AccessOp::Read => {
                        stats.reads = 1;
                        if let Some(s) = size {
                            stats.bytes_read = s;
                        }
                        if let Some(lat) = latency_us {
                            stats.avg_read_latency_us = lat;
                        }
                    }
                    AccessOp::Write => {
                        stats.writes = 1;
                        if let Some(s) = size {
                            stats.bytes_written = s;
                        }
                        if let Some(lat) = latency_us {
                            stats.avg_write_latency_us = lat;
                        }
                    }
                    _ => {}
                }
                stats.first_access = Some(now);
                stats.last_access = Some(now);
                stats
            });

        // Update global stats
        {
            let mut global = self.global_stats.write();
            match op {
                AccessOp::Read => global.total_reads += 1,
                AccessOp::Write => global.total_writes += 1,
                _ => {}
            }
            if let Some(s) = size {
                global.total_bytes += s;
            }
            global.unique_objects = self.object_stats.len();
        }
    }

    /// Get statistics for an object
    pub fn get_object_stats(&self, key: &str) -> Option<ObjectStats> {
        self.object_stats.get(key).map(|s| s.clone())
    }

    /// Get global statistics
    pub fn get_stats(&self) -> AccessStats {
        let mut stats = self.global_stats.read().clone();
        stats.unique_objects = self.object_stats.len();

        // Count hot/cold objects
        let now = Instant::now();
        let mut hot = 0;
        let mut cold = 0;

        for entry in self.object_stats.iter() {
            let stats = entry.value();
            if let Some(last) = stats.last_access {
                let elapsed_mins = now.duration_since(last).as_secs() / 60 + 1;
                let access_rate = (stats.reads + stats.writes) / elapsed_mins;
                if access_rate >= self.hot_threshold {
                    hot += 1;
                } else if access_rate == 0 {
                    cold += 1;
                }
            }
        }

        stats.hot_objects = hot;
        stats.cold_objects = cold;
        stats
    }

    /// Detect access pattern for an object
    pub fn detect_pattern(&self, key: &str) -> AccessPattern {
        let stats = match self.get_object_stats(key) {
            Some(s) => s,
            None => return AccessPattern::Unknown,
        };

        // Analyze read/write ratio and patterns
        let total_ops = stats.reads + stats.writes;
        if total_ops == 0 {
            return AccessPattern::Unknown;
        }

        let read_ratio = stats.reads as f64 / total_ops as f64;

        // Write-once read-many: few writes, many reads
        if stats.writes <= 2 && stats.reads > 10 {
            return AccessPattern::WriteOnceReadMany;
        }

        // Write-heavy: mostly writes
        if read_ratio < 0.3 {
            return AccessPattern::WriteHeavy;
        }

        // Repeated: same object read many times
        if stats.reads > 100 {
            return AccessPattern::Repeated;
        }

        // Check for sequential pattern from history
        if self.is_sequential_access(key) {
            return AccessPattern::Sequential;
        }

        // Default to random
        AccessPattern::Random
    }

    /// Check if accesses to this key are part of a sequential pattern
    fn is_sequential_access(&self, _key: &str) -> bool {
        let history = self.access_history.read();

        // Get recent accesses around this key
        let recent: Vec<&AccessRecord> = history
            .iter()
            .rev()
            .take(self.pattern_window)
            .filter(|r| r.op == AccessOp::Read)
            .collect();

        if recent.len() < 5 {
            return false;
        }

        // Check if keys have sequential numbering
        let keys: Vec<&str> = recent.iter().map(|r| r.key.as_str()).collect();

        // Simple check: see if keys differ only in numbers
        let mut numeric_diffs = 0;
        for window in keys.windows(2) {
            if let (Some(n1), Some(n2)) = (
                extract_trailing_number(window[0]),
                extract_trailing_number(window[1]),
            ) {
                if (n1 as i64 - n2 as i64).abs() <= 5 {
                    numeric_diffs += 1;
                }
            }
        }

        numeric_diffs as f64 / (keys.len() - 1) as f64 > 0.5
    }

    /// Get hot objects (frequently accessed)
    pub fn get_hot_objects(&self, limit: usize) -> Vec<(String, ObjectStats)> {
        let now = Instant::now();
        let mut objects: Vec<_> = self
            .object_stats
            .iter()
            .map(|entry| {
                let key = entry.key().clone();
                let stats = entry.value().clone();
                let elapsed_mins = stats
                    .last_access
                    .map(|t| now.duration_since(t).as_secs() / 60 + 1)
                    .unwrap_or(1);
                let rate = (stats.reads + stats.writes) / elapsed_mins;
                (key, stats, rate)
            })
            .collect();

        objects.sort_by(|a, b| b.2.cmp(&a.2));
        objects
            .into_iter()
            .take(limit)
            .map(|(k, s, _)| (k, s))
            .collect()
    }

    /// Get cold objects (rarely accessed)
    pub fn get_cold_objects(&self, older_than: Duration) -> Vec<String> {
        let now = Instant::now();
        self.object_stats
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .last_access
                    .map(|t| now.duration_since(t) > older_than)
                    .unwrap_or(true)
            })
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get objects that should be prefetched based on patterns
    pub fn get_prefetch_candidates(&self, current_key: &str, limit: usize) -> Vec<String> {
        let history = self.access_history.read();

        // Find position of current key in history
        let mut candidates: HashMap<String, u32> = HashMap::new();

        for (i, record) in history.iter().enumerate() {
            if record.key == current_key {
                // Look at what was accessed after this key historically
                for j in (i + 1)..std::cmp::min(i + 10, history.len()) {
                    let next_key = &history.iter().nth(j).map(|r| r.key.as_str());
                    if let Some(nk) = next_key {
                        *candidates.entry(nk.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Sort by frequency
        let mut sorted: Vec<_> = candidates.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        sorted.into_iter().take(limit).map(|(k, _)| k).collect()
    }

    /// Clear statistics for an object
    pub fn clear_object(&self, key: &str) {
        self.object_stats.remove(key);
    }

    /// Reset all statistics
    pub fn reset(&self) {
        self.object_stats.clear();
        self.access_history.write().clear();
        *self.global_stats.write() = AccessStats::default();
    }
}

impl Default for AccessTracker {
    /// Creates a new AccessTracker with default settings
    fn default() -> Self {
        Self::new()
    }
}

/// Extract trailing number from a string
fn extract_trailing_number(s: &str) -> Option<u64> {
    let num_str: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    if num_str.is_empty() {
        None
    } else {
        num_str.chars().rev().collect::<String>().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_tracker_creation() {
        let tracker = AccessTracker::new();
        let stats = tracker.get_stats();
        assert_eq!(stats.total_reads, 0);
        assert_eq!(stats.total_writes, 0);
    }

    #[test]
    fn test_record_access() {
        let tracker = AccessTracker::new();

        tracker.record("test/file.bin", AccessOp::Read, Some(1024), Some(100));
        tracker.record("test/file.bin", AccessOp::Read, Some(1024), Some(150));
        tracker.record("test/file.bin", AccessOp::Write, Some(2048), Some(200));

        let stats = tracker.get_object_stats("test/file.bin").unwrap();
        assert_eq!(stats.reads, 2);
        assert_eq!(stats.writes, 1);
        assert_eq!(stats.bytes_read, 2048);
        assert_eq!(stats.bytes_written, 2048);
    }

    #[test]
    fn test_global_stats() {
        let tracker = AccessTracker::new();

        tracker.record("file1", AccessOp::Read, Some(100), None);
        tracker.record("file2", AccessOp::Write, Some(200), None);
        tracker.record("file3", AccessOp::Read, Some(300), None);

        let stats = tracker.get_stats();
        assert_eq!(stats.total_reads, 2);
        assert_eq!(stats.total_writes, 1);
        assert_eq!(stats.unique_objects, 3);
    }

    #[test]
    fn test_pattern_detection_worm() {
        let tracker = AccessTracker::new();

        // One write, many reads = Write-Once Read-Many
        tracker.record("model.bin", AccessOp::Write, Some(1000), None);
        for _ in 0..20 {
            tracker.record("model.bin", AccessOp::Read, Some(1000), None);
        }

        let pattern = tracker.detect_pattern("model.bin");
        assert_eq!(pattern, AccessPattern::WriteOnceReadMany);
    }

    #[test]
    fn test_pattern_detection_write_heavy() {
        let tracker = AccessTracker::new();

        // Many writes, few reads = Write-Heavy
        for _ in 0..20 {
            tracker.record("log.txt", AccessOp::Write, Some(100), None);
        }
        tracker.record("log.txt", AccessOp::Read, Some(100), None);

        let pattern = tracker.detect_pattern("log.txt");
        assert_eq!(pattern, AccessPattern::WriteHeavy);
    }

    #[test]
    fn test_extract_trailing_number() {
        assert_eq!(extract_trailing_number("batch_123"), Some(123));
        assert_eq!(extract_trailing_number("data/train/batch_007.bin"), Some(7));
        assert_eq!(extract_trailing_number("no_number"), None);
    }

    #[test]
    fn test_hot_objects() {
        let tracker = AccessTracker::new();

        // Create a "hot" object with many accesses
        for _ in 0..100 {
            tracker.record("hot_file", AccessOp::Read, None, None);
        }

        // Create a "cold" object with few accesses
        tracker.record("cold_file", AccessOp::Read, None, None);

        let hot = tracker.get_hot_objects(10);
        assert!(!hot.is_empty());
        assert_eq!(hot[0].0, "hot_file");
    }
}
