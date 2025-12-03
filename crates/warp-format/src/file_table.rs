//! File_table implementation

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// File entry in the file table
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    /// Relative path
    pub path: String,
    /// Original file size
    pub size: u64,
    /// Unix file mode
    pub mode: u32,
    /// Modification time (Unix timestamp)
    pub mtime: i64,
    /// First chunk index
    pub chunk_start: u64,
    /// Last chunk index (exclusive)
    pub chunk_end: u64,
    /// Whole-file hash
    pub hash: [u8; 32],
}

/// File table for path-to-chunk mapping
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FileTable {
    entries: Vec<FileEntry>,
}

impl FileTable {
    /// Create a new empty file table
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a file entry
    pub fn push(&mut self, entry: FileEntry) {
        self.entries.push(entry);
    }

    /// Get file by path
    pub fn get_by_path(&self, path: &str) -> Option<&FileEntry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// Iterate over files
    pub fn iter(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter()
    }

    /// Number of files
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Serialize to bytes using MessagePack
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        rmp_serde::to_vec(&self).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Deserialize from bytes using MessagePack
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        rmp_serde::from_slice(bytes).map_err(|e| Error::Serialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_table_empty() {
        let table = FileTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert!(table.get_by_path("nonexistent").is_none());
    }

    #[test]
    fn test_file_table_push_get() {
        let mut table = FileTable::new();

        let entry1 = FileEntry {
            path: "file1.txt".to_string(),
            size: 1024,
            mode: 0o644,
            mtime: 1234567890,
            chunk_start: 0,
            chunk_end: 10,
            hash: [1u8; 32],
        };

        let entry2 = FileEntry {
            path: "dir/file2.txt".to_string(),
            size: 2048,
            mode: 0o755,
            mtime: 1234567891,
            chunk_start: 10,
            chunk_end: 20,
            hash: [2u8; 32],
        };

        table.push(entry1.clone());
        table.push(entry2.clone());

        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());

        let retrieved1 = table.get_by_path("file1.txt").unwrap();
        assert_eq!(retrieved1.path, "file1.txt");
        assert_eq!(retrieved1.size, 1024);
        assert_eq!(retrieved1.mode, 0o644);

        let retrieved2 = table.get_by_path("dir/file2.txt").unwrap();
        assert_eq!(retrieved2.path, "dir/file2.txt");
        assert_eq!(retrieved2.size, 2048);

        assert!(table.get_by_path("nonexistent").is_none());
    }

    #[test]
    fn test_file_table_iter() {
        let mut table = FileTable::new();

        for i in 0..5 {
            table.push(FileEntry {
                path: format!("file{}.txt", i),
                size: i * 100,
                mode: 0o644,
                mtime: i as i64,
                chunk_start: i,
                chunk_end: i + 1,
                hash: [i as u8; 32],
            });
        }

        let paths: Vec<String> = table.iter().map(|e| e.path.clone()).collect();
        assert_eq!(paths.len(), 5);
        assert_eq!(paths[0], "file0.txt");
        assert_eq!(paths[4], "file4.txt");
    }

    #[test]
    fn test_file_table_roundtrip_empty() {
        let table = FileTable::new();
        let bytes = table.to_bytes().unwrap();

        let decoded = FileTable::from_bytes(&bytes).unwrap();
        assert!(decoded.is_empty());
        assert_eq!(decoded, table);
    }

    #[test]
    fn test_file_table_roundtrip_single() {
        let mut table = FileTable::new();
        table.push(FileEntry {
            path: "test.txt".to_string(),
            size: 512,
            mode: 0o600,
            mtime: 9999999,
            chunk_start: 5,
            chunk_end: 15,
            hash: [99u8; 32],
        });

        let bytes = table.to_bytes().unwrap();
        let decoded = FileTable::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded, table);

