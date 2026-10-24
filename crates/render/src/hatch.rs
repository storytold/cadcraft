//! Hatch drawing: island detection picks the boundary loops that take part, then a solid,
//! gradient or background fill (even-odd triangles) and the pattern lines (each pattern line
//! family swept across the boundary, even-odd clipped, then dashed).
//!
//! On-screen builds get a level of detail (`Options::min_dash` > 0 is two pixels, `Options::view`
//! the area being looked at): a pattern whose lines run together on screen is drawn as a solid
//! fill of its boundary ([`reads_solid`]), a dash or dot repeat under two pixels as a continuous
//! line (as linetypes already are), and nothing is generated outside the view. Plots, exports and
//! picks pass `min_dash = 0` and `view = None` and get every line.

use super::{Builder, Ctx, Ink, fill, ink, max_scale};
use cadcraft_color::{Color, Rgb};
use cadcraft_doc::library::{PatternLine, pattern_for};
use cadcraft_doc::{Drawing, Entity, Gradient, Hatch};
use cadcraft_geom::{Bounds2, Polyline, Vec2, point_in_polygon};
use std::sync::atomic::{AtomicBool, Ordering};

/// Draw a hatch entity: `rgb` is its resolved colour, `tol` the chord tolerance.
/// Whether a build's cancel flag has been set.
fn is_set(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

pub(super) fn draw(b: &mut Builder, ctx: &Ctx, e: &Entity, h: &Hatch, rgb: Ink, lw: f32, tol: f64) {
    let loops = fill_loops(h, tol);
    // Gradient and background colours given ByLayer follow the hatch's own layer.
    let layer = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer).unwrap_or(Color::Index(7));
    // Level of detail: a pixel in the hatch's own units, along its most stretched axis (so lines
    // that show apart in any direction are never merged); 0 = full detail.
    let stretch = max_scale(&ctx.xf);
    let px = if stretch > 1e-12 { b.opts.min_dash / 2.0 / stretch } else { 0.0 };
    let dense = b.opts.fill && reads_solid(ctx.d, h, px);
    if h.solid || h.pattern.eq_ignore_ascii_case("SOLID") || h.gradient.is_some() || dense {
        if b.opts.fill {
            let tris = fill::triangulate_evenodd(&loops);
            match &h.gradient {
                Some(g) => gradient(b, ctx, g, layer_color, &tris),
                None => b.tris(ctx, rgb, &tris),
            }
        }
        return;
    }
    if let Some(bg) = h.background
        && b.opts.fill
    {
        b.tris(ctx, ink(bg, layer_color, ctx.block_color), &fill::triangulate_evenodd(&loops));
    }
    // The area being looked at, in the hatch's units.
    let view = b.opts.view.and_then(|v| {
        let inv = ctx.xf.inverse()?;
        Some(Bounds2::from_points(v.corners().iter().map(|p| inv.apply(*p))))
    });
    for seg in pattern_lines_until(ctx.d, h, &loops, px, view.as_ref(), b.opts.cancel.as_deref()) {
        if seg.len() == 1 {
            if let Some(p) = seg.first() {
                b.point(ctx, rgb, *p);
            }
        } else {
            b.polyline(ctx, rgb, lw, &seg);
        }
    }
}

/// Upper bound on loop count × vertex count for island detection (the nesting test compares
/// every loop with every other); past it all loops are kept (odd parity).
const MAX_ISLAND_WORK: usize = 20_000_000;

/// True when two polygons have the same vertices (ignoring cyclic shift and winding order).
fn same_polygon(a: &[Vec2], b: &[Vec2]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let n = a.len();
    if n == 0 {
        return true;
    }
    let near = |p1: Vec2, p2: Vec2| p1.near(p2, 1e-4);
    (0..n).any(|k| (0..n).all(|i| near(a[i], b[(i + k) % n]))) || (0..n).any(|k| (0..n).all(|i| near(a[i], b[(k + n - (i % n)) % n])))
}

