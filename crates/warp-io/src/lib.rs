//! warp-io: I/O utilities
//!
//! - Content-defined chunking (Buzhash)
//! - Fixed-size chunking for streaming
//! - Async chunking with tokio
//! - Directory walking (sync and async)
//! - Memory-mapped I/O
//! - Buffer pools

#![warn(missing_docs)]

pub mod chunker;
pub mod fixed_chunker;
pub mod async_chunker;
pub mod walker;
pub mod async_walker;
pub mod mmap;
pub mod pool;

pub use chunker::{Chunker, ChunkerConfig};
pub use fixed_chunker::FixedChunker;
pub use walker::{walk_directory, FileEntry};
pub use async_chunker::{chunk_file_async, chunk_file_stream};
pub use async_walker::{walk_directory_async, walk_directory_stream};

/// I/O error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Walk error
    #[error("Walk error: {0}")]
    Walk(#[from] walkdir::Error),
}

/// Result type for I/O operations
pub type Result<T> = std::result::Result<T, Error>;
