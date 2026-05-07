use binrw::binrw;
use std::io::{self, ErrorKind};

use crate::protocol::Message;

#[binrw]
#[brw(big, repr = u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Internal = 0,
    Invalid = 1,
    NotFound = 2,
    Exists = 3,
}

impl From<ErrorCode> for ErrorKind {
    fn from(value: ErrorCode) -> Self {
        match value {
            ErrorCode::Internal => Self::Other,
            ErrorCode::Invalid => Self::InvalidInput,
            ErrorCode::NotFound => Self::NotFound,
            ErrorCode::Exists => Self::AlreadyExists,
        }
    }
}

impl From<ErrorKind> for ErrorCode {
    fn from(value: ErrorKind) -> Self {
        match value {
            ErrorKind::InvalidInput => Self::Invalid,
            ErrorKind::NotFound => Self::NotFound,
            ErrorKind::AlreadyExists => Self::Exists,

            _ => Self::Internal,
        }
    }
}

impl Message {
    pub fn from_error(request_id: u32, value: io::Error) -> Self {
        Self::Error {
            request_id,
            error_code: value.kind().into(),
            message: value.to_string(),
        }
    }
}

impl TryFrom<Message> for io::Error {
    type Error = ();

    fn try_from(value: Message) -> Result<Self, Self::Error> {
        match value {
            Message::Error {
                error_code,
                message,
                ..
            } => Ok(io::Error::new(error_code.into(), message)),

            _ => Err(()),
        }
    }
}
