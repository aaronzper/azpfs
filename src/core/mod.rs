use std::fmt::Debug;
use tokio::io::{AsyncRead, AsyncWrite};

pub mod client;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod tests;

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
