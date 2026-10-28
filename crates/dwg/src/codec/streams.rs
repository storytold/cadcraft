//! The three streams an object (or the R2007+ header) is split into: main
//! data, strings (R2007+) and handles.
//!
//! Written from knowledge of the spec (chapter 20, "common object format",
//! R2007+ string stream). VERIFY against the spec PDF and real files.

use super::bits::BitReader;
use super::error::{Error, Result};
use super::text;
use super::version::DwgVersion;

pub(crate) struct Streams<'a> {
    pub ver: DwgVersion,
    pub codepage: u16,
    pub dat: BitReader<'a>,
    /// R2007+ string stream; `None` when the object stores no strings.
    pub strs: Option<BitReader<'a>>,
}

impl<'a> Streams<'a> {
    /// TV: 8-bit text in the main stream up to R2004, UTF-16 text in the
    /// string stream from R2007.
    pub fn tv(&mut self) -> Result<String> {
        if self.ver.has_string_stream() {
            match self.strs.as_mut() {
                Some(s) => s.text16(),
                None => Ok(String::new()),
            }
        } else {
            let raw = self.dat.text8()?;
            Ok(text::decode_ansi(&raw, self.codepage))
        }
    }
}

/// Locate the R2007+ string stream that ends just before bit `endbit` of
/// `data`. The bit at `endbit - 1` says whether strings are present; the 16
/// bits before it hold the stream size in bits, with 0x8000 flagging a
/// further 16-bit high part before that. Returns the string reader and the
/// bit where the strings begin.
pub(crate) fn string_stream(data: &[u8], endbit: usize) -> Result<Option<(BitReader<'_>, usize)>> {
    if endbit == 0 || endbit > data.len() * 8 {
        return Err(Error::format(format!("string stream end bit {endbit} out of range")));
    }
    let mut r = BitReader::with_bounds(data, 0, endbit);
    r.set_position(endbit - 1)?;
    if !r.b()? {
        return Ok(None);
    }
    let mut p = endbit.checked_sub(17).ok_or_else(|| Error::format("string stream flag without size"))?;
    r.set_position(p)?;
    let mut size = r.rs()? as usize;
    if size & 0x8000 != 0 {
        p = p.checked_sub(16).ok_or_else(|| Error::format("string stream size underflow"))?;
        r.set_position(p)?;
        let hi = r.rs()? as usize;
        size = (size & 0x7FFF) | (hi << 15);
    }
    let start = p.checked_sub(size).ok_or_else(|| Error::format(format!("string stream of {size} bits does not fit")))?;
    Ok(Some((BitReader::with_bounds(data, start, p), start)))
}
