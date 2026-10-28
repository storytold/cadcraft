//! Bit-level reader for DWG bit-coded data.
//!
//! Spec: ODA Open Design Specification for .dwg files v5.4.1, chapter 2
//! "BIT CODES AND DATA DEFINITIONS" (§2.1–§2.13, verified against the spec
//! text). Bits are consumed most-significant-bit first within each byte.
//! Multi-byte raw values (RS, RL, RD…) are assembled little-endian from
//! bit-aligned bytes.

use super::error::{Error, Result};
use super::version::DwgVersion;

/// A reference to another object as stored in the file: a 4-bit code and
/// the handle (or offset) value. See §2.13.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandleRef {
    pub code: u8,
    pub value: u64,
}

impl HandleRef {
    /// Resolve to an absolute handle. Codes 6, 8, 0xA and 0xC are relative
    /// to the handle of the object that contains the reference.
    pub fn absolute(self, reference: u64) -> u64 {
        match self.code {
            0x6 => reference.wrapping_add(1),
            0x8 => reference.wrapping_sub(1),
            0xA => reference.wrapping_add(self.value),
            0xC => reference.wrapping_sub(self.value),
            _ => self.value,
        }
    }
}

#[derive(Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Current bit position from the start of `data`.
    pos: usize,
    /// Exclusive bit limit. Reads past it fail with `Truncated`.
    end: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, end: data.len() * 8 }
    }

    /// A reader over `data` restricted to bits `[start, end)`.
    pub fn with_bounds(data: &'a [u8], start: usize, end: usize) -> Self {
        let max = data.len() * 8;
        let end = end.min(max);
        BitReader { data, pos: start.min(end), end }
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }
    pub fn position(&self) -> usize {
        self.pos
    }
    pub fn end(&self) -> usize {
        self.end
    }
    pub fn remaining(&self) -> usize {
        self.end.saturating_sub(self.pos)
    }

    pub fn set_position(&mut self, bit: usize) -> Result<()> {
        if bit > self.end {
            return Err(Error::truncated(format!("seek to bit {bit} past limit {}", self.end)));
        }
        self.pos = bit;
        Ok(())
    }

    /// Narrow (never widen past the data) the readable limit.
    pub fn set_end(&mut self, end: usize) {
        self.end = end.min(self.data.len() * 8);
        if self.pos > self.end {
            self.pos = self.end;
        }
    }

    pub fn align_to_byte(&mut self) {
        self.pos = self.pos.saturating_add(7) & !7;
        if self.pos > self.end {
            self.pos = self.end;
        }
    }

    /// The byte at `i`; reading it is within `end` by now, but the data is only a claim.
    #[inline]
    fn byte(&self, i: usize) -> Result<u8> {
        self.data.get(i).copied().ok_or_else(|| Error::truncated(format!("byte {i} is past the data")))
    }

    #[inline]
    fn need(&self, n: usize) -> Result<()> {
        if self.pos.checked_add(n).is_none_or(|e| e > self.end) {
            Err(Error::truncated(format!("need {n} bit(s) at bit {} but limit is {}", self.pos, self.end)))
        } else {
            Ok(())
        }
    }

    pub fn skip_bits(&mut self, n: usize) -> Result<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }

    // ----- raw bit access -------------------------------------------------

    /// B: a single bit.
    #[inline]
    pub fn b(&mut self) -> Result<bool> {
        self.need(1)?;
        let byte = self.byte(self.pos >> 3)?;
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Ok(bit == 1)
    }

    /// Read `n` (≤ 64) bits as an unsigned number, first bit most significant.
    pub fn bits(&mut self, n: u32) -> Result<u64> {
        if n > 64 {
            return Err(Error::format(format!("{n} bits asked for at once")));
        }
        self.need(n as usize)?;
        let mut v: u64 = 0;
        for _ in 0..n {
            let byte = self.byte(self.pos >> 3)?;
            let bit = (byte >> (7 - (self.pos & 7))) & 1;
            v = (v << 1) | bit as u64;
            self.pos += 1;
        }
        Ok(v)
    }

    /// BB: a two-bit code.
    #[inline]
    pub fn bb(&mut self) -> Result<u8> {
        Ok(self.bits(2)? as u8)
    }

    /// 3B: read bits until a zero bit or until three bits were read (§2.1).
    pub fn b3(&mut self) -> Result<u8> {
        let mut v = 0u8;
        for _ in 0..3 {
            let bit = self.b()?;
            v = (v << 1) | bit as u8;
            if !bit {
                break;
            }
        }
        Ok(v)
    }

    // ----- raw (uncompressed) values --------------------------------------

    /// RC: raw char, eight bits, bit-aligned.
    #[inline]
    pub fn rc(&mut self) -> Result<u8> {
        self.need(8)?;
        let shift = self.pos & 7;
        let idx = self.pos >> 3;
        let v = if shift == 0 {
            self.byte(idx)?
        } else {
            let hi = self.byte(idx)? << shift;
            let lo = self.byte(idx + 1)? >> (8 - shift);
            hi | lo
        };
        self.pos += 8;
        Ok(v)
    }

    /// RS: raw short, little-endian.
    pub fn rs(&mut self) -> Result<u16> {
        let lo = self.rc()?;
        let hi = self.rc()?;
        Ok(u16::from_le_bytes([lo, hi]))
    }

    /// RL: raw long, little-endian.
    pub fn rl(&mut self) -> Result<u32> {
        let mut b = [0u8; 4];
        for x in &mut b {
            *x = self.rc()?;
        }
        Ok(u32::from_le_bytes(b))
    }

    /// Raw 64-bit little-endian value.
    pub fn rll(&mut self) -> Result<u64> {
        let mut b = [0u8; 8];
        for x in &mut b {
            *x = self.rc()?;
        }
        Ok(u64::from_le_bytes(b))
    }

    /// RD: raw IEEE double, little-endian.
    pub fn rd(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.rll()?))
    }

    pub fn rd2(&mut self) -> Result<[f64; 2]> {
        Ok([self.rd()?, self.rd()?])
    }

    pub fn rd3(&mut self) -> Result<[f64; 3]> {
        Ok([self.rd()?, self.rd()?, self.rd()?])
    }

    pub fn raw_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        self.need(n.saturating_mul(8))?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.rc()?);
        }
        Ok(out)
    }

    /// SN: 16-byte sentinel.
    pub fn sentinel(&mut self) -> Result<[u8; 16]> {
        let mut s = [0u8; 16];
        for x in &mut s {
            *x = self.rc()?;
        }
        Ok(s)
    }

    // ----- compressed values ---------------------------------------------

    /// BS: bitshort (§2.2). Returned as the raw 16-bit pattern; cast to
    /// `i16` where the field is signed.
    pub fn bs(&mut self) -> Result<u16> {
        match self.bb()? {
            0 => self.rs(),
            1 => Ok(self.rc()? as u16),
            2 => Ok(0),
            _ => Ok(256),
        }
    }

    /// BL: bitlong (§2.3). Cast to `i32` where the field is signed.
    pub fn bl(&mut self) -> Result<u32> {
        match self.bb()? {
            0 => self.rl(),
            1 => Ok(self.rc()? as u32),
            2 => Ok(0),
            _ => Err(Error::format(format!("BITLONG code 11 is unused (bit {})", self.pos - 2))),
        }
    }

    /// BLL: bitlonglong (§2.4). A length l followed by l bytes, least
    /// significant first. The spec says the length takes "1-3 bits"; this
    /// reads it as a plain three-bit number (0–7). VERIFY against real
    /// R2010+ files (used for graphics sizes and REQUIREDVERSIONS).
    pub fn bll(&mut self) -> Result<u64> {
        let len = self.bits(3)? as usize;
        let mut v = 0u64;
        for i in 0..len {
            v |= (self.rc()? as u64) << (8 * i);
        }
        Ok(v)
    }

    /// BD: bitdouble (§2.5).
    pub fn bd(&mut self) -> Result<f64> {
        match self.bb()? {
            0 => self.rd(),
            1 => Ok(1.0),
            2 => Ok(0.0),
            _ => Err(Error::format(format!("BITDOUBLE code 11 is unused (bit {})", self.pos - 2))),
        }
    }

    pub fn bd2(&mut self) -> Result<[f64; 2]> {
        Ok([self.bd()?, self.bd()?])
    }

    pub fn bd3(&mut self) -> Result<[f64; 3]> {
        Ok([self.bd()?, self.bd()?, self.bd()?])
    }

    /// DD: bitdouble with default (§2.9).
    pub fn dd(&mut self, default: f64) -> Result<f64> {
        match self.bb()? {
            0 => Ok(default),
            1 => {
                let mut b = default.to_le_bytes();
                for x in b.iter_mut().take(4) {
                    *x = self.rc()?;
                }
                Ok(f64::from_le_bytes(b))
            }
            2 => {
                let mut b = default.to_le_bytes();
                b[4] = self.rc()?;
                b[5] = self.rc()?;
                for x in b.iter_mut().take(4) {
                    *x = self.rc()?;
                }
                Ok(f64::from_le_bytes(b))
            }
            _ => self.rd(),
        }
    }

    /// BT: bit thickness (§2.10).
    pub fn bt(&mut self, ver: DwgVersion) -> Result<f64> {
        if ver >= DwgVersion::R2000 && self.b()? {
            return Ok(0.0);
        }
        self.bd()
    }

    /// BE: bit extrusion (§2.8).
    pub fn be(&mut self, ver: DwgVersion) -> Result<[f64; 3]> {
        if ver >= DwgVersion::R2000 && self.b()? {
            return Ok([0.0, 0.0, 1.0]);
        }
        self.bd3()
    }

    /// MC, signed form (§2.6): continuation flag 0x80 on every byte, and
    /// 0x40 on the final byte negates the value.
    pub fn mc_signed(&mut self) -> Result<i64> {
        let mut value: i64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.rc()?;
            // At most eight bytes: 7 data bits each, so the value stays below 2^62.
            if shift > 49 {
                return Err(Error::format("modular char too long"));
            }
            if b & 0x80 != 0 {
                value |= i64::from(b & 0x7F) << shift;
                shift += 7;
            } else {
                value |= i64::from(b & 0x3F) << shift;
                return Ok(if b & 0x40 != 0 { -value } else { value });
            }
        }
    }

    /// MC, unsigned form: the final byte keeps all seven data bits. Used
    /// where values are never negative (object-map handle deltas, the R2010+
    /// handle-stream size).
    pub fn mc_unsigned(&mut self) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.rc()?;
            if shift > 63 {
                return Err(Error::format("modular char too long"));
            }
            value |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    /// MS: modular short (§2.7). Little-endian 16-bit units, 0x8000 flags
    /// continuation, fifteen data bits per unit.
    pub fn ms(&mut self) -> Result<u32> {
        let mut value: u32 = 0;
        let mut shift = 0u32;
        loop {
            let w = self.rs()?;
            if shift > 30 {
                return Err(Error::format("modular short too long"));
            }
            value |= ((w & 0x7FFF) as u32) << shift;
            if w & 0x8000 == 0 {
                return Ok(value);
            }
            shift += 15;
        }
    }

    /// H: handle reference (§2.13): 4-bit code, 4-bit byte counter, then the
    /// value's bytes most significant first.
    pub fn handle(&mut self) -> Result<HandleRef> {
        let first = self.rc()?;
        let code = first >> 4;
        let counter = (first & 0x0F) as usize;
        if counter > 8 {
            return Err(Error::format(format!("handle counter {counter} > 8 at bit {}", self.pos - 8)));
        }
        let mut value: u64 = 0;
        for _ in 0..counter {
            value = (value << 8) | self.rc()? as u64;
        }
        Ok(HandleRef { code, value })
    }

    /// OT: object type (§2.12, R2010+).
    pub fn ot(&mut self) -> Result<u16> {
        match self.bb()? {
            0 => Ok(self.rc()? as u16),
            1 => Ok(self.rc()? as u16 + 0x1F0),
            _ => self.rs(),
        }
    }

    /// T (R13–R2004): BS length then that many 8-bit characters in the
    /// drawing's code page. Returned undecoded.
    pub fn text8(&mut self) -> Result<Vec<u8>> {
        let len = self.bs()? as usize;
        self.raw_bytes(len)
    }

    /// TU: BS length (in UTF-16 code units) then UTF-16LE units.
    pub fn text16(&mut self) -> Result<String> {
        let len = self.bs()? as usize;
        self.need(len.saturating_mul(16))?;
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            units.push(self.rs()?);
        }
        while units.last() == Some(&0) {
            units.pop();
        }
        Ok(String::from_utf16_lossy(&units))
    }
}

