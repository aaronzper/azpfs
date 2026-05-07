use crate::{
    client::ClientHandler,
    fs::DiskFs,
    server::handle_client,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tempfile::TempDir;
use tokio::time::timeout;

/// Wire a `ClientHandler` to a `handle_client` task via in-memory duplex
/// channels. Returns the initialized `ClientHandler` (INIT handshake already
/// done) or panics on timeout/error.
///
/// Also returns the `TempDir` so the caller keeps it alive for the test's
/// duration (dropping it would delete the backing directory).
async fn setup() -> (ClientHandler<impl crate::AzpfsWriter>, TempDir) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let fs = Arc::new(Mutex::new(DiskFs::new(dir.path().to_path_buf())));

    // Two duplex pairs, crossed so client reads what server writes and
    // vice-versa
    let (client_reader, server_writer) = tokio::io::duplex(4096);
    let (server_reader, client_writer) = tokio::io::duplex(4096);

    tokio::spawn(handle_client(server_reader, server_writer, fs));

    let handler = timeout(
        Duration::from_secs(5),
        ClientHandler::new(client_reader, client_writer),
    )
    .await
    .expect("setup timed out")
    .expect("ClientHandler::new failed");
    (handler, dir)
}

#[tokio::test]
async fn test_init_handshake() {
    // ClientHandler::new performs the INIT handshake internally.
    // Successful construction is the assertion.
    let (_handler, _dir) = setup().await;
}