/// Remove degenerate and duplicate boundary loops within a shared comparison budget.
/// Retain remaining loops without further comparisons once the budget is exhausted.
fn dedup_polygons(loops: &mut Vec<Vec<Vec2>>) {
    // `same_polygon` is O(n²) in a polygon's vertex count (every cyclic alignment, both windings),
    // and each loop is compared against every unique one kept so far, so adversarial boundary
    // geometry could force quadratic-on-quadratic work. Charge every vertex comparison against a
    // shared budget; once it is spent, stop deduplicating and keep the remaining loops as-is
    // (CWE-407).
    let mut budget: u64 = MAX_DEDUP_COMPARISONS;
    let mut unique: Vec<Vec<Vec2>> = Vec::with_capacity(loops.len());
    for l in loops.drain(..) {
        if l.len() < 3 {
            continue;
        }
        let mut dup = false;
        for u in &unique {
            if u.len() != l.len() {
                continue;
            }
            // Each `same_polygon` call costs about 2·len² comparisons; stop if that would overrun.
            let cost = 2u64.saturating_mul((u.len() as u64).saturating_mul(l.len() as u64));
            match budget.checked_sub(cost) {
                Some(left) => budget = left,
                None => {
                    // Out of budget: keep `l` without further comparison.
                    budget = 0;
                    break;
                }
            }
            if same_polygon(u, &l) {
                dup = true;
                break;
            }
        }
        if !dup {
            unique.push(l);
        }
    }
    *loops = unique;
}

/// Vertex comparisons `dedup_polygons` may perform before it stops deduplicating.
const MAX_DEDUP_COMPARISONS: u64 = 50_000_000;

/// The tessellated boundary loops that take part in the fill under the hatch's island
/// detection style (DXF group 75), filled with odd parity: 0 normal keeps every loop (islands
/// within islands alternate), 1 outer keeps the outermost loops and their first islands, 2
/// ignore keeps only the outermost loops. Nesting depth = how many other loops enclose a loop.
pub fn fill_loops(h: &Hatch, tol: f64) -> Vec<Vec<Vec2>> {
    let mut loops: Vec<Vec<Vec2>> = h.loops.iter().map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(tol)).collect();
    dedup_polygons(&mut loops);
    let max_depth = match h.style {
        1 => 1,
        2 => 0,
        _ => return loops,
    };
    let verts: usize = loops.iter().map(Vec::len).sum();
    if loops.len().saturating_mul(verts) > MAX_ISLAND_WORK {
        return loops;
    }
    let depth = |i: usize, l: &[Vec2]| {
        let Some(p) = probe(l) else { return 0 };
        loops.iter().enumerate().filter(|(j, o)| *j != i && point_in_polygon(o, p)).count()
    };
    loops.iter().enumerate().filter(|(i, l)| depth(*i, l) <= max_depth).map(|(_, l)| l.clone()).collect()
}

/// A point of a loop to test its nesting with: the middle of its first non-degenerate edge
/// (vertices may touch an enclosing loop; edge midpoints rarely do).
fn probe(l: &[Vec2]) -> Option<Vec2> {
    let n = l.len();
    (0..n).find_map(|i| {
        let (a, b) = (*l.get(i)?, *l.get((i + 1) % n)?);
        (a.dist(b) > 1e-12).then(|| (a + b) * 0.5).filter(|m| m.is_finite())
    })
}

/// Colour steps a gradient is drawn with (one display-list primitive each).
const GRADIENT_LEVELS: usize = 64;
/// Gradient triangles are split until no edge is longer than the larger extent over this.
const GRADIENT_CELLS: f64 = 64.0;
/// Upper bound on the triangles of one gradient fill.
const MAX_GRADIENT_TRIS: usize = 200_000;

