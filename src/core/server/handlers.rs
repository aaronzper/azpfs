use crate::{
    fs::{FileAttr, FsBackend, from_unix},
    protocol::{
        ErrorCode, MAX_READ_RES_CHUNK, Message,
        dir_entry::serialize_dir_entires,
    },
};
use std::{
    cmp::min,
    ffi::OsString,
    io::ErrorKind,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;
use tokio::{sync::Mutex, task::JoinSet};
use tracing::*;

fn assemble_read_res_chunks(request_id: u32, data: &[u8]) -> Vec<Message> {
    if data.is_empty() {
        return vec![Message::ReadRes {
            request_id,
            total_length: 0,
            chunk_offset: 0,
            data: vec![],
        }];
    }

    let mut chunks = Vec::new();

    let mut head = 0;
    while head < data.len() {
        let remaining = data.len() - head;
        let chunk_len = min(remaining, MAX_READ_RES_CHUNK);
        let chunk = &data[head..head + chunk_len];

        chunks.push(Message::ReadRes {
            request_id,
            total_length: data.len() as u64,
            chunk_offset: head as u64,
            data: chunk.to_vec(),
        });

        head += chunk_len;
    }

    chunks
}

#[instrument(skip(replier, fs))]
pub async fn handle_msg(
    msg: Message,
    replier: mpsc::Sender<Message>,
    fs: &Mutex<impl FsBackend>,
) {
    match msg {
        Message::InitReq {
            request_id,
            version,
        } => {
            // TODO check for duplicate inits, other pre-init messages
            let reply = Message::InitRes {
                request_id,
                accepted: version == 0x00,
            };

            replier.send(reply).await.unwrap();
        }

        Message::LookupReq {
            request_id,
            dir_inode,
            filename,
        } => {
            let filename_path = PathBuf::from(OsString::from_vec(filename));
            let mut fs = fs.lock().await;
            let reply = match fs.lookup(dir_inode, &filename_path).await {
                Ok(inode) => Message::LookupRes { request_id, inode },
                Err(e) => Message::from_error(request_id, e),
            };
            replier.send(reply).await.unwrap();
        }

        Message::GetAttrReq { request_id, inode } => {
            let mut fs = fs.lock().await;
            let reply = match fs.get_attr(inode).await {
                Ok(attr) => attr.into_message(request_id),
                Err(e) => Message::from_error(request_id, e),
            };
            replier.send(reply).await.unwrap();
        }

        Message::SetAttrReq {
            request_id,
            inode,
            size,
            atime,
            mtime,
            permissions,
            uid,
            gid,
        } => {
            let atime = atime.map(|x| from_unix(x));
            let mtime = mtime.map(|x| from_unix(x));

            let mut fs = fs.lock().await;
            let reply = match fs
                .set_attr(inode, size, atime, mtime, permissions, uid, gid)
                .await
            {
                Ok(()) => Message::SuccessRes { request_id },
                Err(e) => Message::from_error(request_id, e),
            };
            replier.send(reply).await.unwrap();
        }

        Message::StatsReq { request_id } => {
            let mut fs = fs.lock().await;
            let reply = match fs.stats().await {
                Ok(stats) => stats.into_message(request_id),
                Err(e) => Message::from_error(request_id, e),
            };
            replier.send(reply).await.unwrap();
        }

        Message::CreateReq {
            request_id,
            dir_inode,
            permissions,
            unix_flags,
            is_directory,
            filename,
        } => {
            let filename = PathBuf::from(OsString::from_vec(filename));
            let mut fs = fs.lock().await;

            let result = if is_directory {
                fs.create_dir(dir_inode, permissions, &filename).await
            } else {
                fs.create_file(dir_inode, permissions, unix_flags, &filename)
                    .await
            };

            let reply = match result {
                Ok(inode) => Message::LookupRes { request_id, inode },
                Err(e) => Message::from_error(request_id, e),
            };
            replier.send(reply).await.unwrap();
        }

        Message::ReadReq {
            request_id,
            inode,
            offset,
            length,
        } => {
            todo!()
        }

        Message::WriteReq {
            request_id,
            inode,
            offset,
            data,
        } => {
            todo!()
        }

        Message::ReaddirReq { request_id, inode } => {
            let mut fs = fs.lock().await;
            let entries = match fs.read_dir(inode).await {
                Ok(e) => e,
                Err(e) => {
                    let reply = Message::from_error(request_id, e);
                    replier.send(reply).await.unwrap();
                    return;
                }
            };

            let buf = match serialize_dir_entires(&entries) {
                Ok(b) => b,
                Err(e) => {
                    let reply = Message::Error {
                        request_id,
                        error_code: ErrorCode::Internal,
                        message: e.to_string(),
                    };
                    replier.send(reply).await.unwrap();
                    return;
                }
            };

            let replies = assemble_read_res_chunks(request_id, &buf);
            let mut reply_set = JoinSet::new();
            for reply in replies {
                let replier = replier.clone();
                reply_set.spawn(async move {
                    replier.send(reply).await.unwrap();
                });
            }

            reply_set.join_all().await;
        }

        Message::RmReq { request_id, inode } => {
            let mut fs = fs.lock().await;
            let reply = match fs.remove(inode).await {
                Ok(()) => Message::SuccessRes { request_id },
                Err(e) => Message::from_error(request_id, e),
            };
            replier.send(reply).await.unwrap();
        }

        Message::MoveReq {
            request_id,
            inode,
            dest_dir_inode,
            dest_filename,
        } => {
            todo!()
        }

        // Client sent a response message for some reason, ignore
        msg @ _ => {
            warn!(?msg, "Received spurious response message");
        }
    }
}
