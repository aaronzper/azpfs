use clap::Parser;
use libazpfs::fs::DiskFs;
use libazpfs::server::handle_client;
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tokio::{net::TcpListener, sync::Mutex};
use tracing::*;

#[derive(Parser)]
struct Args {
    /// The TCP socket address & port to bind on
    #[arg(required = true)]
    bind_address: SocketAddr,

    /// Path of the root directory to be exposed by the server
    #[arg(required = true)]
    root_path: PathBuf,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(
                        if cfg!(debug_assertions) {
                            "azpfs_server=debug,libazpfs=debug,warn"
                        } else {
                            "warn"
                        },
                    )
                }),
        )
        .pretty()
        .init();

    let args = Args::parse();

    let listener = TcpListener::bind(args.bind_address).await?;
    info!(binding = ?listener.local_addr(), "Server started!");

    let fs = Arc::new(Mutex::new(DiskFs::new(args.root_path)));

    loop {
        let fs = Arc::clone(&fs);
        let (sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (r, w) = sock.into_split();
            handle_client(r, w, fs).await;
        });
    }
}
