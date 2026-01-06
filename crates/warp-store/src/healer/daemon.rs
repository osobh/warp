//! Healer daemon - the main background service

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, broadcast};
use tokio::time::{Instant, interval};
use tracing::{debug, error, info, warn};

use super::{HealerMetrics, RepairJob, RepairPriority, RepairQueue, RepairWorker};
use crate::error::Result;
use crate::replication::{DistributedShardManager, ShardHealth, ShardKey};

/// Configuration for the healer daemon
#[derive(Debug, Clone)]
pub struct HealerConfig {
    /// How often to scan for degraded shards
    pub scan_interval: Duration,

    /// Number of parallel repair workers
    pub worker_count: usize,

    /// Maximum repairs per scan cycle (rate limiting)
    pub max_repairs_per_cycle: usize,

    /// Delay between repair operations (to avoid overwhelming network)
    pub repair_delay: Duration,

    /// Timeout for individual repair operations
    pub repair_timeout: Duration,

    /// Whether to auto-start the daemon
    pub auto_start: bool,

    /// Minimum healthy shards before triggering repair
    pub repair_threshold: f64,

    /// Enable predictive repair (repair before failure)
    pub predictive_repair: bool,
}

impl Default for HealerConfig {
    fn default() -> Self {
        Self {
            scan_interval: Duration::from_secs(60),
            worker_count: 4,
            max_repairs_per_cycle: 100,
            repair_delay: Duration::from_millis(100),
            repair_timeout: Duration::from_secs(300),
            auto_start: true,
            repair_threshold: 0.75, // Repair when <75% shards healthy
            predictive_repair: false,
        }
    }
}

/// State of the healer daemon
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealerState {
    /// Daemon is stopped
    Stopped,
    /// Daemon is starting up
    Starting,
    /// Daemon is running normally
    Running,
    /// Daemon is scanning for degraded shards
    Scanning,
    /// Daemon is processing repairs
    Repairing,
    /// Daemon is shutting down
    ShuttingDown,
}

/// The self-healing daemon
pub struct HealerDaemon {
    /// Configuration
    config: HealerConfig,

    /// Current state
    state: RwLock<HealerState>,

    /// Repair queue
    queue: Arc<RepairQueue>,

    /// Metrics collector
    metrics: Arc<HealerMetrics>,

    /// Shard manager reference
    shard_manager: Arc<DistributedShardManager>,

    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
}

