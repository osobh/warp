//! Fixed-size chunking for streaming operations
//!
//! Unlike content-defined chunking (CDC), fixed-size chunking produces
//! chunks of deterministic size. This is ideal for real-time streaming
//! where consistent latency is more important than deduplication.
//!
//! # Use Cases
//!
//! - Real-time encrypted streaming with latency guarantees
//! - Network protocols with fixed packet sizes
//! - GPU processing with fixed buffer sizes
//!
//! # Example
//!
//! ```
//! use warp_io::FixedChunker;
//! use std::io::Cursor;
//!
//! let data = vec![0u8; 100];
//! let chunker = FixedChunker::new(32);
//! let chunks = chunker.chunk(&mut Cursor::new(data)).unwrap();
//!
//! assert_eq!(chunks.len(), 4); // 100 / 32 = 3 full + 1 partial
//! assert_eq!(chunks[0].len(), 32);
//! assert_eq!(chunks[3].len(), 4); // remaining 4 bytes
//! ```

use std::io::Read;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::Result;

/// Fixed-size chunker for streaming operations
#[derive(Debug, Clone)]
pub struct FixedChunker {
    /// Size of each chunk in bytes
    chunk_size: usize,
}

impl FixedChunker {
    /// Create a new fixed-size chunker
    ///
    /// # Arguments
    /// * `chunk_size` - Size of each chunk in bytes (must be > 0)
    ///
    /// # Panics
    /// Panics if chunk_size is 0
    pub fn new(chunk_size: usize) -> Self {
        assert!(chunk_size > 0, "chunk_size must be > 0");
        Self { chunk_size }
    }

    /// Create a chunker for low-latency streaming (64KB chunks)
    pub fn low_latency() -> Self {
        Self::new(64 * 1024)
    }

    /// Create a chunker for high-throughput streaming (1MB chunks)
    pub fn high_throughput() -> Self {
        Self::new(1024 * 1024)
    }

    /// Get the chunk size
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Chunk data from a synchronous reader
    ///
    /// # Arguments
    /// * `reader` - Source of data to chunk
    ///
    /// # Returns
    /// Vector of chunks (last chunk may be smaller than chunk_size)
    pub fn chunk<R: Read>(&self, reader: &mut R) -> Result<Vec<Vec<u8>>> {
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];

        loop {
            let mut total_read = 0;

            // Read until buffer is full or EOF
            while total_read < self.chunk_size {
                match reader.read(&mut buffer[total_read..]) {
                    Ok(0) => break, // EOF
                    Ok(n) => total_read += n,
                    Err(e) => return Err(e.into()),
                }
            }

            if total_read == 0 {
                break; // No more data
            }

            // Emit chunk (may be partial at EOF)
            chunks.push(buffer[..total_read].to_vec());

            if total_read < self.chunk_size {
                break; // Last partial chunk
            }
        }

        Ok(chunks)
    }

    /// Chunk data from an async reader
    ///
    /// # Arguments
    /// * `reader` - Async source of data to chunk
    ///
    /// # Returns
    /// Vector of chunks (last chunk may be smaller than chunk_size)
    pub async fn chunk_async<R: AsyncRead + Unpin>(&self, reader: &mut R) -> Result<Vec<Vec<u8>>> {
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];

        loop {
            let mut total_read = 0;

            // Read until buffer is full or EOF
            while total_read < self.chunk_size {
                match reader.read(&mut buffer[total_read..]).await {
                    Ok(0) => break, // EOF
                    Ok(n) => total_read += n,
                    Err(e) => return Err(e.into()),
                }
            }

            if total_read == 0 {
                break; // No more data
            }

            // Emit chunk (may be partial at EOF)
            chunks.push(buffer[..total_read].to_vec());

            if total_read < self.chunk_size {
                break; // Last partial chunk
            }
        }

        Ok(chunks)
    }

    /// Stream chunks through a channel (for pipeline processing)
    ///
    /// # Arguments
    /// * `reader` - Async source of data
    /// * `buffer_size` - Channel buffer size (backpressure)
    ///
    /// # Returns
    /// Receiver for chunked data
    pub fn chunk_stream<R: AsyncRead + Unpin + Send + 'static>(
        &self,
        mut reader: R,
        buffer_size: usize,
    ) -> tokio::sync::mpsc::Receiver<Result<Vec<u8>>> {
        let chunk_size = self.chunk_size;
        let (tx, rx) = tokio::sync::mpsc::channel(buffer_size);

        tokio::spawn(async move {
            let mut buffer = vec![0u8; chunk_size];

            loop {
                let mut total_read = 0;

                // Read until buffer is full or EOF
                while total_read < chunk_size {
                    match reader.read(&mut buffer[total_read..]).await {
                        Ok(0) => break, // EOF
                        Ok(n) => total_read += n,
                        Err(e) => {
                            let _ = tx.send(Err(e.into())).await;
                            return;
                        }
                    }
                }

                if total_read == 0 {
                    break; // No more data
                }

                // Send chunk
                if tx.send(Ok(buffer[..total_read].to_vec())).await.is_err() {
                    break; // Receiver dropped
                }

                if total_read < chunk_size {
                    break; // Last partial chunk
                }
            }
        });

        rx
    }

    /// Calculate the number of chunks for a given data size
    ///
    /// # Arguments
    /// * `data_size` - Total size of data in bytes
    ///
    /// # Returns
    /// Number of chunks (including partial last chunk)
    pub fn chunk_count(&self, data_size: usize) -> usize {
        if data_size == 0 {
            0
        } else {
            data_size.div_ceil(self.chunk_size)
        }
    }
}

