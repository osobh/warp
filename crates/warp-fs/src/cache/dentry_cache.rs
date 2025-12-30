//! Directory entry cache implementation

use super::{CacheEntry, CacheStats};
use crate::metadata::FileType;
use dashmap::DashMap;
use std::time::Duration;

/// Key for dentry cache: (parent_ino, name)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct DentryKey {
    parent_ino: u64,
    name: String,
}

impl DentryKey {
    fn new(parent_ino: u64, name: &str) -> Self {
        Self {
            parent_ino,
            name: name.to_string(),
        }
    }
}

/// Cached directory entry value
#[derive(Debug, Clone)]
pub struct DentryValue {
    /// Child inode number
    pub ino: u64,
    /// File type
    pub file_type: FileType,
}

/// Cache for directory entries
///
/// Maps (parent_ino, name) to child inode number
pub struct DentryCache {
    /// The cache storage
    entries: DashMap<DentryKey, CacheEntry<DentryValue>>,
    /// Maximum capacity
    capacity: usize,
    /// Default TTL
    ttl: Duration,
    /// Statistics
    stats: CacheStats,
    /// Negative cache (non-existent entries)
    negative: DashMap<DentryKey, CacheEntry<()>>,
}

impl DentryCache {
    /// Create a new dentry cache
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: DashMap::with_capacity(capacity),
            capacity,
            ttl,
            stats: CacheStats::new(),
            negative: DashMap::with_capacity(capacity / 10),
        }
    }

    /// Look up a directory entry
    pub fn get(&self, parent_ino: u64, name: &str) -> Option<DentryValue> {
        let key = DentryKey::new(parent_ino, name);

        // Check negative cache first
        if let Some(neg) = self.negative.get(&key) {
            if !neg.is_expired() {
                self.stats.hit();
                return None; // Known to not exist
            } else {
                drop(neg);
                self.negative.remove(&key);
            }
        }

        if let Some(entry) = self.entries.get(&key) {
            if entry.is_expired() {
                drop(entry);
                self.entries.remove(&key);
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

    /// Insert a directory entry
    pub fn insert(&self, parent_ino: u64, name: &str, ino: u64, file_type: FileType) {
        self.maybe_evict();

        let key = DentryKey::new(parent_ino, name);

        // Remove from negative cache if present
        self.negative.remove(&key);

        let value = DentryValue { ino, file_type };
        let entry = CacheEntry::new(value, self.ttl);
        self.entries.insert(key, entry);
        self.stats.insert();
    }

    /// Insert a negative entry (known to not exist)
    pub fn insert_negative(&self, parent_ino: u64, name: &str) {
        let key = DentryKey::new(parent_ino, name);
        // Shorter TTL for negative entries
        let entry = CacheEntry::new((), self.ttl / 4);
        self.negative.insert(key, entry);
    }

    /// Remove a directory entry
    pub fn remove(&self, parent_ino: u64, name: &str) {
        let key = DentryKey::new(parent_ino, name);
        self.entries.remove(&key);
        self.negative.remove(&key);
    }

    /// Invalidate all entries for a parent directory
    pub fn invalidate_parent(&self, parent_ino: u64) {
        let to_remove: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.key().parent_ino == parent_ino)
            .map(|e| e.key().clone())
            .collect();

        for key in to_remove {
            self.entries.remove(&key);
        }

        let neg_remove: Vec<_> = self
            .negative
            .iter()
            .filter(|e| e.key().parent_ino == parent_ino)
            .map(|e| e.key().clone())
            .collect();

        for key in neg_remove {
            self.negative.remove(&key);
        }
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
        self.negative.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Maybe evict entries if over capacity
    fn maybe_evict(&self) {
        if self.entries.len() >= self.capacity {
            // Remove expired entries
            let mut to_remove = Vec::new();
            for entry in self.entries.iter() {
                if entry.value().is_expired() {
                    to_remove.push(entry.key().clone());
                }
            }

            for key in to_remove {
                self.entries.remove(&key);
                self.stats.evict();
            }

            // Simple eviction if still over capacity
            while self.entries.len() >= self.capacity {
                if let Some(entry) = self.entries.iter().next() {
                    let key = entry.key().clone();
                    drop(entry);
                    self.entries.remove(&key);
                    self.stats.evict();
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let cache = DentryCache::new(100, Duration::from_secs(60));

        cache.insert(1, "file.txt", 2, FileType::RegularFile);

        let result = cache.get(1, "file.txt").unwrap();
        assert_eq!(result.ino, 2);
        assert_eq!(result.file_type, FileType::RegularFile);

        cache.remove(1, "file.txt");
        assert!(cache.get(1, "file.txt").is_none());
    }

    #[test]
    fn test_negative_cache() {
        let cache = DentryCache::new(100, Duration::from_secs(60));

        // First lookup is a miss
        assert!(cache.get(1, "noexist").is_none());
        assert_eq!(cache.stats().misses(), 1);

        // Add to negative cache
        cache.insert_negative(1, "noexist");

        // Second lookup is a hit (from negative cache)
        assert!(cache.get(1, "noexist").is_none());
        assert_eq!(cache.stats().hits(), 1);
    }

    #[test]
    fn test_invalidate_parent() {
        let cache = DentryCache::new(100, Duration::from_secs(60));

        cache.insert(1, "a", 2, FileType::RegularFile);
        cache.insert(1, "b", 3, FileType::RegularFile);
        cache.insert(2, "c", 4, FileType::RegularFile);

        cache.invalidate_parent(1);

        assert!(cache.get(1, "a").is_none());
        assert!(cache.get(1, "b").is_none());
        assert!(cache.get(2, "c").is_some()); // Different parent
    }
}
