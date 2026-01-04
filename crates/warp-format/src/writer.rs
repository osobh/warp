//! Writer implementation for creating .warp archives

use crate::Result;
use crate::file_table::{FileEntry, FileTable};
use crate::header::{Compression, Encryption, HEADER_SIZE, Header, flags};
use crate::index::{ChunkEntry, ChunkIndex};
use crate::merkle::MerkleTree;

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use warp_compress::{Compressor, Lz4Compressor, ZstdCompressor};
use warp_crypto::encrypt::Key;
use warp_io::{Chunker, ChunkerConfig};

/// Configuration for WarpWriter
#[derive(Clone)]
pub struct WarpWriterConfig {
    /// Compression algorithm to use
    pub compression: Compression,
    /// Chunk size for content-defined chunking (default: 4MB)
    pub chunk_size: u32,
    /// Minimum chunk size (default: 256KB)
    pub min_chunk_size: u32,
    /// Maximum chunk size (default: 16MB)
    pub max_chunk_size: u32,
    /// Encryption algorithm to use
    pub encryption: Encryption,
    /// Encryption key (if encryption is enabled)
    pub encryption_key: Option<Key>,
}

impl Default for WarpWriterConfig {
    fn default() -> Self {
        Self {
            compression: Compression::Zstd,
            chunk_size: 4 * 1024 * 1024,      // 4MB
            min_chunk_size: 256 * 1024,       // 256KB
            max_chunk_size: 16 * 1024 * 1024, // 16MB
            encryption: Encryption::None,
            encryption_key: None,
        }
    }
}

impl WarpWriterConfig {
    /// Create a new configuration with no compression
    pub fn no_compression() -> Self {
        Self {
            compression: Compression::None,
            ..Default::default()
        }
    }

    /// Create a new configuration with zstd compression
    pub fn with_zstd() -> Self {
        Self {
            compression: Compression::Zstd,
            ..Default::default()
        }
    }

    /// Create a new configuration with lz4 compression
    pub fn with_lz4() -> Self {
        Self {
            compression: Compression::Lz4,
            ..Default::default()
        }
    }

    /// Set chunk size
    pub fn chunk_size(mut self, size: u32) -> Self {
        self.chunk_size = size;
        self
    }

    /// Enable encryption with the given key
    pub fn with_encryption(mut self, key: Key) -> Self {
        self.encryption = Encryption::ChaCha20Poly1305;
        self.encryption_key = Some(key);
        self
    }
}

/// Writer for creating .warp archives
pub struct WarpWriter {
    file: File,
    header: Header,
    chunk_index: ChunkIndex,
    file_table: FileTable,
    chunk_hashes: Vec<[u8; 32]>,
    current_offset: u64,
    config: WarpWriterConfig,
    compressor: Option<Box<dyn Compressor>>,
}

impl WarpWriter {
    /// Create a new .warp file with default configuration
    pub fn create(path: &Path) -> Result<Self> {
        Self::create_with_config(path, WarpWriterConfig::default())
    }

    /// Create a new .warp file with custom configuration
    pub fn create_with_config(path: &Path, config: WarpWriterConfig) -> Result<Self> {
        let mut file = File::create(path)?;

        // Create header with appropriate settings
        let mut header = Header::new();
        header.compression = config.compression;
        header.chunk_size_hint = config.chunk_size;
        header.data_offset = HEADER_SIZE as u64;
        header.encryption = config.encryption;

        // Set up encryption if enabled
        if config.encryption != Encryption::None {
            header.flags |= flags::ENCRYPTED;
            // Generate random salt for key derivation
            header.salt = warp_crypto::kdf::generate_salt();
        }

        // Seek past the header - we'll write it at the end
        file.seek(SeekFrom::Start(HEADER_SIZE as u64))?;

        // Initialize compressor based on configuration
        let compressor: Option<Box<dyn Compressor>> = match config.compression {
            Compression::None => None,
            Compression::Zstd => Some(Box::new(ZstdCompressor::new(3)?)),
            Compression::Lz4 => Some(Box::new(Lz4Compressor::new())),
        };

        Ok(Self {
            file,
            header,
            chunk_index: ChunkIndex::new(),
            file_table: FileTable::new(),
            chunk_hashes: Vec::new(),
            current_offset: HEADER_SIZE as u64,
            config,
            compressor,
        })
    }

