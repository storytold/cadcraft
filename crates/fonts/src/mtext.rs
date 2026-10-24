//! MTEXT: inline-format parsing, word wrap and attachment.

use cadcraft_geom::{Bounds2, Vec2};

use crate::{Shaped, TextFont, shape_line, text_width};

pub(crate) mod para;

/// Strip MTEXT formatting codes to plain text with `\n` paragraph breaks.
/// Stacked fractions `\S1/2;` become `1/2`; `\~` is a space; braces are dropped.
pub fn plain_mtext(s: &str) -> String {
    let s = unescape_codes(s);
    let s = s.as_ref();
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => {
                let code = chars.get(i + 1).copied().unwrap_or(' ');
                match code {
                    'P' => {
                        out.push('\n');
                        i += 2;
                    }
                    'N' => {
                        out.push('\n');
                        i += 2;
                    }
                    '~' => {
                        out.push(' ');
                        i += 2;
                    }
                    '\\' | '{' | '}' => {
                        out.push(code);
                        i += 2;
                    }
                    'S' => {
                        // \Snum^den; \Snum/den; \Snum#den;
                        let mut j = i + 2;
                        let mut stack = String::new();
                        while let Some(&ch) = chars.get(j) {
                            if ch == ';' {
                                break;
                            }
                            stack.push(match ch {
                                '^' | '#' => '/',
                                c => c,
                            });
                            j += 1;
                        }
                        // "6\S1/2;" reads as "6 1/2".
                        if out.chars().last().is_some_and(|c| c.is_ascii_digit()) && stack.starts_with(|c: char| c.is_ascii_digit()) {
                            out.push(' ');
                        }
                        out.push_str(stack.trim_end_matches('/'));
                        i = j + 1;
                    }
                    'L' | 'l' | 'O' | 'o' | 'K' | 'k' => i += 2,
                    // Codes with an argument terminated by ';' (\f, \F, \H, \W, \Q, \T, \A, \C, \c, \p).
                    _ if takes_arg(code) => {
                        let mut j = i + 2;
                        while let Some(&ch) = chars.get(j) {
                            if ch == ';' {
                                break;
                            }
                            j += 1;
                        }
                        i = j + 1;
                    }
                    // An unknown code has no argument: drop the backslash and the code only.
                    _ => i += 2,
                }
            }
            '{' | '}' => i += 1,
            '\r' => i += 1,
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// A colour set inline with `\C` (ACI) or `\c` (24-bit RGB as 0xRRGGBB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MTextColor {
    Aci(u16),
    Rgb(u32),
}

/// MTEXT layout parameters.
#[derive(Clone, Debug)]
pub struct MTextParams {
    pub insert: Vec2,
    pub height: f64,
    /// Column (reference rectangle) width; 0 = no wrapping.
    pub width: f64,
    /// 1..=9: TL TC TR ML MC MR BL BC BR.
    pub attach: u8,
    pub rotation: f64,
    pub line_spacing: f64,
    /// Exactly: every line pitch is `line_spacing` times the entity height, whatever the line's
    /// tallest character; At least (`false`): taller characters push lines apart.
    pub line_spacing_exact: bool,
    /// Base font (the text style's), switched inline by `\f` / `\F`.
    pub font: TextFont,
    pub width_factor: f64,
    pub oblique: f64,
    /// Columns the text flows through (each `width` wide); `None`: one column.
    pub columns: Option<para::MTextColumnSpec>,
}

impl MTextParams {
    pub fn new(insert: Vec2, height: f64) -> Self {
        MTextParams {
            insert,
            height,
            width: 0.0,
            attach: 1,
            rotation: 0.0,
            line_spacing: 1.0,
            line_spacing_exact: false,
            font: TextFont::Stroke,
            width_factor: 1.0,
            oblique: 0.0,
            columns: None,
        }
    }
}

/// Geometry of one colour run (`color` = None: the entity's colour).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MTextPiece {
    pub color: Option<MTextColor>,
    pub shaped: Shaped,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MTextLayout {
    /// Every stroke and glyph outline as polylines, in world coordinates.
    pub strokes: Vec<Vec<Vec2>>,
    /// Geometry by colour run (strokes plus fillable glyph outlines).
    pub pieces: Vec<MTextPiece>,
    pub bounds: Bounds2,
    /// Plain text of each laid-out line.
    pub lines: Vec<String>,
}

/// Inline character format.
#[derive(Clone, Debug)]
struct Fmt {
    font: TextFont,
    height: f64,
    color: Option<MTextColor>,
    under: bool,
    over: bool,
    wf: f64,
    oblique: f64,
    /// `\A`: vertical alignment of tolerance stacks (`^`) on the line: 0 bottom, 1 centre, 2 top.
    valign: u8,
}

/// One shaped unit of a word.
#[derive(Clone, Debug)]
enum Seg {
    Text(String),
    /// Stacked text: top, bottom, separator (`/` bar, `#` diagonal, `^` tolerance).
    Stack(String, String, char),
}

#[derive(Clone, Debug)]
enum Atom {
    Word(Vec<(Seg, usize)>),
    Space(usize),
    Para(usize),
}

