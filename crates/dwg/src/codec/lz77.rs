//! Compressor and decompressor for the R2004-family page compression
//! (AC1018, AC1024, AC1027, AC1032).
//!
//! Spec §4.7 "Compression" (verified against the spec text). The stream
//! starts with a literal length, then alternates copy opcodes (a back
//! reference into the output) with literal runs, and ends with opcode 0x11.
//! A back reference copies `compressedBytes` bytes starting `compOffset + 1`
//! bytes behind the current output position; overlapping copies are legal.
//!
//! [`compress`] is original work, used for DWG pages and the native `.blc`
//! format alike. The spec leaves room the decoder in AutoCAD does not
//! accept, so the compressor writes only what AutoCAD's own streams use,
//! observed across its MI-201 files from 2004 to 2018 (black-box: the
//! files, not its code):
//!
//! - every stream ends `11 00 00`, the end opcode followed by a zero two
//!   byte offset; the spec mentions only the `11`;
//! - no copy is shorter than four bytes (no `0x4X` near opcode, no `0x21`);
//! - far copies (more than 0x4000 back) use `0x12`–`0x16` for four to
//!   eight bytes and `0x10` with a count beyond that, never `0x18`–`0x1F`,
//!   which the spec reads as lengths 10–17 but AutoCAD never writes.
//!
//! Its tests check every output by decompressing it again.

use super::error::{Error, Result};

// ---------------------------------------------------------------------------
// Compressor
// ---------------------------------------------------------------------------

/// Largest back-reference distance the opcode forms can express
/// (far form: offset 0x3FFF + 14-bit field, distance = offset + 1).
const MAX_DISTANCE: usize = 0x3FFF + 0x3FFF + 1;
/// The shortest copy AutoCAD writes, in every form.
const MIN_MATCH: usize = 4;
/// The longest far copy AutoCAD writes with a length in the opcode
/// (`0x16`); longer ones take `0x10` and a count.
const FAR_SHORT_MAX: usize = 8;
/// What ends a stream: the end opcode and a zero two byte offset.
pub(crate) const END: [u8; 3] = [0x11, 0x00, 0x00];
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 48;
const GOOD_ENOUGH: usize = 128;

/// Unbounded count: 0x00 bytes add 0xFF each, the first non-zero byte ends
/// the value. `v` must be at least 1.
fn put_long(out: &mut Vec<u8>, mut v: usize) {
    debug_assert!(v >= 1);
    while v > 0xFF {
        out.push(0);
        v -= 0xFF;
    }
    out.push(v as u8);
}

/// Literal run header for `n >= 4` literals.
fn put_literal_length(out: &mut Vec<u8>, n: usize) {
    debug_assert!(n >= 4);
    if n <= 0x12 {
        out.push((n - 3) as u8);
    } else {
        out.push(0);
        put_long(out, n - 0x12);
    }
}

/// Emit a copy opcode. Returns the index of the byte whose low two bits
/// carry the trailing literal count (filled in later, 0 by default).
fn put_copy(out: &mut Vec<u8>, len: usize, distance: usize) -> usize {
    debug_assert!(len >= MIN_MATCH);
    let offset = distance - 1;
    if offset <= 0x3FF && len <= 14 {
        let at = out.len();
        out.push((((len + 1) << 4) | ((offset & 3) << 2)) as u8);
        out.push((offset >> 2) as u8);
        return at;
    }
    let raw = if offset <= 0x3FFF {
        if len <= 33 {
            out.push((0x1E + len) as u8);
        } else {
            out.push(0x20);
            put_long(out, len - 0x21);
        }
        offset
    } else {
        if len <= FAR_SHORT_MAX {
            out.push(0x10 | (len - 2) as u8);
        } else {
            // 0x10 + count gives 10 or more; a nine-byte copy is cut to
            // eight before it gets here.
            debug_assert!(len >= 10);
            out.push(0x10);
            put_long(out, len - 9);
        }
        offset - 0x3FFF
    };
    let at = out.len();
    out.push(((raw & 0x3F) << 2) as u8);
    out.push((raw >> 6) as u8);
    at
}

