//! Decoding of the 8-bit text of R13-R2004 files.

/// Windows-1252 characters for the bytes 0x80-0x9F (the rest of the upper half matches Latin-1);
/// the unassigned positions map to the control character of the same value.
const CP1252_HIGH: [u32; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018,
    0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

/// Decode an 8-bit DWG string up to its first NUL. Every code page is read as Windows-1252, and the
/// `\U+XXXX` escapes AutoCAD writes for characters outside the code page are expanded.
pub(crate) fn decode_ansi(bytes: &[u8], _codepage: u16) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes.iter().take_while(|&&b| b != 0) {
        let c = match b {
            0x80..=0x9F => CP1252_HIGH.get(usize::from(b - 0x80)).copied().unwrap_or(u32::from(b)),
            _ => u32::from(b),
        };
        s.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
    }
    expand_unicode_escapes(&s)
}

/// Replace `\U+XXXX` with the character it names; an escape that names no character stays as it is.
fn expand_unicode_escapes(s: &str) -> String {
    if !s.contains("\\U+") && !s.contains("\\u+") {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '\\'
            && matches!(chars.get(i + 1), Some('U' | 'u'))
            && chars.get(i + 2) == Some(&'+')
            && let Some(hex) = chars.get(i + 3..i + 7)
            && let Ok(v) = u32::from_str_radix(&hex.iter().collect::<String>(), 16)
            && let Some(ch) = char::from_u32(v)
        {
            out.push(ch);
            i += 7;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}
