//! Dimension geometry generation from definition points and a dimension style: extension
//! lines (DIMEXO/DIMEXE, DIMSE1/2), arrowheads (DIMBLK/DIMBLK1/DIMBLK2, drawn in code) or ticks
//! (DIMTSZ, DIMDLE), text placement (DIMTAD, DIMJUST, DIMTIH/DIMTOH, DIMGAP) and text
//! formatting (DIMLUNIT, DIMDEC, DIMZIN, DIMRND, DIMPOST, DIMTOL/DIMLIM, DIMALT).

use cadcraft_doc::{DimKind, DimStyle, Dimension};
use cadcraft_fonts::{MTextParams, TextFont};
use cadcraft_geom::{Arc, Vec2, ccw_sweep};

use crate::units::{NumFormat, format_angle, format_decimal, format_linear};

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
    pub text_pos: Vec2,
    pub text_angle: f64,
    pub text_height: f64,
    /// Plain text of the dimension.
    pub value: String,
    /// The text as MTEXT contents (stacked fractions and tolerances).
    pub mtext: String,
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
/// alternate units, then the user's override text (`<>` = the measurement).
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
        let tf = NumFormat { decimals: st.tol_decimals, ..nf.clone() };
        if (st.tol_plus - st.tol_minus).abs() < 1e-12 {
            s += &format!("%%p{}", format_linear(st.tol_plus, &tf).1);
        } else {
            let plus = format_linear(st.tol_plus, &tf).1;
            let minus = format_linear(st.tol_minus, &tf).1;
            let sign = |x: &str, pos: char| if x.trim_start_matches(['0', '.', ',']).is_empty() { x.to_string() } else { format!("{pos}{x}") };
            let (plus, minus) =
                (sign(&plus, '+'), if st.tol_minus < 0.0 { format!("+{}", minus.trim_start_matches('-')) } else { sign(&minus, '-') });
            s += &format!("{{\\H{};\\S{plus}^{minus};}}", tol_height_code(st));
        }
    }
    if st.alt {
        let af = NumFormat { decimals: st.alt_decimals, unit: 2, ..nf };
        let alt = format_linear(v * st.alt_factor, &af).1;
        s += &format!(" [{}]", post(&st.alt_post, &alt));
    }
    apply_override(&s, d)
}

/// `\H` code for stacked tolerance parts (DIMTFAC relative to the 70% stack scale).
fn tol_height_code(st: &DimStyle) -> String {
    let f = if st.tol_scale.is_finite() && st.tol_scale > 0.0 { st.tol_scale } else { 1.0 };
    format!("{}x", format_decimal(f / 0.7, 4, 8, "."))
}

fn post(template: &str, meas: &str) -> String {
    if template.contains("<>") { template.replace("<>", meas) } else { format!("{meas}{template}") }
}

fn apply_override(base: &str, d: &Dimension) -> String {
    if d.text.is_empty() {
        base.to_string()
    } else if d.text.trim().is_empty() {
        String::new()
    } else {
        d.text.replace("<>", base)
    }
}

