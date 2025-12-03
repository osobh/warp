//! warp-io: I/O utilities
//!
//! - Content-defined chunking (Buzhash)
//! - Directory walking
//! - Memory-mapped I/O
//! - Buffer pools

#![warn(missing_docs)]

pub mod chunker;
pub mod walker;
pub mod mmap;
pub mod pool;

pub use chunker::{Chunker, ChunkerConfig};
pub use walker::walk_directory;

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
