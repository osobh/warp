//! Directory walking utilities

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// File entry from directory walk
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Absolute path
    pub path: PathBuf,
    /// Relative path from root
    pub relative_path: PathBuf,
    /// File size
    pub size: u64,
    /// Is directory
    pub is_dir: bool,
}

/// Walk a directory and return all file entries
pub fn walk_directory(root: &Path) -> crate::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let metadata = entry.metadata()?;

        let relative_path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_path_buf();

        entries.push(FileEntry {
            path: entry.path().to_path_buf(),
            relative_path,
            size: metadata.len(),
            is_dir: metadata.is_dir(),
        });
    }

    Ok(entries)
}

/// Calculate total size of a directory
pub fn directory_size(root: &Path) -> crate::Result<u64> {
    let entries = walk_directory(root)?;
    Ok(entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_walk() {
        let dir = tempdir().unwrap();

        // Create test files
        File::create(dir.path().join("a.txt"))
            .unwrap()
            .write_all(b"hello")
            .unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        File::create(dir.path().join("sub/b.txt"))
            .unwrap()
            .write_all(b"world")
            .unwrap();

        let entries = walk_directory(dir.path()).unwrap();

        let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
        assert_eq!(files.len(), 2);

        let total_size = directory_size(dir.path()).unwrap();
        assert_eq!(total_size, 10); // "hello" + "world"
    }
}
