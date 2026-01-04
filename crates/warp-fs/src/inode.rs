//! Inode management for warp-fs

use crate::metadata::{FileType, InodeMetadata};
use std::sync::atomic::{AtomicU64, Ordering};

/// Inode number type
pub type Ino = u64;

/// Root inode number
pub const ROOT_INO: Ino = 1;

/// Reserved inode for metadata bucket marker
pub const META_INO: Ino = 0;

/// Inode kind (mirrors FileType for convenience)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeKind {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Other (pipe, device, socket)
    Other,
}

impl From<FileType> for InodeKind {
    fn from(ft: FileType) -> Self {
        match ft {
            FileType::RegularFile => InodeKind::File,
            FileType::Directory => InodeKind::Directory,
            FileType::Symlink => InodeKind::Symlink,
            _ => InodeKind::Other,
        }
    }
}

impl From<InodeKind> for FileType {
    fn from(kind: InodeKind) -> Self {
        match kind {
            InodeKind::File => FileType::RegularFile,
            InodeKind::Directory => FileType::Directory,
            InodeKind::Symlink => FileType::Symlink,
            InodeKind::Other => FileType::RegularFile, // Default fallback
        }
    }
}

/// A live inode in memory
///
/// This wraps InodeMetadata with runtime state
#[derive(Debug)]
pub struct Inode {
    /// The underlying metadata
    metadata: InodeMetadata,

    /// Whether the inode has been modified since last flush
    dirty: bool,

    /// Number of open file handles
    open_count: AtomicU64,

    /// Whether this inode is being deleted
    unlinked: bool,
}

impl Inode {
    /// Create a new inode from metadata
    pub fn new(metadata: InodeMetadata) -> Self {
        Self {
            metadata,
            dirty: false,
            open_count: AtomicU64::new(0),
            unlinked: false,
        }
    }

    /// Get the inode number
    pub fn ino(&self) -> Ino {
        self.metadata.ino
    }

    /// Get the kind of inode
    pub fn kind(&self) -> InodeKind {
        self.metadata.file_type.into()
    }

    /// Check if this is a directory
    pub fn is_dir(&self) -> bool {
        self.metadata.is_dir()
    }

    /// Check if this is a regular file
    pub fn is_file(&self) -> bool {
        self.metadata.is_file()
    }

    /// Check if this is a symlink
    pub fn is_symlink(&self) -> bool {
        self.metadata.is_symlink()
    }

    /// Get the file size
    pub fn size(&self) -> u64 {
        self.metadata.size
    }

    /// Get reference to metadata
    pub fn metadata(&self) -> &InodeMetadata {
        &self.metadata
    }

    /// Get mutable reference to metadata (marks dirty)
    pub fn metadata_mut(&mut self) -> &mut InodeMetadata {
        self.dirty = true;
        &mut self.metadata
    }

    /// Check if dirty
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark as clean
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Mark as dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Increment open count
    pub fn open(&self) -> u64 {
        self.open_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Decrement open count
    pub fn close(&self) -> u64 {
        let prev = self.open_count.fetch_sub(1, Ordering::SeqCst);
        prev.saturating_sub(1)
    }

    /// Get current open count
    pub fn open_count(&self) -> u64 {
        self.open_count.load(Ordering::SeqCst)
    }

    /// Mark as unlinked (pending deletion)
    pub fn mark_unlinked(&mut self) {
        self.unlinked = true;
    }

    /// Check if unlinked
    pub fn is_unlinked(&self) -> bool {
        self.unlinked
    }

    /// Check if this inode can be reclaimed
    ///
    /// An inode can be reclaimed when it's unlinked and has no open handles
    pub fn can_reclaim(&self) -> bool {
        self.unlinked && self.open_count() == 0
    }

    /// Convert to file attributes for FUSE
    pub fn to_file_attr(&self, block_size: u32) -> fuser::FileAttr {
        self.metadata.to_file_attr(block_size)
    }

    /// Get symlink target if this is a symlink
    pub fn symlink_target(&self) -> Option<&str> {
        self.metadata.symlink_target.as_deref()
    }
}

impl From<InodeMetadata> for Inode {
    fn from(metadata: InodeMetadata) -> Self {
        Self::new(metadata)
    }
}

/// Inode number allocator
///
/// Thread-safe allocator for new inode numbers
#[derive(Debug)]
pub struct InodeAllocator {
    next: AtomicU64,
}

impl InodeAllocator {
    /// Create a new allocator starting at the given inode
    pub fn new(start: Ino) -> Self {
        Self {
            next: AtomicU64::new(start),
        }
    }

    /// Allocate a new inode number
    pub fn alloc(&self) -> Ino {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    /// Get the next inode that would be allocated
    pub fn peek(&self) -> Ino {
        self.next.load(Ordering::SeqCst)
    }

    /// Update the next inode number (for recovery)
    pub fn set_next(&self, next: Ino) {
        self.next.store(next, Ordering::SeqCst);
    }
}

impl Default for InodeAllocator {
    fn default() -> Self {
        Self::new(2) // 1 is reserved for root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inode_kind_conversion() {
        assert_eq!(InodeKind::from(FileType::RegularFile), InodeKind::File);
        assert_eq!(InodeKind::from(FileType::Directory), InodeKind::Directory);
        assert_eq!(InodeKind::from(FileType::Symlink), InodeKind::Symlink);
    }

    #[test]
    fn test_inode_open_close() {
        let meta = InodeMetadata::new_file(42, 0o644, 1000, 1000);
        let inode = Inode::new(meta);

        assert_eq!(inode.open_count(), 0);
        assert_eq!(inode.open(), 1);
        assert_eq!(inode.open(), 2);
        assert_eq!(inode.open_count(), 2);
        assert_eq!(inode.close(), 1);
        assert_eq!(inode.open_count(), 1);
    }

    #[test]
    fn test_inode_allocator() {
        let alloc = InodeAllocator::new(100);
        assert_eq!(alloc.alloc(), 100);
        assert_eq!(alloc.alloc(), 101);
        assert_eq!(alloc.alloc(), 102);
        assert_eq!(alloc.peek(), 103);
    }

    #[test]
    fn test_inode_reclaim() {
        let meta = InodeMetadata::new_file(42, 0o644, 1000, 1000);
        let mut inode = Inode::new(meta);

        assert!(!inode.can_reclaim());

        inode.open();
        inode.mark_unlinked();
        assert!(!inode.can_reclaim()); // Still open

        inode.close();
        assert!(inode.can_reclaim()); // Unlinked and closed
    }
}
