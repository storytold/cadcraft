//! Right-to-left text at the code-point level, independent of any font:
//! - the Unicode Bidirectional Algorithm (`unicode-bidi`) for one line: visual runs, per-character
//!   embedding levels, mirrored brackets;
//! - Arabic-script joining (Arabic, Persian, Urdu): each letter's contextual form (isolated,
//!   initial, medial, final), with ZWJ / tatweel joining and ZWNJ breaking, and the lam-alef
//!   ligatures, mapped to Unicode presentation forms. Fonts with OpenType Arabic features are
//!   shaped by `harfrust` instead (the `complex` module); the presentation forms serve fonts
//!   without them (older fonts that only map the presentation-form code points).

use std::ops::Range;

use unicode_bidi::{BidiClass, Level, ParagraphBidiInfo, bidi_class};

/// Whether `c` is a strong right-to-left character or an explicit right-to-left control.
pub(crate) fn is_rtl(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI)
}

/// Whether a line needs the right-to-left path (bidi reordering and shaping).
pub fn has_rtl(s: &str) -> bool {
    s.chars().any(is_rtl)
}

/// Whether a line needs OpenType shaping (`complex`) rather than one glyph per character: any
/// right-to-left text, or Thai, whose vowel and tone marks stack above and below the consonant
/// and need the font's mark positioning (GPOS) to sit apart instead of overprinting.
pub fn needs_shaping(s: &str) -> bool {
    s.chars().any(|c| is_rtl(c) || matches!(c, '\u{0E00}'..='\u{0E7F}'))
}

/// Invisible formatting characters: joiners, directional marks, embeddings and isolates. They
/// take no space and draw nothing (a shaper still sees the joiners).
pub(crate) fn is_format_control(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{061C}')
}

/// A combining mark (drawn on the preceding character).
pub(crate) fn is_mark(c: char) -> bool {
    bidi_class(c) == BidiClass::NSM
}

/// Base direction of a paragraph from its first strong character (rules P2/P3): `Some(true)`
/// right-to-left, `Some(false)` left-to-right, `None` when it has no strong character.
pub fn base_rtl(s: &str) -> Option<bool> {
    s.chars().find_map(|c| match bidi_class(c) {
        BidiClass::L => Some(false),
        BidiClass::R | BidiClass::AL => Some(true),
        _ => None,
    })
}

fn para_level(base: Option<bool>) -> Option<Level> {
    base.map(|rtl| if rtl { Level::rtl() } else { Level::ltr() })
}

/// Map character indices of `text` to byte offsets (one extra entry for the end).
fn byte_starts(text: &str) -> Vec<usize> {
    let mut v: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    v.push(text.len());
    v
}

/// The visual runs of one line of `chars`: character-index ranges in left-to-right display order,
/// each flagged right-to-left or not. The paragraph direction comes from the first strong
/// character (or `base` when given). Characters inside a right-to-left run are still in logical
/// order: the caller lays them out from right to left.
pub fn visual_runs(chars: &[char], base: Option<bool>) -> Vec<(Range<usize>, bool)> {
    if chars.is_empty() {
        return Vec::new();
    }
    let text: String = chars.iter().collect();
    let starts = byte_starts(&text);
    let char_at = |byte: usize| starts.partition_point(|b| *b < byte);
    let info = ParagraphBidiInfo::new(&text, para_level(base));
    let (levels, runs) = info.visual_runs(0..text.len());
    runs.into_iter()
        .filter(|r| !r.is_empty())
        .map(|r| {
            let rtl = levels.get(r.start).is_some_and(|l| l.is_rtl());
            (char_at(r.start)..char_at(r.end), rtl)
        })
        .collect()
}

/// Resolved embedding level of every character of one line (after rule L1), for reordering
/// pieces of the line with [`reorder`].
pub fn char_levels(text: &str, base: Option<bool>) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }
    let info = ParagraphBidiInfo::new(text, para_level(base));
    info.reordered_levels_per_char(0..text.len()).into_iter().map(|l| l.number()).collect()
}

/// Display order (rule L2) of items with the given embedding levels: indices into `levels`,
/// left to right.
pub fn reorder(levels: &[u8]) -> Vec<usize> {
    let lv: Vec<Level> = levels.iter().map(|l| Level::new(*l).unwrap_or_else(|_| Level::ltr())).collect();
    ParagraphBidiInfo::reorder_visual(&lv)
}

/// The mirrored glyph of a paired bracket in right-to-left text (rule L4, common pairs).
pub fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        '≤' => '≥',
        '≥' => '≤',
        _ => c,
    }
}

