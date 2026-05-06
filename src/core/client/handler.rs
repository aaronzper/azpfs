use crate::{
    AzpfsReader, AzpfsWriter,
    protocol::{Message, MessageCodec},
};
use binrw::Error;
use futures::{SinkExt, StreamExt};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::*;

#[derive(Debug)]
/// Client-side AZPFS protocol handler
pub struct ClientHandler<R: AzpfsReader, W: AzpfsWriter> {
    reader: FramedRead<R, MessageCodec>,
    writer: FramedWrite<W, MessageCodec>,
    next_id: u32,
}

impl<R: AzpfsReader, W: AzpfsWriter> ClientHandler<R, W> {
    /// Sends the given message over the wire. Note that this method disregards
    /// the given message ID and instead allocates one via `self.next_id`. As
    /// such, the caller should *not* allocate message IDs, and should put a
    /// dummy value (e.g., `0`), in that field.
    ///
    /// Returns the ID of the message
    async fn send_msg(&mut self, mut msg: Message) -> Result<u32, Error> {
        let id = self.next_id;
        msg.set_request_id(id);
        self.next_id = self.next_id.wrapping_add(1); // TODO check for collision

        debug!(?msg, "Sending");
        self.writer.send(msg).await?;

        Ok(id)
    }

    pub async fn new(r: R, w: W) -> Result<Self, Error> {
        let reader = FramedRead::new(r, MessageCodec);
        let writer = FramedWrite::new(w, MessageCodec);

        let mut handler = Self {
            reader,
            writer,
            next_id: 0,
        };

        let init_req_id = handler
            .send_msg(Message::InitReq {
                request_id: 0,
                version: 0,
            })
            .await?;

        if let Some(Ok(Message::InitRes {
            request_id,
            accepted,
        })) = handler.reader.next().await
            && accepted
            && request_id == init_req_id
        {
            Ok(handler)
        } else {
            todo!()
        }
    }
}
