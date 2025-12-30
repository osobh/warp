//! Scrubbing Service for WARP Storage
//!
//! Background service that verifies data integrity by:
//! - Periodically scanning all stored data
//! - Verifying checksums against stored metadata
//! - Detecting silent data corruption (bitrot)
//! - Quarantining bad blocks and triggering repairs
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │              Scrub Daemon                           │
//! ├─────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐                 │
//! │  │  Scheduler  │  │  Scrub Queue │                 │
//! │  │  (cron-like │──▶│  (bucket/   │                 │
//! │  │   timing)   │  │   object)    │                 │
//! │  └─────────────┘  └──────┬───────┘                 │
//! │                          │                          │
//! │  ┌───────────────────────▼───────────────────────┐ │
//! │  │           Scrub Workers                       │ │
//! │  │  - Light scrub: metadata only                 │ │
//! │  │  - Deep scrub: full data verification         │ │
//! │  │  - GPU-accelerated checksums                  │ │
//! │  └───────────────────────────────────────────────┘ │
//! │                          │                          │
//! │  ┌───────────────────────▼───────────────────────┐ │
//! │  │           Quarantine Manager                  │ │
//! │  │  - Bad block tracking                         │ │
//! │  │  - Repair queue integration                   │ │
//! │  └───────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────┘
//! ```

mod daemon;
mod metrics;
mod quarantine;
mod scheduler;

pub use daemon::{ScrubDaemon, ScrubConfig, ScrubJob, ScrubResult};
pub use metrics::{ScrubMetrics, ScrubStats};
pub use scheduler::{ScrubScheduler, ScrubSchedule};
pub use quarantine::{QuarantineManager, QuarantinedBlock, QuarantineReason};
