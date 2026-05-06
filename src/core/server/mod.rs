use futures::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::protocol::MessageCodec;

pub async fn handle_client<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    r: R,
    w: W,
) {
    let mut reader = FramedRead::new(r, MessageCodec);
    let writer = FramedWrite::new(w, MessageCodec);

    while let Some(msg) = reader.next().await {
        println!("{:#?}", msg);
    }

    println!("bye...");
}
