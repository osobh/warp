//! Memory-mapped file I/O

use std::fs::File;
use std::path::Path;
use memmap2::{Mmap, MmapMut};

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
