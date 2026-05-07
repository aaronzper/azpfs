use binrw::binrw;

use crate::fs::FileType;
use super::error::ErrorCode;

#[binrw]
#[brw(big)]
#[derive(Debug, Clone)]
pub enum Message {
    #[brw(magic = 0x00u8)]
    Error {
        request_id: u32,
        error_code: ErrorCode,
        #[br(temp)]
        #[bw(calc = message.len() as u16)]
        message_len: u16,
        #[br(count = message_len, try_map = |v: Vec<u8>| String::from_utf8(v))]
        #[bw(map = |s: &String| s.as_bytes().to_vec())]
        message: String,
    },

    #[brw(magic = 0x01u8)]
    InitReq {
        request_id: u32,
        #[br(map = |b: u8| b >> 4)]
        #[bw(map = |v: &u8| *v << 4)]
        version: u8,
    },

    #[brw(magic = 0x02u8)]
    InitRes {
        request_id: u32,
        #[br(map = |b: u8| b >> 7 != 0)]
        #[bw(map = |a: &bool| (*a as u8) << 7)]
        accepted: bool,
    },

    #[brw(magic = 0x03u8)]
    LookupReq {
        request_id: u32,
        dir_inode: u64,
        #[br(temp)]
        #[bw(calc = filename.len() as u8)]
        filename_len: u8,
        #[br(count = filename_len)]
        filename: Vec<u8>,
    },

    #[brw(magic = 0x04u8)]
    LookupRes {
        request_id: u32,
        inode: u64,
    },

    #[brw(magic = 0x05u8)]
    GetAttrReq {
        request_id: u32,
        inode: u64,
    },

    /// File attributes response (returned by GET_ATTR_REQ).
    #[brw(magic = 0x06u8)]
    FileAttrRes {
        request_id: u32,
        #[br(try_map = |b: u8| FileType::try_from(b >> 5))]
        #[bw(map = |ft: &FileType| (*ft as u8) << 5)]
        file_type: FileType,
        size: u64,
        blocks: u64,
        atime: u64,
        mtime: u64,
        ctime: u64,
        permissions: u16,
        nlinks: u32,
        uid: u32,
        gid: u32,
        rdev: u32,
        blksize: u32,
    },