/// A line in display order, code point by code point: right-to-left runs reversed and their
/// brackets mirrored. Shows what the layout draws, before glyph shaping.
pub fn visual_order(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (r, rtl) in visual_runs(&chars, None) {
        let run = chars.get(r).unwrap_or_default();
        if rtl {
            out.extend(run.iter().rev().map(|c| mirror(*c)));
        } else {
            out.extend(run.iter());
        }
    }
    out
}

/// Arabic joining type (Unicode `ArabicShaping.txt` classes, the letters in common use).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Jt {
    /// Non-joining (also ZWNJ, spaces, Latin).
    U,
    /// Right-joining: joins only the preceding letter (alef, dal, reh, waw …).
    R,
    /// Dual-joining.
    D,
    /// Join-causing (ZWJ, tatweel).
    C,
    /// Transparent (combining marks).
    T,
}

fn joining_type(c: char) -> Jt {
    let u = u32::from(c);
    match u {
        0x0640 | 0x200D => Jt::C,
        0x0622..=0x0625
        | 0x0627
        | 0x0629
        | 0x062F..=0x0632
        | 0x0648
        | 0x0671..=0x0673
        | 0x0675..=0x0677
        | 0x0688..=0x0699
        | 0x06C0
        | 0x06C3..=0x06CB
        | 0x06CD
        | 0x06CF
        | 0x06D2
        | 0x06D3
        | 0x06D5
        | 0x06EE
        | 0x06EF
        | 0x0759..=0x075B
        | 0x076B
        | 0x076C
        | 0x0771
        | 0x0773
        | 0x0774
        | 0x0778
        | 0x0779 => Jt::R,
        0x0620
        | 0x0626
        | 0x0628
        | 0x062A..=0x062E
        | 0x0633..=0x063F
        | 0x0641..=0x0647
        | 0x0649
        | 0x064A
        | 0x066E
        | 0x066F
        | 0x0678..=0x0687
        | 0x069A..=0x06BF
        | 0x06C1
        | 0x06C2
        | 0x06CC
        | 0x06CE
        | 0x06D0
        | 0x06D1
        | 0x06FA..=0x06FC
        | 0x06FF
        | 0x0750..=0x0758
        | 0x075C..=0x076A
        | 0x076D..=0x0770
        | 0x0772
        | 0x0775..=0x0777
        | 0x077A..=0x077F => Jt::D,
        _ if is_mark(c) => Jt::T,
        _ => Jt::U,
    }
}

/// Contextual form of a joining letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Isolated,
    Final,
    Initial,
    Medial,
}

/// The contextual form of every character of a logical-order run (`None` for characters that
/// don't join: spaces, digits, Latin, marks, ZWNJ).
pub fn joining_forms(chars: &[char]) -> Vec<Option<Form>> {
    let jt: Vec<Jt> = chars.iter().map(|c| joining_type(*c)).collect();
    // Neighbour joining types, skipping transparent marks.
    let prev = |i: usize| jt.get(..i).and_then(|s| s.iter().rev().find(|t| **t != Jt::T)).copied().unwrap_or(Jt::U);
    let next = |i: usize| jt.get(i + 1..).and_then(|s| s.iter().find(|t| **t != Jt::T)).copied().unwrap_or(Jt::U);
    jt.iter()
        .enumerate()
        .map(|(i, t)| {
            if !matches!(t, Jt::R | Jt::D) {
                return None;
            }
            let joins_prev = matches!(prev(i), Jt::D | Jt::C);
            let joins_next = *t == Jt::D && matches!(next(i), Jt::R | Jt::D | Jt::C);
            Some(match (joins_prev, joins_next) {
                (true, true) => Form::Medial,
                (true, false) => Form::Final,
                (false, true) => Form::Initial,
                (false, false) => Form::Isolated,
            })
        })
        .collect()
}

/// Presentation forms of the basic Arabic letters (block FE70–FEFF, which starts at U+FE80):
/// each letter takes 1 (hamza), 2 (isolated, final) or 4 (isolated, final, initial, medial)
/// consecutive code points, in this order.
const FORMS_B: &[(u32, u32)] = &[
    (0x0621, 1),
    (0x0622, 2),
    (0x0623, 2),
    (0x0624, 2),
    (0x0625, 2),
    (0x0626, 4),
    (0x0627, 2),
    (0x0628, 4),
    (0x0629, 2),
    (0x062A, 4),
    (0x062B, 4),
    (0x062C, 4),
    (0x062D, 4),
    (0x062E, 4),
    (0x062F, 2),
    (0x0630, 2),
    (0x0631, 2),
    (0x0632, 2),
    (0x0633, 4),
    (0x0634, 4),
    (0x0635, 4),
    (0x0636, 4),
    (0x0637, 4),
    (0x0638, 4),
    (0x0639, 4),
    (0x063A, 4),
    (0x0641, 4),
    (0x0642, 4),
    (0x0643, 4),
    (0x0644, 4),
    (0x0645, 4),
    (0x0646, 4),
    (0x0647, 4),
    (0x0648, 2),
    (0x0649, 2),
    (0x064A, 4),
];

