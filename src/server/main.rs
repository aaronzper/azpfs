use libazpfs::server::handle_client;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("localhost:19310").await?;

    loop {
        let (mut sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (r, w) = sock.split();
            handle_client(r, w).await;
        });
    }
}
