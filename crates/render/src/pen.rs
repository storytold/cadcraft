//! Plot style line ends, line joins and fill patterns ([`Pen`]).
//!
//! While a display list is built with a plot style table, every line and filled area records the
//! end, join and fill style of the style it prints with ([`crate::DisplayList::pens`]). [`apply`] then
//! turns them into plain geometry once the size on paper is known, so every writer (PDF, SVG,
//! PNG, the screen) draws them without knowing about plot styles:
//!
//! - a line wider than [`WIDE_MM`] on paper whose style sets an end or join style becomes the
//!   filled outline of the stroke, with butt, square, round or diamond ends and miter, bevel,
//!   round or diamond joins. "Use object" ends and joins stay round, as lines are drawn anyway;
//! - a filled area whose style sets a fill pattern is filled with that pattern instead.
//!
//! The patterns are CADCraft's own definitions, measured in millimetres on paper and anchored to
//! the paper origin. Layouts apply them while building (paper units are known); a model-space
//! plot applies them once its plot scale is known.

use cadcraft_doc::{EndStyle, FillStyle, JoinStyle};
use cadcraft_geom::Vec2;

use crate::{DisplayList, Kind};

/// End, join and fill style a primitive prints with (all "use object" when no plot style table
/// is applied).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pen {
    pub end: EndStyle,
    pub join: JoinStyle,
    pub fill: FillStyle,
}

impl Pen {
    /// Pen of a plot style.
    pub fn of(s: &cadcraft_doc::PlotStyle) -> Pen {
        Pen { end: s.end, join: s.join, fill: s.fill }
    }
    /// Whether [`apply`] has nothing to do for this pen.
    pub fn is_object(&self) -> bool {
        *self == Pen::default()
    }
    /// End or join styles that change how a wide line is drawn.
    fn shapes_lines(&self) -> bool {
        self.end != EndStyle::Object || self.join != JoinStyle::Object
    }
    /// A fill pattern that replaces a solid fill.
    fn patterned(&self) -> bool {
        !matches!(self.fill, FillStyle::Object | FillStyle::Solid)
    }
}

/// Lines up to this width on paper (mm) are drawn as plain strokes: their ends and joins are too
/// small to see.
pub const WIDE_MM: f32 = 0.25;

/// Repeat distance of the fill patterns on paper (mm).
pub const PATTERN_MM: f64 = 1.0;

/// Pattern cells visited per display list; past it the remaining areas stay solid (huge areas at
/// a tiny plot scale would otherwise produce unbounded geometry).
pub const MAX_PATTERN_CELLS: usize = 2_000_000;

/// Miters longer than this many half widths are drawn bevelled (as PDF's default miter limit).
pub(crate) const MITER_LIMIT: f64 = 10.0;

/// Segments of a half circle (round ends and joins).
const ROUND_STEPS: usize = 12;

/// Turn the pens of `list` into geometry for output where one drawing unit is `mm_per_unit`
/// millimetres on paper. Lines with end or join styles become filled outlines and patterned
/// fills become the pattern's pieces. Each primitive stays one primitive (same order, handle and
/// colour) and the pens are used up, so applying twice changes nothing.
pub fn apply(list: &mut DisplayList, mm_per_unit: f64) {
    let pens = std::mem::take(&mut list.pens);
    if !(mm_per_unit.is_finite() && mm_per_unit > 0.0) {
        return;
    }
    let mut budget = MAX_PATTERN_CELLS;
    for (i, pen) in pens.iter().enumerate() {
        let Some(p) = list.prims.get(i).copied() else { break };
        let out = match p.kind {
            Kind::Polyline if pen.shapes_lines() && p.lw > WIDE_MM => {
                let hw = f64::from(p.lw) / 2.0 / mm_per_unit;
                Some(stroke(list.points(&p), hw, pen.end, pen.join))
            }
            Kind::Tris if pen.patterned() => pattern(list.points(&p), pen.fill, PATTERN_MM / mm_per_unit, &mut budget),
            _ => None,
        };
        let Some(tris) = out else { continue };
        let (Ok(start), Ok(len)) = (u32::try_from(list.tris.len()), u32::try_from(tris.len())) else { continue };
        if start.checked_add(len).is_none() {
            continue;
        }
        let Some(prim) = list.prims.get_mut(i) else { break };
        prim.kind = Kind::Tris;
        prim.start = start;
        prim.len = len;
        prim.lw = 0.0;
        list.tris.extend_from_slice(&tris);
    }
}

