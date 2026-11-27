//! IMAGEFRAME's value in a DXF file: the first group 70 of its last RASTERVARIABLES object.
//! Record layout: [DXF Reference 2017, RASTERVARIABLES](https://help.autodesk.com/cloudhelp/2017/ENU/AutoCAD-DXF/files/GUID-DDCC21A4-822A-469B-9954-1E1EC4F6DF82.htm).
//!
//! The shared parser turns a group's text into a number before this crate maps the drawing, so the
//! spelling is gone by then: `abc` and `NaN` become 0, `2.5` truncates to 2, and each of those
//! would select a valid mode. The mode is a drawing setting and a value that is not one must leave
//! the default, so the text of the selected group is checked here.
//!
//! Which group that is comes from the parser: the library's own section and record splits over its
//! decoded tags, so a record named with an escape (`RASTER\U+0056ARIABLES`) or a section name padded
//! with Unicode whitespace selects what the rest of the reader selects. Only that one group's value
//! text is read from the file's bytes again. Binary DXF has no spelling to lose (the group is a
//! 16-bit number), so there the parsed value is the value.

use std::borrow::Cow;

use craft_dxf::{Tag, record_slices, section_slices};

/// A binary DXF file's first bytes, from the DXF specification.
const BINARY_PREFIX: &[u8] = b"AutoCAD Binary DXF";
/// Bound the text probe's copies and decoded string allocations, not only its tag count.
/// An excessively padded legacy setting leaves the default instead of copying an entire line.
const MAX_TEXT_PROBE_BYTES: usize = 16 * 1024;

/// The IMAGEFRAME mode `bytes` sets in `tags`, or `None` when it sets none that can be used (the
/// caller then leaves the drawing's default, 1).
pub(super) fn image_frame(bytes: &[u8], tags: &[Tag]) -> Option<i64> {
    let (index, parsed) = selected(tags)?;
    if bytes.starts_with(BINARY_PREFIX) {
        // Already a number: 0, 1 and 2 are the modes, anything else is not one.
        return (0..=2).contains(&parsed).then_some(parsed);
    }
    spelled_mode(bytes, index)
}

/// Where the drawing's IMAGEFRAME group is in `tags`, and the number the parser read there: the
/// first group 70 of the last RASTERVARIABLES record of an OBJECTS section, as the reader takes it
/// (`T::i` finds the record's first group 70, and a later record replaces an earlier one).
fn selected(tags: &[Tag]) -> Option<(usize, i64)> {
    let mut found = None;
    for section in section_slices(tags) {
        if section.name != "OBJECTS" {
            continue;
        }
        for (kind, record) in record_slices(section.tags) {
            // A record type is compared exactly, as the reader compares it: only a section name is
            // trimmed, and only by the library's own split.
            if kind != "RASTERVARIABLES" {
                continue;
            }
            found = record.iter().find(|t| t.code == 70).and_then(|t| index_of(tags, t).map(|i| (i, t.i64())));
        }
    }
    found
}

/// The position of `tag` in `tags`, which it is a tag of.
fn index_of(tags: &[Tag], tag: &Tag) -> Option<usize> {
    let base = tags.as_ptr() as usize;
    let at = std::ptr::from_ref(tag) as usize;
    let index = at.checked_sub(base)? / std::mem::size_of::<Tag>();
    (index < tags.len()).then_some(index)
}

/// The mode the group at `index` spells in an ASCII file, or `None` when its text is no plain
/// integer 0, 1 or 2. Only the groups up to it are read, and a file that is UTF-8 (nearly all of
/// them) is not copied.
fn spelled_mode(bytes: &[u8], index: usize) -> Option<i64> {
    let utf8 = std::str::from_utf8(bytes).is_ok();
    let prefix = text_probe_prefix(bytes);
    // The parser skips a byte BOM, then a decoded BOM before the first code.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut lines = lines_of(bytes);
    let mut at = 0usize;
    while let Some(code_line) = lines.next() {
        // The parser skips a blank group-code line, and reads the code as an integer (it accepts
        // zero padding and a sign, as it accepts them for every group).
        let code_text = field_text(code_line, utf8, &prefix)?;
        let code = code_text.trim();
        if code.is_empty() {
            continue;
        }
        let Ok(code) = code.parse::<i32>() else { break };
        let Some(value) = lines.next() else { break };
        if at == index {
            // The parser read a group 70 here; its text is the mode's only trustworthy record.
            if code != 70 {
                return None;
            }
            // Numeric fields do not unescape strings. A text probe must not turn a literal
            // `\U+0030` into a mode, even though the shared text decoder unescapes it.
            if value.contains(&b'\\') {
                return None;
            }
            return plain_mode(&field_text(value, utf8, &prefix)?);
        }
        at += 1;
        // `index` came from the parsed stream, so it precedes its EOF. No later group is needed.
    }
    None
}

