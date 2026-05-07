use super::{DirEntry, FileAttr, FsBackend, FsStats};
use crate::{
    ROOT_INODE,
    fs::{FileType, from_unix},
};
use std::{
    collections::HashMap,
    io::{self, ErrorKind, Result},
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::fs;
use tracing::*;

#[derive(Debug)]
/// A filesystem backed by a directory on disk
pub struct DiskFs {
    /// Maps inode numbers to paths. Used to resolve inode-addressed requests
    inode_map: HashMap<u64, PathBuf>,
}

impl DiskFs {
    pub fn new(root: PathBuf) -> Self {
        let mut inode_map = HashMap::new();
        inode_map.insert(ROOT_INODE, root.clone());
        Self { inode_map }
    }

    fn get_path(&self, inode: u64) -> Result<&Path> {
        let path = self.inode_map.get(&inode).ok_or(io::Error::new(
            ErrorKind::NotFound,
            String::from("Inode not found. Have you LOOKUPed it yet?"),
        ))?;
        Ok(path)
    }
}

impl FsBackend for DiskFs {
    async fn lookup(&mut self, dir_inode: u64, filename: &Path) -> Result<u64> {
        let mut path = self.get_path(dir_inode)?.to_path_buf();
        path.push(filename);

        let metadata = fs::metadata(&path).await?;
        let ino = metadata.ino();

        self.inode_map.entry(ino).or_insert(path);

        Ok(ino)
    }

    async fn get_attr(&mut self, inode: u64) -> Result<FileAttr> {
        let path = self.get_path(inode)?;
        let meta = fs::metadata(path).await?;

        let attrs = FileAttr {
            file_type: meta.file_type().into(),
            size: meta.len(),
            blocks: meta.blocks(),
            access_time: meta.accessed()?,
            modification_time: meta.modified()?,
            change_time: from_unix(meta.ctime() as u64),
            permissions: meta.mode() as u16,
            n_hard_links: meta.nlink() as u32,
            uid: meta.uid(),
            gid: meta.gid(),
            rdev: meta.rdev() as u32,
            block_size: meta.blksize() as u32,
        };

        Ok(attrs)
    }

    async fn set_attr(
        &mut self,
        inode: u64,
        size: Option<u64>,
        atime: Option<SystemTime>,
        mtime: Option<SystemTime>,
        permissions: Option<u16>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> Result<()> {
        let path = self.get_path(inode)?.to_path_buf();

        if let Some(size) = size {
            let file = fs::OpenOptions::new().write(true).open(&path).await?;
            file.set_len(size).await?;
        }

        if atime.is_some() || mtime.is_some() {
            let file = fs::OpenOptions::new().write(true).open(&path).await?;
            let std_file = file.into_std().await;
            let mut times = std::fs::FileTimes::new();
            if let Some(t) = atime {
                times = times.set_accessed(t);
            }
            if let Some(t) = mtime {
                times = times.set_modified(t);
            }
            std_file.set_times(times)?;
        }

        if let Some(perms) = permissions {
            let perms = std::fs::Permissions::from_mode(perms as u32);
            fs::set_permissions(&path, perms).await?;
        }

        if uid.is_some() || gid.is_some() {
            std::os::unix::fs::chown(&path, uid, gid)?;
        }

        Ok(())
    }

    async fn stats(&mut self) -> Result<FsStats> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "Server does not support stats",
        ))
    }

    async fn create_file(
        &mut self,
        parent_inode: u64,
        perms: u16,
        unix_flags: u32,
        filename: &Path,
    ) -> Result<u64> {
        let mut path = self.get_path(parent_inode)?.to_path_buf();
        path.push(filename);

        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true);
        if unix_flags & libc::O_EXCL as u32 != 0 {
            opts.create_new(true);
        }
        if unix_flags & libc::O_TRUNC as u32 != 0 {
            opts.truncate(true);
        }
        let file = opts.open(&path).await?;

        fs::set_permissions(
            &path,
            std::fs::Permissions::from_mode(perms as u32),
        )
        .await?;

        let ino = file.metadata().await?.ino();
        self.inode_map.entry(ino).or_insert(path);
        Ok(ino)
    }

    async fn create_dir(
        &mut self,
        parent_inode: u64,
        permissions: u16,
        dir_name: &Path,
    ) -> Result<u64> {
        let mut path = self.get_path(parent_inode)?.to_path_buf();
        path.push(dir_name);

        fs::create_dir(&path).await?;
        fs::set_permissions(
            &path,
            std::fs::Permissions::from_mode(permissions as u32),
        )
        .await?;

        let ino = fs::metadata(&path).await?.ino();
        self.inode_map.entry(ino).or_insert(path);
        Ok(ino)
    }

    async fn read(
        &mut self,
        inode: u64,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>> {
        todo!()
    }

    async fn write(
        &mut self,
        inode: u64,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        todo!()
    }

    async fn read_dir(&mut self, inode: u64) -> Result<Vec<DirEntry>> {
        let path = self.get_path(inode)?;
        let mut dir = fs::read_dir(path).await?;
        let mut entries = Vec::new();
        while let Some(entry) = dir.next_entry().await? {
            entries.push(DirEntry {
                inode: entry.ino(),
                file_type: entry.file_type().await?.into(),
                filename: entry.file_name().as_bytes().to_vec(),
            });
        }

        Ok(entries)
    }

    async fn remove(&mut self, inode: u64) -> Result<()> {
        let path = self.get_path(inode)?;

        if fs::metadata(path).await?.is_dir() {
            fs::remove_dir_all(path).await?;
        } else {
            fs::remove_file(path).await?;
        }

        self.inode_map.remove(&inode);
        Ok(())
    }

    async fn rename(
        &mut self,
        inode: u64,
        dest_parent_inode: u64,
        dest_filename: &Path,
    ) -> Result<()> {
        if let Ok(dest_inode) =
            self.lookup(dest_parent_inode, dest_filename).await
        {
            let dest_type = self.get_attr(dest_inode).await?.file_type;
            let self_type = self.get_attr(inode).await?.file_type;
            if let FileType::Directory = dest_type {
                return Err(io::Error::new(
                    ErrorKind::AlreadyExists,
                    "Destination is a dir",
                ));
            } else if let FileType::RegularFile = dest_type
                && dest_type != self_type
            {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "Destination and source file types must match",
                ));
            }

            self.remove(dest_inode).await?;
        }

        let old_path = self.get_path(inode)?;
        let mut new_path = self.get_path(dest_parent_inode)?.to_path_buf();
        new_path.push(dest_filename);
        fs::rename(old_path, &new_path).await?;

        self.inode_map.insert(inode, new_path);

        Ok(())
    }
}