        let entry = decoded.get_by_path("test.txt").unwrap();
        assert_eq!(entry.size, 512);
        assert_eq!(entry.mode, 0o600);
        assert_eq!(entry.mtime, 9999999);
        assert_eq!(entry.chunk_start, 5);
        assert_eq!(entry.chunk_end, 15);
        assert_eq!(entry.hash, [99u8; 32]);
    }

    #[test]
    fn test_file_table_roundtrip_multiple() {
        let mut table = FileTable::new();

        for i in 0..20 {
            table.push(FileEntry {
                path: format!("path/to/file{}.dat", i),
                size: i * 1000,
                mode: 0o644 + (i % 2) as u32,
                mtime: 1700000000 + i as i64,
                chunk_start: i * 5,
                chunk_end: (i + 1) * 5,
                hash: [i as u8; 32],
            });
        }

        let bytes = table.to_bytes().unwrap();
        let decoded = FileTable::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.len(), 20);
        assert_eq!(decoded, table);

        for i in 0..20 {
            let entry = decoded.get_by_path(&format!("path/to/file{}.dat", i)).unwrap();
            assert_eq!(entry.size, i * 1000);
            assert_eq!(entry.mode, 0o644 + (i % 2) as u32);
            assert_eq!(entry.mtime, 1700000000 + i as i64);
            assert_eq!(entry.chunk_start, i * 5);
            assert_eq!(entry.chunk_end, (i + 1) * 5);
            assert_eq!(entry.hash, [i as u8; 32]);
        }
    }

    #[test]
    fn test_file_table_unicode_paths() {
        let mut table = FileTable::new();

        table.push(FileEntry {
            path: "文件.txt".to_string(),
            size: 100,
            mode: 0o644,
            mtime: 0,
            chunk_start: 0,
            chunk_end: 1,
            hash: [0u8; 32],
        });

        table.push(FileEntry {
            path: "файл.dat".to_string(),
            size: 200,
            mode: 0o644,
            mtime: 0,
            chunk_start: 1,
            chunk_end: 2,
            hash: [1u8; 32],
        });

        let bytes = table.to_bytes().unwrap();
        let decoded = FileTable::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.len(), 2);
        assert!(decoded.get_by_path("文件.txt").is_some());
        assert!(decoded.get_by_path("файл.dat").is_some());
    }

    #[test]
    fn test_file_table_long_paths() {
        let mut table = FileTable::new();

        let long_path = "a/".repeat(100) + "file.txt";
        table.push(FileEntry {
            path: long_path.clone(),
            size: 1024,
            mode: 0o644,
            mtime: 12345,
            chunk_start: 0,
            chunk_end: 1,
            hash: [42u8; 32],
        });

        let bytes = table.to_bytes().unwrap();
        let decoded = FileTable::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.len(), 1);
        assert!(decoded.get_by_path(&long_path).is_some());
    }

    #[test]
    fn test_file_table_invalid_bytes() {
        let invalid_bytes = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result = FileTable::from_bytes(&invalid_bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_entry_equality() {
        let entry1 = FileEntry {
            path: "test.txt".to_string(),
            size: 100,
            mode: 0o644,
            mtime: 1234,
            chunk_start: 0,
            chunk_end: 1,
            hash: [1u8; 32],
        };

        let entry2 = FileEntry {
            path: "test.txt".to_string(),
            size: 100,
            mode: 0o644,
            mtime: 1234,
            chunk_start: 0,
            chunk_end: 1,
            hash: [1u8; 32],
        };

        let entry3 = FileEntry {
            path: "other.txt".to_string(),
            size: 100,
            mode: 0o644,
            mtime: 1234,
            chunk_start: 0,
            chunk_end: 1,
            hash: [1u8; 32],
        };

        assert_eq!(entry1, entry2);
        assert_ne!(entry1, entry3);
    }

    #[test]
    fn test_file_table_special_values() {
        let mut table = FileTable::new();

        table.push(FileEntry {
            path: "empty".to_string(),
            size: 0,
            mode: 0,
            mtime: 0,
            chunk_start: 0,
            chunk_end: 0,
            hash: [0u8; 32],
        });

        table.push(FileEntry {
            path: "max_values".to_string(),
            size: u64::MAX,
            mode: u32::MAX,
            mtime: i64::MAX,
            chunk_start: u64::MAX - 1,
            chunk_end: u64::MAX,
            hash: [255u8; 32],
        });

        table.push(FileEntry {
            path: "negative_time".to_string(),
            size: 1000,
            mode: 0o644,
            mtime: i64::MIN,
            chunk_start: 0,
            chunk_end: 1,
            hash: [128u8; 32],
        });

        let bytes = table.to_bytes().unwrap();
        let decoded = FileTable::from_bytes(&bytes).unwrap();

        assert_eq!(decoded, table);
    }
}
