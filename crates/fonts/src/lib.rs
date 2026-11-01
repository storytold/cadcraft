//! CADCraft text layout.
//!
//! Turns TEXT and MTEXT into stroke polylines with the built-in single-stroke font
//! ("CADCraft Stroke") or glyph outlines of installed TrueType fonts ([`ttf`]), with a
//! per-character fallback to installed fonts for characters the chosen font lacks. Handles the
//! `%%` control codes, alignment modes, width factor, obliquing, MTEXT inline formatting
//! (paragraphs, stacking, height changes), word wrap and attachment points.
//!
//! Right-to-left lines (Persian, Arabic, Urdu, Hebrew) go through [`bidi`] (the Unicode
//! Bidirectional Algorithm, Arabic joining) and `complex` (OpenType shaping with `harfrust`):
//! each run is shaped in the font that draws it before glyphs are placed, so the fallback font
//! drawing Arabic in a stroke-font style joins its letters too.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod bidi;
mod complex;
mod mtext;
mod stroke;
pub mod subset;
pub mod ttf;

use cadcraft_geom::{Bounds2, Vec2};

pub use mtext::para::{DEFAULT_TAB, ListStyle, MTextColumnSpec, ParaAlign, ParaProps, Paragraphs, TabKind, list_contents};
pub use mtext::{MTextColor, MTextLayout, MTextParams, MTextPiece, layout_mtext, layout_mtext_with, plain_mtext};

mod craft_fonts;
pub use craft_fonts::{CRAFT_FONTS, CraftFont};

/// Name of the built-in font.
pub const BUILTIN_FONT: &str = "CADCraft Stroke";

/// One laid-out run of text: strokes in local coordinates (baseline at y = 0, x from 0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub strokes: Vec<Vec<Vec2>>,
    pub width: f64,
}

/// Replace `%%` control codes: `%%d` degree, `%%p` plus/minus, `%%c` diameter, `%%%` percent,
/// `%%nnn` character code. Underline/overline toggles (`%%u`, `%%o`) are returned as flags
/// per character.
pub fn decode_controls(s: &str) -> Vec<(char, bool, bool)> {
    let mut out = Vec::with_capacity(s.len());
    let mut under = false;
    let mut over = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars.get(i).copied().unwrap_or(' ');
        if c == '%' && chars.get(i + 1) == Some(&'%') {
            let code = chars.get(i + 2).copied().unwrap_or(' ');
            match code.to_ascii_lowercase() {
                'd' => out.push(('°', under, over)),
                'p' => out.push(('±', under, over)),
                'c' => out.push(('⌀', under, over)),
                '%' => out.push(('%', under, over)),
                'u' => under = !under,
                'o' => over = !over,
                d if d.is_ascii_digit() => {
                    let digits: String = chars.iter().skip(i + 2).take(3).take_while(|c| c.is_ascii_digit()).collect();
                    let n: u32 = digits.parse().unwrap_or(32);
                    out.push((char::from_u32(n).unwrap_or('?'), under, over));
                    i += 2 + digits.len();
                    continue;
                }
                _ => {
                    out.push(('%', under, over));
                    i += 1;
                    continue;
                }
            }
            i += 3;
            continue;
        }
        out.push((c, under, over));
        i += 1;
    }
    out
}

/// Advance width of one character at height 1 and width factor 1.
pub fn char_advance(c: char) -> f64 {
    match stroke::glyph(c) {
        Some((w, _)) => (w + stroke::GAP) / stroke::CAP,
        None if c.is_whitespace() => (2.4 + stroke::GAP) / stroke::CAP,
        None => (4.0 + stroke::GAP) / stroke::CAP,
    }
}

/// Whether `c` is an East Asian wide character (CJK ideographs and punctuation, kana, Hangul,
/// fullwidth forms): drawn in a square cell, like a big font's characters.
pub fn is_wide(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x3FFFD)
}

/// Where a CJK font's ideographic em box starts below the baseline, in em: the common value
/// (fonts put the box from -0.12 em to 0.88 em).
const IDEOGRAPHIC_DESCENT: f64 = 0.12;

/// How the stroke font draws a fallback glyph of `c` at height `h`: its origin's height above
/// the baseline and the scale of its font units (at 1000 per em). Wide characters fill a square
/// cell from the baseline to the text height, as a big font's do in AutoCAD-style SHX text,
/// so CJK text keeps the size and spacing its drawing was laid out with; other characters are
/// sized like TrueType text (cap height = text height).
fn stroke_fallback_metrics(g: &ttf::GlyphOutline, c: char, h: f64) -> (f64, f64) {
    if is_wide(c) { (IDEOGRAPHIC_DESCENT * h, h / 1000.0) } else { (0.0, h / g.cap) }
}

/// Whether the stroke font draws `c` from a fallback font.
fn draws_fallback(c: char) -> bool {
    stroke::glyph(c).is_none() && !c.is_whitespace() && ttf::fallback_glyph(c).is_some()
}

