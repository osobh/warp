//! Async content-defined chunking using Buzhash with tokio.
//!
//! This module provides async versions of the chunking functionality,
//! allowing non-blocking I/O operations for better concurrency.

use crate::{Chunker, ChunkerConfig, Result};
use std::path::Path;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

/// Async chunk a file using content-defined chunking.
///
/// This reads the file asynchronously and produces chunks using the Buzhash
/// algorithm. Memory usage is bounded to approximately the max chunk size.
///
/// # Example
/// ```no_run
/// use warp_io::{chunk_file_async, ChunkerConfig};
///
/// # async fn example() -> warp_io::Result<()> {
/// let chunks = chunk_file_async("/path/to/file", ChunkerConfig::default()).await?;
/// println!("Got {} chunks", chunks.len());
/// # Ok(())
/// # }
/// ```
pub async fn chunk_file_async(
    path: impl AsRef<Path>,
    config: ChunkerConfig,
) -> Result<Vec<Vec<u8>>> {
    let file = tokio::fs::File::open(path).await?;
    let chunker = Chunker::new(config);
    chunk_async_reader(file, &chunker).await
}

/// Stream chunks through a channel for pipeline processing.
///
/// This spawns a background task that reads the file and sends chunks
/// through an mpsc channel. The channel has a bounded capacity for backpressure.
///
/// # Arguments
/// * `path` - Path to the file
/// * `config` - Chunker configuration
/// * `channel_capacity` - Maximum number of chunks to buffer (for backpressure)
///
/// # Example
/// ```no_run
/// use warp_io::{chunk_file_stream, ChunkerConfig};
///
/// # async fn example() -> warp_io::Result<()> {
/// let mut rx = chunk_file_stream("/path/to/file", ChunkerConfig::default(), 16);
/// while let Some(result) = rx.recv().await {
///     let chunk = result?;
///     println!("Received chunk of {} bytes", chunk.len());
/// }
/// # Ok(())
/// # }
/// ```
pub fn chunk_file_stream(
    path: impl AsRef<Path> + Send + 'static,
    config: ChunkerConfig,
    channel_capacity: usize,
) -> mpsc::Receiver<Result<Vec<u8>>> {
    let (tx, rx) = mpsc::channel(channel_capacity);
    let path = path.as_ref().to_path_buf();

    tokio::spawn(async move {
        let result = stream_chunks_to_channel(path, config, tx.clone()).await;
        if let Err(e) = result {
            // Try to send error, ignore if receiver dropped
            let _ = tx.send(Err(e)).await;
        }
    });

    rx
}

/// Internal function to stream chunks to a channel.
async fn stream_chunks_to_channel(
    path: impl AsRef<Path>,
    config: ChunkerConfig,
    tx: mpsc::Sender<Result<Vec<u8>>>,
) -> Result<()> {
    let file = tokio::fs::File::open(path).await?;
    let chunker = Chunker::new(config.clone());

    let mut reader = file;
    let mut buffer = vec![0u8; config.max_size];
    let mut current_chunk = Vec::new();
    let mut hash = 0u64;
    let mut window = Vec::with_capacity(config.window_size);

    // Calculate mask (same as sync chunker)
    let bits = (config.target_size as f64).log2() as u32;
    let mask = (1u64 << bits) - 1;

    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        for &byte in &buffer[..n] {
            // Update rolling hash
            if window.len() >= config.window_size {
                let old_byte = window.remove(0);
                hash ^= chunker.table()[old_byte as usize]
                    .rotate_left(config.window_size as u32);
            }
            window.push(byte);
            hash = hash.rotate_left(1) ^ chunker.table()[byte as usize];

            current_chunk.push(byte);

            // Check for chunk boundary
            let size = current_chunk.len();
            if size >= config.min_size
                && ((hash & mask) == 0 || size >= config.max_size)
            {
                let chunk = std::mem::take(&mut current_chunk);
                if tx.send(Ok(chunk)).await.is_err() {
                    // Receiver dropped, stop processing
                    return Ok(());
                }
                hash = 0;
                window.clear();
            }
        }
    }

    // Don't forget the last chunk
    if !current_chunk.is_empty() {
        let _ = tx.send(Ok(current_chunk)).await;
    }

    Ok(())
}

