//! Scrub scheduling with cron-like timing

use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Local, Timelike};

/// Schedule for scrub operations
#[derive(Debug, Clone)]
pub struct ScrubSchedule {
    /// Light scrub interval (metadata only)
    pub light_interval: Duration,

    /// Deep scrub interval (full data verification)
    pub deep_interval: Duration,

    /// Preferred time window start (hour of day, 0-23)
    pub window_start: u8,

    /// Preferred time window end (hour of day, 0-23)
    pub window_end: u8,

    /// Days of week to run deep scrubs (0=Sunday, 6=Saturday)
    pub deep_scrub_days: Vec<u8>,

    /// Whether to pause during high load
    pub pause_during_high_load: bool,

    /// Load threshold to pause (0.0-1.0)
    pub load_threshold: f64,
}

impl Default for ScrubSchedule {
    fn default() -> Self {
        Self {
            light_interval: Duration::from_secs(24 * 3600), // Daily
            deep_interval: Duration::from_secs(7 * 24 * 3600), // Weekly
            window_start: 2,                                // 2 AM
            window_end: 6,                                  // 6 AM
            deep_scrub_days: vec![0],                       // Sunday
            pause_during_high_load: true,
            load_threshold: 0.8,
        }
    }
}

impl ScrubSchedule {
    /// Create a schedule for high-frequency scrubbing (test/critical data)
    pub fn high_frequency() -> Self {
        Self {
            light_interval: Duration::from_secs(4 * 3600), // 4 hours
            deep_interval: Duration::from_secs(24 * 3600), // Daily
            window_start: 0,
            window_end: 24,                             // Any time
            deep_scrub_days: vec![0, 1, 2, 3, 4, 5, 6], // Every day
            pause_during_high_load: false,
            load_threshold: 1.0,
        }
    }

    /// Create a schedule for archive data (low frequency)
    pub fn archive() -> Self {
        Self {
            light_interval: Duration::from_secs(7 * 24 * 3600), // Weekly
            deep_interval: Duration::from_secs(30 * 24 * 3600), // Monthly
            window_start: 0,
            window_end: 6,            // Night only
            deep_scrub_days: vec![0], // Sunday
            pause_during_high_load: true,
            load_threshold: 0.5,
        }
    }

    /// Check if we're in the preferred time window
    pub fn in_window(&self) -> bool {
        let now: DateTime<Local> = Local::now();
        let hour = now.hour() as u8;

        if self.window_start <= self.window_end {
            hour >= self.window_start && hour < self.window_end
        } else {
            // Window spans midnight (e.g., 22:00 to 06:00)
            hour >= self.window_start || hour < self.window_end
        }
    }

    /// Check if today is a deep scrub day
    pub fn is_deep_scrub_day(&self) -> bool {
        let now: DateTime<Local> = Local::now();
        let weekday = now.weekday().num_days_from_sunday() as u8;
        self.deep_scrub_days.contains(&weekday)
    }
}

/// Scheduler for scrub operations
pub struct ScrubScheduler {
    /// Schedule configuration
    schedule: ScrubSchedule,

    /// Last light scrub time
    last_light_scrub: Option<Instant>,

    /// Last deep scrub time
    last_deep_scrub: Option<Instant>,

    /// Current system load (0.0-1.0)
    current_load: f64,

    /// Whether scrubbing is paused
    paused: bool,
}

impl ScrubScheduler {
    /// Create a new scheduler
    pub fn new(schedule: ScrubSchedule) -> Self {
        Self {
            schedule,
            last_light_scrub: None,
            last_deep_scrub: None,
            current_load: 0.0,
            paused: false,
        }
    }

    /// Update current system load
    pub fn update_load(&mut self, load: f64) {
        self.current_load = load.clamp(0.0, 1.0);
    }

