use libazpfs::server::handle_client;
use tokio::net::TcpListener;
use tracing::*;

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

    let listener = TcpListener::bind("0.0.0.0:19310").await?;
    info!(binding = ?listener.local_addr(), "Server started!");

    loop {
        let (sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (r, w) = sock.into_split();
            handle_client(r, w).await;
        });
    }
}
