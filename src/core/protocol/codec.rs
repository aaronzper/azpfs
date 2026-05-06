use bytes::{Buf, BytesMut};
use std::io::Cursor;
use tokio_util::codec::{Decoder, Encoder};

use super::Message;

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

fn is_eof(e: &binrw::Error) -> bool {
    match e {
        binrw::Error::Io(io) => io.kind() == std::io::ErrorKind::UnexpectedEof,
        // If we hit EOF, instead of erring with the above, each enum variant
        // will individuall err with it. Thus, this checks for that to correctly
        // detect EOF.
        binrw::Error::EnumErrors { variant_errors, .. } => {
            variant_errors.iter().all(|(_, e)| is_eof(e))
        }
        _ => false,
    }
}
