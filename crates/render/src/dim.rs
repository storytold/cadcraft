//! Dimension geometry generation from definition points and a dimension style: extension
//! lines (DIMEXO/DIMEXE, DIMSE1/2), arrowheads (DIMBLK/DIMBLK1/DIMBLK2, drawn in code) or ticks
//! (DIMTSZ, DIMDLE), fit (DIMATFIT, DIMTIX, DIMSOXD, DIMTOFL), suppressed halves (DIMSD1/2),
//! fixed-length extension lines (DIMFXL/DIMFXLON), text placement (DIMTAD, DIMTVP, DIMJUST,
//! DIMTIH/DIMTOH, DIMGAP, DIMTMOVE) and text formatting (DIMLUNIT, DIMDEC, DIMZIN, DIMRND,
//! DIMPOST, DIMTOL/DIMLIM with DIMTZIN and DIMTOLJ, DIMALT with DIMALTU/DIMALTZ/DIMALTRND,
//! DIMAZIN, DIMARCSYM). DIMPOST wraps angles as well, as AutoCAD does (#1653).

use cadcraft_doc::{DimKind, DimStyle, Dimension};
use cadcraft_fonts::{MTextParams, TextFont};
use cadcraft_geom::{Arc, Vec2, ccw_sweep};

use crate::units::{NumFormat, format_angle, format_decimal, format_linear};

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
    fn dim(&mut self, l: Vec<Vec2>) {
        if l.len() >= 2 {
            self.lines.push(l);
            self.line_roles.push(LineRole::Dim);
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
        let mut ext2 = Vec::new();
        for (i, (l, rl)) in self.lines.iter().zip(&self.line_roles).enumerate() {
            let pieces = if role.is_none_or(|x| x == *rl) { cut_polyline(l, c, r) } else { vec![l.clone()] };
            for p in pieces {
                if self.ext2_lines.contains(&i) {
                    ext2.push(lines.len());
                }
                lines.push(p);
                roles.push(*rl);
            }
        }
        (self.lines, self.line_roles, self.ext2_lines) = (lines, roles, ext2);
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

/// The arrowheads drawn in code (DIMBLK names; our own geometry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrowhead {
    ClosedFilled,
    ClosedBlank,
    Closed,
    Dot,
    DotSmall,
    DotBlank,
    Oblique,
    ArchTick,
    Open,
    Open90,
    Open30,
    Origin,
    Origin2,
    Small,
    BoxFilled,
    BoxBlank,
    DatumFilled,
    DatumBlank,
    Integral,
    None,
}

impl Arrowhead {
    /// Parse a DIMBLK value ("" / "_DOT" / "Architectural tick" / "closed blank"…).
    pub fn parse(name: &str) -> Arrowhead {
        let n: String = name.trim().trim_start_matches('_').chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match n.as_str() {
            "" | "closedfilled" | "filled" => Arrowhead::ClosedFilled,
            "closedblank" | "blank" => Arrowhead::ClosedBlank,
            "closed" => Arrowhead::Closed,
            "dot" => Arrowhead::Dot,
            "dotsmall" => Arrowhead::DotSmall,
            "dotblank" => Arrowhead::DotBlank,
            "oblique" => Arrowhead::Oblique,
            "archtick" | "architecturaltick" | "tick" => Arrowhead::ArchTick,
            "open" => Arrowhead::Open,
            "open90" | "rightangle" => Arrowhead::Open90,
            "open30" => Arrowhead::Open30,
            "origin" | "originindicator" => Arrowhead::Origin,
            "origin2" | "originindicator2" => Arrowhead::Origin2,
            "small" => Arrowhead::Small,
            "boxfilled" => Arrowhead::BoxFilled,
            "boxblank" | "box" => Arrowhead::BoxBlank,
            "datumfilled" | "datumtrianglefilled" => Arrowhead::DatumFilled,
            "datumblank" | "datumtriangle" => Arrowhead::DatumBlank,
            "integral" => Arrowhead::Integral,
            "none" => Arrowhead::None,
            _ => Arrowhead::ClosedFilled,
        }
    }
    /// Ticks and obliques: the dimension line runs through the tip and extends by DIMDLE.
    pub fn is_tick(self) -> bool {
        matches!(self, Arrowhead::Oblique | Arrowhead::ArchTick | Arrowhead::Integral | Arrowhead::None)
    }
    /// How far back from the tip the dimension line stops (blank heads are not crossed).
    pub fn inset(self, size: f64) -> f64 {
        match self {
            Arrowhead::ClosedBlank | Arrowhead::DatumBlank => size,
            Arrowhead::DotBlank => size / 4.0,
            Arrowhead::Origin | Arrowhead::Origin2 => size / 2.0,
            Arrowhead::BoxBlank => size / 4.0,
            _ => 0.0,
        }
    }
}

/// Arrowhead geometry: open polylines and filled triangles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrowGeom {
    pub lines: Vec<Vec<Vec2>>,
    pub tris: Vec<Vec2>,
}

fn circle_pts(c: Vec2, r: f64) -> Vec<Vec2> {
    (0..=24).map(|i| c + Vec2::from_angle(f64::from(i) * cadcraft_geom::TAU / 24.0) * r).collect()
}

fn disc(c: Vec2, r: f64) -> Vec<Vec2> {
    let mut t = Vec::with_capacity(72);
    for i in 0..24 {
        let a0 = f64::from(i) * cadcraft_geom::TAU / 24.0;
        let a1 = f64::from(i + 1) * cadcraft_geom::TAU / 24.0;
        t.extend_from_slice(&[c, c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a1) * r]);
    }
    t
}

