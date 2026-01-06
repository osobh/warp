//! Data cache implementation

use super::CacheStats;
use bytes::Bytes;
use dashmap::DashMap;
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Block size for caching (64KB)
pub const BLOCK_SIZE: usize = 64 * 1024;

/// Key for data cache: (inode, block_index)
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct DataKey {
    ino: u64,
    block: u64,
}

impl DataKey {
    fn new(ino: u64, offset: u64) -> Self {
        Self {
            ino,
            block: offset / BLOCK_SIZE as u64,
        }
    }

    fn from_block(ino: u64, block: u64) -> Self {
        Self { ino, block }
    }
}

/// A cached data block
#[derive(Debug, Clone)]
struct CachedBlock {
    /// The data
    data: Bytes,
    /// Whether this block has been modified
    dirty: bool,
}

/// Cache for file data
///
/// Uses LRU eviction and tracks total size.
pub struct DataCache {
    /// LRU cache protected by mutex
    cache: Mutex<LruCache<DataKey, CachedBlock>>,
    /// Maximum size in bytes
    max_size: usize,
    /// Current size in bytes
    current_size: AtomicUsize,
    /// Statistics
    stats: CacheStats,
    /// Index: inode -> blocks (for bulk invalidation)
    inode_blocks: DashMap<u64, Vec<u64>>,
}

impl DataCache {
    /// Create a new data cache with the given capacity in bytes
    pub fn new(max_size: usize) -> Self {
        let max_blocks = max_size / BLOCK_SIZE;
        let capacity = NonZeroUsize::new(max_blocks.max(1)).unwrap();

        Self {
            cache: Mutex::new(LruCache::new(capacity)),
            max_size,
            current_size: AtomicUsize::new(0),
            stats: CacheStats::new(),
            inode_blocks: DashMap::new(),
        }
    }

    /// Read data from cache
    ///
    /// Returns (data, hit) where hit is true if the data was in cache
    pub fn read(&self, ino: u64, offset: u64, len: usize) -> Option<Vec<u8>> {
        let start_block = offset / BLOCK_SIZE as u64;
        let end_offset = offset + len as u64;
        let end_block = end_offset.div_ceil(BLOCK_SIZE as u64);

        let mut result = Vec::with_capacity(len);
        let mut cache = self.cache.lock();

        for block_idx in start_block..end_block {
            let key = DataKey::from_block(ino, block_idx);

            if let Some(block) = cache.get(&key) {
                self.stats.hit();

                let block_start = block_idx * BLOCK_SIZE as u64;
                let block_end = block_start + block.data.len() as u64;

                // Calculate the portion of this block we need
                let read_start = if offset > block_start {
                    (offset - block_start) as usize
                } else {
                    0
                };

                let read_end = if end_offset < block_end {
                    (end_offset - block_start) as usize
                } else {
                    block.data.len()
                };

                if read_start < read_end && read_end <= block.data.len() {
                    result.extend_from_slice(&block.data[read_start..read_end]);
                }
            } else {
                self.stats.miss();
                return None; // Cache miss - caller should fetch from storage
            }
        }

        if result.len() >= len {
            result.truncate(len);
            Some(result)
        } else {
            None // Incomplete data
        }
    }

    /// Insert data into cache
    pub fn insert(&self, ino: u64, offset: u64, data: &[u8]) {
        let start_block = offset / BLOCK_SIZE as u64;

        let mut pos = 0;
        let mut block_idx = start_block;
        let block_offset = (offset % BLOCK_SIZE as u64) as usize;

        // Handle first partial block
        if block_offset > 0 {
            let end = (BLOCK_SIZE - block_offset).min(data.len());
            self.insert_block(ino, block_idx, block_offset, &data[..end]);
            pos = end;
            block_idx += 1;
        }

        // Handle full blocks
        while pos + BLOCK_SIZE <= data.len() {
            self.insert_block(ino, block_idx, 0, &data[pos..pos + BLOCK_SIZE]);
            pos += BLOCK_SIZE;
            block_idx += 1;
        }

        // Handle last partial block
        if pos < data.len() {
            self.insert_block(ino, block_idx, 0, &data[pos..]);
        }
    }