/// Async chunk from any AsyncRead source.
async fn chunk_async_reader<R>(mut reader: R, chunker: &Chunker) -> Result<Vec<Vec<u8>>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let config = chunker.config();
    let mut chunks = Vec::new();
    let mut buffer = vec![0u8; config.max_size];
    let mut current_chunk = Vec::new();
    let mut hash = 0u64;
    let mut window = Vec::with_capacity(config.window_size);

    // Calculate mask
    let bits = (config.target_size as f64).log2() as u32;
    let mask = (1u64 << bits) - 1;

    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        for &byte in &buffer[..n] {
            // Update rolling hash
            if window.len() >= config.window_size {
                let old_byte = window.remove(0);
                hash ^= chunker.table()[old_byte as usize]
                    .rotate_left(config.window_size as u32);
            }
            window.push(byte);
            hash = hash.rotate_left(1) ^ chunker.table()[byte as usize];

            current_chunk.push(byte);

            // Check for chunk boundary
            let size = current_chunk.len();
            if size >= config.min_size
                && ((hash & mask) == 0 || size >= config.max_size)
            {
                chunks.push(std::mem::take(&mut current_chunk));
                hash = 0;
                window.clear();
            }
        }
    }

    // Don't forget the last chunk
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Test 1: async_chunk produces same chunks as sync chunker
    #[tokio::test]
    async fn test_async_matches_sync() {
        let mut file = NamedTempFile::new().unwrap();
        let data: Vec<u8> = (0..50_000).map(|i| (i % 256) as u8).collect();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let config = ChunkerConfig {
            min_size: 1024,
            target_size: 4096,
            max_size: 8192,
            window_size: 32,
        };

        // Sync chunking
        let sync_chunker = Chunker::new(config.clone());
        let sync_chunks = sync_chunker
            .chunk(std::io::Cursor::new(&data))
            .unwrap();

        // Async chunking
        let async_chunks = chunk_file_async(file.path(), config).await.unwrap();

        // Should produce same number of chunks
        assert_eq!(async_chunks.len(), sync_chunks.len());

        // Each chunk should be identical
        for (async_chunk, sync_chunk) in async_chunks.iter().zip(sync_chunks.iter()) {
            assert_eq!(async_chunk, sync_chunk);
        }
    }

    /// Test 2: async_chunk works with tokio::fs::File
    #[tokio::test]
    async fn test_async_with_file() {
        let mut file = NamedTempFile::new().unwrap();
        let data = b"test data for async chunking";
        file.write_all(data).unwrap();
        file.flush().unwrap();

        let config = ChunkerConfig {
            min_size: 8,
            target_size: 16,
            max_size: 32,
            window_size: 4,
        };

        let chunks = chunk_file_async(file.path(), config).await.unwrap();

        // Verify all data is accounted for
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, data.len());
    }

    /// Test 3: async_chunk respects min/target/max sizes
    #[tokio::test]
    async fn test_async_chunk_sizes() {
        let mut file = NamedTempFile::new().unwrap();
        let data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let config = ChunkerConfig {
            min_size: 1024,
            target_size: 4096,
            max_size: 8192,
            window_size: 32,
        };

        let chunks = chunk_file_async(file.path(), config.clone()).await.unwrap();

        // All chunks except last should respect min_size
        for chunk in chunks.iter().take(chunks.len().saturating_sub(1)) {
            assert!(
                chunk.len() >= config.min_size,
                "Chunk size {} is less than min_size {}",
                chunk.len(),
                config.min_size
            );
        }

        // All chunks should respect max_size
        for chunk in &chunks {
            assert!(
                chunk.len() <= config.max_size,
                "Chunk size {} exceeds max_size {}",
                chunk.len(),
                config.max_size
            );
        }
    }

    /// Test 4: chunk_stream produces chunks via channel
    #[tokio::test]
    async fn test_chunk_stream() {
        let mut file = NamedTempFile::new().unwrap();
        let data: Vec<u8> = (0..50_000).map(|i| (i % 256) as u8).collect();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let config = ChunkerConfig {
            min_size: 1024,
            target_size: 4096,
            max_size: 8192,
            window_size: 32,
        };

        let mut rx = chunk_file_stream(file.path().to_path_buf(), config, 16);

        let mut chunks = Vec::new();
        while let Some(result) = rx.recv().await {
            chunks.push(result.unwrap());
        }

        // Verify all data is accounted for
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, data.len());
    }

    /// Test 5: async_chunk handles empty input
    #[tokio::test]
    async fn test_async_empty_file() {
        let file = NamedTempFile::new().unwrap();

        let chunks = chunk_file_async(file.path(), ChunkerConfig::default())
            .await
            .unwrap();

        assert!(chunks.is_empty());
    }

    /// Test 6: async_chunk handles input smaller than min_size
    #[tokio::test]
    async fn test_async_small_input() {
        let mut file = NamedTempFile::new().unwrap();
        let data = b"small";
        file.write_all(data).unwrap();
        file.flush().unwrap();

        let config = ChunkerConfig {
            min_size: 1024,
            target_size: 4096,
            max_size: 8192,
            window_size: 32,
        };

        let chunks = chunk_file_async(file.path(), config).await.unwrap();

        // Should produce exactly one chunk with all data
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], data);
    }

    /// Test 7: concurrent chunking of multiple files
    #[tokio::test]
    async fn test_concurrent_chunking() {
        let files: Vec<_> = (0..5)
            .map(|i| {
                let mut file = NamedTempFile::new().unwrap();
                let data: Vec<u8> = (0..10_000).map(|j| ((i * 100 + j) % 256) as u8).collect();
                file.write_all(&data).unwrap();
                file.flush().unwrap();
                (file, data)
            })
            .collect();

        let config = ChunkerConfig {
            min_size: 512,
            target_size: 2048,
            max_size: 4096,
            window_size: 16,
        };

        // Chunk all files concurrently using JoinSet
        let mut set = tokio::task::JoinSet::new();
        for (file, _) in files.iter() {
            let path = file.path().to_path_buf();
            let config = config.clone();
            set.spawn(async move { chunk_file_async(path, config).await });
        }

        let mut results = Vec::new();
        while let Some(result) = set.join_next().await {
            results.push(result.unwrap().unwrap());
        }

        // Verify each result has correct total size
        assert_eq!(results.len(), 5);
        for chunks in &results {
            let total: usize = chunks.iter().map(|c| c.len()).sum();
            assert_eq!(total, 10_000);
        }
    }

    /// Test 8: backpressure handling with bounded channel
    #[tokio::test]
    async fn test_backpressure() {
        let mut file = NamedTempFile::new().unwrap();
        // Large data to ensure many chunks
        let data: Vec<u8> = (0..500_000).map(|i| (i % 256) as u8).collect();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let config = ChunkerConfig {
            min_size: 1024,
            target_size: 4096,
            max_size: 8192,
            window_size: 32,
        };

        // Very small channel capacity to test backpressure
        let mut rx = chunk_file_stream(file.path().to_path_buf(), config, 2);

        let mut chunks = Vec::new();
        let mut count = 0;
        while let Some(result) = rx.recv().await {
            let chunk = result.unwrap();
            chunks.push(chunk);
            count += 1;

            // Simulate slow consumer
            if count % 5 == 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
        }

        // Verify all data received despite backpressure
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, data.len());
    }

    /// Test 9: non-existent file returns error
    #[tokio::test]
    async fn test_async_file_not_found() {
        let result = chunk_file_async("/nonexistent/path", ChunkerConfig::default()).await;
        assert!(result.is_err());
    }

    /// Test 10: stream handles file not found
    #[tokio::test]
    async fn test_stream_file_not_found() {
        let mut rx = chunk_file_stream(
            "/nonexistent/path".to_string(),
            ChunkerConfig::default(),
            16,
        );

        let result = rx.recv().await;
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }
}
