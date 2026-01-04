//! GPU memory pool - main interface for tensor allocation

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use parking_lot::RwLock;

use warp_store::Store;

use crate::cache::TensorCache;
use crate::config::GpuMemConfig;
use crate::error::{GpuMemError, GpuMemResult};
use crate::pager::{GpuPager, PageState};
use crate::prefetch::{PrefetchHint, Prefetcher};
use crate::spill::SpillManager;
use crate::tensor::{TensorDtype, TensorHandle, TensorId, TensorMeta};

/// Pool statistics
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total allocations
    pub allocations: u64,
    /// Total deallocations
    pub deallocations: u64,
    /// Current tensor count
    pub tensor_count: usize,
    /// GPU memory used (bytes)
    pub gpu_memory_used: u64,
    /// GPU memory available (bytes)
    pub gpu_memory_available: u64,
    /// Spilled tensor count
    pub spilled_count: usize,
    /// Spilled bytes
    pub spilled_bytes: u64,
}

/// GPU memory pool
pub struct GpuMemoryPool {
    /// Configuration
    config: GpuMemConfig,
    /// Storage backend
    store: Arc<Store>,
    /// All tensors (by ID)
    tensors: DashMap<TensorId, Arc<TensorHandle>>,
    /// GPU pager
    pager: Arc<GpuPager>,
    /// Tensor cache
    cache: Arc<TensorCache>,
    /// Prefetcher
    prefetcher: Arc<Prefetcher>,
    /// Spill manager
    spill_manager: Arc<SpillManager>,
    /// Statistics
    stats: RwLock<PoolStats>,
}

impl GpuMemoryPool {
    /// Create a new GPU memory pool
    pub fn new(store: Arc<Store>, config: GpuMemConfig) -> Self {
        let pager = Arc::new(GpuPager::new(config.clone()));
        let cache = Arc::new(TensorCache::new(config.cache.clone()));
        let prefetcher = Arc::new(Prefetcher::new(
            config.prefetch_strategy,
            config.prefetch_lookahead,
        ));
        let spill_manager = Arc::new(SpillManager::new(config.clone(), "__gpu_spill__"));

        Self {
            config,
            store,
            tensors: DashMap::new(),
            pager,
            cache,
            prefetcher,
            spill_manager,
            stats: RwLock::new(PoolStats::default()),
        }
    }

    /// Allocate a new tensor
    pub async fn allocate(&self, meta: TensorMeta) -> GpuMemResult<Arc<TensorHandle>> {
        let size = meta.size_bytes;

        // Check if we need to spill first
        if self.pager.gpu_memory_available() < size {
            self.trigger_spill(size).await?;
        }

        // Create tensor handle
        let handle = Arc::new(TensorHandle::new(meta));
        let tensor_id = handle.id();

        // Register with pager
        self.pager.register_resident(&handle);

        // Add to tensors map
        self.tensors.insert(tensor_id, handle.clone());

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.allocations += 1;
            stats.tensor_count = self.tensors.len();
            stats.gpu_memory_used = self.pager.gpu_memory_used();
            stats.gpu_memory_available = self.pager.gpu_memory_available();
        }

