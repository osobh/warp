//! Async directory walking with tokio.
//!
//! This module provides async versions of directory walking functionality,
//! allowing non-blocking enumeration of directory contents.

use crate::Result;
use crate::walker::FileEntry;
use std::path::Path;
use tokio::sync::mpsc;

/// Async walk a directory and return all file entries.
///
/// This is an async wrapper around the sync walker that runs the blocking
/// operation in a spawn_blocking task.
///
/// # Example
/// ```no_run
/// use warp_io::walk_directory_async;
///
/// # async fn example() -> warp_io::Result<()> {
/// let entries = walk_directory_async("/path/to/dir").await?;
/// for entry in entries {
///     println!("{}: {} bytes", entry.path.display(), entry.size);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn walk_directory_async(path: impl AsRef<Path>) -> Result<Vec<FileEntry>> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || crate::walker::walk_directory(&path))
        .await
        .map_err(|e| crate::Error::Io(std::io::Error::other(e)))?
}

/// Stream file entries through a channel.
///
/// This spawns a background task that walks the directory and sends entries
/// through an mpsc channel. The channel has bounded capacity for backpressure.
///
/// # Arguments
/// * `path` - Path to the directory
/// * `channel_capacity` - Maximum number of entries to buffer
///
/// # Example
/// ```no_run
/// use warp_io::walk_directory_stream;
///
/// # async fn example() -> warp_io::Result<()> {
/// let mut rx = walk_directory_stream("/path/to/dir", 100);
/// while let Some(result) = rx.recv().await {
///     let entry = result?;
///     if !entry.is_dir {
///         println!("File: {}", entry.path.display());
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn walk_directory_stream(
    path: impl AsRef<Path>,
    channel_capacity: usize,
) -> mpsc::Receiver<Result<FileEntry>> {
    let (tx, rx) = mpsc::channel(channel_capacity);
    let path = path.as_ref().to_path_buf();

    tokio::spawn(async move {
        let result = stream_entries_to_channel(path, tx.clone()).await;
        if let Err(e) = result {
            let _ = tx.send(Err(e)).await;
        }
    });

    rx
}

/// Internal function to stream entries to a channel.
async fn stream_entries_to_channel(
    path: impl AsRef<Path>,
    tx: mpsc::Sender<Result<FileEntry>>,
) -> Result<()> {
    let path = path.as_ref().to_path_buf();

    // Run the blocking walk in a spawn_blocking
    let entries = tokio::task::spawn_blocking(move || crate::walker::walk_directory(&path))
        .await
        .map_err(|e| crate::Error::Io(std::io::Error::other(e)))??;

    // Send each entry through the channel
    for entry in entries {
        if tx.send(Ok(entry)).await.is_err() {
            // Receiver dropped, stop
            break;
        }
    }

    Ok(())
}

