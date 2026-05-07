pub use crate::fs::{DirEntry, FileType};
use binrw::{BinReaderExt, BinResult, BinWrite};
use std::io::Cursor;

/// Parse a reassembled READDIR reply payload into directory entries.
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

/// Serialize a list of directory entries into a READDIR reply payload, to be
/// chunked and sent out
pub fn serialize_dir_entires(entries: &[DirEntry]) -> BinResult<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    for entry in entries {
        entry.write(&mut cursor)?;
    }
    Ok(cursor.into_inner())
}
