//! Memory-mapped file I/O

use memmap2::{Mmap, MmapMut};
use std::fs::File;
use std::path::Path;

use crate::Result;

/// Memory-mapped read-only file
pub struct MappedFile {
    _file: File,
    mmap: Mmap,
}

impl MappedFile {
    /// Open a file for memory-mapped reading
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: Memory-mapping is safe here because:
        // 1. The file handle is valid (just opened successfully)
        // 2. We keep the File alive in _file for the lifetime of the mapping
        // 3. The mapping is read-only, preventing data races with external writers
        // 4. If the file is modified externally, we may see inconsistent data but
        //    this is a documented limitation of memory-mapped I/O, not UB
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Self { _file: file, mmap })
    }

    /// Get the mapped data
    pub fn as_slice(&self) -> &[u8] {
        &self.mmap
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }
}

/// Memory-mapped read-write file
pub struct MappedFileMut {
    _file: File,
    mmap: MmapMut,
}

impl MappedFileMut {
    /// Open a file for memory-mapped read/write
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        // SAFETY: Memory-mapping is safe here because:
        // 1. The file handle is valid (just opened successfully with read+write)
        // 2. We keep the File alive in _file for the lifetime of the mapping
        // 3. We have exclusive ownership of the MmapMut, preventing aliased mutation
        // 4. Caller must ensure no concurrent external modifications to the file
        //    (this is a documented requirement of memory-mapped I/O)
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        Ok(Self { _file: file, mmap })
    }

    /// Get the mapped data
    pub fn as_slice(&self) -> &[u8] {
        &self.mmap
    }

    /// Get mutable mapped data
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.mmap
    }

    /// Flush changes to disk
    pub fn flush(&self) -> Result<()> {
        self.mmap.flush()?;
        Ok(())
    }
}
