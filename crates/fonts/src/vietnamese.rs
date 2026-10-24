//! Legacy Vietnamese encoding transcoding for CAD text.
//!
//! In Vietnamese AEC CAD workflows, legacy drawings frequently use 8-bit encodings:
//! - **TCVN 5712 / TCVN3 (ABC)**: Paired with fonts starting with `.Vn` or `vn` (e.g. `.VnTime`,
//!   `.VnArial`, `.VnTimeH`, `vnsimli.shx`).
//! - **VNI Windows**: Paired with fonts starting with `VNI-` (e.g. `VNI-Helve`, `VNI-Times`).
//!
//! When CAD files store these strings under `$DWGCODEPAGE ANSI_1252`, characters become mojibake
//! unless transcoded to Unicode NFC. This module provides zero-allocation heuristics and linear-time
//! deterministic transcoding with zero third-party dependencies.

use std::borrow::Cow;

/// Detected or specified legacy Vietnamese encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VietnameseEncoding {
    /// TCVN3 (ABC) single-byte lowercase / composite uppercase encoding.
    Tcvn3,
    /// TCVN3H all-uppercase font encoding (where lowercase byte slots display capital letters).
    Tcvn3H,
    /// VNI Windows composite and extended byte encoding.
    Vni,
}

/// Detect whether a font name indicates a legacy Vietnamese encoding.
pub fn is_vietnamese_font(font: &str) -> Option<VietnameseEncoding> {
    let lower = font.trim().to_ascii_lowercase();
    let stem = lower.strip_suffix(".ttf").or_else(|| lower.strip_suffix(".shx")).or_else(|| lower.strip_suffix(".otf")).unwrap_or(&lower);

    if stem.starts_with("vni")
        || stem.starts_with("vni-")
        || stem.starts_with("vni_")
        || stem.starts_with("vhelv")
        || stem.starts_with("vtime")
        || stem.starts_with("vavon")
        || stem.starts_with("vapto")
        || stem.starts_with("vcomm")
        || stem.starts_with("vkorin")
    {
        Some(VietnameseEncoding::Vni)
    } else if stem.starts_with(".vn") || stem.starts_with("vn") {
        if stem.ends_with('h') { Some(VietnameseEncoding::Tcvn3H) } else { Some(VietnameseEncoding::Tcvn3) }
    } else {
        None
    }
}

#[inline]
fn is_latin1_vietnamese_vowel(c: char) -> bool {
    matches!(
        c,
        'À' | 'Á'
            | 'Â'
            | 'Ã'
            | 'È'
            | 'É'
            | 'Ê'
            | 'Ì'
            | 'Í'
            | 'Ò'
            | 'Ó'
            | 'Ô'
            | 'Õ'
            | 'Ù'
            | 'Ú'
            | 'Ý'
            | 'à'
            | 'á'
            | 'â'
            | 'ã'
            | 'è'
            | 'é'
            | 'ê'
            | 'ì'
            | 'í'
            | 'ò'
            | 'ó'
            | 'ô'
            | 'õ'
            | 'ù'
            | 'ú'
            | 'ý'
    )
}

