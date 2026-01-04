//! GPU pager - handles page faults and memory management

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::Semaphore;

use crate::config::GpuMemConfig;
use crate::error::{GpuMemError, GpuMemResult};
use crate::tensor::{TensorHandle, TensorId, TensorLocation};

/// Page state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageState {
    /// Page is resident in GPU memory
    Resident,
    /// Page is being loaded from storage
    Loading,
    /// Page is being evicted to storage
    Evicting,
    /// Page is in storage
    Spilled,
    /// Page is invalid/freed
    Invalid,
}

/// Page fault type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFault {
    /// Tensor not in GPU memory, needs page-in
    NotResident,
    /// Tensor being evicted, need to wait
    Evicting,
    /// Tensor being loaded, need to wait
    Loading,
}

/// Page fault event for tracking
#[derive(Debug, Clone)]
pub struct PageFaultEvent {
    /// Tensor ID
    pub tensor_id: TensorId,
    /// Fault type
    pub fault_type: PageFault,
    /// Time of fault
    pub timestamp: Instant,
    /// Resolution time (if resolved)
    pub resolved_at: Option<Instant>,
}

/// GPU pager statistics
#[derive(Debug, Clone, Default)]
pub struct PagerStats {
    /// Total page faults
    pub page_faults: u64,
    /// Page-ins completed
    pub page_ins: u64,
    /// Page-outs (evictions) completed
    pub page_outs: u64,
    /// Bytes paged in
    pub bytes_paged_in: u64,
    /// Bytes paged out
    pub bytes_paged_out: u64,
    /// Average page-in latency (microseconds)
    pub avg_page_in_latency_us: u64,
    /// Average page-out latency (microseconds)
    pub avg_page_out_latency_us: u64,
}

/// GPU pager - manages tensor memory paging
pub struct GpuPager {
    /// Configuration
    config: GpuMemConfig,
    /// Page states
    page_states: DashMap<TensorId, PageState>,
    /// Current GPU memory usage (bytes)
    gpu_memory_used: AtomicU64,
    /// Semaphore for concurrent page-ins
    page_in_semaphore: Semaphore,
    /// Semaphore for concurrent page-outs
    page_out_semaphore: Semaphore,
    /// Page fault history
    fault_history: RwLock<VecDeque<PageFaultEvent>>,
    /// Statistics
    stats: RwLock<PagerStats>,
}

impl GpuPager {
    /// Create a new GPU pager
    pub fn new(config: GpuMemConfig) -> Self {
        let page_in_semaphore = Semaphore::new(config.max_concurrent_page_ins);
        let page_out_semaphore = Semaphore::new(config.max_concurrent_spills);

        Self {
            config,
            page_states: DashMap::new(),
            gpu_memory_used: AtomicU64::new(0),
            page_in_semaphore,
            page_out_semaphore,
            fault_history: RwLock::new(VecDeque::with_capacity(1000)),
            stats: RwLock::new(PagerStats::default()),
        }
    }

    /// Get current GPU memory usage
    pub fn gpu_memory_used(&self) -> u64 {
        self.gpu_memory_used.load(Ordering::Relaxed)
    }

    /// Get available GPU memory
    pub fn gpu_memory_available(&self) -> u64 {
        let used = self.gpu_memory_used();
        let max = self.config.max_gpu_memory - self.config.reserved_memory;
        max.saturating_sub(used)
    }

    /// Get memory usage ratio (0.0 - 1.0)
    pub fn memory_usage_ratio(&self) -> f64 {
        let max = self.config.max_gpu_memory - self.config.reserved_memory;
        if max == 0 {
            return 1.0;
        }
        self.gpu_memory_used() as f64 / max as f64
    }

    /// Check if we should trigger spilling
    pub fn should_spill(&self) -> bool {
        self.memory_usage_ratio() > self.config.spill_threshold
    }

    /// Check if we can page in tensors
    pub fn can_page_in(&self) -> bool {
        self.memory_usage_ratio() < self.config.page_in_threshold
    }

    /// Register a tensor as resident
    pub fn register_resident(&self, tensor: &TensorHandle) {
        let size = tensor.size_bytes();
        self.page_states.insert(tensor.id(), PageState::Resident);
        self.gpu_memory_used.fetch_add(size, Ordering::Relaxed);
    }

    /// Get page state for a tensor
    pub fn get_state(&self, tensor_id: TensorId) -> Option<PageState> {
        self.page_states.get(&tensor_id).map(|s| *s)
    }

    /// Check if tensor is resident
    pub fn is_resident(&self, tensor_id: TensorId) -> bool {
        matches!(self.get_state(tensor_id), Some(PageState::Resident))
    }

    /// Handle page fault
    pub fn handle_fault(&self, tensor: &TensorHandle) -> GpuMemResult<PageFault> {
        let tensor_id = tensor.id();

        let fault_type = match self.get_state(tensor_id) {
            Some(PageState::Resident) => return Ok(PageFault::NotResident), // Not actually a fault
            Some(PageState::Loading) => PageFault::Loading,
            Some(PageState::Evicting) => PageFault::Evicting,
            Some(PageState::Spilled) | Some(PageState::Invalid) | None => PageFault::NotResident,
        };

        // Record fault
        let event = PageFaultEvent {
            tensor_id,
            fault_type,
            timestamp: Instant::now(),
            resolved_at: None,
        };

        let mut history = self.fault_history.write();
        history.push_back(event);
        if history.len() > 1000 {
            history.pop_front();
        }

        let mut stats = self.stats.write();
        stats.page_faults += 1;

        Ok(fault_type)
    }