    /// Add a file to the archive
    pub fn add_file(&mut self, path: &Path, archive_path: &str) -> Result<()> {
        // Read file metadata
        let metadata = std::fs::metadata(path)?;
        let file_size = metadata.len();

        // Get modification time
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Get file mode (Unix permissions)
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = 0o644;

        // Open file for reading
        let mut file_reader = File::open(path)?;

        // Create chunker
        let chunker_config = ChunkerConfig {
            target_size: self.config.chunk_size as usize,
            min_size: self.config.min_chunk_size as usize,
            max_size: self.config.max_chunk_size as usize,
            window_size: 48, // Standard window size for buzhash
        };
        let chunker = Chunker::new(chunker_config);

        // Track the chunk range for this file
        let chunk_start = self.chunk_index.len() as u64;

        // Track hashes for whole-file hash computation
        let mut file_hash_data = Vec::new();

        // Chunk the file and write chunks
        let chunks = chunker.chunk(&mut file_reader)?;

        for chunk_data in chunks {
            // Hash the original chunk
            let chunk_hash = warp_hash::hash(&chunk_data);
            self.chunk_hashes.push(chunk_hash);

            // Accumulate data for file hash
            file_hash_data.extend_from_slice(&chunk_data);

            let original_size = chunk_data.len() as u32;
            let (mut data_to_write, mut compressed_size, flags) =
                if let Some(ref compressor) = self.compressor {
                    // Compress the chunk
                    match compressor.compress(&chunk_data) {
                        Ok(compressed) => {
                            // Only use compression if it actually reduces size
                            let compressed_len = compressed.len();
                            if compressed_len < chunk_data.len() {
                                (compressed, compressed_len as u32, 0x01) // compressed flag
                            } else {
                                (chunk_data, original_size, 0x00) // no compression
                            }
                        }
                        Err(_) => {
                            // On compression error, store uncompressed
                            (chunk_data, original_size, 0x00)
                        }
                    }
                } else {
                    (chunk_data, original_size, 0x00)
                };

            // Encrypt chunk after compression if encryption is enabled
            if let Some(ref key) = self.config.encryption_key {
                data_to_write = warp_crypto::encrypt(key, &data_to_write)
                    .map_err(|e| crate::Error::Corrupted(format!("Encryption failed: {}", e)))?;
                compressed_size = data_to_write.len() as u32;
            }

            // Write chunk to file
            self.file.write_all(&data_to_write)?;

            // Create chunk entry
            let entry = ChunkEntry {
                offset: self.current_offset,
                compressed_size,
                original_size,
                hash: chunk_hash,
                flags,
                _reserved: [0u8; 7],
            };

            self.chunk_index.push(entry);
            self.current_offset += compressed_size as u64;

            // Update header statistics
            self.header.total_chunks += 1;
            self.header.original_size += original_size as u64;
            self.header.compressed_size += compressed_size as u64;
        }

        let chunk_end = self.chunk_index.len() as u64;

        // Compute whole-file hash
        let file_hash = warp_hash::hash(&file_hash_data);

        // Add file entry to table
        let file_entry = FileEntry {
            path: archive_path.to_string(),
            size: file_size,
            mode,
            mtime,
            chunk_start,
            chunk_end,
            hash: file_hash,
        };

        self.file_table.push(file_entry);
        self.header.total_files += 1;

        Ok(())
    }

    /// Add a directory recursively
    pub fn add_directory(&mut self, path: &Path, prefix: &str) -> Result<()> {
        // Walk the directory
        let entries = warp_io::walk_directory(path)?;

        // Process each file (skip directories)
        for entry in entries {
            if entry.is_dir {
                continue;
            }

            // Construct archive path with prefix
            let relative_str = entry.relative_path.to_string_lossy();
            let archive_path = if prefix.is_empty() {
                relative_str.to_string()
            } else {
                format!("{}/{}", prefix.trim_end_matches('/'), relative_str)
            };

            // Add the file
            self.add_file(&entry.path, &archive_path)?;
        }

        Ok(())
    }

    /// Finalize and close the archive
    pub fn finish(mut self) -> Result<()> {
        // Build Merkle tree from chunk hashes
        let merkle_tree = if !self.chunk_hashes.is_empty() {
            MerkleTree::from_leaves(self.chunk_hashes)
        } else {
            MerkleTree::from_leaves(vec![[0u8; 32]])
        };
        self.header.merkle_root = merkle_tree.root();

        // Write chunk index
        self.header.index_offset = self.current_offset;
        let index_bytes = self.chunk_index.to_bytes();
        self.file.write_all(&index_bytes)?;
        self.header.index_size = index_bytes.len() as u64;
        self.current_offset += index_bytes.len() as u64;

        // Write file table
        self.header.file_table_offset = self.current_offset;
        let table_bytes = self.file_table.to_bytes()?;
        self.file.write_all(&table_bytes)?;
        self.header.file_table_size = table_bytes.len() as u64;
        self.current_offset += table_bytes.len() as u64;

        // Seek back to start and write header
        self.file.seek(SeekFrom::Start(0))?;
        let header_bytes = self.header.to_bytes();
        self.file.write_all(&header_bytes)?;

        // Flush and close
        self.file.flush()?;
        self.file.sync_all()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{NamedTempFile, tempdir};

    #[test]
    fn test_create_empty_archive() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let writer = WarpWriter::create(path).unwrap();
        writer.finish().unwrap();

        // Verify file exists and has at least a header
        let metadata = fs::metadata(path).unwrap();
        assert!(metadata.len() >= HEADER_SIZE as u64);
    }

