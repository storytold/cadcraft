//! Sentinel-wrapped sections (AcDb:Header, AcDb:Classes): start sentinel,
//! RL size, optional RL high size (R2010+), data, RS CRC, end sentinel.
//!
//! Sentinel byte values from the spec (R13–R15 header §3.2.6 verified; the
//! header/classes sentinels are written from knowledge of chapters 9–10).

use super::error::{Error, Result};
use super::version::DwgVersion;

pub(crate) const HEADER_START: [u8; 16] = [0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33, 0x5F];
pub(crate) const HEADER_END: [u8; 16] = [0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC, 0xA0];
pub(crate) const CLASSES_START: [u8; 16] = [0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6, 0x8A];
pub(crate) const CLASSES_END: [u8; 16] = [0x72, 0x5E, 0x3B, 0x47, 0x3B, 0x56, 0x07, 0x3A, 0x3F, 0x23, 0x0B, 0xA0, 0x18, 0x30, 0x49, 0x75];
/// Ends the R13–R15 file header (§3.2.6, verified).
pub(crate) const FILE_HEADER_END: [u8; 16] = [0x95, 0xA0, 0x4E, 0x28, 0x99, 0x82, 0x1A, 0xE5, 0x5E, 0x41, 0xE0, 0x5F, 0x9D, 0x3A, 0x4D, 0x00];

pub(crate) struct Wrapped<'a> {
    pub body: &'a [u8],
    /// Whether the end sentinel was found where expected.
    pub end_found: bool,
}

/// Find the data body of a sentinel-wrapped section. The optional high
/// size RL (R2010+) is detected by where the end sentinel sits.
pub(crate) fn unwrap<'a>(data: &'a [u8], start: &[u8; 16], end: &[u8; 16], ver: DwgVersion, maintenance: u8) -> Result<Wrapped<'a>> {
    if data.get(..16) != Some(&start[..]) {
        return Err(Error::format("section start sentinel not found"));
    }
    let size = data.get(16..20).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(|| Error::truncated("section size is missing"))?;
    // A size the platform can't address is as good as one past the data (wasm is 32-bit).
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    let candidates: &[bool] = if ver >= DwgVersion::R2010 { &[true, false] } else { &[false] };
    for &hi in candidates {
        let body_start: usize = 20 + if hi { 4 } else { 0 };
        let Some(body_end) = body_start.checked_add(size) else { continue };
        let Some(end_pos) = body_end.checked_add(2) else { continue };
        if end_pos.checked_add(16).and_then(|e| data.get(end_pos..e)) == Some(&end[..])
            && let Some(body) = data.get(body_start..body_end)
        {
            return Ok(Wrapped { body, end_found: true });
        }
    }
    // End sentinel not where expected: fall back to the rule in the spec
    // (high size present for R2018, and for R2010/R2013 with maintenance
    // version above 3) and clamp.
    let hi = ver >= DwgVersion::R2018 || (ver >= DwgVersion::R2010 && maintenance > 3);
    let body_start: usize = 20 + if hi { 4 } else { 0 };
    let body_end = body_start.saturating_add(size).min(data.len());
    let body = data.get(body_start..body_end).ok_or_else(|| Error::truncated("section body runs past data"))?;
    Ok(Wrapped { body, end_found: false })
}