/// Push triangle `a b c` counter-clockwise (so overlapping pieces fill as a union under the
/// non-zero rule); degenerate ones are dropped.
fn tri(out: &mut Vec<Vec2>, a: Vec2, b: Vec2, c: Vec2) {
    let area = (b - a).cross(c - a);
    if !area.is_finite() || area.abs() <= 1e-300 {
        return;
    }
    if area > 0.0 {
        out.extend_from_slice(&[a, b, c]);
    } else {
        out.extend_from_slice(&[a, c, b]);
    }
}

/// Fan of the arc around `c` from `from` to `to` (radius `r`), turning the short way.
fn round_fan(out: &mut Vec<Vec2>, c: Vec2, r: f64, from: Vec2, to: Vec2) {
    let a0 = from.y.atan2(from.x);
    let mut sweep = to.y.atan2(to.x) - a0;
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    let steps = ((sweep.abs() / std::f64::consts::PI * ROUND_STEPS as f64).ceil() as usize).clamp(1, ROUND_STEPS * 2);
    let mut prev = c + Vec2::from_angle(a0) * r;
    for k in 1..=steps {
        let q = c + Vec2::from_angle(a0 + sweep * k as f64 / steps as f64) * r;
        tri(out, c, prev, q);
        prev = q;
    }
}

/// Triangles of the outline of a line through `pts`, `hw` half width, with `end` caps and `join`
/// corners ("use object" = round). A polyline whose last point is its first is closed: joins all
/// round, no ends.
pub fn stroke(pts: &[Vec2], hw: f64, end: EndStyle, join: JoinStyle) -> Vec<Vec2> {
    let mut out = Vec::new();
    if !(hw.is_finite() && hw > 0.0) {
        return out;
    }
    let eps = hw * 1e-9;
    let mut v: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts.iter().filter(|p| p.is_finite()) {
        if v.last().is_none_or(|l| !l.near(*p, eps)) {
            v.push(*p);
        }
    }
    let closed = v.len() > 2 && v.first().zip(v.last()).is_some_and(|(a, b)| a.near(*b, eps));
    if closed {
        v.pop();
    }
    let n = v.len();
    let Some(first) = v.first().copied() else { return out };
    if n == 1 {
        // A dot: both ends' caps back to back.
        cap(&mut out, first, Vec2::X, hw, end);
        cap(&mut out, first, -Vec2::X, hw, end);
        return out;
    }
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let (Some(a), Some(b)) = (v.get(i).copied(), v.get((i + 1) % n).copied()) else { continue };
        let nrm = (b - a).normalized().perp() * hw;
        tri(&mut out, a + nrm, a - nrm, b - nrm);
        tri(&mut out, a + nrm, b - nrm, b + nrm);
    }
    let corners = if closed { 0..n } else { 1..n - 1 };
    for i in corners {
        let (Some(p), Some(c), Some(q)) = (v.get((i + n - 1) % n).copied(), v.get(i).copied(), v.get((i + 1) % n).copied()) else {
            continue;
        };
        corner(&mut out, c, (c - p).normalized(), (q - c).normalized(), hw, join);
    }
    if !closed && let (Some(a), Some(a1), Some(b0), Some(b)) = (v.first(), v.get(1), v.get(n - 2), v.last()) {
        cap(&mut out, *a, (*a - *a1).normalized(), hw, end);
        cap(&mut out, *b, (*b - *b0).normalized(), hw, end);
    }
    out
}

/// The cap at end point `p` of a line leaving it in unit direction `out_dir`.
fn cap(out: &mut Vec<Vec2>, p: Vec2, out_dir: Vec2, hw: f64, end: EndStyle) {
    let n = out_dir.perp() * hw;
    match end {
        EndStyle::Butt => {}
        EndStyle::Square => {
            let f = out_dir * hw;
            tri(out, p + n, p - n, p - n + f);
            tri(out, p + n, p - n + f, p + n + f);
        }
        EndStyle::Diamond => tri(out, p + n, p - n, p + out_dir * hw),
        EndStyle::Round | EndStyle::Object => round_fan(out, p, hw, n, -n),
    }
}

