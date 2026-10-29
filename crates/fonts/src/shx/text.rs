//! Text set in SHX fonts: a text style's main font and big font, horizontal or vertical.
//!
//! Each character is drawn by the font that has it: a double-byte character of the big font's
//! code page (its lead byte in an escape range) from the big font, others from the main font
//! (character code, or Unicode in a Unicode font; `%%d` `%%p` `%%c` from shapes 256–258 or their
//! older or Unicode places). A font draws at text height / its `above` value, and its pen's end
//! position is the advance: there is no added letter gap. Characters neither font has come from
//! the built-in stroke font (and its fallback fonts), so nothing goes missing.
//!
//! Vertical text (a style flag, for fonts with modes 2) runs each shape's vertical-only
//! commands (code 14) and follows the pen downward.

use std::sync::Arc;

use cadcraft_geom::Vec2;
use encoding_rs::Encoding;

use super::{ShxFont, ShxKind};
use crate::{Shaped, bidi};

/// A text style's SHX fonts: the main font, the big font and the orientation. Either font may
/// be missing (not found): its characters then come from the stroke font.
#[derive(Clone, Debug)]
pub struct ShxText {
    pub main: Option<Arc<ShxFont>>,
    pub big: Option<Arc<ShxFont>>,
    /// The style is vertical and the main font supports it.
    pub vertical: bool,
    /// The big font's code page (from its name and escape ranges).
    big_encoding: &'static Encoding,
}

impl ShxText {
    /// The fonts of a style: `big_name` is the big font's file name (it picks the code page).
    pub fn new(main: Option<Arc<ShxFont>>, big: Option<Arc<ShxFont>>, big_name: &str, vertical: bool) -> ShxText {
        let big_encoding = big.as_deref().map_or(encoding_rs::GBK, |b| big_font_encoding(big_name, b.escape_ranges()));
        let vertical = vertical && main.as_ref().is_some_and(|m| m.is_dual());
        ShxText { main, big, vertical, big_encoding }
    }
    /// The code page of the big font's double-byte codes.
    pub fn big_font_encoding(&self) -> &'static str {
        self.big_encoding.name()
    }
}

/// The code page a big font's double-byte codes are in: known Japanese, Chinese and Korean
/// big font names, else its escape ranges (Shift-JIS lead bytes from 0x81; the others start at
/// 0xA1 or above, read as GBK).
fn big_font_encoding(name: &str, ranges: &[(u16, u16)]) -> &'static Encoding {
    let stem = name.trim().rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
    let stem = stem.trim_end_matches(".shx").trim_start_matches('@');
    if stem.starts_with("whg") || stem.starts_with("wht") || stem.contains("kor") || stem.contains("ksc") {
        return encoding_rs::EUC_KR;
    }
    if stem.starts_with("chineset") || stem.contains("big5") || stem.starts_with("cht") {
        return encoding_rs::BIG5;
    }
    if stem.starts_with("gb") || stem.starts_with("hz") || stem.contains("chs") {
        return encoding_rs::GBK;
    }
    if stem.starts_with("bigfont") || stem.starts_with("extfont") || stem.contains("jis") || stem.contains("jpn") {
        return encoding_rs::SHIFT_JIS;
    }
    if ranges.iter().any(|(lo, _)| (0x81..0xA1).contains(lo)) { encoding_rs::SHIFT_JIS } else { encoding_rs::GBK }
}

/// The double-byte code of `c` in `enc`, if it has one.
fn double_byte(c: char, enc: &'static Encoding) -> Option<u16> {
    let mut buf = [0u8; 4];
    let (bytes, _, errors) = enc.encode(c.encode_utf8(&mut buf));
    match (errors, bytes.as_ref()) {
        (false, [a, b]) => Some(u16::from_be_bytes([*a, *b])),
        _ => None,
    }
}

/// The shape numbers that may draw `c` in `font`, best first.
fn candidates(c: char, font: &ShxFont) -> [Option<u16>; 4] {
    let u = u32::from(c);
    let unicode = font.kind() == ShxKind::Unifont && font.encoding() == 0;
    let code = |x: u32| u16::try_from(x).ok();
    // The special characters of `%%d` `%%p` `%%c`: shapes 256–258, the Latin-1 or Unicode code,
    // and the oldest fonts' 127–129.
    let special = match c {
        '°' => Some((256, 0xB0, 127)),
        '±' => Some((257, 0xB1, 128)),
        '⌀' | '∅' => Some((258, u, 129)),
        _ => None,
    };
    if let Some((s, latin, old)) = special {
        return if unicode {
            [code(u), code(if c == '⌀' { 0x2205 } else { latin }), Some(s), Some(old)]
        } else {
            [Some(s), code(latin), Some(old), None]
        };
    }
    if unicode || u < 0x80 {
        return [code(u), None, None, None];
    }
    // A single-byte font: the Windows Latin-1 code of the character (not the old special
    // codes 127–129).
    let mut buf = [0u8; 4];
    let (bytes, _, errors) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
    match (errors, bytes.as_ref()) {
        (false, [b]) if !(127..=129).contains(b) => [Some(u16::from(*b)), None, None, None],
        _ => [None, None, None, None],
    }
}

