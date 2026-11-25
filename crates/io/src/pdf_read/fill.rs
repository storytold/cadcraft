//! Preserve the PDF fill rule when PDF paths become CAD hatch boundaries.
//!
//! Hatch rendering uses even-odd parity. A nonzero PDF fill whose contours neither cross nor touch
//! keeps the contours where the winding number changes between zero and non-zero, so its loops
//! and bulge arcs survive import. Crossing contours are resolved into disjoint triangles by
//! Lyon's tessellator.

use cadcraft_doc::HatchLoop;
use cadcraft_geom::{PolyVertex, Vec2};
use craft_pdf::interpret::FillRule;
use lyon_tessellation::geometry_builder::{FillGeometryBuilder, GeometryBuilder, GeometryBuilderError};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{FillOptions, FillTessellator, FillVertex, VertexId};

/// Work charged to input sampling and containment checks.
const MAX_WORK: usize = 20_000_000;
/// Bounding-box tests between contours before a fill is refused.
const MAX_BOX_CHECKS: usize = 50_000_000;
/// Edge pairs compared when looking for crossings; past this a nonzero fill is tessellated.
const MAX_EDGE_PAIRS: usize = 20_000_000;
/// Sampled points above which a nonzero fill is tessellated without looking for crossings.
const MAX_TOPOLOGY_POINTS: usize = 1_000_000;
/// Lyon uses `f32`; work in a local frame and refuse a fill wider than this many tolerances.
const MAX_SPAN_OVER_TOLERANCE: f64 = 5.0e7;
/// Most points one bulge arc is sampled into.
const MAX_ARC_STEPS: usize = 10_000;

/// Convert PDF boundaries to hatch boundaries that keep the PDF's fill rule.
///
/// `max_vertices` covers sampled input points and output triangle vertices. `tolerance` is how far,
/// in drawing units, a sampled arc may leave the real one. Even-odd loops and nonzero loops that
/// don't cross retain their original curves. Crossing nonzero contours are tessellated into
/// disjoint triangles, each represented by one hatch loop.
pub(super) fn normalize(mut loops: Vec<HatchLoop>, rule: FillRule, max_vertices: usize, tolerance: f64) -> Result<Vec<HatchLoop>, &'static str> {
    if loops.is_empty() {
        return Ok(loops);
    }
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("PDF fill tolerance is not usable");
    }
    let mut work = 0usize;
    let mut sampled = 0usize;
    let mut polygons = Vec::new();
    for loop_ in &loops {
        let polygon = flatten_loop(loop_, tolerance, &mut work, max_vertices.saturating_sub(sampled))?;
        sampled = sampled.checked_add(polygon.len()).ok_or("PDF fill exceeds vertex limit")?;
        polygons.push(polygon);
    }

    match rule {
        FillRule::EvenOdd => {
            // Contours that don't meet are placed by a point of their own boundary: a point nudged
            // inside an edge can land in a contour drawn close to it.
            let apart = sampled <= MAX_TOPOLOGY_POINTS && edges_meet(&polygons) == Some(false);
            let probes: Vec<Vec2> = polygons.iter().map(|p| if apart { boundary_point(p) } else { interior_probe(p) }).collect();
            let mut depth = vec![0usize; loops.len()];
            containment(&polygons, &probes, &mut work, |i, _| depth[i] += 1)?;
            for (loop_, depth) in loops.iter_mut().zip(depth) {
                loop_.outer = depth.is_multiple_of(2);
            }
            Ok(loops)
        }
        FillRule::NonZero if sampled > MAX_TOPOLOGY_POINTS || edges_meet(&polygons).unwrap_or(true) => {
            tessellate_nonzero(&polygons, sampled, max_vertices, tolerance)
        }
        FillRule::NonZero => {
            let sign: Vec<i64> = polygons
                .iter()
                .map(|p| {
                    let area = signed_area(p);
                    if p.len() < 3 || area == 0.0 { 0 } else { area.signum() as i64 }
                })
                .collect();
            // No two contours meet, so a contour lies inside another exactly when a point of its
            // own boundary does.
            let probes: Vec<Vec2> = polygons.iter().map(|p| boundary_point(p)).collect();
            // The winding number just inside each contour.
            let mut winding = sign.clone();
            containment(&polygons, &probes, &mut work, |i, j| winding[i] += sign[j])?;
            // A contour bounds the fill where the winding number changes between zero and non-zero.
            let keep: Vec<bool> = winding.iter().zip(&sign).map(|(&w, &s)| s != 0 && (w != 0) != (w - s != 0)).collect();
            let mut depth = vec![0usize; loops.len()];
            containment(&polygons, &probes, &mut work, |i, j| {
                if keep[i] && keep[j] {
                    depth[i] += 1;
                }
            })?;
            Ok(loops
                .into_iter()
                .enumerate()
                .filter(|(i, _)| keep[*i])
                .map(|(i, mut loop_)| {
                    loop_.outer = depth[i].is_multiple_of(2);
                    loop_
                })
                .collect())
        }
    }
}