/// Check if a string already contains genuine Unicode Vietnamese diacritic characters
/// that exist outside the single-byte 0x00..=0xFF Windows-1252 range (U+0100..=U+024F, U+1E00..=U+1EFF),
/// or precomposed Latin-1 Vietnamese vowels in positions that cannot be VNI or TCVN3 encodings.
/// If true, the text MUST NOT be transcoded, preserving existing modern Unicode text.
pub fn is_already_unicode_vietnamese(s: &str) -> bool {
    let mut prev: Option<char> = None;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        let code = c as u32;
        if (0x0100..=0x024F).contains(&code) || (0x1E00..=0x1EFF).contains(&code) || matches!(c, 'Đ' | 'đ' | 'Ă' | 'ă' | 'Ơ' | 'ơ' | 'Ư' | 'ư')
        {
            return true;
        }

        if is_latin1_vietnamese_vowel(c) {
            let next_c = chars.peek().copied();

            // Case A: Unicode "TÒA", "TÒA ÁN" ('Ò' before 'A')
            if matches!(c, 'Ò' | 'ò') && matches!(next_c, Some('A' | 'a')) {
                return true;
            }

            // Case B: Unicode diphthong OÁ, OÀ, OÃ followed by a final ASCII consonant (TOÁN, HOÀN, KHOÁC, HOÃN)
            if matches!(prev, Some('O' | 'o'))
                && matches!(c, 'Á' | 'á' | 'À' | 'à' | 'Ã' | 'ã')
                && next_c.is_some_and(|post| post.is_ascii_alphabetic() && !matches!(post.to_ascii_uppercase(), 'A' | 'E' | 'I' | 'O' | 'U' | 'Y'))
            {
                return true;
            }

            // Case C: Standalone Latin-1 vowel not forming a VNI composite with `prev` or `next_c`
            let is_vni_prev = prev.is_some_and(|p| match_vni_composite(p, c).is_some());
            let is_vni_next = next_c.is_some_and(|n| match_vni_composite(c, n).is_some());

            if !is_vni_prev && !is_vni_next {
                if matches!(c, 'Ô' | 'ô' | 'Ò' | 'ò' | 'Ó' | 'ó') {
                    // Protect Unicode words like "TÔI", "CÔNG", "CÔ" when preceded by uppercase or followed by vowel/NG
                    if next_c.is_some_and(|post| matches!(post, 'A' | 'a' | 'I' | 'i' | 'N' | 'n'))
                        || (c.is_uppercase() && prev.is_some_and(|p| p.is_ascii_uppercase()) && next_c.is_none_or(|n| n.is_whitespace()))
                    {
                        return true;
                    }
                } else if matches!(c, 'À' | 'Á' | 'Ã' | 'Ì' | 'Í' | 'Ù' | 'Ú' | 'Ý' | 'à')
                    && prev.is_none_or(|p| !p.is_ascii_lowercase() || c.is_lowercase())
                {
                    return true;
                }
            }
        }

        prev = Some(c);
    }
    false
}

/// Detect if a text string has unambiguous characteristic byte signatures of VNI or TCVN3.
pub fn detect_vietnamese_encoding(s: &str) -> Option<VietnameseEncoding> {
    if s.is_ascii() || is_already_unicode_vietnamese(s) {
        return None;
    }

    let mut vni_score = 0;
    let mut tcvn_score = 0;
    let mut composite_found = false;

    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(&next_c) = chars.peek() {
            let mut matched_composite = false;
            let is_diameter = c.eq_ignore_ascii_case(&'O') && next_c == 'Ø';
            if !is_diameter && match_vni_composite(c, next_c).is_some() {
                vni_score += 2;
                composite_found = true;
                matched_composite = true;
            }
            if match_tcvn3_composite(c, next_c).is_some() {
                tcvn_score += 2;
                composite_found = true;
                matched_composite = true;
            }
            if matched_composite {
                chars.next();
                continue;
            }
        }
        if match_vni_single(c).is_some() && !matches!(c, 'Ô' | 'ô' | 'Ò' | 'ò' | 'Ó' | 'ó') {
            vni_score += 1;
        }
        if match_tcvn3_single(c).is_some() && !matches!(c, 'á' | 'â' | 'ã' | 'è' | 'é' | 'ê' | 'ì' | 'í' | 'ò' | 'ó' | 'ô' | 'õ' | 'ù' | 'ú' | 'ý')
        {
            tcvn_score += 1;
        }
    }

    let threshold = if composite_found { 2 } else { 4 };

    if vni_score >= threshold && vni_score > tcvn_score {
        Some(VietnameseEncoding::Vni)
    } else if tcvn_score >= threshold && tcvn_score > vni_score {
        Some(VietnameseEncoding::Tcvn3)
    } else {
        None
    }
}

/// Transcode a VNI Windows string to Unicode NFC.
pub fn transcode_vni(s: &str) -> Cow<'_, str> {
    if s.is_ascii() || is_already_unicode_vietnamese(s) {
        return Cow::Borrowed(s);
    }

    let mut out = String::with_capacity(s.len());
    let mut changed = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(&next) = chars.peek()
            && let Some(comp) = match_vni_composite(c, next)
        {
            out.push_str(comp);
            chars.next();
            changed = true;
            continue;
        }
        if let Some(single) = match_vni_single(c) {
            out.push(single);
            changed = true;
        } else {
            out.push(c);
        }
    }

    if changed { Cow::Owned(out) } else { Cow::Borrowed(s) }
}

