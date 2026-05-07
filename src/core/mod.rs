use std::fmt::Debug;
use tokio::io::{AsyncRead, AsyncWrite};

pub mod client;
pub mod fs;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod tests;

/// Standard inode number for FUSE root
const ROOT_INODE: u64 = 1;

pub trait AzpfsReader:
    AsyncRead + Unpin + Debug + Send + Sync + 'static
{
}
impl<T: AsyncRead + Unpin + Debug + Send + Sync + 'static> AzpfsReader for T {}

pub trait AzpfsWriter:
    AsyncWrite + Unpin + Debug + Send + Sync + 'static
{
}
impl<T: AsyncWrite + Unpin + Debug + Send + Sync + 'static> AzpfsWriter for T {}