/// An arrowhead at `tip` pointing along `dir` (unit), `size` = DIMASZ × scale.
pub fn arrowhead(kind: Arrowhead, tip: Vec2, dir: Vec2, size: f64) -> ArrowGeom {
    let dir = if dir == Vec2::ZERO || !dir.is_finite() { Vec2::X } else { dir };
    let size = if size.is_finite() { size.abs() } else { 0.0 };
    let n = dir.perp();
    let wings = |half_angle: f64, len: f64| {
        let back = -dir;
        (tip + back.rotate(half_angle) * len, tip + back.rotate(-half_angle) * len)
    };
    let mut g = ArrowGeom::default();
    match kind {
        Arrowhead::ClosedFilled => g.tris = arrow(tip, dir, size),
        Arrowhead::Small => g.tris = arrow(tip, dir, size / 2.0),
        Arrowhead::ClosedBlank | Arrowhead::Closed => {
            let back = tip - dir * size;
            let w = n * (size / 6.0);
            g.lines.push(vec![tip, back + w, back - w, tip]);
            if kind == Arrowhead::Closed {
                g.lines.push(vec![back, tip]);
            }
        }
        Arrowhead::Dot => g.tris = disc(tip, size / 4.0),
        Arrowhead::DotSmall => g.tris = disc(tip, size / 8.0),
        Arrowhead::DotBlank => g.lines.push(circle_pts(tip, size / 4.0)),
        Arrowhead::Origin => g.lines.push(circle_pts(tip, size / 2.0)),
        Arrowhead::Origin2 => {
            g.lines.push(circle_pts(tip, size / 2.0));
            g.lines.push(circle_pts(tip, size / 4.0));
        }
        Arrowhead::Oblique => {
            let t = canonical(dir + n).normalized() * (size / 2.0);
            g.lines.push(vec![tip - t, tip + t]);
        }
        Arrowhead::ArchTick => {
            // A heavy 45° stroke: a thin filled parallelogram.
            let t = canonical(dir + n).normalized() * (size / 2.0);
            let w = t.perp().normalized() * (size / 16.0);
            let (a, b) = (tip - t, tip + t);
            g.tris = vec![a - w, b - w, b + w, a - w, b + w, a + w];
        }
        Arrowhead::Integral => {
            // A small integral-sign wave through the tip.
            let t = canonical(dir + n).normalized();
            let pts = (-8..=8)
                .map(|i| {
                    let f = f64::from(i) / 8.0;
                    tip + t * (f * size / 2.0) + t.perp() * ((f * std::f64::consts::PI).sin() * size * 0.12)
                })
                .collect();
            g.lines.push(pts);
        }
        Arrowhead::Open => {
            let (a, b) = wings((1.0f64 / 6.0).atan(), size);
            g.lines.push(vec![a, tip, b]);
        }
        Arrowhead::Open90 => {
            let (a, b) = wings(std::f64::consts::FRAC_PI_4, size / std::f64::consts::SQRT_2);
            g.lines.push(vec![a, tip, b]);
        }
        Arrowhead::Open30 => {
            let (a, b) = wings(15f64.to_radians(), size);
            g.lines.push(vec![a, tip, b]);
        }
        Arrowhead::BoxFilled | Arrowhead::BoxBlank => {
            let h = size / 4.0;
            let (u, v) = (dir * h, n * h);
            let q = [tip - u - v, tip + u - v, tip + u + v, tip - u + v];
            if kind == Arrowhead::BoxFilled {
                g.tris = vec![q[0], q[1], q[2], q[0], q[2], q[3]];
            } else {
                g.lines.push(vec![q[0], q[1], q[2], q[3], q[0]]);
            }
        }
        Arrowhead::DatumFilled | Arrowhead::DatumBlank => {
            let apex = tip - dir * size;
            let w = n * (size / 2.0);
            if kind == Arrowhead::DatumFilled {
                g.tris = vec![tip + w, tip - w, apex];
            } else {
                g.lines.push(vec![tip + w, tip - w, apex, tip + w]);
            }
        }
        Arrowhead::None => {}
    }
    g
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

/// A closed filled arrowhead at `tip` pointing along `dir` (unit), as one triangle.
pub fn arrow(tip: Vec2, dir: Vec2, size: f64) -> Vec<Vec2> {
    let back = tip - dir * size;
    let n = dir.perp() * (size / 6.0);
    vec![tip, back + n, back - n]
}

/// Ticks lean the same way at both ends of a dimension line whichever way they point.
fn canonical(v: Vec2) -> Vec2 {
    if v.x < -1e-12 || (v.x.abs() <= 1e-12 && v.y < 0.0) { -v } else { v }
}

/// A DIMTSZ tick: an oblique stroke centred on `at`.
fn tick(at: Vec2, dir: Vec2, half: f64) -> Vec<Vec2> {
    let d = canonical(dir);
    let t = (d + d.perp()).normalized() * half;
    vec![at - t, at + t]
}

/// Format a linear measurement per the style (plain text).
pub fn format_linear_value(v: f64, st: &DimStyle) -> String {
    format_linear(v * st.linear_factor, &NumFormat::from_style(st)).0
}

/// The text of a linear measurement: (plain, MTEXT) with DIMPOST, tolerances/limits and
/// alternate units, then the user's override text (`<>` = the measurement). DIMRND rounds the
/// measurement (and so the limits) only: tolerances are never rounded and alternate units
/// round by DIMALTRND.
fn linear_text(meas: f64, d: &Dimension, st: &DimStyle, prefix: &str) -> String {
    let v = meas * st.linear_factor;
    let nf = NumFormat::from_style(st);
    let main = if st.limits {
        let up = format_linear(v + st.tol_plus, &nf).1;
        let lo = format_linear(v - st.tol_minus, &nf).1;
        format!("{{\\H{};\\S{up}^{lo};}}", tol_height_code(st))
    } else {
        format!("{prefix}{}", format_linear(v, &nf).1)
    };
    let mut s = post(&st.post, &main);
    if st.tolerance && !st.limits {
        let tf = NumFormat { decimals: st.tol_decimals, zin: st.tol_zero_suppression, round: 0.0, ..nf.clone() };
        // DIMPOST's suffix (what follows `<>`, or all of it) also goes on the tolerance values.
        let suffix = st.post.split_once("<>").map_or(st.post.as_str(), |(_, after)| after);
        if (st.tol_plus - st.tol_minus).abs() < 1e-12 {
            s += &format!("%%p{}{suffix}", format_linear(st.tol_plus, &tf).1);
        } else {
            let plus = format_linear(st.tol_plus, &tf).1 + suffix;
            let minus = format_linear(st.tol_minus, &tf).1 + suffix;
            let sign = |x: &str, pos: char| if x.trim_start_matches(['0', '.', ',']).is_empty() { x.to_string() } else { format!("{pos}{x}") };
            let (plus, minus) =
                (sign(&plus, '+'), if st.tol_minus < 0.0 { format!("+{}", minus.trim_start_matches('-')) } else { sign(&minus, '-') });
            // DIMTOLJ: bottom / middle / top (MTEXT `\A`; middle is the default).
            let tolj = if st.tol_just == 1 { String::new() } else { format!("\\A{};", st.tol_just.min(2)) };
            s += &format!("{{\\H{};{tolj}\\S{plus}^{minus};}}", tol_height_code(st));
        }
    }
    if st.alt {
        // DIMALTU 6 and 7 are architectural and fractional without stacking, 8 is decimal.
        let (unit, frac) = match st.alt_unit {
            u @ 1..=5 => (u, nf.frac),
            6 => (4, 2),
            7 => (5, 2),
            _ => (2, nf.frac),
        };
        let af = NumFormat { unit, decimals: st.alt_decimals, zin: st.alt_zero_suppression, round: st.alt_round, frac, ..nf };
        let alt = format_linear(v * st.alt_factor, &af).1;
        s += &format!(" [{}]", post(&st.alt_post, &alt));
    }
    inspected(apply_override(&s, d), d)
}

/// DIMINSPECT: the label and the rate either side of the value, two spaces apart.
fn inspected(value: String, d: &Dimension) -> String {
    match &d.marks.inspect {
        Some(ins) => [ins.label.as_str(), value.as_str(), ins.rate.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("  "),
        None => value,
    }
}

/// `\H` code for stacked tolerance parts (DIMTFAC relative to the 70% stack scale).
fn tol_height_code(st: &DimStyle) -> String {
    let f = if st.tol_scale.is_finite() && st.tol_scale > 0.0 { st.tol_scale } else { 1.0 };
    format!("{}x", format_decimal(f / 0.7, 4, 8, "."))
}

/// Apply measurement placeholders or append a dimension suffix, bounding the resulting UTF-8 text.
fn post(template: &str, meas: &str) -> String {
    if template.contains("<>") {
        bounded_replace(template, meas)
    } else {
        let mut out = String::new();
        append_dim_text(&mut out, meas);
        append_dim_text(&mut out, template);
        out
    }
}

/// Apply dimension text overrides: empty uses the base, whitespace suppresses text, and other text substitutes it.
fn apply_override(base: &str, d: &Dimension) -> String {
    if d.text.is_empty() {
        base.to_string()
    } else if d.text.trim().is_empty() {
        String::new()
    } else {
        bounded_replace(&d.text, base)
    }
}

/// Largest dimension text produced by substituting the measurement into style/override text. Two
/// modest strings can otherwise expand into gigabytes when the template holds many `<>` markers
/// (CWE-789); past this the output is truncated rather than allocated unbounded.
const MAX_DIM_TEXT: usize = 64 * 1024;

/// Replace every `<>` in `template` with `value`, with the expanded length checked against
/// [`MAX_DIM_TEXT`] before allocating so a many-placeholder template cannot blow up memory.
fn bounded_replace(template: &str, value: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find("<>") {
        if !append_dim_text(&mut out, &rest[..i]) || !append_dim_text(&mut out, value) {
            return out;
        }
        rest = &rest[i + 2..];
    }
    append_dim_text(&mut out, rest);
    out
}

// Returns false when truncation consumed the remaining UTF-8-safe space.
/// Append within the remaining dimension-text byte allowance without splitting a UTF-8 character.
/// Return false if any input text had to be truncated.
fn append_dim_text(out: &mut String, text: &str) -> bool {
    let mut end = text.len().min(MAX_DIM_TEXT.saturating_sub(out.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&text[..end]);
    end == text.len()
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

/// Font and fixed height of the dimension text (from DIMTXSTY's text style).
#[derive(Clone, Debug, Default)]
pub struct DimText {
    pub font: TextFont,
    /// Fixed text height of the style (0 = use DIMTXT × scale).
    pub fixed_height: f64,
    pub width_factor: f64,
    pub oblique: f64,
}

/// Generate the geometry of a dimension with the stroke font.
pub fn dimension_geometry(d: &Dimension, st: &DimStyle, dimscale: f64) -> DimGeometry {
    dimension_geometry_with(d, st, dimscale, &DimText { width_factor: 1.0, ..Default::default() })
}

/// Generate the geometry of a dimension. The dimension's own overrides are applied to `st`.
pub fn dimension_geometry_with(d: &Dimension, st: &DimStyle, dimscale: f64, font: &DimText) -> DimGeometry {
    let st = &st.with_overrides(&d.overrides);
    let k = st.effective_scale(dimscale);
    let asz = st.arrow_size * k;
    let th = if font.fixed_height > 0.0 { font.fixed_height } else { st.text_height * k };
    let gap = st.text_gap.abs() * k;
    let tsz = st.tick_size * k;
    let ticks = tsz > 0.0;
    let blk1 = Arrowhead::parse(if st.arrow_block1.is_empty() { &st.arrow_block } else { &st.arrow_block1 });
    let blk2 = Arrowhead::parse(if st.arrow_block2.is_empty() { &st.arrow_block } else { &st.arrow_block2 });
    let mut g = DimGeometry { text_height: th, ..Default::default() };
    // Measure the text: lay it out once at the origin, centred.
    let wf = if font.width_factor > 0.0 && font.width_factor.is_finite() { font.width_factor } else { 1.0 };
    let params = |mt: Vec2, rot: f64| MTextParams {
        attach: 5,
        rotation: rot,
        font: font.font.clone(),
        width_factor: wf,
        oblique: font.oblique,
        ..MTextParams::new(mt, th)
    };
    let measure = |text: &str| {
        if text.is_empty() {
            return (0.0, 0.0);
        }
        let l = cadcraft_fonts::layout_mtext_with(text, &params(Vec2::ZERO, 0.0));
        (l.bounds.width(), l.bounds.height())
    };
    let mut text_angle = 0.0;
    let mut text_mid = d.text_mid.xy();
    // Place an end marker: tick or arrowhead. Returns how far the dimension line stops short.
    let end = |g: &mut DimGeometry, kind: Arrowhead, tip: Vec2, dir: Vec2| -> f64 {
        if ticks {
            g.dim(tick(tip, dir, tsz));
            0.0
        } else {
            g.add_arrow(arrowhead(kind, tip, dir, asz));
            kind.inset(asz)
        }
    };
    match d.kind {
        DimKind::Linear { .. } | DimKind::Aligned => {
            let p1 = d.p13.xy();
            let p2 = d.p14.xy();
            let on = d.defpt.xy();
            let dir = match d.kind {
                DimKind::Linear { rotation } => Vec2::from_angle(rotation),
                _ => (p2 - p1).normalized(),
            };
            let dir = if dir == Vec2::ZERO || !dir.is_finite() { Vec2::X } else { dir };
            // DIMEDIT Oblique: extension lines at their own angle (not along the dimension line).
            let oblique = d.marks.oblique.map(Vec2::from_angle).filter(|e| e.is_finite() && e.cross(dir).abs() > 1e-6);
            let foot = |p: Vec2| match oblique {
                Some(e) => p + e * ((on - p).cross(dir) / e.cross(dir)),
                None => on + dir * (p - on).dot(dir),
            };
            let f1 = foot(p1);
            let f2 = foot(p2);
            // The measurement is along the dimension direction whatever the extension lines do.
            let meas = (p2 - p1).dot(dir).abs();
            g.mtext = linear_text(meas, d, st, "");
            g.measurement = meas;
            // From here on: the length drawn between the extension lines.
            let meas = f1.dist(f2);
            if !st.suppress_ext1 {
                g.ext_n(ext_line(p1, f1, st, k), false);
            }
            if !st.suppress_ext2 {
                g.ext_n(ext_line(p2, f2, st, k), true);
            }
            let u = (f2 - f1).normalized();
            let u = if u == Vec2::ZERO || !u.is_finite() { dir } else { u };
            let n = u.perp();
            let (tw, tht) = measure(&g.mtext);
            let has_text = tw > 0.0;
            let tick_like = ticks || (blk1.is_tick() && blk2.is_tick());
            let aw = if tick_like { 0.0 } else { asz };
            let arrows_fit = tick_like || meas > 2.0 * asz + 1e-9;
            // Text orientation: inside / outside horizontal.
            let along_ang = readable(u.angle());
            let extent = |ang: f64| {
                // Half-extent of the text box projected on the dimension line.
                let r = Vec2::from_angle(ang);
                (tw * r.dot(u).abs() + tht * r.perp().dot(u).abs()) / 2.0
            };
            let inside_ang = if st.text_inside_horizontal { 0.0 } else { along_ang };
            // DIMTVP (with DIMTAD 0) lifts the text off the line once it is 0.7 text heights or more.
            let tvp = if st.text_above == 0 && st.text_vertical_pos.is_finite() { st.text_vertical_pos } else { 0.0 };
            let centred = st.text_above == 0 && tvp.abs() < 0.7;
            let text_need = if centred { 2.0 * (extent(inside_ang) + gap) } else { 2.0 * extent(inside_ang) };
            let fits = !has_text || meas >= text_need + 2.0 * aw;
            let text_fits = !has_text || meas >= text_need;
            let just = st.text_just.min(4);
            // DIMATFIT / DIMTIX: what goes outside the extension lines when text and arrows
            // don't both fit between them. Text the user placed, or over an extension line, is
            // not part of the fit.
            let (text_in, arrows_inside) = if d.user_text_pos || !has_text || just >= 3 {
                (true, arrows_fit)
            } else if fits {
                (true, true)
            } else if st.text_inside {
                (true, false)
            } else {
                match st.fit {
                    0 => (false, false),
                    1 => (text_fits, false),
                    2 => (false, arrows_fit),
                    _ if arrows_fit => (false, true),
                    _ => (text_fits, false),
                }
            };
            let text_outside = !text_in;
            text_angle = if text_outside { if st.text_outside_horizontal { 0.0 } else { along_ang } } else { inside_ang };
            let half = extent(text_angle);
            // DIMSOXD: arrowheads that don't fit inside are left out.
            let outside_arrows = !arrows_inside && !st.suppress_outside_arrows;
            // Text centre along the dimension line.
            let along_c = if text_outside {
                f2 + u * (2.0 * asz.max(tsz) + gap + half)
            } else {
                match just {
                    1 => f1 + u * (aw + gap + half + asz),
                    2 => f2 - u * (aw + gap + half + asz),
                    _ => f1.mid(f2),
                }
            };
            // Perpendicular offset per DIMTAD.
            let side_out = if (on - p1).dot(n) >= 0.0 { n } else { -n };
            let tr = Vec2::from_angle(text_angle);
            let off_along = |m: Vec2| gap + (tw * tr.dot(m).abs() + tht * tr.perp().dot(m).abs()) / 2.0;
            let above = if text_angle.abs() < 1e-12 && n.y.abs() > 1e-9 {
                if n.y > 0.0 { n } else { -n }
            } else if text_angle.abs() < 1e-12 {
                if n.x < 0.0 { n } else { -n }
            } else if tr.perp().dot(n) >= 0.0 {
                n
            } else {
                -n
            };
            let offset = match st.text_above {
                0 => above * (tvp * th),
                2 => side_out * off_along(side_out),
                4 => -above * off_along(-above),
                _ => above * off_along(above),
            };
            if just >= 3 && !d.user_text_pos && has_text {
                // Over an extension line, aligned with it.
                let (p, f) = if just == 3 { (p1, f1) } else { (p2, f2) };
                let e = (f - p).normalized();
                let e = if e == Vec2::ZERO { side_out } else { e };
                text_angle = readable(e.angle());
                let outward = if just == 3 { -u } else { u };
                text_mid = f + e * (st.ext_extend * k + gap + tw / 2.0) + outward * (gap + tht / 2.0);
            } else if !d.user_text_pos {
                text_mid = along_c + offset;
            }
            // Dimension line. DIMSD1 / DIMSD2 leave out its first / second half (with the arrow).
            let dle = if tick_like { st.dim_line_extend * k } else { 0.0 };
            let lo = if st.suppress_dim1 { meas / 2.0 } else { f64::NEG_INFINITY };
            let hi = if st.suppress_dim2 { meas / 2.0 } else { f64::INFINITY };
            let seg = |g: &mut DimGeometry, a: Vec2, b: Vec2| {
                let (ta, tb) = ((a - f1).dot(u), (b - f1).dot(u));
                let (t0, t1) = (ta.min(tb).max(lo), ta.max(tb).min(hi));
                if t1 > t0 + 1e-12 {
                    g.dim(vec![f1 + u * t0, f1 + u * t1]);
                }
            };
            // The line between `a` and `b`, broken around centred text between the extension lines.
            let inner = |g: &mut DimGeometry, a: Vec2, b: Vec2| {
                if centred && has_text && !text_outside && just <= 2 && !d.user_text_pos {
                    let h = extent(text_angle) + gap;
                    let (t0, t1) = ((along_c - u * h - f1).dot(u), (along_c + u * h - f1).dot(u));
                    if t0 > (a - f1).dot(u) + 1e-9 {
                        seg(g, a, f1 + u * t0);
                    }
                    if t1 < (b - f1).dot(u) - 1e-9 {
                        seg(g, f1 + u * t1, b);
                    }
                } else {
                    seg(g, a, b);
                }
            };
            // Out to text outside the extension lines, from `from` along the line.
            let to_text = |g: &mut DimGeometry, from: Vec2| {
                let to = text_mid - offset - u * (half + gap);
                if text_outside && (to - from).dot(u) > 0.0 {
                    seg(g, from, from + u * (to - from).dot(u));
                }
            };
            if arrows_inside {
                let i1 = if st.suppress_dim1 { 0.0 } else { end(&mut g, blk1, f1, -u) };
                let i2 = if st.suppress_dim2 { 0.0 } else { end(&mut g, blk2, f2, u) };
                inner(&mut g, f1 - u * dle + u * i1, f2 + u * dle - u * i2);
                to_text(&mut g, f2);
            } else {
                let stub = asz * 2.0;
                if outside_arrows {
                    if !st.suppress_dim1 {
                        end(&mut g, blk1, f1, u);
                    }
                    if !st.suppress_dim2 {
                        end(&mut g, blk2, f2, -u);
                    }
                    seg(&mut g, f1 - u * stub, f1);
                    seg(&mut g, f2, f2 + u * stub);
                }
                // DIMTOFL: the line between the extension lines as well.
                if st.force_line_inside {
                    inner(&mut g, f1, f2);
                }
                to_text(&mut g, if outside_arrows { f2 + u * stub } else { f2 });
            }
            // DIMTMOVE 1: a leader from the dimension line to text moved off it.
            if d.user_text_pos && st.text_move == 1 && has_text {
                let base = f1.mid(f2);
                let off = (text_mid - base).dot(n).abs();
                if off > tht / 2.0 + gap + 1e-9 {
                    let side = if (base - text_mid).dot(tr) >= 0.0 { tr } else { -tr };
                    g.dim(vec![base, text_mid + side * (tw / 2.0 + gap)]);
                }
            }
            // DIMJOGLINE: a zigzag in the dimension line, 1.5 text heights tall.
            if let Some(j) = d.marks.jog_line {
                let at = f1 + u * (j.xy() - f1).dot(u);
                let h = 1.5 * th;
                if at.is_finite() && h > 0.0 {
                    let r = h / 2.0;
                    g.cut(at, r, Some(LineRole::Dim));
                    g.dim(vec![at - u * r, at - u * (r / 4.0) + n * (h / 2.0), at + u * (r / 4.0) - n * (h / 2.0), at + u * r]);
                }
            }
        }
        DimKind::Radius if d.marks.jogged.is_some() => 'jogged: {
            // DIMJOGGED: from the arrowhead on the arc toward the centre, a jog at DIMJOGANG,
            // then on to the overriding centre (parallel to the first part).
            let center = d.defpt.xy();
            let tip = d.p15.xy();
            let r = center.dist(tip);
            let j = d.marks.jogged.clone().unwrap_or_default();
            let (o, jp) = (j.center.xy(), j.jog.xy());
            g.mtext = linear_text(r, d, st, "R");
            // A NaN or infinite radius leaves no line to draw (and NaN is no bound for `clamp`).
            if !r.is_finite() {
                break 'jogged;
            }
            let (tw, _) = measure(&g.mtext);
            let u = (tip - center).normalized();
            let u = if u == Vec2::ZERO || !u.is_finite() { Vec2::X } else { u };
            let n = u.perp();
            let h = (o - center).dot(n);
            let ang = if st.jog_angle.is_finite() { st.jog_angle.clamp(5.0, 90.0) } else { 45.0 }.to_radians();
            let run = h.abs() / ang.tan().max(1e-6);
            let tj = (jp - center).dot(u).clamp(0.0, r);
            let (j1, j2) = (center + u * (tj + run / 2.0), center + u * (tj - run / 2.0) + n * h);
            g.dim(vec![tip, j1, j2, o]);
            end(&mut g, blk2, tip, u);
            let tpos = if d.user_text_pos { text_mid } else { tip + u * (asz.max(tsz) * 3.0) };
            if tpos.dist(center) > r {
                g.dim(vec![tip, tpos]);
            }
            text_angle = if st.text_outside_horizontal { 0.0 } else { readable(u.angle()) };
            if !d.user_text_pos {
                let x = Vec2::from_angle(text_angle);
                let x = if x.dot(u) >= 0.0 { x } else { -x };
                text_mid = tpos + x * (tw / 2.0 + gap);
            }
        }
        DimKind::Radius | DimKind::Diameter => {
            let a = d.defpt.xy();
            let b = d.p15.xy();
            let radius = matches!(d.kind, DimKind::Radius);
            let (center, r) = if radius { (a, a.dist(b)) } else { (a.mid(b), a.dist(b) / 2.0) };
            let meas = if radius { r } else { 2.0 * r };
            g.mtext = linear_text(meas, d, st, if radius { "R" } else { "%%c" });
            g.measurement = meas;
            let (tw, tht) = measure(&g.mtext);
            let u = (b - center).normalized();
            let u = if u == Vec2::ZERO || !u.is_finite() { Vec2::X } else { u };
            let tip = center + u * r;
            let tpos = if d.user_text_pos { text_mid } else { tip + u * (asz.max(tsz) * 3.0) };
            // Text placed inside the circle (a dimension line location picked inside): the text
            // sits on the dimension line, which runs from the text out to the arrowhead (for a
            // diameter it spans the circle, broken around the text). DIMTIH turns it horizontal.
            let inside = d.user_text_pos && text_mid.dist(center) < r;
            let horizontal = if inside { st.text_inside_horizontal } else { st.text_outside_horizontal };
            if inside {
                let tr = Vec2::from_angle(if horizontal { 0.0 } else { readable(u.angle()) });
                let half = if tw > 0.0 { (tw * tr.dot(u).abs() + tht * tr.perp().dot(u).abs()) / 2.0 + gap } else { 0.0 };
                let start = if radius { center } else { center - u * r };
                let (t0, t1) = ((text_mid - start).dot(u) - half, (text_mid - start).dot(u) + half);
                if !radius {
                    if t0 > 0.0 {
                        g.dim(vec![start, start + u * t0]);
                    }
                    end(&mut g, blk1, start, -u);
                }
                if t1 < tip.dist(start) {
                    g.dim(vec![start + u * t1.max(0.0), tip]);
                }
            } else {
                // Text outside: a diameter line spans the circle and a radius line runs in to the
                // centre only with DIMTOFL; otherwise just the leader to the text.
                if st.force_line_inside {
                    let start = if radius { center } else { center - u * r };
                    g.dim(vec![start, tip]);
                    if !radius {
                        end(&mut g, blk1, start, -u);
                    }
                }
                // Text the user placed: the leader stops short of it by the offset the default
                // placement leaves between leader end and text.
                let lead = if !d.user_text_pos {
                    tpos
                } else if horizontal {
                    text_mid - Vec2::new(tw / 2.0 + gap, 0.0) * if u.x >= 0.0 { 1.0 } else { -1.0 }
                } else {
                    text_mid - u * (tw / 2.0 + gap)
                };
                g.dim(vec![tip, lead]);
            }
            end(&mut g, blk2, tip, u);
            if horizontal {
                text_angle = 0.0;
                let sx = if u.x >= 0.0 { 1.0 } else { -1.0 };
                if !d.user_text_pos {
                    text_mid = tpos + Vec2::new(tw / 2.0 + gap, 0.0) * sx;
                }
            } else {
                text_angle = readable(u.angle());
                if !d.user_text_pos {
                    text_mid = tpos + u * (tw / 2.0 + gap);
                }
            }
            // The DIMCEN mark is drawn only when no dimension line runs inside the circle (text
            // outside without DIMTOFL), never with a line through the centre.
            let line_outside = !st.force_line_inside && tpos.dist(center) >= r;
            if line_outside && st.center_mark != 0.0 && st.center_mark.is_finite() {
                let c = st.center_mark.abs() * k;
                g.ext(vec![center - Vec2::X * c, center + Vec2::X * c]);
                g.ext(vec![center - Vec2::Y * c, center + Vec2::Y * c]);
                if st.center_mark < 0.0 && r > 2.0 * c {
                    // Centre lines out past the circle.
                    for dv in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                        g.ext(vec![center + dv * (2.0 * c), center + dv * (r + c)]);
                    }
                }
            }
        }
        DimKind::Angular | DimKind::Angular3P => {
            // Angular3P: p15 vertex, p13/p14 on the legs, defpt on the arc.
            // Angular (2 lines): lines p13-p14 and defpt-p15; p16 on the arc.
            let (vertex, a1, a2, arc_pt) = if matches!(d.kind, DimKind::Angular3P) {
                (d.p15.xy(), d.p13.xy(), d.p14.xy(), d.defpt.xy())
            } else {
                let v = cadcraft_geom::line_line_infinite(d.p13.xy(), d.p14.xy(), d.defpt.xy(), d.p15.xy()).map(|x| x.0).unwrap_or(d.p14.xy());
                (v, d.p14.xy(), d.p15.xy(), d.p16.xy())
            };
            let r = vertex.dist(arc_pt).max(1e-9);
            let s = vertex.angle_to(a1);
            let e = vertex.angle_to(a2);
            let arc =
                if cadcraft_geom::angle_in_sweep(vertex.angle_to(arc_pt), s, e) { Arc::new(vertex, r, s, e) } else { Arc::new(vertex, r, e, s) };
            let sweep = ccw_sweep(arc.start, arc.end);
            g.measurement = sweep;
            // DIMAZIN 1 / 2 suppress leading / trailing zeros (DIMZIN's bits 4 / 8).
            let azin = (st.angular_zero_suppression & 3) * 4;
            // DIMPOST wraps the angle too, as AutoCAD 2026 does to its own 2-line and 3-point
            // angular dimensions ("36.9° m" for a style with "<> m"; issue #1653, after #376).
            let txt = post(&st.post, &format_angle(sweep, st.angular_unit, st.angular_decimals, azin));
            g.mtext = inspected(apply_override(&txt, d), d);
            let mut pts = Vec::new();
            arc.tessellate(r * 1e-3, &mut pts);
            g.dim(pts);
            let sp = arc.start_point();
            let ep = arc.end_point();
            // The arc's ends with the outward tangents there, paired with leg 1 and leg 2: when
            // the arc location is outside the leg 1 → leg 2 sweep the arc runs from leg 2 to
            // leg 1, and each leg keeps its own extension line, DIMSE1/2 and DIMBLK1/2.
            let (start, finish) = ((sp, -(sp - vertex).perp().normalized()), (ep, (ep - vertex).perp().normalized()));
            let ((p1, dir1), (p2, dir2)) =
                if cadcraft_geom::angle_in_sweep(vertex.angle_to(arc_pt), s, e) { (start, finish) } else { (finish, start) };
            if !st.suppress_ext1 {
                g.ext_n(ext_line(vertex + (p1 - vertex).normalized() * vertex.dist(a1).min(r), p1, st, k), false);
            }
            if !st.suppress_ext2 {
                g.ext_n(ext_line(vertex + (p2 - vertex).normalized() * vertex.dist(a2).min(r), p2, st, k), true);
            }
            end(&mut g, blk1, p1, dir1);
            end(&mut g, blk2, p2, dir2);
            let (tw, tht) = measure(&g.mtext);
            let mid = arc.mid_point();
            let radial = (mid - vertex).normalized();
            text_angle = if st.text_inside_horizontal { 0.0 } else { readable(radial.perp().angle()) };
            if !d.user_text_pos {
                let tr = Vec2::from_angle(text_angle);
                let off = gap + (tw * tr.dot(radial).abs() + tht * tr.perp().dot(radial).abs()) / 2.0;
                text_mid = mid + radial * off;
            }
        }
        DimKind::Ordinate { x_type } => {
            let feature = d.p13.xy();
            let leader_end = d.p14.xy();
            let origin = d.defpt.xy();
            let meas = if x_type { (feature.x - origin.x).abs() } else { (feature.y - origin.y).abs() };
            g.mtext = linear_text(meas, d, st, "");
            g.measurement = meas;
            let (tw, _) = measure(&g.mtext);
            g.dim(vec![feature, leader_end]);
            if x_type {
                text_angle = std::f64::consts::FRAC_PI_2;
            }
            if !d.user_text_pos {
                let dirv = (leader_end - feature).normalized();
                let dirv = if dirv == Vec2::ZERO { if x_type { Vec2::Y } else { Vec2::X } } else { dirv };
                text_mid = leader_end + dirv * (tw / 2.0 + gap);
            }
        }
        DimKind::ArcLength => {
            let c = d.p15.xy();
            let r = c.dist(d.p13.xy());
            let arc = Arc::new(c, c.dist(d.defpt.xy()).max(1e-9), c.angle_to(d.p13.xy()), c.angle_to(d.p14.xy()));
            let meas = r * arc.sweep();
            // DIMARCSYM: the arc symbol before the text (0), over it (1) or none (2).
            g.mtext = linear_text(meas, d, st, if st.arc_symbol == 0 { "⌒" } else { "" });
            g.measurement = meas;
            let mut pts = Vec::new();
            arc.tessellate(arc.radius * 1e-3, &mut pts);
            g.dim(pts);
            for (i, (p, q)) in [(d.p13.xy(), arc.start_point()), (d.p14.xy(), arc.end_point())].into_iter().enumerate() {
                g.ext_n(ext_line(p, q, st, k), i == 1);
            }
            end(&mut g, blk1, arc.start_point(), -(arc.start_point() - c).perp().normalized());
            end(&mut g, blk2, arc.end_point(), (arc.end_point() - c).perp().normalized());
            let m = arc.mid_point();
            let radial = (m - c).normalized();
            text_angle = if st.text_inside_horizontal { 0.0 } else { readable(radial.perp().angle()) };
            if !d.user_text_pos {
                text_mid = m + radial * (gap + th / 2.0);
            }
            if st.arc_symbol == 1 {
                // A small arc over the text.
                let (tw, tht) = measure(&g.mtext);
                let (x, y) = (Vec2::from_angle(text_angle), Vec2::from_angle(text_angle).perp());
                let half = (tw / 2.0).max(th / 2.0);
                let base = text_mid + y * (tht / 2.0 + gap / 2.0);
                let sym: Vec<Vec2> = (0..=12)
                    .map(|i| {
                        let t = f64::from(i) / 6.0 - 1.0;
                        base + x * (t * half) + y * ((1.0 - t * t).max(0.0).sqrt() * th * 0.35)
                    })
                    .collect();
                g.dim(sym);
            }
        }
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
            g.dim(vec![at(left + gap2 / 2.0, -hh), at(left + gap2 / 2.0, hh)]);
            left += gap2;
        }
        if !ins.rate.is_empty() {
            left += wv;
            g.dim(vec![at(left + gap2 / 2.0, -hh), at(left + gap2 / 2.0, hh)]);
        }
        g.dim(vec![at(-hw, hh), at(hw, hh)]);
        g.dim(vec![at(-hw, -hh), at(hw, -hh)]);
        for side in [-1.0, 1.0] {
            let ends: Vec<Vec2> = if ins.shape == 0 {
                (0..=12).map(|i| f64::from(i) * std::f64::consts::PI / 12.0).map(|a| at(side * (hw + hh * a.sin()), hh * a.cos())).collect()
            } else {
                vec![at(side * hw, hh), at(side * (hw + hh), 0.0), at(side * hw, -hh)]
            };
            g.dim(ends);
        }
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

/// `%%` codes in the plain value become their characters (`%%c` → ⌀, `%%p` → ±, `%%d` → °).
fn decode_value(s: &str) -> String {
    cadcraft_fonts::decode_controls(s).into_iter().map(|(c, _, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec3;

    fn lin(p1: (f64, f64), p2: (f64, f64), on: (f64, f64), rot: f64) -> Dimension {
        Dimension {
            kind: DimKind::Linear { rotation: rot },
            defpt: Vec3::new(on.0, on.1, 0.0),
            text_mid: Vec3::ZERO,
            p13: Vec3::new(p1.0, p1.1, 0.0),
            p14: Vec3::new(p2.0, p2.1, 0.0),
            p15: Vec3::ZERO,
            p16: Vec3::ZERO,
            text: String::new(),
            style: "Standard".into(),
            measurement: 0.0,
            text_rotation: 0.0,
            user_text_pos: false,
            block: None,
            overrides: Default::default(),
            assoc: Vec::new(),
            marks: Default::default(),
        }
    }

    fn st_with(f: impl Fn(&mut DimStyle)) -> DimStyle {
        let mut s = DimStyle::default();
        f(&mut s);
        s
    }

    fn bbox(pts: &[Vec<Vec2>]) -> cadcraft_geom::Bounds2 {
        cadcraft_geom::Bounds2::from_points(pts.iter().flatten().copied())
    }

    #[test]
    fn horizontal_measures_dx() {
        let g = dimension_geometry(&lin((0.0, 0.0), (10.0, 3.0), (0.0, 5.0), 0.0), &DimStyle::default(), 1.0);
        assert_eq!(g.value, "10.0000");
        assert_eq!(g.measurement, 10.0);
        assert!(!g.text.is_empty());
        assert_eq!(g.fills.len(), 2);
        assert_eq!(g.lines_of(LineRole::Ext).count(), 2);
    }

    #[test]
    fn vertical_measures_dy() {
        let g = dimension_geometry(&lin((0.0, 0.0), (10.0, 3.0), (15.0, 0.0), std::f64::consts::FRAC_PI_2), &DimStyle::default(), 1.0);
        assert_eq!(g.value, "3.0000");
        assert!((g.measurement - 3.0).abs() < 1e-12, "{}", g.measurement);
    }

    #[test]
    fn override_text() {
        let mut d = lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0);
        d.text = "<> TYP".into();
        let g = dimension_geometry(&d, &DimStyle::default(), 1.0);
        assert_eq!(g.value, "10.0000 TYP");
        d.text = " ".into();
        assert_eq!(dimension_geometry(&d, &DimStyle::default(), 1.0).value, "");
    }

    #[test]
    fn iso_suppresses_zeros() {
        let g = dimension_geometry(&lin((0.0, 0.0), (12.5, 0.0), (0.0, 5.0), 0.0), &DimStyle::iso25(), 1.0);
        assert_eq!(g.value, "12.5");
    }

    #[test]
    fn ticks_replace_arrows_and_extend_line() {
        let st = st_with(|s| {
            s.tick_size = 0.1;
            s.dim_line_extend = 0.2;
        });
        let g = dimension_geometry(&lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0), &st, 1.0);
        assert!(g.fills.is_empty(), "no filled arrows with ticks");
        // Two 45° ticks of half-length 0.1 centred on the feet.
        let ticks: Vec<_> = g.lines.iter().filter(|l| l.len() == 2 && ((l[1] - l[0]).angle().to_degrees() - 45.0).abs() < 1e-6).collect();
        assert_eq!(ticks.len(), 2);
        for t in &ticks {
            assert!((t[0].dist(t[1]) - 0.2).abs() < 1e-9);
            assert!((t[0].mid(t[1]).y - 5.0).abs() < 1e-9);
        }
        // The dimension line runs past both extension lines by DIMDLE.
        let b = bbox(&g.lines_of(LineRole::Dim).cloned().collect::<Vec<_>>());
        assert!((b.min.x + 0.2).abs() < 1e-9 && (b.max.x - 10.2).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn arrowhead_shapes() {
        let tip = Vec2::new(1.0, 1.0);
        let dir = Vec2::X;
        let cf = arrowhead(Arrowhead::ClosedFilled, tip, dir, 1.0);
        assert_eq!(cf.tris.len(), 3);
        assert!(cf.tris.contains(&tip));
        let cb = arrowhead(Arrowhead::ClosedBlank, tip, dir, 1.0);
        assert!(cb.tris.is_empty() && cb.lines.len() == 1 && cb.lines[0].len() == 4);
        assert_eq!(cb.lines[0].first(), cb.lines[0].last());
        let dot = arrowhead(Arrowhead::Dot, tip, dir, 1.0);
        let r = dot.tris.iter().map(|p| p.dist(tip)).fold(0.0, f64::max);
        assert!((r - 0.25).abs() < 1e-9, "dot radius {r}");
        let ob = arrowhead(Arrowhead::Oblique, tip, dir, 1.0);
        assert_eq!(ob.lines.len(), 1);
        assert!(((ob.lines[0][1] - ob.lines[0][0]).angle().to_degrees() - 45.0).abs() < 1e-9);
        let at = arrowhead(Arrowhead::ArchTick, tip, dir, 1.0);
        assert_eq!(at.tris.len(), 6);
        let open = arrowhead(Arrowhead::Open, tip, dir, 1.0);
        assert!(open.tris.is_empty() && open.lines[0].len() == 3 && open.lines[0][1] == tip);
        let o90 = arrowhead(Arrowhead::Open90, tip, dir, 1.0);
        let (a, b) = (o90.lines[0][0] - tip, o90.lines[0][2] - tip);
        assert!(a.dot(b).abs() < 1e-9, "open 90 wings are perpendicular");
        let origin = arrowhead(Arrowhead::Origin, tip, dir, 1.0);
        assert!(origin.lines[0].iter().all(|p| (p.dist(tip) - 0.5).abs() < 1e-9));
        assert_eq!(arrowhead(Arrowhead::None, tip, dir, 1.0), ArrowGeom::default());
        assert_eq!(Arrowhead::parse("_ARCHTICK"), Arrowhead::ArchTick);
        assert_eq!(Arrowhead::parse("Architectural tick"), Arrowhead::ArchTick);
        assert_eq!(Arrowhead::parse("closed blank"), Arrowhead::ClosedBlank);
        assert_eq!(Arrowhead::parse(""), Arrowhead::ClosedFilled);
        assert_eq!(Arrowhead::parse("_ORIGIN"), Arrowhead::Origin);
        assert_eq!(Arrowhead::parse("whatever"), Arrowhead::ClosedFilled);
    }

    #[test]
    fn dimblk_dot_and_none() {
        let st = st_with(|s| s.arrow_block = "_DOT".into());
        let g = dimension_geometry(&lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0), &st, 1.0);
        assert_eq!(g.fills.len(), 48, "two 24-triangle dots");
        let st = st_with(|s| {
            s.arrow_block1 = "_NONE".into();
            s.arrow_block2 = "_OPEN".into();
        });
        let g = dimension_geometry(&lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0), &st, 1.0);
        assert!(g.fills.is_empty());
        assert!(g.lines.iter().any(|l| l.len() == 3));
    }

    #[test]
    fn text_above_and_centred() {
        let d = lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0);
        let centred = dimension_geometry(&d, &DimStyle::default(), 1.0);
        assert!((centred.text_pos.y - 5.0).abs() < 1e-9);
        // Centred text breaks the dimension line in two.
        assert_eq!(centred.lines_of(LineRole::Dim).count(), 2);
        let above = dimension_geometry(&d, &st_with(|s| s.text_above = 1), 1.0);
        assert!((above.text_pos.y - (5.0 + 0.09 + 0.09)).abs() < 1e-6, "{}", above.text_pos.y);
        assert_eq!(above.lines_of(LineRole::Dim).count(), 1);
        let below = dimension_geometry(&d, &st_with(|s| s.text_above = 4), 1.0);
        assert!(below.text_pos.y < 5.0);
        // Outside: away from the measured points (they are below the line here).
        let outside = dimension_geometry(&d, &st_with(|s| s.text_above = 2), 1.0);
        assert!(outside.text_pos.y > 5.0);
        let d2 = lin((0.0, 10.0), (10.0, 10.0), (0.0, 5.0), 0.0);
        let outside2 = dimension_geometry(&d2, &st_with(|s| s.text_above = 2), 1.0);
        assert!(outside2.text_pos.y < 5.0);
    }

    #[test]
    fn text_alignment_vertical_dimension() {
        let d = lin((0.0, 0.0), (0.0, 10.0), (5.0, 0.0), std::f64::consts::FRAC_PI_2);
        let h = dimension_geometry(&d, &DimStyle::default(), 1.0);
        assert!(h.text_angle.abs() < 1e-12, "DIMTIH on: horizontal");
        let a = dimension_geometry(&d, &st_with(|s| s.text_inside_horizontal = false), 1.0);
        assert!((a.text_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "aligned: {}", a.text_angle);
    }

    #[test]
    fn justification_moves_text() {
        let d = lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0);
        let j1 = dimension_geometry(&d, &st_with(|s| s.text_just = 1), 1.0);
        let j2 = dimension_geometry(&d, &st_with(|s| s.text_just = 2), 1.0);
        assert!(j1.text_pos.x < 5.0 && j2.text_pos.x > 5.0);
        let j3 = dimension_geometry(&d, &st_with(|s| s.text_just = 3), 1.0);
        assert!((j3.text_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(j3.text_pos.y > 5.0 && j3.text_pos.x < 0.0);
    }

    #[test]
    fn text_moves_outside_when_it_does_not_fit() {
        let d = lin((0.0, 0.0), (0.5, 0.0), (0.0, 5.0), 0.0);
        let g = dimension_geometry(&d, &DimStyle::default(), 1.0);
        assert!(g.text_pos.x > 0.5, "text beyond the second extension line: {}", g.text_pos.x);
    }

    #[test]
    fn tolerance_and_limits() {
        let d = lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0);
        let sym = dimension_geometry(
            &d,
            &st_with(|s| {
                s.tolerance = true;
                s.tol_plus = 0.05;
                s.tol_minus = 0.05;
                s.decimals = 2;
                s.tol_decimals = 2;
            }),
            1.0,
        );
        assert_eq!(sym.value, "10.00±0.05");
        let dev = dimension_geometry(
            &d,
            &st_with(|s| {
                s.tolerance = true;
                s.tol_plus = 0.1;
                s.tol_minus = 0.2;
                s.decimals = 2;
                s.tol_decimals = 1;
            }),
            1.0,
        );
        assert_eq!(dev.value, "10.00+0.1/-0.2");
        assert!(dev.mtext.contains("\\S+0.1^-0.2;"));
        // Stacked: the text extends above and below a single line of text.
        let plain = dimension_geometry(&d, &st_with(|s| s.decimals = 2), 1.0);
        assert!(bbox(&dev.text).height() > bbox(&plain.text).height() * 1.3);
        let lim = dimension_geometry(
            &d,
            &st_with(|s| {
                s.limits = true;
                s.tol_plus = 0.1;
                s.tol_minus = 0.2;
                s.decimals = 1;
            }),
            1.0,
        );
        assert_eq!(lim.value, "10.1/9.8");
    }

    #[test]
    fn unit_formats_and_post() {
        let d = lin((0.0, 0.0), (18.5, 0.0), (0.0, 5.0), 0.0);
        let arch = dimension_geometry(&d, &st_with(|s| s.linear_unit = 4), 1.0);
        assert_eq!(arch.value, "1'-6 1/2\"");
        let frac = dimension_geometry(&d, &st_with(|s| s.linear_unit = 5), 1.0);
        assert_eq!(frac.value, "18 1/2");
        let sci = dimension_geometry(
            &d,
            &st_with(|s| {
                s.linear_unit = 1;
                s.decimals = 2;
            }),
            1.0,
        );
        assert_eq!(sci.value, "1.85E+01");
        let post = dimension_geometry(
            &d,
            &st_with(|s| {
                s.post = "<> mm".into();
                s.decimals = 1;
            }),
            1.0,
        );
        assert_eq!(post.value, "18.5 mm");
        let alt = dimension_geometry(
            &lin((0.0, 0.0), (1.0, 0.0), (0.0, 5.0), 0.0),
            &st_with(|s| {
                s.alt = true;
                s.decimals = 2;
            }),
            1.0,
        );
        assert_eq!(alt.value, "1.00 [25.40]");
    }

    #[test]
    fn overrides_apply_per_dimension() {
        let mut d = lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0);
        d.overrides.insert("DIMDEC".into(), serde_json::json!(1));
        d.overrides.insert("textAbove".into(), serde_json::json!(1));
        let g = dimension_geometry(&d, &DimStyle::default(), 1.0);
        assert_eq!(g.value, "10.0");
        assert!(g.text_pos.y > 5.0);
    }

    #[test]
    fn radial_and_angular_text() {
        let mut d = lin((0.0, 0.0), (0.0, 0.0), (0.0, 0.0), 0.0);
        d.kind = DimKind::Radius;
        d.defpt = Vec3::new(0.0, 0.0, 0.0);
        d.p15 = Vec3::new(3.0, 0.0, 0.0);
        let g = dimension_geometry(&d, &st_with(|s| s.decimals = 1), 1.0);
        assert_eq!(g.value, "R3.0");
        d.kind = DimKind::Diameter;
        d.defpt = Vec3::new(-3.0, 0.0, 0.0);
        let g = dimension_geometry(&d, &st_with(|s| s.decimals = 1), 1.0);
        assert_eq!(g.value, "⌀6.0");
        let mut a = lin((10.0, 0.0), (0.0, 10.0), (3.0, 3.0), 0.0);
        a.kind = DimKind::Angular3P;
        a.p15 = Vec3::ZERO;
        let g = dimension_geometry(&a, &DimStyle::default(), 1.0);
        assert_eq!(g.value, "90°");
        let g = dimension_geometry(
            &a,
            &st_with(|s| {
                s.angular_unit = 3;
                s.angular_decimals = 2;
            }),
            1.0,
        );
        assert_eq!(g.value, "1.57r");
    }

    #[test]
    fn hostile_styles_do_not_panic() {
        let d = lin((0.0, 0.0), (10.0, 0.0), (0.0, 5.0), 0.0);
        for st in [
            st_with(|s| s.arrow_size = f64::NAN),
            st_with(|s| s.text_height = -1.0),
            st_with(|s| s.tick_size = f64::INFINITY),
            st_with(|s| s.text_just = 200),
            st_with(|s| s.text_above = 99),
            st_with(|s| s.scale = 0.0),
            st_with(|s| s.decimals = 255),
        ] {
            let _ = dimension_geometry(&d, &st, 1.0);
        }
        let z = lin((0.0, 0.0), (0.0, 0.0), (0.0, 0.0), 0.0);
        let _ = dimension_geometry(&z, &DimStyle::default(), 1.0);
    }

    #[test]
    fn jogged_radius_with_nan_radius_draws_no_lines() {
        let mut d = lin((0.0, 0.0), (0.0, 0.0), (0.0, 0.0), 0.0);
        d.kind = DimKind::Radius;
        d.p15 = Vec3::new(10.0, 0.0, 0.0);
        d.marks.jogged = Some(cadcraft_doc::JoggedRadius { center: Vec3::new(0.0, -4.0, 0.0), jog: Vec3::new(5.0, -2.0, 0.0) });
        // Finite control: the jogged line runs from the arrowhead on the arc to the override centre.
        let g = dimension_geometry(&d, &DimStyle::default(), 1.0);
        let jog = g.lines.iter().find(|l| l.len() == 4).expect("jogged dimension line");
        assert!(jog[0].near(Vec2::new(10.0, 0.0), 1e-9) && jog[3].near(Vec2::new(0.0, -4.0), 1e-9));
        // A NaN in the tip makes the radius NaN: nothing usable to draw, and no panic.
        d.p15 = Vec3::new(f64::NAN, 0.0, 0.0);
        let g = dimension_geometry(&d, &DimStyle::default(), 1.0);
        assert!(g.lines.is_empty() && g.fills.is_empty(), "{:?}", g.lines);
    }
}

#[cfg(test)]
mod text_budget_tests {
    use super::*;
    /// A dimension suffix without placeholders still obeys the total text limit.
    #[test]
    fn bounds_post_without_markers() {
        let text = post(&"x".repeat(MAX_DIM_TEXT * 2), "12");
        assert_eq!(text.len(), MAX_DIM_TEXT);
        assert!(text.starts_with("12x"));
    }
    /// Oversized substituted text retains its prefix and ends at a valid UTF-8 boundary.
    #[test]
    fn oversized_value_is_substituted_and_truncated_at_a_character_boundary() {
        let text = bounded_replace("prefix <> mm", &"é".repeat(MAX_DIM_TEXT));
        assert!(text.starts_with("prefix é"));
        assert!(!text.contains("<>"));
        assert!(text.len() <= MAX_DIM_TEXT && text.len() >= MAX_DIM_TEXT - 1);
    }
}
