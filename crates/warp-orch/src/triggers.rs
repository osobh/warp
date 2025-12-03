//! Reoptimization Trigger Generation
//!
//! This module provides a TriggerGenerator that monitors ongoing transfers
//! and generates ReoptTrigger events by:
//! - Processing ProgressUpdate events to feed DriftDetector
//! - Tracking edge health changes
//! - Detecting load imbalances
//! - Managing schedule staleness
//!
//! The TriggerGenerator integrates with the Orchestrator's reconciliation loop
//! to provide real-time trigger detection.

use crate::progress::ProgressUpdate;
use crate::reconcile::{DriftConfig, DriftDetector, ReoptTrigger};
use crate::types::TransferId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use warp_sched::EdgeIdx;

/// Configuration for trigger generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Health threshold below which EdgeDegraded is triggered (default 0.5)
    pub health_threshold: f64,
    /// Health threshold above which EdgeRecovered is triggered (default 0.7)
    pub recovery_threshold: f64,
    /// Load ratio above which an edge is considered overloaded (default 0.8)
    pub overload_threshold: f64,
    /// Load ratio below which an edge is considered underloaded (default 0.3)
    pub underload_threshold: f64,
    /// Maximum schedule age before ScheduleStale is triggered (default 60000ms)
    pub max_schedule_age_ms: u64,
    /// Minimum edges in imbalance before LoadImbalance is triggered (default 2)
    pub min_imbalance_edges: usize,
    /// Drift threshold for triggering DriftExceeded (default 0.2)
    pub drift_threshold: f64,
}

impl TriggerConfig {
    /// Create a new TriggerConfig with default values
    pub fn new() -> Self {
        Self {
            health_threshold: 0.5,
            recovery_threshold: 0.7,
            overload_threshold: 0.8,
            underload_threshold: 0.3,
            max_schedule_age_ms: 60000,
            min_imbalance_edges: 2,
            drift_threshold: 0.2,
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.health_threshold) {
            return Err("health_threshold must be between 0.0 and 1.0".to_string());
        }
        if !(0.0..=1.0).contains(&self.recovery_threshold) {
            return Err("recovery_threshold must be between 0.0 and 1.0".to_string());
        }
        if self.recovery_threshold < self.health_threshold {
            return Err("recovery_threshold must be >= health_threshold".to_string());
        }
        if !(0.0..=1.0).contains(&self.overload_threshold) {
            return Err("overload_threshold must be between 0.0 and 1.0".to_string());
        }
        if !(0.0..=1.0).contains(&self.underload_threshold) {
            return Err("underload_threshold must be between 0.0 and 1.0".to_string());
        }
        if self.underload_threshold >= self.overload_threshold {
            return Err("underload_threshold must be < overload_threshold".to_string());
        }
        if !(0.0..=1.0).contains(&self.drift_threshold) {
            return Err("drift_threshold must be between 0.0 and 1.0".to_string());
        }
        Ok(())
    }

    /// Set health threshold
    pub fn with_health_threshold(mut self, threshold: f64) -> Self {
        self.health_threshold = threshold;
        self
    }

    /// Set recovery threshold
    pub fn with_recovery_threshold(mut self, threshold: f64) -> Self {
        self.recovery_threshold = threshold;
        self
    }

    /// Set overload threshold
    pub fn with_overload_threshold(mut self, threshold: f64) -> Self {
        self.overload_threshold = threshold;
        self
    }

    /// Set underload threshold
    pub fn with_underload_threshold(mut self, threshold: f64) -> Self {
        self.underload_threshold = threshold;
        self
    }

    /// Set max schedule age
    pub fn with_max_schedule_age_ms(mut self, age: u64) -> Self {
        self.max_schedule_age_ms = age;
        self
    }

    /// Set drift threshold
    pub fn with_drift_threshold(mut self, threshold: f64) -> Self {
        self.drift_threshold = threshold;
        self
    }