/// Advance of `c` in the stroke font at height `h` and width factor `wf`; characters the
/// stroke font lacks take a fallback font's advance when one has them.
fn stroke_advance(c: char, h: f64, wf: f64) -> f64 {
    if bidi::is_format_control(c) {
        return 0.0;
    }
    if stroke::glyph(c).is_none()
        && !c.is_whitespace()
        && let Some(g) = ttf::fallback_glyph(c)
    {
        return g.advance * stroke_fallback_metrics(&g, c, h).1 * wf;
    }
    char_advance(c) * h * wf
}

/// Shape a single line in the stroke font at `height`, with `width_factor` and `oblique`
/// (radians). Characters the stroke font lacks are drawn from an installed font that has them
/// (glyph outlines, sized like TrueType text), else as a missing-glyph box.
pub fn shape_stroke(s: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let sc = h / stroke::CAP;
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let mut out = Shaped::default();
    let mut x = 0.0;
    let mut under_start: Option<f64> = None;
    let mut over_start: Option<f64> = None;
    let mut gap = stroke::GAP * sc * wf;
    if bidi::has_rtl(s)
        && let Some(sh) = complex::shape(None, s, h, wf, oblique)
    {
        return sh;
    }
    for (c, under, over) in decode_controls(s) {
        if bidi::is_format_control(c) {
            continue;
        }
        let mut adv = char_advance(c) * h * wf;
        gap = stroke::GAP * sc * wf;
        match stroke::glyph(c) {
            Some((_, spec)) => draw_stroke_glyph(&mut out, spec, x, sc, wf, shear),
            None if !c.is_whitespace() => match ttf::fallback_glyph(c) {
                Some(g) => {
                    let (y, scale) = stroke_fallback_metrics(&g, c, h);
                    let contours = ttf::place_glyph_at(&g, Vec2::new(x, y), scale, wf, shear);
                    if !contours.is_empty() {
                        out.push_glyph(contours, ttf::text_glyph_at(&g, c, Vec2::new(x + y * shear, y), scale, wf, shear));
                    }
                    adv = g.advance * scale * wf;
                    gap = 0.0;
                }
                None => missing_glyph_box(&mut out, x, h, sc * wf),
            },
            None => {}
        }
        // Decorations.
        match (under, under_start) {
            (true, None) => under_start = Some(x),
            (false, Some(sx)) => {
                out.strokes.push(vec![Vec2::new(sx, -h * 0.2), Vec2::new(x, -h * 0.2)]);
                under_start = None;
            }
            _ => {}
        }
        match (over, over_start) {
            (true, None) => over_start = Some(x),
            (false, Some(sx)) => {
                out.strokes.push(vec![Vec2::new(sx, h * 1.2), Vec2::new(x, h * 1.2)]);
                over_start = None;
            }
            _ => {}
        }
        x += adv;
    }
    // The stroke font's gap after the last character isn't part of the line; a fallback glyph
    // has none.
    let end = (x - gap).max(0.0);
    if let Some(sx) = under_start {
        out.strokes.push(vec![Vec2::new(sx, -h * 0.2), Vec2::new(end, -h * 0.2)]);
    }
    if let Some(sx) = over_start {
        out.strokes.push(vec![Vec2::new(sx, h * 1.2), Vec2::new(end, h * 1.2)]);
    }
    out.width = end;
    out
}

/// Draw stroke-font glyph `spec` with its left edge at `x` (`sc` = text height / cap height).
fn draw_stroke_glyph(out: &mut Shaped, spec: &str, x: f64, sc: f64, wf: f64, shear: f64) {
    for st in stroke::strokes(spec) {
        let pts: Vec<Vec2> = st.iter().map(|(gx, gy)| Vec2::new(x + (gx * sc * wf) + gy * sc * shear, gy * sc)).collect();
        if pts.len() == 1 {
            if let Some(p) = pts.first() {
                out.strokes.push(vec![*p, *p + Vec2::new(sc * 0.2, 0.0)]);
            }
        } else {
            out.strokes.push(pts);
        }
    }
}

/// A character no font has: a small box, like a missing-glyph rectangle (`k` = text height /
/// cap height × width factor).
fn missing_glyph_box(out: &mut Shaped, x: f64, h: f64, k: f64) {
    let w = 4.0 * k;
    out.strokes.push(vec![Vec2::new(x, 0.0), Vec2::new(x + w, 0.0), Vec2::new(x + w, h), Vec2::new(x, h), Vec2::new(x, 0.0)]);
}

/// Lay out a single line at `height`, with `width_factor` and `oblique` (radians), in the
/// stroke font (fallback glyphs become closed outlines).
pub fn layout_line(s: &str, height: f64, width_factor: f64, oblique: f64) -> Run {
    let sh = shape_stroke(s, height, width_factor, oblique);
    let mut strokes = sh.strokes;
    strokes.extend(sh.glyphs.into_iter().flatten());
    Run { strokes, width: sh.width }
}

