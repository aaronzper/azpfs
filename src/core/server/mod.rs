use crate::{
    protocol::{Message, MessageCodec},
    server::handlers::handle_msg,
};
use futures::{SinkExt, StreamExt};
use std::fmt::Debug;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::*;

mod handlers;

#[instrument]
/// Handles receiving and sending messages for a single client
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

                handle_msg(msg, tx).await;
            }
            .instrument(Span::current()),
        );
    }
}