/// Presentation forms of Persian and Urdu letters (block FB50–FDFF): letter, first form, count.
const FORMS_A: &[(u32, u32, u32)] = &[
    (0x0671, 0xFB50, 2), // alef wasla
    (0x067E, 0xFB56, 4), // peh پ
    (0x0679, 0xFB66, 4), // tteh
    (0x0686, 0xFB7A, 4), // tcheh چ
    (0x0688, 0xFB88, 2), // ddal
    (0x0698, 0xFB8A, 2), // jeh ژ
    (0x0691, 0xFB8C, 2), // rreh
    (0x06A9, 0xFB8E, 4), // keheh ک
    (0x06AF, 0xFB92, 4), // gaf گ
    (0x06BA, 0xFB9E, 2), // noon ghunna
    (0x06C0, 0xFBA4, 2), // heh with yeh above
    (0x06C1, 0xFBA6, 4), // heh goal
    (0x06BE, 0xFBAA, 4), // heh doachashmee
    (0x06D2, 0xFBAE, 2), // yeh barree
    (0x06CC, 0xFBFC, 4), // farsi yeh ی
];

/// The presentation-form code point of `c` in `form`, when Unicode has one.
pub fn presentation_form(c: char, form: Form) -> Option<char> {
    let u = u32::from(c);
    let slot = match form {
        Form::Isolated => 0,
        Form::Final => 1,
        Form::Initial => 2,
        Form::Medial => 3,
    };
    let mut at = 0xFE80;
    let mut found = None;
    for (base, n) in FORMS_B {
        if *base == u {
            found = Some((at, *n));
            break;
        }
        at += n;
    }
    let (first, n) = found.or_else(|| FORMS_A.iter().find(|(b, _, _)| *b == u).map(|(_, f, n)| (*f, *n)))?;
    // A letter with only isolated and final forms keeps them in initial / medial position.
    let slot = if slot >= n { slot % 2 } else { slot };
    if slot >= n {
        return None;
    }
    char::from_u32(first + slot)
}

/// Lam-alef ligature for lam followed by an alef, isolated or final.
fn lam_alef(alef: char, final_form: bool) -> Option<char> {
    let first = match alef {
        '\u{0622}' => 0xFEF5,
        '\u{0623}' => 0xFEF7,
        '\u{0625}' => 0xFEF9,
        '\u{0627}' => 0xFEFB,
        _ => return None,
    };
    char::from_u32(first + u32::from(final_form))
}