/// Upper bound on parsed atoms (hostile contents).
const MAX_ATOMS: usize = 200_000;
/// Scale of stacked text relative to the surrounding text.
const STACK_SCALE: f64 = 0.7;

/// Codes that take an argument terminated by `;` (besides `\S`, which has its own scan). Any
/// other code stands alone, so a missing `;` must not swallow the text after it.
pub(crate) fn takes_arg(code: char) -> bool {
    matches!(code, 'f' | 'F' | 'H' | 'W' | 'Q' | 'T' | 'A' | 'C' | 'c' | 'p')
}

/// The line-spacing factor to lay out with: AutoCAD's 0.25..=4 range, 1 for a missing or
/// non-finite value.
pub(crate) fn sane_line_spacing(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v.clamp(0.25, 4.0) } else { 1.0 }
}

/// Four hex digits at `chars[at..at + 4]`.
fn hex4(chars: &[char], at: usize) -> Option<u32> {
    let digits = chars.get(at..at.checked_add(4)?)?;
    digits.iter().try_fold(0u32, |acc, c| Some(acc * 16 + c.to_digit(16)?))
}

/// Push a decoded character so that the tokenizers read it as text, never as markup.
fn push_literal(out: &mut String, c: char) {
    if matches!(c, '\\' | '{' | '}') {
        out.push('\\');
    }
    out.push(c);
}

/// Decode `\U+hhhh` (one UTF-16 unit; a surrogate pair gives one character, a lone surrogate
/// U+FFFD) and `\M+nhhhh` (double-byte East Asian characters) to the characters they stand
/// for. Malformed escapes are left for the tokenizers, which drop the backslash and the code.
/// An escaped backslash (`\\U+0041`) is not an escape.
pub(crate) fn unescape_codes(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains("\\U+") && !s.contains("\\M+") {
        return std::borrow::Cow::Borrowed(s);
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c != '\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let (n, plus) = (chars.get(i + 1).copied(), chars.get(i + 2).copied());
        if n == Some('U') && plus == Some('+') {
            if let Some(unit) = hex4(&chars, i + 3) {
                i += 7;
                let low = |at: usize| {
                    (chars.get(at) == Some(&'\\') && chars.get(at + 1) == Some(&'U') && chars.get(at + 2) == Some(&'+'))
                        .then(|| hex4(&chars, at + 3))
                        .flatten()
                        .filter(|u| (0xDC00..0xE000).contains(u))
                };
                let ch = if (0xD800..0xDC00).contains(&unit) {
                    match low(i) {
                        Some(lo) => {
                            i += 7;
                            char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (lo - 0xDC00))
                        }
                        None => None,
                    }
                } else {
                    char::from_u32(unit)
                };
                push_literal(&mut out, ch.unwrap_or('\u{fffd}'));
                continue;
            }
        } else if n == Some('M') && plus == Some('+') && chars.get(i + 3).is_some_and(char::is_ascii_digit) && hex4(&chars, i + 4).is_some() {
            push_literal(&mut out, cadcraft_dxf::mif_escape_at(&chars, i).unwrap_or('\u{fffd}'));
            i += 8;
            continue;
        }
        // Any other backslash pair stays as it is (and an escaped backslash stays one).
        out.push('\\');
        i += 1;
        if let Some(&next) = chars.get(i) {
            out.push(next);
            i += 1;
        }
    }
    std::borrow::Cow::Owned(out)
}

fn arg(chars: &[char], from: usize) -> (String, usize) {
    let mut j = from;
    let mut a = String::new();
    while let Some(&ch) = chars.get(j) {
        if ch == ';' {
            return (a, j + 1);
        }
        a.push(ch);
        j += 1;
    }
    (a, j)
}

