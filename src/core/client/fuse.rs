use crate::fs::FsBackend;
use fuser::{
    Errno, FileAttr, FileHandle, FileType, Filesystem, Generation, INodeNo,
    ReplyAttr, ReplyDirectory, ReplyEntry, Request,
};
use libc::ENOENT;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};
use tracing::*;

const TEST_FILENAME: &str = "foo";

#[derive(Debug)]
/// Main client-side structure, implementing FUSE API endpoints that invoke
/// AZPFS protocol requests to the server
pub struct FUSEFilesytem<F: FsBackend> {
    backend: Mutex<F>,
    async_rt: tokio::runtime::Handle,
}

impl<F: FsBackend> FUSEFilesytem<F> {
    pub fn new(backend: F) -> Self {
        Self {
            backend: Mutex::new(backend),
            async_rt: tokio::runtime::Handle::current(),
        }
    }
}

fn dir_attr(ino: INodeNo) -> FileAttr {
    FileAttr {
        ino,
        size: 0,
        blocks: 0,
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: FileType::Directory,
        perm: 0o755,
        nlink: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 512,
        flags: 0,
    }
}

impl<F: FsBackend> Filesystem for FUSEFilesytem<F> {
    #[instrument(skip(self, _req, reply))]
    fn lookup(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        reply: ReplyEntry,
    ) {
        match self.async_rt.block_on(async {
            self.backend
                .lock()
                .unwrap()
                .lookup(parent.0, Path::new(name))
                .await
        }) {
            Ok(ino) => reply.entry(
                &Duration::from_secs(0),
                &dir_attr(INodeNo(ino)),
                Generation(0),
            ),
            Err(_) => reply.error(Errno::from_i32(ENOENT)),
        }
    }

    #[instrument]
    fn getattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: Option<FileHandle>,
        reply: ReplyAttr,
    ) {
        match ino {
            INodeNo(1) | INodeNo(2) => {
                reply.attr(&Duration::from_secs(0), &dir_attr(ino));
            }
            _ => {
                reply.error(Errno::from_i32(ENOENT));
            }
        }
    }

    #[instrument]
    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        if ino != INodeNo(1) && ino != INodeNo(2) {
            reply.error(Errno::from_i32(ENOENT));
            return;
        }
        let entries = vec![
            (INodeNo(1), FileType::Directory, "."),
            (INodeNo(1), FileType::Directory, ".."),
            (INodeNo(2), FileType::Directory, TEST_FILENAME),
        ];
        for (i, (ino, kind, name)) in
            entries.into_iter().enumerate().skip(offset as usize)
        {
            if reply.add(ino, (i + 1) as u64, kind, name) {
                return;
            }
        }

        reply.ok();
    }
}