/// Width of a line without building strokes.
pub fn line_width(s: &str, height: f64, width_factor: f64) -> f64 {
    if bidi::has_rtl(s) {
        let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
        let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
        if let Some(sh) = complex::shape(None, s, h, wf, 0.0) {
            return sh.width;
        }
    }
    let chars = decode_controls(s);
    let n: f64 = chars.iter().map(|(c, _, _)| stroke_advance(*c, height, width_factor)).sum();
    // The stroke font's gap after the last character isn't part of the line; a fallback glyph
    // has none.
    let last = chars.iter().rev().map(|(c, _, _)| *c).find(|c| !bidi::is_format_control(*c));
    let gap = if last.is_some_and(draws_fallback) { 0.0 } else { stroke::GAP / stroke::CAP * height * width_factor };
    (n - gap).max(0.0)
}

/// Width of the gap MTEXT leaves between two words. Words are measured without their trailing
/// letter gap, so the stroke font's gap is added on both sides of the space, as TEXT lays it out.
pub fn space_width(font: &TextFont, height: f64, width_factor: f64) -> f64 {
    if let TextFont::Outline(face) = font
        && let Some(w) = ttf::width(face, " ", height, width_factor)
    {
        return w;
    }
    stroke_advance(' ', height, width_factor) + stroke::GAP / stroke::CAP * height * width_factor
}

/// The font a piece of text is set in: the built-in stroke font or an installed TrueType /
/// OpenType face (outlines). Either way, characters the font lacks come from a fallback font.
#[derive(Clone, Debug, Default)]
pub enum TextFont {
    #[default]
    Stroke,
    Outline(ttf::Face),
}

impl TextFont {
    /// Resolve a text style font name: an installed TTF/OTF/TTC when found; else, for a font we
    /// can't find (an SHX file, a missing TTF), the FONTALT font ([`ttf::font_alt`]) when set;
    /// else the stroke font.
    pub fn resolve(name: &str) -> TextFont {
        if let Some(f) = ttf::find(name) {
            return TextFont::Outline(f);
        }
        let n = name.trim();
        if n.is_empty() || n.eq_ignore_ascii_case(BUILTIN_FONT) {
            return TextFont::Stroke;
        }
        ttf::find(&ttf::font_alt()).map(TextFont::Outline).unwrap_or(TextFont::Stroke)
    }
    pub fn is_outline(&self) -> bool {
        matches!(self, TextFont::Outline(_))
    }
}

/// Shaped text in local or world coordinates: open strokes (stroke-font glyphs, underlines,
/// fraction bars) and closed glyph outlines grouped per glyph (fill each group even-odd).
/// Outline glyphs also come as [`TextGlyph`]s, for output that keeps text as text (PDF).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shaped {
    pub strokes: Vec<Vec<Vec2>>,
    pub glyphs: Vec<Vec<Vec<Vec2>>>,
    pub width: f64,
    /// The outline glyphs as font glyphs: which face and glyph each of [`Shaped::glyphs`] is,
    /// and where it sits.
    pub text: Vec<TextGlyph>,
}

/// A glyph of an installed outline font, placed: its outline is `Shaped::glyphs[outline]`.
/// Point `(u, v)` of the glyph in em units (1000 font units per em, as PDF glyph space) lands
/// at `origin + x * u / 1000 + y * v / 1000`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextGlyph {
    pub face: ttf::Face,
    /// Glyph id in the face.
    pub gid: u16,
    /// The character it shows.
    pub ch: char,
    /// Index of its outline in [`Shaped::glyphs`].
    pub outline: usize,
    /// The glyph origin (on the baseline).
    pub origin: Vec2,
    /// One em along the baseline (width factor applied).
    pub x: Vec2,
    /// One em up (obliquing applied).
    pub y: Vec2,
}

impl Shaped {
    /// Add a glyph outline and, when it is a font glyph, its [`TextGlyph`].
    /// An empty outline is retained only when text metadata needs its slot.
    pub(crate) fn push_glyph(&mut self, contours: Vec<Vec<Vec2>>, text: Option<TextGlyph>) {
        if contours.is_empty() && text.is_none() {
            return;
        }
        if let Some(t) = text {
            self.text.push(TextGlyph { outline: self.glyphs.len(), ..t });
        }
        self.glyphs.push(contours);
    }
    /// Apply a point transform to every vertex (an affine map: text glyphs keep their axes).
    pub fn map(&mut self, f: impl Fn(Vec2) -> Vec2) {
        for t in &mut self.text {
            let o = f(t.origin);
            t.x = f(t.origin + t.x) - o;
            t.y = f(t.origin + t.y) - o;
            t.origin = o;
        }
        for s in &mut self.strokes {
            for p in s.iter_mut() {
                *p = f(*p);
            }
        }
        for g in &mut self.glyphs {
            for c in g.iter_mut() {
                for p in c.iter_mut() {
                    *p = f(*p);
                }
            }
        }
    }
    /// Append another shaped piece (already in the same coordinates).
    pub fn extend(&mut self, other: Shaped) {
        let base = self.glyphs.len();
        self.text.extend(other.text.into_iter().map(|t| TextGlyph { outline: t.outline + base, ..t }));
        self.strokes.extend(other.strokes);
        self.glyphs.extend(other.glyphs);
    }
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.glyphs.is_empty()
    }
    /// All geometry as polylines (outlines become closed polylines).
    pub fn outlines(&self) -> Vec<Vec<Vec2>> {
        let mut v = self.strokes.clone();
        v.extend(self.glyphs.iter().flatten().cloned());
        v
    }
}

