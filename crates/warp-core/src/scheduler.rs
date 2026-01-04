//! Scheduler module

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Chunk scheduling priority
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkPriority {
    /// Chunk index
    pub chunk_index: u64,
    /// Priority score (higher = more urgent)
    pub score: i32,
}

impl Ord for ChunkPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score.cmp(&other.score)
    }
}

impl PartialOrd for ChunkPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Chunk scheduler with priority queue
pub struct ChunkScheduler {
    queue: BinaryHeap<ChunkPriority>,
}

impl ChunkScheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
        }
    }

    /// Add a chunk to the schedule
    pub fn schedule(&mut self, chunk_index: u64, score: i32) {
        self.queue.push(ChunkPriority { chunk_index, score });
    }

    /// Get the next chunk to process
    pub fn next(&mut self) -> Option<u64> {
        self.queue.pop().map(|p| p.chunk_index)
    }

    /// Check if scheduler is empty
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Default for ChunkScheduler {
    fn default() -> Self {
        Self::new()
    }
}
