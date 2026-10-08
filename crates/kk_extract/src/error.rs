//! Error type shared by all modules.
use std::fmt;

#[derive(Debug)]
pub struct Error(pub String);

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(format!("io: {e}"))
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error(format!("json: {e}"))
    }
}
impl From<png::EncodingError> for Error {
    fn from(e: png::EncodingError) -> Self {
        Error(format!("png: {e}"))
    }
}
impl From<String> for Error {
    fn from(e: String) -> Self {
        Error(e)
    }
}
impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error(e.to_string())
    }
}

/// `err!("fmt", args)` builds an `Error`.
#[macro_export]
macro_rules! err {
    ($($t:tt)*) => { $crate::error::Error(format!($($t)*)) };
}

/// Little-endian readers with bounds checks.
pub fn rd_u16(d: &[u8], p: usize) -> Result<u16> {
    d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| err!("read u16 past end at {p:#x}"))
}
pub fn rd_u32(d: &[u8], p: usize) -> Result<u32> {
    d.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| err!("read u32 past end at {p:#x}"))
}
pub fn rd_i16(d: &[u8], p: usize) -> Result<i16> {
    rd_u16(d, p).map(|v| v as i16)
}
pub fn rd_f32(d: &[u8], p: usize) -> Result<f32> {
    rd_u32(d, p).map(f32::from_bits)
}
