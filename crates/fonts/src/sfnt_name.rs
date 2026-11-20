//! A font file's family name, read from its `name` table alone: the table directory and the
//! table, not the whole file. Font lists name every installed font this way; reading each whole
//! file (system font folders hold hundreds of megabytes) only to list it was too slow.
//!
//! The layout is OpenType's (`sfnt` header, table records, `name` table records), for single
//! fonts and collections (the first font of a `ttcf`).

/// Most tables a font directory may list.
const MAX_TABLES: usize = 512;
/// Most bytes of a `name` table read.
const MAX_NAME_TABLE: usize = 1 << 20;
/// Longest family name kept, in characters.
const MAX_CHARS: usize = 80;

/// Name ids: the typographic family (16) when it is set, else the family (1).
const TYPOGRAPHIC_FAMILY: u16 = 16;
const FAMILY: u16 = 1;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// The family name of the font whose bytes `read(offset, len)` gives (fewer bytes at the end of
/// the file are fine; `None` when the range can't be read at all).
pub(crate) fn family(read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>) -> Option<String> {
    let head = read(0, 16)?;
    let font = if head.get(..4) == Some(b"ttcf") { u64::from(u32_at(&head, 12)?) } else { 0 };
    let sfnt = read(font, 12)?;
    let tables = usize::from(u16_at(&sfnt, 4)?).min(MAX_TABLES);
    let dir = read(font.checked_add(12)?, tables.checked_mul(16)?)?;
    let (offset, length) = (0..tables).find_map(|i| {
        let r = i.checked_mul(16)?;
        (dir.get(r..r.checked_add(4)?)? == b"name").then(|| Some((u32_at(&dir, r + 8)?, u32_at(&dir, r + 12)?)))?
    })?;
    let table = read(u64::from(offset), usize::try_from(length).ok()?.min(MAX_NAME_TABLE))?;
    family_in(&table)
}

/// The family name in a `name` table.
pub(crate) fn family_in(t: &[u8]) -> Option<String> {
    let count = usize::from(u16_at(t, 2)?);
    let strings = usize::from(u16_at(t, 4)?);
    // (score, text): Windows Unicode in US English first, then any Windows Unicode, then
    // Unicode, then Macintosh Roman.
    let mut best: Option<(u8, u16, String)> = None;
    for i in 0..count.min(4096) {
        let r = i.checked_mul(12)?.checked_add(6)?;
        let (platform, encoding, language, id) = (u16_at(t, r)?, u16_at(t, r + 2)?, u16_at(t, r + 4)?, u16_at(t, r + 6)?);
        if id != FAMILY && id != TYPOGRAPHIC_FAMILY {
            continue;
        }
        let (len, off) = (usize::from(u16_at(t, r + 8)?), usize::from(u16_at(t, r + 10)?));
        let start = strings.checked_add(off)?;
        let Some(raw) = t.get(start..start.checked_add(len)?) else { continue };
        let (score, text) = match platform {
            3 if encoding == 1 || encoding == 10 => (if language == 0x409 { 4 } else { 3 }, utf16(raw)),
            0 => (2, utf16(raw)),
            1 if encoding == 0 => (1, raw.iter().map(|b| char::from(*b)).collect()),
            _ => continue,
        };
        let text: String = text.chars().filter(|c| !c.is_control()).take(MAX_CHARS).collect();
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        // The typographic family wins over the family at the same score.
        let rank = (score, u16::from(id == TYPOGRAPHIC_FAMILY));
        if best.as_ref().is_none_or(|(s, i, _)| rank > (*s, u16::from(*i == TYPOGRAPHIC_FAMILY))) {
            best = Some((score, id, text));
        }
    }
    best.map(|(_, _, t)| t)
}

fn utf16(raw: &[u8]) -> String {
    let units: Vec<u16> = raw.chunks_exact(2).filter_map(|c| Some(u16::from_be_bytes([*c.first()?, *c.get(1)?]))).collect();
    char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

/// [`family`] of a font in memory.
pub(crate) fn family_of_bytes(bytes: &[u8]) -> Option<String> {
    family(&mut |off, len| {
        let start = usize::try_from(off).ok()?;
        let end = start.checked_add(len)?.min(bytes.len());
        bytes.get(start..end).map(<[u8]>::to_vec)
    })
}

/// [`family`] of a font file, reading only its directory and `name` table.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn family_of_file(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    family(&mut |off, len| {
        f.seek(SeekFrom::Start(off)).ok()?;
        let mut buf = Vec::new();
        (&mut f).take(u64::try_from(len).ok()?).read_to_end(&mut buf).ok()?;
        Some(buf)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-table font whose `name` table holds `records` (platform, encoding, language, id,
    /// text).
    fn font(records: &[(u16, u16, u16, u16, &str)]) -> Vec<u8> {
        let mut strings: Vec<u8> = Vec::new();
        let mut recs: Vec<u8> = Vec::new();
        for (p, e, l, id, text) in records {
            let bytes: Vec<u8> = if *p == 1 { text.bytes().collect() } else { text.encode_utf16().flat_map(u16::to_be_bytes).collect() };
            for v in [*p, *e, *l, *id, bytes.len() as u16, strings.len() as u16] {
                recs.extend(v.to_be_bytes());
            }
            strings.extend(bytes);
        }
        let mut name = Vec::new();
        for v in [0u16, records.len() as u16, (6 + recs.len()) as u16] {
            name.extend(v.to_be_bytes());
        }
        name.extend(recs);
        name.extend(strings);
        let mut f = Vec::new();
        f.extend(0x0001_0000u32.to_be_bytes());
        f.extend(1u16.to_be_bytes());
        f.extend([0u8; 6]);
        f.extend(*b"name");
        f.extend(0u32.to_be_bytes());
        f.extend(28u32.to_be_bytes());
        f.extend((name.len() as u32).to_be_bytes());
        f.extend(name);
        f
    }

    #[test]
    fn the_family_comes_from_the_name_table() {
        let f = font(&[(1, 0, 0, 1, "Mac Name"), (3, 1, 0x409, 1, "Arial"), (3, 1, 0x40C, 1, "Arial FR"), (3, 1, 0x409, 4, "Arial Regular")]);
        assert_eq!(family_of_bytes(&f).as_deref(), Some("Arial"));
        // The typographic family wins.
        let f = font(&[(3, 1, 0x409, 1, "Segoe UI Light"), (3, 1, 0x409, 16, "Segoe UI")]);
        assert_eq!(family_of_bytes(&f).as_deref(), Some("Segoe UI"));
        // A Macintosh-only name still names it.
        assert_eq!(family_of_bytes(&font(&[(1, 0, 0, 1, "Old")])).as_deref(), Some("Old"));
    }

    #[test]
    fn broken_fonts_have_no_family() {
        assert_eq!(family_of_bytes(&[]), None);
        assert_eq!(family_of_bytes(b"ttcf\0\0\0\0\0\0\0\x01\xff\xff\xff\xff"), None);
        let mut f = font(&[(3, 1, 0x409, 1, "Arial")]);
        f.truncate(40);
        assert_eq!(family_of_bytes(&f), None);
    }
}
