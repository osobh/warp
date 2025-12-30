//! Metadata structures for warp-fs
//!
//! All metadata is stored as objects in warp-store under a special prefix.

use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// File type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub enum FileType {
    /// Regular file
    RegularFile,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Named pipe (FIFO)
    NamedPipe,
    /// Character device
    CharDevice,
    /// Block device
    BlockDevice,
    /// Unix socket
    Socket,
}

impl FileType {
    /// Convert to FUSE file type
    pub fn to_fuse(&self) -> fuser::FileType {
        match self {
            FileType::RegularFile => fuser::FileType::RegularFile,
            FileType::Directory => fuser::FileType::Directory,
            FileType::Symlink => fuser::FileType::Symlink,
            FileType::NamedPipe => fuser::FileType::NamedPipe,
            FileType::CharDevice => fuser::FileType::CharDevice,
            FileType::BlockDevice => fuser::FileType::BlockDevice,
            FileType::Socket => fuser::FileType::Socket,
        }
    }

    /// Convert from mode bits
    pub fn from_mode(mode: u32) -> Option<Self> {
        match mode & libc::S_IFMT as u32 {
            m if m == libc::S_IFREG as u32 => Some(FileType::RegularFile),
            m if m == libc::S_IFDIR as u32 => Some(FileType::Directory),
            m if m == libc::S_IFLNK as u32 => Some(FileType::Symlink),
            m if m == libc::S_IFIFO as u32 => Some(FileType::NamedPipe),
            m if m == libc::S_IFCHR as u32 => Some(FileType::CharDevice),
            m if m == libc::S_IFBLK as u32 => Some(FileType::BlockDevice),
            m if m == libc::S_IFSOCK as u32 => Some(FileType::Socket),
            _ => None,
        }
    }

    /// Get the mode bits for this file type
    pub fn mode_bits(&self) -> u32 {
        match self {
            FileType::RegularFile => libc::S_IFREG as u32,
            FileType::Directory => libc::S_IFDIR as u32,
            FileType::Symlink => libc::S_IFLNK as u32,
            FileType::NamedPipe => libc::S_IFIFO as u32,
            FileType::CharDevice => libc::S_IFCHR as u32,
            FileType::BlockDevice => libc::S_IFBLK as u32,
            FileType::Socket => libc::S_IFSOCK as u32,
        }
    }
}

/// Stored timestamp (nanoseconds since UNIX epoch)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub struct StoredTime {
    /// Seconds since UNIX epoch
    pub secs: i64,
    /// Nanoseconds (0-999_999_999)
    pub nsecs: u32,
}

impl StoredTime {
    /// Current time
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    /// Convert from SystemTime
    pub fn from_system_time(time: SystemTime) -> Self {
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => Self {
                secs: d.as_secs() as i64,
                nsecs: d.subsec_nanos(),
            },
            Err(e) => {
                let d = e.duration();
                Self {
                    secs: -(d.as_secs() as i64),
                    nsecs: d.subsec_nanos(),
                }
            }
        }
    }

    /// Convert to SystemTime
    pub fn to_system_time(&self) -> SystemTime {
        if self.secs >= 0 {
            SystemTime::UNIX_EPOCH + std::time::Duration::new(self.secs as u64, self.nsecs)
        } else {
            SystemTime::UNIX_EPOCH - std::time::Duration::new((-self.secs) as u64, self.nsecs)
        }
    }
}

impl Default for StoredTime {
    fn default() -> Self {
        Self::now()
    }
}

/// A mapping of a file region to a warp-store object
#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub struct DataExtent {
    /// Offset within the file
    pub file_offset: u64,

    /// Length of this extent
    pub length: u64,

    /// Object key in warp-store (bucket/key)
    pub object_key: String,

    /// Offset within the object
    pub object_offset: u64,
}

