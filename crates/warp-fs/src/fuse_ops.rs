//! FUSE filesystem operations
//!
//! Implements the fuser::Filesystem trait for warp-fs.

use crate::WarpFsConfig;
use crate::error::Error;
use crate::inode::ROOT_INO;
use crate::metadata::FileType;
use crate::vfs::VirtualFilesystem;

use fuser::{
    FileAttr, FileType as FuseFileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request, TimeOrNow,
};
use libc::{ENOENT, ENOSYS};
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::runtime::Runtime;
use tracing::{debug, error, warn};

/// TTL for cached attributes
const ATTR_TTL: Duration = Duration::from_secs(1);

/// The FUSE filesystem implementation
pub struct WarpFuseFs {
    /// VFS layer
    vfs: Arc<VirtualFilesystem>,
    /// Configuration
    config: WarpFsConfig,
    /// Tokio runtime for async operations
    rt: Runtime,
}

impl WarpFuseFs {
    /// Create a new FUSE filesystem
    pub fn new(vfs: Arc<VirtualFilesystem>, config: WarpFsConfig) -> Self {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        Self { vfs, config, rt }
    }

    /// Convert our error to FUSE errno
    fn error_to_errno(e: &Error) -> i32 {
        e.to_errno()
    }

    /// Get file attributes for an inode
    fn get_attr(&self, ino: u64) -> Result<FileAttr, i32> {
        self.rt.block_on(async {
            let inode = self.vfs.load_inode(ino).await.map_err(|e| {
                debug!(ino, error = %e, "Failed to load inode");
                Self::error_to_errno(&e)
            })?;

            let guard = inode.read();
            Ok(guard.to_file_attr(self.config.block_size))
        })
    }
}

impl Filesystem for WarpFuseFs {
    fn init(
        &mut self,
        _req: &Request<'_>,
        _config: &mut fuser::KernelConfig,
    ) -> Result<(), libc::c_int> {
        debug!("FUSE init");
        Ok(())
    }

