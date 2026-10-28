//! Multilines (MLINE) and multiline styles (MLINESTYLE): parallel lines — the style's
//! elements — run at fixed offsets along a path of vertices, with optional caps, joints and a
//! fill between the outermost elements (DXF Reference: MLINE entity, MLINESTYLE object).
//!
//! An [`MLine`] carries a copy of its style ([`MLine::def`]), so its geometry needs nothing
//! else from the drawing: snapping, selection and trimming work from the entity alone.

use cadcraft_color::Color;
use cadcraft_geom::{Arc as GArc, Line as GLine, PI, Segment, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Most elements a multiline style has (as in the style dialog of the reference program).
pub const MAX_MLINE_ELEMENTS: usize = 16;
/// Most vertices of one multiline (hostile files).
pub const MAX_MLINE_VERTICES: usize = 100_000;
/// Most breaks per vertex (all elements together).
pub const MAX_MLINE_CUTS: usize = 1_000;
/// The style every drawing has.
pub const STANDARD_MLINE_STYLE: &str = "STANDARD";

/// One line of a multiline style: its offset from the style's zero line (+ = left of the
/// drawing direction), colour and linetype.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLineElement {
    pub offset: f64,
    pub color: Color,
    pub linetype: String,
}

impl Default for MLineElement {
    fn default() -> Self {
        MLineElement { offset: 0.0, color: Color::ByLayer, linetype: "ByLayer".into() }
    }
}

/// The caps at one end of a multiline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLineCaps {
    /// A line across all elements.
    pub line: bool,
    /// A half circle joining the outermost elements.
    pub outer_arc: bool,
    /// Half circles joining pairs of inner elements.
    pub inner_arcs: bool,
    /// Angle of the end against the drawing direction, radians (π/2 = square).
    pub angle: f64,
}

impl Default for MLineCaps {
    fn default() -> Self {
        MLineCaps { line: false, outer_arc: false, inner_arcs: false, angle: PI / 2.0 }
    }
}

/// A multiline style (MLSTYLE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLineStyle {
    pub name: String,
    /// The handle it was read with (0: none yet): written back, so other objects and other
    /// programs keep pointing at it.
    #[serde(default, skip_serializing_if = "crate::KeptHandle::is_none")]
    pub handle: crate::KeptHandle,
    pub description: String,
    /// Elements from the largest offset down.
    pub elements: Vec<MLineElement>,
    pub start_caps: MLineCaps,
    pub end_caps: MLineCaps,
    /// Lines across all elements at every vertex ("display joints").
    pub joints: bool,
    /// Fill colour between the outermost elements; `None` = no fill.
    pub fill: Option<Color>,
}

impl Default for MLineStyle {
    /// The STANDARD style: two lines one unit apart, no caps, no fill.
    fn default() -> Self {
        MLineStyle {
            handle: Default::default(),
            name: STANDARD_MLINE_STYLE.into(),
            description: String::new(),
            elements: vec![MLineElement { offset: 0.5, ..Default::default() }, MLineElement { offset: -0.5, ..Default::default() }],
            start_caps: MLineCaps::default(),
            end_caps: MLineCaps::default(),
            joints: false,
            fill: None,
        }
    }
}

impl MLineStyle {
    /// The style with hostile values made safe: 1..=16 finite elements sorted by offset (largest
    /// first), cap angles within 10°..170°.
    pub fn sanitized(mut self) -> MLineStyle {
        self.elements.retain(|e| e.offset.is_finite() && e.offset.abs() < 1e12);
        self.elements.truncate(MAX_MLINE_ELEMENTS);
        if self.elements.is_empty() {
            self.elements.push(MLineElement::default());
        }
        self.elements.sort_by(|a, b| b.offset.total_cmp(&a.offset));
        for c in [&mut self.start_caps, &mut self.end_caps] {
            c.angle = if c.angle.is_finite() { c.angle.clamp(10f64.to_radians(), 170f64.to_radians()) } else { PI / 2.0 };
        }
        self
    }
    pub fn max_offset(&self) -> f64 {
        self.elements.iter().map(|e| e.offset).fold(f64::NEG_INFINITY, f64::max).max(-1e12)
    }
    pub fn min_offset(&self) -> f64 {
        self.elements.iter().map(|e| e.offset).fold(f64::INFINITY, f64::min).min(1e12)
    }
}

