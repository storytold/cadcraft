//! R2004-family container (AC1018, AC1024, AC1027, AC1032): an encrypted
//! file header, a page map, a section map and compressed data pages.
//!
//! Spec §4.1–§4.7 (verified against the spec text). R2010, R2013 and R2018
//! use the same container (spec chapters 6–8).

use super::container::{Container, SectionLayout};
use super::crc::{crc32, page_checksum};
use super::error::{Error, ErrorKind, Result};
use super::lz77;
use super::types::{Diagnostic, SectionInfo, Severity};
use super::version::DwgVersion;
use std::collections::HashMap;

const FILE_ID: &[u8; 12] = b"AcFssFcAJMB\0";
const PAGE_MAP_TYPE: u32 = 0x4163_0E3B;
const SECTION_MAP_TYPE: u32 = 0x4163_003B;
const DATA_PAGE_TYPE: u32 = 0x4163_043B;
const PAGE_MASK: u32 = 0x4164_536B;
/// Refuse to allocate absurd section sizes from corrupt maps.
const MAX_SECTION: u64 = 2 * 1024 * 1024 * 1024;
/// What all of a file's sections together may declare, as a multiple of the file's size. A
/// section's size is only a claim of its map: a damaged or hostile map can declare gigabytes in
/// a file of a few kilobytes. AutoCAD's own files hold one to three times their size (the MI-201
/// corpus, 2004 to 2018, measured 1.2 to 3.0), so 64 times leaves room for drawings that compress
/// far better while keeping what a file can make the reader allocate in proportion to the file.
const MAX_EXPANSION: u64 = 64;
/// The least that budget is, so small files are never refused for it.
const MIN_BUDGET: u64 = 64 << 20;

/// The bytes all of `file`'s sections may decompress to (`MAX_EXPANSION`).
/// The most a page map or section map is unpacked to (16 MB: about two million pages).
const MAX_SYSTEM_PAGE: usize = 16 << 20;

fn section_budget(file: &[u8]) -> u64 {
    (file.len() as u64).saturating_mul(MAX_EXPANSION).max(MIN_BUDGET)
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(d: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(o..o + 8)?.try_into().ok()?))
}

/// The "magic number sequence" (§4.1): an LCG with seed 1.
pub(crate) fn magic_sequence(len: usize) -> Vec<u8> {
    let mut seed: u32 = 1;
    (0..len)
        .map(|_| {
            seed = seed.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
            (seed >> 16) as u8
        })
        .collect()
}

struct Page {
    address: u64,
    size: u32,
}

#[derive(Debug)]
struct PageRef {
    number: i32,
    data_size: u32,
    start: u64,
}

#[derive(Debug)]
struct SectionDesc {
    name: String,
    size: u64,
    max_decompressed: u32,
    compressed: u32,
    encrypted: u32,
    pages: Vec<PageRef>,
}

/// Read a system page (page map or section map) at `addr`.
fn system_page(file: &[u8], addr: u64, notes: &mut Vec<Diagnostic>) -> Result<(u32, Vec<u8>)> {
    let a = usize::try_from(addr).map_err(|_| Error::format("page address overflow"))?;
    let hdr =
        a.checked_add(20).and_then(|e| file.get(a..e)).ok_or_else(|| Error::truncated(format!("system page header at {a:#x} past end of file")))?;
    let kind = u32_at(hdr, 0).unwrap_or(0);
    let decompressed = u32_at(hdr, 4).unwrap_or(0) as usize;
    let compressed = u32_at(hdr, 8).unwrap_or(0) as usize;
    let compression = u32_at(hdr, 12).unwrap_or(0);
    let stored_checksum = u32_at(hdr, 16).unwrap_or(0);
    let body = a
        .checked_add(20)
        .and_then(|s| Some(s..s.checked_add(compressed)?))
        .and_then(|r| file.get(r))
        .ok_or_else(|| Error::truncated(format!("system page data at {a:#x} past end of file")))?;
    // §4.3: header checksum (seed 0, checksum field zeroed), then the data
    // with that result as the seed. Reported, not enforced.
    let mut h = hdr.to_vec();
    if let Some(field) = h.get_mut(16..20) {
        field.fill(0);
    }
    let computed = page_checksum(page_checksum(0, &h), body);
    if computed != stored_checksum {
        notes.push(Diagnostic {
            severity: Severity::Info,
            message: format!("system page {kind:#010x} at {a:#x}: checksum {stored_checksum:#010x} != computed {computed:#010x}"),
        });
    }
    // A system page is a map: kilobytes even for the largest drawings. Its declared size is a claim.
    let decompressed = decompressed.min(MAX_SYSTEM_PAGE);
    let data =
        if compression == 2 { lz77::decompress(body, decompressed).map_err(|e| e.context(format!("system page at {a:#x}")))? } else { body.to_vec() };
    Ok((kind, data))
}

