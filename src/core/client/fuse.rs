use crate::fs::FsBackend;
use fuser::{
    BsdFileFlags, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags,
    Generation, INodeNo, LockOwner, OpenFlags, RenameFlags, ReplyAttr,
    ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyStatfs,
    ReplyWrite, Request, TimeOrNow, WriteFlags,
};
use libc::{EACCES, EEXIST, EINVAL, EIO, ENOENT};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::*;

#[derive(Debug)]
/// Main client-side structure, implementing FUSE API endpoints that invoke
/// AZPFS protocol requests to the server
pub struct FUSEFilesystem<F: FsBackend> {
    backend: Mutex<F>,
    async_rt: tokio::runtime::Handle,
}

impl<F: FsBackend> FUSEFilesystem<F> {
    pub fn new(backend: F) -> Self {
        Self {
            backend: Mutex::new(backend),
            async_rt: tokio::runtime::Handle::current(),
        }
    }
}

fn to_fuser_attr(ino: INodeNo, attr: crate::fs::FileAttr) -> FileAttr {
    FileAttr {
        ino,
        size: attr.size,
        blocks: attr.blocks,
        atime: attr.access_time,
        mtime: attr.modification_time,
        ctime: attr.change_time,
        crtime: UNIX_EPOCH,
        kind: attr.file_type.into(),
        perm: attr.permissions,
        nlink: attr.n_hard_links,
        uid: attr.uid,
        gid: attr.gid,
        rdev: attr.rdev,
        blksize: attr.block_size,
        flags: 0,
    }
}

fn io_to_errno(e: &std::io::Error) -> Errno {
    Errno::from_i32(match e.kind() {
        std::io::ErrorKind::NotFound => ENOENT,
        std::io::ErrorKind::PermissionDenied => EACCES,
        std::io::ErrorKind::AlreadyExists => EEXIST,
        std::io::ErrorKind::InvalidInput => EINVAL,
        _ => EIO,
    })
}

fn resolve_time(t: Option<TimeOrNow>) -> Option<SystemTime> {
    match t {
        Some(TimeOrNow::Now) => Some(SystemTime::now()),
        Some(TimeOrNow::SpecificTime(t)) => Some(t),
        None => None,
    }
}

const TTL: Duration = Duration::from_secs(0);

impl<F: FsBackend> Filesystem for FUSEFilesystem<F> {
    #[instrument(skip(self, _req, reply))]
    fn lookup(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        reply: ReplyEntry,
    ) {
        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            let ino = backend.lookup(parent.0, Path::new(name)).await?;
            let attr = backend.get_attr(ino).await?;
            Ok::<_, std::io::Error>((ino, attr))
        }) {
            Ok((ino, attr)) => {
                reply.entry(&TTL, &to_fuser_attr(INodeNo(ino), attr), Generation(0))
            }
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, _fh, reply))]
    fn getattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: Option<FileHandle>,
        reply: ReplyAttr,
    ) {
        match self.async_rt.block_on(async {
            self.backend.lock().unwrap().get_attr(ino.0).await
        }) {
            Ok(attr) => reply.attr(&TTL, &to_fuser_attr(ino, attr)),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let permissions = mode.map(|m| m as u16);
        let atime = resolve_time(atime);
        let mtime = resolve_time(mtime);

        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            backend
                .set_attr(ino.0, size, atime, mtime, permissions, uid, gid)
                .await?;
            backend.get_attr(ino.0).await
        }) {
            Ok(attr) => reply.attr(&TTL, &to_fuser_attr(ino, attr)),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            let ino = backend
                .create_dir(parent.0, mode as u16, Path::new(name))
                .await?;
            let attr = backend.get_attr(ino).await?;
            Ok::<_, std::io::Error>((ino, attr))
        }) {
            Ok((ino, attr)) => {
                reply.entry(&TTL, &to_fuser_attr(INodeNo(ino), attr), Generation(0))
            }
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn unlink(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        reply: ReplyEmpty,
    ) {
        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            let ino = backend.lookup(parent.0, Path::new(name)).await?;
            backend.remove(ino).await
        }) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn rmdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        reply: ReplyEmpty,
    ) {
        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            let ino = backend.lookup(parent.0, Path::new(name)).await?;
            backend.remove(ino).await
        }) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        _flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            let ino = backend.lookup(parent.0, Path::new(name)).await?;
            backend
                .rename(ino, newparent.0, Path::new(newname))
                .await
        }) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        match self.async_rt.block_on(async {
            self.backend
                .lock()
                .unwrap()
                .read(ino.0, offset, size as u64)
                .await
        }) {
            Ok(data) => reply.data(&data),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, data, reply))]
    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let len = data.len();
        match self.async_rt.block_on(async {
            self.backend
                .lock()
                .unwrap()
                .write(ino.0, offset, data)
                .await
        }) {
            Ok(()) => reply.written(len as u32),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    #[instrument(skip(self, _req, reply))]
    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        match self.async_rt.block_on(async {
            self.backend.lock().unwrap().read_dir(ino.0).await
        }) {
            Ok(entries) => {
                for (i, e) in entries.into_iter().enumerate().skip(offset as usize)
                {
                    let ft: FileType = e.file_type.into();
                    let name = OsStr::from_bytes(&e.filename);
                    if reply.add(INodeNo(e.inode), (i + 1) as u64, ft, name) {
                        return;
                    }
                }
                reply.ok();
            }
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        match self.async_rt.block_on(async {
            let mut backend = self.backend.lock().unwrap();
            let ino = backend
                .create_file(parent.0, mode as u16, flags as u32, Path::new(name))
                .await?;
            let attr = backend.get_attr(ino).await?;
            Ok::<_, std::io::Error>((ino, attr))
        }) {
            Ok((ino, attr)) => reply.created(
                &TTL,
                &to_fuser_attr(INodeNo(ino), attr),
                Generation(0),
                FileHandle(0),
                FopenFlags::empty(),
            ),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }

    #[instrument(skip(self, _req, reply))]
    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        match self.async_rt.block_on(async {
            self.backend.lock().unwrap().stats().await
        }) {
            Ok(s) => reply.statfs(
                s.total_blocks,
                s.free_blocks,
                s.avail_blocks,
                s.total_inodes,
                s.free_inodes,
                s.block_size,
                s.max_filename_len,
                s.fragment_size,
            ),
            Err(e) => reply.error(io_to_errno(&e)),
        }
    }
}
