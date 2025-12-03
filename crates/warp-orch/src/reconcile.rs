//! Drift Detection & Reoptimization Triggers
//!
//! This module provides automatic reconciliation capabilities for distributed transfers:
//! - Monitors actual vs expected performance through drift detection
//! - Tracks per-edge performance metrics with EMA smoothing
//! - Evaluates when to trigger reoptimization based on drift and health
//! - Enforces cooldown periods to prevent excessive reoptimization
//!
//! The reconciliation flow:
//! 1. DriftDetector records transfer samples and calculates drift metrics
//! 2. ReoptTrigger identifies specific reasons for reoptimization
//! 3. ReoptEvaluator decides whether and how to reoptimize

use crate::types::TransferId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use warp_sched::{ChunkId, EdgeIdx};

/// Configuration for drift detection
#[derive(Debug, Clone)]
pub struct DriftConfig {
    /// Time window for drift calculation in milliseconds (default 5000)
    pub sample_window_ms: u64,
    /// Minimum samples before calculating drift (default 5)
    pub min_samples: usize,
    /// Threshold for considering drift significant (default 0.2 = 20%)
    pub drift_threshold: f64,
    /// EMA smoothing factor (default 0.3)
    pub ema_alpha: f64,
}

impl DriftConfig {
    /// Create a new DriftConfig with default values
    pub fn new() -> Self {
        Self {
            sample_window_ms: 5000,
            min_samples: 5,
            drift_threshold: 0.2,
            ema_alpha: 0.3,
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.sample_window_ms == 0 {
            return Err("sample_window_ms must be greater than 0".to_string());
        }
        if self.min_samples == 0 {
            return Err("min_samples must be greater than 0".to_string());
        }
        if !(0.0..=1.0).contains(&self.drift_threshold) {
            return Err("drift_threshold must be between 0.0 and 1.0".to_string());
        }
        if !(0.0..=1.0).contains(&self.ema_alpha) {
            return Err("ema_alpha must be between 0.0 and 1.0".to_string());
        }
        Ok(())
    }

    /// Set sample window
    pub fn with_sample_window_ms(mut self, ms: u64) -> Self {
        self.sample_window_ms = ms;
        self
    }

    /// Set minimum samples
    pub fn with_min_samples(mut self, samples: usize) -> Self {
        self.min_samples = samples;
        self
    }

    /// Set drift threshold
    pub fn with_drift_threshold(mut self, threshold: f64) -> Self {
        self.drift_threshold = threshold;
        self
    }

    /// Set EMA alpha
    pub fn with_ema_alpha(mut self, alpha: f64) -> Self {
        self.ema_alpha = alpha;
        self
    }
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics tracking actual vs expected performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftMetrics {
    /// Expected transfer speed in bits per second
    pub expected_speed_bps: u64,
    /// Measured transfer speed in bits per second
    pub actual_speed_bps: u64,
    /// Expected completion time in milliseconds
    pub expected_completion_ms: u64,
    /// Actual elapsed time in milliseconds
    pub actual_elapsed_ms: u64,
    /// Ratio of actual/expected (1.0 = on track)
    pub drift_ratio: f64,
    /// Per-edge drift ratios
    pub edge_drift: HashMap<EdgeIdx, f64>,
}

impl DriftMetrics {
    /// Create new DriftMetrics with zero values
    pub fn new() -> Self {
        Self {
            expected_speed_bps: 0,
            actual_speed_bps: 0,
            expected_completion_ms: 0,
            actual_elapsed_ms: 0,
            drift_ratio: 1.0,
            edge_drift: HashMap::new(),
        }
    }

    /// Check if performance is slower than expected
    pub fn is_slower(&self) -> bool {
        self.drift_ratio > 1.0
    }

    /// Check if performance is faster than expected
    pub fn is_faster(&self) -> bool {
        self.drift_ratio < 1.0
    }
}

impl Default for DriftMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Sample data for drift calculation
#[derive(Debug, Clone)]
struct TransferSample {
    edge_idx: EdgeIdx,
    bytes: u64,
    duration_ms: u64,
    timestamp: Instant,
}

/// Per-edge performance tracking
#[derive(Debug, Clone)]
struct EdgePerformance {
    samples: VecDeque<TransferSample>,
    ema_speed_bps: f64,
    total_bytes: u64,
    total_duration_ms: u64,
}

impl EdgePerformance {
    fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            ema_speed_bps: 0.0,
            total_bytes: 0,
            total_duration_ms: 0,
        }
    }

