use std::time::{Duration, UNIX_EPOCH};

use fuser::{
    Errno, FileAttr, FileHandle, FileType, Filesystem, Generation, INodeNo,
    ReplyAttr, ReplyDirectory, ReplyEntry, Request,
};
use libc::ENOENT;
use std::ffi::OsStr;

const TEST_FILENAME: &str = "foo";

pub struct FUSEFilesytem;

impl FUSEFilesytem {
    pub fn new() -> Self {
        Self
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

impl Filesystem for FUSEFilesytem {
    fn lookup(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        reply: ReplyEntry,
    ) {
        if parent == INodeNo(1) && name == TEST_FILENAME {
            reply.entry(
                &Duration::from_secs(0),
                &dir_attr(INodeNo(2)),
                Generation(0),
            );
        } else {
            reply.error(Errno::from_i32(ENOENT));
        }
    }

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
