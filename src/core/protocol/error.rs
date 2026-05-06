use binrw::binrw;

#[binrw]
#[brw(big, repr = u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Internal = 0,
    Invalid = 1,
    NotFound = 2,
    Exists = 3,
}
