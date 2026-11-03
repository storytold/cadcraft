//! Text decoding for DXF files.
//!
//! R2007 (AC1021) and later DXF files are UTF-8. Older ones store text in the code page named by
//! the `$DWGCODEPAGE` header variable (`ANSI_1252`, `ANSI_932`, `DOS850`, …); characters outside
//! it are written as `\U+XXXX` (any character) or `\M+nXXXX` (a double-byte character of an East
//! Asian code page) escapes, which [`crate::parse`] decodes after the bytes.
//!
//! The Windows, ISO and East Asian code pages are decoded with `encoding_rs`; DOS 437, 850 and
//! 852 with the tables below (the upper halves of the IBM PC code pages; the lower halves are
//! ASCII). Decoding never fails: a byte sequence that is invalid in the code page (a truncated
//! double-byte character, an unassigned byte) becomes U+FFFD and the bytes after it decode
//! normally, so line ends and the group structure survive.

use std::borrow::Cow;

use encoding_rs::Encoding;

/// How the bytes of a code page become text.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Decoder {
    Utf8,
    Encoding(&'static Encoding),
    /// Single-byte DOS code page: ASCII, then these 128 characters for bytes 0x80–0xFF.
    Table(&'static [char; 128]),
}

/// The decoder for a `$DWGCODEPAGE` value (case-insensitive), or `None` when CADCraft doesn't
/// know the code page.
pub(crate) fn decoder(code_page: &str) -> Option<Decoder> {
    use encoding_rs as e;
    let name = code_page.trim().to_ascii_uppercase().replace(['-', ' '], "_");
    let enc = match name.as_str() {
        "UTF8" | "UTF_8" | "UNICODE" => return Some(Decoder::Utf8),
        "ASCII" | "ANSI_1252" | "ISO8859_1" | "ISO_8859_1" => e::WINDOWS_1252,
        "ANSI_874" => e::WINDOWS_874,
        "ANSI_932" | "DOS932" | "SHIFT_JIS" => e::SHIFT_JIS,
        "ANSI_936" | "GB2312" | "GBK" => e::GBK,
        "ANSI_949" | "KSC5601" | "KOREAN" => e::EUC_KR,
        "ANSI_950" | "BIG5" => e::BIG5,
        "ANSI_1250" => e::WINDOWS_1250,
        "ANSI_1251" => e::WINDOWS_1251,
        "ANSI_1253" => e::WINDOWS_1253,
        "ANSI_1254" | "ISO8859_9" | "ISO_8859_9" => e::WINDOWS_1254,
        "ANSI_1255" => e::WINDOWS_1255,
        "ANSI_1256" => e::WINDOWS_1256,
        "ANSI_1257" => e::WINDOWS_1257,
        "ANSI_1258" => e::WINDOWS_1258,
        "ISO8859_2" | "ISO_8859_2" => e::ISO_8859_2,
        "ISO8859_3" | "ISO_8859_3" => e::ISO_8859_3,
        "ISO8859_4" | "ISO_8859_4" => e::ISO_8859_4,
        "ISO8859_5" | "ISO_8859_5" => e::ISO_8859_5,
        "ISO8859_6" | "ISO_8859_6" => e::ISO_8859_6,
        "ISO8859_7" | "ISO_8859_7" => e::ISO_8859_7,
        "ISO8859_8" | "ISO_8859_8" => e::ISO_8859_8,
        "DOS866" => e::IBM866,
        "MACINTOSH" | "MAC_ROMAN" => e::MACINTOSH,
        "DOS437" => return Some(Decoder::Table(&DOS437)),
        "DOS850" => return Some(Decoder::Table(&DOS850)),
        "DOS852" => return Some(Decoder::Table(&DOS852)),
        _ => return None,
    };
    Some(Decoder::Encoding(enc))
}

/// Decode `bytes` from the code page `code_page` names (a `$DWGCODEPAGE` value such as
/// `ANSI_1251`), or `None` when CADCraft doesn't know that code page. Invalid sequences become
/// U+FFFD; escapes (`\U+XXXX`, `\M+nXXXX`) are left as they are.
pub fn decode_code_page(bytes: &[u8], code_page: &str) -> Option<String> {
    decoder(code_page).map(|d| decode(bytes, d).into_owned())
}

/// The code page a pre-2007 file's text is written in, or `None` for pure ASCII with escapes:
/// when CADCraft doesn't know `code_page`, and for `ASCII` (the reader takes such files as
/// Windows-1252, but a writer must keep them ASCII) and `UTF8`/`UNICODE`, which no pre-2007
/// reader expects.
pub(crate) fn encoder(code_page: &str) -> Option<Decoder> {
    if code_page.trim().eq_ignore_ascii_case("ASCII") {
        return None;
    }
    decoder(code_page).filter(|d| !matches!(d, Decoder::Utf8))
}

/// Append the bytes of the non-ASCII character `c` in the code page `d`, and return true, when
/// the code page has it; otherwise append nothing and return false (the caller escapes it).
pub(crate) fn encode_char(c: char, d: Decoder, out: &mut Vec<u8>) -> bool {
    let mut buf = [0; 4];
    match d {
        // Not a pre-2007 code page ([`encoder`] never gives it): escaped.
        Decoder::Utf8 => return false,
        Decoder::Encoding(e) => {
            let (bytes, _, unmappable) = e.encode(c.encode_utf8(&mut buf));
            // A code page byte that is ASCII would read back as another character, and a NUL,
            // CR or LF would break the group, so only all-high-bit sequences or proper double-byte
            // characters are kept (their trail byte may be ASCII, as in Shift-JIS).
            if unmappable || bytes.is_empty() || !bytes.first().is_some_and(|b| *b >= 0x80) || bytes.iter().any(|b| matches!(b, 0 | b'\r' | b'\n')) {
                return false;
            }
            out.extend_from_slice(&bytes);
        }
        Decoder::Table(t) => {
            let Some(i) = t.iter().position(|&x| x == c).and_then(|i| u8::try_from(i).ok()) else { return false };
            out.push(0x80 | i);
        }
    }
    true
}

/// Decode bytes with a decoder. UTF-8 that isn't valid falls back to Windows-1252.
pub(crate) fn decode(bytes: &[u8], d: Decoder) -> Cow<'_, str> {
    match d {
        Decoder::Utf8 => match std::str::from_utf8(bytes) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0,
        },
        Decoder::Encoding(e) => e.decode_without_bom_handling(bytes).0,
        Decoder::Table(t) => match std::str::from_utf8(bytes) {
            Ok(s) if s.is_ascii() => Cow::Borrowed(s),
            _ => Cow::Owned(
                bytes
                    .iter()
                    .map(|&b| if b.is_ascii() { char::from(b) } else { t.get(usize::from(b & 0x7F)).copied().unwrap_or('\u{FFFD}') })
                    .collect(),
            ),
        },
    }
}

