use super::{DirEntry, FileAttr, FsBackend, FsStats};
use crate::{
    AzpfsReader, AzpfsWriter,
    fs::to_unix,
    protocol::{
        ErrorCode, Message, MessageCodec, dir_entry::parse_dir_entries,
    },
};
use futures::{SinkExt, StreamExt};
use std::{
    collections::{BTreeMap, HashMap},
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::Path,
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

/// Blocks until a success message on the given listener is received
async fn await_success(
    listener: &mut mpsc::UnboundedReceiver<Message>,
) -> io::Result<()> {
    while let Some(msg) = listener.recv().await {
        match msg {
            Message::SuccessRes { .. } => {
                return Ok(());
            }
            Message::Error { .. } => {
                return Err(msg.try_into().unwrap());
            }
            _ => continue,
        }
    }

    Err(ErrorKind::ConnectionReset.into())
}

/// Blocks until a non-error message on the given listener is received
async fn next_message(
    listener: &mut mpsc::UnboundedReceiver<Message>,
) -> io::Result<Message> {
    if let Some(msg) = listener.recv().await {
        match msg {
            Message::Error { .. } => {
                return Err(msg.try_into().unwrap());
            }
            _ => return Ok(msg),
        }
    }

    Err(ErrorKind::ConnectionReset.into())
}

/// Blocks to receive chunked read response data, reassembles it, and returns
/// the underlying data
async fn combine_read_chunks(
    listener: &mut mpsc::UnboundedReceiver<Message>,
) -> io::Result<Vec<u8>> {
    let mut chunks = BTreeMap::new();
    let mut received = 0;

    while let Some(msg) = listener.recv().await {
        match msg {
            Message::ReadRes {
                total_length,
                chunk_offset,
                data,
                ..
            } => {
                received += data.len();
                chunks.insert(chunk_offset, data);

                if received == total_length as usize {
                    return Ok(chunks
                        .into_iter()
                        .map(|(_, v)| v)
                        .flatten()
                        .collect());
                }
            }
            Message::Error { .. } => {
                return Err(msg.try_into().unwrap());
            }
            _ => continue,
        }
    }

    Err(ErrorKind::ConnectionReset.into())
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
    ) -> io::Result<mpsc::UnboundedReceiver<Message>> {
        let id = self.next_id;
        msg.set_request_id(id);
        let listener = self.register_listener(id);
        self.next_id = self.next_id.wrapping_add(1); // TODO check for collision

        debug!(?msg, "Sending");
        match self.writer.send(msg).await {
            Ok(_) => Ok(listener),
            Err(e) => Err(io::Error::new(ErrorKind::Other, e)),
        }
    }

    pub async fn new<R: AzpfsReader>(r: R, w: W) -> io::Result<Self> {
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

    /// Conducts the INIT handshake
    async fn init(&mut self) -> io::Result<()> {
        let mut init_listener = self
            .send_msg(Message::InitReq {
                request_id: 0,
                version: 0,
            })
            .await?;

        match init_listener.recv().await {
            Some(Message::InitRes {
                request_id: _,
                accepted: true,
            }) => Ok(()),
            Some(Message::InitRes { .. }) => Err(io::Error::new(
                ErrorKind::ConnectionRefused,
                "server rejected the INIT handshake",
            )),
            Some(Message::Error {
                error_code,
                message,
                ..
            }) => Err(io::Error::new(
                ErrorKind::ConnectionRefused,
                format!(
                    "server returned an error during INIT: {error_code:?}: {message}"
                ),
            )),
            // The server sent a message that is not a valid INIT reply.
            Some(other) => Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("unexpected reply to INIT_REQ: {other:?}"),
            )),
            // The receive loop closed the channel before any reply arrived.
            None => Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "connection closed during the INIT handshake",
            )),
        }
    }
}

impl<W: AzpfsWriter> FsBackend for ClientHandler<W> {
    async fn lookup(
        &mut self,
        dir_inode: u64,
        filename: &Path,
    ) -> io::Result<u64> {
        let mut listener = self
            .send_msg(Message::LookupReq {
                request_id: 0,
                dir_inode,
                filename: filename.as_os_str().as_bytes().to_vec(),
            })
            .await
            .map_err(|e| io::Error::other(e))?;

        loop {
            match next_message(&mut listener).await? {
                Message::LookupRes {
                    request_id: _,
                    inode,
                } => return Ok(inode),
                _ => continue,
            }
        }
    }

