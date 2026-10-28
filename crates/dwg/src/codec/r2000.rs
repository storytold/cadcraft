//! R13–R15 container (AC1012, AC1014, AC1015): a flat file with a table
//! of section locators at the start.
//!
//! Spec §3 "R13-R15 DWG FILE FORMAT ORGANIZATION" (verified against the
//! spec text).

use super::container::Container;
use super::crc::{crc16, file_header_crc_xor};
use super::error::{Error, Result};
use super::section::FILE_HEADER_END;
use super::types::{Diagnostic, SectionInfo, Severity};
use super::version::DwgVersion;

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

pub(crate) fn read(file: &[u8], _ver: DwgVersion) -> Result<Container> {
    let short = || Error::truncated("file header is incomplete");
    let maintenance = *file.get(0x0B).ok_or_else(short)?;
    let codepage = u16_at(file, 0x13).ok_or_else(short)?;
    let n = u32_at(file, 0x15).ok_or_else(short)? as usize;
    if n == 0 || n > 64 {
        return Err(Error::format(format!("implausible section-locator count {n}")));
    }
    let mut notes = Vec::new();
    let mut records = Vec::with_capacity(n);
    for i in 0..n {
        let base = 0x19 + i * 9;
        let number = *file.get(base).ok_or_else(short)?;
        let seeker = u32_at(file, base + 1).ok_or_else(short)? as usize;
        let size = u32_at(file, base + 5).ok_or_else(short)? as usize;
        records.push((number, seeker, size));
    }
    let crc_pos = 0x19 + n * 9;
    if let Some(stored) = u16_at(file, crc_pos)
        && let Some(head) = file.get(..crc_pos)
    {
        let computed = crc16(0, head) ^ file_header_crc_xor(n as u32);
        if computed != stored {
            notes.push(Diagnostic { severity: Severity::Info, message: format!("file header CRC {stored:#06X} != computed {computed:#06X}") });
        }
    }
    if file.get(crc_pos + 2..crc_pos + 18) != Some(&FILE_HEADER_END[..]) {
        notes.push(Diagnostic { severity: Severity::Warning, message: "file header end sentinel not found".into() });
    }

    let slice = |number: u8| -> Option<Vec<u8>> {
        let &(_, seeker, size) = records.iter().find(|r| r.0 == number)?;
        let end = seeker.checked_add(size)?;
        file.get(seeker..end).map(|s| s.to_vec())
    };
    let name = |number: u8| match number {
        0 => "Header",
        1 => "Classes",
        2 => "Object map",
        3 => "Special table",
        4 => "Template",
        _ => "Unknown",
    };
    let sections = records
        .iter()
        .map(|&(num, _, size)| SectionInfo {
            name: format!("{} (record {num})", name(num)),
            size: size as u64,
            pages: 1,
            compressed: false,
            encrypted: false,
        })
        .collect();

    let handles = slice(2).ok_or_else(|| Error::format("object map locator (record 2) missing or out of range"))?;
    Ok(Container {
        maintenance,
        codepage,
        header: slice(0),
        classes: slice(1),
        handles,
        objects: file.to_vec(),
        sections,
        others: Vec::new(),
        layout: Vec::new(),
        notes,
    })
}
