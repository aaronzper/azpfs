use crate::{client::ClientHandler, server::handle_client};
use std::time::Duration;
use tokio::time::timeout;

/// Wire a `ClientHandler` to a `handle_client` task via in-memory duplex
/// channels. Returns the initialized `ClientHandler` (INIT handshake already
/// done) or panics on timeout/error.
///
/// Usage in future tests:
/// ```ignore
/// let handler = setup().await;
/// // call handler methods, assert responses
/// ```
async fn setup() -> ClientHandler<impl crate::AzpfsWriter> {
    // Two duplex pairs, crossed so client reads what server writes and
    // vice-versa
    let (client_reader, server_writer) = tokio::io::duplex(4096);
    let (server_reader, client_writer) = tokio::io::duplex(4096);

    tokio::spawn(handle_client(server_reader, server_writer));

    timeout(
        Duration::from_secs(5),
        ClientHandler::new(client_reader, client_writer),
    )
    .await
    .expect("setup timed out")
    .expect("ClientHandler::new failed")
}

#[tokio::test]
async fn test_init_handshake() {
    // ClientHandler::new performs the INIT handshake internally.
    // Successful construction is the assertion.
    let _handler = setup().await;
}
