//! dBASE III tables (`.dbf`), the attribute tables of shapefiles: a 32-byte header, 32-byte
//! field descriptors ending at 0x0D, then fixed-width records each led by a deletion flag.

use crate::{IoError, Result};

const MAX_FIELDS: usize = 2048;

/// The records of a table as (field name, value) pairs, empty values left out.
#[derive(Debug, Default)]
pub(super) struct Table {
    pub rows: Vec<Vec<(String, String)>>,
    pub notes: Vec<String>,
}

struct Field {
    name: String,
    kind: u8,
    start: usize,
    len: usize,
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    let s: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
    Some(usize::from(u16::from_le_bytes(s)))
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    let s: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(s)).ok()
}

/// Text in UTF-8 when it is valid UTF-8, else read as Latin-1.
fn text(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| char::from(c)).collect(),
    }
}

fn value(kind: u8, raw: &[u8]) -> String {
    let s = text(raw);
    let s = s.trim_matches(|c: char| c == ' ' || c == '\0');
    match kind {
        b'L' => match s.chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('T' | 'Y') => "true".into(),
            Some('F' | 'N') => "false".into(),
            _ => String::new(),
        },
        _ => s.to_string(),
    }
}

/// Read a dBASE table. Memo fields (their text is in a `.dbt` file) are left out.
pub(super) fn read(b: &[u8]) -> Result<Table> {
    let bad = || IoError::Format("the shapefile's .dbf attribute table is damaged".into());
    let records = u32_at(b, 4).ok_or_else(bad)?;
    let header = u16_at(b, 8).ok_or_else(bad)?;
    let width = u16_at(b, 10).ok_or_else(bad)?;
    if header < 33 || width == 0 || header > b.len() {
        return Err(bad());
    }
    let mut fields = Vec::new();
    let mut start = 1usize;
    let mut at = 32usize;
    while at + 32 <= header && fields.len() < MAX_FIELDS {
        let d = b.get(at..at + 32).ok_or_else(bad)?;
        if d.first() == Some(&0x0D) {
            break;
        }
        let name_bytes = d.get(..11).unwrap_or_default();
        let name_end = name_bytes.iter().position(|&c| c == 0).unwrap_or(name_bytes.len());
        let name = text(name_bytes.get(..name_end).unwrap_or_default()).trim().to_string();
        let kind = d.get(11).copied().unwrap_or(b'C').to_ascii_uppercase();
        let len = usize::from(d.get(16).copied().unwrap_or(0));
        fields.push(Field { name, kind, start, len });
        start += len;
        at += 32;
    }
    let mut table = Table::default();
    let available = (b.len() - header) / width;
    if available < records {
        table.notes.push(format!("GIS: the .dbf attribute table ends after {available} of its {records} records"));
    }
    let mut short = false;
    for r in 0..records.min(available) {
        let row = b.get(header + r * width..header + (r + 1) * width).ok_or_else(bad)?;
        let mut attrs = Vec::new();
        for f in fields.iter().filter(|f| f.kind != b'M' && !f.name.is_empty()) {
            let Some(raw) = row.get(f.start..f.start + f.len) else {
                short = true;
                continue;
            };
            let v = value(f.kind, raw);
            if !v.is_empty() {
                attrs.push((f.name.clone(), v));
            }
        }
        table.rows.push(attrs);
    }
    if short {
        table.notes.push("GIS: some .dbf fields run past the record length and were left out".into());
    }
    Ok(table)
}