    /// Set minimum imbalance edges
    pub fn with_min_imbalance_edges(mut self, min: usize) -> Self {
        self.min_imbalance_edges = min;
        self
    }
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Edge health state for tracking changes
#[derive(Debug, Clone)]
struct EdgeHealthState {
    current_health: f64,
    was_degraded: bool,
    last_update: Instant,
}

/// Edge load state for tracking imbalances
#[derive(Debug, Clone)]
struct EdgeLoadState {
    load_ratio: f64,
    active_transfers: u32,
    max_transfers: u32,
    last_update: Instant,
}

/// Generates reoptimization triggers from various sources
pub struct TriggerGenerator {
    config: TriggerConfig,
    drift_detector: DriftDetector,
    /// Current health state per edge
    health_states: HashMap<EdgeIdx, EdgeHealthState>,
    /// Current load state per edge
    load_states: HashMap<EdgeIdx, EdgeLoadState>,
    /// Pending triggers to be collected
    pending_triggers: Vec<ReoptTrigger>,
    /// When the current schedule was created
    schedule_created_at: Option<Instant>,
    /// Transfer ID to expected speed mapping
    transfer_expectations: HashMap<TransferId, u64>,
}

impl TriggerGenerator {
    /// Create a new TriggerGenerator with the given configuration
    pub fn new(config: TriggerConfig, drift_config: DriftConfig) -> Self {
        Self {
            config,
            drift_detector: DriftDetector::new(drift_config),
            health_states: HashMap::new(),
            load_states: HashMap::new(),
            pending_triggers: Vec::new(),
            schedule_created_at: None,
            transfer_expectations: HashMap::new(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(TriggerConfig::default(), DriftConfig::default())
    }

    /// Set the schedule creation time for staleness tracking
    pub fn set_schedule_created(&mut self, instant: Instant) {
        self.schedule_created_at = Some(instant);
    }

    /// Mark current schedule as refreshed
    pub fn refresh_schedule(&mut self) {
        self.schedule_created_at = Some(Instant::now());
    }

    /// Set expected speed for a transfer
    pub fn set_transfer_baseline(
        &mut self,
        transfer_id: TransferId,
        expected_speed_bps: u64,
        expected_completion_ms: u64,
    ) {
        self.transfer_expectations.insert(transfer_id, expected_speed_bps);
        self.drift_detector.set_baseline(transfer_id, expected_speed_bps, expected_completion_ms);
    }

    /// Process a progress update from the ProgressTracker
    ///
    /// This feeds the DriftDetector and may generate DriftExceeded triggers.
    pub fn on_progress_update(&mut self, update: &ProgressUpdate, edge_idx: EdgeIdx) {
        // Calculate bytes transferred in this update
        // We assume this is called incrementally, so we estimate bytes from speed
        let duration_ms = 100; // Assume ~100ms between updates
        let bytes = (update.current_speed_bps * duration_ms / 1000) as u64;

        // Record sample in drift detector
        self.drift_detector.record_sample(
            update.transfer_id,
            edge_idx,
            bytes,
            duration_ms,
        );

        // Check for drift
        if self.drift_detector.is_drifting(update.transfer_id, self.config.drift_threshold) {
            let metrics = self.drift_detector.calculate_drift(update.transfer_id);
            self.pending_triggers.push(ReoptTrigger::DriftExceeded {
                drift_ratio: metrics.drift_ratio,
                threshold: self.config.drift_threshold,
            });
        }
    }

    /// Process a health change for an edge
    ///
    /// Generates EdgeDegraded or EdgeRecovered triggers based on threshold crossings.
    pub fn on_health_change(&mut self, edge_idx: EdgeIdx, new_health: f64) {
        let now = Instant::now();

        let (trigger, was_degraded) = if let Some(state) = self.health_states.get(&edge_idx) {
            let old_degraded = state.was_degraded;
            let now_degraded = new_health < self.config.health_threshold;
            let now_recovered = !old_degraded || new_health >= self.config.recovery_threshold;

            if now_degraded && !old_degraded {
                // Newly degraded
                (Some(ReoptTrigger::EdgeDegraded {
                    edge_idx,
                    health_score: new_health,
                }), true)
            } else if old_degraded && now_recovered && new_health >= self.config.recovery_threshold {
                // Recovered
                (Some(ReoptTrigger::EdgeRecovered {
                    edge_idx,
                    health_score: new_health,
                }), false)
            } else {
                (None, now_degraded)
            }
        } else {
            // First health report
            let now_degraded = new_health < self.config.health_threshold;
            if now_degraded {
                (Some(ReoptTrigger::EdgeDegraded {
                    edge_idx,
                    health_score: new_health,
                }), true)
            } else {
                (None, false)
            }
        };

        // Update state
        self.health_states.insert(edge_idx, EdgeHealthState {
            current_health: new_health,
            was_degraded,
            last_update: now,
        });

        // Add trigger if generated
        if let Some(t) = trigger {
            self.pending_triggers.push(t);
        }
    }

    /// Process a load update for an edge
    ///
    /// Tracks load ratios and generates LoadImbalance triggers.
    pub fn on_load_update(&mut self, edge_idx: EdgeIdx, active: u32, max: u32) {
        let load_ratio = if max > 0 {
            active as f64 / max as f64
        } else {
            0.0
        };

        self.load_states.insert(edge_idx, EdgeLoadState {
            load_ratio,
            active_transfers: active,
            max_transfers: max,
            last_update: Instant::now(),
        });
    }

    /// Check for load imbalances and generate triggers
    fn check_load_imbalance(&mut self) {
        let mut overloaded = Vec::new();
        let mut underloaded = Vec::new();

        for (edge_idx, state) in &self.load_states {
            if state.load_ratio >= self.config.overload_threshold {
                overloaded.push(*edge_idx);
            } else if state.load_ratio <= self.config.underload_threshold && state.max_transfers > 0 {
                underloaded.push(*edge_idx);
            }
        }

        // Only trigger if we have enough edges in imbalance
        if overloaded.len() >= self.config.min_imbalance_edges
            || (overloaded.len() >= 1 && underloaded.len() >= self.config.min_imbalance_edges)
        {
            self.pending_triggers.push(ReoptTrigger::LoadImbalance {
                overloaded,
                underloaded,
            });
        }
    }

    /// Check for schedule staleness
    fn check_schedule_staleness(&mut self) {
        if let Some(created_at) = self.schedule_created_at {
            let age_ms = created_at.elapsed().as_millis() as u64;
            if age_ms >= self.config.max_schedule_age_ms {
                self.pending_triggers.push(ReoptTrigger::ScheduleStale {
                    age_ms,
                    max_age_ms: self.config.max_schedule_age_ms,
                });
            }
        }
    }

    /// Add a manual trigger
    pub fn trigger_manual(&mut self) {
        self.pending_triggers.push(ReoptTrigger::Manual);
    }

    /// Collect all pending triggers, clearing the internal buffer
    ///
    /// This also performs periodic checks (load imbalance, staleness).
    pub fn collect_triggers(&mut self) -> Vec<ReoptTrigger> {
        // Run periodic checks
        self.check_load_imbalance();
        self.check_schedule_staleness();

        // Drain and return pending triggers
        std::mem::take(&mut self.pending_triggers)
    }

    /// Get current health state for an edge
    pub fn get_health(&self, edge_idx: EdgeIdx) -> Option<f64> {
        self.health_states.get(&edge_idx).map(|s| s.current_health)
    }

    /// Get current load ratio for an edge
    pub fn get_load_ratio(&self, edge_idx: EdgeIdx) -> Option<f64> {
        self.load_states.get(&edge_idx).map(|s| s.load_ratio)
    }

    /// Check if an edge is currently degraded
    pub fn is_degraded(&self, edge_idx: EdgeIdx) -> bool {
        self.health_states
            .get(&edge_idx)
            .map(|s| s.was_degraded)
            .unwrap_or(false)
    }

    /// Get all degraded edges
    pub fn degraded_edges(&self) -> Vec<EdgeIdx> {
        self.health_states
            .iter()
            .filter(|(_, s)| s.was_degraded)
            .map(|(e, _)| *e)
            .collect()
    }

    /// Get all overloaded edges
    pub fn overloaded_edges(&self) -> Vec<EdgeIdx> {
        self.load_states
            .iter()
            .filter(|(_, s)| s.load_ratio >= self.config.overload_threshold)
            .map(|(e, _)| *e)
            .collect()
    }

    /// Get all underloaded edges
    pub fn underloaded_edges(&self) -> Vec<EdgeIdx> {
        self.load_states
            .iter()
            .filter(|(_, s)| s.load_ratio <= self.config.underload_threshold && s.max_transfers > 0)
            .map(|(e, _)| *e)
            .collect()
    }

    /// Clear transfer tracking for a completed transfer
    pub fn clear_transfer(&mut self, transfer_id: TransferId) {
        self.drift_detector.clear(transfer_id);
        self.transfer_expectations.remove(&transfer_id);
    }

    /// Clear all tracking data
    pub fn clear_all(&mut self) {
        self.drift_detector.clear_all();
        self.health_states.clear();
        self.load_states.clear();
        self.pending_triggers.clear();
        self.schedule_created_at = None;
        self.transfer_expectations.clear();
    }

    /// Get reference to the drift detector for advanced queries
    pub fn drift_detector(&self) -> &DriftDetector {
        &self.drift_detector
    }

    /// Get the configuration
    pub fn config(&self) -> &TriggerConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_config_default() {
        let config = TriggerConfig::default();
        assert_eq!(config.health_threshold, 0.5);
        assert_eq!(config.recovery_threshold, 0.7);
        assert_eq!(config.overload_threshold, 0.8);
        assert_eq!(config.underload_threshold, 0.3);
        assert_eq!(config.max_schedule_age_ms, 60000);
        assert_eq!(config.drift_threshold, 0.2);
    }

    #[test]
    fn test_trigger_config_validation() {
        let valid = TriggerConfig::default();
        assert!(valid.validate().is_ok());

        let invalid = TriggerConfig::default()
            .with_health_threshold(1.5);
        assert!(invalid.validate().is_err());

        let invalid_recovery = TriggerConfig::default()
            .with_health_threshold(0.8)
            .with_recovery_threshold(0.5);
        assert!(invalid_recovery.validate().is_err());

        let invalid_load = TriggerConfig::default()
            .with_underload_threshold(0.9)
            .with_overload_threshold(0.5);
        assert!(invalid_load.validate().is_err());
    }

    #[test]
    fn test_trigger_generator_creation() {
        let mut tg = TriggerGenerator::with_defaults();
        assert!(tg.collect_triggers().is_empty());
    }

    #[test]
    fn test_health_change_degraded() {
        let mut tg = TriggerGenerator::with_defaults();

        // First update with good health - no trigger
        tg.on_health_change(EdgeIdx(0), 0.9);
        let triggers = tg.collect_triggers();
        assert!(triggers.is_empty());

        // Health drops below threshold - should trigger
        tg.on_health_change(EdgeIdx(0), 0.4);
        let triggers = tg.collect_triggers();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            ReoptTrigger::EdgeDegraded { edge_idx, health_score } => {
                assert_eq!(*edge_idx, EdgeIdx(0));
                assert!(*health_score < 0.5);
            }
            _ => panic!("Expected EdgeDegraded trigger"),
        }
    }

    #[test]
    fn test_health_change_recovered() {
        let mut tg = TriggerGenerator::with_defaults();

        // Start degraded
        tg.on_health_change(EdgeIdx(0), 0.3);
        let _ = tg.collect_triggers(); // Clear the degraded trigger

        // Recover above recovery threshold
        tg.on_health_change(EdgeIdx(0), 0.8);
        let triggers = tg.collect_triggers();
        assert_eq!(triggers.len(), 1);
        match &triggers[0] {
            ReoptTrigger::EdgeRecovered { edge_idx, health_score } => {
                assert_eq!(*edge_idx, EdgeIdx(0));
                assert!(*health_score >= 0.7);
            }
            _ => panic!("Expected EdgeRecovered trigger"),
        }
    }

    #[test]
    fn test_health_no_trigger_in_hysteresis() {
        let mut tg = TriggerGenerator::with_defaults();

        // Start degraded
        tg.on_health_change(EdgeIdx(0), 0.3);
        let _ = tg.collect_triggers();

        // Improve but not above recovery threshold
        tg.on_health_change(EdgeIdx(0), 0.6);
        let triggers = tg.collect_triggers();
        // No recovery trigger yet (still in degraded state)
        assert!(triggers.iter().all(|t| !matches!(t, ReoptTrigger::EdgeRecovered { .. })));
    }

    #[test]
    fn test_load_imbalance() {
        let config = TriggerConfig::default().with_overload_threshold(0.8);
        let mut tg = TriggerGenerator::new(config, DriftConfig::default());

        // Add overloaded edges
        tg.on_load_update(EdgeIdx(0), 9, 10);
        tg.on_load_update(EdgeIdx(1), 8, 10);
        // Add underloaded edge
        tg.on_load_update(EdgeIdx(2), 1, 10);

        let triggers = tg.collect_triggers();
        assert!(triggers.iter().any(|t| matches!(t, ReoptTrigger::LoadImbalance { .. })));
    }

    #[test]
    fn test_no_load_imbalance_with_few_edges() {
        let config = TriggerConfig::default()
            .with_min_imbalance_edges(3);
        let mut tg = TriggerGenerator::new(config, DriftConfig::default());

        // Only one overloaded edge - not enough
        tg.on_load_update(EdgeIdx(0), 9, 10);

        let triggers = tg.collect_triggers();
        assert!(!triggers.iter().any(|t| matches!(t, ReoptTrigger::LoadImbalance { .. })));
    }

    #[test]
    fn test_schedule_staleness() {
        let config = TriggerConfig::default()
            .with_max_schedule_age_ms(100); // Very short for testing
        let mut tg = TriggerGenerator::new(config, DriftConfig::default());

        tg.set_schedule_created(Instant::now() - std::time::Duration::from_millis(200));

        let triggers = tg.collect_triggers();
        assert!(triggers.iter().any(|t| matches!(t, ReoptTrigger::ScheduleStale { .. })));
    }

    #[test]
    fn test_schedule_not_stale() {
        let config = TriggerConfig::default()
            .with_max_schedule_age_ms(60000);
        let mut tg = TriggerGenerator::new(config, DriftConfig::default());

        tg.refresh_schedule();

        let triggers = tg.collect_triggers();
        assert!(!triggers.iter().any(|t| matches!(t, ReoptTrigger::ScheduleStale { .. })));
    }

    #[test]
    fn test_manual_trigger() {
        let mut tg = TriggerGenerator::with_defaults();

        tg.trigger_manual();

        let triggers = tg.collect_triggers();
        assert_eq!(triggers.len(), 1);
        assert!(matches!(triggers[0], ReoptTrigger::Manual));
    }

    #[test]
    fn test_degraded_edges() {
        let mut tg = TriggerGenerator::with_defaults();

        tg.on_health_change(EdgeIdx(0), 0.3);
        tg.on_health_change(EdgeIdx(1), 0.9);
        tg.on_health_change(EdgeIdx(2), 0.4);

        let degraded = tg.degraded_edges();
        assert_eq!(degraded.len(), 2);
        assert!(degraded.contains(&EdgeIdx(0)));
        assert!(degraded.contains(&EdgeIdx(2)));
    }

    #[test]
    fn test_overloaded_underloaded_edges() {
        let mut tg = TriggerGenerator::with_defaults();

        tg.on_load_update(EdgeIdx(0), 9, 10); // 90% load
        tg.on_load_update(EdgeIdx(1), 5, 10); // 50% load
        tg.on_load_update(EdgeIdx(2), 2, 10); // 20% load

        let overloaded = tg.overloaded_edges();
        assert_eq!(overloaded.len(), 1);
        assert!(overloaded.contains(&EdgeIdx(0)));

        let underloaded = tg.underloaded_edges();
        assert_eq!(underloaded.len(), 1);
        assert!(underloaded.contains(&EdgeIdx(2)));
    }

    #[test]
    fn test_clear_transfer() {
        let mut tg = TriggerGenerator::with_defaults();

        let transfer_id = TransferId::new(1);
        tg.set_transfer_baseline(transfer_id, 1_000_000, 10000);

        tg.clear_transfer(transfer_id);
        // Should not panic and should clear expectations
    }

    #[test]
    fn test_clear_all() {
        let mut tg = TriggerGenerator::with_defaults();

        tg.on_health_change(EdgeIdx(0), 0.5);
        tg.on_load_update(EdgeIdx(0), 5, 10);
        tg.refresh_schedule();

        tg.clear_all();

        assert!(tg.health_states.is_empty());
        assert!(tg.load_states.is_empty());
        assert!(tg.schedule_created_at.is_none());
    }

    #[test]
    fn test_get_health() {
        let mut tg = TriggerGenerator::with_defaults();

        assert!(tg.get_health(EdgeIdx(0)).is_none());

        tg.on_health_change(EdgeIdx(0), 0.75);
        assert_eq!(tg.get_health(EdgeIdx(0)), Some(0.75));
    }

    #[test]
    fn test_get_load_ratio() {
        let mut tg = TriggerGenerator::with_defaults();

        assert!(tg.get_load_ratio(EdgeIdx(0)).is_none());

        tg.on_load_update(EdgeIdx(0), 3, 10);
        assert_eq!(tg.get_load_ratio(EdgeIdx(0)), Some(0.3));
    }

    #[test]
    fn test_config_builders() {
        let config = TriggerConfig::new()
            .with_health_threshold(0.6)
            .with_recovery_threshold(0.8)
            .with_overload_threshold(0.9)
            .with_underload_threshold(0.2)
            .with_max_schedule_age_ms(30000)
            .with_drift_threshold(0.15);

        assert_eq!(config.health_threshold, 0.6);
        assert_eq!(config.recovery_threshold, 0.8);
        assert_eq!(config.overload_threshold, 0.9);
        assert_eq!(config.underload_threshold, 0.2);
        assert_eq!(config.max_schedule_age_ms, 30000);
        assert_eq!(config.drift_threshold, 0.15);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_is_degraded() {
        let mut tg = TriggerGenerator::with_defaults();

        assert!(!tg.is_degraded(EdgeIdx(0)));

        tg.on_health_change(EdgeIdx(0), 0.3);
        assert!(tg.is_degraded(EdgeIdx(0)));

        tg.on_health_change(EdgeIdx(0), 0.8);
        let _ = tg.collect_triggers(); // Process recovery
        assert!(!tg.is_degraded(EdgeIdx(0)));
    }

    #[test]
    fn test_multiple_triggers_collected() {
        let config = TriggerConfig::default()
            .with_max_schedule_age_ms(10);
        let mut tg = TriggerGenerator::new(config, DriftConfig::default());

        // Create multiple trigger conditions
        tg.on_health_change(EdgeIdx(0), 0.3); // EdgeDegraded
        tg.trigger_manual(); // Manual
        tg.set_schedule_created(Instant::now() - std::time::Duration::from_millis(20)); // ScheduleStale

        let triggers = tg.collect_triggers();
        assert!(triggers.len() >= 3);
    }

    #[test]
    fn test_triggers_cleared_after_collect() {
        let mut tg = TriggerGenerator::with_defaults();

        tg.trigger_manual();
        let triggers1 = tg.collect_triggers();
        assert_eq!(triggers1.len(), 1);

        // Second collect should be empty
        let triggers2 = tg.collect_triggers();
        assert!(triggers2.is_empty());
    }
}
