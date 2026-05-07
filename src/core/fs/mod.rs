mod disk;
mod handler;
pub use disk::DiskFs;
pub use handler::ClientHandler;
use std::{fmt::Debug, future::Future, io::Result, path::Path};

/// A generic trait for any filesystem backend
pub trait FsBackend: Debug + Send + Sync + 'static {
    /// Looks up a file by name and directory inode, and returns that files
    /// inode
    fn lookup(
        &mut self,
        dir_inode: u64,
        filename: &Path,
    ) -> impl Future<Output = Result<u64>> + Send;
}