/// A two-colour gradient over the fill triangles `tris`: a base fill in the middle colour (it
/// hides seams between steps), then the triangles split into small pieces, each drawn in the
/// colour step of the gradient value at its centroid.
fn gradient(b: &mut Builder, ctx: &Ctx, g: &Gradient, layer: Color, tris: &[Vec2]) {
    let c1 = ink(g.color1, layer, ctx.block_color);
    let c2 = ink(g.color2, layer, ctx.block_color);
    b.tris(ctx, mix(c1, c2, 0.5), tris);
    let Some(field) = Field::new(g, tris) else { return };
    let mut steps: Vec<Vec<Vec2>> = vec![Vec::new(); GRADIENT_LEVELS];
    let max2 = field.cell * field.cell;
    let mut stack: Vec<[Vec2; 3]> = tris.as_chunks::<3>().0.to_vec();
    let mut drawn = 0usize;
    while let Some([a, bb, c]) = stack.pop() {
        // Split the longest edge while the budget allows.
        let edges = [(a, bb, c), (bb, c, a), (c, a, bb)];
        let (p, q, r) = edges.into_iter().max_by(|x, y| (x.0 - x.1).len2().total_cmp(&(y.0 - y.1).len2())).unwrap_or((a, bb, c));
        if (p - q).len2() > max2 && drawn + stack.len() + 2 <= MAX_GRADIENT_TRIS {
            let m = (p + q) * 0.5;
            stack.push([p, m, r]);
            stack.push([m, q, r]);
            continue;
        }
        let t = field.value((a + bb + c) / 3.0);
        let k = (t * (GRADIENT_LEVELS - 1) as f64).round() as usize;
        if let Some(s) = steps.get_mut(k.min(GRADIENT_LEVELS - 1)) {
            s.extend_from_slice(&[a, bb, c]);
        }
        drawn += 1;
    }
    for (k, s) in steps.iter().enumerate() {
        b.tris(ctx, mix(c1, c2, k as f64 / (GRADIENT_LEVELS - 1) as f64), s);
    }
}

/// `a` blended towards `b` by `t`; colour 7 only when both ends are.
fn mix(a: Ink, b: Ink, t: f64) -> Ink {
    let l = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    // The plot style follows the nearer end's colour.
    let aci = if t < 0.5 { a.aci } else { b.aci };
    Ink { rgb: Rgb(l(a.rgb.0, b.rgb.0), l(a.rgb.1, b.rgb.1), l(a.rgb.2, b.rgb.2)), aci7: a.aci7 && b.aci7, aci }
}

/// Gradient shapes by name (the `INV` forms swap the two colours).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    /// Colour 1 to colour 2 along the gradient direction.
    Linear,
    /// Colour 1 at both sides, colour 2 along the middle.
    Cylinder,
    /// Like linear but bending quickly towards colour 2.
    Curved,
    /// Colour 2 at the centre, colour 1 from the farthest side outwards.
    Spherical,
    /// Like spherical, centred on the bottom edge (in the gradient direction's frame).
    Hemispherical,
}

/// The gradient value (0 = colour 1, 1 = colour 2) over the fill, in a frame whose x axis is
/// the gradient direction.
struct Field {
    shape: Shape,
    inverted: bool,
    dir: Vec2,
    /// Fill bounds in the gradient frame.
    bb: Bounds2,
    /// Centre in the gradient frame (moved up and back when the gradient isn't centred).
    centre: Vec2,
    /// Radius at which a radial gradient reaches colour 1: the centre's distance to the farthest
    /// side of `bb` (the corners beyond it are colour 1).
    reach: f64,
    /// Largest edge length of the pieces drawn.
    cell: f64,
}

impl Field {
    fn new(g: &Gradient, tris: &[Vec2]) -> Option<Field> {
        let name = g.name.trim().to_ascii_uppercase();
        let (inverted, base) = match name.strip_prefix("INV") {
            Some(rest) => (true, rest),
            None => (false, name.as_str()),
        };
        let shape = match base {
            "CYLINDER" => Shape::Cylinder,
            "CURVED" => Shape::Curved,
            "SPHERICAL" => Shape::Spherical,
            "HEMISPHERICAL" => Shape::Hemispherical,
            _ => Shape::Linear,
        };
        let angle = if g.angle.is_finite() { g.angle } else { 0.0 };
        let dir = Vec2::new(angle.cos(), angle.sin());
        let bb = Bounds2::from_points(tris.iter().filter(|p| p.is_finite()).map(|p| Vec2::new(p.dot(dir), p.dot(dir.perp()))));
        let size = bb.width().max(bb.height());
        if bb.is_empty() || !(size.is_finite() && size > 1e-12) {
            return None;
        }
        let (fx, fy) = if g.centered { (0.5, 0.5) } else { (0.3, 0.7) };
        let mut centre = Vec2::new(bb.min.x + bb.width() * fx, bb.min.y + bb.height() * fy);
        if shape == Shape::Hemispherical {
            centre.y = bb.min.y;
        }
        let reach = [centre.x - bb.min.x, bb.max.x - centre.x, centre.y - bb.min.y, bb.max.y - centre.y].into_iter().fold(0.0, f64::max).max(1e-12);
        Some(Field { shape, inverted, dir, bb, centre, reach, cell: size / GRADIENT_CELLS })
    }

