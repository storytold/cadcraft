//! Dimension geometry generation from definition points and a dimension style: extension
//! lines (DIMEXO/DIMEXE, DIMSE1/2), arrowheads (DIMBLK/DIMBLK1/DIMBLK2, drawn in code) or ticks
//! (DIMTSZ, DIMDLE), fit (DIMATFIT, DIMTIX, DIMSOXD, DIMTOFL), suppressed halves (DIMSD1/2),
//! fixed-length extension lines (DIMFXL/DIMFXLON), text placement (DIMTAD, DIMTVP, DIMJUST,
//! DIMTIH/DIMTOH, DIMGAP, DIMTMOVE) and text formatting (DIMLUNIT, DIMDEC, DIMZIN, DIMRND,
//! DIMPOST, DIMTOL/DIMLIM with DIMTZIN and DIMTOLJ, DIMALT with DIMALTU/DIMALTZ/DIMALTRND,
//! DIMAZIN, DIMARCSYM) with its background (DIMTFILL).

use cadcraft_doc::{DimKind, DimStyle, Dimension};
use cadcraft_fonts::{MTextParams, TextFont};
use cadcraft_geom::Vec2;

use crate::units::{NumFormat, format_decimal, format_linear};

mod arrowheads;
mod text;

use arrowheads::tick;
pub use arrowheads::{ArrowGeom, Arrowhead, arrow, arrowhead};
pub use text::{DimText, format_linear_value};

/// Most DIMBREAK gaps drawn per dimension.
const MAX_BREAKS: usize = 1000;

/// What a dimension line primitive is (for DIMCLRD / DIMCLRE colours).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineRole {
    /// Dimension line, arrowheads, ticks, leaders (DIMCLRD).
    Dim,
    /// Extension lines and centre marks (DIMCLRE).
    Ext,
}

#[derive(Clone, Debug, Default)]
pub struct DimGeometry {
    /// Every line (dimension and extension lines, open arrowheads, ticks).
    pub lines: Vec<Vec<Vec2>>,
    /// Role of each entry of `lines`.
    pub line_roles: Vec<LineRole>,
    /// Filled triangles (three points each) in the dimension line colour.
    pub fills: Vec<Vec<Vec2>>,
    /// DIMTFILL 1 or 2: the corners of the text's background, the text box widened by DIMGAP
    /// on every side (empty without a fill or text).
    pub text_fill: Vec<Vec2>,
    /// Indices into `lines` of the marks drawn with the text, over `text_fill`: the arc length
    /// symbol above the text (DIMARCSYM 1) and the DIMINSPECT frame.
    pub text_marks: Vec<usize>,
    /// Text strokes (stroke font, fraction bars, underlines).
    pub text: Vec<Vec<Vec2>>,
    /// Text glyph outlines (TrueType), grouped per glyph.
    pub text_glyphs: Vec<Vec<Vec<Vec2>>>,
    /// The font glyphs of `text_glyphs` (outline indices into it).
    pub text_runs: Vec<cadcraft_fonts::TextGlyph>,
    pub text_pos: Vec2,
    /// The measured value (DXF group 42): a length, or the angle in radians.
    pub measurement: f64,
    pub text_angle: f64,
    pub text_height: f64,
    /// Plain text of the dimension.
    pub value: String,
    /// The text as MTEXT contents (stacked fractions and tolerances).
    pub mtext: String,
    /// Indices into `lines` of the second extension line (DIMLTEX2); the other extension
    /// lines take DIMLTEX1.
    pub ext2_lines: Vec<usize>,
}