/// The font and shape that draw `c`.
fn pick(t: &ShxText, c: char) -> Option<(&ShxFont, u16)> {
    let from_main = || {
        let m = t.main.as_deref()?;
        candidates(c, m).into_iter().flatten().find(|n| m.has_shape(*n)).map(|n| (m, n))
    };
    let from_big = || {
        let b = t.big.as_deref()?;
        let code = double_byte(c, t.big_encoding)?;
        let [lead, _] = code.to_be_bytes();
        (b.is_lead_byte(lead) && b.has_shape(code)).then_some((b, code))
    };
    if u32::from(c) < 0x100 || matches!(c, '⌀' | '∅') { from_main().or_else(from_big) } else { from_big().or_else(from_main) }
}

fn finite_or(v: f64, d: f64) -> f64 {
    if v.is_finite() { v } else { d }
}

/// Shape one line (baseline at y = 0, x from 0).
pub(crate) fn shape(t: &ShxText, s: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let shear = finite_or(oblique, 0.0).tan().clamp(-10.0, 10.0);
    let chars = crate::decode_controls(s);
    let logical: Vec<char> = chars.iter().map(|v| v.0).collect();
    let mut out = Shaped::default();
    let mut spans: Vec<(f64, f64, bool, bool)> = Vec::new();
    let mut pen = Vec2::ZERO;
    let mut max_x: f64 = 0.0;
    let runs = if t.vertical { vec![(0..logical.len(), false)] } else { bidi::visual_runs(&logical, None) };
    for (run, rtl) in runs {
        let order: Box<dyn Iterator<Item = usize>> = if rtl { Box::new(run.rev()) } else { Box::new(run) };
        for i in order {
            let Some(&(c, under, over)) = chars.get(i) else { continue };
            if bidi::is_format_control(c) {
                continue;
            }
            let c = if rtl { bidi::mirror(c) } else { c };
            let x0 = pen.x;
            match pick(t, c).and_then(|(f, n)| Some((f, f.glyph(n, t.vertical)?))) {
                Some((f, g)) => {
                    let k = h / f.units_per_height();
                    let place = |p: Vec2| Vec2::new(pen.x + p.x * k * wf + p.y * k * shear, pen.y + p.y * k);
                    for st in &g.strokes {
                        let pts: Vec<Vec2> = st.iter().map(|p| place(*p)).collect();
                        max_x = pts.iter().fold(max_x, |m, p| m.max(p.x));
                        out.strokes.push(pts);
                    }
                    let adv = Vec2::new(g.advance.x * k * wf, g.advance.y * k);
                    pen = if t.vertical { pen + adv } else { Vec2::new(pen.x + adv.x, pen.y) };
                }
                None => {
                    // The stroke font draws it, at the pen.
                    let mut fb = crate::shape_stroke(&c.to_string(), h, wf, oblique);
                    let at = pen;
                    fb.map(|p| p + at);
                    max_x = max_x.max(at.x + fb.width);
                    out.extend(fb);
                    if t.vertical {
                        pen.y -= h * 1.5;
                    } else {
                        pen.x += crate::stroke_advance(c, h, wf);
                    }
                }
            }
            if !t.vertical {
                spans.push((x0, pen.x, under, over));
            }
        }
    }
    out.strokes.extend(crate::decorations(&spans, h));
    out.width = if t.vertical { max_x.max(0.0) } else { pen.x.max(0.0) };
    out
}

/// The width of one line: the sum of the advances.
pub(crate) fn width(t: &ShxText, s: &str, height: f64, width_factor: f64) -> f64 {
    if t.vertical || bidi::has_rtl(s) {
        return shape(t, s, height, width_factor, 0.0).width;
    }
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    crate::decode_controls(s)
        .into_iter()
        .filter(|(c, _, _)| !bidi::is_format_control(*c))
        .map(|(c, _, _)| match pick(t, c).and_then(|(f, n)| Some((f, f.glyph(n, false)?))) {
            Some((f, g)) => g.advance.x * h / f.units_per_height() * wf,
            None => crate::stroke_advance(c, h, wf),
        })
        .sum::<f64>()
        .max(0.0)
}
