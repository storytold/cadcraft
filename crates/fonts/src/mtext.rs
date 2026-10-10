//! MTEXT: inline-format parsing, word wrap and attachment.

use cadcraft_geom::{Bounds2, Vec2};

use crate::{Shaped, TextFont, shape_line, text_width};

/// Strip MTEXT formatting codes to plain text with `\n` paragraph breaks.
/// Stacked fractions `\S1/2;` become `1/2`; `\~` is a space; braces are dropped.
pub fn plain_mtext(s: &str) -> String {
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
                    _ => {
                        let mut j = i + 2;
                        while let Some(&ch) = chars.get(j) {
                            if ch == ';' {
                                break;
                            }
                            j += 1;
                        }
                        i = j + 1;
                    }
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
    /// Base font (the text style's), switched inline by `\f` / `\F`.
    pub font: TextFont,
    pub width_factor: f64,
    pub oblique: f64,
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
            font: TextFont::Stroke,
            width_factor: 1.0,
            oblique: 0.0,
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
                            derive(&mut fmts, &mut cur, &|f| f.height = if rel { f.height * v } else { v });
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
                    _ => {
                        // \A, \T, \p and unknown codes: argument up to ';'.
                        let (_, n) = arg(&chars, i);
                        i = n;
                    }
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
                    out.extend(put(top, hs, Vec2::new(x, h * 0.55)));
                    out.extend(put(bottom, hs, Vec2::new(x, h * 0.45 - hs)));
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

#[derive(Default)]
struct Line {
    items: Vec<(Seg, usize, f64)>,
    width: f64,
    max_h: f64,
    text: String,
}

/// Lay out MTEXT with the stroke font. `attach` 1..=9 (TL, TC, TR, ML, MC, MR, BL, BC, BR).
pub fn layout_mtext(contents: &str, insert: Vec2, height: f64, width: f64, attach: u8, rotation: f64, line_spacing: f64) -> MTextLayout {
    layout_mtext_with(contents, &MTextParams { insert, height, width, attach, rotation, line_spacing, ..MTextParams::new(insert, height) })
}

/// Lay out MTEXT: inline formatting runs, stacked fractions, paragraphs, word wrap to the
/// column width and the attachment point.
pub fn layout_mtext_with(contents: &str, p: &MTextParams) -> MTextLayout {
    let h = if p.height > 0.0 && p.height.is_finite() { p.height } else { 1.0 };
    let wf = if p.width_factor.is_finite() && p.width_factor > 0.0 { p.width_factor } else { 1.0 };
    let width = if p.width.is_finite() && p.width > 0.0 { p.width } else { 0.0 };
    let rotation = if p.rotation.is_finite() { p.rotation } else { 0.0 };
    let base = Fmt { font: p.font.clone(), height: h, color: None, under: false, over: false, wf, oblique: p.oblique };
    let (atoms, fmts) = parse(contents, base.clone());
    let fmt = |i: usize| fmts.get(i).unwrap_or(&base);
    let mut lines: Vec<Line> = Vec::new();
    let mut line = Line::default();
    let mut pending_space: Option<usize> = None;
    let mut last_fmt = 0usize;
    let finish = |lines: &mut Vec<Line>, line: &mut Line, f: &Fmt| {
        if line.items.is_empty() {
            line.max_h = f.height;
        }
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
                finish(&mut lines, &mut line, fmt(*f));
                last_fmt = *f;
            }
            Atom::Word(segs) => {
                let ww: f64 = segs.iter().map(|(sg, f)| seg_width(sg, fmt(*f))).sum();
                let sw = pending_space.map(|f| text_width(&fmt(f).font, " ", fmt(f).height, fmt(f).wf)).unwrap_or(0.0);
                if width > 0.0 && !line.items.is_empty() && line.width + sw + ww > width + 1e-9 {
                    finish(&mut lines, &mut line, fmt(last_fmt));
                } else if !line.items.is_empty() || pending_space.is_some() {
                    line.width += sw;
                    if pending_space.is_some() {
                        line.text.push(' ');
                    }
                }
                pending_space = None;
                for (sg, f) in segs {
                    let fw = seg_width(sg, fmt(*f));
                    line.max_h = line.max_h.max(fmt(*f).height);
                    line.text.push_str(&seg_plain(sg));
                    line.items.push((sg.clone(), *f, line.width));
                    line.width += fw;
                    last_fmt = *f;
                }
            }
        }
    }
    finish(&mut lines, &mut line, fmt(last_fmt));
    let ls = if p.line_spacing > 0.0 && p.line_spacing.is_finite() { p.line_spacing } else { 1.0 };
    // Baselines below the top of the first line.
    let mut baselines = Vec::with_capacity(lines.len());
    let mut y = 0.0;
    for (i, l) in lines.iter().enumerate() {
        let lh = if l.max_h > 0.0 { l.max_h } else { h };
        y -= if i == 0 { lh } else { lh * 5.0 / 3.0 * ls };
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
        for (sg, f, x) in &l.items {
            let fm = fmt(*f);
            let mut sh = seg_shape(sg, fm, *x);
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

    #[test]
    fn strips_formatting() {
        assert_eq!(plain_mtext("{\\fArial|b1;Bold}\\Pline 2"), "Bold\nline 2");
        assert_eq!(plain_mtext("1\\S1/2;\""), "1 1/2\"");
        assert_eq!(plain_mtext("a\\~b\\\\c"), "a b\\c");
        assert_eq!(plain_mtext("\\H2.5x;Big"), "Big");
        assert_eq!(plain_mtext("unterminated \\H2"), "unterminated ");
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

    fn lay(s: &str, width: f64) -> MTextLayout {
        layout_mtext(s, Vec2::ZERO, 1.0, width, 1, 0.0, 1.0)
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

    #[test]
    fn hostile_contents_do_not_panic() {
        for s in ["\\", "\\S", "\\S/;", "{{{{", "}}}}", "\\H;", "\\H-1;x", "\\Hx;", "\\C999;a", "\\f;", "\\W0;a", "\\Q90;a", "\\P\\P\\P"] {
            let _ = lay(s, 1.0);
        }
        let long = "word ".repeat(5000);
        assert!(lay(&long, 10.0).lines.len() > 100);
    }
}