/// The lines of ASCII DXF text: LF, CRLF and a lone CR each end a line, with no empty line after a
/// final terminator, as the parser splits them. The parser's own line splitter is not public; this
/// is the splitter for one already-selected field, not a second DXF reader.
fn lines_of(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest.iter().position(|b| matches!(b, b'\r' | b'\n')).unwrap_or(rest.len());
        let line = rest.get(..end).unwrap_or_default();
        let after = rest.get(end..).unwrap_or_default();
        rest = match after {
            [b'\r', b'\n', tail @ ..] | [b'\r' | b'\n', tail @ ..] => tail,
            _ => after,
        };
        Some(line)
    })
}

/// A bounded text-only probe for the parser's own code-page decoder. Its raw-header lookup is
/// deliberately the one the pinned decoder uses: the first name before the first ENDSEC, with
/// group 3 immediately after it. Decoded/escaped header names must not choose a different codec.
fn text_probe_prefix(bytes: &[u8]) -> Vec<u8> {
    let mut code_page = "UTF8";
    let mut lines = bytes.split(|b| matches!(b, b'\r' | b'\n')).map(<[u8]>::trim_ascii).filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        if line == b"ENDSEC" {
            break;
        }
        if line.eq_ignore_ascii_case(b"$DWGCODEPAGE") {
            if let (Some(b"3"), Some(name)) = (lines.next(), lines.next()) {
                // Every supported name is short ASCII. An unknown name uses the parser's UTF8
                // fallback (which decodes invalid UTF-8 as Windows-1252), without a large copy.
                if let Ok(name) = std::str::from_utf8(name) {
                    let name = name.trim();
                    if name.len() <= 32 && craft_dxf::decode_code_page(b"", name).is_some() {
                        code_page = name;
                    }
                }
            }
            break;
        }
    }
    let mut prefix = b"0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\n".to_vec();
    prefix.extend_from_slice(code_page.as_bytes());
    // The source file is not UTF-8 on this path. Keep that same whole-file decision in the
    // probe: East Asian codecs decode every line; other codecs preserve valid UTF-8 per line.
    prefix.extend_from_slice(b"\n0\nENDSEC\n999\n\xFF\n1000\n");
    prefix
}

/// Decode only a group-code line or the selected value with the existing parser. ASCII and
/// whole-file UTF-8 borrow their text; a legacy field allocates at most its own eight-tag probe.
fn field_text<'a>(line: &'a [u8], utf8: bool, prefix: &[u8]) -> Option<Cow<'a, str>> {
    if utf8 || line.is_ascii() {
        return std::str::from_utf8(line).ok().map(Cow::Borrowed);
    }
    let capacity = prefix.len().checked_add(line.len())?.checked_add(7)?;
    if capacity > MAX_TEXT_PROBE_BYTES {
        return None;
    }
    let mut probe = Vec::new();
    probe.try_reserve_exact(capacity).ok()?;
    probe.extend_from_slice(prefix);
    probe.extend_from_slice(line);
    probe.extend_from_slice(b"\n0\nEOF\n");
    let tag = craft_dxf::parse_with_limit(&probe, 8).ok()?.into_iter().find(|t| t.code == 1000)?;
    match tag.value {
        craft_dxf::Value::Str(text) => Some(Cow::Owned(text)),
        _ => None,
    }
}