impl DataExtent {
    /// Create a new data extent
    pub fn new(file_offset: u64, length: u64, object_key: String, object_offset: u64) -> Self {
        Self {
            file_offset,
            length,
            object_key,
            object_offset,
        }
    }

    /// Check if this extent contains the given file offset
    pub fn contains(&self, offset: u64) -> bool {
        offset >= self.file_offset && offset < self.file_offset + self.length
    }

    /// Check if this extent overlaps with the given range
    pub fn overlaps(&self, start: u64, len: u64) -> bool {
        let end = self.file_offset + self.length;
        let range_end = start + len;
        start < end && range_end > self.file_offset
    }
}

/// Complete inode metadata
///
/// This is stored as a serialized object in warp-store under
/// `__warp_fs_meta__/inodes/{ino}`
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub struct InodeMetadata {
    /// Inode number
    pub ino: u64,

    /// File type
    pub file_type: FileType,

    /// Permission bits (without file type)
    pub mode: u32,

    /// Owner user ID
    pub uid: u32,

    /// Owner group ID
    pub gid: u32,

    /// File size in bytes
    pub size: u64,

    /// Number of hard links
    pub nlink: u32,

    /// Last access time
    pub atime: StoredTime,

    /// Last modification time
    pub mtime: StoredTime,

    /// Last status change time
    pub ctime: StoredTime,

    /// Creation/birth time
    pub crtime: StoredTime,

    /// Device ID (for device files)
    pub rdev: u64,

    /// Symlink target (for symlinks)
    pub symlink_target: Option<String>,

    /// Extended attributes
    pub xattrs: HashMap<String, Vec<u8>>,

    /// Data extents (for regular files)
    /// Maps file regions to warp-store objects
    pub data_extents: Vec<DataExtent>,

    /// Flags (reserved for future use)
    pub flags: u32,

    /// Generation number (for NFS)
    pub generation: u64,
}

impl InodeMetadata {
    /// Create metadata for a new directory
    pub fn new_directory(ino: u64, mode: u32, uid: u32, gid: u32) -> Self {
        let now = StoredTime::now();
        Self {
            ino,
            file_type: FileType::Directory,
            mode: mode & 0o7777,
            uid,
            gid,
            size: 0,
            nlink: 2, // . and ..
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            rdev: 0,
            symlink_target: None,
            xattrs: HashMap::new(),
            data_extents: Vec::new(),
            flags: 0,
            generation: 0,
        }
    }

    /// Create metadata for a new regular file
    pub fn new_file(ino: u64, mode: u32, uid: u32, gid: u32) -> Self {
        let now = StoredTime::now();
        Self {
            ino,
            file_type: FileType::RegularFile,
            mode: mode & 0o7777,
            uid,
            gid,
            size: 0,
            nlink: 1,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            rdev: 0,
            symlink_target: None,
            xattrs: HashMap::new(),
            data_extents: Vec::new(),
            flags: 0,
            generation: 0,
        }
    }

    /// Create metadata for a new symlink
    pub fn new_symlink(ino: u64, uid: u32, gid: u32, target: String) -> Self {
        let now = StoredTime::now();
        let size = target.len() as u64;
        Self {
            ino,
            file_type: FileType::Symlink,
            mode: 0o777,
            uid,
            gid,
            size,
            nlink: 1,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            rdev: 0,
            symlink_target: Some(target),
            xattrs: HashMap::new(),
            data_extents: Vec::new(),
            flags: 0,
            generation: 0,
        }
    }

    /// Create the root directory inode
    pub fn root(uid: u32, gid: u32, mode: u32) -> Self {
        Self::new_directory(1, mode, uid, gid)
    }

    /// Get full mode including file type
    pub fn full_mode(&self) -> u32 {
        self.file_type.mode_bits() | self.mode
    }

    /// Check if this is a directory
    pub fn is_dir(&self) -> bool {
        self.file_type == FileType::Directory
    }

    /// Check if this is a regular file
    pub fn is_file(&self) -> bool {
        self.file_type == FileType::RegularFile
    }