fn num(a: &str) -> Option<f64> {
    a.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `\fArial|b1|i0|c0|p34;` → the font, trying a bold/italic face first.
fn font_switch(a: &str) -> Option<TextFont> {
    let mut parts = a.split('|');
    let name = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    let (mut bold, mut italic) = (false, false);
    for p in parts {
        match p {
            "b1" => bold = true,
            "i1" => italic = true,
            _ => {}
        }
    }
    let style = match (bold, italic) {
        (true, true) => Some("bold italic"),
        (true, false) => Some("bold"),
        (false, true) => Some("italic"),
        _ => None,
    };
    if let Some(st) = style {
        for cand in [format!("{name} {st}"), format!("{name}-{}", st.replace(' ', "")), format!("{name}{}", if bold { "bd" } else { "i" })] {
            if let Some(f) = crate::ttf::find(&cand) {
                return Some(TextFont::Outline(f));
            }
        }
    }
    Some(TextFont::resolve(name))
}

/// Parse MTEXT contents into words, spaces and paragraph breaks with their formats.
fn parse(contents: &str, base: Fmt) -> (Vec<Atom>, Vec<Fmt>) {
    let chars: Vec<char> = contents.chars().collect();
    let h_min = base.height * 1e-3;
    let h_max = if (base.height * 1e3).is_finite() { base.height * 1e3 } else { f64::MAX };
    let mut fmts = vec![base];
    let mut cur = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    let mut atoms: Vec<Atom> = Vec::new();
    let mut word: Vec<(Seg, usize)> = Vec::new();
    let push_char = |word: &mut Vec<(Seg, usize)>, c: char, f: usize| {
        if let Some((Seg::Text(t), wf)) = word.last_mut()
            && *wf == f
        {
            t.push(c);
            return;
        }
        word.push((Seg::Text(c.to_string()), f));
    };
    let end_word = |atoms: &mut Vec<Atom>, word: &mut Vec<(Seg, usize)>| {
        if !word.is_empty() {
            atoms.push(Atom::Word(std::mem::take(word)));
        }
    };
    // Derive a new format from the current one.
    let derive = |fmts: &mut Vec<Fmt>, cur: &mut usize, f: &dyn Fn(&mut Fmt)| {
        let mut n = fmts.get(*cur).cloned().unwrap_or(Fmt {
            font: TextFont::Stroke,
            height: 1.0,
            color: None,
            under: false,
            over: false,
            wf: 1.0,
            oblique: 0.0,
            valign: 1,
        });
        f(&mut n);
        if fmts.len() < 100_000 {
            fmts.push(n);
            *cur = fmts.len() - 1;
        }
    };
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if atoms.len() > MAX_ATOMS {
            break;
        }
        match c {
            '\\' => {
                let code = chars.get(i + 1).copied().unwrap_or(' ');
                i += 2;
                match code {
                    'P' | 'N' => {
                        end_word(&mut atoms, &mut word);
                        atoms.push(Atom::Para(cur));
                    }
                    '~' => push_char(&mut word, '\u{a0}', cur),
                    '\\' | '{' | '}' => push_char(&mut word, code, cur),
                    'L' => derive(&mut fmts, &mut cur, &|f| f.under = true),
                    'l' => derive(&mut fmts, &mut cur, &|f| f.under = false),
                    'O' => derive(&mut fmts, &mut cur, &|f| f.over = true),
                    'o' => derive(&mut fmts, &mut cur, &|f| f.over = false),
                    'K' | 'k' => {}
                    'S' => {
                        let mut j = i;
                        let (mut top, mut bottom, mut sep) = (String::new(), String::new(), None);
                        while let Some(&ch) = chars.get(j) {
                            if ch == ';' {
                                break;
                            }
                            if ch == '\\' {
                                if let Some(&n) = chars.get(j + 1) {
                                    if sep.is_none() { top.push(n) } else { bottom.push(n) }
                                }
                                j += 2;
                                continue;
                            }
                            if sep.is_none() && matches!(ch, '/' | '#' | '^') {
                                sep = Some(ch);
                            } else if sep.is_none() {
                                top.push(ch);
                            } else {
                                bottom.push(ch);
                            }
                            j += 1;
                        }
                        i = j + 1;
                        word.push((Seg::Stack(top, bottom, sep.unwrap_or('/')), cur));
                    }
                    'f' | 'F' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        if let Some(font) = font_switch(&a) {
                            derive(&mut fmts, &mut cur, &|f| f.font = font.clone());
                        }
                    }
                    'H' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        let (rel, v) = match a.trim().strip_suffix(['x', 'X']) {
                            Some(r) => (true, num(r)),
                            None => (false, num(&a)),
                        };
                        if let Some(v) = v.filter(|v| *v > 0.0 && *v < 1e6) {
                            // Relative heights compound; keep the result within 1e-3..1e3 times the
                            // MTEXT's own height, never infinite.
                            derive(&mut fmts, &mut cur, &|f| f.height = if rel { (f.height * v).min(h_max).max(h_min) } else { v });
                        }
                    }
                    'W' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        if let Some(v) = num(a.trim_end_matches(['x', 'X'])).filter(|v| *v > 0.01 && *v < 100.0) {
                            derive(&mut fmts, &mut cur, &|f| f.wf = v);
                        }
                    }
                    'Q' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        if let Some(v) = num(&a).filter(|v| v.abs() < 85.0) {
                            derive(&mut fmts, &mut cur, &|f| f.oblique = v.to_radians());
                        }
                    }
                    'C' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        if let Ok(v) = a.trim().parse::<u16>() {
                            let col = if v == 0 || v >= 256 { None } else { Some(MTextColor::Aci(v)) };
                            derive(&mut fmts, &mut cur, &|f| f.color = col);
                        }
                    }
                    'c' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        if let Ok(v) = a.trim().parse::<u32>() {
                            derive(&mut fmts, &mut cur, &|f| f.color = Some(MTextColor::Rgb(v & 0xFF_FFFF)));
                        }
                    }
                    'A' => {
                        let (a, n) = arg(&chars, i);
                        i = n;
                        if let Ok(v @ 0..=2) = a.trim().parse::<u8>() {
                            derive(&mut fmts, &mut cur, &|f| f.valign = v);
                        }
                    }
                    _ if takes_arg(code) => {
                        // \T and \p: argument up to ';'.
                        let (_, n) = arg(&chars, i);
                        i = n;
                    }
                    // An unknown code has no argument: only the backslash and the code are dropped.
                    _ => {}
                }
            }
            '{' => {
                if stack.len() < 256 {
                    stack.push(cur);
                }
                i += 1;
            }
            '}' => {
                if let Some(f) = stack.pop() {
                    cur = f;
                }
                i += 1;
            }
            '\r' => i += 1,
            '\n' => {
                end_word(&mut atoms, &mut word);
                atoms.push(Atom::Para(cur));
                i += 1;
            }
            ' ' => {
                end_word(&mut atoms, &mut word);
                atoms.push(Atom::Space(cur));
                i += 1;
            }
            _ => {
                push_char(&mut word, c, cur);
                i += 1;
            }
        }
    }
    end_word(&mut atoms, &mut word);
    (atoms, fmts)
}