fn is_tcvn3_all_caps(s: &str) -> bool {
    let mut has_upper = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && chars.peek() == Some(&'%') {
            chars.next();
            if let Some(&next) = chars.peek() {
                if next.is_ascii_alphabetic() {
                    chars.next();
                    continue;
                }
                if next.is_ascii_digit() {
                    for _ in 0..3 {
                        if let Some(&d) = chars.peek() {
                            if d.is_ascii_digit() {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    continue;
                }
            }
            continue;
        }
        if c.is_ascii_lowercase() {
            return false;
        }
        if c.is_ascii_uppercase() {
            has_upper = true;
        }
    }
    has_upper
}

/// Transcode a TCVN3 (ABC) standard string to Unicode NFC.
pub fn transcode_tcvn3(s: &str) -> Cow<'_, str> {
    if s.is_ascii() || is_already_unicode_vietnamese(s) {
        return Cow::Borrowed(s);
    }

    let all_caps = is_tcvn3_all_caps(s);
    let mut out = String::with_capacity(s.len());
    let mut changed = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(&next) = chars.peek()
            && let Some(comp) = match_tcvn3_composite(c, next)
        {
            out.push_str(comp);
            chars.next();
            changed = true;
            continue;
        }
        if let Some(single) = match_tcvn3_single(c) {
            if all_caps {
                out.extend(single.to_uppercase());
            } else {
                out.push(single);
            }
            changed = true;
        } else {
            out.push(c);
        }
    }

    if changed { Cow::Owned(out) } else { Cow::Borrowed(s) }
}

/// Transcode a TCVN3H all-uppercase string to Unicode NFC uppercase.
pub fn transcode_tcvn3_h(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        let upper = s.to_uppercase();
        if upper == s { Cow::Borrowed(s) } else { Cow::Owned(upper) }
    } else {
        let transcoded = transcode_tcvn3(s);
        let upper = transcoded.to_uppercase();
        if upper == s { Cow::Borrowed(s) } else { Cow::Owned(upper) }
    }
}

/// Transcode text using an optional font hint, falling back to signature detection.
pub fn transcode_vietnamese<'a>(s: &'a str, font_hint: Option<&str>) -> Cow<'a, str> {
    let encoding = font_hint.and_then(is_vietnamese_font).or_else(|| detect_vietnamese_encoding(s));

    if (s.is_ascii() && encoding != Some(VietnameseEncoding::Tcvn3H)) || is_already_unicode_vietnamese(s) {
        return Cow::Borrowed(s);
    }

    match encoding {
        Some(VietnameseEncoding::Vni) => transcode_vni(s),
        Some(VietnameseEncoding::Tcvn3) => transcode_tcvn3(s),
        Some(VietnameseEncoding::Tcvn3H) => transcode_tcvn3_h(s),
        None => Cow::Borrowed(s),
    }
}

// ---------------------------------------------------------------------------
// VNI Windows lookup tables
// ---------------------------------------------------------------------------