        Ok(handle)
    }

    /// Allocate a tensor with specific shape and dtype
    pub async fn allocate_tensor<T: TensorType>(
        &self,
        shape: &[usize],
    ) -> GpuMemResult<Arc<TensorHandle>> {
        let meta = TensorMeta::new(shape.to_vec(), T::dtype());
        self.allocate(meta).await
    }

    /// Get a tensor by ID
    pub fn get(&self, tensor_id: TensorId) -> Option<Arc<TensorHandle>> {
        self.tensors.get(&tensor_id).map(|t| t.clone())
    }

    /// Access a tensor (may trigger page-in)
    pub async fn access(&self, tensor_id: TensorId) -> GpuMemResult<Arc<TensorHandle>> {
        let handle = self
            .get(tensor_id)
            .ok_or_else(|| GpuMemError::TensorNotFound(tensor_id.to_string()))?;

        // Record access for prefetcher
        self.prefetcher.record_access(&handle);

        // Check if tensor needs to be paged in
        if !handle.is_resident() {
            self.page_in(&handle).await?;
        }

        // Record access
        handle.record_access();

        // Trigger prefetch for predicted tensors
        let hints = self.prefetcher.get_hints(&handle);
        self.prefetch(hints).await;

        Ok(handle)
    }

    /// Free a tensor
    pub async fn free(&self, tensor_id: TensorId) -> GpuMemResult<()> {
        let handle = self
            .tensors
            .remove(&tensor_id)
            .map(|(_, h)| h)
            .ok_or_else(|| GpuMemError::TensorNotFound(tensor_id.to_string()))?;

        // Free from pager
        self.pager.free(&handle);

        // Remove from cache
        self.cache.remove(tensor_id);

        // Remove spill record if any
        self.spill_manager.remove(tensor_id);

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.deallocations += 1;
            stats.tensor_count = self.tensors.len();
            stats.gpu_memory_used = self.pager.gpu_memory_used();
            stats.gpu_memory_available = self.pager.gpu_memory_available();
        }

        Ok(())
    }

    /// Trigger spill to free memory
    async fn trigger_spill(&self, bytes_needed: u64) -> GpuMemResult<()> {
        // Select tensors to evict
        let to_evict = self.cache.select_for_eviction(bytes_needed);

        if to_evict.is_empty() {
            // No candidates - try to evict any non-pinned tensor
            let mut candidates: Vec<(TensorId, f64)> = self
                .tensors
                .iter()
                .filter(|entry| {
                    let handle = entry.value();
                    !handle.is_pinned() && handle.is_resident()
                })
                .map(|entry| {
                    let handle = entry.value();
                    let priority = self
                        .spill_manager
                        .calculate_priority(handle, handle.time_since_access().as_millis() as u64);
                    (*entry.key(), priority)
                })
                .collect();

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let mut freed = 0u64;
            for (tensor_id, _) in candidates {
                if freed >= bytes_needed {
                    break;
                }
                if let Some(handle) = self.get(tensor_id) {
                    freed += handle.size_bytes();
                    self.spill_tensor(&handle).await?;
                }
            }

            if freed < bytes_needed {
                return Err(GpuMemError::OutOfMemory {
                    requested: bytes_needed,
                    available: self.pager.gpu_memory_available(),
                });
            }
        } else {
            for tensor_id in &to_evict {
                if let Some(handle) = self.get(*tensor_id) {
                    self.spill_tensor(&handle).await?;
                }
            }
        }

        self.cache.record_eviction(to_evict.len());
        Ok(())
    }

    /// Spill a tensor to storage
    async fn spill_tensor(&self, handle: &TensorHandle) -> GpuMemResult<()> {
        let _permit = self.spill_manager.acquire_spill_permit().await?;
        let start = Instant::now();

        // Mark as evicting
        self.pager.mark_evicting(handle.id());
        handle.set_location(crate::tensor::TensorLocation::Transferring);

        // Generate storage key
        let storage_key = self.spill_manager.generate_storage_key(handle);

        // TODO: Actually write tensor data to storage
        // For now, just simulate the spill
        // let data = handle.read_data();
        // self.store.put(&ObjectKey::new("__gpu_spill__", &storage_key)?, data).await?;

        let latency_us = start.elapsed().as_micros() as u64;

        // Record spill
        self.spill_manager
            .record_spill(handle, storage_key, vec![], latency_us);

        // Complete page-out
        self.pager.complete_page_out(handle, latency_us);

        Ok(())
    }

    /// Page in a tensor from storage
    async fn page_in(&self, handle: &TensorHandle) -> GpuMemResult<()> {
        let _permit = self.pager.acquire_page_in_permit().await?;
        let start = Instant::now();

        // Check if we need to free memory first
        if self.pager.gpu_memory_available() < handle.size_bytes() {
            self.trigger_spill(handle.size_bytes()).await?;
        }

        // Mark as loading
        self.pager.mark_loading(handle.id());
        handle.set_location(crate::tensor::TensorLocation::Transferring);

        // TODO: Actually read tensor data from storage
        // let storage_key = self.spill_manager.get_storage_key(handle.id())
        //     .ok_or_else(|| GpuMemError::TensorNotFound(handle.id().to_string()))?;
        // let data = self.store.get(&ObjectKey::new("__gpu_spill__", &storage_key)?).await?;
        // handle.write_data(data);

        let latency_us = start.elapsed().as_micros() as u64;

        // Complete page-in
        self.pager.complete_page_in(handle, latency_us);

        // Record restore
        self.spill_manager.record_restore(handle.id(), latency_us);

        Ok(())
    }

    /// Prefetch tensors based on hints
    async fn prefetch(&self, hints: Vec<PrefetchHint>) {
        for hint in hints {
            if let Some(handle) = self.get(hint.tensor_id) {
                if !handle.is_resident() {
                    self.prefetcher.register_prefetch(hint.tensor_id);
                    // Fire and forget prefetch
                    let pool = self.clone_for_prefetch();
                    let tensor_id = hint.tensor_id;
                    tokio::spawn(async move {
                        let _ = pool.page_in_by_id(tensor_id).await;
                    });
                }
            }
        }
    }

    /// Page in by tensor ID
    async fn page_in_by_id(&self, tensor_id: TensorId) -> GpuMemResult<()> {
        if let Some(handle) = self.get(tensor_id) {
            self.page_in(&handle).await
        } else {
            Err(GpuMemError::TensorNotFound(tensor_id.to_string()))
        }
    }

    /// Clone pool for prefetch operations
    fn clone_for_prefetch(&self) -> GpuMemoryPoolHandle {
        GpuMemoryPoolHandle {
            tensors: self.tensors.clone(),
            pager: self.pager.clone(),
            spill_manager: self.spill_manager.clone(),
            prefetcher: self.prefetcher.clone(),
        }
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let mut stats = self.stats.read().clone();
        stats.gpu_memory_used = self.pager.gpu_memory_used();
        stats.gpu_memory_available = self.pager.gpu_memory_available();
        stats.spilled_count = self.spill_manager.list_spilled().len();
        stats.spilled_bytes = self.spill_manager.total_spilled_bytes();
        stats
    }

    /// Get GPU memory usage ratio
    pub fn memory_usage_ratio(&self) -> f64 {
        self.pager.memory_usage_ratio()
    }

    /// Pin a tensor (prevent spilling)
    pub fn pin(&self, tensor_id: TensorId) -> GpuMemResult<()> {
        let handle = self
            .get(tensor_id)
            .ok_or_else(|| GpuMemError::TensorNotFound(tensor_id.to_string()))?;
        handle.pin();
        Ok(())
    }

    /// Unpin a tensor
    pub fn unpin(&self, tensor_id: TensorId) -> GpuMemResult<()> {
        let handle = self
            .get(tensor_id)
            .ok_or_else(|| GpuMemError::TensorNotFound(tensor_id.to_string()))?;
        handle.unpin();
        Ok(())
    }
}