pub(crate) fn read(file: &[u8], ver: DwgVersion) -> Result<Container> {
    let short = || Error::truncated("file is shorter than the R2004 file header");
    if file.len() < 0x100 {
        return Err(short());
    }
    let maintenance = *file.get(0x0B).ok_or_else(short)?;
    let codepage = file.get(0x13..0x15).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).ok_or_else(short)?;
    let security_flags = u32_at(file, 0x18).unwrap_or(0);
    let mut notes = Vec::new();
    if security_flags != 0 {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: format!("security flags {security_flags:#x}: the drawing may be password protected"),
        });
    }

    // Decrypt the 0x6C-byte header block at 0x80.
    let mask = magic_sequence(0x6C);
    let mut hdr: Vec<u8> = file.get(0x80..0x80 + 0x6C).ok_or_else(short)?.iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    if hdr.get(..12) != Some(&FILE_ID[..]) {
        return Err(Error::format(format!("{}: encrypted file header did not decode to the expected file ID", ver.magic())));
    }
    let stored_crc = u32_at(&hdr, 0x68).unwrap_or(0);
    if let Some(field) = hdr.get_mut(0x68..0x6C) {
        field.fill(0);
    }
    let computed_crc = crc32(0, &hdr);
    if computed_crc != stored_crc {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: format!("file header CRC32 {stored_crc:#010x} != computed {computed_crc:#010x}"),
        });
    }
    let page_map_address = u64_at(&hdr, 0x54).unwrap_or(0).saturating_add(0x100);
    let section_map_id = u32_at(&hdr, 0x5C).unwrap_or(0) as i32;

    // Page map: page number -> file address.
    let (kind, pm) = system_page(file, page_map_address, &mut notes).map_err(|e| e.context("page map"))?;
    if kind != PAGE_MAP_TYPE {
        return Err(Error::format(format!("page map has type {kind:#010x}, expected {PAGE_MAP_TYPE:#010x}")));
    }
    let mut pages: HashMap<i32, Page> = HashMap::new();
    let mut addr: u64 = 0x100;
    let mut p = 0usize;
    while p + 8 <= pm.len() {
        let number = u32_at(&pm, p).unwrap_or(0) as i32;
        let size = u32_at(&pm, p + 4).unwrap_or(0);
        p += 8;
        if number < 0 {
            p += 16; // gap: parent, left, right, 0
        } else {
            pages.insert(number, Page { address: addr, size });
        }
        addr = addr.saturating_add(u64::from(size));
    }

    // Section map.
    let sm_addr =
        pages.get(&section_map_id).map(|pg| pg.address).ok_or_else(|| Error::format(format!("section map page {section_map_id} not in page map")))?;
    let (kind, sm) = system_page(file, sm_addr, &mut notes).map_err(|e| e.context("section map"))?;
    if kind != SECTION_MAP_TYPE {
        return Err(Error::format(format!("section map has type {kind:#010x}, expected {SECTION_MAP_TYPE:#010x}")));
    }
    let n_desc = u32_at(&sm, 0).ok_or_else(|| Error::truncated("section map header"))? as usize;
    let mut descs = Vec::new();
    let mut q = 20usize;
    for _ in 0..n_desc.min(4096) {
        let size = u64_at(&sm, q).ok_or_else(|| Error::truncated("section description"))?;
        let page_count = u32_at(&sm, q + 8).unwrap_or(0) as usize;
        let max_decompressed = u32_at(&sm, q + 12).unwrap_or(0x7400);
        let compressed = u32_at(&sm, q + 20).unwrap_or(2);
        let encrypted = u32_at(&sm, q + 28).unwrap_or(0);
        let name_bytes = sm.get(q + 32..q + 96).ok_or_else(|| Error::truncated("section name"))?;
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(64);
        let name = String::from_utf8_lossy(name_bytes.get(..name_len).unwrap_or(name_bytes)).into_owned();
        q += 96;
        let mut page_refs = Vec::with_capacity(page_count.min(65_536));
        for _ in 0..page_count {
            let number = u32_at(&sm, q).ok_or_else(|| Error::truncated("section page record"))? as i32;
            let data_size = u32_at(&sm, q + 4).unwrap_or(0);
            let start = u64_at(&sm, q + 8).unwrap_or(0);
            q += 16;
            page_refs.push(PageRef { number, data_size, start });
        }
        descs.push(SectionDesc { name, size, max_decompressed, compressed, encrypted, pages: page_refs });
    }

    let sections: Vec<SectionInfo> = descs
        .iter()
        .map(|d| SectionInfo {
            name: if d.name.is_empty() { "(unnamed)".into() } else { d.name.clone() },
            size: d.size,
            pages: d.pages.len() as u32,
            compressed: d.compressed == 2,
            encrypted: d.encrypted == 1,
        })
        .collect();

    let find = |name: &str| descs.iter().find(|d| d.name == name);
    let mut budget = section_budget(file);
    let load = |name: &str, notes: &mut Vec<Diagnostic>, budget: &mut u64| -> Result<Option<Vec<u8>>> {
        match find(name) {
            None => Ok(None),
            Some(d) => assemble(file, d, &pages, notes, budget).map(Some).map_err(|e| e.context(name)),
        }
    };
    let header = load("AcDb:Header", &mut notes, &mut budget).unwrap_or_else(|e| {
        notes.push(Diagnostic { severity: Severity::Warning, message: e.to_string() });
        None
    });
    let classes = load("AcDb:Classes", &mut notes, &mut budget).unwrap_or_else(|e| {
        notes.push(Diagnostic { severity: Severity::Warning, message: e.to_string() });
        None
    });
    let handles = load("AcDb:Handles", &mut notes, &mut budget)?.ok_or_else(|| Error::format("no AcDb:Handles section"))?;
    let objects = load("AcDb:AcDbObjects", &mut notes, &mut budget)?.ok_or_else(|| Error::format("no AcDb:AcDbObjects section"))?;
    let mut others = Vec::new();
    for d in &descs {
        if d.name.is_empty() || matches!(d.name.as_str(), "AcDb:Header" | "AcDb:Classes" | "AcDb:Handles" | "AcDb:AcDbObjects") {
            continue;
        }
        let mut quiet = Vec::new();
        if let Ok(bytes) = assemble(file, d, &pages, &mut quiet, &mut budget) {
            others.push((d.name.clone(), bytes));
        }
    }

    let layout = descs
        .iter()
        .map(|d| SectionLayout {
            name: d.name.clone(),
            page_size: d.max_decompressed,
            compressed: d.compressed == 2,
            encrypted: d.encrypted == 1,
            first_page: d.pages.first().map_or(0, |p| p.number),
        })
        .collect();
    Ok(Container { maintenance, codepage, header, classes, handles, objects, sections, others, layout, notes })
}

