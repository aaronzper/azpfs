use futures::{SinkExt, StreamExt};
use std::fmt::Debug;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::*;

use crate::protocol::{Message, MessageCodec};

#[instrument]
pub async fn handle_client<R, W>(r: R, w: W)
where
    R: AsyncRead + Unpin + Debug,
    W: AsyncWrite + Unpin + Debug + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel(32);
    let mut writer = FramedWrite::new(w, MessageCodec);
    tokio::spawn(
        async move {
            while let Some(msg) = rx.recv().await {
                debug!(?msg, "Sending");
                if let Err(e) = writer.send(msg).await {
                    warn!(error=?e, "Reply error");
                }
            }
        }
        .instrument(Span::current()),
    );

    let mut reader = FramedRead::new(r, MessageCodec);
    while let Some(msg) = reader.next().await {
        let tx = tx.clone();
        tokio::spawn(
            async move {
                let Ok(msg) = msg else {
                    warn!(error=?msg, "Couldn't decode message");
                    return;
                };
                debug!(?msg, "Received");

                // test reply for now
                let reply = Message::Error {
                    request_id: 1,
                    error_code: crate::protocol::ErrorCode::Invalid,
                    message: "Not implemented".to_string(),
                };

                if let Err(e) = tx.send(reply).await {
                    warn!(error=?e, "Reply error");
                }
            }
            .instrument(Span::current()),
        );
    }

    println!("bye...");
}