fn seg_width(seg: &Seg, f: &Fmt) -> f64 {
    match seg {
        Seg::Text(t) => text_width(&f.font, t, f.height, f.wf),
        Seg::Stack(top, bottom, sep) => {
            let hs = f.height * STACK_SCALE;
            let wt = text_width(&f.font, top, hs, f.wf);
            let wb = text_width(&f.font, bottom, hs, f.wf);
            match sep {
                '#' => wt + wb + f.height * 0.5,
                '^' => wt.max(wb),
                _ => wt.max(wb) + f.height * 0.2,
            }
        }
    }
}

/// Shape a segment with its left end at `x` on the baseline.
fn seg_shape(seg: &Seg, f: &Fmt, x: f64) -> Shaped {
    let h = f.height;
    let mut out = Shaped::default();
    let put = |s: &str, size: f64, at: Vec2| {
        if s.is_empty() {
            return Shaped::default();
        }
        let mut sh = shape_line(&f.font, s, size, f.wf, f.oblique);
        sh.map(|p| p + at);
        sh
    };
    let w = seg_width(seg, f);
    match seg {
        Seg::Text(t) => out.extend(put(t, h, Vec2::new(x, 0.0))),
        Seg::Stack(top, bottom, sep) => {
            let hs = h * STACK_SCALE;
            let wt = text_width(&f.font, top, hs, f.wf);
            let wb = text_width(&f.font, bottom, hs, f.wf);
            match sep {
                '#' => {
                    out.extend(put(top, hs, Vec2::new(x, h * 0.4)));
                    let sx = x + wt + h * 0.1;
                    out.extend(put(bottom, hs, Vec2::new(sx + h * 0.4, -h * 0.05)));
                    out.strokes.push(vec![Vec2::new(sx, -h * 0.1), Vec2::new(sx + h * 0.3, h * 1.1)]);
                }
                '^' => {
                    // Tolerances: centred on the line, or the lower value on the baseline (\A0) or
                    // the upper value level with the text (\A2).
                    let (yt, yb) = match f.valign {
                        0 => (hs + h * 0.1, 0.0),
                        2 => (0.0, -hs - h * 0.1),
                        _ => (h * 0.55, h * 0.45 - hs),
                    };
                    out.extend(put(top, hs, Vec2::new(x, yt)));
                    out.extend(put(bottom, hs, Vec2::new(x, yb)));
                }
                _ => {
                    let bar = h * 0.45;
                    out.extend(put(top, hs, Vec2::new(x + (w - wt) / 2.0, bar + h * 0.12)));
                    out.extend(put(bottom, hs, Vec2::new(x + (w - wb) / 2.0, bar - h * 0.12 - hs)));
                    out.strokes.push(vec![Vec2::new(x, bar), Vec2::new(x + w, bar)]);
                }
            }
        }
    }
    if f.under {
        out.strokes.push(vec![Vec2::new(x, -h * 0.2), Vec2::new(x + w, -h * 0.2)]);
    }
    if f.over {
        out.strokes.push(vec![Vec2::new(x, h * 1.2), Vec2::new(x + w, h * 1.2)]);
    }
    out
}

fn seg_plain(seg: &Seg) -> String {
    match seg {
        Seg::Text(t) => t.replace('\u{a0}', " "),
        Seg::Stack(a, b, _) => format!("{a}/{b}"),
    }
}

/// One laid-out piece of a line: a segment (`None`: the space between two words), its format,
/// x offset, width and the character offset of its text in [`Line::text`].
struct Item {
    seg: Option<Seg>,
    fmt: usize,
    x: f64,
    w: f64,
    at: usize,
}

#[derive(Default)]
struct Line {
    items: Vec<Item>,
    width: f64,
    max_h: f64,
    text: String,
    /// Running character count of `text`, kept incrementally so appending a segment need not
    /// rescan the whole accumulated line (which is multiplicative on a long line) (CWE-407).
    chars: usize,
    /// Index of the paragraph the line belongs to.
    para: usize,
}

impl Line {
    /// Put the items in display order for right-to-left text: the Unicode Bidirectional
    /// Algorithm over the line's text (paragraph direction `base`), pieces reordered by the level
    /// of their first character, x offsets recomputed. Lines without right-to-left text are
    /// left as they are.
    fn reorder_bidi(&mut self, base: Option<bool>) {
        if !crate::bidi::has_rtl(&self.text) {
            return;
        }
        let levels = crate::bidi::char_levels(&self.text, base);
        let item_levels: Vec<u8> = self.items.iter().map(|it| levels.get(it.at).copied().unwrap_or(0)).collect();
        let order = crate::bidi::reorder(&item_levels);
        let mut items: Vec<Option<Item>> = std::mem::take(&mut self.items).into_iter().map(Some).collect();
        let mut x = 0.0;
        for i in order {
            if let Some(mut it) = items.get_mut(i).and_then(Option::take) {
                it.x = x;
                x += it.w;
                self.items.push(it);
            }
        }
    }
}

