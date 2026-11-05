//! The installed and registered fonts by family and style, as a font picker lists them. Only
//! each file's table directory and naming table are read, so listing every installed font is
//! cheap; the installed fonts are listed once per process.

use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, OnceLock};

use skrifa::FontRef;
use skrifa::raw::tables::name::{Name, NameId};
use skrifa::raw::types::Tag;
use skrifa::raw::{FontData, FontRead};

use super::{Source, lock};

/// A font face a text style can name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontEntry {
    /// The family ("Arial"): the typographic family when the font names one.
    pub family: String,
    /// The style within the family ("Regular", "Bold Italic").
    pub style: String,
    /// What a text style stores to use this face: its file name ("arialbd.ttf").
    pub file: String,
}

/// Faces listed at most, and the longest family or style name kept.
const MAX_ENTRIES: usize = 5000;
const MAX_NAME_CHARS: usize = 128;
/// Bounds on what is read of one file: table records, and the naming table's size.
const MAX_TABLES: usize = 512;
const MAX_NAME_TABLE: usize = 1 << 20;

/// Every installed or registered font face (the first face of a collection), by family then
/// style (Regular, Italic, Bold, Bold Italic, then the rest by name). Fonts without a usable
/// naming table are left out.
pub fn installed_fonts() -> Vec<FontEntry> {
    static INSTALLED: OnceLock<Vec<FontEntry>> = OnceLock::new();
    // Registered fonts (web builds, tests) have no file; installed files loaded so far are
    // listed from their paths.
    let registered: Vec<(String, Arc<Vec<u8>>)> = {
        let g = lock();
        g.files.iter().filter(|(k, _)| !g.paths.contains_key(*k)).map(|(k, b)| (k.clone(), b.clone())).collect()
    };
    let installed = INSTALLED.get_or_init(|| {
        let paths: Vec<(String, Source)> = lock().paths.iter().map(|(k, s)| (k.clone(), s.clone())).collect();
        let mut out = Vec::new();
        for (stem, source) in paths {
            let entry = match source {
                Source::File(path) => file_names(&path).map(|(family, style)| {
                    let file = path.file_name().map(|f| f.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| format!("{stem}.ttf"));
                    FontEntry { family, style, file }
                }),
                Source::Static(bytes) => memory_names(bytes).map(|(family, style)| FontEntry { family, style, file: format!("{stem}.ttf") }),
            };
            out.extend(entry);
            if out.len() >= MAX_ENTRIES {
                break;
            }
        }
        out
    });
    let mut all = installed.clone();
    for (stem, bytes) in registered {
        if let Some((family, style)) = memory_names(&bytes) {
            all.push(FontEntry { family, style, file: format!("{stem}.ttf") });
        }
    }
    sorted(all)
}

/// `entries` sorted, one per family and style (the first file listed wins).
fn sorted(mut entries: Vec<FontEntry>) -> Vec<FontEntry> {
    entries.sort_by(|a, b| {
        a.family.to_lowercase().cmp(&b.family.to_lowercase()).then(style_rank(&a.style).cmp(&style_rank(&b.style))).then(a.file.cmp(&b.file))
    });
    entries.dedup_by(|a, b| a.family.eq_ignore_ascii_case(&b.family) && a.style.eq_ignore_ascii_case(&b.style));
    entries.truncate(MAX_ENTRIES);
    entries
}

/// Regular, Italic, Bold and Bold Italic first, as AutoCAD's Font Style list has them.
fn style_rank(style: &str) -> (usize, String) {
    let s = style.to_lowercase();
    let rank = ["regular", "italic", "bold", "bold italic"].iter().position(|r| *r == s).unwrap_or(4);
    (rank, s)
}

/// (family, style) of a font file's first face, reading only its table directory and naming
/// table.
fn file_names(path: &std::path::Path) -> Option<(String, String)> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut read = |offset: u64, len: usize| -> Option<Vec<u8>> {
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    names_from(&mut read)
}

/// (family, style) of the first face of font bytes held in memory.
fn memory_names(bytes: &[u8]) -> Option<(String, String)> {
    let font = FontRef::from_index(bytes, 0).ok()?;
    let data = font.table_data(Tag::new(b"name"))?;
    family_and_style(data.as_bytes())
}