    /// The value at world point `p`, in 0..=1.
    fn value(&self, p: Vec2) -> f64 {
        let q = Vec2::new(p.dot(self.dir), p.dot(self.dir.perp()));
        // Position along the direction, 0..1, with the centre mapped to 0.5.
        let along = || {
            let (lo, c, hi) = (self.bb.min.x, self.centre.x, self.bb.max.x);
            if q.x < c { 0.5 * (q.x - lo) / (c - lo).max(1e-12) } else { 0.5 + 0.5 * (q.x - c) / (hi - c).max(1e-12) }
        };
        let t = match self.shape {
            Shape::Linear => along(),
            Shape::Cylinder => 1.0 - (along() - 0.5).abs() * 2.0,
            Shape::Curved => 1.0 - (1.0 - along().clamp(0.0, 1.0)).powi(2),
            Shape::Spherical | Shape::Hemispherical => 1.0 - q.dist(self.centre) / self.reach,
        };
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        if self.inverted { 1.0 - t } else { t }
    }
}

/// Upper bound on the line families drawn from a pattern definition read from a file.
const MAX_PATTERN_LINES: usize = 1024;
/// Upper bound on the runs of one hatch (AutoCAD's HPMAXLINES default), checked as runs are made
/// (a hostile or mis-scaled pattern can't exhaust memory). A view-limited build makes at most the
/// runs of a full build, so it never leaves out runs that a full build draws.
const MAX_RUNS: usize = 1_000_000;
/// Shared budget on dash-processing steps across all of a hatch's families and spans, counted
/// independently of emitted geometry so gap-only patterns cannot spin without bound (CWE-400).
const MAX_HATCH_WORK: u64 = 50_000_000;
/// Upper bound on the lines one family sweeps over the whole hatch; past it the family is left
/// out (a full-detail build of a pattern far too fine for its area).
const MAX_FAMILY_LINES: i64 = 20_000;
/// The same bound when the sweep is limited to a view: the count then follows the view's size
/// on screen, and lines closer than a pixel already read as solid, so only very large displays
/// come near it.
const MAX_VIEW_FAMILY_LINES: i64 = 100_000;
/// Line families closer than this on screen (pixels) run together: the hatch reads as solid.
pub const SOLID_SPACING_PX: f64 = 1.0;
/// Average marks per pixel from which a pattern reads as solid (scattered marks leave about
/// e^-2 ≈ 14% of the pixels empty there).
pub const SOLID_COVERAGE: f64 = 2.0;

/// The line families a hatch draws with: the lines its file defined (as its author's CAD drew
/// them), else the library's pattern of that name as drawn in `d`, else ANSI31.
fn lines_of(d: &Drawing, h: &Hatch) -> Vec<PatternLine> {
    if h.pattern_lines.is_empty() {
        pattern_for(d, &h.pattern).or_else(|| pattern_for(d, "ANSI31")).map(|p| p.lines).unwrap_or_default()
    } else {
        h.pattern_lines.iter().take(MAX_PATTERN_LINES).cloned().collect()
    }
}

/// The hatch's pattern scale (1 when unusable).
fn scale_of(h: &Hatch) -> f64 {
    if h.scale.is_finite() && h.scale > 1e-9 { h.scale } else { 1.0 }
}

