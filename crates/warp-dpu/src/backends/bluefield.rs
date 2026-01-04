//! NVIDIA BlueField DPU backend via DOCA SDK
//!
//! This module is a placeholder for BlueField DPU integration.
//! Enable with `--features bluefield` once DOCA SDK bindings are implemented.

use crate::{DpuBackend, DpuCapabilities, DpuError, DpuResult, OffloadRequest, OffloadResult};
use async_trait::async_trait;

/// BlueField DPU backend implementation
pub struct BlueFieldBackend {
    // Reserved for DOCA context
    _private: (),
}

impl BlueFieldBackend {
    /// Create a new BlueField backend
    ///
    /// # Errors
    /// Returns error if DOCA SDK initialization fails
    pub fn new() -> DpuResult<Self> {
        Err(DpuError::NotSupported(
            "BlueField backend not yet implemented".to_string(),
        ))
    }
}

#[async_trait]
impl DpuBackend for BlueFieldBackend {
    fn name(&self) -> &str {
        "bluefield"
    }

    fn capabilities(&self) -> DpuCapabilities {
        DpuCapabilities::default()
    }

    async fn offload(&self, _request: OffloadRequest) -> DpuResult<OffloadResult> {
        Err(DpuError::NotSupported(
            "BlueField backend not yet implemented".to_string(),
        ))
    }
}