/// Compress `src` into the §4.7 opcode stream. Returns `None` for inputs of
/// 1–3 bytes, which the format cannot express (the first literal run must
/// be at least four bytes); store those uncompressed.
// The compressor indexes its own input and hash tables, sized from that input.
#[allow(clippy::indexing_slicing)]
pub fn compress(src: &[u8]) -> Option<Vec<u8>> {
    let n = src.len();
    if n == 0 {
        return Some(END.to_vec());
    }
    if n < 4 {
        return None;
    }
    let mut out = Vec::with_capacity(n / 2 + 16);
    let hash_size = 1usize << HASH_BITS;
    let mut head = vec![usize::MAX; hash_size];
    let mut prev = vec![usize::MAX; n];
    let hash = |p: usize| -> usize {
        let v = (src[p] as u32) | ((src[p + 1] as u32) << 8) | ((src[p + 2] as u32) << 16);
        (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
    };
    let insert = |p: usize, head: &mut Vec<usize>, prev: &mut Vec<usize>| {
        if p + 3 <= n {
            let h = hash(p);
            prev[p] = head[h];
            head[h] = p;
        }
    };

    let mut lit_start = 0usize;
    // Byte holding the previous copy's literal-count bits, once a copy exists.
    let mut last_copy: Option<usize> = None;
    let mut pos = 0usize;

    let flush = |out: &mut Vec<u8>, last_copy: Option<usize>, lits: &[u8]| {
        if lits.is_empty() {
            return;
        }
        match last_copy {
            Some(at) if lits.len() <= 3 => {
                out[at] |= lits.len() as u8;
            }
            _ => put_literal_length(out, lits.len()),
        }
        out.extend_from_slice(lits);
    };

    while pos < n {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        // The opening literal run must be at least four bytes long.
        if pos + 3 <= n && (last_copy.is_some() || pos >= 4) {
            let mut cand = head[hash(pos)];
            let mut chain = 0;
            while cand != usize::MAX && chain < MAX_CHAIN {
                let dist = pos - cand;
                if dist > MAX_DISTANCE {
                    break;
                }
                let mut l = 0;
                while pos + l < n && src[cand + l] == src[pos + l] {
                    l += 1;
                }
                if l >= MIN_MATCH && l > best_len {
                    best_len = l;
                    best_dist = dist;
                    if l >= GOOD_ENOUGH {
                        break;
                    }
                }
                cand = prev[cand];
                chain += 1;
            }
        }
        if best_len == FAR_SHORT_MAX + 1 && best_dist > 0x4000 {
            best_len = FAR_SHORT_MAX;
        }
        if best_len > 0 {
            flush(&mut out, last_copy, &src[lit_start..pos]);
            last_copy = Some(put_copy(&mut out, best_len, best_dist));
            for p in pos..pos + best_len {
                insert(p, &mut head, &mut prev);
            }
            pos += best_len;
            lit_start = pos;
        } else {
            insert(pos, &mut head, &mut prev);
            pos += 1;
        }
    }
    flush(&mut out, last_copy, &src[lit_start..n]);
    out.extend_from_slice(&END);
    Some(out)
}

// ---------------------------------------------------------------------------
// Decompressor
// ---------------------------------------------------------------------------

struct Input<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Input<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self.src.get(self.pos).ok_or_else(|| Error::truncated(format!("compressed data ends at byte {}", self.pos)))?;
        self.pos += 1;
        Ok(b)
    }

    /// Literal length (§4.7): 0x01–0x0F → value + 3; 0x00 → 0x0F plus
    /// 0xFF for every further zero byte plus the first non-zero byte, + 3.
    fn literal_length(&mut self, first: u8) -> Result<usize> {
        if first != 0 {
            return Ok(first as usize + 3);
        }
        let mut total = 0x0Fusize;
        loop {
            let b = self.byte()?;
            if b == 0 {
                total += 0xFF;
            } else {
                total += b as usize;
                return Ok(total + 3);
            }
        }
    }

    /// Long compression offset (§4.7): a non-zero byte is used as is; each
    /// 0x00 adds 0xFF and the first non-zero byte ends the value.
    fn long_count(&mut self) -> Result<usize> {
        let mut total = 0usize;
        loop {
            let b = self.byte()?;
            if b == 0 {
                total += 0xFF;
            } else {
                return Ok(total + b as usize);
            }
        }
    }

    /// Two byte offset (§4.7). Returns (offset, literal count 0–3).
    fn two_byte_offset(&mut self) -> Result<(usize, usize)> {
        let first = self.byte()?;
        let second = self.byte()?;
        let offset = ((first >> 2) as usize) | ((second as usize) << 6);
        Ok((offset, (first & 0x03) as usize))
    }
}