/// Async calculate total size of a directory.
///
/// # Example
/// ```no_run
/// use warp_io::async_walker::directory_size_async;
///
/// # async fn example() -> warp_io::Result<()> {
/// let size = directory_size_async("/path/to/dir").await?;
/// println!("Total size: {} bytes", size);
/// # Ok(())
/// # }
/// ```
pub async fn directory_size_async(path: impl AsRef<Path>) -> Result<u64> {
    let entries = walk_directory_async(path).await?;
    Ok(entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    /// Test 1: walk_directory_async finds all files
    #[tokio::test]
    async fn test_walk_async_finds_files() {
        let dir = tempdir().unwrap();

        // Create test structure
        File::create(dir.path().join("a.txt"))
            .unwrap()
            .write_all(b"hello")
            .unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        File::create(dir.path().join("sub/b.txt"))
            .unwrap()
            .write_all(b"world")
            .unwrap();

        let entries = walk_directory_async(dir.path()).await.unwrap();

        let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
        assert_eq!(files.len(), 2);
    }

    /// Test 2: walk_directory_async matches sync walker results
    #[tokio::test]
    async fn test_async_matches_sync() {
        let dir = tempdir().unwrap();

        // Create test structure
        File::create(dir.path().join("file1.txt"))
            .unwrap()
            .write_all(b"content1")
            .unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        File::create(dir.path().join("subdir/file2.txt"))
            .unwrap()
            .write_all(b"content2")
            .unwrap();

        let sync_entries = crate::walker::walk_directory(dir.path()).unwrap();
        let async_entries = walk_directory_async(dir.path()).await.unwrap();

        assert_eq!(sync_entries.len(), async_entries.len());

        // Sort by path for comparison
        let mut sync_paths: Vec<_> = sync_entries.iter().map(|e| &e.path).collect();
        let mut async_paths: Vec<_> = async_entries.iter().map(|e| &e.path).collect();
        sync_paths.sort();
        async_paths.sort();

        assert_eq!(sync_paths, async_paths);
    }

    /// Test 3: walk_directory_stream produces entries via channel
    #[tokio::test]
    async fn test_stream_produces_entries() {
        let dir = tempdir().unwrap();

        File::create(dir.path().join("a.txt"))
            .unwrap()
            .write_all(b"hello")
            .unwrap();
        File::create(dir.path().join("b.txt"))
            .unwrap()
            .write_all(b"world")
            .unwrap();

        let mut rx = walk_directory_stream(dir.path().to_path_buf(), 100);

        let mut entries = Vec::new();
        while let Some(result) = rx.recv().await {
            entries.push(result.unwrap());
        }

        let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
        assert_eq!(files.len(), 2);
    }

    /// Test 4: handles symlinks (doesn't follow them)
    #[tokio::test]
    async fn test_handles_symlinks() {
        let dir = tempdir().unwrap();

        File::create(dir.path().join("real.txt"))
            .unwrap()
            .write_all(b"real file")
            .unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("link.txt"))
                .unwrap();

            let entries = walk_directory_async(dir.path()).await.unwrap();

            // Should see both the real file and the symlink
            let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
            assert_eq!(files.len(), 2);
        }

        #[cfg(not(unix))]
        {
            // On Windows, just verify basic walking works
            let entries = walk_directory_async(dir.path()).await.unwrap();
            let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
            assert_eq!(files.len(), 1);
        }
    }

    /// Test 5: handles non-existent directory
    #[tokio::test]
    async fn test_handles_nonexistent() {
        let result = walk_directory_async("/nonexistent/path/that/does/not/exist").await;
        assert!(result.is_err());
    }

    /// Test 6: directory_size_async calculates correct size
    #[tokio::test]
    async fn test_directory_size_async() {
        let dir = tempdir().unwrap();

        File::create(dir.path().join("a.txt"))
            .unwrap()
            .write_all(b"hello")
            .unwrap();
        File::create(dir.path().join("b.txt"))
            .unwrap()
            .write_all(b"world")
            .unwrap();

        let size = directory_size_async(dir.path()).await.unwrap();
        assert_eq!(size, 10); // "hello" + "world"
    }

    /// Test 7: stream handles empty directory
    #[tokio::test]
    async fn test_stream_empty_dir() {
        let dir = tempdir().unwrap();

        let mut rx = walk_directory_stream(dir.path().to_path_buf(), 100);

        let mut entries = Vec::new();
        while let Some(result) = rx.recv().await {
            entries.push(result.unwrap());
        }

        // Should have just the directory itself
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_dir);
    }

    /// Test 8: handles deep directory structures
    #[tokio::test]
    async fn test_deep_directory() {
        let dir = tempdir().unwrap();

        // Create deep nested structure
        let mut path = dir.path().to_path_buf();
        for i in 0..5 {
            path = path.join(format!("level{}", i));
            fs::create_dir(&path).unwrap();
            File::create(path.join("file.txt"))
                .unwrap()
                .write_all(format!("content{}", i).as_bytes())
                .unwrap();
        }

        let entries = walk_directory_async(dir.path()).await.unwrap();

        let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
        assert_eq!(files.len(), 5);

        let dirs: Vec<_> = entries.iter().filter(|e| e.is_dir).collect();
        assert_eq!(dirs.len(), 6); // root + 5 levels
    }

    /// Test 9: walk_directory_stream handles non-existent path
    #[tokio::test]
    async fn test_stream_nonexistent() {
        let mut rx = walk_directory_stream("/nonexistent/path".to_string(), 100);

        let result = rx.recv().await;
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    /// Test 10: concurrent directory walks
    #[tokio::test]
    async fn test_concurrent_walks() {
        let dirs: Vec<_> = (0..3)
            .map(|i| {
                let dir = tempdir().unwrap();
                File::create(dir.path().join(format!("file{}.txt", i)))
                    .unwrap()
                    .write_all(b"content")
                    .unwrap();
                dir
            })
            .collect();

        let mut set = tokio::task::JoinSet::new();
        for dir in &dirs {
            let path = dir.path().to_path_buf();
            set.spawn(async move { walk_directory_async(path).await });
        }

        let mut count = 0;
        while let Some(result) = set.join_next().await {
            let entries = result.unwrap().unwrap();
            let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
            assert_eq!(files.len(), 1);
            count += 1;
        }

        assert_eq!(count, 3);
    }
}