/// The `$DWGCODEPAGE` value of an ASCII DXF file: the line two lines after a `$DWGCODEPAGE`
/// line (the group code `3` between them), looked for in the HEADER section only (the scan stops
/// at the first `ENDSEC`). Works on the raw bytes, before they are decoded.
fn ascii_code_page(bytes: &[u8]) -> Option<&[u8]> {
    let mut lines = bytes.split(|&b| b == b'\n').map(<[u8]>::trim_ascii);
    while let Some(line) = lines.next() {
        if line == b"ENDSEC" {
            return None;
        }
        if line.eq_ignore_ascii_case(b"$DWGCODEPAGE") {
            return match (lines.next(), lines.next()) {
                (Some(b"3"), Some(value)) => Some(value),
                _ => None,
            };
        }
    }
    None
}

/// Decode a whole ASCII DXF file. Valid UTF-8 is read as UTF-8: R2007+ files, pure-ASCII files,
/// and older files from writers that ignored the code page. Anything else is decoded from the
/// file's `$DWGCODEPAGE`, or from Windows-1252 (the usual `ANSI_1252`) when the header has none
/// or names a code page CADCraft doesn't know, which is logged as a warning.
pub(crate) fn decode_ascii_file(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(s);
    }
    let d = match ascii_code_page(bytes) {
        Some(name) => {
            let name = String::from_utf8_lossy(name);
            decoder(&name).unwrap_or_else(|| {
                log::warn!("DXF: unknown code page `{name}`; reading the text as ANSI_1252");
                Decoder::Utf8
            })
        }
        None => Decoder::Utf8,
    };
    decode(bytes, d)
}