/// Decompress one page. `expected` is the decompressed size recorded in the
/// page header; producing more than that is treated as corruption.
pub fn decompress(src: &[u8], expected: usize) -> Result<Vec<u8>> {
    // `expected` comes from the file; grow as data arrives instead of
    // trusting it for one large allocation.
    let mut out: Vec<u8> = Vec::with_capacity(expected.min(src.len().saturating_mul(8)).min(1 << 20));
    let mut inp = Input { src, pos: 0 };

    let copy_literals = |inp: &mut Input, out: &mut Vec<u8>, n: usize| -> Result<()> {
        if out.len() + n > expected {
            return Err(Error::format(format!("literal run of {n} bytes overflows page size {expected}")));
        }
        let end = inp.pos + n;
        let bytes = inp.src.get(inp.pos..end).ok_or_else(|| Error::truncated(format!("literal run of {n} bytes at {} runs past input", inp.pos)))?;
        out.extend_from_slice(bytes);
        inp.pos = end;
        Ok(())
    };

    let mut op = inp.byte()?;
    if op & 0xF0 == 0 {
        let n = inp.literal_length(op)?;
        copy_literals(&mut inp, &mut out, n)?;
        op = inp.byte()?;
    }

    loop {
        if op == 0x11 {
            break;
        }
        let (count, offset, literals) = match op {
            0x10 => {
                let count = inp.long_count()? + 9;
                let (off, lit) = inp.two_byte_offset()?;
                (count, off + 0x3FFF, lit)
            }
            0x12..=0x1F => {
                let count = (op & 0x0F) as usize + 2;
                let (off, lit) = inp.two_byte_offset()?;
                (count, off + 0x3FFF, lit)
            }
            0x20 => {
                let count = inp.long_count()? + 0x21;
                let (off, lit) = inp.two_byte_offset()?;
                (count, off, lit)
            }
            0x21..=0x3F => {
                let count = op as usize - 0x1E;
                let (off, lit) = inp.two_byte_offset()?;
                (count, off, lit)
            }
            0x40..=0xFF => {
                let count = ((op & 0xF0) >> 4) as usize - 1;
                let op2 = inp.byte()?;
                let off = ((op2 as usize) << 2) | (((op & 0x0C) >> 2) as usize);
                (count, off, (op & 0x03) as usize)
            }
            _ => return Err(Error::format(format!("unexpected opcode {op:#04x} at compressed byte {}", inp.pos - 1))),
        };

        let distance = offset + 1;
        if distance > out.len() {
            return Err(Error::format(format!("back reference {distance} bytes behind output of {} bytes", out.len())));
        }
        if out.len() + count > expected {
            return Err(Error::format(format!("copy of {count} bytes overflows page size {expected}")));
        }
        // Byte by byte: a copy may overlap what it produces (a run).
        let start = out.len() - distance;
        for k in 0..count {
            let b = out.get(start + k).copied().ok_or_else(|| Error::format("back reference outside the output"))?;
            out.push(b);
        }

        if literals > 0 {
            copy_literals(&mut inp, &mut out, literals)?;
            op = inp.byte()?;
        } else {
            op = inp.byte()?;
            if op & 0xF0 == 0 {
                let n = inp.literal_length(op)?;
                copy_literals(&mut inp, &mut out, n)?;
                op = inp.byte()?;
            }
        }
    }
    Ok(out)
}

/// One copy in a stream: its opcode, length and distance back.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Copy {
    pub op: u8,
    pub len: usize,
    pub distance: usize,
}

/// The copies of a stream, in order, and the bytes after its end opcode.
#[cfg(test)]
pub(crate) fn copies(src: &[u8]) -> Result<(Vec<Copy>, Vec<u8>)> {
    let mut inp = Input { src, pos: 0 };
    let mut out = Vec::new();
    let mut produced = 0usize;
    let mut op = inp.byte()?;
    if op & 0xF0 == 0 {
        let n = inp.literal_length(op)?;
        inp.pos += n;
        produced += n;
        op = inp.byte()?;
    }
    while op != 0x11 {
        let (len, offset, mut literals) = match op {
            0x10 => (inp.long_count()? + 9, inp.two_byte_offset().map(|(o, l)| (o + 0x3FFF, l))?),
            0x12..=0x1F => ((op & 0x0F) as usize + 2, inp.two_byte_offset().map(|(o, l)| (o + 0x3FFF, l))?),
            0x20 => (inp.long_count()? + 0x21, inp.two_byte_offset()?),
            0x21..=0x3F => (op as usize - 0x1E, inp.two_byte_offset()?),
            0x40..=0xFF => {
                let op2 = inp.byte()?;
                (((op & 0xF0) >> 4) as usize - 1, (((op2 as usize) << 2) | (((op & 0x0C) >> 2) as usize), (op & 3) as usize))
            }
            _ => return Err(Error::format(format!("opcode {op:#04x}"))),
        }
        .flatten();
        out.push(Copy { op, len, distance: offset + 1 });
        produced += len;
        if literals == 0 {
            let b = inp.byte()?;
            if b & 0xF0 == 0 {
                literals = inp.literal_length(b)?;
            } else {
                inp.pos -= 1;
            }
        }
        inp.pos += literals;
        produced += literals;
        op = inp.byte()?;
    }
    let _ = produced;
    Ok((out, src[inp.pos..].to_vec()))
}