/// True when, at `px` hatch units per pixel, the pattern covers its area on screen, so drawing
/// it line by line would fill (nearly) every pixel anyway. Each line, dash or dot is drawn at
/// least a pixel wide, so that is the case when
/// - some line family has its lines closer than [`SOLID_SPACING_PX`] and no gap along them a
///   pixel or longer (continuous lines, or a repeat that reads as a continuous line), or
/// - the families together put on average [`SOLID_COVERAGE`] or more marks on every pixel
///   (dense dot or dash patterns made of several families).
///
/// A sparse dot or dash pattern keeps its look. `px <= 0` (full detail) is never solid.
pub fn reads_solid(d: &Drawing, h: &Hatch, px: f64) -> bool {
    if !(px.is_finite() && px > 0.0) {
        return false;
    }
    let scale = scale_of(h);
    let mut coverage = 0.0;
    for f in lines_of(d, h) {
        let spacing = (f.delta.1 * scale).abs();
        if !(spacing.is_finite() && spacing > 0.0) {
            continue;
        }
        let pat_len: f64 = f.dashes.iter().map(|d| (d * scale).abs()).sum();
        // The share of a line inked on screen (a dash or dot is at least a pixel long there).
        let ink: f64 = f.dashes.iter().filter(|d| **d >= 0.0).map(|d| (d * scale).max(px)).sum();
        let inked = if f.dashes.is_empty() || (ink > 0.0 && pat_len < 2.0 * px) {
            1.0
        } else if pat_len > 0.0 {
            (ink / pat_len).min(1.0)
        } else {
            0.0
        };
        let gap = f.dashes.iter().filter(|d| **d < 0.0).map(|d| (d * scale).abs()).fold(0.0, f64::max);
        let draws = f.dashes.is_empty() || ink > 0.0;
        if spacing < SOLID_SPACING_PX * px && draws && (inked >= 1.0 || gap < px) {
            return true;
        }
        coverage += inked * px / spacing;
    }
    coverage.is_finite() && coverage >= SOLID_COVERAGE
}

/// Hatch line runs in world coordinates (single-point runs are dots) for the fill `loops`.
/// `px`: a pixel in the hatch's units for the level of detail (a dash or dot repeat under two
/// pixels is one continuous run); 0 = full detail, as plots, exports and picks use. `view`: only
/// the part of the hatch inside it is generated; `None` = all of it.
pub fn pattern_lines(d: &Drawing, h: &Hatch, loops: &[Vec<Vec2>], px: f64, view: Option<&Bounds2>) -> Vec<Vec<Vec2>> {
    pattern_lines_until(d, h, loops, px, view, None)
}

/// [`pattern_lines`] that stops early (with what it has) once `cancel` is set: a dense pattern
/// over a large area is where most of a cancelled build's time would go.
pub(crate) fn pattern_lines_until(
    d: &Drawing,
    h: &Hatch,
    loops: &[Vec<Vec2>],
    px: f64,
    view: Option<&Bounds2>,
    cancel: Option<&AtomicBool>,
) -> Vec<Vec<Vec2>> {
    let lines = lines_of(d, h);
    let scale = scale_of(h);
    let px = if px.is_finite() && px > 0.0 { px } else { 0.0 };
    let mut out = Vec::new();
    // Shared work budget across all families and spans, counting dash-processing steps even when
    // they emit no geometry: gap-only families produce no runs, so `out.len()` alone cannot bound
    // their cost (CWE-400).
    let mut work: u64 = MAX_HATCH_WORK;
    for fam in &lines {
        // A family with dashes but no positive (ink) element can never draw: skip it so its gaps
        // are not swept at all.
        if !fam.dashes.is_empty() && !fam.dashes.iter().any(|d| *d >= 0.0) {
            continue;
        }
        let ang = fam.angle.to_radians() + h.angle;
        let origin = h.origin + Vec2::new(fam.origin.0, fam.origin.1).rotate(h.angle) * scale;
        let delta = Vec2::new(fam.delta.0, fam.delta.1).rotate(ang) * scale;
        let spacing = (Vec2::new(fam.delta.0, fam.delta.1).y * scale).abs();
        if !spacing.is_finite() || spacing < 1e-9 {
            continue;
        }
        let dashes: Vec<f64> = fam.dashes.iter().map(|d| d * scale).collect();
        Family { origin, ang, delta, spacing, dashes: &dashes }.sweep(loops, px, view, cancel, &mut out, &mut work);
        if out.len() >= MAX_RUNS || work == 0 || is_set(cancel) {
            break;
        }
    }
    out
}