impl HealerDaemon {
    /// Create a new healer daemon
    pub fn new(config: HealerConfig, shard_manager: Arc<DistributedShardManager>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            config,
            state: RwLock::new(HealerState::Stopped),
            queue: Arc::new(RepairQueue::new()),
            metrics: Arc::new(HealerMetrics::new()),
            shard_manager,
            shutdown_tx,
        }
    }

    /// Start the healer daemon
    pub async fn start(&self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state != HealerState::Stopped {
            return Ok(()); // Already running
        }
        *state = HealerState::Starting;
        drop(state);

        info!(
            scan_interval = ?self.config.scan_interval,
            workers = self.config.worker_count,
            "Starting healer daemon"
        );

        // Spawn the main scan loop
        let _scan_handle = self.spawn_scan_loop();

        // Spawn repair workers
        let _worker_handles = self.spawn_workers();

        // Update state
        *self.state.write().await = HealerState::Running;

        self.metrics.record_start();
        info!("Healer daemon started");

        Ok(())
    }

    /// Stop the healer daemon
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state == HealerState::Stopped {
            return Ok(());
        }
        *state = HealerState::ShuttingDown;
        drop(state);

        info!("Stopping healer daemon");

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        // Wait for queue to drain (with timeout)
        let drain_timeout = Duration::from_secs(30);
        let start = Instant::now();
        while !self.queue.is_empty() && start.elapsed() < drain_timeout {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        *self.state.write().await = HealerState::Stopped;
        self.metrics.record_stop();
        info!("Healer daemon stopped");

        Ok(())
    }

    /// Get current daemon state
    pub async fn state(&self) -> HealerState {
        *self.state.read().await
    }

    /// Get metrics
    pub fn metrics(&self) -> &HealerMetrics {
        &self.metrics
    }

    /// Get the repair queue
    pub fn queue(&self) -> &RepairQueue {
        &self.queue
    }

    /// Trigger an immediate scan
    pub async fn trigger_scan(&self) -> Result<usize> {
        self.scan_for_repairs().await
    }

    /// Manually queue a repair job
    pub async fn queue_repair(&self, shard_key: ShardKey, priority: RepairPriority) {
        let job = RepairJob::new(shard_key, priority);
        self.queue.push(job);
        self.metrics.record_queued();
    }

    // =========================================================================
    // Internal methods
    // =========================================================================

    fn spawn_scan_loop(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let queue = self.queue.clone();
        let metrics = self.metrics.clone();
        let shard_manager = self.shard_manager.clone();
        let _state = Arc::new(self.state.read());
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut scan_interval = interval(config.scan_interval);

            loop {
                tokio::select! {
                    _ = scan_interval.tick() => {
                        // Perform scan
                        match Self::do_scan(&shard_manager, &queue, &metrics, &config).await {
                            Ok(count) => {
                                if count > 0 {
                                    info!(repairs_queued = count, "Scan complete");
                                } else {
                                    debug!("Scan complete, no repairs needed");
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Scan failed");
                                metrics.record_scan_error();
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        debug!("Scan loop received shutdown signal");
                        break;
                    }
                }
            }
        })
    }

    fn spawn_workers(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::with_capacity(self.config.worker_count);

        for worker_id in 0..self.config.worker_count {
            let worker = RepairWorker::new(
                worker_id,
                self.queue.clone(),
                self.shard_manager.clone(),
                self.metrics.clone(),
                self.config.repair_timeout,
            );

            let mut shutdown_rx = self.shutdown_tx.subscribe();
            let repair_delay = self.config.repair_delay;

            let handle = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = worker.process_one() => {
                            match result {
                                Ok(Some(_)) => {
                                    // Processed a job, add delay
                                    tokio::time::sleep(repair_delay).await;
                                }
                                Ok(None) => {
                                    // No jobs, wait a bit
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                }
                                Err(e) => {
                                    warn!(worker_id, error = %e, "Worker error");
                                    tokio::time::sleep(Duration::from_secs(1)).await;
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            debug!(worker_id, "Worker received shutdown signal");
                            break;
                        }
                    }
                }
            });

            handles.push(handle);
        }

        handles
    }

    async fn do_scan(
        shard_manager: &DistributedShardManager,
        queue: &RepairQueue,
        metrics: &HealerMetrics,
        config: &HealerConfig,
    ) -> Result<usize> {
        metrics.record_scan_start();
        let start = Instant::now();

        let mut repairs_queued = 0;

        // Get all shard distributions
        let distributions = shard_manager.get_all_distributions().await;

        for (_object_key, distribution) in distributions {
            // Check health of each shard (locations is HashMap<ShardIndex, Vec<ShardLocation>>)
            let healthy_count = distribution
                .locations
                .iter()
                .filter(|(_, locs)| locs.iter().any(|loc| loc.health == ShardHealth::Healthy))
                .count();

            let total_shards = distribution.total_shards;
            let health_ratio = healthy_count as f64 / total_shards as f64;

            if health_ratio < config.repair_threshold {
                // Find degraded/lost shards
                for (shard_idx, locations) in &distribution.locations {
                    // Check if any location for this shard is healthy
                    let has_healthy = locations.iter().any(|l| l.health == ShardHealth::Healthy);
                    if has_healthy {
                        continue;
                    }

                    // Get the worst health status from all locations
                    let worst_health = locations
                        .iter()
                        .map(|l| &l.health)
                        .min_by_key(|h| match h {
                            ShardHealth::Lost => 0,
                            ShardHealth::Degraded => 1,
                            ShardHealth::Unknown => 2,
                            ShardHealth::Repairing => 3,
                            ShardHealth::Healthy => 4,
                        })
                        .cloned()
                        .unwrap_or(ShardHealth::Unknown);

                    let priority = match worst_health {
                        ShardHealth::Lost => RepairPriority::Critical,
                        ShardHealth::Degraded => RepairPriority::High,
                        ShardHealth::Repairing => continue, // Skip already repairing
                        ShardHealth::Unknown => RepairPriority::Low,
                        ShardHealth::Healthy => continue,
                    };

                    let shard_key =
                        ShardKey::new(&distribution.bucket, &distribution.key, *shard_idx);

                    // Check if already in queue
                    if !queue.contains(&shard_key) {
                        let job = RepairJob::new(shard_key, priority);
                        queue.push(job);
                        repairs_queued += 1;
                        metrics.record_queued();

                        if repairs_queued >= config.max_repairs_per_cycle {
                            break;
                        }
                    }
                }
            }

            if repairs_queued >= config.max_repairs_per_cycle {
                break;
            }
        }

        metrics.record_scan_complete(start.elapsed());
        Ok(repairs_queued)
    }

    async fn scan_for_repairs(&self) -> Result<usize> {
        Self::do_scan(
            &self.shard_manager,
            &self.queue,
            &self.metrics,
            &self.config,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = HealerConfig::default();
        assert_eq!(config.worker_count, 4);
        assert_eq!(config.scan_interval, Duration::from_secs(60));
    }
}