/// Underline / overline strokes from per-character spans `(x0, x1, underline, overline)`.
pub(crate) fn decorations(spans: &[(f64, f64, bool, bool)], h: f64) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    for (which, y) in [(0usize, -h * 0.2), (1usize, h * 1.2)] {
        let mut start: Option<f64> = None;
        let mut end = 0.0;
        for (x0, x1, u, o) in spans {
            let on = if which == 0 { *u } else { *o };
            match (on, start) {
                (true, None) => {
                    start = Some(*x0);
                    end = *x1;
                }
                (true, Some(_)) => end = *x1,
                (false, Some(sx)) => {
                    out.push(vec![Vec2::new(sx, y), Vec2::new(end, y)]);
                    start = None;
                }
                (false, None) => {}
            }
        }
        if let Some(sx) = start {
            out.push(vec![Vec2::new(sx, y), Vec2::new(end, y)]);
        }
    }
    out
}

/// Shape one line in `font` (baseline at y = 0, x from 0). TrueType fonts that fail to parse
/// fall back to the stroke font.
pub fn shape_line(font: &TextFont, s: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
    if let TextFont::Outline(face) = font
        && let Some(sh) = ttf::shape(face, s, height, width_factor, oblique)
    {
        return sh;
    }
    shape_stroke(s, height, width_factor, oblique)
}

/// Width of one line in `font`.
pub fn text_width(font: &TextFont, s: &str, height: f64, width_factor: f64) -> f64 {
    if let TextFont::Outline(face) = font
        && let Some(w) = ttf::width(face, s, height, width_factor)
    {
        return w;
    }
    line_width(s, height, width_factor)
}

/// Horizontal / vertical alignment for single-line text (matches DXF 72/73 semantics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    /// Fit between two points, scaling height.
    Aligned,
    /// Centred horizontally and vertically.
    Middle,
    /// Fit between two points, scaling width.
    Fit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Baseline,
    Bottom,
    Middle,
    Top,
}

/// Single-line text placement: insertion, alignment, rotation and the text style's
/// generation flags. Shared by the stroke and TrueType fonts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextParams {
    pub insert: Vec2,
    pub align_pt: Option<Vec2>,
    pub height: f64,
    pub rotation: f64,
    pub width_factor: f64,
    pub oblique: f64,
    pub h: Align,
    pub v: VAlign,
    /// Mirrored in X (text style "Backwards").
    pub backwards: bool,
    /// Mirrored in Y (text style "Upside down").
    pub upside_down: bool,
}

impl TextParams {
    pub fn new(insert: Vec2, height: f64) -> Self {
        TextParams {
            insert,
            align_pt: None,
            height,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            h: Align::Left,
            v: VAlign::Baseline,
            backwards: false,
            upside_down: false,
        }
    }
    /// Centred on `p` both ways (dimension and table text).
    pub fn centered(p: Vec2, height: f64, rotation: f64) -> Self {
        TextParams { align_pt: Some(p), rotation, h: Align::Middle, v: VAlign::Middle, ..TextParams::new(p, height) }
    }
}

/// Where a single-line TEXT's baseline starts (the left end of its first character's baseline),
/// in world coordinates: what DXF/DWG store as a justified text's insertion point (group 10),
/// computed from its alignment point (group 11) and its width in `font`.
pub fn baseline_start(font: &TextFont, s: &str, p: &TextParams) -> Vec2 {
    let (origin, local, rot, mirror) = frame(font, s, p);
    origin + Vec2::new(local.x * mirror.x, local.y * mirror.y).rotate(rot)
}

/// How far below the baseline a bottom-justified text's alignment point lies: a third of the
/// height for the stroke font (as AutoCAD places it with SHX fonts), the face's descent for an
/// outline font (#2229).
fn bottom_depth(font: &TextFont, height: f64) -> f64 {
    match font {
        TextFont::Outline(face) => ttf::descent_ratio(face).map_or(height / 3.0, |r| r * height),
        TextFont::Stroke => height / 3.0,
    }
}

/// The vertical middle of shaped text's ink (its strokes and outlines), above the baseline;
/// `None` when it draws nothing. A Middle (M) text is centred there: AutoCAD centres it on the
/// string's own extents, descenders included, not at half the height (#2229).
fn ink_middle(sh: &Shaped) -> Option<f64> {
    let ys = sh.strokes.iter().flatten().chain(sh.glyphs.iter().flatten().flatten()).map(|q| q.y).filter(|y| y.is_finite());
    let (lo, hi) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(y), hi.max(y)));
    (lo <= hi).then_some((lo + hi) / 2.0)
}