/// Which line of the style runs through the picked points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MLineJust {
    /// The element with the largest offset.
    #[default]
    Top,
    /// The style's zero line.
    Zero,
    /// The element with the smallest offset.
    Bottom,
}

impl MLineJust {
    /// DXF group 70 of MLINE (and CMLJUST).
    pub fn to_dxf(self) -> i64 {
        match self {
            MLineJust::Top => 0,
            MLineJust::Zero => 1,
            MLineJust::Bottom => 2,
        }
    }
    pub fn from_dxf(v: i64) -> MLineJust {
        match v {
            1 => MLineJust::Zero,
            2 => MLineJust::Bottom,
            _ => MLineJust::Top,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            MLineJust::Top => "Top",
            MLineJust::Zero => "Zero",
            MLineJust::Bottom => "Bottom",
        }
    }
}

/// A break in one element along the segment that starts at a vertex: distances from the
/// element's start on that segment (MLEDIT Cut).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MLineCut {
    pub element: usize,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MLineVertex {
    pub p: Vec3,
    /// Breaks in the segment that starts here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<MLineCut>,
}

impl MLineVertex {
    pub fn new(p: Vec3) -> Self {
        MLineVertex { p, cuts: Vec::new() }
    }
}

/// A multiline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MLine {
    /// Style name (the MLINESTYLE it was drawn with).
    pub style: String,
    /// The style's elements, caps, joints and fill as this multiline uses them.
    #[serde(default)]
    pub def: MLineStyle,
    /// Element offsets are multiplied by this; negative flips the elements to the other side.
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub justification: MLineJust,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub no_start_caps: bool,
    #[serde(default)]
    pub no_end_caps: bool,
    pub vertices: Vec<MLineVertex>,
}

fn one() -> f64 {
    1.0
}

/// A multiline's drawn geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MLineGeometry {
    /// Element pieces: (element index into `def.elements`, from, to).
    pub lines: Vec<(usize, Vec2, Vec2)>,
    /// Caps and joints, drawn with the multiline's own properties.
    pub caps: Vec<Segment>,
    /// The fill between the outermost elements (even-odd loops); empty without fill.
    pub fill: Vec<Vec<Vec2>>,
}

impl MLine {
    /// A multiline through `pts` in style `def`.
    pub fn new(def: &MLineStyle, pts: &[Vec2], closed: bool, just: MLineJust, scale: f64) -> MLine {
        MLine {
            style: def.name.clone(),
            def: def.clone(),
            scale,
            justification: just,
            closed,
            no_start_caps: false,
            no_end_caps: false,
            vertices: pts.iter().map(|p| MLineVertex::new(p.to3(0.0))).collect(),
        }
    }

    /// The offset the vertex path sits at, in style units.
    pub fn ref_offset(&self) -> f64 {
        match self.justification {
            MLineJust::Top => self.def.max_offset(),
            MLineJust::Zero => 0.0,
            MLineJust::Bottom => self.def.min_offset(),
        }
    }

    /// Each element's distance from the vertex path (+ = left of the drawing direction).
    pub fn offsets(&self) -> Vec<f64> {
        let r = self.ref_offset();
        let s = if self.scale.is_finite() { self.scale } else { 1.0 };
        self.def.elements.iter().map(|e| (e.offset - r) * s).collect()
    }

    pub fn points(&self) -> Vec<Vec2> {
        self.vertices.iter().map(|v| v.p.xy()).collect()
    }