/// The text state of a binary DXF file, learnt from its header as the tags go by: `$ACADVER`
/// (R2007+ strings are UTF-8) and `$DWGCODEPAGE` (the code page of older files). Both come
/// first in the HEADER section and are ASCII, so every later string is decoded right.
#[derive(Default)]
pub(crate) struct BinaryText {
    /// The header variable whose value comes next (after a group 9).
    var: Option<HeaderVar>,
    utf8: bool,
    code_page: Option<Decoder>,
}

#[derive(Clone, Copy, PartialEq)]
enum HeaderVar {
    AcadVer,
    CodePage,
}

impl BinaryText {
    /// Decode one string value of group `code` and update the state from it. Strings of R2007+
    /// files, and of files without a code page, are UTF-8 when valid (else Windows-1252); older
    /// files' strings are in their code page.
    pub(crate) fn string(&mut self, code: i32, raw: &[u8]) -> String {
        let d = if self.utf8 { Decoder::Utf8 } else { self.code_page.unwrap_or(Decoder::Utf8) };
        let s = decode(raw, d).into_owned();
        match (code, self.var.take()) {
            (9, _) => {
                self.var = match s.trim() {
                    "$ACADVER" => Some(HeaderVar::AcadVer),
                    "$DWGCODEPAGE" => Some(HeaderVar::CodePage),
                    _ => None,
                }
            }
            (1, Some(HeaderVar::AcadVer)) => self.utf8 = is_utf8_version(&s),
            (3, Some(HeaderVar::CodePage)) => {
                self.code_page = decoder(&s);
                if self.code_page.is_none() {
                    log::warn!("DXF: unknown code page `{}`; reading the text as ANSI_1252", s.trim());
                }
            }
            _ => {}
        }
        s
    }
}

/// True for an `$ACADVER` of R2007 (AC1021) or later, whose text is UTF-8.
fn is_utf8_version(acadver: &str) -> bool {
    acadver.trim().strip_prefix("AC").and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n >= 1021)
}

/// The double-byte code page of a `\M+n` escape: 1 Japanese (Shift-JIS), 2 Traditional Chinese
/// (Big5), 3 Korean (Wansung), 5 Simplified Chinese (GB 2312). 4 (Korean Johab) is not
/// supported, so its escapes stay literal.
pub(crate) fn mif_encoding(n: char) -> Option<&'static Encoding> {
    match n {
        '1' => Some(encoding_rs::SHIFT_JIS),
        '2' => Some(encoding_rs::BIG5),
        '3' => Some(encoding_rs::EUC_KR),
        '5' => Some(encoding_rs::GBK),
        _ => None,
    }
}

// Generated from the IBM PC code page definitions (bytes 0x80–0xFF in order).
const DOS437: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}', '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}',
    '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}', '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}', '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}',
    '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}', '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}', '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}',
    '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}', '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}',
    '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}', '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}', '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}',
    '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}', '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];