/// The corner at `c` between a segment arriving in direction `d0` and one leaving in `d1`.
fn corner(out: &mut Vec<Vec2>, c: Vec2, d0: Vec2, d1: Vec2, hw: f64, join: JoinStyle) {
    let turn = d0.cross(d1);
    if turn.abs() <= 1e-12 && d0.dot(d1) > 0.0 {
        return; // straight on
    }
    // The outer side of the turn: right of a left turn, left of a right turn.
    let side = if turn > 0.0 { -1.0 } else { 1.0 };
    let n0 = d0.perp() * (hw * side);
    let n1 = d1.perp() * (hw * side);
    let (o0, o1) = (c + n0, c + n1);
    let bis = (n0 + n1).normalized();
    // Distance from the corner to the miter tip, in half widths.
    let cos_half = bis.dot(n0) / hw;
    let miter = if cos_half > 1e-9 { 1.0 / cos_half } else { f64::INFINITY };
    match join {
        JoinStyle::Bevel => tri(out, c, o0, o1),
        JoinStyle::Miter if miter <= MITER_LIMIT => {
            let tip = c + bis * (hw * miter);
            tri(out, c, o0, tip);
            tri(out, c, tip, o1);
        }
        JoinStyle::Miter => tri(out, c, o0, o1),
        JoinStyle::Diamond => {
            // A point one half width out along the bisector, like a diamond end.
            tri(out, c, o0, o1);
            tri(out, o0, c + bis * hw, o1);
        }
        JoinStyle::Round | JoinStyle::Object => round_fan(out, c, hw, n0, n1),
    }
}

/// A family of pattern cells: bands across `a` (`width` wide every `pitch`), crossed with bands
/// across `b` when given; `checker` keeps every other cell of the grid.
struct Grid {
    a: Vec2,
    b: Option<Vec2>,
    pitch: f64,
    width: f64,
    checker: bool,
}

/// The cell families of a fill pattern with repeat distance `p` (drawing units).
fn grids(fill: FillStyle, p: f64) -> Vec<Grid> {
    let g = |a: Vec2, b: Option<Vec2>, pitch: f64, width: f64| Grid { a, b, pitch, width, checker: false };
    let r = std::f64::consts::FRAC_1_SQRT_2;
    match fill {
        FillStyle::Object | FillStyle::Solid => Vec::new(),
        FillStyle::HorizontalBars => vec![g(Vec2::Y, None, p, p / 3.0)],
        FillStyle::VerticalBars => vec![g(Vec2::X, None, p, p / 3.0)],
        // Bands across (1, -1) run like `/`, across (1, 1) like `\`.
        FillStyle::SlantRight => vec![g(Vec2::new(r, -r), None, p, p / 3.0)],
        FillStyle::SlantLeft => vec![g(Vec2::new(r, r), None, p, p / 3.0)],
        FillStyle::Crosshatch => vec![g(Vec2::Y, None, p, p / 5.0), g(Vec2::X, None, p, p / 5.0)],
        FillStyle::SquareDots => vec![g(Vec2::X, Some(Vec2::Y), p / 2.0, p / 5.0)],
        FillStyle::Checkerboard => vec![Grid { checker: true, ..g(Vec2::X, Some(Vec2::Y), p / 2.0, p / 2.0) }],
        FillStyle::Diamonds => vec![g(Vec2::new(r, r), Some(Vec2::new(r, -r)), p * 0.7, p * 0.35)],
    }
}

/// Keep the part of convex polygon `poly` where `dot(n, p) >= c`.
fn clip_half(poly: &[Vec2], n: Vec2, c: f64) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    let Some(mut prev) = poly.last().copied() else { return out };
    let mut dp = n.dot(prev) - c;
    for &q in poly {
        let dq = n.dot(q) - c;
        if (dp >= 0.0) != (dq >= 0.0) {
            let t = dp / (dp - dq);
            out.push(prev + (q - prev) * t);
        }
        if dq >= 0.0 {
            out.push(q);
        }
        prev = q;
        dp = dq;
    }
    out
}