impl DimGeometry {
    /// `value` as the contents of a plain MTEXT (the dimension's `*D` block, its EXPLODE
    /// pieces): the diameter sign as `%%c`, not the decoded ⌀ (U+2300), which no SHX font has,
    /// so other programs plot nothing for it, while every font draws `%%c` (#1645). A `%`
    /// that would run into a code is written `%%%`.
    pub fn mtext_contents(&self) -> String {
        let mut out = String::with_capacity(self.value.len() + 2);
        let mut chars = self.value.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '⌀' => out.push_str("%%c"),
                '%' if matches!(chars.peek(), Some('%' | '⌀')) => out.push_str("%%%"),
                c => out.push(c),
            }
        }
        out
    }
    fn dim(&mut self, l: Vec<Vec2>) {
        if l.len() >= 2 {
            self.lines.push(l);
            self.line_roles.push(LineRole::Dim);
        }
    }
    /// A dimension line that marks the text (see `text_marks`).
    fn text_mark(&mut self, l: Vec<Vec2>) {
        if l.len() >= 2 {
            self.text_marks.push(self.lines.len());
            self.dim(l);
        }
    }
    fn ext(&mut self, l: Vec<Vec2>) {
        if l.len() >= 2 {
            self.lines.push(l);
            self.line_roles.push(LineRole::Ext);
        }
    }
    /// Leave out the parts of the lines (of `role`, or all) within `r` of `c`.
    fn cut(&mut self, c: Vec2, r: f64, role: Option<LineRole>) {
        if !(r > 0.0 && r.is_finite() && c.is_finite()) {
            return;
        }
        let mut lines = Vec::with_capacity(self.lines.len());
        let mut roles = Vec::with_capacity(self.lines.len());
        let (mut ext2, mut marks) = (Vec::new(), Vec::new());
        for (i, (l, rl)) in self.lines.iter().zip(&self.line_roles).enumerate() {
            let pieces = if role.is_none_or(|x| x == *rl) { cut_polyline(l, c, r) } else { vec![l.clone()] };
            for p in pieces {
                if self.ext2_lines.contains(&i) {
                    ext2.push(lines.len());
                }
                if self.text_marks.contains(&i) {
                    marks.push(lines.len());
                }
                lines.push(p);
                roles.push(*rl);
            }
        }
        (self.lines, self.line_roles, self.ext2_lines, self.text_marks) = (lines, roles, ext2, marks);
    }
    /// Extension line 1 or 2 (`second`).
    fn ext_n(&mut self, l: Option<Vec<Vec2>>, second: bool) {
        if let Some(l) = l.filter(|l| l.len() >= 2) {
            if second {
                self.ext2_lines.push(self.lines.len());
            }
            self.ext(l);
        }
    }
    fn fill(&mut self, tris: Vec<Vec2>) {
        for t in tris.chunks(3) {
            if t.len() == 3 {
                self.fills.push(t.to_vec());
            }
        }
    }
    /// The lines with a role.
    pub fn lines_of(&self, role: LineRole) -> impl Iterator<Item = &Vec<Vec2>> {
        self.lines.iter().zip(&self.line_roles).filter(move |(_, r)| **r == role).map(|(l, _)| l)
    }
    fn add_arrow(&mut self, a: ArrowGeom) {
        for l in a.lines {
            self.dim(l);
        }
        self.fill(a.tris);
    }
}

/// The pieces of a polyline outside the circle (`c`, `r`).
fn cut_polyline(l: &[Vec2], c: Vec2, r: f64) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    let mut keep = |a: Vec2, b: Vec2| {
        if a.dist(b) <= 1e-12 {
            return;
        }
        match out.last_mut() {
            Some(last) if last.last() == Some(&a) => last.push(b),
            _ => out.push(vec![a, b]),
        }
    };
    for w in l.windows(2) {
        let (p, q) = (w[0], w[1]);
        let dv = q - p;
        let (qa, f) = (dv.dot(dv), p - c);
        let (qb, qc) = (2.0 * f.dot(dv), f.dot(f) - r * r);
        let disc = qb * qb - 4.0 * qa * qc;
        if qa < 1e-24 || disc.is_nan() || disc <= 0.0 {
            keep(p, q);
            continue;
        }
        let sq = disc.sqrt();
        let (t0, t1) = ((-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa));
        if t1 <= 0.0 || t0 >= 1.0 {
            keep(p, q);
            continue;
        }
        if t0 > 0.0 {
            keep(p, p + dv * t0);
        }
        if t1 < 1.0 {
            keep(p + dv * t1, q);
        }
    }
    out
}

fn ext_line(origin: Vec2, foot: Vec2, st: &DimStyle, k: f64) -> Option<Vec<Vec2>> {
    let v = foot - origin;
    let len = v.len();
    if len < 1e-12 {
        return None;
    }
    let u = v / len;
    let mut start = origin + u * (st.ext_offset * k);
    // DIMFXLON: a fixed length from the dimension line toward the origin (never past the offset).
    let fixed = st.fixed_ext_length * k;
    if st.fixed_ext && fixed.is_finite() && fixed >= 0.0 && fixed < len - st.ext_offset * k {
        start = foot - u * fixed;
    }
    Some(vec![start, foot + u * (st.ext_extend * k)])
}

