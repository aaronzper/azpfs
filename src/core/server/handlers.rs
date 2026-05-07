use std::{
    ffi::OsString,
    io::ErrorKind,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use crate::{
    fs::{FileAttr, FsBackend, from_unix},
    protocol::{ErrorCode, Message},
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
