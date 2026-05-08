use crate::protocol::Message;
use binrw::binrw;
use std::{
    fmt::Debug,
    fs,
    future::Future,
    io::Result,
    os::unix::fs::FileTypeExt,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::*;

mod disk;
mod handler;

pub use disk::DiskFs;
pub use handler::ClientHandler;

#[binrw]
#[brw(big, repr = u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Pipe = 0,
    CharDevice = 1,
    BlockDevice = 2,
    Directory = 3,
    RegularFile = 4,
    Symlink = 5,
    Socket = 6,
}

impl TryFrom<u8> for FileType {
    type Error = binrw::Error;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Pipe),
            1 => Ok(Self::CharDevice),
            2 => Ok(Self::BlockDevice),
            3 => Ok(Self::Directory),
            4 => Ok(Self::RegularFile),
            5 => Ok(Self::Symlink),
            6 => Ok(Self::Socket),
            _ => Err(binrw::Error::NoVariantMatch { pos: 0 }),
        }
    }
}

impl From<FileType> for fuser::FileType {
    fn from(value: FileType) -> Self {
        match value {
            FileType::Pipe => Self::NamedPipe,
            FileType::CharDevice => Self::CharDevice,
            FileType::BlockDevice => Self::BlockDevice,
            FileType::Directory => Self::Directory,
            FileType::RegularFile => Self::RegularFile,
            FileType::Symlink => Self::Symlink,
            FileType::Socket => Self::Socket,
        }
    }
}

impl From<fs::FileType> for FileType {
    fn from(value: fs::FileType) -> Self {
        if value.is_fifo() {
            Self::Pipe
        } else if value.is_char_device() {
            Self::CharDevice
        } else if value.is_block_device() {
            Self::BlockDevice
        } else if value.is_dir() {
            Self::Directory
        } else if value.is_file() {
            Self::RegularFile
        } else if value.is_symlink() {
            Self::Symlink
        } else if value.is_socket() {
            Self::Socket
        } else {
            error!(type=?value, "Invalid fs::FileType. This shouldn't happen.");
            panic!("Unrecoverable error")
        }
    }
}

#[binrw]
#[brw(big)]
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub inode: u64,
    #[br(try_map = |b: u8| FileType::try_from(b))]
    #[bw(map = |ft: &FileType| *ft as u8)]
    pub file_type: FileType,
    #[br(temp)]
    #[bw(calc = filename.len() as u8)]
    filename_len: u8,
    #[br(count = filename_len)]
    pub filename: Vec<u8>,
}