    /// Insert a single block
    fn insert_block(&self, ino: u64, block_idx: u64, offset: usize, data: &[u8]) {
        let key = DataKey::from_block(ino, block_idx);
        let mut cache = self.cache.lock();

        // Get or create the block
        let block = if let Some(existing) = cache.get_mut(&key) {
            // Merge with existing data
            let mut buf = existing.data.to_vec();
            if offset + data.len() > buf.len() {
                buf.resize(offset + data.len(), 0);
            }
            buf[offset..offset + data.len()].copy_from_slice(data);
            existing.data = Bytes::from(buf);
            return;
        } else {
            // New block
            let mut buf = vec![0u8; offset + data.len()];
            buf[offset..offset + data.len()].copy_from_slice(data);
            CachedBlock {
                data: Bytes::from(buf),
                dirty: false,
            }
        };

        // Evict if needed
        while self.current_size.load(Ordering::Relaxed) + block.data.len() > self.max_size {
            if let Some((_, evicted)) = cache.pop_lru() {
                self.current_size
                    .fetch_sub(evicted.data.len(), Ordering::Relaxed);
                self.stats.evict();
            } else {
                break;
            }
        }

        // Insert
        self.current_size
            .fetch_add(block.data.len(), Ordering::Relaxed);
        cache.put(key, block);
        self.stats.insert();

        // Track for bulk invalidation
        self.inode_blocks.entry(ino).or_default().push(block_idx);
    }

    /// Invalidate all cached data for an inode
    pub fn invalidate_inode(&self, ino: u64) {
        if let Some((_, blocks)) = self.inode_blocks.remove(&ino) {
            let mut cache = self.cache.lock();
            for block_idx in blocks {
                let key = DataKey::from_block(ino, block_idx);
                if let Some(evicted) = cache.pop(&key) {
                    self.current_size
                        .fetch_sub(evicted.data.len(), Ordering::Relaxed);
                }
            }
        }
    }

    /// Invalidate a range of data
    pub fn invalidate_range(&self, ino: u64, offset: u64, len: u64) {
        let start_block = offset / BLOCK_SIZE as u64;
        let end_block = (offset + len).div_ceil(BLOCK_SIZE as u64);

        let mut cache = self.cache.lock();
        for block_idx in start_block..end_block {
            let key = DataKey::from_block(ino, block_idx);
            if let Some(evicted) = cache.pop(&key) {
                self.current_size
                    .fetch_sub(evicted.data.len(), Ordering::Relaxed);
            }
        }
    }

    /// Get current cache size in bytes
    pub fn size(&self) -> usize {
        self.current_size.load(Ordering::Relaxed)
    }

    /// Get number of cached blocks
    pub fn block_count(&self) -> usize {
        self.cache.lock().len()
    }

    /// Clear the entire cache
    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
        self.current_size.store(0, Ordering::Relaxed);
        self.inode_blocks.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Get all dirty blocks for an inode
    pub fn get_dirty_blocks(&self, ino: u64) -> Vec<(u64, Bytes)> {
        let mut result = Vec::new();

        if let Some(blocks) = self.inode_blocks.get(&ino) {
            let cache = self.cache.lock();
            for &block_idx in blocks.value() {
                let key = DataKey::from_block(ino, block_idx);
                if let Some(block) = cache.peek(&key) {
                    if block.dirty {
                        result.push((block_idx * BLOCK_SIZE as u64, block.data.clone()));
                    }
                }
            }
        }

        result
    }

    /// Mark blocks as clean
    pub fn mark_clean(&self, ino: u64) {
        if let Some(blocks) = self.inode_blocks.get(&ino) {
            let mut cache = self.cache.lock();
            for &block_idx in blocks.value() {
                let key = DataKey::from_block(ino, block_idx);
                if let Some(block) = cache.get_mut(&key) {
                    block.dirty = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_read_write() {
        let cache = DataCache::new(1024 * 1024); // 1MB

        let data = b"Hello, World!";
        cache.insert(1, 0, data);

        let result = cache.read(1, 0, data.len()).unwrap();
        assert_eq!(&result, data);
    }

    #[test]
    fn test_partial_read() {
        let cache = DataCache::new(1024 * 1024);

        let data = b"Hello, World!";
        cache.insert(1, 0, data);

        let result = cache.read(1, 7, 5).unwrap();
        assert_eq!(&result, b"World");
    }

    #[test]
    fn test_cache_miss() {
        let cache = DataCache::new(1024 * 1024);

        assert!(cache.read(1, 0, 10).is_none());
        assert_eq!(cache.stats().misses(), 1);
    }

    #[test]
    fn test_invalidate() {
        let cache = DataCache::new(1024 * 1024);

        cache.insert(1, 0, b"test data");
        assert!(cache.read(1, 0, 9).is_some());

        cache.invalidate_inode(1);
        assert!(cache.read(1, 0, 9).is_none());
    }
}