/// An angle reduced to the readable range (text never upside down).
fn readable(a: f64) -> f64 {
    let mut a = cadcraft_geom::norm_angle(a);
    if a > std::f64::consts::FRAC_PI_2 + 1e-9 && a <= 3.0 * std::f64::consts::FRAC_PI_2 + 1e-9 {
        a -= std::f64::consts::PI;
    }
    a
}

struct DimConfig<'a> {
    dimension: &'a Dimension,
    style: &'a DimStyle,
    font: &'a DimText,
    scale: f64,
    arrow_size: f64,
    text_height: f64,
    gap: f64,
    tick_size: f64,
    ticks: bool,
    first_arrow: Arrowhead,
    second_arrow: Arrowhead,
    width_factor: f64,
}

impl<'a> DimConfig<'a> {
    fn new(dimension: &'a Dimension, style: &'a DimStyle, dimscale: f64, font: &'a DimText) -> Self {
        let scale = style.effective_scale(dimscale);
        let arrow_size = style.arrow_size * scale;
        let text_height = if font.fixed_height > 0.0 { font.fixed_height } else { style.text_height * scale };
        let tick_size = style.tick_size * scale;
        let first_arrow = if style.arrow_block1.is_empty() { &style.arrow_block } else { &style.arrow_block1 };
        let second_arrow = if style.arrow_block2.is_empty() { &style.arrow_block } else { &style.arrow_block2 };
        Self {
            dimension,
            style,
            font,
            scale,
            arrow_size,
            text_height,
            gap: style.text_gap.abs() * scale,
            tick_size,
            ticks: tick_size > 0.0,
            first_arrow: Arrowhead::parse(first_arrow),
            second_arrow: Arrowhead::parse(second_arrow),
            width_factor: if font.width_factor > 0.0 && font.width_factor.is_finite() { font.width_factor } else { 1.0 },
        }
    }

    fn text_params(&self, at: Vec2, rotation: f64) -> MTextParams {
        MTextParams {
            attach: 5,
            rotation,
            font: self.font.font.clone(),
            width_factor: self.width_factor,
            oblique: self.font.oblique,
            ..MTextParams::new(at, self.text_height)
        }
    }

    fn measure(&self, text: &str) -> (f64, f64) {
        if text.is_empty() {
            return (0.0, 0.0);
        }
        let layout = cadcraft_fonts::layout_mtext_with(text, &self.text_params(Vec2::ZERO, 0.0));
        (layout.bounds.width(), layout.bounds.height())
    }

    fn end(&self, geometry: &mut DimGeometry, kind: Arrowhead, tip: Vec2, direction: Vec2) -> f64 {
        if self.ticks {
            geometry.dim(tick(tip, direction, self.tick_size));
            0.0
        } else {
            geometry.add_arrow(arrowhead(kind, tip, direction, self.arrow_size));
            kind.inset(self.arrow_size)
        }
    }
}

/// Generate the geometry of a dimension with the stroke font.
pub fn dimension_geometry(d: &Dimension, st: &DimStyle, dimscale: f64) -> DimGeometry {
    dimension_geometry_with(d, st, dimscale, &DimText { width_factor: 1.0, ..Default::default() })
}