/// Arabic joining with Unicode presentation forms, for a logical-order run: each joining letter
/// becomes its contextual form and lam + alef becomes one ligature. Returns the output code
/// points with the index of the source character each comes from (a ligature takes the lam's).
/// Joiners stay in place (a shaper hides them); `keep` decides whether a form may be used (a font
/// lacking it keeps the base letter).
pub fn presentation_forms(chars: &[char], keep: impl Fn(char) -> bool) -> Vec<(char, usize)> {
    let forms = joining_forms(chars);
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let form = forms.get(i).copied().flatten();
        if c == '\u{0644}'
            && let Some(f) = form
            && let Some(&a) = chars.get(i + 1)
            && let Some(lig) = lam_alef(a, matches!(f, Form::Medial | Form::Final))
            && keep(lig)
        {
            out.push((lig, i));
            i += 2;
            continue;
        }
        let shaped = form.and_then(|f| presentation_form(c, f)).filter(|p| keep(*p)).unwrap_or(c);
        out.push((shaped, i));
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        presentation_forms(&chars, |_| true).into_iter().map(|(c, _)| c).collect()
    }

    #[test]
    fn salam_takes_contextual_forms_and_the_lam_alef_ligature() {
        // س initial, ل + ا ligature (final, since lam joins seen), م isolated (alef doesn't join
        // forward).
        let chars: Vec<char> = "سلام".chars().collect();
        assert_eq!(joining_forms(&chars), vec![Some(Form::Initial), Some(Form::Medial), Some(Form::Final), Some(Form::Isolated)]);
        assert_eq!(forms("سلام"), "\u{FEB3}\u{FEFC}\u{FEE1}");
        let src: Vec<usize> = presentation_forms(&chars, |_| true).into_iter().map(|(_, i)| i).collect();
        assert_eq!(src, vec![0, 1, 3]);
        // A word-initial lam-alef is the isolated ligature.
        assert_eq!(forms("لا"), "\u{FEFB}");
    }

    #[test]
    fn persian_letters_join() {
        // پ چ ژ گ ی ک: initial, medial, final and isolated Persian forms.
        assert_eq!(forms("پی"), "\u{FB58}\u{FBFD}");
        assert_eq!(forms("چک"), "\u{FB7C}\u{FB8F}");
        assert_eq!(forms("گژ"), "\u{FB94}\u{FB8B}");
        assert_eq!(forms("کیک"), "\u{FB90}\u{FBFF}\u{FB8F}");
        // Jeh is right-joining: it never joins the next letter.
        assert_eq!(forms("ژی"), "\u{FB8A}\u{FBFC}");
    }

    #[test]
    fn zwnj_breaks_and_zwj_forces_joining() {
        // می‌روم ("I go"): ZWNJ after meem keeps it from joining yeh.
        let chars: Vec<char> = "می\u{200C}روم".chars().collect();
        let f = joining_forms(&chars);
        assert_eq!(f.get(1), Some(&Some(Form::Final)), "yeh before ZWNJ is final");
        assert_eq!(f.get(2), Some(&None), "ZWNJ itself does not join");
        assert_eq!(f.get(3), Some(&Some(Form::Isolated)), "reh after ZWNJ starts afresh");
        // ZWJ makes an isolated letter join forward.
        let z: Vec<char> = "ب\u{200D}".chars().collect();
        assert_eq!(joining_forms(&z).first(), Some(&Some(Form::Initial)));
        // Marks are transparent: بَب joins across the fatha.
        let m: Vec<char> = "ب\u{064E}ب".chars().collect();
        assert_eq!(joining_forms(&m), vec![Some(Form::Initial), None, Some(Form::Final)]);
    }

    #[test]
    fn mixed_line_visual_order() {
        // "Price 25 toman": a right-to-left paragraph; the number stays left-to-right.
        let v = visual_order("قیمت 25 تومان");
        assert_eq!(v, "ناموت 25 تمیق");
        let chars: Vec<char> = "قیمت 25 تومان".chars().collect();
        let runs = visual_runs(&chars, None);
        assert_eq!(runs, vec![(7..13, true), (5..7, false), (0..5, true)]);
        // Latin inside Farsi, and Farsi inside a Latin paragraph.
        assert_eq!(visual_order("فایل CAD باز"), "زاب CAD لیاف");
        assert_eq!(visual_order("open فایل now"), "open لیاف now");
        // Brackets mirror in right-to-left runs.
        assert_eq!(visual_order("(سلام)"), "(مالس)");
    }

    /// Bidi levels, display reordering, and which lines need OpenType shaping (RTL and Thai).
    #[test]
    fn levels_and_reorder() {
        assert!(has_rtl("abc سلام") && !has_rtl("abc 123"));
        assert!(needs_shaping("ห้องนอน ชั้น 2") && needs_shaping("abc سلام") && !needs_shaping("abc 123"));
        assert!(!has_rtl("ห้องนอน"), "Thai is left to right");
        assert_eq!(base_rtl("  123 سلام abc"), Some(true));
        assert_eq!(base_rtl("123"), None);
        let lv = char_levels("ab سل", None);
        assert_eq!(lv, vec![0, 0, 0, 1, 1]);
        assert_eq!(reorder(&[1, 1, 2, 1]), vec![3, 2, 1, 0]);
        assert_eq!(reorder(&[0, 1, 1, 0]), vec![0, 2, 1, 3]);
        assert!(visual_runs(&[], None).is_empty() && char_levels("", None).is_empty());
    }

    #[test]
    fn hostile_input_never_panics() {
        for s in [
            "a\nسلام\u{2029}b\r",
            "\u{202E}\u{202E}\u{202E}abc",
            "\u{2067}\u{2069}\u{2069}",
            "\u{0644}",
            "\u{0644}\u{200D}",
            "\u{064E}\u{064E}",
            "\u{FEFF}",
        ] {
            let _ = visual_order(s);
            let chars: Vec<char> = s.chars().collect();
            let _ = presentation_forms(&chars, |_| true);
            let _ = char_levels(s, Some(true));
        }
        let long: String = "سلام abc 123 ".repeat(2000);
        assert_eq!(visual_order(&long).chars().count(), long.chars().count());
        assert_eq!(reorder(&[200, 0, 255]).len(), 3);
    }
}
