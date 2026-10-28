//! The R2004-family paged container (spec chapter 4), as AutoCAD 2027
//! lays out an AC1032 file: the file header with its encrypted block, the
//! data pages of each section in stream order, the section map, the page
//! map, and the encrypted block again at the end.
//!
//! The choices the spec leaves open follow AutoCAD's own files of the
//! MI-201 corpus (2004, 2010, 2013 and 2018 alike): data pages are
//! numbered from 1 in stream order and the section map and page map take
//! the numbers 3 and 4 past the last data page; each system page is
//! followed by a copy of its header with the sizes and checksum zeroed;
//! the gaps that keep pages on 0x20 boundaries are filled from the magic
//! number sequence; the second copy of the encrypted header follows the
//! page map directly.

// Writes only buffers this file sizes itself from the sections it is given (never a size read
// from a file), so indexing them is checked by construction and by the round-trip tests.
#![allow(clippy::indexing_slicing)]

use super::bits::BitWriter;
use super::crc::{crc16, crc32, page_checksum};
use super::error::{Error, Result};
use super::lz77;
use super::r2004::magic_sequence;

const FILE_ID: &[u8; 12] = b"AcFssFcAJMB\0";
const PAGE_MAP_TYPE: u32 = 0x4163_0E3B;
const SECTION_MAP_TYPE: u32 = 0x4163_003B;
const DATA_PAGE_TYPE: u32 = 0x4163_043B;
const PAGE_MASK: u32 = 0x4164_536B;
/// The page size of compressed sections.
pub const MAX_PAGE: usize = 0x7400;

/// One section to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Its name in the section map (`AcDb:Header`, `AcDb:AcDbObjects`, ...).
    pub name: String,
    /// Its bytes, unpacked.
    pub data: Vec<u8>,
    pub compressed: bool,
    /// Bytes per page before compression; pages are padded to it.
    pub page_size: usize,
}

/// What the file header records beside the maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    pub version: [u8; 6],
    /// Byte 0x0B.
    pub maintenance: u8,
    /// Bytes 0x11 and 0x12: the application version and its maintenance
    /// release.
    pub app_version: u8,
    pub app_maintenance: u8,
    pub codepage: u16,
    /// Bytes 0x3C and 0x40 (R2018).
    pub acad_maintenance: u32,
}

