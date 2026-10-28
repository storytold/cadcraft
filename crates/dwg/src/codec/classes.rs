//! AcDb:Classes — custom object classes (type codes 500 and up).
//!
//! Written from knowledge of the spec, chapter 10. VERIFY against the spec
//! PDF and real files.

use super::bits::BitReader;
use super::error::{Error, Result};
use super::section::{self, CLASSES_END, CLASSES_START};
use super::streams::{Streams, string_stream};
use super::types::ClassDef;
use super::version::DwgVersion as V;
use super::version::DwgVersion;

pub(crate) fn parse(data: &[u8], ver: DwgVersion, maintenance: u8, codepage: u16) -> Result<(Vec<ClassDef>, Vec<String>)> {
    let mut notes = Vec::new();
    let wrapped = section::unwrap(data, &CLASSES_START, &CLASSES_END, ver, maintenance)?;
    if !wrapped.end_found {
        notes.push("classes: end sentinel not found where expected".to_string());
    }
    let body = wrapped.body;
    if ver < V::R2007 {
        let s = Streams { ver, codepage, dat: BitReader::new(body), strs: None };
        return Ok((read_records(s)?, notes));
    }
    // R2007+: an RL bit size, then records whose strings sit in a string
    // stream ending at that bit. Which origin the bit size counts from is
    // not confirmed, so try both and keep the one that parses cleanly.
    let bitsize = body.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(|| Error::truncated("classes body too short"))?;
    let bitsize = usize::try_from(bitsize).unwrap_or(usize::MAX);
    let mut last_err = Error::format("no class layout matched");
    for endbit in [bitsize, bitsize.saturating_add(32)] {
        if endbit == 0 || endbit > body.len().saturating_mul(8) {
            continue;
        }
        let strs = match string_stream(body, endbit) {
            Ok(Some((sr, _))) => Some(sr),
            Ok(None) => None,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        let mut dat = BitReader::with_bounds(body, 32, endbit);
        dat.set_end(endbit);
        let s = Streams { ver, codepage, dat, strs };
        match read_records(s) {
            Ok(list) if list.iter().all(plausible) => return Ok((list, notes)),
            Ok(_) => last_err = Error::format("class records implausible"),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn plausible(c: &ClassDef) -> bool {
    c.number >= 500
        && (c.item_class_id == 0x1F2 || c.item_class_id == 0x1F3)
        && !c.dxf_name.is_empty()
        && c.dxf_name.chars().all(|ch| ch.is_ascii_graphic())
}

fn read_records(mut s: Streams) -> Result<Vec<ClassDef>> {
    let ver = s.ver;
    let mut expected: Option<usize> = None;
    if ver >= V::R2004 {
        let max_num = s.dat.bs()? as usize;
        let _zero1 = s.dat.rc()?;
        let _zero2 = s.dat.rc()?;
        let _flag = s.dat.b()?;
        expected = Some(max_num.saturating_sub(499));
    }
    let mut out = Vec::new();
    loop {
        if let Some(n) = expected {
            if out.len() >= n {
                break;
            }
        } else if s.dat.remaining() < 13 {
            break;
        }
        let mut c = ClassDef { number: s.dat.bs()?, proxy_flags: s.dat.bs()?, ..Default::default() };
        c.app_name = s.tv()?;
        c.cpp_name = s.tv()?;
        c.dxf_name = s.tv()?;
        c.was_zombie = s.dat.b()?;
        c.item_class_id = s.dat.bs()?;
        if ver >= V::R2004 {
            c.instance_count = s.dat.bl()?;
            // BL, as spec §5.8 gives them for R2007, not BS as §10.2 does:
            // real AC1018 and AC1032 files both parse only this way (a
            // maintenance number of 377 shows it).
            c.dwg_version = s.dat.bl()?;
            c.maintenance_version = s.dat.bl()?;
            let _unknown1 = s.dat.bl()?;
            let _unknown2 = s.dat.bl()?;
        }
        if c.number < 500 && expected.is_none() {
            // Padding or garbage after the last record.
            break;
        }
        out.push(c);
        if out.len() > 65_535 {
            return Err(Error::format("class count runaway"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct W {
        bytes: Vec<u8>,
        bits: usize,
    }
    impl W {
        fn bit(&mut self, v: bool) {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if v {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
        fn n(&mut self, v: u64, count: u32) {
            for i in (0..count).rev() {
                self.bit(v >> i & 1 != 0);
            }
        }
        fn rc(&mut self, v: u8) {
            self.n(v as u64, 8);
        }
        fn rs(&mut self, v: u16) {
            self.rc(v as u8);
            self.rc((v >> 8) as u8);
        }
        fn rl(&mut self, v: u32) {
            self.rs(v as u16);
            self.rs((v >> 16) as u16);
        }
        fn bs(&mut self, v: u16) {
            match v {
                0 => self.n(0b10, 2),
                256 => self.n(0b11, 2),
                1..=255 => {
                    self.n(0b01, 2);
                    self.rc(v as u8);
                }
                _ => {
                    self.n(0b00, 2);
                    self.rs(v);
                }
            }
        }
        fn bl(&mut self, v: u32) {
            match v {
                0 => self.n(0b10, 2),
                1..=255 => {
                    self.n(0b01, 2);
                    self.rc(v as u8);
                }
                _ => {
                    self.n(0b00, 2);
                    self.rl(v);
                }
            }
        }
        fn tu(&mut self, s: &str) {
            let units: Vec<u16> = s.encode_utf16().collect();
            self.bs(units.len() as u16);
            for u in units {
                self.rs(u);
            }
        }
    }

    /// An R2007+ classes section laid out as an AC1032 file saved by
    /// AutoCAD LT 2026 has it: the class's DWG and maintenance versions are
    /// BL (spec §5.8), not BS (§10.2); 377 does not fit a BS read here.
    #[test]
    fn r2007_plus_class_versions_are_bitlongs() {
        let names = ["ObjectDBX Classes", "AcDbDetailViewStyle", "ACDBDETAILVIEWSTYLE"];
        let mut w = W::default();
        w.rl(0); // bit size, patched below
        w.bs(500); // maximum class number
        w.rc(0);
        w.rc(0);
        w.bit(true);
        w.bs(500);
        w.bs(0x401);
        w.bit(false); // was a zombie
        w.bs(0x1F3);
        w.bl(1); // objects of this class
        w.bl(33); // DWG version
        w.bl(377); // maintenance version
        w.bl(0);
        w.bl(0);
        let strings_start = w.bits;
        for s in names {
            w.tu(s);
        }
        let size = (w.bits - strings_start) as u16;
        w.rs(size);
        w.bit(true); // string stream present
        let endbit = w.bits as u32;
        w.bytes[..4].copy_from_slice(&endbit.to_le_bytes());
        let body = w.bytes;

        let mut sec = CLASSES_START.to_vec();
        sec.extend_from_slice(&(body.len() as u32).to_le_bytes());
        sec.extend_from_slice(&0u32.to_le_bytes()); // high size (R2018)
        sec.extend_from_slice(&body);
        sec.extend_from_slice(&[0, 0]); // CRC
        sec.extend_from_slice(&CLASSES_END);

        let (list, _) = parse(&sec, V::R2018, 0, 30).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].number, 500);
        assert_eq!(list[0].app_name, names[0]);
        assert_eq!(list[0].cpp_name, names[1]);
        assert_eq!(list[0].dxf_name, names[2]);
        assert_eq!(list[0].item_class_id, 0x1F3);
    }
}
