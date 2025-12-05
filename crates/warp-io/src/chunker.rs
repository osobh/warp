//! Content-defined chunking using Buzhash

use std::io::Read;

/// Chunker configuration
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Minimum chunk size
    pub min_size: usize,
    /// Target chunk size
    pub target_size: usize,
    /// Maximum chunk size
    pub max_size: usize,
    /// Window size for rolling hash
    pub window_size: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            min_size: 1024 * 1024,      // 1MB
            target_size: 4 * 1024 * 1024, // 4MB
            max_size: 16 * 1024 * 1024,  // 16MB
            window_size: 48,
        }
    }
}

/// Content-defined chunker using Buzhash rolling hash
pub struct Chunker {
    config: ChunkerConfig,
    mask: u64,
    table: [u64; 256],
}

impl Chunker {
    /// Create a new chunker
    pub fn new(config: ChunkerConfig) -> Self {
        // Calculate mask for target size (find boundary when hash & mask == 0)
        let bits = (config.target_size as f64).log2() as u32;
        let mask = (1u64 << bits) - 1;

        // Initialize Buzhash table with pseudo-random values
        let mut table = [0u64; 256];
        let mut state = 0x123456789ABCDEFu64;
        for entry in &mut table {
            // Simple xorshift
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *entry = state;
        }

        Self { config, mask, table }
    }

    /// Get the chunker configuration
    pub fn config(&self) -> &ChunkerConfig {
        &self.config
    }

    /// Get the Buzhash table
    pub fn table(&self) -> &[u64; 256] {
        &self.table
    }

    /// Get the boundary mask
    pub fn mask(&self) -> u64 {
        self.mask
    }
    
    /// Chunk data from a reader
    pub fn chunk<R: Read>(&self, mut reader: R) -> crate::Result<Vec<Vec<u8>>> {
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.config.max_size];
        let mut current_chunk = Vec::with_capacity(self.config.target_size);
        let mut hash = 0u64;

        // Use fixed-size circular buffer instead of Vec for O(1) operations
        // Max window size is 64 bytes (typical values are 32-48)
        let mut window = [0u8; 64];
        let mut window_pos = 0usize;
        let mut window_len = 0usize;
        let window_size = self.config.window_size;

        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }

            for &byte in &buffer[..n] {
                // Update rolling hash using circular buffer (O(1) instead of O(n))
                if window_len >= window_size {
                    let old_byte = window[window_pos];
                    hash ^= self.table[old_byte as usize]
                        .rotate_left(window_size as u32);
                } else {
                    window_len += 1;
                }
                window[window_pos] = byte;
                window_pos = (window_pos + 1) % window_size;
                hash = hash.rotate_left(1) ^ self.table[byte as usize];

                current_chunk.push(byte);

                // Check for chunk boundary
                let size = current_chunk.len();
                if size >= self.config.min_size
                    && ((hash & self.mask) == 0 || size >= self.config.max_size)
                {
                    chunks.push(std::mem::take(&mut current_chunk));
                    current_chunk = Vec::with_capacity(self.config.target_size);
                    hash = 0;
                    window_pos = 0;
                    window_len = 0;
                }
            }
        }

        // Don't forget the last chunk
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        Ok(chunks)
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(ChunkerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    
    #[test]
    fn test_chunker() {
        let config = ChunkerConfig {
            min_size: 64,
            target_size: 256,
            max_size: 1024,
            window_size: 16,
        };
        
        let chunker = Chunker::new(config);
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        
        let chunks = chunker.chunk(Cursor::new(&data)).unwrap();
        
        // Verify all data is accounted for
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, data.len());
        
        // Verify chunks respect max size
        for chunk in &chunks {
            assert!(chunk.len() <= 1024);
        }
    }
}