/// Range of band indices across `axis` that a triangle can touch.
fn band_range(t: &[Vec2; 3], axis: Vec2, pitch: f64) -> Option<(f64, f64)> {
    let (lo, hi) = t.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(axis.dot(*p)), hi.max(axis.dot(*p))));
    let (a, b) = ((lo / pitch).floor() - 1.0, (hi / pitch).floor());
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

/// The pieces of the triangles `tris` covered by `fill`'s pattern at repeat distance `pitch`, or
/// `None` (draw solid) when the pattern would take more than the remaining `budget` of cells.
pub fn pattern(tris: &[Vec2], fill: FillStyle, pitch: f64, budget: &mut usize) -> Option<Vec<Vec2>> {
    if !(pitch.is_finite() && pitch > 0.0) {
        return None;
    }
    let families = grids(fill, pitch);
    let mut out = Vec::new();
    for t in tris.as_chunks::<3>().0 {
        if t.iter().any(|p| !p.is_finite()) {
            continue;
        }
        let [a, b, c] = *t;
        let base: Vec<Vec2> = if (b - a).cross(c - a) >= 0.0 { vec![a, b, c] } else { vec![a, c, b] };
        for g in &families {
            let (i0, i1) = band_range(t, g.a, g.pitch)?;
            let (j0, j1) = match g.b {
                Some(axis) => band_range(t, axis, g.pitch)?,
                None => (0.0, 0.0),
            };
            let cells = (i1 - i0 + 1.0) * (j1 - j0 + 1.0);
            if !(cells.is_finite() && cells <= *budget as f64) {
                return None;
            }
            *budget = budget.saturating_sub(cells as usize);
            // Counted loops: at huge coordinates `i + 1.0` may not move.
            for ki in 0..=(i1 - i0) as usize {
                let i = i0 + ki as f64;
                let s = i * g.pitch;
                let band = clip_half(&clip_half(&base, g.a, s), -g.a, -(s + g.width));
                if band.len() < 3 {
                    continue;
                }
                let Some(axis) = g.b else {
                    fan(&mut out, &band);
                    continue;
                };
                for kj in 0..=(j1 - j0) as usize {
                    let j = j0 + kj as f64;
                    if g.checker && !(ki + kj + (i0 + j0).rem_euclid(2.0) as usize).is_multiple_of(2) {
                        continue;
                    }
                    let s = j * g.pitch;
                    fan(&mut out, &clip_half(&clip_half(&band, axis, s), -axis, -(s + g.width)));
                }
            }
        }
    }
    Some(out)
}

/// Triangle fan of a convex polygon.
fn fan(out: &mut Vec<Vec2>, poly: &[Vec2]) {
    let Some(first) = poly.first().copied() else { return };
    for w in poly.get(1..).unwrap_or(&[]).windows(2) {
        if let [b, c] = w {
            tri(out, first, *b, *c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(tris: &[Vec2]) -> f64 {
        tris.as_chunks::<3>().0.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a) / 2.0).sum()
    }

    #[test]
    fn caps_change_the_outline_length() {
        let line = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        let butt = area(&stroke(&line, 1.0, EndStyle::Butt, JoinStyle::Miter));
        let square = area(&stroke(&line, 1.0, EndStyle::Square, JoinStyle::Miter));
        let diamond = area(&stroke(&line, 1.0, EndStyle::Diamond, JoinStyle::Miter));
        let round = area(&stroke(&line, 1.0, EndStyle::Round, JoinStyle::Miter));
        assert!((butt - 20.0).abs() < 1e-9, "{butt}");
        assert!((square - 24.0).abs() < 1e-9, "{square}");
        assert!((diamond - 22.0).abs() < 1e-9, "{diamond}");
        assert!(round > 23.0 && round < 20.0 + std::f64::consts::PI, "{round}");
    }

    #[test]
    fn pattern_is_bounded() {
        let big = [Vec2::new(0.0, 0.0), Vec2::new(1e9, 0.0), Vec2::new(0.0, 1e9)];
        let mut budget = 1000;
        assert!(pattern(&big, FillStyle::Checkerboard, 1.0, &mut budget).is_none());
        let mut budget = MAX_PATTERN_CELLS;
        assert!(pattern(&big, FillStyle::Checkerboard, f64::NAN, &mut budget).is_none());
        assert!(stroke(&[Vec2::new(f64::NAN, 0.0)], 1.0, EndStyle::Round, JoinStyle::Round).is_empty());
    }
}