/// Repeats of a pattern across a swatch ([`pattern_swatch`]).
const SWATCH_REPEATS: f64 = 6.0;
/// Most runs a swatch holds.
const MAX_SWATCH_RUNS: usize = 4000;

/// A preview of a pattern for a pattern picker: the runs of its line families `lines` inside the
/// `side` × `side` square with a corner at the origin, at a scale that shows a few repeats of the
/// pattern (each family's line spacing or dash repeat, whichever is longer). Empty for a pattern
/// with no lines (SOLID).
pub fn pattern_swatch(lines: &[PatternLine], side: f64) -> Vec<Vec<Vec2>> {
    let repeat =
        lines.iter().map(|l| l.delta.1.abs().max(l.dashes.iter().map(|d| d.abs()).sum::<f64>())).filter(|r| r.is_finite()).fold(0.0, f64::max);
    if !side.is_finite() || side <= 0.0 || repeat < 1e-9 {
        return Vec::new();
    }
    let h = Hatch {
        pattern: String::new(),
        solid: false,
        loops: Vec::new(),
        scale: side / (SWATCH_REPEATS * repeat),
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: lines.to_vec(),
    };
    let square = vec![Vec2::new(0.0, 0.0), Vec2::new(side, 0.0), Vec2::new(side, side), Vec2::new(0.0, side)];
    let mut runs = pattern_lines(&Drawing::default(), &h, &[square], 0.0, None);
    runs.truncate(MAX_SWATCH_RUNS);
    runs
}

/// One pattern line family, scaled and placed: parallel lines `spacing` apart through `origin`
/// at angle `ang`, each shifted along itself by `delta`'s along-line part from the last.
struct Family<'a> {
    origin: Vec2,
    ang: f64,
    delta: Vec2,
    spacing: f64,
    dashes: &'a [f64],
}

