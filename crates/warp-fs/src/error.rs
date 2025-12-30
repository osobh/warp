//! Error types for warp-fs

use std::io;
use thiserror::Error;

/// Result type for warp-fs operations
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur in warp-fs
#[derive(Error, Debug)]
pub enum Error {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// warp-store error
    #[error("Store error: {0}")]
    Store(#[from] warp_store::Error),

    /// Invalid inode number
    #[error("Invalid inode: {0}")]
    InvalidInode(u64),

    /// Inode not found
    #[error("Inode not found: {0}")]
    InodeNotFound(u64),

    /// Path not found
    #[error("Path not found: {0}")]
    PathNotFound(String),

    /// File not found
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// Directory not found
    #[error("Directory not found: {0}")]
    DirectoryNotFound(String),

    /// Not a directory
    #[error("Not a directory: {0}")]
    NotADirectory(String),

    /// Not a file
    #[error("Not a file: {0}")]
    NotAFile(String),

    /// Is a directory
    #[error("Is a directory: {0}")]
    IsADirectory(String),

    /// Directory not empty
    #[error("Directory not empty: {0}")]
    DirectoryNotEmpty(String),

    /// File already exists
    #[error("File already exists: {0}")]
    FileExists(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Invalid file name
    #[error("Invalid file name: {0}")]
    InvalidFileName(String),

    /// Invalid path
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Invalid file handle
    #[error("Invalid file handle: {0}")]
    InvalidFileHandle(u64),

    /// Stale file handle
    #[error("Stale file handle")]
    StaleFileHandle,

    /// Read-only filesystem
    #[error("Read-only filesystem")]
    ReadOnlyFilesystem,

    /// Filesystem full (quota exceeded)
    #[error("Filesystem full")]
    FilesystemFull,

    /// Operation not supported
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Cross-device link
    #[error("Cross-device link not supported")]
    CrossDeviceLink,

    /// Too many open files
    #[error("Too many open files")]
    TooManyOpenFiles,

    /// Metadata serialization error
    #[error("Metadata serialization error: {0}")]
    MetadataSerialization(String),

    /// Metadata deserialization error
    #[error("Metadata deserialization error: {0}")]
    MetadataDeserialization(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Convert error to POSIX errno
    pub fn to_errno(&self) -> i32 {
        match self {
            Error::Io(e) => e.raw_os_error().unwrap_or(libc::EIO),
            Error::Store(_) => libc::EIO,
            Error::InvalidInode(_) => libc::EINVAL,
            Error::InodeNotFound(_) => libc::ENOENT,
            Error::PathNotFound(_) => libc::ENOENT,
            Error::FileNotFound(_) => libc::ENOENT,
            Error::DirectoryNotFound(_) => libc::ENOENT,
            Error::NotADirectory(_) => libc::ENOTDIR,
            Error::NotAFile(_) => libc::EISDIR,
            Error::IsADirectory(_) => libc::EISDIR,
            Error::DirectoryNotEmpty(_) => libc::ENOTEMPTY,
            Error::FileExists(_) => libc::EEXIST,
            Error::PermissionDenied(_) => libc::EACCES,
            Error::InvalidFileName(_) => libc::EINVAL,
            Error::InvalidPath(_) => libc::EINVAL,
            Error::InvalidFileHandle(_) => libc::EBADF,
            Error::StaleFileHandle => libc::ESTALE,
            Error::ReadOnlyFilesystem => libc::EROFS,
            Error::FilesystemFull => libc::ENOSPC,
            Error::NotSupported(_) => libc::ENOTSUP,
            Error::CrossDeviceLink => libc::EXDEV,
            Error::TooManyOpenFiles => libc::EMFILE,
            Error::MetadataSerialization(_) => libc::EIO,
            Error::MetadataDeserialization(_) => libc::EIO,
            Error::Internal(_) => libc::EIO,
        }
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        let kind = match &e {
            Error::Io(io_err) => return io::Error::new(io_err.kind(), e.to_string()),
            Error::FileNotFound(_) | Error::PathNotFound(_) | Error::InodeNotFound(_) => {
                io::ErrorKind::NotFound
            }
            Error::PermissionDenied(_) => io::ErrorKind::PermissionDenied,
            Error::FileExists(_) => io::ErrorKind::AlreadyExists,
            Error::NotADirectory(_) | Error::InvalidFileName(_) | Error::InvalidPath(_) => {
                io::ErrorKind::InvalidInput
            }
            Error::NotSupported(_) => io::ErrorKind::Unsupported,
            _ => io::ErrorKind::Other,
        };
        io::Error::new(kind, e.to_string())
    }
}
