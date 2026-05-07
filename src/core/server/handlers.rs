use std::{
    ffi::OsString,
    io::ErrorKind,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use crate::{
    protocol::{ErrorCode, Message},
    fs::FsBackend,
};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::*;

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
                Err(e) => match e.kind() {
                    ErrorKind::NotFound => Message::Error {
                        request_id,
                        error_code: ErrorCode::NotFound,
                        message: e.to_string(),
                    },
                    _ => Message::Error {
                        request_id,
                        error_code: ErrorCode::Internal,
                        message: e.to_string(),
                    },
                },
            };
            replier.send(reply).await.unwrap();
        }

        Message::GetAttrReq { request_id, inode } => {
            todo!()
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
            todo!()
        }

        Message::StatsReq { request_id } => {
            todo!()
        }

        Message::CreateReq {
            request_id,
            dir_inode,
            permissions,
            unix_flags,
            is_directory,
            filename,
        } => {
            todo!()
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
            todo!()
        }

        Message::RmReq { request_id, inode } => {
            todo!()
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
        _ => (),
    }
}