/// The offset of a line's baseline start from its alignment point, for justification `h`/`v`.
fn justify_offset(font: &TextFont, sh: &Shaped, height: f64, h: Align, v: VAlign) -> Vec2 {
    let dx = match h {
        Align::Left | Align::Aligned | Align::Fit => 0.0,
        Align::Center | Align::Middle => -sh.width / 2.0,
        Align::Right => -sh.width,
    };
    let dy = match (h, v) {
        (Align::Middle, _) => -ink_middle(sh).unwrap_or(height / 2.0),
        (_, VAlign::Baseline) => 0.0,
        (_, VAlign::Bottom) => bottom_depth(font, height),
        (_, VAlign::Middle) => -height / 2.0,
        (_, VAlign::Top) => -height,
    };
    Vec2::new(dx, dy)
}

/// How [`place`] puts a line in the world: the point it is placed by, the offset of its baseline
/// start from that point (along the text), its rotation and its mirroring (backwards, upside
/// down). Aligned and fit text are placed by their insertion point.
fn frame(font: &TextFont, s: &str, p: &TextParams) -> (Vec2, Vec2, f64, Vec2) {
    let height = if p.height > 0.0 && p.height.is_finite() { p.height } else { 1.0 };
    let rot = if p.rotation.is_finite() { p.rotation } else { 0.0 };
    if matches!(p.h, Align::Aligned | Align::Fit) {
        let rot = p.align_pt.map_or(rot, |p2| p.insert.angle_to(p2));
        return (p.insert, Vec2::ZERO, rot, Vec2::new(1.0, 1.0));
    }
    let sh = shape_line(font, s, height, p.width_factor, p.oblique);
    let origin = match (p.h, p.v) {
        (Align::Left, VAlign::Baseline) => p.insert,
        _ => p.align_pt.unwrap_or(p.insert),
    };
    let mirror = Vec2::new(if p.backwards { -1.0 } else { 1.0 }, if p.upside_down { -1.0 } else { 1.0 });
    (origin, justify_offset(font, &sh, height, p.h, p.v), rot, mirror)
}

/// Place a single-line TEXT in world coordinates with any font. Returns the shaped text and its
/// bounding box.
pub fn place(font: &TextFont, s: &str, p: &TextParams) -> (Shaped, Bounds2) {
    let mut height = if p.height > 0.0 && p.height.is_finite() { p.height } else { 1.0 };
    let mut wf = p.width_factor;
    let mut rot = if p.rotation.is_finite() { p.rotation } else { 0.0 };
    let insert = p.insert;
    if matches!(p.h, Align::Aligned | Align::Fit)
        && let Some(p2) = p.align_pt
    {
        let len = insert.dist(p2);
        rot = insert.angle_to(p2);
        let w = text_width(font, s, height, wf);
        if w > 1e-12 && len > 1e-12 {
            let k = len / w;
            if p.h == Align::Aligned {
                height *= k;
            } else {
                wf *= k;
            }
        }
    }
    let mut sh = shape_line(font, s, height, wf, p.oblique);
    let width = sh.width;
    // Every justification but baseline-left (and aligned/fit, which span both points) is placed by
    // its alignment point (DXF group 11); the insertion point (group 10) is then a computed point.
    let origin = match (p.h, p.v) {
        (Align::Left, VAlign::Baseline) | (Align::Aligned | Align::Fit, _) => insert,
        _ => p.align_pt.unwrap_or(insert),
    };
    let local = justify_offset(font, &sh, height, p.h, p.v);
    let (mx, my) = (if p.backwards { -1.0 } else { 1.0 }, if p.upside_down { -1.0 } else { 1.0 });
    let xf = |q: Vec2| {
        let l = q + local;
        origin + Vec2::new(l.x * mx, l.y * my).rotate(rot)
    };
    sh.map(xf);
    let depth = bottom_depth(font, height);
    let bb = Bounds2::from_points([Vec2::new(0.0, -depth), Vec2::new(width, -depth), Vec2::new(width, height), Vec2::new(0.0, height)].map(xf));
    (sh, bb)
}