    /// Mark tensor as being loaded
    pub fn mark_loading(&self, tensor_id: TensorId) {
        self.page_states.insert(tensor_id, PageState::Loading);
    }

    /// Mark tensor as being evicted
    pub fn mark_evicting(&self, tensor_id: TensorId) {
        self.page_states.insert(tensor_id, PageState::Evicting);
    }

    /// Complete page-in
    pub fn complete_page_in(&self, tensor: &TensorHandle, latency_us: u64) {
        let size = tensor.size_bytes();
        self.page_states.insert(tensor.id(), PageState::Resident);
        self.gpu_memory_used.fetch_add(size, Ordering::Relaxed);
        tensor.set_location(TensorLocation::Gpu);

        let mut stats = self.stats.write();
        stats.page_ins += 1;
        stats.bytes_paged_in += size;
        // Update running average
        let total_latency = stats.avg_page_in_latency_us * (stats.page_ins - 1) + latency_us;
        stats.avg_page_in_latency_us = total_latency / stats.page_ins;
    }

    /// Complete page-out (eviction)
    pub fn complete_page_out(&self, tensor: &TensorHandle, latency_us: u64) {
        let size = tensor.size_bytes();
        self.page_states.insert(tensor.id(), PageState::Spilled);
        self.gpu_memory_used.fetch_sub(size, Ordering::Relaxed);
        tensor.set_location(TensorLocation::Storage);

        let mut stats = self.stats.write();
        stats.page_outs += 1;
        stats.bytes_paged_out += size;
        // Update running average
        if stats.page_outs > 0 {
            let total_latency = stats.avg_page_out_latency_us * (stats.page_outs - 1) + latency_us;
            stats.avg_page_out_latency_us = total_latency / stats.page_outs;
        }
    }

    /// Free a tensor
    pub fn free(&self, tensor: &TensorHandle) {
        let tensor_id = tensor.id();
        if let Some(state) = self.get_state(tensor_id) {
            if state == PageState::Resident {
                self.gpu_memory_used
                    .fetch_sub(tensor.size_bytes(), Ordering::Relaxed);
            }
        }
        self.page_states.insert(tensor_id, PageState::Invalid);
    }

    /// Remove tensor tracking
    pub fn remove(&self, tensor_id: TensorId) {
        self.page_states.remove(&tensor_id);
    }

    /// Acquire page-in permit
    pub async fn acquire_page_in_permit(&self) -> GpuMemResult<tokio::sync::SemaphorePermit<'_>> {
        self.page_in_semaphore
            .acquire()
            .await
            .map_err(|_| GpuMemError::PageInFailed("semaphore closed".to_string()))
    }

    /// Acquire page-out permit
    pub async fn acquire_page_out_permit(&self) -> GpuMemResult<tokio::sync::SemaphorePermit<'_>> {
        self.page_out_semaphore
            .acquire()
            .await
            .map_err(|_| GpuMemError::SpillFailed("semaphore closed".to_string()))
    }

    /// Get statistics
    pub fn stats(&self) -> PagerStats {
        self.stats.read().clone()
    }

    /// Get recent fault history
    pub fn recent_faults(&self, count: usize) -> Vec<PageFaultEvent> {
        let history = self.fault_history.read();
        history.iter().rev().take(count).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::TensorMeta;

    #[test]
    fn test_pager_creation() {
        let config = GpuMemConfig::default();
        let pager = GpuPager::new(config);

        assert_eq!(pager.gpu_memory_used(), 0);
        assert!(pager.gpu_memory_available() > 0);
    }

    #[test]
    fn test_register_resident() {
        let config = GpuMemConfig::default();
        let pager = GpuPager::new(config);

        let meta = TensorMeta::new(vec![1024, 1024], crate::tensor::TensorDtype::Float32);
        let handle = TensorHandle::new(meta);
        let size = handle.size_bytes();

        pager.register_resident(&handle);

        assert!(pager.is_resident(handle.id()));
        assert_eq!(pager.gpu_memory_used(), size);
    }

    #[test]
    fn test_memory_usage_ratio() {
        let mut config = GpuMemConfig::default();
        config.max_gpu_memory = 1000;
        config.reserved_memory = 0;
        let pager = GpuPager::new(config);

        // Manually set usage
        pager.gpu_memory_used.store(500, Ordering::Relaxed);

        assert!((pager.memory_usage_ratio() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_spill_threshold() {
        let mut config = GpuMemConfig::default();
        config.max_gpu_memory = 1000;
        config.reserved_memory = 0;
        config.spill_threshold = 0.8;
        let pager = GpuPager::new(config);

        pager.gpu_memory_used.store(700, Ordering::Relaxed);
        assert!(!pager.should_spill());

        pager.gpu_memory_used.store(850, Ordering::Relaxed);
        assert!(pager.should_spill());
    }
}