    #[test]
    fn test_create_archive_with_config() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let config = WarpWriterConfig::with_lz4();
        let writer = WarpWriter::create_with_config(path, config).unwrap();
        writer.finish().unwrap();

        // Read header and verify compression setting
        let data = fs::read(path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();
        assert_eq!(header.compression, Compression::Lz4);
    }

    #[test]
    fn test_add_single_file() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create a test file
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, b"Hello, warp!").unwrap();

        // Create archive and add file
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer.add_file(&test_file, "test.txt").unwrap();
        writer.finish().unwrap();

        // Verify archive was created
        let metadata = fs::metadata(archive_path).unwrap();
        assert!(metadata.len() > HEADER_SIZE as u64);

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.total_files, 1);
        assert!(header.total_chunks > 0);
        assert!(header.original_size >= 12);
    }

    #[test]
    fn test_add_multiple_files() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test files
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("file1.txt"), b"Content 1").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), b"Content 2").unwrap();
        fs::write(temp_dir.path().join("file3.txt"), b"Content 3").unwrap();

        // Create archive and add files
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer
            .add_file(&temp_dir.path().join("file1.txt"), "file1.txt")
            .unwrap();
        writer
            .add_file(&temp_dir.path().join("file2.txt"), "file2.txt")
            .unwrap();
        writer
            .add_file(&temp_dir.path().join("file3.txt"), "file3.txt")
            .unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.total_files, 3);
        assert!(header.total_chunks >= 3);
    }

    #[test]
    fn test_add_directory() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test directory structure
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("root.txt"), b"Root file").unwrap();

        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();
        fs::write(sub_dir.join("sub.txt"), b"Sub file").unwrap();

        // Create archive and add directory
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer.add_directory(temp_dir.path(), "data").unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.total_files, 2);
    }

    #[test]
    fn test_no_compression() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create a test file with incompressible data
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("data.bin");
        let data = vec![42u8; 1000];
        fs::write(&test_file, &data).unwrap();

        // Create archive with no compression
        let config = WarpWriterConfig::no_compression();
        let mut writer = WarpWriter::create_with_config(archive_path, config).unwrap();
        writer.add_file(&test_file, "data.bin").unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let archive_data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = archive_data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.compression, Compression::None);
        assert_eq!(header.original_size, header.compressed_size);
    }

    #[test]
    fn test_large_file_chunking() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create a larger test file
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("large.dat");
        let data = vec![b'A'; 10 * 1024 * 1024]; // 10MB
        fs::write(&test_file, &data).unwrap();

        // Create archive with custom chunk size
        let config = WarpWriterConfig::default().chunk_size(2 * 1024 * 1024); // 2MB chunks
        let mut writer = WarpWriter::create_with_config(archive_path, config).unwrap();
        writer.add_file(&test_file, "large.dat").unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let archive_data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = archive_data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.total_files, 1);
        // Should have at least one chunk, but actual count depends on content-defined chunking
        assert!(header.total_chunks >= 1);
        assert_eq!(header.original_size, 10 * 1024 * 1024);
    }

    #[test]
    fn test_empty_file() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create empty file
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("empty.txt");
        fs::write(&test_file, b"").unwrap();

        // Create archive and add empty file
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer.add_file(&test_file, "empty.txt").unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.total_files, 1);
        assert_eq!(header.original_size, 0);
    }

    #[test]
    fn test_merkle_root_populated() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test file
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, b"Test data for merkle tree").unwrap();

        // Create archive
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer.add_file(&test_file, "test.txt").unwrap();
        writer.finish().unwrap();

        // Read and verify header has merkle root
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        // Merkle root should not be all zeros
        assert_ne!(header.merkle_root, [0u8; 32]);
    }

    #[test]
    fn test_unicode_paths() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test file with unicode name
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, b"Unicode test").unwrap();

        // Create archive with unicode archive path
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer.add_file(&test_file, "文件/测试.txt").unwrap();
        writer.finish().unwrap();

        // Verify archive was created successfully
        let metadata = fs::metadata(archive_path).unwrap();
        assert!(metadata.len() > HEADER_SIZE as u64);
    }

    #[test]
    fn test_config_builder() {
        let config = WarpWriterConfig::default().chunk_size(8 * 1024 * 1024);

        assert_eq!(config.chunk_size, 8 * 1024 * 1024);
        assert_eq!(config.compression, Compression::Zstd);
    }

    #[test]
    fn test_add_directory_empty_prefix() {
        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test directory
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("file.txt"), b"Test").unwrap();

        // Add directory with empty prefix
        let mut writer = WarpWriter::create(archive_path).unwrap();
        writer.add_directory(temp_dir.path(), "").unwrap();
        writer.finish().unwrap();

        // Verify success
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();
        assert_eq!(header.total_files, 1);
    }

    #[test]
    fn test_encrypted_archive() {
        use crate::header::{Encryption, flags};
        use warp_crypto::encrypt::Key;

        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test file
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("secret.txt");
        fs::write(&test_file, b"Secret data that should be encrypted").unwrap();

        // Create encrypted archive
        let key = Key::generate();
        let config = WarpWriterConfig::default().with_encryption(key);
        let mut writer = WarpWriter::create_with_config(archive_path, config).unwrap();
        writer.add_file(&test_file, "secret.txt").unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        // Verify encryption is set
        assert_eq!(header.encryption, Encryption::ChaCha20Poly1305);
        assert_eq!(header.flags & flags::ENCRYPTED, flags::ENCRYPTED);
        assert_ne!(header.salt, [0u8; 16]); // Salt should be non-zero

        // Verify data is actually encrypted (not plaintext)
        let data_section = &data[HEADER_SIZE..];
        let plaintext = b"Secret data that should be encrypted";
        assert!(
            !data_section
                .windows(plaintext.len())
                .any(|w| w == plaintext),
            "Data should be encrypted, not plaintext"
        );
    }

    #[test]
    fn test_encrypted_with_compression() {
        use warp_crypto::encrypt::Key;

        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test file with compressible data
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("data.txt");
        let data = vec![b'A'; 10000]; // Highly compressible
        fs::write(&test_file, &data).unwrap();

        // Create encrypted + compressed archive
        let key = Key::generate();
        let config = WarpWriterConfig::with_zstd().with_encryption(key);
        let mut writer = WarpWriter::create_with_config(archive_path, config).unwrap();
        writer.add_file(&test_file, "data.txt").unwrap();
        writer.finish().unwrap();

        // Verify archive was created and is smaller than original
        let metadata = fs::metadata(archive_path).unwrap();
        assert!(metadata.len() > HEADER_SIZE as u64);
        // With compression + encryption, should still be much smaller than 10KB original
        assert!(metadata.len() < 5000);
    }

    #[test]
    fn test_encrypted_no_compression() {
        use warp_crypto::encrypt::Key;

        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create test file
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("data.bin");
        fs::write(&test_file, b"Test data").unwrap();

        // Create encrypted archive without compression
        let key = Key::generate();
        let config = WarpWriterConfig::no_compression().with_encryption(key);
        let mut writer = WarpWriter::create_with_config(archive_path, config).unwrap();
        writer.add_file(&test_file, "data.bin").unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.compression, Compression::None);
        assert_eq!(header.encryption, Encryption::ChaCha20Poly1305);
    }

    #[test]
    fn test_encrypted_multiple_files() {
        use warp_crypto::encrypt::Key;

        let temp_archive = NamedTempFile::new().unwrap();
        let archive_path = temp_archive.path();

        // Create multiple test files
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("file1.txt"), b"Content 1").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), b"Content 2").unwrap();
        fs::write(temp_dir.path().join("file3.txt"), b"Content 3").unwrap();

        // Create encrypted archive
        let key = Key::generate();
        let config = WarpWriterConfig::default().with_encryption(key);
        let mut writer = WarpWriter::create_with_config(archive_path, config).unwrap();
        writer
            .add_file(&temp_dir.path().join("file1.txt"), "file1.txt")
            .unwrap();
        writer
            .add_file(&temp_dir.path().join("file2.txt"), "file2.txt")
            .unwrap();
        writer
            .add_file(&temp_dir.path().join("file3.txt"), "file3.txt")
            .unwrap();
        writer.finish().unwrap();

        // Read and verify header
        let data = fs::read(archive_path).unwrap();
        let header_bytes: [u8; HEADER_SIZE] = data[0..HEADER_SIZE].try_into().unwrap();
        let header = Header::from_bytes(&header_bytes).unwrap();

        assert_eq!(header.total_files, 3);
        assert_eq!(header.encryption, Encryption::ChaCha20Poly1305);
    }
}