    /// Number of segments (the closing one included).
    pub fn segment_count(&self) -> usize {
        let n = self.vertices.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }

    /// Unit direction of each segment (zero for a degenerate one).
    pub fn directions(&self) -> Vec<Vec2> {
        let pts = self.points();
        let n = pts.len();
        (0..self.segment_count())
            .map(|i| (pts.get((i + 1) % n.max(1)).copied().unwrap_or_default() - pts.get(i).copied().unwrap_or_default()).normalized())
            .collect()
    }

    /// The miter vector at each vertex: the element at offset `d` passes through `v + m * d`.
    pub fn miters(&self) -> Vec<Vec2> {
        let dirs = self.directions();
        let n = self.vertices.len();
        let nseg = dirs.len();
        // A degenerate segment takes its neighbour's direction.
        let dir = |i: usize| -> Vec2 {
            (0..nseg).map(|k| dirs.get((i + k) % nseg.max(1)).copied().unwrap_or_default()).find(|d| *d != Vec2::ZERO).unwrap_or(Vec2::X)
        };
        let dir_back = |i: usize| -> Vec2 {
            (0..nseg)
                .map(|k| dirs.get((i + nseg - k % nseg) % nseg.max(1)).copied().unwrap_or_default())
                .find(|d| *d != Vec2::ZERO)
                .unwrap_or(Vec2::X)
        };
        let end = |d: Vec2, angle: f64| {
            let u = d.rotate(angle);
            let s = angle.sin();
            if s.abs() < 1e-6 { d.perp() } else { u / s }
        };
        (0..n)
            .map(|i| {
                if nseg == 0 {
                    return Vec2::Y;
                }
                let open_end = !self.closed && (i == 0 || i + 1 == n);
                if open_end {
                    return if i == 0 { end(dir(0), self.def.start_caps.angle) } else { end(dir_back(n - 2), PI - self.def.end_caps.angle) };
                }
                let d_in = dir_back((i + nseg - 1) % nseg);
                let d_out = dir(i % nseg);
                let (n1, n2) = (d_in.perp(), d_out.perp());
                let b = (n1 + n2).normalized();
                if b == Vec2::ZERO {
                    return n1;
                }
                // Keep sharp corners finite.
                b / b.dot(n1).max(0.05)
            })
            .collect()
    }