/// Handle for prefetch operations
struct GpuMemoryPoolHandle {
    tensors: DashMap<TensorId, Arc<TensorHandle>>,
    pager: Arc<GpuPager>,
    spill_manager: Arc<SpillManager>,
    prefetcher: Arc<Prefetcher>,
}

impl GpuMemoryPoolHandle {
    async fn page_in_by_id(&self, tensor_id: TensorId) -> GpuMemResult<()> {
        // Simplified page-in for prefetch
        Ok(())
    }
}

/// Trait for tensor types
pub trait TensorType {
    /// Get the dtype
    fn dtype() -> TensorDtype;
}

impl TensorType for f32 {
    fn dtype() -> TensorDtype {
        TensorDtype::Float32
    }
}

impl TensorType for f64 {
    fn dtype() -> TensorDtype {
        TensorDtype::Float64
    }
}

impl TensorType for i32 {
    fn dtype() -> TensorDtype {
        TensorDtype::Int32
    }
}

impl TensorType for i64 {
    fn dtype() -> TensorDtype {
        TensorDtype::Int64
    }
}

impl TensorType for i8 {
    fn dtype() -> TensorDtype {
        TensorDtype::Int8
    }
}

impl TensorType for u8 {
    fn dtype() -> TensorDtype {
        TensorDtype::UInt8
    }
}

impl TensorType for bool {
    fn dtype() -> TensorDtype {
        TensorDtype::Bool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp_store::StoreConfig;

    async fn create_test_pool() -> GpuMemoryPool {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_config = StoreConfig {
            root_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let store = Arc::new(Store::new(store_config).await.unwrap());
        let config = GpuMemConfig::default();
        GpuMemoryPool::new(store, config)
    }

    #[tokio::test]
    async fn test_pool_allocation() {
        let pool = create_test_pool().await;

        let handle = pool.allocate_tensor::<f32>(&[1024, 1024]).await.unwrap();

        assert!(handle.is_resident());
        assert_eq!(handle.size_bytes(), 1024 * 1024 * 4);

        let stats = pool.stats();
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.tensor_count, 1);
    }

    #[tokio::test]
    async fn test_pool_free() {
        let pool = create_test_pool().await;

        let handle = pool.allocate_tensor::<f32>(&[1024, 1024]).await.unwrap();
        let tensor_id = handle.id();

        pool.free(tensor_id).await.unwrap();

        let stats = pool.stats();
        assert_eq!(stats.deallocations, 1);
        assert_eq!(stats.tensor_count, 0);
    }

    #[tokio::test]
    async fn test_pool_pin_unpin() {
        let pool = create_test_pool().await;

        let handle = pool.allocate_tensor::<f32>(&[1024]).await.unwrap();
        let tensor_id = handle.id();

        pool.pin(tensor_id).unwrap();
        assert!(handle.is_pinned());

        pool.unpin(tensor_id).unwrap();
        assert!(!handle.is_pinned());
    }
}