fn round_up(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

fn put_u32(v: &mut [u8], at: usize, x: u32) {
    v[at..at + 4].copy_from_slice(&x.to_le_bytes());
}
fn put_u64(v: &mut [u8], at: usize, x: u64) {
    v[at..at + 8].copy_from_slice(&x.to_le_bytes());
}

/// Fill to `to` bytes with the magic number sequence (§4.3).
fn pad_to(out: &mut Vec<u8>, to: usize) {
    let n = to.saturating_sub(out.len());
    out.extend_from_slice(&magic_sequence(n));
}

/// A system page: header, compressed data, the header again with sizes
/// and checksum zeroed (§4.3).
fn system_page(kind: u32, data: &[u8], compress: Compress) -> Vec<u8> {
    let comp = compress(data).unwrap_or_else(|| data.to_vec());
    let mut hdr = [0u8; 20];
    put_u32(&mut hdr, 0, kind);
    put_u32(&mut hdr, 4, data.len() as u32);
    put_u32(&mut hdr, 8, comp.len() as u32);
    put_u32(&mut hdr, 12, 2);
    let sum = page_checksum(page_checksum(0, &hdr), &comp);
    put_u32(&mut hdr, 16, sum);
    let mut out = hdr.to_vec();
    out.extend_from_slice(&comp);
    let mut tail = [0u8; 20];
    put_u32(&mut tail, 0, kind);
    put_u32(&mut tail, 12, 2);
    out.extend_from_slice(&tail);
    out
}

/// The size AutoCAD records for the page map's page: room beyond what it
/// holds, by the number of pages (fitted to AutoCAD's files with 15, 17
/// and 18 pages: 0x5C0, 0x5E0, 0x600). The page map is the last page, so
/// the size places nothing.
fn page_map_extent(pages: usize) -> usize {
    round_up(0x4D0 + 16 * pages, 0x20)
}

struct PageRec {
    number: u32,
    address: usize,
    extent: usize,
}

/// Pack the sections, given in stream order. The section map lists them
/// with descending ids in the order given by `map_order` (names), which
/// must name every section once.
pub fn pack(info: &FileInfo, sections: &[Section], map_order: &[&str]) -> Result<Vec<u8>> {
    pack_with(info, sections, map_order, lz77::compress)
}

/// The page compressor (§4.7).
pub(crate) type Compress = fn(&[u8]) -> Option<Vec<u8>>;

/// The simplest stream §4.7 allows: one literal run and the end opcode.
#[cfg(test)]
pub(crate) fn literal_only(src: &[u8]) -> Option<Vec<u8>> {
    if src.len() < 4 {
        return None;
    }
    let mut out = Vec::with_capacity(src.len() + 8);
    let n = src.len();
    if n <= 0x12 {
        out.push((n - 3) as u8);
    } else {
        out.push(0);
        let mut v = n - 0x12;
        while v > 0xFF {
            out.push(0);
            v -= 0xFF;
        }
        out.push(v as u8);
    }
    out.extend_from_slice(src);
    out.extend_from_slice(&lz77::END);
    Some(out)
}

/// [`pack`] with a chosen compressor.
pub(crate) fn pack_with(info: &FileInfo, sections: &[Section], map_order: &[&str], compress: Compress) -> Result<Vec<u8>> {
    if let Some(s) = sections.iter().find(|s| s.name.len() > 63 || s.name.is_empty()) {
        return Err(Error::format(format!("section name {:?} must be 1 to 63 bytes", s.name)));
    }
    if let Some(s) = sections.iter().find(|s| s.page_size == 0) {
        return Err(Error::format(format!("section {} has a page size of 0", s.name)));
    }
    let mut out = vec![0u8; 0x100];
    let mut pages: Vec<PageRec> = Vec::new();
    // Section id by name: the empty section is 0, then descending in map
    // order.
    let n = map_order.len() as u32;
    let id_of = |name: &str| -> Result<u32> {
        let i = map_order.iter().position(|m| *m == name).ok_or_else(|| Error::format(format!("section {name} is not in the section map")))?;
        Ok(n - i as u32)
    };
    // (name, page number, data size, start) for the section map.
    let mut page_refs: Vec<(&str, u32, u32, u64)> = Vec::new();
    let mut first_page_address: Vec<(&str, usize)> = Vec::new();
    for s in sections {
        let id = id_of(&s.name)?;
        let chunks: Vec<&[u8]> = if s.data.is_empty() { Vec::new() } else { s.data.chunks(s.page_size).collect() };
        for (i, chunk) in chunks.iter().enumerate() {
            let mut page = chunk.to_vec();
            page.resize(s.page_size, 0);
            let stored = if s.compressed { compress(&page).unwrap_or(page) } else { page };
            let address = out.len();
            if i == 0 {
                first_page_address.push((s.name.as_str(), address));
            }
            let extent = round_up(32 + stored.len(), 0x20);
            let number = pages.len() as u32 + 1;
            let start = (i * s.page_size) as u64;
            let data_sum = page_checksum(0, &stored);
            let mut hdr = [0u8; 32];
            put_u32(&mut hdr, 0, DATA_PAGE_TYPE);
            put_u32(&mut hdr, 4, id);
            put_u32(&mut hdr, 8, stored.len() as u32);
            // §4.6: the page size decompressed (the reader's limit for the page), not its extent.
            put_u32(&mut hdr, 12, s.page_size as u32);
            put_u64(&mut hdr, 16, start);
            put_u32(&mut hdr, 28, data_sum);
            let header_sum = page_checksum(data_sum, &hdr);
            put_u32(&mut hdr, 24, header_sum);
            let m = PAGE_MASK ^ address as u32;
            for w in hdr.chunks_exact_mut(4) {
                let v = u32::from_le_bytes([w[0], w[1], w[2], w[3]]) ^ m;
                w.copy_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&hdr);
            out.extend_from_slice(&stored);
            pad_to(&mut out, address + extent);
            pages.push(PageRec { number, address, extent });
            page_refs.push((s.name.as_str(), number, stored.len() as u32, start));
        }
    }
    let last_data = pages.len() as u32;

    // Section map (§4.5).
    let mut sm = Vec::new();
    for v in [n + 1, 2, MAX_PAGE as u32, 0, n + 1] {
        sm.extend_from_slice(&v.to_le_bytes());
    }
    let mut describe = |name: &str, size: u64, page_size: u32, compressed: bool, id: u32| {
        let refs: Vec<_> = page_refs.iter().filter(|r| r.0 == name && !name.is_empty()).collect();
        sm.extend_from_slice(&size.to_le_bytes());
        sm.extend_from_slice(&(refs.len() as u32).to_le_bytes());
        sm.extend_from_slice(&page_size.to_le_bytes());
        sm.extend_from_slice(&1u32.to_le_bytes());
        sm.extend_from_slice(&(if compressed { 2u32 } else { 1 }).to_le_bytes());
        sm.extend_from_slice(&id.to_le_bytes());
        sm.extend_from_slice(&0u32.to_le_bytes());
        // Names are checked to fit (with their NUL) before anything is written.
        let mut nm = [0u8; 64];
        for (to, from) in nm.iter_mut().zip(name.bytes().take(63)) {
            *to = from;
        }
        sm.extend_from_slice(&nm);
        for r in refs {
            sm.extend_from_slice(&r.1.to_le_bytes());
            sm.extend_from_slice(&r.2.to_le_bytes());
            sm.extend_from_slice(&r.3.to_le_bytes());
        }
    };
    describe("", 0, MAX_PAGE as u32, true, 0);
    for name in map_order {
        let s = sections.iter().find(|s| s.name == *name).ok_or_else(|| Error::format(format!("section map names {name}, which is not written")))?;
        let id = id_of(&s.name)?;
        describe(&s.name, s.data.len() as u64, s.page_size as u32, s.compressed, id);
    }
    let section_map_id = last_data + 3;
    let sm_address = out.len();
    let sm_page = system_page(SECTION_MAP_TYPE, &sm, compress);
    let sm_extent = round_up(sm_page.len(), 0x20);
    out.extend_from_slice(&sm_page);
    pad_to(&mut out, sm_address + sm_extent);
    pages.push(PageRec { number: section_map_id, address: sm_address, extent: sm_extent });

    // Page map (§4.4), itself the last page.
    let page_map_id = last_data + 4;
    let pm_address = out.len();
    let pm_extent = page_map_extent(pages.len() + 1);
    pages.push(PageRec { number: page_map_id, address: pm_address, extent: pm_extent });
    let mut pm = Vec::new();
    for p in &pages {
        pm.extend_from_slice(&p.number.to_le_bytes());
        pm.extend_from_slice(&(p.extent as u32).to_le_bytes());
    }
    debug_assert!(pages.windows(2).all(|w| w[0].address + w[0].extent == w[1].address));
    out.extend_from_slice(&system_page(PAGE_MAP_TYPE, &pm, compress));
    let second_header = out.len();

    // The encrypted header block (§4.1).
    let mut e = [0u8; 0x6C];
    e[..12].copy_from_slice(FILE_ID);
    put_u32(&mut e, 0x10, 0x6C);
    put_u32(&mut e, 0x14, 4);
    put_u32(&mut e, 0x24, 1);
    put_u32(&mut e, 0x28, page_map_id);
    put_u64(&mut e, 0x2C, (pm_address + pm_extent - 0x100) as u64);
    put_u64(&mut e, 0x34, second_header as u64);
    put_u32(&mut e, 0x40, pages.len() as u32);
    put_u32(&mut e, 0x44, 0x20);
    put_u32(&mut e, 0x48, 0x80);
    put_u32(&mut e, 0x4C, 0x40);
    put_u32(&mut e, 0x50, page_map_id);
    put_u64(&mut e, 0x54, (pm_address - 0x100) as u64);
    put_u32(&mut e, 0x5C, section_map_id);
    put_u32(&mut e, 0x60, page_map_id);
    let crc = crc32(0, &e);
    put_u32(&mut e, 0x68, crc);
    let mask = magic_sequence(0x100);
    let encrypted: Vec<u8> = e.iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    out.extend_from_slice(&encrypted);

    // The plain file header.
    let at = |name: &str| -> u32 { first_page_address.iter().find(|(n, _)| *n == name).map(|(_, a)| *a as u32 + 0x20).unwrap_or(0) };
    let h = &mut out[..0x100];
    h[..6].copy_from_slice(&info.version);
    h[0x0B] = info.maintenance;
    h[0x0C] = 3;
    put_u32(h, 0x0D, at("AcDb:Preview"));
    h[0x11] = info.app_version;
    h[0x12] = info.app_maintenance;
    h[0x13..0x15].copy_from_slice(&info.codepage.to_le_bytes());
    put_u32(h, 0x20, at("AcDb:SummaryInfo"));
    put_u32(h, 0x28, 0x80);
    put_u32(h, 0x2C, at("AcDb:AppInfo"));
    put_u32(h, 0x30, at("AcDb:AppInfoHistory"));
    put_u32(h, 0x34, 0x202);
    put_u32(h, 0x38, 0x1DD);
    put_u32(h, 0x3C, info.acad_maintenance);
    put_u32(h, 0x40, info.acad_maintenance);
    h[0x80..0xEC].copy_from_slice(&encrypted);
    h[0xEC..0x100].copy_from_slice(&mask[0xEC..0x100]);
    Ok(out)
}

/// The most bytes of entries in one object map block (§20.1: a block holds at most 2032).
const MAP_BLOCK: usize = 2032;

/// The object map (AcDb:Handles) for `(handle, offset)` entries in ascending handle order: blocks
/// of handle and offset deltas, each with its size and checksum, ended by an empty block.
pub fn object_map(entries: &[(u64, u64)]) -> Result<Vec<u8>> {
    if let Some(w) = entries.windows(2).find(|w| w.first().map(|a| a.0) >= w.get(1).map(|b| b.0)) {
        return Err(Error::format(format!("object map entries must be in ascending handle order ({w:?})")));
    }
    let mut out = Vec::new();
    let mut rest = entries;
    while !rest.is_empty() {
        let mut block: Vec<u8> = vec![0, 0];
        let (mut last_h, mut last_o) = (0u64, 0i64);
        let mut taken = 0;
        for &(h, o) in rest {
            let o = i64::try_from(o).map_err(|_| Error::format(format!("object offset {o} is too large")))?;
            let mut w = BitWriter::new();
            w.mc_unsigned(h - last_h);
            w.mc_signed(o - last_o);
            let bytes = w.into_bytes();
            if block.len() + bytes.len() > MAP_BLOCK && taken > 0 {
                break;
            }
            block.extend_from_slice(&bytes);
            (last_h, last_o) = (h, o);
            taken += 1;
        }
        rest = rest.get(taken..).unwrap_or_default();
        let n = u16::try_from(block.len()).map_err(|_| Error::format("object map block too large"))?;
        block[..2].copy_from_slice(&n.to_be_bytes());
        let crc = crc16(0xC0C1, &block);
        out.extend_from_slice(&block);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let end = [0u8, 2];
    out.extend_from_slice(&end);
    out.extend_from_slice(&crc16(0xC0C1, &end).to_be_bytes());
    Ok(out)
}