fn flatten_loop(loop_: &HatchLoop, tolerance: f64, work: &mut usize, max_vertices: usize) -> Result<Vec<Vec2>, &'static str> {
    let n = loop_.vertices.len();
    // Two vertices enclose an area only through an arc.
    if n < 2 || (n == 2 && loop_.vertices.iter().all(|v| v.bulge.abs() < 1e-12)) {
        return Ok(Vec::new());
    }
    let mut points = Vec::new();
    for i in 0..n {
        let vertex = loop_.vertices[i];
        if !vertex.p.is_finite() || !vertex.bulge.is_finite() {
            return Err("PDF fill contains invalid coordinates");
        }
        push_bounded(&mut points, vertex.p, max_vertices)?;
        let next = loop_.vertices[(i + 1) % n].p;
        if vertex.bulge.abs() < 1e-12 || vertex.p == next {
            continue;
        }
        // A polyline bulge is tan(sweep/4). Sampling is used only for containment and lyon's
        // topology. Even-odd output keeps the original circular arc vertices.
        let dx = next.x - vertex.p.x;
        let dy = next.y - vertex.p.y;
        let bulge = vertex.bulge;
        let center = Vec2::new(
            (vertex.p.x + next.x) * 0.5 - dy * (1.0 - bulge * bulge) / (4.0 * bulge),
            (vertex.p.y + next.y) * 0.5 + dx * (1.0 - bulge * bulge) / (4.0 * bulge),
        );
        let start = (vertex.p.y - center.y).atan2(vertex.p.x - center.x);
        let sweep = 4.0 * bulge.atan();
        let radius = vertex.p.dist(center);
        if !radius.is_finite() {
            return Err("PDF fill contains invalid coordinates");
        }
        let max_angle = if radius <= tolerance { std::f64::consts::TAU } else { 2.0 * (1.0 - tolerance / radius).clamp(-1.0, 1.0).acos() };
        let steps = (sweep.abs() / max_angle.max(1e-12)).ceil().max(1.0) as usize;
        if steps > MAX_ARC_STEPS {
            return Err("PDF fill arc exceeds sampling limit");
        }
        for step in 1..steps {
            let angle = start + sweep * step as f64 / steps as f64;
            let point = Vec2::new(center.x + radius * angle.cos(), center.y + radius * angle.sin());
            if !point.is_finite() {
                return Err("PDF fill contains invalid coordinates");
            }
            push_bounded(&mut points, point, max_vertices)?;
        }
    }
    *work = work.checked_add(points.len()).ok_or("PDF fill exceeds work limit")?;
    if *work > MAX_WORK {
        return Err("PDF fill exceeds work limit");
    }
    Ok(points)
}

fn push_bounded(points: &mut Vec<Vec2>, point: Vec2, max_vertices: usize) -> Result<(), &'static str> {
    if points.len() >= max_vertices {
        return Err("PDF fill exceeds vertex limit");
    }
    points.push(point);
    Ok(())
}

/// Call `found(i, j)` for each contour `j` that contains `probes[i]`, the point standing for contour `i`.
fn containment(polygons: &[Vec<Vec2>], probes: &[Vec2], work: &mut usize, mut found: impl FnMut(usize, usize)) -> Result<(), &'static str> {
    let boxes: Vec<(Vec2, Vec2)> = polygons.iter().map(|p| bounds(p)).collect();
    let mut checks = 0usize;
    for (i, polygon) in polygons.iter().enumerate() {
        if polygon.len() < 3 {
            continue;
        }
        let probe = probes[i];
        for (j, other) in polygons.iter().enumerate() {
            if i == j || other.len() < 3 {
                continue;
            }
            checks += 1;
            if checks > MAX_BOX_CHECKS {
                return Err("PDF fill exceeds work limit");
            }
            let (lo, hi) = boxes[j];
            if probe.x < lo.x || probe.x > hi.x || probe.y < lo.y || probe.y > hi.y {
                continue;
            }
            *work = work.checked_add(other.len()).ok_or("PDF fill exceeds work limit")?;
            if *work > MAX_WORK {
                return Err("PDF fill exceeds work limit");
            }
            if contains(other, probe) {
                found(i, j);
            }
        }
    }
    Ok(())
}

