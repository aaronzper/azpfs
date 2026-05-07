use crate::ROOT_INODE;
use super::FsBackend;
use std::{
    collections::HashMap,
    io::{self, ErrorKind, Result},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
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
}