/// (family, style) of the first face of a font file read through `read(offset, len)`: the sfnt
/// or collection header, the table records, then the naming table.
pub(super) fn names_from(read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>) -> Option<(String, String)> {
    let head = read(0, 12)?;
    let mut face = 0u64;
    if head.get(0..4)? == b"ttcf" {
        // A collection: the offset of its first face's table directory.
        let first = read(12, 4)?;
        face = u64::from(be32(&first, 0)?);
    }
    let dir = if face == 0 { head } else { read(face, 12)? };
    let tables = usize::from(be16(&dir, 4)?).min(MAX_TABLES);
    let records = read(face + 12, tables * 16)?;
    let (offset, len) = (0..tables).find_map(|i| {
        let r = records.get(i * 16..i * 16 + 16)?;
        (r.get(0..4)? == b"name").then(|| Some((be32(r, 8)?, be32(r, 12)?)))?
    })?;
    let len = usize::try_from(len).ok().filter(|l| *l > 0 && *l <= MAX_NAME_TABLE)?;
    family_and_style(&read(u64::from(offset), len)?)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The family and style a naming table gives: the typographic family and subfamily (IDs 16, 17)
/// when present, else the family and subfamily (IDs 1, 2); English names preferred.
fn family_and_style(table: &[u8]) -> Option<(String, String)> {
    let name = Name::read(FontData::new(table)).ok()?;
    let data = name.string_data();
    // Lower is better: Windows English, other Windows, Mac English, Unicode, anything else.
    let rank = |platform: u16, language: u16| match (platform, language) {
        (3, 0x0409) => 0,
        (3, _) => 1,
        (1, 0) => 2,
        (0, _) => 3,
        _ => 4,
    };
    let best = |id: NameId| -> Option<String> {
        let mut found: Option<(u8, String)> = None;
        for r in name.name_record().iter().filter(|r| r.name_id() == id) {
            let rk = rank(r.platform_id(), r.language_id());
            if found.as_ref().is_some_and(|(k, _)| *k <= rk) {
                continue;
            }
            let Ok(s) = r.string(data) else { continue };
            let s: String = s.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
            let s = s.trim().to_string();
            if !s.is_empty() {
                found = Some((rk, s));
            }
        }
        found.map(|(_, s)| s)
    };
    let family = best(NameId::TYPOGRAPHIC_FAMILY_NAME).or_else(|| best(NameId::FAMILY_NAME))?;
    let style = best(NameId::TYPOGRAPHIC_SUBFAMILY_NAME).or_else(|| best(NameId::SUBFAMILY_NAME)).unwrap_or_else(|| "Regular".into());
    Some((family, style))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16v(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_be_bytes());
    }

    /// A naming table with Windows English records (platform 3, encoding 1, UTF-16BE).
    pub(crate) fn name_table(records: &[(u16, &str)]) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut recs = Vec::new();
        for (id, s) in records {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
            for v in [3u16, 1, 0x0409, *id, bytes.len() as u16, strings.len() as u16] {
                be16v(&mut recs, v);
            }
            strings.extend(bytes);
        }
        let mut t = Vec::new();
        for v in [0u16, records.len() as u16, (6 + recs.len()) as u16] {
            be16v(&mut t, v);
        }
        t.extend(recs);
        t.extend(strings);
        t
    }

    /// An sfnt with only a naming table, or a collection of one such face.
    pub(crate) fn font_with_names(records: &[(u16, &str)], collection: bool) -> Vec<u8> {
        let name = name_table(records);
        let base = if collection { 16u32 } else { 0 };
        let mut out = Vec::new();
        if collection {
            out.extend_from_slice(b"ttcf");
            out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
            out.extend_from_slice(&1u32.to_be_bytes());
            out.extend_from_slice(&base.to_be_bytes());
        }
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        for v in [1u16, 16, 0, 0] {
            be16v(&mut out, v);
        }
        out.extend_from_slice(b"name");
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(base + 28).to_be_bytes());
        out.extend_from_slice(&(name.len() as u32).to_be_bytes());
        out.extend(name);
        out
    }

    fn read_from(bytes: &[u8]) -> impl FnMut(u64, usize) -> Option<Vec<u8>> + '_ {
        |offset, len| {
            let start = usize::try_from(offset).ok()?;
            bytes.get(start..start.checked_add(len)?).map(<[u8]>::to_vec)
        }
    }

    #[test]
    fn families_and_styles_come_from_the_naming_table() {
        let plain = font_with_names(&[(1, "Test Sans Semibold"), (2, "Regular"), (16, "Test Sans"), (17, "Semibold")], false);
        assert_eq!(names_from(&mut read_from(&plain)), Some(("Test Sans".into(), "Semibold".into())), "typographic names first");
        let old = font_with_names(&[(1, "Old Face"), (2, "Bold Italic")], true);
        assert_eq!(names_from(&mut read_from(&old)), Some(("Old Face".into(), "Bold Italic".into())), "a collection's first face");
        assert_eq!(memory_names(&font_with_names(&[(1, "In Memory")], false)), Some(("In Memory".into(), "Regular".into())));
        assert_eq!(names_from(&mut read_from(&font_with_names(&[(2, "Bold")], false))), None, "no family");
    }

    #[test]
    fn styles_sort_as_autocad_lists_them() {
        let e = |family: &str, style: &str, file: &str| FontEntry { family: family.into(), style: style.into(), file: file.into() };
        let got = sorted(vec![
            e("b", "Bold", "bb.ttf"),
            e("B", "Light", "bl.ttf"),
            e("b", "Regular", "b.ttf"),
            e("a", "Regular", "a.ttf"),
            e("b", "Bold Italic", "bz.ttf"),
            e("B", "regular", "copy.ttf"),
            e("b", "Italic", "bi.ttf"),
        ]);
        let order: Vec<&str> = got.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(order, ["a.ttf", "b.ttf", "bi.ttf", "bb.ttf", "bz.ttf", "bl.ttf"]);
    }

    #[test]
    fn hostile_font_files_never_panic() {
        let good = font_with_names(&[(1, "Fine")], true);
        let mut cases: Vec<Vec<u8>> = vec![Vec::new(), b"ttcf".to_vec(), vec![0xFF; 12], vec![0; 40]];
        for cut in [3, 12, 15, 20, 30, 40, good.len() - 1] {
            cases.push(good[..cut.min(good.len())].to_vec());
        }
        // A huge table count, a name table past the end, an absurd length.
        let mut many = good.clone();
        many[20..22].copy_from_slice(&u16::MAX.to_be_bytes());
        cases.push(many);
        let mut far = good.clone();
        far[36..40].copy_from_slice(&u32::MAX.to_be_bytes());
        cases.push(far);
        let mut long = good.clone();
        long[40..44].copy_from_slice(&u32::MAX.to_be_bytes());
        cases.push(long);
        let mut seed = 0x9E37_79B9u32;
        for _ in 0..200 {
            let mut b = good.clone();
            for _ in 0..4 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let i = seed as usize % b.len();
                b[i] = (seed >> 24) as u8;
            }
            cases.push(b);
        }
        for c in &cases {
            let _ = names_from(&mut read_from(c));
            let _ = memory_names(c);
            let _ = family_and_style(c);
        }
    }
}