const DOS850: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}', '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}',
    '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}', '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00F8}', '\u{00A3}', '\u{00D8}', '\u{00D7}', '\u{0192}', '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}',
    '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}', '\u{00BF}', '\u{00AE}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{00C1}', '\u{00C2}', '\u{00C0}', '\u{00A9}', '\u{2563}', '\u{2551}', '\u{2557}',
    '\u{255D}', '\u{00A2}', '\u{00A5}', '\u{2510}', '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{00E3}', '\u{00C3}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{00A4}', '\u{00F0}', '\u{00D0}', '\u{00CA}', '\u{00CB}',
    '\u{00C8}', '\u{0131}', '\u{00CD}', '\u{00CE}', '\u{00CF}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{00A6}', '\u{00CC}', '\u{2580}',
    '\u{00D3}', '\u{00DF}', '\u{00D4}', '\u{00D2}', '\u{00F5}', '\u{00D5}', '\u{00B5}', '\u{00FE}', '\u{00DE}', '\u{00DA}', '\u{00DB}', '\u{00D9}',
    '\u{00FD}', '\u{00DD}', '\u{00AF}', '\u{00B4}', '\u{00AD}', '\u{00B1}', '\u{2017}', '\u{00BE}', '\u{00B6}', '\u{00A7}', '\u{00F7}', '\u{00B8}',
    '\u{00B0}', '\u{00A8}', '\u{00B7}', '\u{00B9}', '\u{00B3}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];
