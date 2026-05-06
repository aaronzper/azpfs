use crate::{
    AzpfsReader, AzpfsWriter,
    protocol::{Message, MessageCodec},
};
use binrw::Error;
use futures::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::*;

#[instrument(skip_all)]
async fn receive_loop<R: AzpfsReader>(
    mut reader: FramedRead<R, MessageCodec>,
    listeners: Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<Message>>>>,
) {
    while let Some(msg) = reader.next().await {
        let Ok(msg) = msg else {
            warn!(?msg, "Failed to receive message");
            continue;
        };

        debug!(?msg, "Received");

        let id = msg.request_id();
        let mut listeners = listeners.lock().unwrap();
        if let Some(tx) = listeners.get(&id) {
            if let Err(e) = tx.send(msg) {
                // other end is closed -- requester is done listening
                listeners.remove(&id);
                warn!(msg=?e.0, "Dropping server message with stale listener");
            }
        } else {
            warn!(?msg, "Dropping server message with no listener");
        }
    }
}

#[derive(Debug)]
/// Client-side AZPFS protocol handler
pub struct ClientHandler<W: AzpfsWriter> {
    writer: FramedWrite<W, MessageCodec>,
    next_id: u32,
    reply_listeners: Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<Message>>>>,
}

impl<W: AzpfsWriter> ClientHandler<W> {
    /// Registers a reply listener for the given request ID. Allows
    /// request-generating methods to block waiting for replies to their request
    fn register_listener(
        &mut self,
        req_id: u32,
    ) -> mpsc::UnboundedReceiver<Message> {
        let mut listeners = self.reply_listeners.lock().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        listeners.insert(req_id, tx);

        rx
    }

    /// Sends the given message over the wire. Note that this method disregards
    /// the given message ID and instead allocates one via `self.next_id`. As
    /// such, the caller should *not* allocate message IDs, and should put a
    /// dummy value (e.g., `0`), in that field.
    ///
    /// Returns a channel receiver used to listen for replies to this msg ID
    async fn send_msg(
        &mut self,
        mut msg: Message,
    ) -> Result<mpsc::UnboundedReceiver<Message>, Error> {
        let id = self.next_id;
        msg.set_request_id(id);
        let listener = self.register_listener(id);
        self.next_id = self.next_id.wrapping_add(1); // TODO check for collision

        debug!(?msg, "Sending");
        self.writer.send(msg).await?;

        Ok(listener)
    }

    /// Conducts the INIT handshake
    async fn init(&mut self) -> Result<(), Error> {
        let mut init_listener = self
            .send_msg(Message::InitReq {
                request_id: 0,
                version: 0,
            })
            .await?;

        if let Some(Message::InitRes {
            request_id: _,
            accepted,
        }) = init_listener.recv().await
            && accepted
        {
            Ok(())
        } else {
            todo!()
        }
    }

    pub async fn new<R: AzpfsReader>(r: R, w: W) -> Result<Self, Error> {
        let reader = FramedRead::new(r, MessageCodec);
        let writer = FramedWrite::new(w, MessageCodec);

        let mut handler = Self {
            writer,
            next_id: 0,
            reply_listeners: Arc::new(Mutex::new(HashMap::new())),
        };

        let reply_listeners = Arc::clone(&handler.reply_listeners);
        tokio::spawn(
            async move { receive_loop(reader, reply_listeners).await },
        );

        handler.init().await?;
        Ok(handler)
    }
}
