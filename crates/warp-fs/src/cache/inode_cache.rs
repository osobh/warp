//! Inode cache implementation

use super::{CacheEntry, CacheStats};
use crate::inode::Inode;
use crate::metadata::InodeMetadata;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

/// Cache for inode metadata
///
/// Maps inode number to metadata. Uses DashMap for concurrent access.
pub struct InodeCache {
    /// The cache storage
    entries: DashMap<u64, CacheEntry<Arc<RwLock<Inode>>>>,
    /// Maximum capacity
    capacity: usize,
    /// Default TTL
    ttl: Duration,
    /// Statistics
    stats: CacheStats,
}

impl InodeCache {
    /// Create a new inode cache
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: DashMap::with_capacity(capacity),
            capacity,
            ttl,
            stats: CacheStats::new(),
        }
    }

    /// Get an inode from the cache
    pub fn get(&self, ino: u64) -> Option<Arc<RwLock<Inode>>> {
        if let Some(entry) = self.entries.get(&ino) {
            if entry.is_expired() {
                drop(entry); // Release lock before removal
                self.entries.remove(&ino);
                self.stats.miss();
                return None;
            }
            self.stats.hit();
            Some(entry.value.clone())
        } else {
            self.stats.miss();
            None
        }
    }

    /// Insert or update an inode in the cache
    pub fn insert(&self, ino: u64, inode: Inode) -> Arc<RwLock<Inode>> {
        self.maybe_evict();

        let arc = Arc::new(RwLock::new(inode));
        let entry = CacheEntry::new(arc.clone(), self.ttl);
        self.entries.insert(ino, entry);
        self.stats.insert();
        arc
    }

    /// Insert an already-wrapped inode
    pub fn insert_arc(&self, ino: u64, inode: Arc<RwLock<Inode>>) {
        self.maybe_evict();

        let entry = CacheEntry::new(inode, self.ttl);
        self.entries.insert(ino, entry);
        self.stats.insert();
    }

    /// Get or insert an inode
    pub fn get_or_insert<F>(&self, ino: u64, f: F) -> Arc<RwLock<Inode>>
    where
        F: FnOnce() -> Option<Inode>,
    {
        if let Some(inode) = self.get(ino) {
            return inode;
        }

        // Cache miss - try to load
        if let Some(inode) = f() {
            self.insert(ino, inode)
        } else {
            // Return a dummy that will cause ENOENT
            // This shouldn't normally happen
            let dummy = Inode::new(InodeMetadata::new_file(ino, 0, 0, 0));
            Arc::new(RwLock::new(dummy))
        }
    }

    /// Remove an inode from the cache
    pub fn remove(&self, ino: u64) -> Option<Arc<RwLock<Inode>>> {
        self.entries.remove(&ino).map(|(_, e)| e.value)
    }

    /// Check if an inode is cached
    pub fn contains(&self, ino: u64) -> bool {
        self.entries.contains_key(&ino)
    }

    /// Get the number of cached entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear the cache
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Maybe evict entries if over capacity
    fn maybe_evict(&self) {
        if self.entries.len() >= self.capacity {
            // Simple eviction: remove expired entries first
            let mut to_remove = Vec::new();
            for entry in self.entries.iter() {
                if entry.value().is_expired() {
                    to_remove.push(*entry.key());
                }
            }

            for ino in to_remove {
                self.entries.remove(&ino);
                self.stats.evict();
            }

            // If still over capacity, remove oldest entries
            // This is a simple strategy; could be improved with LRU
            while self.entries.len() >= self.capacity {
                if let Some(entry) = self.entries.iter().next() {
                    let ino = *entry.key();
                    drop(entry);
                    self.entries.remove(&ino);
                    self.stats.evict();
                } else {
                    break;
                }
            }
        }
    }

    /// Refresh an entry's TTL
    pub fn refresh(&self, ino: u64) {
        if let Some(mut entry) = self.entries.get_mut(&ino) {
            entry.refresh();
        }
    }

    /// Get all dirty inodes
    pub fn get_dirty(&self) -> Vec<(u64, Arc<RwLock<Inode>>)> {
        let mut dirty = Vec::new();
        for entry in self.entries.iter() {
            let cache_entry = entry.value();
            let inode = cache_entry.value.read();
            if inode.is_dirty() {
                dirty.push((*entry.key(), cache_entry.value.clone()));
            }
        }
        dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::InodeMetadata;

    #[test]
    fn test_basic_operations() {
        let cache = InodeCache::new(100, Duration::from_secs(60));

        let meta = InodeMetadata::new_file(42, 0o644, 1000, 1000);
        let inode = Inode::new(meta);

        cache.insert(42, inode);
        assert!(cache.contains(42));

        let retrieved = cache.get(42).unwrap();
        assert_eq!(retrieved.read().ino(), 42);

        cache.remove(42);
        assert!(!cache.contains(42));
    }

    #[test]
    fn test_stats() {
        let cache = InodeCache::new(100, Duration::from_secs(60));

        // Miss
        assert!(cache.get(1).is_none());
        assert_eq!(cache.stats().misses(), 1);

        // Insert and hit
        let meta = InodeMetadata::new_file(1, 0o644, 1000, 1000);
        cache.insert(1, Inode::new(meta));
        assert!(cache.get(1).is_some());
        assert_eq!(cache.stats().hits(), 1);
    }
}