/// Generate the geometry of a dimension. The dimension's own overrides are applied to `st`.
pub fn dimension_geometry_with(d: &Dimension, st: &DimStyle, dimscale: f64, font: &DimText) -> DimGeometry {
    let overridden = st.with_overrides(&d.overrides);
    let config = DimConfig::new(d, &overridden, dimscale, font);
    let st = config.style;
    let th = config.text_height;
    let gap = config.gap;
    let mut g = DimGeometry { text_height: th, ..Default::default() };
    let params = |at: Vec2, rotation: f64| config.text_params(at, rotation);
    let measure = |text: &str| config.measure(text);
    let mut text_angle = 0.0;
    let mut text_mid = d.text_mid.xy();
    // Place an end marker: tick or arrowhead. Returns how far the dimension line stops short.
    match d.kind {
        DimKind::Linear { .. } | DimKind::Aligned => linear::draw_linear(&config, &mut g, &mut text_angle, &mut text_mid),
        DimKind::Radius if d.marks.jogged.is_some() => variants::draw_jogged_radius(&config, &mut g, &mut text_angle, &mut text_mid),
        DimKind::Radius | DimKind::Diameter => variants::draw_radius_or_diameter(&config, &mut g, &mut text_angle, &mut text_mid),
        DimKind::Angular | DimKind::Angular3P => variants::draw_angular(&config, &mut g, &mut text_angle, &mut text_mid),
        DimKind::Ordinate { x_type } => variants::draw_ordinate(&config, &mut g, &mut text_angle, &mut text_mid, x_type),
        DimKind::ArcLength => variants::draw_arc_length(&config, &mut g, &mut text_angle, &mut text_mid),
    }
    // DIMTEDIT Angle: an explicit text rotation wins.
    if d.text_rotation != 0.0 && d.text_rotation.is_finite() {
        text_angle = d.text_rotation;
    }
    // DIMBREAK: gaps in the dimension and extension lines.
    for b in d.marks.breaks.iter().take(MAX_BREAKS) {
        g.cut(b.at.xy(), b.size / 2.0, None);
    }
    // DIMINSPECT: a frame round the text, with separators between label, value and rate.
    if let Some(ins) = &d.marks.inspect
        && ins.shape < 2
        && !g.mtext.is_empty()
    {
        let (w, h) = measure(&g.mtext);
        let (x, y) = (Vec2::from_angle(text_angle), Vec2::from_angle(text_angle).perp());
        let at = |px: f64, py: f64| text_mid + x * px + y * py;
        let (hw, hh) = (w / 2.0 + gap, h / 2.0 + gap);
        // Widths of the label, value and rate as laid out: the two spaces between parts measured
        // in "x  x", the value is what the rest leaves.
        let gap2 = (measure("x  x").0 - 2.0 * measure("x").0).max(0.0);
        let (wl, wr) = (measure(&ins.label).0, measure(&ins.rate).0);
        let seps = usize::from(!ins.label.is_empty()) + usize::from(!ins.rate.is_empty());
        let wv = (w - wl - wr - gap2 * seps as f64).max(0.0);
        let mut left = -w / 2.0;
        if !ins.label.is_empty() {
            left += wl;
            g.text_mark(vec![at(left + gap2 / 2.0, -hh), at(left + gap2 / 2.0, hh)]);
            left += gap2;
        }
        if !ins.rate.is_empty() {
            left += wv;
            g.text_mark(vec![at(left + gap2 / 2.0, -hh), at(left + gap2 / 2.0, hh)]);
        }
        g.text_mark(vec![at(-hw, hh), at(hw, hh)]);
        g.text_mark(vec![at(-hw, -hh), at(hw, -hh)]);
        for side in [-1.0, 1.0] {
            let ends: Vec<Vec2> = if ins.shape == 0 {
                (0..=12).map(|i| f64::from(i) * std::f64::consts::PI / 12.0).map(|a| at(side * (hw + hh * a.sin()), hh * a.cos())).collect()
            } else {
                vec![at(side * hw, hh), at(side * (hw + hh), 0.0), at(side * hw, -hh)]
            };
            g.text_mark(ends);
        }
    }
    // DIMTFILL: a background behind the text, the text box plus DIMGAP on every side.
    if st.text_fill != 0 && !g.mtext.is_empty() {
        let (w, h) = measure(&g.mtext);
        let (x, y) = (Vec2::from_angle(text_angle), Vec2::from_angle(text_angle).perp());
        let (hw, hh) = (w / 2.0 + gap, h / 2.0 + gap);
        g.text_fill = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].into_iter().map(|(px, py)| text_mid + x * px + y * py).collect();
    }
    g.value = decode_value(&cadcraft_fonts::plain_mtext(&g.mtext));
    if !g.mtext.is_empty() {
        let l = cadcraft_fonts::layout_mtext_with(&g.mtext, &params(text_mid, text_angle));
        for p in l.pieces {
            let base = g.text_glyphs.len();
            g.text_runs.extend(p.shaped.text.into_iter().map(|t| cadcraft_fonts::TextGlyph { outline: t.outline + base, ..t }));
            g.text.extend(p.shaped.strokes);
            g.text_glyphs.extend(p.shaped.glyphs);
        }
    }
    g.text_pos = text_mid;
    g.text_angle = text_angle;
    g
}

mod linear;
mod variants;
/// `%%` codes in the plain value become their characters (`%%c` → ⌀, `%%p` → ±, `%%d` → °).
fn decode_value(s: &str) -> String {
    cadcraft_fonts::decode_controls(s).into_iter().map(|(c, _, _)| c).collect()
}

#[cfg(test)]
mod tests;