pub fn to_unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn from_unix(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[derive(Debug, Clone)]
pub struct FileAttr {
    pub file_type: FileType,
    pub size: u64,
    pub blocks: u64,
    pub access_time: SystemTime,
    pub modification_time: SystemTime,
    pub change_time: SystemTime,
    /// Unix permission bits (e.g. 0o755)
    pub permissions: u16,
    pub n_hard_links: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub block_size: u32,
}

impl FileAttr {
    pub fn into_message(self, request_id: u32) -> Message {
        Message::FileAttrRes {
            request_id,
            file_type: self.file_type,
            size: self.size,
            blocks: self.blocks,
            atime: to_unix(self.access_time),
            mtime: to_unix(self.modification_time),
            ctime: to_unix(self.change_time),
            permissions: self.permissions,
            nlinks: self.n_hard_links,
            uid: self.uid,
            gid: self.gid,
            rdev: self.rdev,
            blksize: self.block_size,
        }
    }

    pub fn from_message(msg: Message) -> Option<Self> {
        match msg {
            Message::FileAttrRes {
                file_type,
                size,
                blocks,
                atime,
                mtime,
                ctime,
                permissions,
                nlinks,
                uid,
                gid,
                rdev,
                blksize,
                ..
            } => Some(Self {
                file_type,
                size,
                blocks,
                access_time: from_unix(atime),
                modification_time: from_unix(mtime),
                change_time: from_unix(ctime),
                permissions,
                n_hard_links: nlinks,
                uid,
                gid,
                rdev,
                block_size: blksize,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FsStats {
    pub block_size: u32,
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub avail_blocks: u64,
    pub total_inodes: u64,
    pub free_inodes: u64,
    pub max_filename_len: u32,
    pub fragment_size: u32,
}

impl FsStats {
    pub fn into_message(self, request_id: u32) -> Message {
        Message::StatsRes {
            request_id,
            blksize: self.block_size,
            blocks: self.total_blocks,
            free_blocks: self.free_blocks,
            avail_blocks: self.avail_blocks,
            total_inodes: self.total_inodes,
            free_inodes: self.free_inodes,
            max_filename_len: self.max_filename_len,
            fragment_size: self.fragment_size,
        }
    }

    pub fn from_message(msg: Message) -> Option<Self> {
        match msg {
            Message::StatsRes {
                blksize,
                blocks,
                free_blocks,
                avail_blocks,
                total_inodes,
                free_inodes,
                max_filename_len,
                fragment_size,
                ..
            } => Some(Self {
                block_size: blksize,
                total_blocks: blocks,
                free_blocks,
                avail_blocks,
                total_inodes,
                free_inodes,
                max_filename_len,
                fragment_size,
            }),
            _ => None,
        }
    }
}

/// A generic trait for any filesystem backend
pub trait FsBackend: Debug + Send + Sync + 'static {
    /// Looks up a file by name and directory inode, and returns that file's
    /// inode
    fn lookup(
        &mut self,
        parent_inode: u64,
        filename: &Path,
    ) -> impl Future<Output = Result<u64>> + Send;

    /// Gets the attributes of a file by inode num
    fn get_attr(
        &mut self,
        inode: u64,
    ) -> impl Future<Output = Result<FileAttr>> + Send;

    /// Sets attributes on a file. Only the provided (Some) fields are changed.
    fn set_attr(
        &mut self,
        inode: u64,
        size: Option<u64>,
        atime: Option<SystemTime>,
        mtime: Option<SystemTime>,
        permissions: Option<u16>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Returns information on the filesystem
    fn stats(&mut self) -> impl Future<Output = Result<FsStats>> + Send;

    /// Creates a new file, returning its inode
    fn create_file(
        &mut self,
        parent_inode: u64,
        perms: u16,
        unix_flags: u32,
        filename: &Path,
    ) -> impl Future<Output = Result<u64>> + Send;

    /// Creates a new directory, returning its inode
    fn create_dir(
        &mut self,
        parent_inode: u64,
        permissions: u16,
        dir_name: &Path,
    ) -> impl Future<Output = Result<u64>> + Send;

    /// Reads data from the given inode, starting at the given offset for the
    /// given length
    fn read(
        &mut self,
        inode: u64,
        offset: u64,
        len: u64,
    ) -> impl Future<Output = Result<Vec<u8>>> + Send;

    /// Writes the given data to the given inode, starting at the given offset
    fn write(
        &mut self,
        inode: u64,
        offset: u64,
        data: &[u8],
    ) -> impl Future<Output = Result<()>> + Send;

    /// Reads the contents of the given directory
    fn read_dir(
        &mut self,
        inode: u64,
    ) -> impl Future<Output = Result<Vec<DirEntry>>> + Send;

    /// Removes the given inode. If invoked on a directory, the directory and
    /// all its contents are also deleted.
    fn remove(&mut self, inode: u64)
    -> impl Future<Output = Result<()>> + Send;

    /// Moves/renames the given inode. If the destination exists and is a file,
    /// it is overwritten. If it's a directory, this errors. If the source and
    /// dest are different types (e.g. file vs dir), it also errors.
    fn rename(
        &mut self,
        inode: u64,
        dest_parent_inode: u64,
        dest_filename: &Path,
    ) -> impl Future<Output = Result<()>> + Send;
}