    fn destroy(&mut self) {
        debug!("FUSE destroy");
        // Sync any pending data
        let _ = self.rt.block_on(self.vfs.sync());
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!(parent, name = ?name, "lookup");

        match self.rt.block_on(self.vfs.lookup(parent, name)) {
            Ok(inode) => {
                let guard = inode.read();
                let attr = guard.to_file_attr(self.config.block_size);
                reply.entry(&ATTR_TTL, &attr, 0);
            }
            Err(e) => {
                debug!(parent, name = ?name, error = %e, "lookup failed");
                reply.error(Self::error_to_errno(&e));
            }
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!(ino, "getattr");

        match self.get_attr(ino) {
            Ok(attr) => reply.attr(&ATTR_TTL, &attr),
            Err(errno) => reply.error(errno),
        }
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!(ino, "setattr");

        let result = self.rt.block_on(async {
            let inode = self.vfs.load_inode(ino).await?;
            let mut guard = inode.write();
            let meta = guard.metadata_mut();

            if let Some(m) = mode {
                meta.mode = m & 0o7777;
            }
            if let Some(u) = uid {
                meta.uid = u;
            }
            if let Some(g) = gid {
                meta.gid = g;
            }
            if let Some(s) = size {
                meta.size = s;
                // TODO: Truncate/extend file data
            }
            if let Some(a) = atime {
                meta.atime = match a {
                    TimeOrNow::Now => crate::metadata::StoredTime::now(),
                    TimeOrNow::SpecificTime(t) => crate::metadata::StoredTime::from_system_time(t),
                };
            }
            if let Some(m) = mtime {
                meta.mtime = match m {
                    TimeOrNow::Now => crate::metadata::StoredTime::now(),
                    TimeOrNow::SpecificTime(t) => crate::metadata::StoredTime::from_system_time(t),
                };
            }

            meta.touch_ctime();
            let attr = meta.to_file_attr(self.config.block_size);

            // Save to storage
            self.vfs.save_inode(meta).await?;

            Ok::<_, Error>(attr)
        });

        match result {
            Ok(attr) => reply.attr(&ATTR_TTL, &attr),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!(ino, offset, size, "read");

        let result = self.rt.block_on(self.vfs.read(ino, offset as u64, size));

        match result {
            Ok(data) => reply.data(&data),
            Err(e) => {
                error!(ino, offset, size, error = %e, "read failed");
                reply.error(Self::error_to_errno(&e));
            }
        }
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!(ino, offset, size = data.len(), "write");

        let result = self.rt.block_on(self.vfs.write(ino, offset as u64, data));

        match result {
            Ok(written) => reply.written(written),
            Err(e) => {
                error!(ino, offset, error = %e, "write failed");
                reply.error(Self::error_to_errno(&e));
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!(ino, offset, "readdir");

        let result = self.rt.block_on(async {
            let inode = self.vfs.load_inode(ino).await?;
            let guard = inode.read();

            if !guard.is_dir() {
                return Err(Error::NotADirectory(format!("inode {}", ino)));
            }

            let dir = self.vfs.load_directory(ino).await?;
            Ok((guard.metadata().clone(), dir))
        });

        let (meta, dir) = match result {
            Ok(d) => d,
            Err(e) => {
                reply.error(Self::error_to_errno(&e));
                return;
            }
        };

        let mut entries = vec![
            (ino, FuseFileType::Directory, "."),
            (dir.parent_ino, FuseFileType::Directory, ".."),
        ];

        for entry in &dir.entries {
            entries.push((entry.ino, entry.file_type.to_fuse(), &entry.name));
        }

        for (i, (ino, file_type, name)) in entries.iter().enumerate().skip(offset as usize) {
            // Reply buffer full
            if reply.add(*ino, (i + 1) as i64, *file_type, name) {
                break;
            }
        }

        reply.ok();
    }

    fn create(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        debug!(parent, name = ?name, mode, "create");

        let uid = req.uid();
        let gid = req.gid();

        let result = self.rt.block_on(async {
            let inode = self.vfs.create_file(parent, name, mode, uid, gid).await?;
            let ino = inode.read().ino();
            let fh = self.vfs.open(ino, flags)?;
            let attr = inode.read().to_file_attr(self.config.block_size);
            Ok::<_, Error>((attr, fh))
        });

        match result {
            Ok((attr, fh)) => reply.created(&ATTR_TTL, &attr, 0, fh, 0),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn mkdir(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        debug!(parent, name = ?name, mode, "mkdir");

        let uid = req.uid();
        let gid = req.gid();

        let result = self.rt.block_on(async {
            let inode = self.vfs.create_dir(parent, name, mode, uid, gid).await?;
            let attr = inode.read().to_file_attr(self.config.block_size);
            Ok::<_, Error>(attr)
        });

        match result {
            Ok(attr) => reply.entry(&ATTR_TTL, &attr, 0),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!(parent, name = ?name, "unlink");

        let result = self.rt.block_on(self.vfs.unlink(parent, name));

        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!(parent, name = ?name, "rmdir");

        let result = self.rt.block_on(self.vfs.rmdir(parent, name));

        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        debug!(ino, flags, "open");

        match self.vfs.open(ino, flags) {
            Ok(fh) => reply.opened(fh, 0),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        debug!(ino, fh, "release");

        let result = self.rt.block_on(self.vfs.close(fh));

        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn opendir(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        debug!(ino, flags, "opendir");

        // Verify it's a directory
        let result = self.rt.block_on(async {
            let inode = self.vfs.load_inode(ino).await?;
            let guard = inode.read();
            if !guard.is_dir() {
                return Err(Error::NotADirectory(format!("inode {}", ino)));
            }
            Ok(())
        });

        match result {
            Ok(()) => {
                // Directory handles don't need tracking for now
                reply.opened(0, 0);
            }
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn releasedir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        _flags: i32,
        reply: ReplyEmpty,
    ) {
        debug!(ino, fh, "releasedir");
        reply.ok();
    }

    fn fsync(&mut self, _req: &Request<'_>, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        debug!(ino, fh, datasync, "fsync");

        let result = self.rt.block_on(self.vfs.sync());

        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn fsyncdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        datasync: bool,
        reply: ReplyEmpty,
    ) {
        debug!(ino, fh, datasync, "fsyncdir");
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request<'_>, ino: u64, reply: fuser::ReplyStatfs) {
        debug!(ino, "statfs");

        let stats = self.vfs.stats();

        // Provide reasonable defaults for a virtual filesystem
        let blocks = stats.total_bytes / self.config.block_size as u64;
        let free_blocks = u64::MAX / 2; // Unlimited-ish

        reply.statfs(
            blocks.max(1),          // blocks
            free_blocks,            // bfree
            free_blocks,            // bavail
            stats.total_objects,    // files
            u64::MAX / 2,           // ffree
            self.config.block_size, // bsize
            255,                    // namelen
            self.config.block_size, // frsize
        );
    }

    fn access(&mut self, _req: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        debug!(ino, mask, "access");

        // For now, allow all access (DefaultPermissions handles checks)
        match self.get_attr(ino) {
            Ok(_) => reply.ok(),
            Err(errno) => reply.error(errno),
        }
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        debug!(parent, name = ?name, newparent, newname = ?newname, "rename");

        let result = self.rt.block_on(async {
            let name_str = name
                .to_str()
                .ok_or_else(|| Error::InvalidFileName(format!("{:?}", name)))?;
            let newname_str = newname
                .to_str()
                .ok_or_else(|| Error::InvalidFileName(format!("{:?}", newname)))?;

            // Load source directory and find entry
            let mut src_dir = self.vfs.load_directory(parent).await?;
            let entry = src_dir
                .get(name_str)
                .ok_or_else(|| Error::FileNotFound(name_str.to_string()))?
                .clone();

            // Load destination directory
            let mut dst_dir = if parent == newparent {
                src_dir.clone()
            } else {
                self.vfs.load_directory(newparent).await?
            };

            // Check if destination exists
            if dst_dir.get(newname_str).is_some() {
                // Remove existing destination
                dst_dir.remove(newname_str);
            }

            // Remove from source
            src_dir.remove(name_str);

            // Add to destination
            dst_dir.add(crate::metadata::DirectoryEntry::new(
                newname_str.to_string(),
                entry.ino,
                entry.file_type,
            ));

            // Save directories
            self.vfs.save_directory(&src_dir).await?;
            if parent != newparent {
                self.vfs.save_directory(&dst_dir).await?;
            }

            Ok::<_, Error>(())
        });

        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(Self::error_to_errno(&e)),
        }
    }

    fn flush(&mut self, _req: &Request<'_>, ino: u64, fh: u64, lock_owner: u64, reply: ReplyEmpty) {
        debug!(ino, fh, lock_owner, "flush");
        reply.ok();
    }
}