fn match_vni_composite(c1: char, c2: char) -> Option<&'static str> {
    match (c1, c2) {
        ('A', 'Ø') => Some("À"),
        ('A', 'Ù') => Some("Á"),
        ('A', 'Â') => Some("Â"),
        ('A', 'Õ') => Some("Ã"),
        ('A', 'Ï') => Some("Ạ"),
        ('A', 'Û') => Some("Ả"),
        ('A', 'Á') => Some("Ấ"),
        ('A', 'À') => Some("Ầ"),
        ('A', 'Å') => Some("Ẩ"),
        ('A', 'Ã') => Some("Ẫ"),
        ('A', 'Ä') => Some("Ậ"),
        ('A', 'Ê') => Some("Ă"),
        ('A', 'É') => Some("Ắ"),
        ('A', 'È') => Some("Ằ"),
        ('A', 'Ú') => Some("Ẳ"),
        ('A', 'Ü') => Some("Ẵ"),
        ('A', 'Ë') => Some("Ặ"),
        ('E', 'Ø') => Some("È"),
        ('E', 'Ù') => Some("É"),
        ('E', 'Â') => Some("Ê"),
        ('E', 'Õ') => Some("Ẽ"),
        ('E', 'Ï') => Some("Ẹ"),
        ('E', 'Û') => Some("Ẻ"),
        ('E', 'Á') => Some("Ế"),
        ('E', 'À') => Some("Ề"),
        ('E', 'Å') => Some("Ể"),
        ('E', 'Ã') => Some("Ễ"),
        ('E', 'Ä') => Some("Ệ"),
        ('I', 'Ø') => Some("Ì"),
        ('I', 'Ù') => Some("Í"),
        ('I', 'Û') => Some("Ỉ"),
        ('I', 'Õ') => Some("Ĩ"),
        ('I', 'Ï') => Some("Ị"),
        ('O', 'Ø') => Some("Ò"),
        ('O', 'Ù') => Some("Ó"),
        ('O', 'Â') => Some("Ô"),
        ('O', 'Õ') => Some("Õ"),
        ('O', 'Ï') => Some("Ọ"),
        ('O', 'Û') => Some("Ỏ"),
        ('O', 'Á') => Some("Ố"),
        ('O', 'À') => Some("Ồ"),
        ('O', 'Å') => Some("Ổ"),
        ('O', 'Ã') => Some("Ỗ"),
        ('O', 'Ä') => Some("Ộ"),
        ('Ô', 'Ù') => Some("Ớ"),
        ('Ô', 'Ø') => Some("Ờ"),
        ('Ô', 'Û') => Some("Ở"),
        ('Ô', 'Õ') => Some("Ỡ"),
        ('Ô', 'Ï') => Some("Ợ"),
        ('U', 'Ø') => Some("Ù"),
        ('U', 'Ù') => Some("Ú"),
        ('U', 'Õ') => Some("Ũ"),
        ('U', 'Ï') => Some("Ụ"),
        ('U', 'Û') => Some("Ủ"),
        ('Ö', 'Ù') => Some("Ứ"),
        ('Ö', 'Ø') => Some("Ừ"),
        ('Ö', 'Û') => Some("Ử"),
        ('Ö', 'Õ') => Some("Ữ"),
        ('Ö', 'Ï') => Some("Ự"),
        ('Y', 'Ø') => Some("Ỳ"),
        ('Y', 'Ù') => Some("Ý"),
        ('Y', 'Õ') => Some("Ỹ"),
        ('Y', 'Û') => Some("Ỷ"),
        ('Y', 'Ï') => Some("Ỵ"),
        ('a', 'ø') => Some("à"),
        ('a', 'ù') => Some("á"),
        ('a', 'â') => Some("â"),
        ('a', 'õ') => Some("ã"),
        ('a', 'ï') => Some("ạ"),
        ('a', 'û') => Some("ả"),
        ('a', 'á') => Some("ấ"),
        ('a', 'à') => Some("ầ"),
        ('a', 'å') => Some("ẩ"),
        ('a', 'ã') => Some("ẫ"),
        ('a', 'ä') => Some("ậ"),
        ('a', 'ê') => Some("ă"),
        ('a', 'é') => Some("ắ"),
        ('a', 'è') => Some("ằ"),
        ('a', 'ú') => Some("ẳ"),
        ('a', 'ü') => Some("ẵ"),
        ('a', 'ë') => Some("ặ"),
        ('e', 'ø') => Some("è"),
        ('e', 'ù') => Some("é"),
        ('e', 'â') => Some("ê"),
        ('e', 'õ') => Some("ẽ"),
        ('e', 'ï') => Some("ẹ"),
        ('e', 'û') => Some("ẻ"),
        ('e', 'á') => Some("ế"),
        ('e', 'à') => Some("ề"),
        ('e', 'å') => Some("ể"),
        ('e', 'ã') => Some("ễ"),
        ('e', 'ä') => Some("ệ"),
        ('i', 'ø') => Some("ì"),
        ('i', 'ù') => Some("í"),
        ('i', 'û') => Some("ỉ"),
        ('i', 'õ') => Some("ĩ"),
        ('i', 'ï') => Some("ị"),
        ('o', 'ø') => Some("ò"),
        ('o', 'ù') => Some("ó"),
        ('o', 'â') => Some("ô"),
        ('o', 'õ') => Some("õ"),
        ('o', 'ï') => Some("ọ"),
        ('o', 'û') => Some("ỏ"),
        ('o', 'á') => Some("ố"),
        ('o', 'à') => Some("ồ"),
        ('o', 'å') => Some("ổ"),
        ('o', 'ã') => Some("ỗ"),
        ('o', 'ä') => Some("ộ"),
        ('ô', 'ù') => Some("ớ"),
        ('ô', 'ø') => Some("ờ"),
        ('ô', 'û') => Some("ở"),
        ('ô', 'õ') => Some("ỡ"),
        ('ô', 'ï') => Some("ợ"),
        ('u', 'ø') => Some("ù"),
        ('u', 'ù') => Some("ú"),
        ('u', 'õ') => Some("ũ"),
        ('u', 'ï') => Some("ụ"),
        ('u', 'û') => Some("ủ"),
        ('ö', 'ù') => Some("ứ"),
        ('ö', 'ø') => Some("ừ"),
        ('ö', 'û') => Some("ử"),
        ('ö', 'õ') => Some("ữ"),
        ('ö', 'ï') => Some("ự"),
        ('y', 'ø') => Some("ỳ"),
        ('y', 'ù') => Some("ý"),
        ('y', 'õ') => Some("ỹ"),
        ('y', 'û') => Some("ỷ"),
        ('y', 'ï') => Some("ỵ"),
        _ => None,
    }
}