impl Default for FixedChunker {
    fn default() -> Self {
        Self::low_latency()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_chunker_creation() {
        let chunker = FixedChunker::new(64);
        assert_eq!(chunker.chunk_size(), 64);
    }

    #[test]
    #[should_panic(expected = "chunk_size must be > 0")]
    fn test_chunker_zero_size() {
        FixedChunker::new(0);
    }

    #[test]
    fn test_low_latency_preset() {
        let chunker = FixedChunker::low_latency();
        assert_eq!(chunker.chunk_size(), 64 * 1024);
    }

    #[test]
    fn test_high_throughput_preset() {
        let chunker = FixedChunker::high_throughput();
        assert_eq!(chunker.chunk_size(), 1024 * 1024);
    }

    #[test]
    fn test_chunk_empty_data() {
        let chunker = FixedChunker::new(32);
        let data: Vec<u8> = vec![];
        let chunks = chunker.chunk(&mut Cursor::new(data)).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_exact_multiple() {
        let chunker = FixedChunker::new(10);
        let data = vec![0u8; 30]; // Exactly 3 chunks
        let chunks = chunker.chunk(&mut Cursor::new(data)).unwrap();

        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.len() == 10));
    }

    #[test]
    fn test_chunk_with_remainder() {
        let chunker = FixedChunker::new(10);
        let data = vec![0u8; 25]; // 2 full + 5 bytes
        let chunks = chunker.chunk(&mut Cursor::new(data)).unwrap();

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks[2].len(), 5);
    }

    #[test]
    fn test_chunk_smaller_than_size() {
        let chunker = FixedChunker::new(100);
        let data = vec![42u8; 50];
        let chunks = chunker.chunk(&mut Cursor::new(data)).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 50);
        assert!(chunks[0].iter().all(|&b| b == 42));
    }

    #[test]
    fn test_chunk_preserves_data() {
        let chunker = FixedChunker::new(4);
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let chunks = chunker.chunk(&mut Cursor::new(data.clone())).unwrap();

        let reassembled: Vec<u8> = chunks.into_iter().flatten().collect();
        assert_eq!(reassembled, data);
    }

    #[tokio::test]
    async fn test_chunk_async_empty() {
        let chunker = FixedChunker::new(32);
        let data: Vec<u8> = vec![];
        let chunks = chunker.chunk_async(&mut Cursor::new(data)).await.unwrap();
        assert!(chunks.is_empty());
    }

    #[tokio::test]
    async fn test_chunk_async_exact_multiple() {
        let chunker = FixedChunker::new(10);
        let data = vec![0u8; 30];
        let chunks = chunker.chunk_async(&mut Cursor::new(data)).await.unwrap();

        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.len() == 10));
    }

    #[tokio::test]
    async fn test_chunk_async_with_remainder() {
        let chunker = FixedChunker::new(10);
        let data = vec![0u8; 25];
        let chunks = chunker.chunk_async(&mut Cursor::new(data)).await.unwrap();

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[2].len(), 5);
    }

    #[tokio::test]
    async fn test_chunk_stream() {
        let chunker = FixedChunker::new(10);
        let data = vec![0u8; 25];
        let mut rx = chunker.chunk_stream(Cursor::new(data), 4);

        let mut chunks = Vec::new();
        while let Some(result) = rx.recv().await {
            chunks.push(result.unwrap());
        }

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks[2].len(), 5);
    }

    #[tokio::test]
    async fn test_chunk_stream_preserves_data() {
        let chunker = FixedChunker::new(4);
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut rx = chunker.chunk_stream(Cursor::new(data.clone()), 4);

        let mut reassembled = Vec::new();
        while let Some(result) = rx.recv().await {
            reassembled.extend(result.unwrap());
        }

        assert_eq!(reassembled, data);
    }

    #[test]
    fn test_chunk_count() {
        let chunker = FixedChunker::new(10);

        assert_eq!(chunker.chunk_count(0), 0);
        assert_eq!(chunker.chunk_count(10), 1);
        assert_eq!(chunker.chunk_count(11), 2);
        assert_eq!(chunker.chunk_count(30), 3);
        assert_eq!(chunker.chunk_count(35), 4);
    }

    #[test]
    fn test_default() {
        let chunker = FixedChunker::default();
        assert_eq!(chunker.chunk_size(), 64 * 1024); // Low latency default
    }
}