fn bounds(points: &[Vec2]) -> (Vec2, Vec2) {
    let first = points.first().copied().unwrap_or(Vec2::ZERO);
    points.iter().fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))))
}

/// Whether two contours cross or touch, or one crosses, touches or folds back on itself. `None`
/// when the search would compare more than [`MAX_EDGE_PAIRS`] edge pairs.
fn edges_meet(polygons: &[Vec<Vec2>]) -> Option<bool> {
    let rings: Vec<Vec<Vec2>> = polygons
        .iter()
        .map(|p| {
            let mut ring: Vec<Vec2> = Vec::with_capacity(p.len());
            for &q in p {
                if ring.last() != Some(&q) {
                    ring.push(q);
                }
            }
            while ring.len() > 1 && ring.first() == ring.last() {
                ring.pop();
            }
            ring
        })
        .collect();
    let edge = |r: usize, k: usize| (rings[r][k], rings[r][(k + 1) % rings[r].len()]);
    // Each edge by its smallest x, so only edges whose x ranges overlap are compared.
    let mut edges: Vec<(f64, usize, usize)> = Vec::new();
    for (r, ring) in rings.iter().enumerate() {
        if ring.len() < 3 {
            continue;
        }
        for k in 0..ring.len() {
            let (a, b) = edge(r, k);
            edges.push((a.x.min(b.x), r, k));
        }
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pairs = 0usize;
    for (n, &(_, r, k)) in edges.iter().enumerate() {
        let (a, b) = edge(r, k);
        let right = a.x.max(b.x);
        for &(left, s, m) in &edges[n + 1..] {
            if left > right {
                break;
            }
            pairs += 1;
            if pairs > MAX_EDGE_PAIRS {
                return None;
            }
            let (c, d) = edge(s, m);
            if a.y.max(b.y) < c.y.min(d.y) || c.y.max(d.y) < a.y.min(b.y) {
                continue;
            }
            if r == s {
                let len = rings[r].len();
                // Neighbouring edges share a vertex; they only meet elsewhere by folding back.
                if (k + 1) % len == m {
                    if folds_back(a, b, d) {
                        return Some(true);
                    }
                    continue;
                }
                if (m + 1) % len == k {
                    if folds_back(c, d, b) {
                        return Some(true);
                    }
                    continue;
                }
            }
            if segments_meet(a, b, c, d) {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// Whether the path a to b to c turns straight back over itself at b.
fn folds_back(a: Vec2, b: Vec2, c: Vec2) -> bool {
    (b - a).cross(c - b) == 0.0 && (b.x - a.x) * (c.x - b.x) + (b.y - a.y) * (c.y - b.y) < 0.0
}

/// Whether the closed segments a to b and c to d share a point.
fn segments_meet(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let within = |p: Vec2, q: Vec2, r: Vec2| r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && within(c, d, a)) || (d2 == 0.0 && within(c, d, b)) || (d3 == 0.0 && within(a, b, c)) || (d4 == 0.0 && within(a, b, d))
}

/// Find a point slightly inside the longest edge. The scan across edges handles concave loops
/// without relying on their vertex average being inside.
fn interior_probe(polygon: &[Vec2]) -> Vec2 {
    let area = signed_area(polygon);
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let Some((a, b)) = longest_edge(polygon) else { return Vec2::ZERO };
    let edge = b - a;
    let len = edge.len().max(1e-30);
    let mut epsilon = (len * 1e-6).max(1e-12);
    for _ in 0..32 {
        let probe = Vec2::new((a.x + b.x) * 0.5 - sign * edge.y / len * epsilon, (a.y + b.y) * 0.5 + sign * edge.x / len * epsilon);
        if contains(polygon, probe) {
            return probe;
        }
        epsilon *= 0.5;
    }
    Vec2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

/// The middle of the longest edge.
fn boundary_point(polygon: &[Vec2]) -> Vec2 {
    longest_edge(polygon).map_or(Vec2::ZERO, |(a, b)| (a + b) * 0.5)
}

fn longest_edge(polygon: &[Vec2]) -> Option<(Vec2, Vec2)> {
    polygon
        .iter()
        .copied()
        .zip(polygon.iter().copied().cycle().skip(1))
        .take(polygon.len())
        .max_by(|(a, b), (c, d)| a.dist(*b).total_cmp(&c.dist(*d)))
}

fn signed_area(polygon: &[Vec2]) -> f64 {
    // Relative to the first vertex, so the products far from the origin don't round the area away.
    let Some(&o) = polygon.first() else { return 0.0 };
    polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()).map(|(a, b)| (*a - o).cross(*b - o)).sum::<f64>() * 0.5
}

fn contains(polygon: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
        if (a.y > point.y) != (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

fn tessellate_nonzero(polygons: &[Vec<Vec2>], sampled_vertices: usize, max_vertices: usize, tolerance: f64) -> Result<Vec<HatchLoop>, &'static str> {
    let Some(origin) = polygons.iter().flatten().next().copied() else { return Ok(Vec::new()) };
    let (min, max) = polygons
        .iter()
        .flatten()
        .fold((origin, origin), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))));
    let span = tolerance * MAX_SPAN_OVER_TOLERANCE;
    if max.x - min.x > span || max.y - min.y > span {
        return Err("PDF fill is too large for stable vector conversion");
    }
    let mut path = Path::builder();
    for polygon in polygons {
        let Some(first) = polygon.first() else { continue };
        path.begin(point((first.x - origin.x) as f32, (first.y - origin.y) as f32));
        for p in polygon.iter().skip(1) {
            path.line_to(point((p.x - origin.x) as f32, (p.y - origin.y) as f32));
        }
        path.end(true);
    }
    let mut output = CappedGeometry::new(max_vertices.saturating_sub(sampled_vertices));
    FillTessellator::new().tessellate_path(&path.build(), &FillOptions::non_zero(), &mut output).map_err(|_| "PDF fill tessellation failed")?;
    if output.exceeded || output.indices.len().checked_add(sampled_vertices).is_none_or(|n| n > max_vertices) {
        return Err("PDF fill exceeds vertex limit");
    }
    if !output.indices.len().is_multiple_of(3) {
        return Err("PDF fill tessellation returned invalid triangles");
    }
    output
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let vertices = triangle
                .iter()
                .map(|&index| {
                    output
                        .vertices
                        .get(index as usize)
                        .copied()
                        .map(|p| PolyVertex::new(Vec2::new(p.x + origin.x, p.y + origin.y)))
                        .ok_or("PDF fill tessellation returned an invalid index")
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(HatchLoop { vertices, outer: true, boundary_flags: None, sources: Vec::new() })
        })
        .collect()
}

/// Lyon's triangle callback cannot report a capacity error. This builder caps both buffers and
/// records rejected triangles so extraction remains bounded even for pathological intersections.
struct CappedGeometry {
    vertices: Vec<Vec2>,
    indices: Vec<u32>,
    limit: usize,
    exceeded: bool,
}

impl CappedGeometry {
    fn new(limit: usize) -> Self {
        Self { vertices: Vec::new(), indices: Vec::new(), limit, exceeded: false }
    }
}

impl GeometryBuilder for CappedGeometry {
    fn add_triangle(&mut self, a: VertexId, b: VertexId, c: VertexId) {
        if self.indices.len().saturating_add(3) > self.limit {
            self.exceeded = true;
            return;
        }
        self.indices.extend([a.0, b.0, c.0]);
    }
}

impl FillGeometryBuilder for CappedGeometry {
    fn add_fill_vertex(&mut self, vertex: FillVertex<'_>) -> Result<VertexId, GeometryBuilderError> {
        if self.vertices.len() >= self.limit {
            self.exceeded = true;
            return Err(GeometryBuilderError::TooManyVertices);
        }
        let id = u32::try_from(self.vertices.len()).map_err(|_| GeometryBuilderError::TooManyVertices)?;
        let p = vertex.position();
        self.vertices.push(Vec2::new(f64::from(p.x), f64::from(p.y)));
        Ok(VertexId(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polygon(points: &[(f64, f64)]) -> HatchLoop {
        HatchLoop {
            vertices: points.iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect(),
            outer: false,
            boundary_flags: None,
            sources: Vec::new(),
        }
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64, clockwise: bool) -> HatchLoop {
        let points = if clockwise { vec![(x0, y0), (x0, y1), (x1, y1), (x1, y0)] } else { vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)] };
        polygon(&points)
    }

    /// The arc tolerance of a drawing in millimetres.
    const TOL: f64 = 0.02;

    fn filled(loops: &[HatchLoop], point: Vec2) -> bool {
        loops.iter().filter(|l| contains(&l.vertices.iter().map(|v| v.p).collect::<Vec<_>>(), point)).count() % 2 == 1
    }

    /// A circle about the origin as four quarter arcs.
    fn circle(r: f64, clockwise: bool) -> HatchLoop {
        let quarter = (std::f64::consts::PI / 8.0).tan();
        let turn = if clockwise { -1.0 } else { 1.0 };
        HatchLoop {
            vertices: [(r, 0.0), (0.0, r * turn), (-r, 0.0), (0.0, -r * turn)]
                .iter()
                .map(|&(x, y)| PolyVertex { bulge: quarter * turn, ..PolyVertex::new(Vec2::new(x, y)) })
                .collect(),
            outer: false,
            boundary_flags: None,
            sources: Vec::new(),
        }
    }

    #[test]
    fn a_small_circle_keeps_its_curve_when_tessellated_in_a_large_unit() {
        // A circle of radius 5 mm in a drawing in metres, crossed by a bar so the fill is
        // tessellated. Its arcs are sampled to the given tolerance, not cut to their four vertices.
        let r = 0.005;
        let metres = 0.02 * 25.4 / 72.0 / 1000.0;
        let bar = square(0.5 * r, -0.2 * r, 2.0 * r, 0.2 * r, false);
        let result = normalize(vec![circle(r, false), bar], FillRule::NonZero, 100_000, metres).expect("valid contours");
        assert!(result.len() > 3, "tessellated into triangles");
        assert!(filled(&result, Vec2::new(0.65 * r, 0.65 * r)), "the arc between two vertices stays filled");
    }

    #[test]
    fn a_disc_of_two_half_circles_is_kept() {
        let half = |x: f64| PolyVertex { bulge: 1.0, ..PolyVertex::new(Vec2::new(x, 0.0)) };
        let disc = HatchLoop { vertices: vec![half(-1.0), half(1.0)], outer: false, boundary_flags: None, sources: Vec::new() };
        for rule in [FillRule::EvenOdd, FillRule::NonZero] {
            let result = normalize(vec![disc.clone()], rule, 10_000, TOL).expect("valid contour");
            assert_eq!(result.len(), 1);
            assert!(result[0].outer);
            assert_eq!(result[0].vertices, disc.vertices, "the arcs are kept");
        }
    }

    #[test]
    fn nonzero_contour_hugging_its_parents_edge_keeps_the_fill() {
        // The inner square runs 1e-7 from the outer one's left edge, closer than a point nudged
        // inside that edge: each contour is placed by a point of its own boundary.
        let input = vec![square(0.0, 0.0, 10.0, 10.0, false), square(1e-7, 4.0, 1.0, 6.0, false)];
        let result = normalize(input, FillRule::NonZero, 1000, TOL).expect("valid contours");
        assert!(filled(&result, Vec2::new(5.0, 5.0)) && filled(&result, Vec2::new(0.5, 5.0)), "{result:?}");
        assert!(result.iter().all(|l| l.vertices.len() == 4), "kept as drawn, not split into triangles");
    }

    #[test]
    fn even_odd_disconnected_contours_are_both_outer() {
        let input = vec![square(0.0, 0.0, 2.0, 2.0, false), square(4.0, 0.0, 6.0, 2.0, false)];
        let result = normalize(input, FillRule::EvenOdd, 100, TOL).expect("valid contours");
        assert!(result.iter().all(|loop_| loop_.outer));
        assert!(filled(&result, Vec2::new(1.0, 1.0)));
        assert!(filled(&result, Vec2::new(5.0, 1.0)));
    }

    #[test]
    fn nonzero_nested_same_direction_contours_keep_the_center_filled() {
        let result = normalize(vec![square(0.0, 0.0, 10.0, 10.0, false), square(2.0, 2.0, 8.0, 8.0, false)], FillRule::NonZero, 1000, TOL)
            .expect("valid contours");
        assert!(filled(&result, Vec2::new(5.0, 5.0)));
        assert!(filled(&result, Vec2::new(1.0, 1.0)));
        assert!(result.iter().all(|l| l.vertices.len() == 4), "the outer square is kept, not split into triangles");
    }

    #[test]
    fn nonzero_nested_opposite_direction_contours_leave_a_hole() {
        let result = normalize(vec![square(0.0, 0.0, 10.0, 10.0, false), square(2.0, 2.0, 8.0, 8.0, true)], FillRule::NonZero, 1000, TOL)
            .expect("valid contours");
        assert!(!filled(&result, Vec2::new(5.0, 5.0)));
        assert!(filled(&result, Vec2::new(1.0, 1.0)));
        assert!(result.iter().all(|l| l.vertices.len() == 4), "both squares are kept as drawn");
        assert!(result.iter().any(|l| l.outer) && result.iter().any(|l| !l.outer));
    }

    #[test]
    fn nonzero_concave_contour_stays_one_loop() {
        let points = [(0.0, 0.0), (10.0, 0.0), (10.0, 3.0), (3.0, 3.0), (3.0, 10.0), (0.0, 10.0)];
        let result = normalize(vec![polygon(&points)], FillRule::NonZero, 1000, TOL).expect("valid contour");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].vertices, polygon(&points).vertices);
        assert!(result[0].outer);
    }

    #[test]
    fn nonzero_curved_nested_contours_keep_their_arcs() {
        // Circles of radius 10 and 5, wound opposite ways.
        let result = normalize(vec![circle(10.0, false), circle(5.0, true)], FillRule::NonZero, 10_000, TOL).expect("valid contours");
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|l| l.vertices.iter().all(|v| v.bulge != 0.0)), "the arcs are kept");
    }

    #[test]
    fn nonzero_overlapping_contours_keep_the_overlap_filled() {
        let result = normalize(vec![square(0.0, 0.0, 6.0, 6.0, false), square(3.0, 0.0, 9.0, 6.0, false)], FillRule::NonZero, 1000, TOL)
            .expect("valid contours");
        assert!(filled(&result, Vec2::new(4.0, 3.0)));
        assert!(filled(&result, Vec2::new(1.0, 3.0)));
        assert!(filled(&result, Vec2::new(8.0, 3.0)));
    }

    #[test]
    fn nonzero_self_crossing_contour_is_split_into_triangles() {
        let points = [(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (6.0, 0.0), (3.0, 8.0)];
        let result = normalize(vec![polygon(&points)], FillRule::NonZero, 1000, TOL).expect("valid contour");
        assert!(result.len() > 1);
        assert!(filled(&result, Vec2::new(4.0, 5.0)));
    }

    #[test]
    fn nonzero_tessellation_keeps_coordinates_away_from_the_local_origin() {
        let shift = 50_000.0;
        // The third square crosses the first, so the fill is tessellated.
        let input = vec![
            square(shift, shift, shift + 10.0, shift + 10.0, false),
            square(shift + 2.0, shift + 2.0, shift + 8.0, shift + 8.0, true),
            square(shift + 9.0, shift + 4.0, shift + 12.0, shift + 6.0, false),
        ];
        let result = normalize(input, FillRule::NonZero, 1000, TOL).expect("valid translated contours");
        assert!(result.len() > 3, "tessellated into triangles");
        assert!(!filled(&result, Vec2::new(shift + 5.0, shift + 5.0)));
        assert!(filled(&result, Vec2::new(shift + 1.0, shift + 1.0)));
        assert!(filled(&result, Vec2::new(shift + 11.0, shift + 5.1)));
    }

    #[test]
    fn a_unit_square_far_from_the_origin_keeps_its_area() {
        // At 2^29 the products of the coordinates are too large to keep a unit of area.
        for shift in [0.0, 2f64.powi(29)] {
            let result = normalize(vec![square(shift, shift, shift + 1.0, shift + 1.0, false)], FillRule::NonZero, 100, TOL).expect("valid contour");
            assert_eq!(result.len(), 1, "at {shift}");
        }
    }

    #[test]
    fn even_odd_hole_close_to_its_outer_edge_is_classified_by_depth() {
        // The hole runs 1e-6 below the outer square's top edge, closer than a point nudged inside
        // that edge.
        let input = vec![square(0.0, 0.0, 10.0, 8.0, false), square(4.0, 7.0, 6.0, 7.999_999, false)];
        let result = normalize(input, FillRule::EvenOdd, 1000, TOL).expect("valid contours");
        assert!(result[0].outer && !result[1].outer, "{result:?}");
    }

    #[test]
    fn even_odd_nested_contours_are_classified_by_depth() {
        let result = normalize(vec![square(0.0, 0.0, 10.0, 10.0, false), square(2.0, 2.0, 8.0, 8.0, false)], FillRule::EvenOdd, 1000, TOL)
            .expect("valid contours");
        assert!(result[0].outer);
        assert!(!result[1].outer);
        assert!(!filled(&result, Vec2::new(5.0, 5.0)));
        assert!(filled(&result, Vec2::new(1.0, 1.0)));
    }
}
