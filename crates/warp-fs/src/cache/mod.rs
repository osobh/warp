//! Caching layers for warp-fs
//!
//! Provides multi-tier caching for performance:
//! - **Inode cache**: Maps inode numbers to metadata
//! - **Dentry cache**: Maps (parent_ino, name) to child inode
//! - **Data cache**: Caches file content blocks

mod inode_cache;
mod dentry_cache;
mod data_cache;

pub use inode_cache::InodeCache;
pub use dentry_cache::DentryCache;
pub use data_cache::DataCache;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    /// Total cache hits
    pub hits: AtomicU64,
    /// Total cache misses
    pub misses: AtomicU64,
    /// Total evictions
    pub evictions: AtomicU64,
    /// Total insertions
    pub insertions: AtomicU64,
}

impl CacheStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a hit
    pub fn hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a miss
    pub fn miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an eviction
    pub fn evict(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an insertion
    pub fn insert(&self) {
        self.insertions.fetch_add(1, Ordering::Relaxed);
    }

    /// Get hit count
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Get miss count
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// Get eviction count
    pub fn evictions(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }

    /// Calculate hit ratio
    pub fn hit_ratio(&self) -> f64 {
        let hits = self.hits() as f64;
        let total = hits + self.misses() as f64;
        if total == 0.0 {
            0.0
        } else {
            hits / total
        }
    }
}

/// A cached entry with expiration tracking
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    /// The cached value
    pub value: T,
    /// When this entry was inserted
    pub inserted_at: Instant,
    /// Time-to-live
    pub ttl: Duration,
}

impl<T> CacheEntry<T> {
    /// Create a new cache entry
    pub fn new(value: T, ttl: Duration) -> Self {
        Self {
            value,
            inserted_at: Instant::now(),
            ttl,
        }
    }

    /// Check if this entry has expired
    pub fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }

    /// Get remaining TTL
    pub fn remaining_ttl(&self) -> Duration {
        let elapsed = self.inserted_at.elapsed();
        if elapsed > self.ttl {
            Duration::ZERO
        } else {
            self.ttl - elapsed
        }
    }

    /// Refresh the entry (reset expiration)
    pub fn refresh(&mut self) {
        self.inserted_at = Instant::now();
    }
}

/// Combined cache manager
pub struct CacheManager {
    /// Inode cache
    pub inodes: InodeCache,
    /// Directory entry cache
    pub dentries: DentryCache,
    /// Data cache
    pub data: DataCache,
    /// Default TTL
    pub ttl: Duration,
}

impl CacheManager {
    /// Create a new cache manager
    pub fn new(
        inode_capacity: usize,
        dentry_capacity: usize,
        data_capacity_bytes: usize,
        ttl: Duration,
    ) -> Self {
        Self {
            inodes: InodeCache::new(inode_capacity, ttl),
            dentries: DentryCache::new(dentry_capacity, ttl),
            data: DataCache::new(data_capacity_bytes),
            ttl,
        }
    }

    /// Invalidate all caches for an inode
    pub fn invalidate_inode(&self, ino: u64) {
        self.inodes.remove(ino);
        self.data.invalidate_inode(ino);
    }

    /// Invalidate a directory entry
    pub fn invalidate_dentry(&self, parent_ino: u64, name: &str) {
        self.dentries.remove(parent_ino, name);
    }

    /// Clear all caches
    pub fn clear(&self) {
        self.inodes.clear();
        self.dentries.clear();
        self.data.clear();
    }

    /// Get combined statistics
    pub fn stats(&self) -> CombinedCacheStats {
        CombinedCacheStats {
            inode_hits: self.inodes.stats().hits(),
            inode_misses: self.inodes.stats().misses(),
            dentry_hits: self.dentries.stats().hits(),
            dentry_misses: self.dentries.stats().misses(),
            data_hits: self.data.stats().hits(),
            data_misses: self.data.stats().misses(),
            data_bytes_cached: self.data.size(),
        }
    }
}

/// Combined cache statistics
#[derive(Debug, Clone, Default)]
pub struct CombinedCacheStats {
    /// Inode cache hits
    pub inode_hits: u64,
    /// Inode cache misses
    pub inode_misses: u64,
    /// Dentry cache hits
    pub dentry_hits: u64,
    /// Dentry cache misses
    pub dentry_misses: u64,
    /// Data cache hits
    pub data_hits: u64,
    /// Data cache misses
    pub data_misses: u64,
    /// Bytes currently cached
    pub data_bytes_cached: usize,
}