    fn add_sample(&mut self, sample: TransferSample, alpha: f64) {
        let speed_bps = if sample.duration_ms > 0 {
            (sample.bytes * 8 * 1000) / sample.duration_ms
        } else {
            0
        };

        if self.ema_speed_bps == 0.0 {
            self.ema_speed_bps = speed_bps as f64;
        } else {
            self.ema_speed_bps = alpha * speed_bps as f64 + (1.0 - alpha) * self.ema_speed_bps;
        }

        self.total_bytes += sample.bytes;
        self.total_duration_ms += sample.duration_ms;
        self.samples.push_back(sample);
    }

    fn prune_old_samples(&mut self, window_ms: u64) {
        let now = Instant::now();
        while let Some(sample) = self.samples.front() {
            if now.duration_since(sample.timestamp).as_millis() as u64 > window_ms {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    fn sample_count(&self) -> usize {
        self.samples.len()
    }

    fn current_speed_bps(&self) -> u64 {
        self.ema_speed_bps as u64
    }
}

/// Per-transfer tracking data
#[derive(Debug)]
struct TransferTracking {
    edge_performance: HashMap<EdgeIdx, EdgePerformance>,
    expected_speed_bps: u64,
    expected_completion_ms: u64,
    started_at: Instant,
}

impl TransferTracking {
    fn new(expected_speed_bps: u64, expected_completion_ms: u64) -> Self {
        Self {
            edge_performance: HashMap::new(),
            expected_speed_bps,
            expected_completion_ms,
            started_at: Instant::now(),
        }
    }
}

/// Monitor transfer performance drift
pub struct DriftDetector {
    config: DriftConfig,
    transfers: HashMap<TransferId, TransferTracking>,
}

impl DriftDetector {
    /// Create a new DriftDetector with the given configuration
    pub fn new(config: DriftConfig) -> Self {
        Self {
            config,
            transfers: HashMap::new(),
        }
    }

    /// Record a transfer sample for drift calculation
    pub fn record_sample(
        &mut self,
        transfer_id: TransferId,
        edge_idx: EdgeIdx,
        bytes: u64,
        duration_ms: u64,
    ) {
        let sample = TransferSample {
            edge_idx,
            bytes,
            duration_ms,
            timestamp: Instant::now(),
        };

        let tracking = self
            .transfers
            .entry(transfer_id)
            .or_insert_with(|| TransferTracking::new(0, 0));

        let edge_perf = tracking
            .edge_performance
            .entry(edge_idx)
            .or_insert_with(EdgePerformance::new);

        edge_perf.add_sample(sample, self.config.ema_alpha);
        edge_perf.prune_old_samples(self.config.sample_window_ms);
    }

    /// Set expected performance baseline for a transfer
    pub fn set_baseline(
        &mut self,
        transfer_id: TransferId,
        expected_speed_bps: u64,
        expected_completion_ms: u64,
    ) {
        self.transfers.insert(
            transfer_id,
            TransferTracking::new(expected_speed_bps, expected_completion_ms),
        );
    }

    /// Calculate drift metrics for a transfer
    pub fn calculate_drift(&self, transfer_id: TransferId) -> DriftMetrics {
        let tracking = match self.transfers.get(&transfer_id) {
            Some(t) => t,
            None => return DriftMetrics::new(),
        };

        let mut total_bytes = 0u64;
        let mut total_duration_ms = 0u64;
        let mut edge_drift = HashMap::new();

        for (edge_idx, perf) in &tracking.edge_performance {
            if perf.sample_count() < self.config.min_samples {
                continue;
            }

            total_bytes += perf.total_bytes;
            total_duration_ms += perf.total_duration_ms;

            if tracking.expected_speed_bps > 0 {
                let actual_speed = perf.current_speed_bps();
                let drift = actual_speed as f64 / tracking.expected_speed_bps as f64;
                edge_drift.insert(*edge_idx, drift);
            }
        }

        let actual_speed_bps = if total_duration_ms > 0 {
            (total_bytes * 8 * 1000) / total_duration_ms
        } else {
            0
        };

        let actual_elapsed_ms = tracking.started_at.elapsed().as_millis() as u64;

        let drift_ratio = if tracking.expected_completion_ms > 0 {
            actual_elapsed_ms as f64 / tracking.expected_completion_ms as f64
        } else {
            1.0
        };

        DriftMetrics {
            expected_speed_bps: tracking.expected_speed_bps,
            actual_speed_bps,
            expected_completion_ms: tracking.expected_completion_ms,
            actual_elapsed_ms,
            drift_ratio,
            edge_drift,
        }
    }

    /// Check if a transfer is drifting beyond the threshold
    pub fn is_drifting(&self, transfer_id: TransferId, threshold: f64) -> bool {
        let metrics = self.calculate_drift(transfer_id);
        (metrics.drift_ratio - 1.0).abs() > threshold
    }

    /// Get edges performing below the threshold
    pub fn get_slow_edges(&self, threshold: f64) -> Vec<EdgeIdx> {
        let mut slow_edges = Vec::new();

        for tracking in self.transfers.values() {
            for (edge_idx, perf) in &tracking.edge_performance {
                if perf.sample_count() < self.config.min_samples {
                    continue;
                }

                if tracking.expected_speed_bps > 0 {
                    let actual_speed = perf.current_speed_bps();
                    let ratio = actual_speed as f64 / tracking.expected_speed_bps as f64;
                    if ratio < threshold && !slow_edges.contains(edge_idx) {
                        slow_edges.push(*edge_idx);
                    }
                }
            }
        }

        slow_edges
    }

    /// Get edges performing above the threshold
    pub fn get_fast_edges(&self, threshold: f64) -> Vec<EdgeIdx> {
        let mut fast_edges = Vec::new();

        for tracking in self.transfers.values() {
            for (edge_idx, perf) in &tracking.edge_performance {
                if perf.sample_count() < self.config.min_samples {
                    continue;
                }

                if tracking.expected_speed_bps > 0 {
                    let actual_speed = perf.current_speed_bps();
                    let ratio = actual_speed as f64 / tracking.expected_speed_bps as f64;
                    if ratio > threshold && !fast_edges.contains(edge_idx) {
                        fast_edges.push(*edge_idx);
                    }
                }
            }
        }

        fast_edges
    }

    /// Clear tracking data for a transfer
    pub fn clear(&mut self, transfer_id: TransferId) {
        self.transfers.remove(&transfer_id);
    }

    /// Clear all tracking data
    pub fn clear_all(&mut self) {
        self.transfers.clear();
    }
}

/// Reasons to trigger reoptimization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReoptTrigger {
    /// Drift exceeded acceptable threshold
    DriftExceeded { drift_ratio: f64, threshold: f64 },
    /// Edge health degraded
    EdgeDegraded { edge_idx: EdgeIdx, health_score: f64 },
    /// Edge health recovered
    EdgeRecovered { edge_idx: EdgeIdx, health_score: f64 },
    /// Load imbalance detected
    LoadImbalance {
        overloaded: Vec<EdgeIdx>,
        underloaded: Vec<EdgeIdx>,
    },
    /// Schedule is stale
    ScheduleStale { age_ms: u64, max_age_ms: u64 },
    /// Manual trigger
    Manual,
}

/// Decision on whether/how to reoptimize
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReoptDecision {
    /// No reoptimization needed
    NoAction,
    /// Partial reoptimization for specific chunks/edges
    PartialReopt {
        affected_chunks: Vec<ChunkId>,
        affected_edges: Vec<EdgeIdx>,
    },
    /// Full reoptimization needed
    FullReopt { reason: String },
    /// Wait before deciding
    Pause { duration_ms: u64, reason: String },
}

/// Configuration for reoptimization evaluation
#[derive(Debug, Clone)]
pub struct ReoptConfig {
    /// Minimum drift ratio for partial reoptimization (default 0.3)
    pub min_drift_for_partial: f64,
    /// Minimum drift ratio for full reoptimization (default 0.5)
    pub min_drift_for_full: f64,
    /// Minimum time between reoptimizations in milliseconds (default 10000)
    pub cooldown_ms: u64,
    /// Maximum age before schedule is stale in milliseconds (default 60000)
    pub max_schedule_age_ms: u64,
    /// Minimum health score before triggering (default 0.5)
    pub health_threshold: f64,
}

impl ReoptConfig {
    /// Create a new ReoptConfig with default values
    pub fn new() -> Self {
        Self {
            min_drift_for_partial: 0.3,
            min_drift_for_full: 0.5,
            cooldown_ms: 10000,
            max_schedule_age_ms: 60000,
            health_threshold: 0.5,
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.min_drift_for_partial < 0.0 {
            return Err("min_drift_for_partial must be >= 0.0".to_string());
        }
        if self.min_drift_for_full < self.min_drift_for_partial {
            return Err("min_drift_for_full must be >= min_drift_for_partial".to_string());
        }
        if !(0.0..=1.0).contains(&self.health_threshold) {
            return Err("health_threshold must be between 0.0 and 1.0".to_string());
        }
        Ok(())
    }
}

impl Default for ReoptConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Decide when to trigger reoptimization
pub struct ReoptEvaluator {
    config: ReoptConfig,
    last_reopt: Option<Instant>,
}

impl ReoptEvaluator {
    /// Create a new ReoptEvaluator with the given configuration
    pub fn new(config: ReoptConfig) -> Self {
        Self {
            config,
            last_reopt: None,
        }
    }

    /// Evaluate drift metrics and edge health to make a reoptimization decision
    pub fn evaluate(
        &self,
        drift: &DriftMetrics,
        edge_health: &HashMap<EdgeIdx, f64>,
    ) -> ReoptDecision {
        if let Some(remaining) = self.cooldown_remaining() {
            return ReoptDecision::Pause {
                duration_ms: remaining.as_millis() as u64,
                reason: "Cooldown period active".to_string(),
            };
        }

        let drift_magnitude = (drift.drift_ratio - 1.0).abs();

        if drift_magnitude >= self.config.min_drift_for_full {
            return ReoptDecision::FullReopt {
                reason: format!("Drift ratio {:.2} exceeds full threshold", drift.drift_ratio),
            };
        }

        let mut affected_edges = Vec::new();
        for (edge_idx, health) in edge_health {
            if *health < self.config.health_threshold {
                affected_edges.push(*edge_idx);
            }
        }

        for (edge_idx, edge_drift) in &drift.edge_drift {
            if (edge_drift - 1.0).abs() >= self.config.min_drift_for_partial {
                if !affected_edges.contains(edge_idx) {
                    affected_edges.push(*edge_idx);
                }
            }
        }

        if !affected_edges.is_empty() {
            return ReoptDecision::PartialReopt {
                affected_chunks: Vec::new(),
                affected_edges,
            };
        }

        if drift_magnitude >= self.config.min_drift_for_partial {
            return ReoptDecision::PartialReopt {
                affected_chunks: Vec::new(),
                affected_edges: Vec::new(),
            };
        }

        ReoptDecision::NoAction
    }

    /// Evaluate a list of triggers to make a reoptimization decision
    pub fn should_reoptimize(&self, triggers: &[ReoptTrigger]) -> ReoptDecision {
        if let Some(remaining) = self.cooldown_remaining() {
            return ReoptDecision::Pause {
                duration_ms: remaining.as_millis() as u64,
                reason: "Cooldown period active".to_string(),
            };
        }

        if triggers.is_empty() {
            return ReoptDecision::NoAction;
        }

        let mut affected_edges = Vec::new();
        let mut full_reopt_reasons = Vec::new();

        for trigger in triggers {
            match trigger {
                ReoptTrigger::DriftExceeded { drift_ratio, threshold } => {
                    let magnitude = (drift_ratio - 1.0).abs();
                    if magnitude >= self.config.min_drift_for_full {
                        full_reopt_reasons.push(format!(
                            "Drift ratio {:.2} exceeds full threshold {:.2}",
                            drift_ratio, self.config.min_drift_for_full
                        ));
                    } else if magnitude >= self.config.min_drift_for_partial {
                        full_reopt_reasons.push(format!(
                            "Drift ratio {:.2} exceeds partial threshold {:.2}",
                            drift_ratio, self.config.min_drift_for_partial
                        ));
                    }
                }
                ReoptTrigger::EdgeDegraded { edge_idx, health_score } => {
                    if *health_score < self.config.health_threshold {
                        affected_edges.push(*edge_idx);
                    }
                }
                ReoptTrigger::EdgeRecovered { edge_idx, .. } => {
                    if !affected_edges.contains(edge_idx) {
                        affected_edges.push(*edge_idx);
                    }
                }
                ReoptTrigger::LoadImbalance { overloaded, underloaded } => {
                    for edge in overloaded {
                        if !affected_edges.contains(edge) {
                            affected_edges.push(*edge);
                        }
                    }
                    for edge in underloaded {
                        if !affected_edges.contains(edge) {
                            affected_edges.push(*edge);
                        }
                    }
                }
                ReoptTrigger::ScheduleStale { age_ms, .. } => {
                    if *age_ms >= self.config.max_schedule_age_ms {
                        full_reopt_reasons.push(format!("Schedule age {}ms exceeds max", age_ms));
                    }
                }
                ReoptTrigger::Manual => {
                    full_reopt_reasons.push("Manual trigger".to_string());
                }
            }
        }

        if !full_reopt_reasons.is_empty() {
            return ReoptDecision::FullReopt {
                reason: full_reopt_reasons.join("; "),
            };
        }

        if !affected_edges.is_empty() {
            return ReoptDecision::PartialReopt {
                affected_chunks: Vec::new(),
                affected_edges,
            };
        }

        ReoptDecision::NoAction
    }

    /// Get remaining cooldown time
    pub fn cooldown_remaining(&self) -> Option<Duration> {
        if let Some(last) = self.last_reopt {
            let elapsed = last.elapsed();
            let cooldown = Duration::from_millis(self.config.cooldown_ms);
            if elapsed < cooldown {
                return Some(cooldown - elapsed);
            }
        }
        None
    }

    /// Record that a reoptimization occurred
    pub fn record_reopt(&mut self) {
        self.last_reopt = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drift_config() {
        let config = DriftConfig::default();
        assert_eq!(config.sample_window_ms, 5000);
        assert_eq!(config.min_samples, 5);
        assert!(config.validate().is_ok());

        let config = DriftConfig::new()
            .with_sample_window_ms(10000)
            .with_min_samples(10)
            .with_drift_threshold(0.3)
            .with_ema_alpha(0.5);
        assert_eq!(config.sample_window_ms, 10000);

        assert!(DriftConfig { sample_window_ms: 0, ..DriftConfig::new() }.validate().is_err());
        assert!(DriftConfig { drift_threshold: 1.5, ..DriftConfig::new() }.validate().is_err());
    }

    #[test]
    fn test_drift_metrics() {
        let metrics = DriftMetrics::default();
        assert_eq!(metrics.drift_ratio, 1.0);

        let mut slow = DriftMetrics::new();
        slow.drift_ratio = 1.5;
        assert!(slow.is_slower());

        let mut fast = DriftMetrics::new();
        fast.drift_ratio = 0.7;
        assert!(fast.is_faster());
    }

    #[test]
    fn test_drift_detector_basic() {
        let detector = DriftDetector::new(DriftConfig::new());
        assert_eq!(detector.transfers.len(), 0);

        let mut detector = DriftDetector::new(DriftConfig::new());
        detector.record_sample(TransferId::new(1), EdgeIdx::new(0), 1000, 100);
        assert!(detector.transfers.contains_key(&TransferId::new(1)));

        detector.set_baseline(TransferId::new(2), 1_000_000, 5000);
        let tracking = detector.transfers.get(&TransferId::new(2)).unwrap();
        assert_eq!(tracking.expected_speed_bps, 1_000_000);
    }

    #[test]
    fn test_drift_detector_calculate_drift() {
        let detector = DriftDetector::new(DriftConfig::new());
        let metrics = detector.calculate_drift(TransferId::new(999));
        assert_eq!(metrics.expected_speed_bps, 0);

        let mut detector = DriftDetector::new(DriftConfig::new().with_min_samples(5));
        detector.set_baseline(TransferId::new(1), 10_000, 1000);
        for _ in 0..3 {
            detector.record_sample(TransferId::new(1), EdgeIdx::new(0), 100, 10);
        }
        assert_eq!(detector.calculate_drift(TransferId::new(1)).actual_speed_bps, 0);

        detector.set_baseline(TransferId::new(2), 80_000, 1000);
        for _ in 0..5 {
            detector.record_sample(TransferId::new(2), EdgeIdx::new(0), 1000, 100);
        }
        assert!(detector.calculate_drift(TransferId::new(2)).actual_speed_bps > 0);
    }

    #[test]
    fn test_drift_detector_edge_detection() {
        let mut detector = DriftDetector::new(DriftConfig::new().with_min_samples(3));

        detector.set_baseline(TransferId::new(1), 100_000, 100);
        std::thread::sleep(Duration::from_millis(150));
        for _ in 0..5 {
            detector.record_sample(TransferId::new(1), EdgeIdx::new(0), 1000, 100);
        }
        assert!(detector.is_drifting(TransferId::new(1), 0.2));

        detector.set_baseline(TransferId::new(2), 100_000, 1000);
        for _ in 0..5 {
            detector.record_sample(TransferId::new(2), EdgeIdx::new(0), 500, 100);
        }
        assert!(detector.get_slow_edges(0.8).contains(&EdgeIdx::new(0)));

        detector.set_baseline(TransferId::new(3), 10_000, 1000);
        for _ in 0..5 {
            detector.record_sample(TransferId::new(3), EdgeIdx::new(1), 2000, 100);
        }
        assert!(detector.get_fast_edges(1.2).contains(&EdgeIdx::new(1)));
    }

    #[test]
    fn test_drift_detector_clear_operations() {
        let mut detector = DriftDetector::new(DriftConfig::new());
        detector.set_baseline(TransferId::new(1), 100_000, 1000);
        detector.clear(TransferId::new(1));
        assert!(!detector.transfers.contains_key(&TransferId::new(1)));

        detector.set_baseline(TransferId::new(1), 100_000, 1000);
        detector.set_baseline(TransferId::new(2), 200_000, 2000);
        detector.clear_all();
        assert_eq!(detector.transfers.len(), 0);
    }

    #[test]
    fn test_reopt_triggers_and_decisions() {
        assert!(matches!(ReoptTrigger::DriftExceeded { drift_ratio: 1.5, threshold: 0.3 }, ReoptTrigger::DriftExceeded { .. }));
        assert!(matches!(ReoptTrigger::Manual, ReoptTrigger::Manual));
        assert!(matches!(ReoptDecision::NoAction, ReoptDecision::NoAction));
        assert!(matches!(ReoptDecision::FullReopt { reason: "test".to_string() }, ReoptDecision::FullReopt { .. }));
    }

    #[test]
    fn test_reopt_config() {
        let config = ReoptConfig::default();
        assert_eq!(config.min_drift_for_partial, 0.3);
        assert_eq!(config.min_drift_for_full, 0.5);
        assert!(config.validate().is_ok());

        assert!(ReoptConfig { min_drift_for_partial: -0.1, ..ReoptConfig::new() }.validate().is_err());
        assert!(ReoptConfig { min_drift_for_full: 0.2, min_drift_for_partial: 0.3, ..ReoptConfig::new() }.validate().is_err());
    }

    #[test]
    fn test_reopt_evaluator_evaluate() {
        let evaluator = ReoptEvaluator::new(ReoptConfig::new());

        let decision = evaluator.evaluate(&DriftMetrics { drift_ratio: 1.1, ..DriftMetrics::new() }, &HashMap::new());
        assert!(matches!(decision, ReoptDecision::NoAction));

        let decision = evaluator.evaluate(&DriftMetrics { drift_ratio: 1.35, ..DriftMetrics::new() }, &HashMap::new());
        assert!(matches!(decision, ReoptDecision::PartialReopt { .. }));

        let mut edge_health = HashMap::new();
        edge_health.insert(EdgeIdx::new(0), 0.3);
        let decision = evaluator.evaluate(&DriftMetrics::new(), &edge_health);
        if let ReoptDecision::PartialReopt { affected_edges, .. } = decision {
            assert!(affected_edges.contains(&EdgeIdx::new(0)));
        }

        let decision = evaluator.evaluate(&DriftMetrics { drift_ratio: 1.6, ..DriftMetrics::new() }, &HashMap::new());
        assert!(matches!(decision, ReoptDecision::FullReopt { .. }));
    }

    #[test]
    fn test_reopt_evaluator_cooldown() {
        let mut evaluator = ReoptEvaluator::new(ReoptConfig::new());
        assert!(evaluator.cooldown_remaining().is_none());

        evaluator.record_reopt();
        assert!(evaluator.cooldown_remaining().is_some());

        let decision = evaluator.evaluate(&DriftMetrics { drift_ratio: 1.6, ..DriftMetrics::new() }, &HashMap::new());
        assert!(matches!(decision, ReoptDecision::Pause { .. }));
    }

    #[test]
    fn test_reopt_evaluator_should_reoptimize() {
        let evaluator = ReoptEvaluator::new(ReoptConfig::new());

        assert!(matches!(evaluator.should_reoptimize(&[]), ReoptDecision::NoAction));
        assert!(matches!(evaluator.should_reoptimize(&[ReoptTrigger::Manual]), ReoptDecision::FullReopt { .. }));
        assert!(matches!(
            evaluator.should_reoptimize(&[ReoptTrigger::DriftExceeded { drift_ratio: 1.6, threshold: 0.3 }]),
            ReoptDecision::FullReopt { .. }
        ));
        assert!(matches!(
            evaluator.should_reoptimize(&[ReoptTrigger::ScheduleStale { age_ms: 70000, max_age_ms: 60000 }]),
            ReoptDecision::FullReopt { .. }
        ));

        if let ReoptDecision::PartialReopt { affected_edges, .. } =
            evaluator.should_reoptimize(&[ReoptTrigger::EdgeDegraded { edge_idx: EdgeIdx::new(0), health_score: 0.3 }])
        {
            assert!(affected_edges.contains(&EdgeIdx::new(0)));
        }

        if let ReoptDecision::PartialReopt { affected_edges, .. } = evaluator.should_reoptimize(&[ReoptTrigger::LoadImbalance {
            overloaded: vec![EdgeIdx::new(0)],
            underloaded: vec![EdgeIdx::new(1)],
        }]) {
            assert_eq!(affected_edges.len(), 2);
        }
    }

    #[test]
    fn test_integration_drift_to_decision() {
        let mut detector = DriftDetector::new(DriftConfig::new().with_min_samples(3));
        detector.set_baseline(TransferId::new(1), 100_000, 100);
        std::thread::sleep(Duration::from_millis(200));
        for _ in 0..5 {
            detector.record_sample(TransferId::new(1), EdgeIdx::new(0), 1000, 100);
        }

        let metrics = detector.calculate_drift(TransferId::new(1));
        let trigger = ReoptTrigger::DriftExceeded { drift_ratio: metrics.drift_ratio, threshold: 0.3 };
        let decision = ReoptEvaluator::new(ReoptConfig::new()).should_reoptimize(&[trigger]);
        assert!(matches!(decision, ReoptDecision::FullReopt { .. } | ReoptDecision::PartialReopt { .. }));
    }

    #[test]
    fn test_multiple_edges_and_ema() {
        let mut detector = DriftDetector::new(DriftConfig::new().with_min_samples(3));
        detector.set_baseline(TransferId::new(1), 100_000, 1000);
        for _ in 0..5 {
            detector.record_sample(TransferId::new(1), EdgeIdx::new(0), 1000, 100);
            detector.record_sample(TransferId::new(1), EdgeIdx::new(1), 2000, 100);
        }
        assert_eq!(detector.calculate_drift(TransferId::new(1)).edge_drift.len(), 2);

        let mut perf = EdgePerformance::new();
        perf.add_sample(TransferSample { edge_idx: EdgeIdx::new(0), bytes: 1000, duration_ms: 100, timestamp: Instant::now() }, 0.3);
        assert_eq!(perf.ema_speed_bps, 80_000.0);
        perf.add_sample(TransferSample { edge_idx: EdgeIdx::new(0), bytes: 2000, duration_ms: 100, timestamp: Instant::now() }, 0.3);
        assert_eq!(perf.ema_speed_bps, 0.3 * 160_000.0 + 0.7 * 80_000.0);
    }

    #[test]
    fn test_per_edge_drift_evaluation() {
        let mut drift = DriftMetrics::new();
        drift.edge_drift.insert(EdgeIdx::new(0), 0.5);
        drift.edge_drift.insert(EdgeIdx::new(1), 1.5);

        if let ReoptDecision::PartialReopt { affected_edges, .. } = ReoptEvaluator::new(ReoptConfig::new()).evaluate(&drift, &HashMap::new()) {
            assert!(affected_edges.contains(&EdgeIdx::new(0)));
            assert!(affected_edges.contains(&EdgeIdx::new(1)));
        }
    }

    #[test]
    fn test_serialization() {
        let mut metrics = DriftMetrics::new();
        metrics.expected_speed_bps = 100_000;
        metrics.drift_ratio = 1.25;
        let json = serde_json::to_string(&metrics).unwrap();
        let deser: DriftMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.expected_speed_bps, 100_000);

        let trigger = ReoptTrigger::EdgeDegraded { edge_idx: EdgeIdx::new(5), health_score: 0.3 };
        assert_eq!(trigger, serde_json::from_str(&serde_json::to_string(&trigger).unwrap()).unwrap());

        let decision = ReoptDecision::PartialReopt { affected_chunks: vec![ChunkId::new(1)], affected_edges: vec![EdgeIdx::new(0)] };
        assert_eq!(decision, serde_json::from_str(&serde_json::to_string(&decision).unwrap()).unwrap());
    }
}