    /// Check if this is a symlink
    pub fn is_symlink(&self) -> bool {
        self.file_type == FileType::Symlink
    }

    /// Update access time
    pub fn touch_atime(&mut self) {
        self.atime = StoredTime::now();
    }

    /// Update modification time (also updates ctime)
    pub fn touch_mtime(&mut self) {
        let now = StoredTime::now();
        self.mtime = now;
        self.ctime = now;
    }

    /// Update ctime only
    pub fn touch_ctime(&mut self) {
        self.ctime = StoredTime::now();
    }

    /// Convert to FUSE file attributes
    pub fn to_file_attr(&self, block_size: u32) -> fuser::FileAttr {
        let blocks = (self.size + block_size as u64 - 1) / block_size as u64;

        fuser::FileAttr {
            ino: self.ino,
            size: self.size,
            blocks,
            atime: self.atime.to_system_time(),
            mtime: self.mtime.to_system_time(),
            ctime: self.ctime.to_system_time(),
            crtime: self.crtime.to_system_time(),
            kind: self.file_type.to_fuse(),
            perm: self.mode as u16,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: self.rdev as u32,
            blksize: block_size,
            flags: self.flags,
        }
    }

    /// Serialize to bytes using rkyv for storage
    pub fn to_bytes(&self) -> Result<Vec<u8>, crate::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.to_vec())
            .map_err(|e| crate::Error::MetadataSerialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|e| crate::Error::MetadataDeserialization(e.to_string()))
    }
}

/// Directory entry for storage
#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub struct DirectoryEntry {
    /// Entry name
    pub name: String,

    /// Inode number
    pub ino: u64,

    /// File type (for readdir optimization)
    pub file_type: FileType,
}

impl DirectoryEntry {
    /// Create a new directory entry
    pub fn new(name: String, ino: u64, file_type: FileType) -> Self {
        Self {
            name,
            ino,
            file_type,
        }
    }
}

/// Directory contents
///
/// Stored as `__warp_fs_meta__/dirs/{parent_ino}`
#[derive(Debug, Clone, Default, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub struct DirectoryContents {
    /// Parent inode
    pub parent_ino: u64,

    /// This directory's inode
    pub self_ino: u64,

    /// Entries (excluding . and ..)
    pub entries: Vec<DirectoryEntry>,
}

impl DirectoryContents {
    /// Create new empty directory contents
    pub fn new(self_ino: u64, parent_ino: u64) -> Self {
        Self {
            parent_ino,
            self_ino,
            entries: Vec::new(),
        }
    }

    /// Add an entry
    pub fn add(&mut self, entry: DirectoryEntry) {
        // Remove existing entry with same name if any
        self.entries.retain(|e| e.name != entry.name);
        self.entries.push(entry);
    }

    /// Remove an entry by name
    pub fn remove(&mut self, name: &str) -> Option<DirectoryEntry> {
        if let Some(pos) = self.entries.iter().position(|e| e.name == name) {
            Some(self.entries.remove(pos))
        } else {
            None
        }
    }

    /// Get an entry by name
    pub fn get(&self, name: &str) -> Option<&DirectoryEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Check if empty (excluding . and ..)
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, crate::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.to_vec())
            .map_err(|e| crate::Error::MetadataSerialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|e| crate::Error::MetadataDeserialization(e.to_string()))
    }
}

/// Superblock for the filesystem
///
/// Stored as `__warp_fs_meta__/superblock`
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
#[derive(SerdeSerialize, SerdeDeserialize)]
pub struct Superblock {
    /// Magic number for validation
    pub magic: u64,

    /// Filesystem version
    pub version: u32,

    /// Next available inode number
    pub next_ino: u64,

    /// Total inodes in use
    pub inode_count: u64,

    /// Total bytes stored
    pub total_bytes: u64,

    /// Creation time
    pub created: StoredTime,

    /// Last mount time
    pub last_mount: StoredTime,