    /// The drawn geometry: element pieces (breaks taken out), caps, joints and fill.
    pub fn geometry(&self) -> MLineGeometry {
        let mut g = MLineGeometry::default();
        let pts = self.points();
        let n = pts.len();
        let nseg = self.segment_count();
        if nseg == 0 || pts.iter().any(|p| !p.is_finite()) {
            return g;
        }
        let miters = self.miters();
        let offs = self.offsets();
        let at = |k: usize, d: f64| pts.get(k % n).copied().unwrap_or_default() + miters.get(k % n).copied().unwrap_or_default() * d;
        for k in 0..nseg {
            let cuts = self.vertices.get(k).map(|v| v.cuts.as_slice()).unwrap_or(&[]);
            for (e, d) in offs.iter().enumerate() {
                let (a, b) = (at(k, *d), at(k + 1, *d));
                let len = a.dist(b);
                if len <= 1e-12 {
                    continue;
                }
                let dir = (b - a) / len;
                for (s, t) in pieces(len, cuts.iter().filter(|c| c.element == e).map(|c| (c.start, c.end))) {
                    g.lines.push((e, a + dir * s, a + dir * t));
                }
            }
        }
        // Elements by offset, largest (leftmost) first.
        let mut order: Vec<usize> = (0..offs.len()).collect();
        order.sort_by(|a, b| offs.get(*b).copied().unwrap_or(0.0).total_cmp(&offs.get(*a).copied().unwrap_or(0.0)));
        let (Some(&top), Some(&bottom)) = (order.first(), order.last()) else { return g };
        let off = |e: usize| offs.get(e).copied().unwrap_or(0.0);
        let across = |k: usize| Segment::Line(GLine::new(at(k, off(top)), at(k, off(bottom))));
        if !self.closed {
            for (k, caps, skip, start) in [(0, self.def.start_caps, self.no_start_caps, true), (n - 1, self.def.end_caps, self.no_end_caps, false)] {
                if skip {
                    continue;
                }
                if caps.line && order.len() > 1 {
                    g.caps.push(across(k));
                }
                let pairs = order.len() / 2;
                for i in 0..pairs {
                    if (i == 0 && !caps.outer_arc) || (i > 0 && !caps.inner_arcs) {
                        continue;
                    }
                    let (Some(&ea), Some(&eb)) = (order.get(i), order.get(order.len() - 1 - i)) else { continue };
                    let (a, b) = (at(k, off(ea)), at(k, off(eb)));
                    if let Some(arc) = half_circle(a, b, start) {
                        g.caps.push(arc);
                    }
                }
            }
        }
        if self.def.joints && order.len() > 1 {
            let inner = if self.closed { 0..n } else { 1..n.saturating_sub(1) };
            for k in inner {
                g.caps.push(across(k));
            }
        }
        if self.def.fill.is_some() {
            let side = |e: usize| (0..n).map(|k| at(k, off(e))).collect::<Vec<Vec2>>();
            if self.closed {
                g.fill = vec![side(top), side(bottom)];
            } else {
                let mut poly = side(top);
                poly.extend(side(bottom).into_iter().rev());
                g.fill = vec![poly];
            }
        }
        g
    }

    /// Segments of every element piece, cap and joint.
    pub fn segments(&self) -> Vec<Segment> {
        let g = self.geometry();
        g.lines.iter().map(|(_, a, b)| Segment::Line(GLine::new(*a, *b))).chain(g.caps).collect()
    }
}

/// The parts of `[0, len]` left after taking out the `gaps` (each `(start, end)`, any order).
fn pieces(len: f64, gaps: impl Iterator<Item = (f64, f64)>) -> Vec<(f64, f64)> {
    let mut gaps: Vec<(f64, f64)> =
        gaps.filter(|(a, b)| a.is_finite() && b.is_finite()).map(|(a, b)| (a.min(b).max(0.0), a.max(b).min(len))).filter(|(a, b)| b > a).collect();
    gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut from = 0.0;
    for (a, b) in gaps {
        if a > from + 1e-12 {
            out.push((from, a));
        }
        from = f64::max(from, b);
    }
    if len > from + 1e-12 {
        out.push((from, len));
    }
    out
}

/// A half circle from `a` to `b` bulging backwards (`start`) or forwards along the multiline.
fn half_circle(a: Vec2, b: Vec2, start: bool) -> Option<Segment> {
    let r = a.dist(b) / 2.0;
    if r <= 1e-12 {
        return None;
    }
    let c = a.mid(b);
    let (sa, sb) = (c.angle_to(a), c.angle_to(b));
    // From the left element to the right one: counter-clockwise passes behind the start.
    Some(if start { Segment::Arc { arc: GArc::new(c, r, sa, sb), ccw: true } } else { Segment::Arc { arc: GArc::new(c, r, sb, sa), ccw: false } })
}

impl crate::Drawing {
    /// The multiline style named `name` (any case).
    pub fn mline_style(&self, name: &str) -> Option<&MLineStyle> {
        self.mline_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }
    /// The current multiline style (CMLSTYLE), STANDARD when it is missing.
    pub fn current_mline_style(&self) -> MLineStyle {
        let name = self.header.str("CMLSTYLE", STANDARD_MLINE_STYLE);
        self.mline_style(&name).or_else(|| self.mline_style(STANDARD_MLINE_STYLE)).cloned().unwrap_or_default()
    }
}