    /// Pause scrubbing
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume scrubbing
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Check if scrubbing is paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Check if a light scrub should run now
    pub fn should_light_scrub(&self) -> bool {
        if self.paused {
            return false;
        }

        if self.schedule.pause_during_high_load && self.current_load > self.schedule.load_threshold
        {
            return false;
        }

        match self.last_light_scrub {
            None => true,
            Some(last) => last.elapsed() >= self.schedule.light_interval,
        }
    }

    /// Check if a deep scrub should run now
    pub fn should_deep_scrub(&self) -> bool {
        if self.paused {
            return false;
        }

        if !self.schedule.in_window() {
            return false;
        }

        if !self.schedule.is_deep_scrub_day() {
            return false;
        }

        if self.schedule.pause_during_high_load && self.current_load > self.schedule.load_threshold
        {
            return false;
        }

        match self.last_deep_scrub {
            None => true,
            Some(last) => last.elapsed() >= self.schedule.deep_interval,
        }
    }

    /// Record that a light scrub completed
    pub fn record_light_scrub(&mut self) {
        self.last_light_scrub = Some(Instant::now());
    }

    /// Record that a deep scrub completed
    pub fn record_deep_scrub(&mut self) {
        self.last_deep_scrub = Some(Instant::now());
    }

    /// Get time until next light scrub
    pub fn time_until_light_scrub(&self) -> Duration {
        match self.last_light_scrub {
            None => Duration::ZERO,
            Some(last) => {
                let elapsed = last.elapsed();
                if elapsed >= self.schedule.light_interval {
                    Duration::ZERO
                } else {
                    self.schedule.light_interval - elapsed
                }
            }
        }
    }

    /// Get time until next deep scrub
    pub fn time_until_deep_scrub(&self) -> Duration {
        match self.last_deep_scrub {
            None => Duration::ZERO,
            Some(last) => {
                let elapsed = last.elapsed();
                if elapsed >= self.schedule.deep_interval {
                    Duration::ZERO
                } else {
                    self.schedule.deep_interval - elapsed
                }
            }
        }
    }

    /// Get the schedule
    pub fn schedule(&self) -> &ScrubSchedule {
        &self.schedule
    }

    /// Update the schedule
    pub fn set_schedule(&mut self, schedule: ScrubSchedule) {
        self.schedule = schedule;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_initial_state() {
        let scheduler = ScrubScheduler::new(ScrubSchedule::default());

        // Should want to scrub immediately when no previous scrubs
        assert!(scheduler.should_light_scrub());
    }

    #[test]
    fn test_scheduler_after_scrub() {
        let mut scheduler = ScrubScheduler::new(ScrubSchedule {
            light_interval: Duration::from_secs(3600),
            ..Default::default()
        });

        scheduler.record_light_scrub();

        // Should not want to scrub immediately after
        assert!(!scheduler.should_light_scrub());
    }

    #[test]
    fn test_scheduler_pause() {
        let mut scheduler = ScrubScheduler::new(ScrubSchedule::default());

        scheduler.pause();
        assert!(!scheduler.should_light_scrub());
        assert!(!scheduler.should_deep_scrub());

        scheduler.resume();
        assert!(scheduler.should_light_scrub());
    }

    #[test]
    fn test_scheduler_load_threshold() {
        let mut scheduler = ScrubScheduler::new(ScrubSchedule {
            pause_during_high_load: true,
            load_threshold: 0.8,
            ..Default::default()
        });

        scheduler.update_load(0.5);
        assert!(scheduler.should_light_scrub());

        scheduler.update_load(0.9);
        assert!(!scheduler.should_light_scrub());
    }

    #[test]
    fn test_schedule_presets() {
        let high_freq = ScrubSchedule::high_frequency();
        assert_eq!(high_freq.light_interval, Duration::from_secs(4 * 3600));

        let archive = ScrubSchedule::archive();
        assert_eq!(archive.light_interval, Duration::from_secs(7 * 24 * 3600));
    }
}
