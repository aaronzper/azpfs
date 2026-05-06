use std::io::Cursor;

use binrw::{BinReaderExt, BinResult, binrw};

#[binrw]
#[brw(big, repr = u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Pipe = 0,
    CharDevice = 1,
    BlockDevice = 2,
    Directory = 3,
    RegularFile = 4,
    Symlink = 5,
    Socket = 6,
}

impl TryFrom<u8> for FileType {
    type Error = binrw::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Pipe),
            1 => Ok(Self::CharDevice),
            2 => Ok(Self::BlockDevice),
            3 => Ok(Self::Directory),
            4 => Ok(Self::RegularFile),
            5 => Ok(Self::Symlink),
            6 => Ok(Self::Socket),
            _ => Err(binrw::Error::NoVariantMatch { pos: 0 }),
        }
    }
}

#[binrw]
#[brw(big)]
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub inode: u64,
    #[br(try_map = |b: u8| FileType::try_from(b))]
    #[bw(map = |ft: &FileType| *ft as u8)]
    pub file_type: FileType,
    #[br(temp)]
    #[bw(calc = filename.len() as u8)]
    filename_len: u8,
    #[br(count = filename_len)]
    pub filename: Vec<u8>,
}

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
