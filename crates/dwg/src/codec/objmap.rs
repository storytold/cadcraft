//! The object map (AcDb:Handles): handle → object location.
//!
//! Written from knowledge of the spec, chapter 23. The map is a series of
//! blocks, each starting with a big-endian 16-bit size (which counts the
//! two size bytes but not the trailing two-byte CRC), holding pairs of
//! modular chars: handle delta (never negative) and location delta
//! (signed). A block of size 2 ends the map.
//!
//! Whether the running handle/location restart at zero in every block is
//! not confirmed, so both readings are produced and the caller keeps the
//! one whose entries match the objects' own handles.

use super::bits::BitReader;
use super::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MapEntry {
    pub handle: u64,
    pub location: i64,
}

pub(crate) fn parse(data: &[u8], reset_per_block: bool) -> Result<Vec<MapEntry>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut last_handle: u64 = 0;
    let mut last_loc: i64 = 0;
    let mut blocks = 0usize;
    while let Some(&[hi, lo]) = data.get(pos..pos.saturating_add(2)) {
        let size = usize::from(u16::from_be_bytes([hi, lo]));
        if size <= 2 {
            break;
        }
        let body_end = pos.saturating_add(size);
        if body_end > data.len() {
            return Err(Error::truncated(format!("object map block at {pos} claims {size} bytes, only {} left", data.len().saturating_sub(pos))));
        }
        if reset_per_block {
            last_handle = 0;
            last_loc = 0;
        }
        let mut r = BitReader::with_bounds(data, (pos + 2) * 8, body_end * 8);
        while r.remaining() >= 16 {
            let dh = r.mc_unsigned()?;
            let dl = r.mc_signed()?;
            last_handle = last_handle.wrapping_add(dh);
            last_loc = last_loc.wrapping_add(dl);
            out.push(MapEntry { handle: last_handle, location: last_loc });
        }
        pos = body_end + 2; // skip the CRC
        blocks += 1;
        if blocks > 10_000_000 {
            return Err(Error::format("object map block runaway"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_blocks_with_reset() {
        // Block 1 (size 7 = 2 size bytes + 5 data bytes): (+1, +100), (+2, +20).
        // 100 has bit 0x40 set, so as a signed MC it takes two bytes: E4 00.
        // Block 2 (size 5): (+5, +300) with 300 = AC 02. Each block is
        // followed by a two-byte CRC; a size-2 block ends the map.
        let data = [
            0x00, 0x07, 0x01, 0xE4, 0x00, 0x02, 0x14, 0xAA, 0xBB, // block 1 + CRC
            0x00, 0x05, 0x05, 0xAC, 0x02, 0xCC, 0xDD, // block 2 + CRC
            0x00, 0x02, // end
        ];
        let reset = parse(&data, true).unwrap();
        assert_eq!(reset, vec![MapEntry { handle: 1, location: 100 }, MapEntry { handle: 3, location: 120 }, MapEntry { handle: 5, location: 300 },]);
        let running = parse(&data, false).unwrap();
        assert_eq!(running[2], MapEntry { handle: 8, location: 420 });
    }
}