/// Lay out MTEXT with the stroke font. `attach` 1..=9 (TL, TC, TR, ML, MC, MR, BL, BC, BR).
pub fn layout_mtext(contents: &str, insert: Vec2, height: f64, width: f64, attach: u8, rotation: f64, line_spacing: f64) -> MTextLayout {
    layout_mtext_with(contents, &MTextParams { insert, height, width, attach, rotation, line_spacing, ..MTextParams::new(insert, height) })
}

/// Lay out MTEXT: inline formatting runs, stacked fractions, paragraphs, word wrap to the
/// column width and the attachment point.
pub fn layout_mtext_with(contents: &str, p: &MTextParams) -> MTextLayout {
    let contents = unescape_codes(contents);
    let contents = contents.as_ref();
    let h = if p.height > 0.0 && p.height.is_finite() { p.height } else { 1.0 };
    let wf = if p.width_factor.is_finite() && p.width_factor > 0.0 { p.width_factor } else { 1.0 };
    let width = if p.width.is_finite() && p.width > 0.0 { p.width } else { 0.0 };
    let rotation = if p.rotation.is_finite() { p.rotation } else { 0.0 };
    let oblique = if p.oblique.is_finite() { p.oblique } else { 0.0 };
    let base = Fmt { font: p.font.clone(), height: h, color: None, under: false, over: false, wf, oblique, valign: 1 };
    let (atoms, fmts) = parse(contents, base.clone());
    if para::needed(contents, p) {
        return para::layout(contents, p, &base);
    }
    let fmt = |i: usize| fmts.get(i).unwrap_or(&base);
    let mut lines: Vec<Line> = Vec::new();
    let mut line = Line::default();
    let mut pending_space: Option<usize> = None;
    let mut last_fmt = 0usize;
    let mut para = 0usize;
    let finish = |lines: &mut Vec<Line>, line: &mut Line, f: &Fmt, para: usize| {
        if line.items.is_empty() {
            line.max_h = f.height;
        }
        line.para = para;
        lines.push(std::mem::take(line));
    };
    for a in &atoms {
        match a {
            Atom::Space(f) => {
                if !line.items.is_empty() || pending_space.is_some() {
                    pending_space = Some(*f);
                }
                last_fmt = *f;
            }
            Atom::Para(f) => {
                pending_space = None;
                finish(&mut lines, &mut line, fmt(*f), para);
                para += 1;
                last_fmt = *f;
            }
            Atom::Word(segs) => {
                let ww: f64 = segs.iter().map(|(sg, f)| seg_width(sg, fmt(*f))).sum();
                let sw = pending_space.map(|f| crate::space_width(&fmt(f).font, fmt(f).height, fmt(f).wf)).unwrap_or(0.0);
                if width > 0.0 && !line.items.is_empty() && line.width + sw + ww > width + 1e-9 {
                    finish(&mut lines, &mut line, fmt(last_fmt), para);
                } else if !line.items.is_empty() || pending_space.is_some() {
                    if let Some(f) = pending_space {
                        let at = line.chars;
                        line.items.push(Item { seg: None, fmt: f, x: line.width, w: sw, at });
                        line.text.push(' ');
                        line.chars += 1;
                    }
                    line.width += sw;
                }
                pending_space = None;
                for (sg, f) in segs {
                    let fw = seg_width(sg, fmt(*f));
                    line.max_h = line.max_h.max(fmt(*f).height);
                    let at = line.chars;
                    let plain = seg_plain(sg);
                    line.chars += plain.chars().count();
                    line.text.push_str(&plain);
                    line.items.push(Item { seg: Some(sg.clone()), fmt: *f, x: line.width, w: fw, at });
                    line.width += fw;
                    last_fmt = *f;
                }
            }
        }
    }
    finish(&mut lines, &mut line, fmt(last_fmt), para);
    // Right-to-left text: each line in display order, its paragraph's direction from the
    // paragraph's first strong character.
    let mut bases: Vec<Option<bool>> = Vec::new();
    for l in &lines {
        if bases.len() <= l.para {
            bases.resize(l.para + 1, None);
        }
        if let Some(b) = bases.get_mut(l.para)
            && b.is_none()
        {
            *b = crate::bidi::base_rtl(&l.text);
        }
    }
    for l in &mut lines {
        l.reorder_bidi(bases.get(l.para).copied().flatten());
    }
    let ls = sane_line_spacing(p.line_spacing);
    // Baselines below the top of the first line.
    let mut baselines = Vec::with_capacity(lines.len());
    let mut y = 0.0;
    for (i, l) in lines.iter().enumerate() {
        let lh = if l.max_h > 0.0 { l.max_h } else { h };
        y -= if i == 0 {
            lh
        } else if p.line_spacing_exact {
            h * 5.0 / 3.0 * ls
        } else {
            lh * 5.0 / 3.0 * ls
        };
        baselines.push(y);
    }
    let block_h = -y;
    let block_w = if width > 0.0 { width } else { lines.iter().map(|l| l.width).fold(0.0, f64::max) };
    let col = (p.attach.clamp(1, 9) - 1) % 3;
    let row = (p.attach.clamp(1, 9) - 1) / 3;
    let top = match row {
        0 => 0.0,
        1 => block_h / 2.0,
        _ => block_h,
    };
    let insert = p.insert;
    let xf = |q: Vec2| insert + q.rotate(rotation);
    let mut out = MTextLayout { lines: lines.iter().map(|l| l.text.clone()).collect(), ..Default::default() };
    for (l, base_y) in lines.iter().zip(baselines) {
        let x0 = match col {
            0 => 0.0,
            1 => -l.width / 2.0,
            _ => -l.width,
        };
        for it in &l.items {
            let Some(sg) = &it.seg else { continue };
            let fm = fmt(it.fmt);
            let mut sh = seg_shape(sg, fm, it.x);
            let off = Vec2::new(x0, top + base_y);
            sh.map(|q| xf(q + off));
            match out.pieces.last_mut() {
                Some(pc) if pc.color == fm.color => pc.shaped.extend(sh),
                _ => out.pieces.push(MTextPiece { color: fm.color, shaped: sh }),
            }
        }
    }
    for pc in &out.pieces {
        out.strokes.extend(pc.shaped.outlines());
    }
    let x0 = match col {
        0 => 0.0,
        1 => -block_w / 2.0,
        _ => -block_w,
    };
    out.bounds = Bounds2::from_points(
        [Vec2::new(x0, top), Vec2::new(x0 + block_w, top), Vec2::new(x0 + block_w, top - block_h), Vec2::new(x0, top - block_h)].map(xf),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Incremental character accounting preserves the text of a multi-word line without introducing wrapping.
    #[test]
    fn line_char_offsets_track_incrementally() {
        // A multi-word line lays out correctly even though the per-line character count is now
        // maintained incrementally rather than rescanned per segment (CWE-407, row 20).
        let l = layout_mtext("alpha beta gamma", Vec2::ZERO, 1.0, 0.0, 1, 0.0, 1.0);
        assert_eq!(l.lines.len(), 1, "no wrap width: one line");
        assert_eq!(l.lines[0], "alpha beta gamma");
    }

    #[test]
    fn strips_formatting() {
        assert_eq!(plain_mtext("{\\fArial|b1;Bold}\\Pline 2"), "Bold\nline 2");
        assert_eq!(plain_mtext("1\\S1/2;\""), "1 1/2\"");
        assert_eq!(plain_mtext("a\\~b\\\\c"), "a b\\c");
        assert_eq!(plain_mtext("\\H2.5x;Big"), "Big");
        assert_eq!(plain_mtext("unterminated \\H2"), "unterminated ");
    }

    #[test]
    fn unicode_escapes_are_decoded_in_every_parser() {
        // The escape has no `;`, so it must not swallow the rest of the text.
        assert_eq!(plain_mtext("A\\U+00B0 B C;D"), "A\u{b0} B C;D");
        assert_eq!(lay("A\\U+00B0 B C;D", 0.0).lines, vec!["A\u{b0} B C;D"]);
        // The paragraph path (`\p`) and the plain path agree.
        assert_eq!(lay("\\pxql;A\\U+00B0 B C;D", 0.0).lines, vec!["A\u{b0} B C;D"]);
        let scanned = para::Paragraphs::scan("\\U+00B0\\pi1;x\\U+0041 y;z");
        assert!(scanned.formatted());
        // A surrogate pair is one character; a lone surrogate is U+FFFD.
        assert_eq!(plain_mtext("\\U+D83D\\U+DE00!"), "\u{1f600}!");
        assert_eq!(lay("\\U+D83D\\U+DE00!", 0.0).lines, vec!["\u{1f600}!"]);
        assert_eq!(plain_mtext("a\\U+D83Db"), "a\u{fffd}b");
        assert_eq!(plain_mtext("a\\U+DE00b\\U+D83D"), "a\u{fffd}b\u{fffd}");
        // A decoded backslash or brace is text, not markup; an escaped backslash is not an escape.
        assert_eq!(plain_mtext("\\U+005CH2;x"), "\\H2;x");
        assert_eq!(plain_mtext("\\\\U+0041"), "\\U+0041");
        // `\M+n` double-byte characters, decoded like the DXF reader does.
        assert_eq!(plain_mtext("\\M+5BCFE x;y"), "\u{4ef6} x;y");
        // Malformed escapes lose only the backslash and the code.
        assert_eq!(plain_mtext("a\\U+12 b"), "a+12 b");
        assert_eq!(plain_mtext("a\\M+ b"), "a+ b");
    }

    #[test]
    fn unknown_code_without_semicolon_keeps_the_text() {
        assert_eq!(plain_mtext("A\\Zbc d"), "Abc d");
        assert_eq!(lay("A\\Zbc d", 0.0).lines, vec!["Abc d"]);
        assert_eq!(lay("\\pxql;A\\Zbc d", 0.0).lines, vec!["Abc d"]);
        // A trailing lone backslash is dropped.
        assert_eq!(plain_mtext("A\\"), "A");
        // Codes with an argument still take it up to the `;`, or to the end.
        assert_eq!(plain_mtext("A\\H2;B"), "AB");
        assert_eq!(plain_mtext("A\\H2 B"), "A");
    }

    #[test]
    fn non_finite_oblique_and_line_spacing_are_repaired() {
        let run = |oblique: f64, spacing: f64| {
            let p = MTextParams { oblique, line_spacing: spacing, ..MTextParams::new(Vec2::ZERO, 1.0) };
            layout_mtext_with("AB\\PCD", &p)
        };
        let finite =
            |l: &MTextLayout| l.bounds.min.x.is_finite() && l.bounds.max.x.is_finite() && l.bounds.min.y.is_finite() && l.bounds.max.y.is_finite();
        let reference = run(0.0, 1.0);
        for o in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let l = run(o, 1.0);
            assert!(finite(&l), "oblique {o}");
            assert_eq!(l.bounds, reference.bounds, "oblique {o}");
        }
        for s in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -2.0] {
            let l = run(0.0, s);
            assert!(finite(&l), "spacing {s}");
            assert_eq!(l.bounds, reference.bounds, "spacing {s}");
        }
        // Spacing is clamped to AutoCAD's 0.25..=4 range.
        assert_eq!(run(0.0, 1e9).bounds, run(0.0, 4.0).bounds);
        assert_eq!(run(0.0, 1e-9).bounds, run(0.0, 0.25).bounds);
        assert!(run(0.0, 4.0).bounds.height() > reference.bounds.height());
        // The paragraph layout takes the same path.
        let p = MTextParams { oblique: f64::NAN, line_spacing: f64::NAN, ..MTextParams::new(Vec2::ZERO, 1.0) };
        assert!(finite(&layout_mtext_with("\\pxql;AB\\PCD", &p)));
    }

    #[test]
    fn relative_heights_do_not_run_away() {
        let src = format!("{}A", "\\H999999x;".repeat(60));
        let l = lay(&src, 0.0);
        assert!(l.bounds.height().is_finite() && l.bounds.width().is_finite());
        assert!(l.bounds.height() <= 1e3 * 1.5, "{}", l.bounds.height());
        assert!(l.bounds.height() > 100.0, "the glyph must not vanish: {}", l.bounds.height());
        // Shrinking is bounded too.
        let tiny = lay(&format!("{}A", "\\H0.000001x;".repeat(60)), 0.0);
        assert!(tiny.bounds.height() > 0.0);
    }

    #[test]
    fn wraps_to_width() {
        let l = layout_mtext("the quick brown fox jumps over the lazy dog", Vec2::ZERO, 1.0, 8.0, 1, 0.0, 1.0);
        assert!(l.lines.len() > 2);
        assert!(l.bounds.max.y.abs() < 1e-9);
        assert!((l.bounds.width() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn middle_centre_attach() {
        let l = layout_mtext("AB", Vec2::new(5.0, 5.0), 1.0, 0.0, 5, 0.0, 1.0);
        let c = l.bounds.center();
        assert!((c.x - 5.0).abs() < 1e-9 && (c.y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn stroke_word_space_matches_text() {
        let l = layout_mtext("A B", Vec2::ZERO, 1.0, 0.0, 1, 0.0, 1.0);
        assert!((l.bounds.width() - crate::line_width("A B", 1.0, 1.0)).abs() < 1e-9);
    }

    fn lay(s: &str, width: f64) -> MTextLayout {
        layout_mtext(s, Vec2::ZERO, 1.0, width, 1, 0.0, 1.0)
    }

    #[test]
    fn indents_are_drawing_units() {
        // An AutoCAD file's `\pi4.8634;` on 4.6652 high text in a 50.2 wide box indents the
        // first line by 4.8634 units: the text still fits on one line.
        let l = layout_mtext("\\pi4.8634;AAA BBB CCC", Vec2::ZERO, 4.6652, 50.2, 1, 0.0, 1.0);
        assert_eq!(l.lines, vec!["AAA BBB CCC"]);
    }

    #[test]
    fn paragraphs_make_lines() {
        let l = lay("one\\Ptwo\\P\\Pfour", 0.0);
        assert_eq!(l.lines, vec!["one", "two", "", "four"]);
        // Four lines: cap height plus three pitches of 5/3.
        assert!((l.bounds.height() - (1.0 + 3.0 * 5.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn absolute_and_relative_height() {
        let small = lay("AB", 0.0).bounds.width();
        let abs = lay("\\H2;AB", 0.0);
        assert!((abs.bounds.width() - 2.0 * small).abs() < 1e-6);
        assert!((abs.bounds.height() - 2.0).abs() < 1e-9);
        let rel = lay("{\\H3x;AB}AB", 0.0);
        assert!((rel.bounds.width() - 4.0 * small).abs() < 1e-6, "{}", rel.bounds.width());
        assert!((rel.bounds.height() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn colour_runs() {
        let l = lay("A{\\C1;B}C", 0.0);
        let cols: Vec<Option<MTextColor>> = l.pieces.iter().map(|p| p.color).collect();
        assert_eq!(cols, vec![None, Some(MTextColor::Aci(1)), None]);
        let l = lay("\\c16711680;R", 0.0);
        assert_eq!(l.pieces.first().map(|p| p.color), Some(Some(MTextColor::Rgb(0xFF0000))));
    }

    #[test]
    fn underline_and_overline() {
        let plain = lay("AB", 0.0).strokes.len();
        let u = lay("\\LAB\\l", 0.0);
        assert_eq!(u.strokes.len(), plain + 1);
        // Top-left attachment: the first baseline is one text height below the insert.
        assert!(u.strokes.iter().any(|s| s.len() == 2 && s.iter().all(|p| (p.y + 1.2).abs() < 1e-9)));
        let o = lay("\\OAB\\o", 0.0);
        assert!(o.strokes.iter().any(|s| s.len() == 2 && s.iter().all(|p| (p.y - 0.2).abs() < 1e-9)));
    }

    #[test]
    fn stacked_fraction_is_small_over_large() {
        let l = lay("\\S1/2;", 0.0);
        assert_eq!(l.lines, vec!["1/2"]);
        // A horizontal bar plus numerator above it and denominator below.
        let bar = l.strokes.iter().find(|s| s.len() == 2 && (s[0].y - s[1].y).abs() < 1e-12 && (s[0].y + 0.55).abs() < 1e-9);
        assert!(bar.is_some());
        let ys: Vec<f64> = l.strokes.iter().flatten().map(|p| p.y).collect();
        let max = ys.iter().cloned().fold(f64::MIN, f64::max);
        let min = ys.iter().cloned().fold(f64::MAX, f64::min);
        assert!(max > 0.0 && min < -1.0, "{min}..{max}");
        // Narrower than the two digits side by side at full size.
        assert!(l.bounds.width() < crate::line_width("12", 1.0, 1.0));
    }

    #[test]
    fn wrap_respects_runs_and_width() {
        let l = lay("alpha {\\H2x;beta} gamma delta", 6.0);
        assert!(l.lines.len() >= 2);
        let w: f64 = crate::line_width("alpha", 1.0, 1.0);
        assert!(w < 6.0);
        assert_eq!(l.lines.first().map(String::as_str), Some("alpha"));
    }

    #[test]
    fn font_switch_falls_back_to_stroke() {
        let l = lay("{\\fNoSuchFontAnywhere|b1;AB}", 0.0);
        assert!(!l.strokes.is_empty());
        assert!(l.pieces.iter().all(|p| p.shaped.glyphs.is_empty()));
    }

    /// Stroke-font geometry that isn't a missing-glyph box (Latin letters here).
    fn latin_points(l: &MTextLayout) -> Vec<Vec2> {
        l.pieces.iter().flat_map(|p| p.shaped.strokes.iter()).filter(|s| !(s.len() == 5 && s.first() == s.last())).flatten().copied().collect()
    }

    #[test]
    fn rtl_paragraph_lays_words_out_right_to_left() {
        // A Persian paragraph: the first word is rightmost, the Latin word at the end of the
        // sentence is leftmost (without any installed font the Persian letters are boxes).
        let l = lay("سلام دنیا abc", 0.0);
        let xs = latin_points(&l);
        assert!(!xs.is_empty());
        let w = l.bounds.width();
        assert!(xs.iter().all(|p| p.x < w / 2.0), "abc on the left of a right-to-left line");
        assert_eq!(l.lines, vec!["سلام دنیا abc"], "plain text stays in logical order");
        // The same words in a left-to-right paragraph keep abc first.
        let l = lay("abc سلام دنیا", 0.0);
        assert!(latin_points(&l).iter().all(|p| p.x < l.bounds.width() / 2.0));
        assert!((l.bounds.width() - w).abs() < 1e-9, "same width either way");
    }

    #[test]
    fn rtl_wrapping_keeps_the_column_and_attachment() {
        // Top-right attachment, wrapped to a 6-unit column: every line ends at the insertion
        // point's x and nothing leaves the column.
        let text = "قیمت 25 تومان و abc def";
        let l = layout_mtext(text, Vec2::ZERO, 1.0, 6.0, 3, 0.0, 1.0);
        assert!(l.lines.len() >= 2, "{:?}", l.lines);
        assert!(l.bounds.max.x.abs() < 1e-9 && (l.bounds.width() - 6.0).abs() < 1e-9);
        let pts: Vec<Vec2> = l.strokes.iter().flatten().copied().collect();
        // Allow an installed font's overhang (a tenth of the height), so this holds on any machine.
        assert!(pts.iter().all(|p| p.x <= 0.1 && p.x >= -6.0 - 0.1), "inside the column");
        // Centred: the line box is centred on the insertion point.
        let c = layout_mtext(text, Vec2::ZERO, 1.0, 0.0, 2, 0.0, 1.0);
        assert!(c.bounds.center().x.abs() < 1e-9);
    }

    #[test]
    fn hostile_contents_do_not_panic() {
        for s in [
            "\u{202E}{\\C1;سل}ام\\P\u{2067}",
            "\\S\u{05D0}/ب;",
            "\\",
            "\\S",
            "\\S/;",
            "{{{{",
            "}}}}",
            "\\H;",
            "\\H-1;x",
            "\\Hx;",
            "\\C999;a",
            "\\f;",
            "\\W0;a",
            "\\Q90;a",
            "\\P\\P\\P",
        ] {
            let _ = lay(s, 1.0);
        }
        let long = "word ".repeat(5000);
        assert!(lay(&long, 10.0).lines.len() > 100);
    }
}
