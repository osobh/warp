//! Scrub daemon - the main background service

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use super::{QuarantineManager, QuarantineReason, QuarantinedBlock, ScrubMetrics, ScrubScheduler, ScrubSchedule};
use crate::replication::{DistributedShardManager, ShardHealth, ShardKey, ShardLocation};
use crate::healer::{RepairQueue, RepairJob, RepairPriority};
use crate::error::{Error, Result};

/// Configuration for the scrub daemon
#[derive(Debug, Clone)]
pub struct ScrubConfig {
    /// Schedule for scrub operations
    pub schedule: ScrubSchedule,

    /// Number of parallel scrub workers
    pub worker_count: usize,

    /// Batch size for object scanning
    pub batch_size: usize,

    /// Rate limit: max objects per second
    pub max_objects_per_sec: usize,

    /// Whether to auto-start the daemon
    pub auto_start: bool,

    /// Enable GPU-accelerated checksums
    pub gpu_acceleration: bool,

    /// Maximum repair attempts for quarantined blocks
    pub max_repair_attempts: u32,

    /// Quarantine retention period
    pub quarantine_retention: Duration,
}

impl Default for ScrubConfig {
    fn default() -> Self {
        Self {
            schedule: ScrubSchedule::default(),
            worker_count: 2,
            batch_size: 100,
            max_objects_per_sec: 1000,
            auto_start: true,
            gpu_acceleration: false,
            max_repair_attempts: 3,
            quarantine_retention: Duration::from_secs(30 * 24 * 3600), // 30 days
        }
    }
}

/// State of the scrub daemon
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrubState {
    /// Daemon is stopped
    Stopped,
    /// Daemon is idle, waiting for next scrub
    Idle,
    /// Running a light scrub
    LightScrub,
    /// Running a deep scrub
    DeepScrub,
    /// Daemon is shutting down
    ShuttingDown,
}

/// A scrub job
#[derive(Debug, Clone)]
pub struct ScrubJob {
    /// Unique job ID
    pub id: u64,

    /// Bucket to scrub
    pub bucket: String,

    /// Object prefix (empty for all)
    pub prefix: String,

    /// Whether this is a deep scrub
    pub deep: bool,

    /// When the job was created
    pub created_at: Instant,
}

impl ScrubJob {
    /// Create a new scrub job
    pub fn new(bucket: String, prefix: String, deep: bool) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            bucket,
            prefix,
            deep,
            created_at: Instant::now(),
        }
    }

    /// Create a full scrub job for a bucket
    pub fn full_bucket(bucket: String, deep: bool) -> Self {
        Self::new(bucket, String::new(), deep)
    }
}

/// Result of a scrub operation
#[derive(Debug, Clone)]
pub struct ScrubResult {
    /// Job that was processed
    pub job_id: u64,

    /// Whether the scrub succeeded
    pub success: bool,

    /// Number of objects scanned
    pub objects_scanned: u64,

    /// Number of bytes verified
    pub bytes_verified: u64,

    /// Number of errors found
    pub errors_found: u64,

    /// Time taken
    pub duration: Duration,

    /// Error message if failed
    pub error: Option<String>,
}

/// The scrub daemon
pub struct ScrubDaemon {
    /// Configuration
    config: ScrubConfig,

    /// Current state
    state: RwLock<ScrubState>,

    /// Scheduler
    scheduler: RwLock<ScrubScheduler>,

    /// Metrics collector
    metrics: Arc<ScrubMetrics>,

    /// Quarantine manager
    quarantine: Arc<QuarantineManager>,

    /// Shard manager reference
    shard_manager: Arc<DistributedShardManager>,

    /// Repair queue reference (for triggering repairs)
    repair_queue: Arc<RepairQueue>,

    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
}