    /// Set file attributes (returns SUCCESS_RES).
    ///
    /// Wire format: `field_mask` byte followed only by the fields whose bits are set.
    /// Bit 0=size, 1=atime, 2=mtime, 3=permissions, 4=uid, 5=gid.
    /// `None` fields are omitted from the wire; `Some` fields are written in bit order.
    #[brw(magic = 0x07u8)]
    SetAttrReq {
        request_id: u32,
        inode: u64,
        #[br(temp, map = |b: u8| b >> 2)]
        #[bw(calc = {
            let mut m = 0u8;
            if size.is_some()        { m |= 1 << 0; }
            if atime.is_some()       { m |= 1 << 1; }
            if mtime.is_some()       { m |= 1 << 2; }
            if permissions.is_some() { m |= 1 << 3; }
            if uid.is_some()         { m |= 1 << 4; }
            if gid.is_some()         { m |= 1 << 5; }
            m << 2  // mask in top 6 bits; low 2 bits reserved = 0
        })]
        field_mask: u8,
        #[br(if(field_mask & (1 << 0) != 0))]
        size: Option<u64>,
        #[br(if(field_mask & (1 << 1) != 0))]
        atime: Option<u64>,
        #[br(if(field_mask & (1 << 2) != 0))]
        mtime: Option<u64>,
        #[br(if(field_mask & (1 << 3) != 0))]
        permissions: Option<u16>,
        #[br(if(field_mask & (1 << 4) != 0))]
        uid: Option<u32>,
        #[br(if(field_mask & (1 << 5) != 0))]
        gid: Option<u32>,
    },

    /// Generic success (returned by SET_ATTR_REQ, WRITE_REQ, RM_REQ, MOVE_REQ).
    #[brw(magic = 0x08u8)]
    SuccessRes {
        request_id: u32,
    },

    #[brw(magic = 0x09u8)]
    StatsReq {
        request_id: u32,
    },

    #[brw(magic = 0x0Au8)]
    StatsRes {
        request_id: u32,
        blksize: u32,
        blocks: u64,
        free_blocks: u64,
        avail_blocks: u64,
        total_inodes: u64,
        free_inodes: u64,
        max_filename_len: u32,
        fragment_size: u32,
    },

    /// Create a file or directory (returns LOOKUP_RES).
    /// When `is_directory` is true, `unix_flags` is ignored by the server.
    #[brw(magic = 0x0Bu8)]
    CreateReq {
        request_id: u32,
        dir_inode: u64,
        permissions: u16,
        unix_flags: u32,
        #[br(map = |b: u8| b >> 7 != 0)]
        #[bw(map = |d: &bool| (*d as u8) << 7)]
        is_directory: bool,
        #[br(temp)]
        #[bw(calc = filename.len() as u8)]
        filename_len: u8,
        #[br(count = filename_len)]
        filename: Vec<u8>,
    },

    #[brw(magic = 0x0Cu8)]
    ReadReq {
        request_id: u32,
        inode: u64,
        offset: u64,
        length: u64,
    },

    /// One chunk of a read response (returned by READ_REQ and READDIR_REQ).
    ///
    /// `total_length` is the total byte count across all chunks for this request,
    /// used by the client to pre-allocate a reassembly buffer.
    /// `eof` is true if this chunk contains the last byte of the file (ignored for READDIR).
    /// Callers must reassemble all chunks by `chunk_offset` before parsing directory entries.
    #[brw(magic = 0x0Du8)]
    ReadRes {
        request_id: u32,
        total_length: u64,
        // Wire: bit 15 = eof, bits 14..0 = chunk_length (derived from data.len() on write).
        #[br(temp)]
        #[bw(calc = (*eof as u16) << 15 | data.len() as u16)]
        eof_and_chunk_len: u16,
        #[br(calc = eof_and_chunk_len >> 15 != 0)]
        #[bw(ignore)]
        eof: bool,
        chunk_offset: u64,
        #[br(count = (eof_and_chunk_len & 0x7FFF) as usize)]
        data: Vec<u8>,
    },

    #[brw(magic = 0x0Eu8)]
    WriteReq {
        request_id: u32,
        inode: u64,
        offset: u64,
        #[br(temp)]
        #[bw(calc = data.len() as u32)]
        length: u32,
        #[br(count = length)]
        data: Vec<u8>,
    },

    #[brw(magic = 0x0Fu8)]
    ReaddirReq {
        request_id: u32,
        inode: u64,
    },

    #[brw(magic = 0x10u8)]
    RmReq {
        request_id: u32,
        inode: u64,
    },

    #[brw(magic = 0x11u8)]
    MoveReq {
        request_id: u32,
        inode: u64,
        dest_dir_inode: u64,
        #[br(temp)]
        #[bw(calc = dest_filename.len() as u8)]
        dest_filename_len: u8,
        #[br(count = dest_filename_len)]
        dest_filename: Vec<u8>,
    },
}

impl Message {
    pub fn request_id(&self) -> u32 {
        match self {
            Self::Error { request_id, .. }
            | Self::InitReq { request_id, .. }
            | Self::InitRes { request_id, .. }
            | Self::LookupReq { request_id, .. }
            | Self::LookupRes { request_id, .. }
            | Self::GetAttrReq { request_id, .. }
            | Self::FileAttrRes { request_id, .. }
            | Self::SetAttrReq { request_id, .. }
            | Self::SuccessRes { request_id, .. }
            | Self::StatsReq { request_id, .. }
            | Self::StatsRes { request_id, .. }
            | Self::CreateReq { request_id, .. }
            | Self::ReadReq { request_id, .. }
            | Self::ReadRes { request_id, .. }
            | Self::WriteReq { request_id, .. }
            | Self::ReaddirReq { request_id, .. }
            | Self::RmReq { request_id, .. }
            | Self::MoveReq { request_id, .. } => *request_id,
        }
    }

    pub fn set_request_id(&mut self, id: u32) {
        match self {
            Self::Error { request_id, .. }
            | Self::InitReq { request_id, .. }
            | Self::InitRes { request_id, .. }
            | Self::LookupReq { request_id, .. }
            | Self::LookupRes { request_id, .. }
            | Self::GetAttrReq { request_id, .. }
            | Self::FileAttrRes { request_id, .. }
            | Self::SetAttrReq { request_id, .. }
            | Self::SuccessRes { request_id, .. }
            | Self::StatsReq { request_id, .. }
            | Self::StatsRes { request_id, .. }
            | Self::CreateReq { request_id, .. }
            | Self::ReadReq { request_id, .. }
            | Self::ReadRes { request_id, .. }
            | Self::WriteReq { request_id, .. }
            | Self::ReaddirReq { request_id, .. }
            | Self::RmReq { request_id, .. }
            | Self::MoveReq { request_id, .. } => *request_id = id,
        }
    }
}
