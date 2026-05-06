use bytes::{Buf, BytesMut};
use std::io::Cursor;
use tokio_util::codec::{Decoder, Encoder};

use super::Message;

#[derive(Debug)]
pub struct MessageCodec;

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = binrw::Error;

    fn decode(
        &mut self,
        src: &mut BytesMut,
    ) -> Result<Option<Message>, binrw::Error> {
        let mut cursor = Cursor::new(&src[..]);
        match binrw::BinRead::read(&mut cursor) {
            Ok(msg) => {
                src.advance(cursor.position() as usize);
                Ok(Some(msg))
            }
            Err(e) if is_eof(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = binrw::Error;

    fn encode(
        &mut self,
        msg: Message,
        dst: &mut BytesMut,
    ) -> Result<(), binrw::Error> {
        let mut cursor = Cursor::new(Vec::new());
        binrw::BinWrite::write(&msg, &mut cursor)?;
        dst.extend_from_slice(&cursor.into_inner());
        Ok(())
    }
}

/// `binrw`'s built-in `is_eof` doesn't correctly unwrap backtrace errors into
/// underlying EOF errors, thus we use this
fn is_eof(e: &binrw::Error) -> bool {
    match e {
        binrw::Error::Io(io) => io.kind() == std::io::ErrorKind::UnexpectedEof,
        binrw::Error::Backtrace(bt) => is_eof(&bt.error),
        binrw::Error::EnumErrors { variant_errors, .. } => {
            variant_errors.iter().any(|(_, e)| is_eof(e))
        }
        _ => false,
    }
}