    async fn get_attr(&mut self, inode: u64) -> io::Result<FileAttr> {
        let mut listener = self
            .send_msg(Message::GetAttrReq {
                request_id: 0,
                inode,
            })
            .await?;

        loop {
            match next_message(&mut listener).await? {
                msg @ Message::FileAttrRes { .. } => {
                    return Ok(FileAttr::from_message(msg).unwrap());
                }
                _ => continue,
            }
        }
    }

    async fn set_attr(
        &mut self,
        inode: u64,
        size: Option<u64>,
        atime: Option<std::time::SystemTime>,
        mtime: Option<std::time::SystemTime>,
        permissions: Option<u16>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> io::Result<()> {
        let mut listener = self
            .send_msg(Message::SetAttrReq {
                request_id: 0,
                inode,
                size,
                atime: atime.map(|x| to_unix(x)),
                mtime: mtime.map(|x| to_unix(x)),
                permissions,
                uid,
                gid,
            })
            .await?;

        await_success(&mut listener).await
    }

    async fn stats(&mut self) -> io::Result<FsStats> {
        let mut listener =
            self.send_msg(Message::StatsReq { request_id: 0 }).await?;

        loop {
            match next_message(&mut listener).await? {
                msg @ Message::StatsRes { .. } => {
                    return Ok(FsStats::from_message(msg).unwrap());
                }
                _ => continue,
            }
        }
    }

    async fn create_file(
        &mut self,
        dir_inode: u64,
        permissions: u16,
        unix_flags: u32,
        filename: &Path,
    ) -> io::Result<u64> {
        let mut listener = self
            .send_msg(Message::CreateReq {
                request_id: 0,
                dir_inode,
                permissions,
                unix_flags,
                is_directory: false,
                filename: filename.as_os_str().as_bytes().to_vec(),
            })
            .await
            .map_err(|e| io::Error::other(e))?;

        loop {
            match next_message(&mut listener).await? {
                Message::LookupRes {
                    request_id: _,
                    inode,
                } => return Ok(inode),
                _ => continue,
            }
        }
    }

    async fn create_dir(
        &mut self,
        parent_inode: u64,
        permissions: u16,
        dir_name: &Path,
    ) -> io::Result<u64> {
        let mut listener = self
            .send_msg(Message::CreateReq {
                request_id: 0,
                dir_inode: parent_inode,
                permissions,
                unix_flags: 0,
                is_directory: true,
                filename: dir_name.as_os_str().as_bytes().to_vec(),
            })
            .await
            .map_err(|e| io::Error::other(e))?;

        loop {
            match next_message(&mut listener).await? {
                Message::LookupRes {
                    request_id: _,
                    inode,
                } => return Ok(inode),
                _ => continue,
            }
        }
    }

    async fn read(
        &mut self,
        inode: u64,
        offset: u64,
        length: u64,
    ) -> io::Result<Vec<u8>> {
        let mut listener = self
            .send_msg(Message::ReadReq {
                request_id: 0,
                inode,
                offset,
                length,
            })
            .await
            .map_err(|e| io::Error::other(e))?;

        let data = combine_read_chunks(&mut listener).await?;
        Ok(data)
    }

    async fn write(
        &mut self,
        inode: u64,
        offset: u64,
        data: &[u8],
    ) -> io::Result<()> {
        let mut listener = self
            .send_msg(Message::WriteReq {
                request_id: 0,
                inode,
                offset,
                data: data.to_vec(),
            })
            .await?;

        await_success(&mut listener).await
    }

    async fn read_dir(&mut self, inode: u64) -> io::Result<Vec<DirEntry>> {
        let mut listener = self
            .send_msg(Message::ReaddirReq {
                request_id: 0,
                inode,
            })
            .await
            .map_err(|e| io::Error::other(e))?;

        let data = combine_read_chunks(&mut listener).await?;
        match parse_dir_entries(&data) {
            Ok(e) => Ok(e),
            Err(e) => {
                return Err(io::Error::other(e));
            }
        }
    }

    async fn remove(&mut self, inode: u64) -> io::Result<()> {
        let mut listener = self
            .send_msg(Message::RmReq {
                request_id: 0,
                inode,
            })
            .await?;

        await_success(&mut listener).await
    }

    async fn rename(
        &mut self,
        inode: u64,
        dest_dir_inode: u64,
        dest_filename: &Path,
    ) -> io::Result<()> {
        let mut listener = self
            .send_msg(Message::MoveReq {
                request_id: 0,
                inode,
                dest_dir_inode,
                dest_filename: dest_filename.as_os_str().as_bytes().to_vec(),
            })
            .await?;

        await_success(&mut listener).await
    }
}