/// `text` as a plain decimal integer whose value is 0, 1 or 2. A leading sign and zero padding are
/// valid integer spellings; a decimal point, an exponent, a non-number or an overflow is not. Both
/// ends are trimmed, as the parser trims a numeric group's text.
fn plain_mode(text: &str) -> Option<i64> {
    let n = text.trim().parse::<i64>().ok()?;
    (0..=2).contains(&n).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tags of an ASCII file, as the parser makes them.
    fn tags_of(file: &str) -> Vec<Tag> {
        craft_dxf::parse(file.as_bytes()).unwrap()
    }

    /// A file whose RASTERVARIABLES object holds `frame` as group 70's text in the OBJECTS section.
    fn file(frame: &str) -> String {
        format!(
            "  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n  0\nRASTERVARIABLES\n  5\nA1\n 70\n{frame}\n  0\nENDSEC\n  0\nEOF\n"
        )
    }

    fn mode(file: &str) -> Option<i64> {
        image_frame(file.as_bytes(), &tags_of(file))
    }

    #[test]
    fn valid_modes_are_read_from_the_spelling() {
        for (text, want) in [("0", 0), ("1", 1), ("2", 2), ("2 ", 2), ("     2", 2), ("02", 2), ("0000", 0), ("+2", 2), ("-0", 0)] {
            assert_eq!(mode(&file(text)), Some(want), "{text:?}");
        }
        let padded_codes = file("0").replace("  0\n", "+0\n").replace("  2\n", "002\n").replace(" 70\n", "070\n");
        assert_eq!(mode(&padded_codes), Some(0), "group-code integer spellings");
    }

    #[test]
    fn a_malformed_spelling_is_not_a_mode() {
        // Each of these the typed parser would turn into a valid mode: 0 or 2.
        for text in ["abc", "NaN", "inf", "2.5", "-1", "+", "--0", "3", "99999999999999999999", "1e0"] {
            assert_eq!(mode(&file(text)), None, "{text:?}");
        }
        assert_eq!(mode(&file("")), None, "an empty value");
    }

    #[test]
    fn a_value_outside_the_objects_section_or_record_is_ignored() {
        let outside = "  0\nSECTION\n  2\nENTITIES\n  0\nRASTERVARIABLES\n 70\n0\n  0\nENDSEC\n  0\nEOF\n";
        assert_eq!(mode(outside), None);
        let other = "  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n 70\n0\n  0\nENDSEC\n  0\nEOF\n";
        assert_eq!(mode(other), None);
    }

    #[test]
    fn the_last_record_wins_and_its_first_group_70() {
        let both = "  0\nSECTION\n  2\nOBJECTS\n  0\nRASTERVARIABLES\n 70\n0\n  0\nRASTERVARIABLES\n 70\n2\n  0\nENDSEC\n  0\nEOF\n";
        assert_eq!(mode(both), Some(2));
        let crlf = "  0\r\nSECTION\r\n  2\r\nOBJECTS\r\n  0\r\nRASTERVARIABLES\r\n 70\r\n0\r\n  0\r\nENDSEC\r\n";
        assert_eq!(mode(crlf), Some(0));
        let lone_cr = crlf.replace("\r\n", "\r");
        assert_eq!(mode(&lone_cr), Some(0), "a lone CR ends a line too");
        let twice = "  0\nSECTION\n  2\nOBJECTS\n  0\nRASTERVARIABLES\n 70\n0\n 70\n2\n  0\nENDSEC\n  0\nEOF\n";
        assert_eq!(mode(twice), Some(0), "the record's first group 70");
        let last = "  0\nSECTION\n  2\nOBJECTS\n  0\nRASTERVARIABLES\n 70\n2\n  0\nRASTERVARIABLES\n  5\nA2\n  0\nENDSEC\n  0\nEOF\n";
        assert_eq!(mode(last), None, "a last record without a mode sets none");
    }

    #[test]
    fn section_and_record_boundaries_match_the_tag_reader() {
        let initial = "0\nSECTION\n2\nOBJECTS\n0\nRASTERVARIABLES\n70\n2\n0\nENDSEC\n";
        for suffix in [
            "0\nSECTION\n999\nignored\n2\nOBJECTS\n0\nRASTERVARIABLES\n70\n0\n0\nENDSEC\n",
            "0\nSECTION\n2\nUNKNOWN\n0\nSECTION\n2\nOBJECTS\n0\nRASTERVARIABLES\n70\n0\n0\nENDSEC\n",
            "0\nSECTION\n2\nOBJECTS\n0\nRASTERVARIABLES \n70\n0\n0\nENDSEC\n",
        ] {
            let file = format!("{initial}{suffix}0\nEOF\n");
            assert_eq!(mode(&file), Some(2), "{suffix:?}");
        }
    }

    #[test]
    fn a_record_named_with_an_escape_is_the_record_the_reader_takes() {
        // The parser unescapes a record name before the reader compares it, so `\U+0056` is the
        // `V` of RASTERVARIABLES: the mode is 0, not the default.
        let escaped = file("0").replace("RASTERVARIABLES", "RASTER\\U+0056ARIABLES");
        assert_eq!(mode(&escaped), Some(0));
        // A section name the parser trims of Unicode whitespace is the OBJECTS section.
        let padded = file("0").replace("\nOBJECTS\n", "\nOBJECTS\u{a0}\n");
        assert_eq!(mode(&padded), Some(0));
        // Unicode whitespace around a group code and a value is trimmed, as the parser trims it.
        let spaced = file("0").replace(" 70\n", "\u{a0}70\n");
        assert_eq!(mode(&spaced), Some(0));
    }

    #[test]
    fn a_bom_and_blank_lines_are_skipped_as_the_parser_skips_them() {
        let bom = format!("\u{feff}{}", file("0"));
        assert_eq!(mode(&bom), Some(0));
        let blank = file("0").replace("  5\nA1\n", "\n  5\nA1\n\n");
        assert_eq!(mode(&blank), Some(0));
    }

    #[test]
    fn binary_files_use_the_parsed_value() {
        let mut binary = BINARY_PREFIX.to_vec();
        binary.extend([0, 0, 0, 0]);
        let tags = tags_of(&file("2"));
        // The tag stream is the same one the parser made; only the spelling is unavailable.
        assert_eq!(image_frame(&binary, &tags), Some(2));
        assert_eq!(image_frame(&binary, &tags_of(&file("7"))), None, "not a mode");
        assert_eq!(image_frame(&binary, &[]), None, "no raster variables object");
        // An ASCII file is scanned, and the spelling decides, not the parsed 0.
        assert_eq!(image_frame(file("1").as_bytes(), &tags_of(&file("1"))), Some(1));
        assert_eq!(image_frame(file("abc").as_bytes(), &tags_of(&file("abc"))), None);
    }
}