impl Family<'_> {
    /// Into the frame where the family's lines are horizontal.
    fn to_local(&self, p: Vec2) -> Vec2 {
        (p - self.origin).rotate(-self.ang)
    }
    fn to_world(&self, p: Vec2) -> Vec2 {
        self.origin + p.rotate(self.ang)
    }

    /// Intersect a pattern family with boundary loops, respecting view, output, cancellation, and shared dash-work limits.
    fn sweep(&self, loops: &[Vec<Vec2>], px: f64, view: Option<&Bounds2>, cancel: Option<&AtomicBool>, out: &mut Vec<Vec<Vec2>>, work: &mut u64) {
        let local: Vec<Vec<Vec2>> = loops.iter().map(|l| l.iter().map(|p| self.to_local(*p)).collect()).collect();
        let bb = Bounds2::from_points(local.iter().flatten().copied());
        if bb.is_empty() {
            return;
        }
        // The lines swept: the boundary's extent, cut to the view's (its corners in this frame)
        // when there is one. Spans are cut to the view's extent only: they already lie within the
        // boundary, and cutting them to its extent too would move an end that sits on a boundary
        // corner by a rounding error, so a view covering the hatch would change its lines.
        let (mut lo, mut hi) = (bb.min, bb.max);
        let mut span_cut = None;
        if let Some(v) = view {
            let vb = Bounds2::from_points(v.corners().iter().map(|p| self.to_local(*p)));
            if vb.is_empty() || !vb.intersects(&bb) {
                return;
            }
            lo = Vec2::new(lo.x.max(vb.min.x), lo.y.max(vb.min.y));
            hi = Vec2::new(hi.x.min(vb.max.x), hi.y.min(vb.max.y));
            span_cut = Some((vb.min.x, vb.max.x));
        }
        let shift_per_line = self.delta.rotate(-self.ang).x;
        let k0 = (lo.y / self.spacing).floor() as i64;
        let k1 = (hi.y / self.spacing).ceil() as i64;
        let max_lines = if view.is_some() { MAX_VIEW_FAMILY_LINES } else { MAX_FAMILY_LINES };
        if k1.saturating_sub(k0) > max_lines {
            return;
        }
        let pat_len: f64 = self.dashes.iter().map(|d| d.abs()).sum();
        // Level of detail: a whole repeat under two pixels reads as a continuous line (when the
        // family draws anything at all).
        let draws = self.dashes.iter().any(|d| *d >= 0.0);
        let solid_line = self.dashes.is_empty() || pat_len < 1e-12 || (px > 0.0 && pat_len < 2.0 * px && draws);
        for k in k0..=k1 {
            if out.len() >= MAX_RUNS || *work == 0 || is_set(cancel) {
                return;
            }
            let y = k as f64 * self.spacing;
            let mut xs: Vec<f64> = Vec::new();
            for l in &local {
                let n = l.len();
                for i in 0..n {
                    let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
                    if (a.y > y) != (b.y > y) {
                        xs.push(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            let offset = shift_per_line * k as f64;
            for span in xs.chunks(2) {
                let [x0, x1] = span else { continue };
                let (x0, x1) = match span_cut {
                    Some((a, b)) => (x0.max(a), x1.min(b)),
                    None => (*x0, *x1),
                };
                if span_cut.is_some() && x1 <= x0 {
                    continue;
                }
                if solid_line {
                    out.push(vec![self.to_world(Vec2::new(x0, y)), self.to_world(Vec2::new(x1, y))]);
                } else {
                    self.dash((x0, x1), y, offset, pat_len, out, work);
                }
            }
        }
    }

    /// The dashes and dots of the span `x0..x1` of the line at `y`, phase anchored at x = `offset`.
    fn dash(&self, (x0, x1): (f64, f64), y: f64, offset: f64, pat_len: f64, out: &mut Vec<Vec<Vec2>>, work: &mut u64) {
        let start_rep = ((x0 - offset) / pat_len).floor();
        let mut x = offset + start_rep * pat_len;
        let mut guard = 0;
        'outer: while x < x1 {
            for d in self.dashes {
                guard += 1;
                *work = work.saturating_sub(1);
                if guard > 100_000 || out.len() >= MAX_RUNS || *work == 0 {
                    break 'outer;
                }
                let len = d.abs();
                if *d > 0.0 {
                    let a = x.max(x0);
                    let b = (x + len).min(x1);
                    if b > a {
                        out.push(vec![self.to_world(Vec2::new(a, y)), self.to_world(Vec2::new(b, y))]);
                    }
                } else if *d == 0.0 && x >= x0 && x <= x1 {
                    out.push(vec![self.to_world(Vec2::new(x, y))]);
                }
                x += len;
                if x >= x1 {
                    break 'outer;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Hatch, HatchLoop};
    use cadcraft_geom::PolyVertex;

    #[test]
    fn fill_loops_deduplicates_coincident_loops() {
        let loop1 = HatchLoop {
            vertices: vec![
                PolyVertex::new(Vec2::new(0.0, 0.0)),
                PolyVertex::new(Vec2::new(10.0, 0.0)),
                PolyVertex::new(Vec2::new(10.0, 10.0)),
                PolyVertex::new(Vec2::new(0.0, 10.0)),
            ],
            outer: true,
            sources: vec![],
        };
        let loop2 = loop1.clone();
        let hatch = Hatch {
            origin: Vec2::ZERO,
            pattern: "SOLID".into(),
            solid: true,
            loops: vec![loop1, loop2],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            background: None,
            pattern_lines: vec![],
        };
        let loops = fill_loops(&hatch, 1e-4);
        assert_eq!(loops.len(), 1, "fill_loops must deduplicate duplicate loops");
        let tris = fill::triangulate_evenodd(&loops);
        let area: f64 = tris.chunks(3).map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs()).sum();
        assert!((area - 100.0).abs() < 1e-6, "triangulated area must be 100.0, not zero");
    }
}

#[cfg(test)]
mod dedup_budget_tests {
    use super::*;
    /// Loops with different vertex counts leave enough comparison fuel to remove a later duplicate.
    #[test]
    fn unequal_loops_do_not_spend_the_comparison_budget() {
        // The old estimate exhausted the budget on this unequal pair before seeing the triangle.
        let a = vec![Vec2::ZERO; 5_000];
        let b = vec![Vec2::ZERO; 5_001];
        let tri = vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
        let mut loops = vec![a, b, tri.clone(), tri];
        dedup_polygons(&mut loops);
        assert_eq!(loops.len(), 3);
    }
}