/// Bit-level writer, the mirror of [`BitReader`]: bits go most significant
/// first within each byte, multi-byte raw values little-endian. Each
/// compressed form picks the shortest encoding the reader accepts, which
/// is also the choice AutoCAD makes (checked against AC1032 files written
/// by AutoCAD 2027).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitWriter {
    out: Vec<u8>,
    /// Number of bits written.
    n: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bits written so far.
    pub fn position(&self) -> usize {
        self.n
    }

    /// The bytes, the last one padded with zero bits.
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }

    pub fn bytes(&self) -> &[u8] {
        &self.out
    }

    /// B
    pub fn b(&mut self, v: bool) {
        if self.n.is_multiple_of(8) {
            self.out.push(0);
        }
        if v {
            let last = self.out.len().saturating_sub(1);
            if let Some(byte) = self.out.get_mut(last) {
                *byte |= 0x80 >> (self.n % 8);
            }
        }
        self.n += 1;
    }

    /// `count` (≤ 64) bits of `v`, most significant first.
    pub fn bits(&mut self, v: u64, count: u32) {
        for i in (0..count).rev() {
            self.b((v >> i) & 1 == 1);
        }
    }

    /// Append every bit another writer holds.
    pub fn append(&mut self, other: &BitWriter) {
        if self.n.is_multiple_of(8) {
            self.out.extend_from_slice(&other.out);
            self.n += other.n;
            // `other` may end mid-byte: its padding bits are zero, as ours
            // would be.
            return;
        }
        let full = other.n / 8;
        for &byte in other.out.iter().take(full) {
            self.rc(byte);
        }
        let rest = other.n % 8;
        if rest > 0 {
            let last = other.out.get(full).copied().unwrap_or(0);
            self.bits(u64::from(last >> (8 - rest)), rest as u32);
        }
    }

    /// Zero bits up to the next byte boundary.
    pub fn pad_to_byte(&mut self) {
        self.n = self.out.len() * 8;
    }

    /// BB
    pub fn bb(&mut self, v: u8) {
        self.bits(v as u64 & 3, 2);
    }

    /// 3B (§2.1): 0 → "0", 2 → "10", 6 → "110", 7 → "111".
    pub fn b3(&mut self, v: u8) {
        match v {
            0 => self.b(false),
            2 => self.bits(0b10, 2),
            6 => self.bits(0b110, 3),
            _ => self.bits(0b111, 3),
        }
    }

    /// RC
    pub fn rc(&mut self, v: u8) {
        if self.n.is_multiple_of(8) {
            self.out.push(v);
            self.n += 8;
        } else {
            self.bits(v as u64, 8);
        }
    }

    /// RS
    pub fn rs(&mut self, v: u16) {
        for b in v.to_le_bytes() {
            self.rc(b);
        }
    }

    /// RL
    pub fn rl(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.rc(b);
        }
    }

    pub fn rll(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.rc(b);
        }
    }

    /// RD
    pub fn rd(&mut self, v: f64) {
        self.rll(v.to_bits());
    }

    pub fn rd2(&mut self, v: [f64; 2]) {
        for x in v {
            self.rd(x);
        }
    }

    pub fn rd3(&mut self, v: [f64; 3]) {
        for x in v {
            self.rd(x);
        }
    }

    pub fn raw_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.rc(b);
        }
    }

    /// BS (§2.2): 0 and 256 in two bits, 1–255 in a byte.
    pub fn bs(&mut self, v: u16) {
        match v {
            0 => self.bb(2),
            256 => self.bb(3),
            1..=255 => {
                self.bb(1);
                self.rc(v as u8);
            }
            _ => {
                self.bb(0);
                self.rs(v);
            }
        }
    }

    /// BL (§2.3).
    pub fn bl(&mut self, v: u32) {
        match v {
            0 => self.bb(2),
            1..=255 => {
                self.bb(1);
                self.rc(v as u8);
            }
            _ => {
                self.bb(0);
                self.rl(v);
            }
        }
    }

    /// BLL (§2.4), with the length as three bits, as [`BitReader::bll`]
    /// reads it.
    pub fn bll(&mut self, v: u64) {
        let len = (8 - v.leading_zeros() as usize / 8).min(7);
        let len = if v == 0 { 0 } else { len };
        self.bits(len as u64, 3);
        for i in 0..len {
            self.rc((v >> (8 * i)) as u8);
        }
    }

    /// BD (§2.5): 1.0 and 0.0 in two bits (exactly those values, so a
    /// negative zero keeps its sign).
    pub fn bd(&mut self, v: f64) {
        if v.to_bits() == 1.0f64.to_bits() {
            self.bb(1);
        } else if v.to_bits() == 0 {
            self.bb(2);
        } else {
            self.bb(0);
            self.rd(v);
        }
    }

    pub fn bd2(&mut self, v: [f64; 2]) {
        for x in v {
            self.bd(x);
        }
    }

    pub fn bd3(&mut self, v: [f64; 3]) {
        for x in v {
            self.bd(x);
        }
    }

    /// DD (§2.9): only the bytes that differ from `default`.
    pub fn dd(&mut self, v: f64, default: f64) {
        let b = v.to_le_bytes();
        let d = default.to_le_bytes();
        if b == d {
            self.bb(0);
        } else if b[4..] == d[4..] {
            self.bb(1);
            self.raw_bytes(&b[..4]);
        } else if b[6..] == d[6..] {
            self.bb(2);
            self.rc(b[4]);
            self.rc(b[5]);
            self.raw_bytes(&b[..4]);
        } else {
            self.bb(3);
            self.rd(v);
        }
    }

    /// BT (§2.10).
    pub fn bt(&mut self, v: f64, ver: DwgVersion) {
        if ver >= DwgVersion::R2000 {
            if v.to_bits() == 0 {
                self.b(true);
                return;
            }
            self.b(false);
        }
        self.bd(v);
    }

    /// BE (§2.8).
    pub fn be(&mut self, v: [f64; 3], ver: DwgVersion) {
        if ver >= DwgVersion::R2000 {
            if v[0].to_bits() == 0 && v[1].to_bits() == 0 && v[2].to_bits() == 1.0f64.to_bits() {
                self.b(true);
                return;
            }
            self.b(false);
        }
        self.bd3(v);
    }

    /// MC, signed form (§2.6).
    pub fn mc_signed(&mut self, v: i64) {
        let neg = v < 0;
        let mut m = v.unsigned_abs();
        loop {
            if m < 0x40 {
                self.rc(m as u8 | if neg { 0x40 } else { 0 });
                return;
            }
            self.rc((m & 0x7F) as u8 | 0x80);
            m >>= 7;
        }
    }

    /// MC, unsigned form: the last byte keeps all seven data bits.
    pub fn mc_unsigned(&mut self, mut v: u64) {
        loop {
            if v < 0x80 {
                self.rc(v as u8);
                return;
            }
            self.rc((v & 0x7F) as u8 | 0x80);
            v >>= 7;
        }
    }

    /// MS (§2.7).
    pub fn ms(&mut self, mut v: u32) {
        loop {
            if v < 0x8000 {
                self.rs(v as u16);
                return;
            }
            self.rs((v & 0x7FFF) as u16 | 0x8000);
            v >>= 15;
        }
    }

    /// H (§2.13): the code, then the value in as few bytes as it needs,
    /// most significant first.
    pub fn handle(&mut self, code: u8, value: u64) {
        let len = if value == 0 { 0 } else { 8 - value.leading_zeros() as usize / 8 };
        self.rc((code << 4) | len as u8);
        for i in (0..len).rev() {
            self.rc((value >> (8 * i)) as u8);
        }
    }

    /// OT (§2.12, R2010+).
    pub fn ot(&mut self, t: u16) {
        if t < 0x100 {
            self.bb(0);
            self.rc(t as u8);
        } else if (0x1F0..0x2F0).contains(&t) {
            self.bb(1);
            self.rc((t - 0x1F0) as u8);
        } else {
            self.bb(2);
            self.rs(t);
        }
    }

    /// T (R13–R2004): BS length then the bytes.
    pub fn text8(&mut self, bytes: &[u8]) {
        self.bs(bytes.len() as u16);
        self.raw_bytes(bytes);
    }

    /// TU: BS length in UTF-16 units (no terminator), then the units.
    pub fn text16(&mut self, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        self.bs(units.len() as u16);
        for u in units {
            self.rs(u);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write with `write`, read back with `read`, and check the reader ends where the writer did.
    fn round_trip(write: impl Fn(&mut BitWriter), read: impl Fn(&mut BitReader)) {
        let mut w = BitWriter::new();
        write(&mut w);
        let bytes = w.clone().into_bytes();
        let mut r = BitReader::new(&bytes);
        read(&mut r);
        assert_eq!(r.position(), w.position());
    }

    const SHORTS: [u16; 7] = [0, 1, 255, 256, 257, 0xFFFF, 0x8000];
    const LONGS: [u32; 6] = [0, 1, 255, 256, 0x1234_5678, u32::MAX];
    const DOUBLES: [f64; 6] = [0.0, 1.0, -0.0, 2.5, -1e300, f64::MIN_POSITIVE];
    const DEFAULTED: [(f64, f64); 4] = [(1.0, 1.0), (1.000_000_1, 1.0), (120.5, 120.0), (3.0, -7.0)];

    /// Bits, shorts, longs, doubles and doubles with a default read back as written.
    #[test]
    fn plain_values_round_trip() {
        round_trip(
            |w| {
                w.b(true);
                SHORTS.iter().for_each(|&v| w.bs(v));
                LONGS.iter().for_each(|&v| w.bl(v));
                DOUBLES.iter().for_each(|&v| w.bd(v));
                DEFAULTED.iter().for_each(|&(v, d)| w.dd(v, d));
            },
            |r| {
                assert!(r.b().unwrap());
                SHORTS.iter().for_each(|&v| assert_eq!(r.bs().unwrap(), v));
                LONGS.iter().for_each(|&v| assert_eq!(r.bl().unwrap(), v));
                DOUBLES.iter().for_each(|&v| assert_eq!(r.bd().unwrap().to_bits(), v.to_bits()));
                DEFAULTED.iter().for_each(|&(v, d)| assert_eq!(r.dd(d).unwrap(), v));
            },
        );
    }

    const SIGNED: [i64; 8] = [0, 63, 64, 100, -100, 4610, -1413, 112_823_273];
    const UNSIGNED: [u64; 5] = [0, 127, 128, 4610, 1 << 40];
    const MODULAR_SHORTS: [u32; 4] = [0, 0x7FFF, 0x8000, 4_650_033];

    /// Modular chars (signed and unsigned) and modular shorts read back as written.
    #[test]
    fn modular_values_round_trip() {
        round_trip(
            |w| {
                SIGNED.iter().for_each(|&v| w.mc_signed(v));
                UNSIGNED.iter().for_each(|&v| w.mc_unsigned(v));
                MODULAR_SHORTS.iter().for_each(|&v| w.ms(v));
            },
            |r| {
                SIGNED.iter().for_each(|&v| assert_eq!(r.mc_signed().unwrap(), v));
                UNSIGNED.iter().for_each(|&v| assert_eq!(r.mc_unsigned().unwrap(), v));
                MODULAR_SHORTS.iter().for_each(|&v| assert_eq!(r.ms().unwrap(), v));
            },
        );
    }

    const HANDLES: [(u8, u64); 4] = [(5, 0), (4, 0x3D0B5), (2, 0xFF), (3, u64::MAX)];
    const TYPES: [u16; 5] = [0x13, 0x1F2, 0x1F4, 0x2EF, 0x300];
    const LONG_LONGS: [u64; 5] = [0, 1, 0xFF, 0x100, 0x12_3456_789A];
    const THREE_BITS: [u8; 4] = [0, 2, 6, 7];

    /// Handles, object types, long longs and three-bit codes read back as written.
    #[test]
    fn coded_values_round_trip() {
        round_trip(
            |w| {
                HANDLES.iter().for_each(|&(c, v)| w.handle(c, v));
                TYPES.iter().for_each(|&t| w.ot(t));
                LONG_LONGS.iter().for_each(|&v| w.bll(v));
                THREE_BITS.iter().for_each(|&v| w.b3(v));
            },
            |r| {
                HANDLES.iter().for_each(|&(c, v)| assert_eq!(r.handle().unwrap(), HandleRef { code: c, value: v }));
                TYPES.iter().for_each(|&t| assert_eq!(r.ot().unwrap(), t));
                LONG_LONGS.iter().for_each(|&v| assert_eq!(r.bll().unwrap(), v));
                THREE_BITS.iter().for_each(|&v| assert_eq!(r.b3().unwrap(), v));
            },
        );
    }

    /// Thickness, extrusion and Unicode text read back as written.
    #[test]
    fn entity_values_round_trip() {
        round_trip(
            |w| {
                w.bt(0.0, DwgVersion::R2018);
                w.bt(0.25, DwgVersion::R2018);
                w.be([0.0, 0.0, 1.0], DwgVersion::R2018);
                w.be([0.0, 1.0, 0.0], DwgVersion::R2018);
                w.text16("Größe Ω");
            },
            |r| {
                assert_eq!(r.bt(DwgVersion::R2018).unwrap(), 0.0);
                assert_eq!(r.bt(DwgVersion::R2018).unwrap(), 0.25);
                assert_eq!(r.be(DwgVersion::R2018).unwrap(), [0.0, 0.0, 1.0]);
                assert_eq!(r.be(DwgVersion::R2018).unwrap(), [0.0, 1.0, 0.0]);
                assert_eq!(r.text16().unwrap(), "Größe Ω");
            },
        );
    }

    /// The spec's worked examples come out bit for bit.
    #[test]
    fn writer_matches_the_spec_examples() {
        let mut w = BitWriter::new();
        for v in [257u16, 0, 256, 15, 0] {
            w.bs(v);
        }
        assert_eq!(w.position(), 34);
        assert_eq!(w.into_bytes(), bits_to_bytes("0000000001000000011011010000111110"));
        let mut w = BitWriter::new();
        w.mc_signed(4610);
        assert_eq!(w.into_bytes(), [0b1000_0010, 0b0010_0100]);
        let mut w = BitWriter::new();
        w.mc_signed(-1413);
        assert_eq!(w.into_bytes(), [0b1000_0101, 0b0100_1011]);
        let mut w = BitWriter::new();
        w.ms(4_650_033);
        assert_eq!(w.into_bytes(), [0b0011_0001, 0b1111_0100, 0b1000_1101, 0b0000_0000]);
        let mut w = BitWriter::new();
        w.handle(5, 0x5E7);
        assert_eq!(w.into_bytes(), [0x52, 0x05, 0xE7]);
    }

    #[test]
    fn appending_keeps_every_bit() {
        let mut a = BitWriter::new();
        a.bits(0b101, 3);
        let mut b = BitWriter::new();
        b.bits(0b1_1001_0101, 9);
        a.append(&b);
        assert_eq!(a.position(), 12);
        assert_eq!(a.into_bytes(), [0b1011_1001, 0b0101_0000]);
        let mut a = BitWriter::new();
        a.rc(0xAB);
        a.append(&b);
        assert_eq!(a.position(), 17);
        assert_eq!(a.into_bytes(), [0xAB, 0b1100_1010, 0b1000_0000]);
    }

    /// Turn a string of '0'/'1' characters (spaces ignored) into bytes,
    /// padding the last byte with zero bits.
    fn bits_to_bytes(s: &str) -> Vec<u8> {
        let bits: Vec<u8> = s.chars().filter(|c| *c == '0' || *c == '1').map(|c| (c == '1') as u8).collect();
        let mut out = vec![0u8; bits.len().div_ceil(8)];
        for (i, b) in bits.iter().enumerate() {
            out[i / 8] |= b << (7 - (i % 8));
        }
        out
    }

    // Worked examples printed in the spec, §2.2.
    #[test]
    fn bitshort_spec_example() {
        let data = bits_to_bytes("0000000001000000011011010000111110");
        let mut r = BitReader::new(&data);
        assert_eq!(r.bs().unwrap(), 257);
        assert_eq!(r.bs().unwrap(), 0);
        assert_eq!(r.bs().unwrap(), 256);
        assert_eq!(r.bs().unwrap(), 15);
        assert_eq!(r.bs().unwrap(), 0);
        assert_eq!(r.position(), 34);
    }

    // §2.3.
    #[test]
    fn bitlong_spec_example() {
        let data = bits_to_bytes("000000000100000001000000000000000010010000111110");
        let mut r = BitReader::new(&data);
        assert_eq!(r.bl().unwrap(), 257);
        assert_eq!(r.bl().unwrap(), 0);
        assert_eq!(r.bl().unwrap(), 15);
        assert_eq!(r.bl().unwrap(), 0);
    }

    // §2.6.
    #[test]
    fn modular_char_spec_examples() {
        let mut r = BitReader::new(&[0b1000_0010, 0b0010_0100]);
        assert_eq!(r.mc_signed().unwrap(), 4610);
        let mut r = BitReader::new(&[0b1110_1001, 0b1001_0111, 0b1110_0110, 0b0011_0101]);
        assert_eq!(r.mc_signed().unwrap(), 112_823_273);
        let mut r = BitReader::new(&[0b1000_0101, 0b0100_1011]);
        assert_eq!(r.mc_signed().unwrap(), -1413);
        let mut r = BitReader::new(&[0b1000_0010, 0b0010_0100]);
        assert_eq!(r.mc_unsigned().unwrap(), 4610);
    }

    // §2.7.
    #[test]
    fn modular_short_spec_example() {
        let mut r = BitReader::new(&[0b0011_0001, 0b1111_0100, 0b1000_1101, 0b0000_0000]);
        assert_eq!(r.ms().unwrap(), 4_650_033);
    }

    #[test]
    fn unaligned_raw_values() {
        // One leading zero bit, then RS 0x1234 little-endian (0x34, 0x12).
        let data = bits_to_bytes("0 00110100 00010010");
        let mut r = BitReader::new(&data);
        assert!(!r.b().unwrap());
        assert_eq!(r.rs().unwrap(), 0x1234);
    }

    #[test]
    fn bitdouble_and_default() {
        // 01 -> 1.0, 10 -> 0.0
        let data = bits_to_bytes("01 10");
        let mut r = BitReader::new(&data);
        assert_eq!(r.bd().unwrap(), 1.0);
        assert_eq!(r.bd().unwrap(), 0.0);
        // DD 00 keeps the default.
        let data = bits_to_bytes("00");
        let mut r = BitReader::new(&data);
        assert_eq!(r.dd(12.5).unwrap(), 12.5);
        // DD 01 patches the low four bytes of the default.
        let default: f64 = 2.0; // 0x4000000000000000
        let mut s = String::from("01");
        for b in [0x01u8, 0x00, 0x00, 0x00] {
            s.push_str(&format!("{b:08b}"));
        }
        let data = bits_to_bytes(&s);
        let mut r = BitReader::new(&data);
        let v = r.dd(default).unwrap();
        assert_eq!(v.to_bits(), 0x4000_0000_0000_0001);
    }

    #[test]
    fn handle_reference() {
        // code 5, counter 2, value 0x01A3
        let mut r = BitReader::new(&[0x52, 0x01, 0xA3]);
        let h = r.handle().unwrap();
        assert_eq!(h, HandleRef { code: 5, value: 0x01A3 });
        assert_eq!(HandleRef { code: 6, value: 0 }.absolute(0x10), 0x11);
        assert_eq!(HandleRef { code: 0xC, value: 3 }.absolute(0x10), 0x0D);
    }

    #[test]
    fn triplet() {
        let data = bits_to_bytes("0 10 110 111");
        let mut r = BitReader::new(&data);
        assert_eq!(r.b3().unwrap(), 0);
        assert_eq!(r.b3().unwrap(), 2);
        assert_eq!(r.b3().unwrap(), 6);
        assert_eq!(r.b3().unwrap(), 7);
    }

    #[test]
    fn truncation_is_an_error_not_a_panic() {
        let mut r = BitReader::new(&[0xFF]);
        assert!(r.rs().is_err());
        let mut r = BitReader::new(&[]);
        assert!(r.b().is_err());
    }
}
