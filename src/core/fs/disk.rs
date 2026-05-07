use crate::ROOT_INODE;
use super::{DirEntry, FileAttr, FsBackend, FsStats};
use std::{
    collections::HashMap,
    io::{self, ErrorKind, Result},
    os::unix::fs::MetadataExt,
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
}

impl FsBackend for DiskFs {
    #[instrument]
    async fn lookup(&mut self, dir_inode: u64, filename: &Path) -> Result<u64> {
        debug!("Made it!");
        let path = self
            .inode_map
            .get(&dir_inode)
            .ok_or::<io::Error>(ErrorKind::NotFound.into())?;
        let mut path = path.clone();
        path.push(filename);

        let metadata = fs::metadata(&path).await?;
        let ino = metadata.ino();

        self.inode_map.entry(ino).or_insert(path);

        Ok(ino)
    }

    async fn get_attr(&mut self, inode: u64) -> Result<FileAttr> {
        todo!()
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
        todo!()
    }

    async fn stats(&mut self) -> Result<FsStats> {
        todo!()
    }

    async fn create_file(
        &mut self,
        parent_inode: u64,
        perms: u16,
        unix_flags: u32,
        filename: &Path,
    ) -> Result<u64> {
        todo!()
    }

    async fn create_dir(
        &mut self,
        parent_inode: u64,
        permissions: u16,
        dir_name: &Path,
    ) -> Result<u64> {
        todo!()
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
        todo!()
    }

    async fn remove(&mut self, inode: u64) -> Result<()> {
        todo!()
    }

    async fn rename(
        &mut self,
        inode: u64,
        dest_parent_inode: u64,
        dest_filename: &Path,
    ) -> Result<()> {
        todo!()
    }
}