/// Reassemble one section from its pages, taking its size from what is left of the file's
/// `budget` (`section_budget`).
fn assemble(file: &[u8], d: &SectionDesc, pages: &HashMap<i32, Page>, notes: &mut Vec<Diagnostic>, budget: &mut u64) -> Result<Vec<u8>> {
    if d.encrypted == 1 {
        return Err(Error::new(ErrorKind::Unsupported, "section is encrypted (password-protected drawing)"));
    }
    if d.size > MAX_SECTION {
        return Err(Error::format(format!("section size {} is implausible", d.size)));
    }
    if d.size > *budget {
        return Err(Error::format(format!(
            "section size {} is more than a file of {} bytes holds (its sections may hold {} more)",
            d.size,
            file.len(),
            *budget
        )));
    }
    *budget -= d.size;
    let mut buf = vec![0u8; usize::try_from(d.size).map_err(|_| Error::format("section size is implausible"))?];
    for pr in &d.pages {
        let page = pages.get(&pr.number).ok_or_else(|| Error::format(format!("page {} not in page map", pr.number)))?;
        let past = || Error::truncated(format!("data page header at {:#x} past end of file", page.address));
        let a = usize::try_from(page.address).map_err(|_| past())?;
        let raw_hdr = a.checked_add(32).and_then(|e| file.get(a..e)).ok_or_else(past)?;
        // §4.6: each 32-bit word of the header is XORed with 0x4164536B ^ address.
        let m = PAGE_MASK ^ (page.address as u32);
        let word = |i: usize| u32_at(raw_hdr, i * 4).unwrap_or(0) ^ m;
        let page_type = word(0);
        if page_type != DATA_PAGE_TYPE {
            notes.push(Diagnostic {
                severity: Severity::Info,
                message: format!("{}: page {} header type {page_type:#010x} unexpected", d.name, pr.number),
            });
        }
        let header_size = word(2) as usize;
        let comp_size = if pr.data_size > 0 { pr.data_size as usize } else { header_size };
        let body = (a + 32)
            .checked_add(comp_size)
            .and_then(|e| file.get(a + 32..e))
            .ok_or_else(|| Error::truncated(format!("data page {} ({comp_size} bytes at {a:#x}) past end of file", pr.number)))?;
        let _ = page.size;
        let data = if d.compressed == 2 {
            let limit = (d.max_decompressed.max(word(3)) as usize).clamp(1, 0x10_0000);
            lz77::decompress(body, limit).map_err(|e| e.context(format!("page {}", pr.number)))?
        } else {
            body.to_vec()
        };
        if let Ok(start) = usize::try_from(pr.start)
            && start < buf.len()
        {
            let n = data.len().min(buf.len() - start);
            if let (Some(to), Some(from)) = (buf.get_mut(start..start + n), data.get(..n)) {
                to.copy_from_slice(from);
            }
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(size: u64) -> SectionDesc {
        SectionDesc { name: "AcDb:AcDbObjects".into(), size, max_decompressed: 0x7400, compressed: 2, encrypted: 0, pages: Vec::new() }
    }

    /// A section map's sizes are claims: one larger than the file can hold is refused before
    /// anything is allocated for it (a few kilobytes can't make the reader take gigabytes).
    #[test]
    fn a_section_larger_than_the_file_allows_is_refused_before_allocating() {
        let file = [0u8; 1000];
        let mut budget = section_budget(&file);
        assert_eq!(budget, MIN_BUDGET);
        let e = assemble(&file, &section(1 << 30), &HashMap::new(), &mut Vec::new(), &mut budget).unwrap_err();
        assert!(e.to_string().contains("more than a file of 1000 bytes holds"), "{e}");
        assert_eq!(budget, MIN_BUDGET, "nothing was taken");
        // A large file's budget grows with it.
        assert_eq!(section_budget(&vec![0u8; 2 << 20]), 128 << 20);
    }

    /// The budget is shared: many sections can't each claim the whole of it.
    #[test]
    fn every_section_takes_from_the_same_budget() {
        let file = [0u8; 16];
        let mut budget = 100;
        assert_eq!(assemble(&file, &section(60), &HashMap::new(), &mut Vec::new(), &mut budget).unwrap().len(), 60);
        assert_eq!(budget, 40);
        assert!(assemble(&file, &section(60), &HashMap::new(), &mut Vec::new(), &mut budget).is_err());
        assert_eq!(assemble(&file, &section(40), &HashMap::new(), &mut Vec::new(), &mut budget).unwrap().len(), 40);
        assert_eq!(budget, 0);
    }

    #[test]
    fn magic_sequence_starts_as_documented_lcg() {
        // seed = 1 * 0x343FD + 0x269EC3 = 0x29E2C0 -> first byte 0x29.
        let m = magic_sequence(4);
        assert_eq!(m[0], 0x29);
        // Recompute independently.
        let mut s: u64 = 1;
        let mut v = Vec::new();
        for _ in 0..4 {
            s = (s * 0x343FD + 0x269EC3) & 0xFFFF_FFFF;
            v.push((s >> 16) as u8);
        }
        assert_eq!(m, v);
    }
}