#[cfg(test)]
trait Flatten {
    fn flatten(self) -> (usize, usize, usize);
}
#[cfg(test)]
impl Flatten for (usize, (usize, usize)) {
    fn flatten(self) -> (usize, usize, usize) {
        (self.0, self.1.0, self.1.1)
    }
}

/// Whether a stream keeps to what AutoCAD writes (see the module notes).
/// Returns the first departure.
#[cfg(test)]
pub(crate) fn autocad_form(src: &[u8]) -> std::result::Result<(), String> {
    let (copies, tail) = copies(src).map_err(|e| e.message)?;
    if tail != [0, 0] {
        return Err(format!("stream ends 11 {tail:02x?}, not 11 00 00"));
    }
    for c in copies {
        if c.len < MIN_MATCH {
            return Err(format!("copy of {} bytes (opcode {:#04x})", c.len, c.op));
        }
        if (0x17..=0x1F).contains(&c.op) {
            return Err(format!("far opcode {:#04x}", c.op));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_only() {
        // 0x05 -> 8 literals (spec example: 0x05 = 0x08), then terminator.
        let mut src = vec![0x05];
        src.extend_from_slice(b"ABCDEFGH");
        src.push(0x11);
        assert_eq!(decompress(&src, 8).unwrap(), b"ABCDEFGH");
    }

    #[test]
    fn long_literal_length() {
        // Spec example: 0x00 0x02 = 0x14 literals.
        let payload: Vec<u8> = (0u8..0x14).collect();
        let mut src = vec![0x00, 0x02];
        src.extend_from_slice(&payload);
        src.push(0x11);
        assert_eq!(decompress(&src, 0x14).unwrap(), payload);
    }

    #[test]
    fn short_back_reference() {
        // "abcd", then opcode 0x4C: count ((0x40 & 0xF0) >> 4) - 1 = 3,
        // offset (0 << 2) | ((0x4C & 0x0C) >> 2) = 3 -> copy 3 bytes from 4
        // back; literal count 0, so the next byte is read as a literal length
        // or opcode (0x11 ends).
        let src = [0x01, b'a', b'b', b'c', b'd', 0x4C, 0x00, 0x11];
        assert_eq!(decompress(&src, 7).unwrap(), b"abcdabc");
    }

    #[test]
    fn two_byte_offset_with_trailing_literal() {
        // Four literals "xyz?", then opcode 0x22 (count 0x22 - 0x1E = 4) with
        // two-byte offset bytes 0x0D 0x00: offset = (0x0D >> 2) | (0 << 6) = 3
        // (distance 4), literal count = 0x0D & 3 = 1 -> one literal '!'.
        let src = [0x01, b'x', b'y', b'z', b'?', 0x22, 0x0D, 0x00, b'!', 0x11];
        assert_eq!(decompress(&src, 9).unwrap(), b"xyz?xyz?!");
    }

    #[test]
    fn long_compression_count() {
        // Opcode 0x20 with long count byte 0x01 -> 0x01 + 0x21 = 34 bytes,
        // distance 1 (offset 0): a run of the last literal.
        let src = [0x01, b'a', b'b', b'c', b'd', 0x20, 0x01, 0x00, 0x00, 0x11];
        let out = decompress(&src, 4 + 34).unwrap();
        assert_eq!(&out[..4], b"abcd");
        assert!(out[4..].iter().all(|&b| b == b'd'));
        assert_eq!(out.len(), 38);
    }

    #[test]
    fn corrupt_reference_is_error() {
        let src = [0x01, b'a', b'b', b'c', b'd', 0x4C, 0xFF, 0x11];
        assert!(decompress(&src, 100).is_err());
        assert!(decompress(&[], 10).is_err());
    }
}

#[cfg(test)]
mod compressor {
    use super::{END, autocad_form, compress, decompress};

    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn writes_only_the_forms_autocad_writes() {
        let mut seed = 0x5EED_u64;
        let block: Vec<u8> = (0..40_000).map(|_| rng(&mut seed) as u8).collect();
        let mut data = Vec::new();
        data.extend_from_slice(&block[..20_000]);
        // Far copies of every length from 3 to 20, each after fresh bytes.
        for len in 3..=20 {
            data.extend_from_slice(&block[20_000 + len * 50..20_000 + len * 50 + 40]);
            data.extend_from_slice(&block[len * 300..len * 300 + len]);
        }
        // Near copies of 3 bytes.
        data.extend_from_slice(b"abc1abc2abc3abc4");
        data.extend_from_slice(&vec![0u8; 0x7400 - data.len() % 0x7400]);
        for chunk in data.chunks(0x7400) {
            let c = compress(chunk).unwrap();
            assert_eq!(decompress(&c, chunk.len()).unwrap(), chunk);
            autocad_form(&c).unwrap();
        }
        for n in 4..80 {
            let v: Vec<u8> = (0..n).map(|i| (i % 7) as u8).collect();
            autocad_form(&compress(&v).unwrap()).unwrap();
        }
        // The checker catches what the compressor used to write.
        let mut bare = vec![0x05];
        bare.extend_from_slice(b"ABCDEFGH");
        bare.push(0x11);
        assert!(autocad_form(&bare).is_err());
        let three = [0x01, b'a', b'b', b'c', b'd', 0x4C, 0x00, 0x11, 0, 0];
        assert!(autocad_form(&three).is_err());
    }

    fn roundtrip(data: &[u8]) {
        let c = compress(data).expect("compressible length");
        let d = decompress(&c, data.len()).expect("decompress own output");
        assert_eq!(d, data, "roundtrip failed for {} bytes", data.len());
    }

    #[test]
    fn tiny_inputs() {
        assert_eq!(compress(&[]).unwrap(), END.to_vec());
        assert!(compress(b"a").is_none());
        assert!(compress(b"abc").is_none());
        for n in 4..64 {
            roundtrip(&vec![b'q'; n]);
            let v: Vec<u8> = (0..n as u8).collect();
            roundtrip(&v);
        }
    }

    #[test]
    fn all_opcode_forms() {
        let mut seed = 0xDEAD_BEEF_u64;
        let block: Vec<u8> = (0..40_000).map(|_| rng(&mut seed) as u8).collect();
        // Near short, near long (two-byte), far (> 0x4000) and very long runs.
        let mut data = Vec::new();
        data.extend_from_slice(&block[..2000]);
        data.extend_from_slice(&block[100..110]); // 0x40 form
        data.extend_from_slice(&block[..300]); // near two-byte, long count
        data.extend_from_slice(&block[2000..22_000]);
        data.extend_from_slice(&block[..12]); // far, short
        data.extend_from_slice(&block[500..900]); // far, long
        data.extend_from_slice(&vec![7u8; 5000]); // overlapping run
        data.extend_from_slice(&block[22_000..]);
        roundtrip(&data);
        let c = compress(&data).unwrap();
        assert!(c.len() < data.len());
    }

    #[test]
    fn random_and_structured() {
        let mut seed = 0x0123_4567_89AB_CDEF_u64;
        for round in 0..300 {
            let len = 4 + (rng(&mut seed) % 5000) as usize;
            let alphabet = 1 + (rng(&mut seed) % 255) as u8;
            let data: Vec<u8> = (0..len)
                .map(|i| {
                    let r = rng(&mut seed);
                    if round % 3 == 0 {
                        (r % alphabet as u64) as u8
                    } else {
                        // Repetitive text-like content.
                        b"LINE 10 20 30 LAYER 0 "[(i + (r as usize % 3)) % 22]
                    }
                })
                .collect();
            roundtrip(&data);
        }
    }

    #[test]
    fn literal_counts_after_copies() {
        // Copies followed by 1, 2, 3, 4 and 19+ literals.
        let mut data = b"abcdefgh".to_vec();
        for k in [1usize, 2, 3, 4, 5, 18, 19, 20, 300] {
            data.extend_from_slice(b"abcdefgh");
            data.extend((0..k).map(|i| 200u8.wrapping_add((i * 7 + k) as u8)));
        }
        roundtrip(&data);
    }
}

#[cfg(test)]
mod robustness {
    #[test]
    fn random_input_does_not_panic() {
        let mut x: u64 = 0x1234_5678_9ABC_DEF1;
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let len = (x % 300) as usize;
            let src: Vec<u8> = (0..len)
                .map(|i| {
                    let v = x.rotate_left((i % 64) as u32) as u8;
                    v ^ (i as u8)
                })
                .collect();
            let _ = super::decompress(&src, 0x7400);
        }
    }
}