impl ScrubDaemon {
    /// Create a new scrub daemon
    pub fn new(
        config: ScrubConfig,
        shard_manager: Arc<DistributedShardManager>,
        repair_queue: Arc<RepairQueue>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            scheduler: RwLock::new(ScrubScheduler::new(config.schedule.clone())),
            quarantine: Arc::new(QuarantineManager::with_settings(
                config.max_repair_attempts,
                config.quarantine_retention,
            )),
            config,
            state: RwLock::new(ScrubState::Stopped),
            metrics: Arc::new(ScrubMetrics::new()),
            shard_manager,
            repair_queue,
            shutdown_tx,
        }
    }

    /// Start the scrub daemon
    pub async fn start(&self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state != ScrubState::Stopped {
            return Ok(()); // Already running
        }
        *state = ScrubState::Idle;
        drop(state);

        info!(
            workers = self.config.worker_count,
            "Starting scrub daemon"
        );

        // Spawn the main scheduling loop
        self.spawn_scheduler_loop();

        self.metrics.record_start();
        info!("Scrub daemon started");

        Ok(())
    }

    /// Stop the scrub daemon
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state == ScrubState::Stopped {
            return Ok(());
        }
        *state = ScrubState::ShuttingDown;
        drop(state);

        info!("Stopping scrub daemon");

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        *self.state.write().await = ScrubState::Stopped;
        self.metrics.record_stop();
        info!("Scrub daemon stopped");

        Ok(())
    }

    /// Get current daemon state
    pub async fn state(&self) -> ScrubState {
        *self.state.read().await
    }

    /// Get metrics
    pub fn metrics(&self) -> &ScrubMetrics {
        &self.metrics
    }

    /// Get quarantine manager
    pub fn quarantine(&self) -> &QuarantineManager {
        &self.quarantine
    }

    /// Trigger an immediate light scrub
    pub async fn trigger_light_scrub(&self) -> Result<ScrubResult> {
        self.do_scrub(false).await
    }

    /// Trigger an immediate deep scrub
    pub async fn trigger_deep_scrub(&self) -> Result<ScrubResult> {
        self.do_scrub(true).await
    }

    /// Pause scrubbing
    pub async fn pause(&self) {
        self.scheduler.write().await.pause();
    }

    /// Resume scrubbing
    pub async fn resume(&self) {
        self.scheduler.write().await.resume();
    }

    /// Update system load
    pub async fn update_load(&self, load: f64) {
        self.scheduler.write().await.update_load(load);
    }

    // =========================================================================
    // Internal methods
    // =========================================================================

    fn spawn_scheduler_loop(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let metrics = self.metrics.clone();
        let quarantine = self.quarantine.clone();
        let shard_manager = self.shard_manager.clone();
        let repair_queue = self.repair_queue.clone();
        let scheduler = Arc::new(parking_lot::RwLock::new(ScrubScheduler::new(config.schedule.clone())));
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_secs(60)); // Check every minute

            loop {
                tokio::select! {
                    _ = check_interval.tick() => {
                        // Determine what action to take while holding the lock briefly
                        let action = {
                            let sched = scheduler.read();
                            if sched.should_deep_scrub() {
                                Some(true) // deep scrub
                            } else if sched.should_light_scrub() {
                                Some(false) // light scrub
                            } else {
                                None
                            }
                        }; // Lock is dropped here

                        // Perform the action without holding the lock
                        if let Some(is_deep) = action {
                            if is_deep {
                                info!("Starting scheduled deep scrub");
                                if let Err(e) = Self::do_scrub_internal(
                                    true,
                                    &shard_manager,
                                    &repair_queue,
                                    &quarantine,
                                    &metrics,
                                    &config,
                                ).await {
                                    error!(error = %e, "Deep scrub failed");
                                }
                                scheduler.write().record_deep_scrub();
                            } else {
                                info!("Starting scheduled light scrub");
                                if let Err(e) = Self::do_scrub_internal(
                                    false,
                                    &shard_manager,
                                    &repair_queue,
                                    &quarantine,
                                    &metrics,
                                    &config,
                                ).await {
                                    error!(error = %e, "Light scrub failed");
                                }
                                scheduler.write().record_light_scrub();
                            }
                        }

                        // Periodic cleanup
                        let removed = quarantine.cleanup_expired();
                        if removed > 0 {
                            debug!(removed, "Cleaned up expired quarantine entries");
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        debug!("Scheduler loop received shutdown signal");
                        break;
                    }
                }
            }
        })
    }

    async fn do_scrub(&self, deep: bool) -> Result<ScrubResult> {
        Self::do_scrub_internal(
            deep,
            &self.shard_manager,
            &self.repair_queue,
            &self.quarantine,
            &self.metrics,
            &self.config,
        ).await
    }

    async fn do_scrub_internal(
        deep: bool,
        shard_manager: &DistributedShardManager,
        repair_queue: &RepairQueue,
        quarantine: &QuarantineManager,
        metrics: &ScrubMetrics,
        config: &ScrubConfig,
    ) -> Result<ScrubResult> {
        metrics.record_scrub_start(deep);
        let start = Instant::now();

        let mut objects_scanned = 0u64;
        let mut bytes_verified = 0u64;
        let mut errors_found = 0u64;

        // Get all shard distributions
        let distributions = shard_manager.get_all_distributions().await;

        for (_object_key, distribution) in distributions {
            // Rate limiting
            if objects_scanned as usize >= config.max_objects_per_sec {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            // locations is HashMap<ShardIndex, Vec<ShardLocation>>
            for (shard_idx, locations) in &distribution.locations {
                let shard_key = ShardKey::new(
                    &distribution.bucket,
                    &distribution.key,
                    *shard_idx,
                );

                // Skip already quarantined blocks
                if quarantine.is_quarantined(&shard_key) {
                    continue;
                }

                // Check each location for this shard
                for location in locations {
                    // Light scrub: just check metadata and health status
                    if !deep {
                        if location.health != ShardHealth::Healthy {
                            errors_found += 1;
                            metrics.record_metadata_error();

                            // Quarantine degraded shards
                            let block = QuarantinedBlock::new(
                                shard_key.clone(),
                                QuarantineReason::MetadataCorruption,
                            ).with_diagnostic(format!("Health status: {:?}", location.health));

                            quarantine.quarantine(block);
                            metrics.record_quarantine();

                            // Trigger repair
                            let job = RepairJob::new(shard_key.clone(), RepairPriority::High);
                            repair_queue.push(job);
                            metrics.record_repair_triggered();
                        }
                    } else {
                        // Deep scrub: verify actual data
                        match shard_manager.verify_shard(&shard_key, location).await {
                            Ok(verification) => {
                                bytes_verified += verification.bytes_verified;
                                metrics.record_object_scanned(verification.bytes_verified);

                                if !verification.checksum_valid {
                                    errors_found += 1;
                                    metrics.record_checksum_error();

                                    let block = if let (Some(expected), Some(actual)) =
                                        (verification.expected_checksum, verification.actual_checksum)
                                    {
                                        QuarantinedBlock::with_checksums(
                                            shard_key.clone(),
                                            QuarantineReason::ChecksumMismatch,
                                            expected,
                                            actual,
                                        )
                                    } else {
                                        QuarantinedBlock::new(
                                            shard_key.clone(),
                                            QuarantineReason::ChecksumMismatch,
                                        )
                                    };

                                    quarantine.quarantine(block);
                                    metrics.record_quarantine();

                                    // Trigger repair for checksum errors
                                    let job = RepairJob::new(shard_key.clone(), RepairPriority::Critical);
                                    repair_queue.push(job);
                                    metrics.record_repair_triggered();
                                }
                            }
                            Err(e) => {
                                errors_found += 1;
                                let err_str: String = e.to_string();
                                warn!(shard = ?shard_key, error = %err_str, "Failed to verify shard");

                                let block = QuarantinedBlock::new(
                                    shard_key.clone(),
                                    QuarantineReason::ReadError,
                                ).with_diagnostic(err_str);

                                quarantine.quarantine(block);
                                metrics.record_quarantine();
                            }
                        }
                    }

                    objects_scanned += 1;
                }
            }
        }

        let duration = start.elapsed();
        metrics.record_scrub_complete(duration);

        info!(
            deep,
            objects_scanned,
            bytes_verified,
            errors_found,
            duration = ?duration,
            "Scrub complete"
        );

        Ok(ScrubResult {
            job_id: 0,
            success: true,
            objects_scanned,
            bytes_verified,
            errors_found,
            duration,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ScrubConfig::default();
        assert_eq!(config.worker_count, 2);
        assert_eq!(config.batch_size, 100);
    }

    #[test]
    fn test_scrub_job() {
        let job = ScrubJob::full_bucket("my-bucket".to_string(), true);
        assert!(job.deep);
        assert!(job.prefix.is_empty());
    }
}