fn ext_line(origin: Vec2, foot: Vec2, st: &DimStyle, k: f64) -> Option<Vec<Vec2>> {
    let v = foot - origin;
    let len = v.len();
    if len < 1e-12 {
        return None;
    }
    let u = v / len;
    Some(vec![origin + u * (st.ext_offset * k), foot + u * (st.ext_extend * k)])
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
            let foot = |p: Vec2| on + dir * (p - on).dot(dir);
            let f1 = foot(p1);
            let f2 = foot(p2);
            let meas = f1.dist(f2);
            g.mtext = linear_text(meas, d, st, "");
            if !st.suppress_ext1 {
                g.lines.extend(ext_line(p1, f1, st, k));
                g.line_roles.resize(g.lines.len(), LineRole::Ext);
            }
            if !st.suppress_ext2 {
                g.lines.extend(ext_line(p2, f2, st, k));
                g.line_roles.resize(g.lines.len(), LineRole::Ext);
            }
            let u = (f2 - f1).normalized();
            let u = if u == Vec2::ZERO || !u.is_finite() { dir } else { u };
            let n = u.perp();
            let (tw, tht) = measure(&g.mtext);
            let has_text = tw > 0.0;
            let tick_like = ticks || (blk1.is_tick() && blk2.is_tick());
            let aw = if tick_like { 0.0 } else { asz };
            let arrows_inside = tick_like || meas > 2.0 * asz + 1e-9;
            // Text orientation: inside / outside horizontal.
            let along_ang = readable(u.angle());
            let extent = |ang: f64| {
                // Half-extent of the text box projected on the dimension line.
                let r = Vec2::from_angle(ang);
                (tw * r.dot(u).abs() + tht * r.perp().dot(u).abs()) / 2.0
            };
            let inside_ang = if st.text_inside_horizontal { 0.0 } else { along_ang };
            let centred = st.text_above == 0;
            let fits =
                !has_text || if centred { meas >= 2.0 * (extent(inside_ang) + gap) + 2.0 * aw } else { meas >= 2.0 * extent(inside_ang) + 2.0 * aw };
            let just = st.text_just.min(4);
            let text_outside = !d.user_text_pos && has_text && !fits && just <= 2;
            text_angle = if text_outside { if st.text_outside_horizontal { 0.0 } else { along_ang } } else { inside_ang };
            let half = extent(text_angle);
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
                0 => Vec2::ZERO,
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
            // Dimension line.
            let dle = if tick_like { st.dim_line_extend * k } else { 0.0 };
            if arrows_inside {
                let i1 = end(&mut g, blk1, f1, -u);
                let i2 = end(&mut g, blk2, f2, u);
                let a = f1 - u * dle + u * i1;
                let b = f2 + u * dle - u * i2;
                if centred && has_text && !text_outside && just <= 2 && !d.user_text_pos {
                    let c = along_c;
                    let h = extent(text_angle) + gap;
                    let t0 = (c - u * h - f1).dot(u);
                    let t1 = (c + u * h - f1).dot(u);
                    if t0 > (a - f1).dot(u) + 1e-9 {
                        g.dim(vec![a, f1 + u * t0]);
                    }
                    if t1 < (b - f1).dot(u) - 1e-9 {
                        g.dim(vec![f1 + u * t1, b]);
                    }
                } else {
                    g.dim(vec![a, b]);
                }
                if text_outside {
                    // Extend the dimension line out to the text.
                    let to = text_mid - offset - u * (half + gap);
                    if (to - f2).dot(u) > 0.0 {
                        g.dim(vec![f2, f2 + u * (to - f2).dot(u)]);
                    }
                }
            } else {
                end(&mut g, blk1, f1, u);
                end(&mut g, blk2, f2, -u);
                g.dim(vec![f1 - u * (asz * 2.0), f2 + u * (asz * 2.0)]);
                if text_outside {
                    let to = text_mid - offset - u * (half + gap);
                    if (to - f2).dot(u) > asz * 2.0 {
                        g.dim(vec![f2 + u * (asz * 2.0), f2 + u * (to - f2).dot(u)]);
                    }
                }
            }
        }
        DimKind::Radius | DimKind::Diameter => {
            let a = d.defpt.xy();
            let b = d.p15.xy();
            let radius = matches!(d.kind, DimKind::Radius);
            let (center, r) = if radius { (a, a.dist(b)) } else { (a.mid(b), a.dist(b) / 2.0) };
            let meas = if radius { r } else { 2.0 * r };
            g.mtext = linear_text(meas, d, st, if radius { "R" } else { "%%c" });
            let (tw, _) = measure(&g.mtext);
            let u = (b - center).normalized();
            let u = if u == Vec2::ZERO || !u.is_finite() { Vec2::X } else { u };
            let tip = center + u * r;
            let tpos = if d.user_text_pos { text_mid } else { tip + u * (asz.max(tsz) * 3.0) };
            if !radius {
                let start = center - u * r;
                g.dim(vec![start, tip]);
                end(&mut g, blk1, start, -u);
            }
            g.dim(vec![tip, tpos]);
            end(&mut g, blk2, tip, u);
            if st.text_outside_horizontal {
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
            if st.center_mark != 0.0 && st.center_mark.is_finite() {
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
            let txt = format_angle(sweep, st.angular_unit, st.angular_decimals, st.zero_suppression & 12);
            g.mtext = apply_override(&post(&st.post, &txt), d);
            let mut pts = Vec::new();
            arc.tessellate(r * 1e-3, &mut pts);
            g.dim(pts);
            let sp = arc.start_point();
            let ep = arc.end_point();
            if !st.suppress_ext1 {
                g.lines.extend(ext_line(vertex + (sp - vertex).normalized() * vertex.dist(a1).min(r), sp, st, k));
            }
            if !st.suppress_ext2 {
                g.lines.extend(ext_line(vertex + (ep - vertex).normalized() * vertex.dist(a2).min(r), ep, st, k));
            }
            g.line_roles.resize(g.lines.len(), LineRole::Ext);
            end(&mut g, blk1, sp, -(sp - vertex).perp().normalized());
            end(&mut g, blk2, ep, (ep - vertex).perp().normalized());
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
            g.mtext = linear_text(meas, d, st, "⌒");
            let mut pts = Vec::new();
            arc.tessellate(arc.radius * 1e-3, &mut pts);
            g.dim(pts);
            for (p, q) in [(d.p13.xy(), arc.start_point()), (d.p14.xy(), arc.end_point())] {
                g.lines.extend(ext_line(p, q, st, k));
            }
            g.line_roles.resize(g.lines.len(), LineRole::Ext);
            end(&mut g, blk1, arc.start_point(), -(arc.start_point() - c).perp().normalized());
            end(&mut g, blk2, arc.end_point(), (arc.end_point() - c).perp().normalized());
            let m = arc.mid_point();
            let radial = (m - c).normalized();
            text_angle = if st.text_inside_horizontal { 0.0 } else { readable(radial.perp().angle()) };
            if !d.user_text_pos {
                text_mid = m + radial * (gap + th / 2.0);
            }
        }
    }
    // DIMTEDIT Angle: an explicit text rotation wins.
    if d.text_rotation != 0.0 && d.text_rotation.is_finite() {
        text_angle = d.text_rotation;
    }
    g.value = decode_value(&cadcraft_fonts::plain_mtext(&g.mtext));
    if !g.mtext.is_empty() {
        let l = cadcraft_fonts::layout_mtext_with(&g.mtext, &params(text_mid, text_angle));
        for p in l.pieces {
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
        assert!(!g.text.is_empty());
        assert_eq!(g.fills.len(), 2);
        assert_eq!(g.lines_of(LineRole::Ext).count(), 2);
    }

    #[test]
    fn vertical_measures_dy() {
        let g = dimension_geometry(&lin((0.0, 0.0), (10.0, 3.0), (15.0, 0.0), std::f64::consts::FRAC_PI_2), &DimStyle::default(), 1.0);
        assert_eq!(g.value, "3.0000");
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
}
