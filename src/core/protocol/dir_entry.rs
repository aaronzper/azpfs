use std::io::Cursor;

use binrw::{BinReaderExt, BinResult};

pub use crate::fs::{DirEntry, FileType};

/// Parse a reassembled READDIR payload into directory entries.
///
/// The `data` slice must be the fully reassembled payload from all READ_RES
/// chunks for a given READDIR_REQ (chunks reassembled in chunk_offset order).
pub fn parse_dir_entries(data: &[u8]) -> BinResult<Vec<DirEntry>> {
    let mut cursor = Cursor::new(data);
    let mut entries = Vec::new();
    while (cursor.position() as usize) < data.len() {
        entries.push(cursor.read_be::<DirEntry>()?);
    }
    Ok(entries)
}