    /// Block size
    pub block_size: u32,

    /// Filesystem label
    pub label: String,

    /// Filesystem UUID
    pub uuid: [u8; 16],
}

impl Superblock {
    /// Magic number for warp-fs
    pub const MAGIC: u64 = 0x5741_5250_4653_0001; // "WARPFS\x00\x01"

    /// Current version
    pub const VERSION: u32 = 1;

    /// Create a new superblock
    pub fn new(block_size: u32, label: String) -> Self {
        let now = StoredTime::now();
        let mut uuid = [0u8; 16];
        // Generate a random UUID
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        now.secs.hash(&mut hasher);
        now.nsecs.hash(&mut hasher);
        let h = hasher.finish();
        uuid[0..8].copy_from_slice(&h.to_le_bytes());
        uuid[8..16].copy_from_slice(&h.to_be_bytes());
        // Set version and variant bits for UUID v4
        uuid[6] = (uuid[6] & 0x0f) | 0x40;
        uuid[8] = (uuid[8] & 0x3f) | 0x80;

        Self {
            magic: Self::MAGIC,
            version: Self::VERSION,
            next_ino: 2, // 1 is root
            inode_count: 1,
            total_bytes: 0,
            created: now,
            last_mount: now,
            block_size,
            label,
            uuid,
        }
    }

    /// Allocate a new inode number
    pub fn alloc_ino(&mut self) -> u64 {
        let ino = self.next_ino;
        self.next_ino += 1;
        self.inode_count += 1;
        ino
    }

    /// Free an inode
    pub fn free_ino(&mut self) {
        self.inode_count = self.inode_count.saturating_sub(1);
    }

    /// Validate the superblock
    pub fn validate(&self) -> Result<(), crate::Error> {
        if self.magic != Self::MAGIC {
            return Err(crate::Error::Internal("Invalid superblock magic".to_string()));
        }
        if self.version > Self::VERSION {
            return Err(crate::Error::Internal(format!(
                "Unsupported filesystem version: {}",
                self.version
            )));
        }
        Ok(())
    }

    /// Update last mount time
    pub fn touch_mount(&mut self) {
        self.last_mount = StoredTime::now();
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, crate::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.to_vec())
            .map_err(|e| crate::Error::MetadataSerialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|e| crate::Error::MetadataDeserialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_type_mode_bits() {
        assert_eq!(FileType::RegularFile.mode_bits(), libc::S_IFREG as u32);
        assert_eq!(FileType::Directory.mode_bits(), libc::S_IFDIR as u32);
        assert_eq!(FileType::Symlink.mode_bits(), libc::S_IFLNK as u32);
    }

    #[test]
    fn test_inode_metadata_serialization() {
        let meta = InodeMetadata::new_file(42, 0o644, 1000, 1000);
        let bytes = meta.to_bytes().unwrap();
        let restored = InodeMetadata::from_bytes(&bytes).unwrap();
        assert_eq!(restored.ino, 42);
        assert_eq!(restored.mode, 0o644);
    }

    #[test]
    fn test_directory_contents() {
        let mut dir = DirectoryContents::new(2, 1);
        dir.add(DirectoryEntry::new("file.txt".to_string(), 3, FileType::RegularFile));
        dir.add(DirectoryEntry::new("subdir".to_string(), 4, FileType::Directory));

        assert_eq!(dir.len(), 2);
        assert!(dir.get("file.txt").is_some());

        let bytes = dir.to_bytes().unwrap();
        let restored = DirectoryContents::from_bytes(&bytes).unwrap();
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn test_superblock() {
        let sb = Superblock::new(4096, "test-fs".to_string());
        assert!(sb.validate().is_ok());

        let bytes = sb.to_bytes().unwrap();
        let restored = Superblock::from_bytes(&bytes).unwrap();
        assert!(restored.validate().is_ok());
        assert_eq!(restored.label, "test-fs");
    }
}
