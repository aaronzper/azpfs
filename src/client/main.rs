use clap::Parser;
use fuser::{Config, MountOption, SessionACL, mount2};
use libazpfs::client::{ClientHandler, FUSEFilesytem};
use std::{net::SocketAddr, path::PathBuf};
use tokio::net::TcpStream;
use tracing::*;

#[derive(Parser)]
struct Args {
    /// Mount path on the local filesystem
    #[arg(required = true)]
    mountpoint: PathBuf,

    /// The TCP socket address of the remote AZPFS server to connect to
    #[arg(required = true)]
    address: SocketAddr,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(
                        if cfg!(debug_assertions) {
                            "azpfsd=debug,libazpfs=debug,warn"
                        } else {
                            "warn"
                        },
                    )
                }),
        )
        .pretty()
        .init();

    let args = Args::parse();

    let stream = TcpStream::connect(args.address).await?;
    let (r, w) = stream.into_split();
    let handler = match ClientHandler::new(r, w).await {
        Ok(h) => h,
        Err(e) => {
            error!(error=?e, "Failed to connect to server,");
            return Err(std::io::ErrorKind::NotConnected.into());
        }
    };
    let fs = FUSEFilesytem::new(handler);

    info!(mountpoint = args.mountpoint.to_str(), "azpfsd starting");

    let mut config = Config::default();
    config.mount_options.push(MountOption::AutoUnmount);
    config.acl = SessionACL::All;
    mount2(fs, args.mountpoint, &config)
}