fn match_vni_single(c: char) -> Option<char> {
    match c {
        'Ñ' => Some('Đ'),
        'ñ' => Some('đ'),
        'Ô' => Some('Ơ'),
        'ô' => Some('ơ'),
        'Ö' => Some('Ư'),
        'ö' => Some('ư'),
        'Æ' => Some('Ỉ'),
        'æ' => Some('ỉ'),
        'Ò' => Some('Ị'),
        'ò' => Some('ị'),
        'Î' => Some('Ỵ'),
        'î' => Some('ỵ'),
        'Ó' => Some('Ĩ'),
        'ó' => Some('ĩ'),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// TCVN3 (ABC) lookup tables
// ---------------------------------------------------------------------------

fn match_tcvn3_composite(c1: char, c2: char) -> Option<&'static str> {
    match (c1, c2) {
        ('A', 'µ') => Some("À"),
        ('A', '¸') => Some("Á"),
        ('A', '·') => Some("Ã"),
        ('A', '¹') => Some("Ạ"),
        ('A', '¶') => Some("Ả"),
        ('E', 'Ì') => Some("È"),
        ('E', 'Ð') => Some("É"),
        ('E', 'Ñ') => Some("Ẹ"),
        ('E', 'Î') => Some("Ẻ"),
        ('E', 'Ï') => Some("Ẽ"),
        ('I', '×') => Some("Ì"),
        ('I', 'Ý') => Some("Í"),
        ('I', 'Ü') => Some("Ĩ"),
        ('I', 'Ø') => Some("Ỉ"),
        ('I', 'Þ') => Some("Ị"),
        ('O', 'ß') => Some("Ò"),
        ('O', 'ã') => Some("Ó"),
        ('O', 'â') => Some("Õ"),
        ('O', 'ä') => Some("Ọ"),
        ('O', 'á') => Some("Ỏ"),
        ('U', 'ï') => Some("Ù"),
        ('U', 'ó') => Some("Ú"),
        ('U', 'ò') => Some("Ũ"),
        ('U', 'ô') => Some("Ụ"),
        ('U', 'ñ') => Some("Ủ"),
        ('Y', 'ý') => Some("Ý"),
        ('Y', 'ú') => Some("Ỳ"),
        ('Y', 'þ') => Some("Ỵ"),
        ('Y', 'û') => Some("Ỷ"),
        ('Y', 'ü') => Some("Ỹ"),
        ('¢', 'Ê') => Some("Ấ"),
        ('¢', 'Ç') => Some("Ầ"),
        ('¢', 'È') => Some("Ẩ"),
        ('¢', 'É') => Some("Ẫ"),
        ('¢', 'Ë') => Some("Ậ"),
        ('¡', '¾') => Some("Ắ"),
        ('¡', '»') => Some("Ằ"),
        ('¡', '¼') => Some("Ẳ"),
        ('¡', '½') => Some("Ẵ"),
        ('¡', 'Æ') => Some("Ặ"),
        ('£', 'Õ') => Some("Ế"),
        ('£', 'Ò') => Some("Ề"),
        ('£', 'Ó') => Some("Ể"),
        ('£', 'Ô') => Some("Ễ"),
        ('£', 'Ö') => Some("Ệ"),
        ('¤', 'è') => Some("Ố"),
        ('¤', 'å') => Some("Ồ"),
        ('¤', 'æ') => Some("Ổ"),
        ('¤', 'ç') => Some("Ỗ"),
        ('¤', 'é') => Some("Ộ"),
        ('¥', 'í') => Some("Ớ"),
        ('¥', 'ê') => Some("Ờ"),
        ('¥', 'ë') => Some("Ở"),
        ('¥', 'ì') => Some("Ỡ"),
        ('¥', 'î') => Some("Ợ"),
        ('¦', 'ø') => Some("Ứ"),
        ('¦', 'õ') => Some("Ừ"),
        ('¦', 'ö') => Some("Ử"),
        ('¦', '÷') => Some("Ữ"),
        ('¦', 'ù') => Some("Ự"),
        _ => None,
    }
}

fn match_tcvn3_single(c: char) -> Option<char> {
    match c {
        '¢' => Some('Â'),
        '£' => Some('Ê'),
        '¤' => Some('Ô'),
        '¥' => Some('Ơ'),
        '¦' => Some('Ư'),
        '§' => Some('Đ'),
        '¡' => Some('Ă'),
        'µ' => Some('à'),
        '¸' => Some('á'),
        '¶' => Some('ả'),
        '·' => Some('ã'),
        '¹' => Some('ạ'),
        '¨' => Some('ă'),
        '»' => Some('ằ'),
        '¾' => Some('ắ'),
        '¼' => Some('ẳ'),
        '½' => Some('ẵ'),
        'Æ' => Some('ặ'),
        '©' => Some('â'),
        'Ç' => Some('ầ'),
        'Ê' => Some('ấ'),
        'È' => Some('ẩ'),
        'É' => Some('ẫ'),
        'Ë' => Some('ậ'),
        '®' => Some('đ'),
        'Ì' => Some('è'),
        'Ð' => Some('é'),
        'Î' => Some('ẻ'),
        'Ï' => Some('ẽ'),
        'Ñ' => Some('ẹ'),
        'ª' => Some('ê'),
        'Ò' => Some('ề'),
        'Õ' => Some('ế'),
        'Ó' => Some('ể'),
        'Ô' => Some('ễ'),
        'Ö' => Some('ệ'),
        '×' => Some('ì'),
        'Ý' => Some('í'),
        'Ø' => Some('ỉ'),
        'Ü' => Some('ĩ'),
        'Þ' => Some('ị'),
        'ß' => Some('ò'),
        'ã' => Some('ó'),
        'á' => Some('ỏ'),
        'â' => Some('õ'),
        'ä' => Some('ọ'),
        '«' => Some('ô'),
        'å' => Some('ồ'),
        'è' => Some('ố'),
        'æ' => Some('ổ'),
        'ç' => Some('ỗ'),
        'é' => Some('ộ'),
        '¬' => Some('ơ'),
        'ê' => Some('ờ'),
        'í' => Some('ớ'),
        'ë' => Some('ở'),
        'ì' => Some('ỡ'),
        'î' => Some('ợ'),
        'ï' => Some('ù'),
        'ó' => Some('ú'),
        'ñ' => Some('ủ'),
        'ò' => Some('ũ'),
        'ô' => Some('ụ'),
        '­' => Some('ư'),
        'õ' => Some('ừ'),
        'ø' => Some('ứ'),
        'ö' => Some('ử'),
        '÷' => Some('ữ'),
        'ù' => Some('ự'),
        'ú' => Some('ỳ'),
        'ý' => Some('ý'),
        'û' => Some('ỷ'),
        'ü' => Some('ỹ'),
        'þ' => Some('ỵ'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vni_nhom_day() {
        assert_eq!(transcode_vni("NHOÂM 3003 DAØY 2MM"), "NHÔM 3003 DÀY 2MM");
    }

    #[test]
    fn test_vni_tong_khoi_luong() {
        assert_eq!(transcode_vni("TOÅNG KHOÁI LÖÔÏNG"), "TỔNG KHỐI LƯỢNG");
    }

    #[test]
    fn test_vni_quy_cach() {
        assert_eq!(transcode_vni("QUY CAÙCH"), "QUY CÁCH");
    }

    #[test]
    fn test_vni_dang_hinh_dung() {
        assert_eq!(transcode_vni("DAÙNG"), "DÁNG");
        assert_eq!(transcode_vni("ÑÖÙNG"), "ĐỨNG");
        assert_eq!(transcode_vni("HIØNH ÑÖÙNG"), "HÌNH ĐỨNG");
    }

    #[test]
    fn test_vni_composites() {
        assert_eq!(transcode_vni("OÂ"), "Ô");
        assert_eq!(transcode_vni("AÙ"), "Á");
        assert_eq!(transcode_vni("OÅ"), "Ổ");
        assert_eq!(transcode_vni("OÃ"), "Ỗ");
        assert_eq!(transcode_vni("EÂ"), "Ê");
        assert_eq!(transcode_vni("ÖÔÏ"), "ƯỢ");
        assert_eq!(transcode_vni("oâ"), "ô");
        assert_eq!(transcode_vni("aù"), "á");
        assert_eq!(transcode_vni("öôï"), "ượ");
    }

    #[test]
    fn test_already_unicode_never_transcoded() {
        for unicode in ["HÌNH CHIẾU ĐỨNG", "HÌNH DÁNG", "TÒA NHÀ", "CHÚNG TÔI", "BÁO CÁO"] {
            assert!(is_already_unicode_vietnamese(unicode), "failed on {unicode}");
            assert_eq!(transcode_vietnamese(unicode, Some("VNI-Helve")), unicode);
            assert_eq!(transcode_vietnamese(unicode, Some(".VnTime")), unicode);
            assert_eq!(transcode_vietnamese(unicode, None), unicode);
        }
    }

    #[test]
    fn test_false_positives_never_transcoded() {
        for word in ["Hòa Bình", "Chúng tôi", "Báo cáo công tác quý II", "Bê tông mác 200, cát vàng", "Piñón", "Café"] {
            assert_eq!(detect_vietnamese_encoding(word), None, "false positive on {word}");
            assert_eq!(transcode_vietnamese(word, None), word);
            assert_eq!(transcode_vietnamese(word, Some("Standard")), word);
        }
    }

    #[test]
    fn test_diameter_symbols_never_transcoded() {
        for sym in ["Ø20", "OØ20", "oØ20", "2OØ25"] {
            assert_eq!(detect_vietnamese_encoding(sym), None, "diameter false positive on {sym}");
            assert_eq!(transcode_vietnamese(sym, None), sym);
            assert_eq!(transcode_vietnamese(sym, Some("Standard")), sym);
        }
    }

    #[test]
    fn test_tcvn3h_ascii_and_legacy() {
        assert_eq!(transcode_vietnamese("ban ve chi tiet", Some(".VnTimeH")), "BAN VE CHI TIET");
        assert_eq!(transcode_vietnamese("B¶n vÏ chi tiÕt", Some(".VnTimeH")), "BẢN VẼ CHI TIẾT");
        match transcode_vietnamese("BAN VE", Some(".VnTimeH")) {
            Cow::Borrowed(b) => assert_eq!(b, "BAN VE"),
            Cow::Owned(_) => panic!("expected borrowed string for already-uppercase ASCII"),
        }
    }

    #[test]
    fn test_tcvn3_all_caps() {
        assert_eq!(transcode_tcvn3("MÆT B»NG KÕT CÊU"), "MẶT BẰNG KẾT CẤU");
        assert_eq!(transcode_tcvn3("B¶N VÏ CHI TIÕT"), "BẢN VẼ CHI TIẾT");
    }

    #[test]
    fn test_hinh_dang_unicode() {
        assert_eq!(transcode_vietnamese("HÌNH DÁNG", None), "HÌNH DÁNG");
    }

    #[test]
    fn test_generic_font_transcodes_when_vietnamese_detected() {
        assert_eq!(transcode_vietnamese("THOÂNG SOÁ", Some("Standard")), "THÔNG SỐ");
        assert_eq!(transcode_vietnamese("NHOÂM 3003 DAØY 2MM", Some("simplex.shx")), "NHÔM 3003 DÀY 2MM");
        assert_eq!(transcode_vietnamese("TOÅNG KHOÁI LÖÔÏNG", Some("Arial")), "TỔNG KHỐI LƯỢNG");
        assert_eq!(transcode_vietnamese("QUY CAÙCH", None), "QUY CÁCH");
    }

    #[test]
    fn test_tcvn3_ban_ve_chi_tiet() {
        assert_eq!(transcode_tcvn3("B¶n vÏ chi tiÕt"), "Bản vẽ chi tiết");
    }

    #[test]
    fn test_tcvn3_mat_bang_ket_cau() {
        assert_eq!(transcode_tcvn3("MÆt b»ng kÕt cÊu"), "Mặt bằng kết cấu");
    }

    #[test]
    fn test_tcvn3_cad_controls() {
        assert_eq!(transcode_tcvn3("%%c20 thÐp gèi"), "%%c20 thép gối");
        assert_eq!(transcode_tcvn3("%%c20 B¶N VÏ"), "%%c20 BẢN VẼ");
        assert_eq!(transcode_tcvn3("%%d45 GãC NGHI£NG"), "%%d45 GÓC NGHIÊNG");
    }

    #[test]
    fn test_is_vietnamese_font() {
        assert_eq!(is_vietnamese_font(".VnTime"), Some(VietnameseEncoding::Tcvn3));
        assert_eq!(is_vietnamese_font(".VnArialH"), Some(VietnameseEncoding::Tcvn3H));
        assert_eq!(is_vietnamese_font("vnsimli.shx"), Some(VietnameseEncoding::Tcvn3));
        assert_eq!(is_vietnamese_font("VNI-Helve"), Some(VietnameseEncoding::Vni));
        assert_eq!(is_vietnamese_font("VNI-Times.ttf"), Some(VietnameseEncoding::Vni));
        assert_eq!(is_vietnamese_font("arial.ttf"), None);
    }

    #[test]
    fn test_transcode_vietnamese_with_font_hint() {
        assert_eq!(transcode_vietnamese("NHOÂM 3003 DAØY 2MM", Some("VNI-Helve")), "NHÔM 3003 DÀY 2MM");
        assert_eq!(transcode_vietnamese("B¶n vÏ chi tiÕt", Some(".VnTime")), "Bản vẽ chi tiết");
        let ascii = "SIMPLE TEXT 123";
        match transcode_vietnamese(ascii, Some(".VnTime")) {
            Cow::Borrowed(b) => assert_eq!(b, ascii),
            Cow::Owned(_) => panic!("expected borrowed string for ascii"),
        }
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    #[test]
    fn test_unicode_latin1_standalone_never_corrupted() {
        let words = ["TÒA NHÀ", "TÒA ÁN", "TÔI ĐI", "CÔ GÁI", "BÁO CÁO", "CHÚ Ý", "LÝ DO", "ĐỒNG HỒ", "GIA ĐÌNH", "TÓM TẮT"];
        for w in words {
            assert_eq!(transcode_vietnamese(w, Some("VNI-Helve")), w, "Failed on {w}");
            assert_eq!(transcode_vietnamese(w, None), w, "Failed on {w}");
        }
    }

    #[test]
    fn test_unicode_diphthongs_never_corrupted() {
        let words = ["DỰ TOÁN", "THANH TOÁN", "HOÀN CÔNG", "BẢN VẼ HOÀN CÔNG", "AN TOÀN LAO ĐỘNG", "ÁO KHOÁC", "THOÁT HIỂM", "HOÀ BÌNH", "TRÌ HOÃN"];
        for w in words {
            assert_eq!(transcode_vietnamese(w, Some("VNI-Helve")), w, "Failed on {w}");
            assert_eq!(transcode_vietnamese(w, None), w, "Failed on {w}");
        }
    }

    #[test]
    fn test_genuine_vni_still_transcodes() {
        let tests = [
            ("SOÁ LIEÄU", "SỐ LIỆU"),
            ("PHOÁ COÅ", "PHỐ CỔ"),
            ("MOÃI NGAØY", "MỖI NGÀY"),
            ("NHOÂM 6063", "NHÔM 6063"),
            ("TOÅNG COÄNG", "TỔNG CỘNG"),
            ("QUY CAÙCH", "QUY CÁCH"),
        ];
        for (input, expected) in tests {
            assert_eq!(transcode_vietnamese(input, Some("VNI-Helve")), expected, "Failed on {input}");
            assert_eq!(transcode_vietnamese(input, None), expected, "Failed on {input}");
        }
    }

    #[test]
    fn test_technical_and_foreign_strings() {
        let safe = ["Ø20", "OØ20", "Café", "Piñón", "Château", "%%c25"];
        for w in safe {
            assert_eq!(transcode_vietnamese(w, None), w, "Failed on {w}");
            assert_eq!(detect_vietnamese_encoding(w), None, "Failed on {w}");
        }
    }
}