/// Place a single-line TEXT in world coordinates with the stroke font. Returns strokes and the
/// text's bounding box.
#[allow(clippy::too_many_arguments)]
pub fn place_text(
    s: &str,
    insert: Vec2,
    align_pt: Option<Vec2>,
    height: f64,
    rotation: f64,
    width_factor: f64,
    oblique: f64,
    h: Align,
    v: VAlign,
) -> (Vec<Vec<Vec2>>, Bounds2) {
    let p = TextParams { insert, align_pt, height, rotation, width_factor, oblique, h, v, backwards: false, upside_down: false };
    let (sh, bb) = place(&TextFont::Stroke, s, &p);
    (sh.strokes, bb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ascii_printable_has_a_glyph() {
        for c in ' '..='~' {
            assert!(stroke::glyph(c).is_some(), "missing glyph {c:?}");
        }
    }

    #[test]
    fn glyph_points_stay_in_grid() {
        for c in ' '..='~' {
            let (w, spec) = stroke::glyph(c).unwrap();
            for st in stroke::strokes(spec) {
                for (x, y) in st {
                    assert!(x >= 0.0 && x <= w, "{c:?} x={x} w={w}");
                    assert!((-2.0..=6.0).contains(&y), "{c:?} y={y}");
                }
            }
        }
    }

    #[test]
    fn control_codes() {
        let d: String = decode_controls("45%%d %%p0.1 %%c10 100%%%").iter().map(|x| x.0).collect();
        assert_eq!(d, "45° ±0.1 ⌀10 100%");
        let u = decode_controls("%%uab%%uc");
        assert!(u[0].1 && u[1].1 && !u[2].1);
        assert_eq!(decode_controls("%%065")[0].0, 'A');
        assert_eq!(decode_controls("%%").len(), 2);
    }

    #[test]
    fn line_metrics_scale_with_height() {
        let a = layout_line("HELLO", 1.0, 1.0, 0.0);
        let b = layout_line("HELLO", 2.0, 1.0, 0.0);
        assert!((b.width - 2.0 * a.width).abs() < 1e-9);
        assert!((line_width("HELLO", 1.0, 1.0) - a.width).abs() < 1e-9);
    }

    #[test]
    fn center_alignment_centres_on_point() {
        let (_, bb) = place_text("ABC", Vec2::ZERO, Some(Vec2::new(10.0, 0.0)), 1.0, 0.0, 1.0, 0.0, Align::Center, VAlign::Baseline);
        assert!((bb.center().x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn rtl_text_justification_works_on_the_text_box() {
        // Right- and centre-justified right-to-left TEXT: the box sits on the alignment point
        // and every glyph stays inside it.
        for (h, want) in [(Align::Right, 10.0), (Align::Center, 10.0 - 0.5), (Align::Left, 0.0)] {
            let p = TextParams { h, align_pt: Some(Vec2::new(10.0, 0.0)), ..TextParams::new(Vec2::ZERO, 1.0) };
            let (sh, bb) = place(&TextFont::Stroke, "قیمت 25 تومان", &p);
            let edge = match h {
                Align::Right => bb.max.x,
                Align::Center => bb.center().x - 0.5,
                _ => bb.min.x,
            };
            assert!((edge - want).abs() < 1e-9, "{h:?}: {edge}");
            // An installed Arabic-script font may overhang its advance a little (ink outside
            // the box is normal); a tenth of the height keeps this independent of the machine.
            for q in sh.outlines().iter().flatten() {
                assert!(q.x >= bb.min.x - 0.1 && q.x <= bb.max.x + 0.1, "{h:?}: {} outside {:?}", q.x, bb);
            }
        }
    }

    #[test]
    fn fit_spans_both_points() {
        let (_, bb) = place_text("ABC", Vec2::ZERO, Some(Vec2::new(10.0, 0.0)), 1.0, 0.0, 1.0, 0.0, Align::Fit, VAlign::Baseline);
        assert!((bb.width() - 10.0).abs() < 1e-6);
        assert!((bb.max.y - 1.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod ttf_tests {
    #[test]
    fn system_font_outlines_when_available() {
        // Skips cleanly when no common system font is installed.
        let Some(face) = ["Arial", "Helvetica", "DejaVuSans", "Verdana", "LiberationSans-Regular"].iter().find_map(|n| crate::ttf::find(n)) else {
            return;
        };
        let run = crate::ttf::layout_line(&face, "CAD", 1.0, 1.0, 0.0).unwrap();
        assert!(run.strokes.len() >= 3, "glyph contours");
        assert!(run.width > 1.5 && run.width < 4.0, "width {}", run.width);
        let maxy = run.strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((maxy - 1.0).abs() < 0.1, "cap height ~ text height, got {maxy}");
    }

    #[test]
    fn stroke_names_are_not_ttf() {
        assert!(crate::ttf::find("txt.shx").is_none());
        assert!(crate::ttf::find("").is_none());
        let junk = crate::ttf::Face { bytes: std::sync::Arc::new(b"not a font".to_vec()), index: 0 };
        assert!(crate::ttf::layout_line(&junk, "x", 1.0, 1.0, 0.0).is_none());
    }

    #[test]
    fn fallback_ranking_prefers_wide_coverage_regular_fonts() {
        let stems: Vec<String> =
            ["zapfino", "dejavusans", "notosanscjk-bold", "notosanscjk-regular", "wqy-zenhei", "notosansarabic-regular", "arial"]
                .map(String::from)
                .to_vec();
        let r = crate::ttf::rank_fallback(&stems);
        assert_eq!(r.first().map(String::as_str), Some("notosanscjk-regular"));
        assert_eq!(r.get(1).map(String::as_str), Some("notosanscjk-bold"));
        assert_eq!(r.get(2).map(String::as_str), Some("wqy-zenhei"));
        assert_eq!(r.last().map(String::as_str), Some("zapfino"));
        assert_eq!(r.len(), stems.len());
    }

    #[test]
    fn missing_glyph_without_fallback_draws_a_box() {
        // Private-use code points never take a fallback font: the stroke font's box, no panic.
        let sh = crate::shape_stroke("A\u{E000}", 1.0, 1.0, 0.0);
        let sh2 = crate::shape_stroke("A\u{E000}B", 1.0, 1.0, 0.0);
        assert!(sh.glyphs.is_empty() && sh2.glyphs.is_empty());
        let boxed = sh.strokes.iter().any(|s| s.len() == 5 && s.first() == s.last() && s.iter().any(|p| (p.y - 1.0).abs() < 1e-9));
        assert!(boxed, "missing-glyph box");
        assert!((crate::line_width("A\u{E000}B", 1.0, 1.0) - sh2.width).abs() < 1e-9);
    }

    #[test]
    fn hostile_collections_never_panic() {
        // A TTC header claiming many faces with bogus or truncated offsets.
        let mut ttc = b"ttcf\x00\x01\x00\x00\x7f\xff\xff\xff".to_vec();
        ttc.extend_from_slice(&[0xff; 16]);
        for bytes in [ttc.clone(), ttc[..10].to_vec(), b"ttcf".to_vec(), Vec::new()] {
            let arc = std::sync::Arc::new(bytes);
            assert!(crate::ttf::faces(&arc).is_empty());
            assert!(crate::ttf::Face::new(arc.clone(), 3).is_none());
            assert!(!crate::ttf::Face { bytes: arc, index: u32::MAX }.has_glyph('a'));
        }
        crate::ttf::register("cadcraft-hostile-test.ttc", ttc);
        assert!(crate::ttf::find("cadcraft-hostile-test.ttc").is_none());
        let _ = crate::ttf::fallback_for('\u{4E2D}');
    }

    #[test]
    fn font_alt_that_is_not_installed_keeps_the_stroke_font() {
        crate::ttf::set_fallback_fonts(" no-such-font-130 ; . ,other-missing ");
        assert_eq!(crate::ttf::fallback_fonts(), "no-such-font-130, other-missing");
        crate::ttf::set_fallback_fonts("");
        crate::ttf::set_font_alt("no-such-font-130");
        assert_eq!(crate::ttf::font_alt(), "no-such-font-130");
        assert!(!crate::TextFont::resolve("romans.shx").is_outline());
        crate::ttf::set_font_alt(".");
        assert_eq!(crate::ttf::font_alt(), "");
    }

    #[test]
    fn cjk_falls_back_to_an_installed_font_when_available() {
        // Skips cleanly when no installed font covers CJK.
        if crate::ttf::fallback_for('\u{4E2D}').is_none() {
            return;
        }
        let sh = crate::shape_stroke("AB\u{4E2D}\u{6587}", 1.0, 1.0, 0.0);
        assert_eq!(sh.glyphs.len(), 2, "two fallback glyphs");
        assert!(!sh.strokes.iter().any(|s| s.len() == 5 && s.first() == s.last()), "no missing-glyph boxes");
        assert!((crate::line_width("AB\u{4E2D}\u{6587}", 1.0, 1.0) - sh.width).abs() < 1e-9);
    }

    /// #674: CJK characters in stroke-font (SHX) text fill a square cell from the baseline to the
    /// text height, as a big font's do, instead of being sized by the fallback font's Latin cap
    /// height (which made them about 1.4 times too large and too widely spaced).
    #[test]
    fn cjk_in_stroke_text_fills_a_square_cell_like_a_big_font() {
        if crate::ttf::fallback_for('\u{4E2D}').is_none() {
            return;
        }
        let (h, wf) = (2.5, 0.8);
        let one = crate::shape_stroke("\u{4E2D}", h, wf, 0.0);
        let four = crate::shape_stroke("\u{4E2D}\u{6587}\u{5B57}\u{4F53}", h, wf, 0.0);
        let pts: Vec<_> = four.glyphs.iter().flatten().flatten().copied().collect();
        let (miny, maxy) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
        assert!(miny > -0.1 * h && maxy < 1.05 * h && maxy > 0.75 * h, "glyphs span {miny}..{maxy} for height {h}");
        let cell = h * wf;
        assert!(
            (one.width - cell).abs() < 0.05 * cell && (four.width - 4.0 * cell).abs() < 0.2 * cell,
            "widths {} and {}, cell {cell}",
            one.width,
            four.width
        );
        assert!((crate::line_width("\u{4E2D}\u{6587}\u{5B57}\u{4F53}", h, wf) - four.width).abs() < 1e-9);
        assert!(crate::is_wide('\u{3042}') && crate::is_wide('\u{AC00}') && crate::is_wide('\u{FF21}') && !crate::is_wide('A'));
    }
}

#[cfg(test)]
mod justify_tests {
    use super::*;

    const H: f64 = 2.5;

    /// `s` placed by the alignment point (0, 0) with justification `h`/`v` in `font`.
    fn placed(font: &TextFont, s: &str, h: Align, v: VAlign) -> (Shaped, Vec2) {
        let p = TextParams { align_pt: Some(Vec2::ZERO), h, v, ..TextParams::new(Vec2::ZERO, H) };
        (place(font, s, &p).0, baseline_start(font, s, &p))
    }

    /// The lowest and highest point of placed text's ink.
    fn ink_y(sh: &Shaped) -> (f64, f64) {
        let ys = sh.strokes.iter().flatten().chain(sh.glyphs.iter().flatten().flatten()).map(|q| q.y);
        ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(y), hi.max(y)))
    }

    /// A Middle (M) text is centred on its own ink, descenders included, as AutoCAD centres it,
    /// not at half the height: lowercase and descenders move the baseline (#2229).
    #[test]
    fn a_middle_text_is_centred_on_its_ink() {
        let font = TextFont::Stroke;
        let mut baselines = Vec::new();
        for s in ["ABC 123", "gjpqy", "Tg 1234 ABC", "ace"] {
            let (sh, start) = placed(&font, s, Align::Middle, VAlign::Baseline);
            let (lo, hi) = ink_y(&sh);
            assert!(((lo + hi) / 2.0).abs() < 1e-9, "{s}: ink {lo}..{hi} is centred on the alignment point");
            // The insertion point a DWG keeps (group 10) is the baseline start, below the ink's
            // middle by as much as the unplaced text's ink middle is above its baseline.
            let (lo0, hi0) = ink_y(&shape_line(&font, s, H, 1.0, 0.0));
            assert!((start.y + (lo0 + hi0) / 2.0).abs() < 1e-9, "{s}: baseline start {start:?}");
            baselines.push((s, start.y));
        }
        // Capitals and digits span the height, so they stay at half of it.
        assert!((baselines[0].1 + H / 2.0).abs() < 1e-9, "{baselines:?}");
        // Descenders, or lowercase alone, bring the ink's middle down, so the baseline sits higher.
        for (s, y) in &baselines[1..] {
            if *s != "Tg 1234 ABC" {
                assert!(*y > -H / 2.0 + 0.1, "{s}: {baselines:?}");
            }
        }
    }

    /// Text that draws nothing has no ink to centre: it keeps half the height.
    #[test]
    fn a_middle_text_without_ink_keeps_half_the_height() {
        let (_, start) = placed(&TextFont::Stroke, " ", Align::Middle, VAlign::Baseline);
        assert!((start.y + H / 2.0).abs() < 1e-9, "{start:?}");
    }

    /// With the stroke font (as with AutoCAD's SHX fonts), a bottom-justified text's baseline is
    /// a third of the height above its alignment point, and Middle Center still uses half.
    #[test]
    fn stroke_font_bottom_and_middle_center_are_unchanged() {
        let font = TextFont::Stroke;
        for h in [Align::Left, Align::Center, Align::Right] {
            let (_, start) = placed(&font, "gjpqy", h, VAlign::Bottom);
            assert!((start.y - H / 3.0).abs() < 1e-9, "{h:?}: {start:?}");
        }
        let (_, start) = placed(&font, "gjpqy", Align::Center, VAlign::Middle);
        assert!((start.y + H / 2.0).abs() < 1e-9, "MC: {start:?}");
    }

    /// With a TrueType font, a bottom-justified text's alignment point is at the face's descent,
    /// where its descenders end, as AutoCAD places it (#2229: Arial's is 0.296 of the height).
    /// Uses the first of these common fonts that is installed (Windows, macOS and Linux each have
    /// one); skips without any.
    #[test]
    fn a_bottom_justified_truetype_text_sits_on_its_descent() {
        let found = ["Arial", "Helvetica", "DejaVuSans", "Verdana", "LiberationSans-Regular"].into_iter().find_map(|n| ttf::find(n).map(|f| (n, f)));
        let Some((name, face)) = found else { return };
        let ratio = ttf::descent_ratio(&face).unwrap();
        let font = TextFont::Outline(face);
        let (sh, start) = placed(&font, "gjpqy", Align::Left, VAlign::Bottom);
        assert!((start.y - ratio * H).abs() < 1e-9, "{name}: {start:?}");
        if name == "Arial" {
            assert!((ratio - 0.296).abs() < 0.01, "Arial's descent over cap height: {ratio}");
            // The descenders reach the alignment point's line: h/3 left them 0.1 above it.
            let (lo, _) = ink_y(&sh);
            assert!(lo.abs() < 0.02 * H, "descender bottom {lo} from the alignment point");
        }
        // And Middle centres the ink, as with the stroke font.
        let (sh, _) = placed(&font, "Tg 1234 ABC", Align::Middle, VAlign::Baseline);
        let (lo, hi) = ink_y(&sh);
        assert!(((lo + hi) / 2.0).abs() < 1e-9, "{name}: {lo}..{hi}");
    }
}