const DOS852: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{016F}', '\u{0107}', '\u{00E7}', '\u{0142}', '\u{00EB}', '\u{0150}', '\u{0151}',
    '\u{00EE}', '\u{0179}', '\u{00C4}', '\u{0106}', '\u{00C9}', '\u{0139}', '\u{013A}', '\u{00F4}', '\u{00F6}', '\u{013D}', '\u{013E}', '\u{015A}',
    '\u{015B}', '\u{00D6}', '\u{00DC}', '\u{0164}', '\u{0165}', '\u{0141}', '\u{00D7}', '\u{010D}', '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}',
    '\u{0104}', '\u{0105}', '\u{017D}', '\u{017E}', '\u{0118}', '\u{0119}', '\u{00AC}', '\u{017A}', '\u{010C}', '\u{015F}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{00C1}', '\u{00C2}', '\u{011A}', '\u{015E}', '\u{2563}', '\u{2551}', '\u{2557}',
    '\u{255D}', '\u{017B}', '\u{017C}', '\u{2510}', '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{0102}', '\u{0103}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{00A4}', '\u{0111}', '\u{0110}', '\u{010E}', '\u{00CB}',
    '\u{010F}', '\u{0147}', '\u{00CD}', '\u{00CE}', '\u{011B}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{0162}', '\u{016E}', '\u{2580}',
    '\u{00D3}', '\u{00DF}', '\u{00D4}', '\u{0143}', '\u{0144}', '\u{0148}', '\u{0160}', '\u{0161}', '\u{0154}', '\u{00DA}', '\u{0155}', '\u{0170}',
    '\u{00FD}', '\u{00DD}', '\u{0163}', '\u{00B4}', '\u{00AD}', '\u{02DD}', '\u{02DB}', '\u{02C7}', '\u{02D8}', '\u{00A7}', '\u{00F7}', '\u{00B8}',
    '\u{00B0}', '\u{00A8}', '\u{02D9}', '\u{0171}', '\u{0158}', '\u{0159}', '\u{25A0}', '\u{00A0}',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_page_names() {
        let d = |bytes: &[u8], cp: &str| decode_code_page(bytes, cp);
        assert_eq!(d(b"\xCF\xF0\xE8\xE2\xE5\xF2", "ANSI_1251").as_deref(), Some("Привет"));
        assert_eq!(d(b"\xCF\xF0\xE8\xE2\xE5\xF2", " ansi_1251 ").as_deref(), Some("Привет"));
        assert_eq!(d(b"\x90\x7D\x96\xCA", "ANSI_932").as_deref(), Some("図面"));
        assert_eq!(d(b"\xCD\xBC\xB2\xE3", "GB2312").as_deref(), Some("图层"));
        assert_eq!(d(b"\xB9\xCF\xBC\x68", "BIG5").as_deref(), Some("圖層"));
        assert_eq!(d(b"\xB5\xB5\xB8\xE9", "ANSI_949").as_deref(), Some("도면"));
        assert_eq!(d(b"\xD3\xF7\xDD\xE4\xE9\xEF", "ANSI_1253").as_deref(), Some("Σχέδιο"));
        assert_eq!(d(b"\xC0\xD2\xBE", "ANSI_874").as_deref(), Some("ภาพ"));
        assert_eq!(d(b"\x80", "ANSI_1252").as_deref(), Some("€"));
        // DOS 437 and 850 agree on letters but differ on 0x9B (¢ / ø).
        assert_eq!(d(b"Gr\x94\xE1e \x9B", "DOS437").as_deref(), Some("Größe ¢"));
        assert_eq!(d(b"Gr\x94\xE1e \x9B", "dos850").as_deref(), Some("Größe ø"));
        assert_eq!(d(b"\xBD\xA2\x88w", "DOS852").as_deref(), Some("Żółw"));
        assert_eq!(d(b"\x91\xAB\xAE\xA9", "DOS866").as_deref(), Some("Слой"));
        assert_eq!(d("é".as_bytes(), "UTF8").as_deref(), Some("é"));
        assert_eq!(d(b"x", "ANSI_1361"), None);
        assert_eq!(d(b"x", "EBCDIC"), None);
        assert_eq!(d(b"x", ""), None);
    }

    #[test]
    fn invalid_sequences_become_replacement_characters() {
        // A Shift-JIS lead byte cut off by the line end keeps the line end.
        assert_eq!(decode_code_page(b"\x90\n1\n", "ANSI_932").as_deref(), Some("\u{FFFD}\n1\n"));
        assert_eq!(decode_code_page(b"\x90", "ANSI_932").as_deref(), Some("\u{FFFD}"));
        assert_eq!(decode_code_page(b"\xFF\xFF", "ANSI_936").as_deref(), Some("\u{FFFD}\u{FFFD}"));
    }

    #[test]
    fn header_scan() {
        let head = b"0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$DWGCODEPAGE\n3\nANSI_1251\r\n0\nENDSEC\n";
        assert_eq!(ascii_code_page(head), Some(&b"ANSI_1251"[..]));
        // Only the HEADER section counts.
        assert_eq!(ascii_code_page(b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n9\n$DWGCODEPAGE\n3\nANSI_1251\n"), None);
        // Wrong group code, or the file ends early.
        assert_eq!(ascii_code_page(b"9\n$DWGCODEPAGE\n1\nANSI_1251\n"), None);
        assert_eq!(ascii_code_page(b"9\n$DWGCODEPAGE\n3"), None);
        assert!(is_utf8_version("AC1021") && is_utf8_version(" AC1032 "));
        assert!(!is_utf8_version("AC1018") && !is_utf8_version("AC1009") && !is_utf8_version("garbage"));
    }

    #[test]
    fn mif_escapes() {
        let one = |s: &str| crate::parse(format!("1\n{s}\n0\nEOF\n").as_bytes()).unwrap()[0].str();
        assert_eq!(one("\\M+5BCFE\\M+1907D\\M+2B9CF\\M+3B5B5!"), "件図圖도!");
        assert_eq!(one("\\M+5BCFE\\U+0021"), "件!");
        // Johab, unknown code pages, bad hex, short escapes and unassigned codes stay literal.
        for s in ["\\M+4ABCD", "\\M+9BCFE", "\\M+5GGGG", "\\M+5BC", "\\M+", "\\M+50041", "\\M+5FFFF"] {
            assert_eq!(one(s), s);
        }
    }
}
