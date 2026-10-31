//! TRIM on a hatch: the cutting edges divide the hatched area, and the part picked is taken out
//! of it. The area left is traced again as boundary loops (arcs in the boundary become short
//! lines), and the hatch stops being associative, as AutoCAD's does.

use cadcraft_doc::{EntityKind, Hatch, HatchLoop};
use cadcraft_geom::{Bounds2, Line, PolyVertex, Segment, Vec2, point_in_polygon, region};

use crate::{EngineError, Result};

/// Most cutting-edge points taken into the area tracing (hostile drawings).
const MAX_CUT_POINTS: usize = 200_000;
/// Most boundary edges handled.
const MAX_EDGES: usize = 200_000;

fn no_cut() -> EngineError {
    EngineError::Other("Object does not intersect a cutting edge.".into())
}

/// Closed polygon edges as (a, b) pairs.
fn edges_of(poly: &[Vec2]) -> Vec<(Vec2, Vec2)> {
    let n = poly.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n).filter_map(|i| Some((*poly.get(i)?, *poly.get((i + 1) % n)?))).filter(|(a, b)| a.dist(*b) > 0.0).collect()
}

/// Split the edge a→b at the points of `pts` lying on it (within `tol`). `pts` is sorted by x:
/// only those within the edge's x range are looked at.
fn split_at(a: Vec2, b: Vec2, pts: &[Vec2], tol: f64, out: &mut Vec<(Vec2, Vec2)>) {
    let ln = Line::new(a, b);
    let len = a.dist(b);
    let (x0, x1) = (a.x.min(b.x) - tol, a.x.max(b.x) + tol);
    let from = pts.partition_point(|p| p.x < x0);
    let to = pts.partition_point(|p| p.x <= x1).max(from);
    let mut ts: Vec<f64> = pts
        .get(from..to)
        .unwrap_or(&[])
        .iter()
        .filter(|p| ln.dist(**p) <= tol)
        .map(|p| ln.param_of(*p))
        .filter(|t| *t * len > tol && (1.0 - *t) * len > tol)
        .collect();
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|x, y| (*x - *y).abs() * len <= tol);
    let mut prev = a;
    for t in ts {
        let q = ln.at(t);
        out.push((prev, q));
        prev = q;
    }
    out.push((prev, b));
}

/// A face's outline without the dangling curves it runs out along and back (A→B→A).
fn despike(mut poly: Vec<Vec2>, tol: f64) -> Vec<Vec2> {
    let mut changed = true;
    while changed && poly.len() >= 3 {
        changed = false;
        let n = poly.len();
        for i in 0..n {
            let (Some(prev), Some(next)) = (poly.get((i + n - 1) % n), poly.get((i + 1) % n)) else { continue };
            if prev.near(*next, tol) {
                // Drop the tip and one of the two equal neighbours.
                let (a, b) = if (i + 1) % n > i { (i, i + 1) } else { (0, i) };
                poly.remove(b);
                poly.remove(a);
                changed = true;
                break;
            }
        }
    }
    poly
}

/// Points by grid cell (`cell` wide), to find the ones near a point without a scan of all.
struct PointGrid {
    cell: f64,
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
}

impl PointGrid {
    fn new(cell: f64) -> Self {
        PointGrid { cell: if cell.is_finite() && cell > 0.0 { cell } else { 1.0 }, cells: std::collections::HashMap::new() }
    }
    fn key(&self, p: Vec2) -> (i64, i64) {
        // Saturating float-to-int casts: a far or non-finite point lands in an edge cell.
        ((p.x / self.cell).floor() as i64, (p.y / self.cell).floor() as i64)
    }
    fn insert(&mut self, p: Vec2, i: usize) {
        let k = self.key(p);
        self.cells.entry(k).or_default().push(i);
    }
    /// The indices stored in the cells around `p` (its own and the eight next to it).
    fn near(&self, p: Vec2) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = self.key(p);
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (x.saturating_add(dx), y.saturating_add(dy))))
            .filter_map(|k| self.cells.get(&k))
            .flatten()
            .copied()
    }
}

/// Most edge entries over all bands of an [`EdgeRows`].
const MAX_BAND_ENTRIES: usize = 4_000_000;

/// Edges by horizontal band, for even-odd inside tests and nearness in about the square root
/// of the edge count each, not a scan of every edge (the hover preview runs these per move).
struct EdgeRows {
    edges: Vec<(Vec2, Vec2)>,
    rows: Vec<Vec<usize>>,
    y0: f64,
    dy: f64,
}

impl EdgeRows {
    fn new(edges: Vec<(Vec2, Vec2)>) -> Self {
        let (lo, hi) =
            edges.iter().flat_map(|(a, b)| [a.y, b.y]).filter(|y| y.is_finite()).fold((f64::MAX, f64::MIN), |(l, h), y| (l.min(y), h.max(y)));
        let mut n = ((edges.len() as f64).sqrt() as usize).clamp(1, 4096);
        let (mut y0, mut dy) = if lo < hi { (lo, (hi - lo) / n as f64) } else { (lo.min(0.0), 1.0) };
        // Edges as tall as the area are in every band: past this many entries one band holds
        // them all (a plain scan per test, as without bands), so memory stays bounded.
        let probe = EdgeRows { edges: Vec::new(), rows: vec![Vec::new(); n], y0, dy };
        let entries: usize = edges.iter().map(|(a, b)| probe.row(a.y.max(b.y)) - probe.row(a.y.min(b.y)) + 1).fold(0, usize::saturating_add);
        if entries > MAX_BAND_ENTRIES {
            n = 1;
            (y0, dy) = (0.0, 1.0);
        }
        let mut r = EdgeRows { edges: Vec::new(), rows: vec![Vec::new(); n], y0, dy };
        for (i, (a, b)) in edges.iter().enumerate() {
            let (r0, r1) = (r.row(a.y.min(b.y)), r.row(a.y.max(b.y)));
            for row in r.rows.get_mut(r0..=r1).into_iter().flatten() {
                row.push(i);
            }
        }
        r.edges = edges;
        r
    }
    fn row(&self, y: f64) -> usize {
        let k = ((y - self.y0) / self.dy).floor();
        if k.is_nan() || k < 0.0 { 0 } else { (k as usize).min(self.rows.len().saturating_sub(1)) }
    }
    /// Inside by even-odd crossings (as `point_in_polygon` over every loop together).
    fn contains(&self, p: Vec2) -> bool {
        let mut inside = false;
        for &i in self.rows.get(self.row(p.y)).into_iter().flatten() {
            // In `point_in_polygon`'s order (from an edge's end back to its start), so points on
            // the boundary come out the same.
            let Some(&(b, a)) = self.edges.get(i) else { continue };
            if (a.y > p.y) != (b.y > p.y) {
                let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }
    /// Whether an edge passes within `d` of `p`.
    fn near(&self, p: Vec2, d: f64) -> bool {
        let (r0, r1) = (self.row(p.y - d), self.row(p.y + d));
        self.rows.get(r0..=r1).into_iter().flatten().flatten().any(|&i| self.edges.get(i).is_some_and(|(a, b)| Line::new(*a, *b).dist(p) <= d))
    }
}

/// Chain directed edges into closed loops (start of one = end of the last).
fn chain(edges: Vec<(Vec2, Vec2)>, tol: f64) -> Vec<Vec<Vec2>> {
    let mut starts = PointGrid::new(tol);
    for (i, (a, _)) in edges.iter().enumerate() {
        starts.insert(*a, i);
    }
    let mut used = vec![false; edges.len()];
    let mut loops = Vec::new();
    for first in (0..edges.len()).rev() {
        let (Some(&(a, b)), Some(false)) = (edges.get(first), used.get(first).copied()) else { continue };
        if let Some(u) = used.get_mut(first) {
            *u = true;
        }
        let mut pts = vec![a];
        let mut end = b;
        let mut guard = 0usize;
        while !end.near(a, tol) && guard < MAX_EDGES {
            guard += 1;
            let next = starts
                .near(end)
                .filter(|&i| !used.get(i).copied().unwrap_or(true))
                .filter(|&i| edges.get(i).is_some_and(|(p, _)| p.near(end, tol)))
                .min();
            let Some(i) = next else { break };
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            let Some(&(p, q)) = edges.get(i) else { break };
            pts.push(p);
            end = q;
        }
        if end.near(a, tol) && pts.len() >= 3 {
            loops.push(pts);
        }
    }
    loops
}

/// Trim the hatch at `pick` against the cutting edges `cut`: the hatch that is left (`None`: none
/// of its area is) and the outline of the part taken away (its outer loop, then its islands).
pub(crate) fn trim_hatch(h: &Hatch, cut: &[Segment], pick: Vec2) -> Result<(Option<EntityKind>, Vec<Vec<Vec2>>)> {
    // The exact extents, arcs included: a circle kept as two half-circle arcs is as tall as it is
    // wide, not the flat sliver its chords make.
    let bounds = Bounds2::from_points(
        h.loops
            .iter()
            .map(|l| cadcraft_geom::Polyline { vertices: l.vertices.clone(), closed: true }.bounds())
            .flat_map(|b| [b.min, b.max])
            .filter(|p| p.is_finite()),
    );
    let size = bounds.width().hypot(bounds.height());
    if !size.is_finite() || size <= 0.0 {
        return Err(EngineError::Other("Cannot trim that object.".into()));
    }
    let tess = size * 2e-4;
    let tol = size * 1e-9;
    let mut loops = cadcraft_render::hatch_fill_loops(h, tess);
    for l in &mut loops {
        if l.len() > 1 && l.first().zip(l.last()).is_some_and(|(f, z)| f.near(*z, tol)) {
            l.pop();
        }
    }
    let hatch_rows = EdgeRows::new(loops.iter().flat_map(|l| edges_of(l)).collect());
    let in_hatch = |p: Vec2| hatch_rows.contains(p);
    // A pick on the hatch's edge (the hatch is picked along its boundary too) trims the part
    // just inside it.
    let pick = if in_hatch(pick) {
        pick
    } else {
        [1e-3, 3e-3, 1e-2, 3e-2]
            .iter()
            .flat_map(|r| (0..16).map(move |k| Vec2::polar(pick, size * r, std::f64::consts::TAU * f64::from(k) / 16.0)))
            .find(|q| in_hatch(*q))
            .ok_or_else(|| EngineError::Other("Pick the hatch inside its area.".into()))?
    };
    // The cutting edges near the hatch, as polylines.
    let near = bounds.expand(size * 0.01);
    let mut cuts: Vec<Vec<Vec2>> = Vec::new();
    let mut n = 0usize;
    for c in cut {
        let mut pts = Vec::new();
        c.tessellate(tess, &mut pts);
        let cb = Bounds2::from_points(pts.iter().copied());
        if !cb.intersects(&near) {
            continue;
        }
        n += pts.len();
        if n > MAX_CUT_POINTS {
            return Err(EngineError::Other("Too many cutting edges near the hatch.".into()));
        }
        cuts.push(pts);
    }
    let mut polys: Vec<Vec<Vec2>> = loops
        .iter()
        .map(|l| {
            let mut c = l.clone();
            c.extend(l.first().copied());
            c
        })
        .collect();
    polys.extend(cuts);
    let b = region::boundary_at(&polys, pick, tol.max(size * 1e-7))
        .ok_or_else(|| EngineError::Other("Could not work out the part of the hatch to trim.".into()))?;
    let snap = size * 1e-7;
    let face: Vec<Vec<Vec2>> = std::iter::once(b.outer.clone())
        .chain(b.islands.iter().map(|i| i.outline.clone()))
        .map(|l| despike(l, snap))
        .filter(|l| l.len() >= 3)
        .collect();
    if face.is_empty() {
        return Err(no_cut());
    }
    // Inside the face: its outer loop and not an island (even-odd over its loops, the islands
    // lying inside the outer loop and apart).
    let face_rows = EdgeRows::new(face.iter().flat_map(|l| edges_of(l)).collect());
    let in_face = |p: Vec2| face_rows.contains(p);
    // A face made of the hatch's own boundary only: no cutting edge crosses this area.
    let on_hatch = |a: Vec2, z: Vec2| hatch_rows.near((a + z) * 0.5, snap * 10.0);
    if face.iter().all(|f| edges_of(f).iter().all(|(a, z)| on_hatch(*a, *z))) {
        return Err(no_cut());
    }
    // Every candidate edge: the hatch's loops split where the face's corners touch them, and the
    // face's own edges.
    let mut corners: Vec<Vec2> = face.iter().flatten().copied().filter(|p| p.is_finite()).collect();
    corners.sort_by(|p, q| p.x.total_cmp(&q.x));
    let mut cand: Vec<(Vec2, Vec2)> = Vec::new();
    for l in &loops {
        for (a, z) in edges_of(l) {
            split_at(a, z, &corners, snap, &mut cand);
            if cand.len() > MAX_EDGES {
                return Err(EngineError::Other("The hatch is too complex to trim.".into()));
            }
        }
    }
    cand.extend(face.iter().flat_map(|f| edges_of(f)));
    // Keep the edges with the area left on exactly one side, turned to have it on the left.
    let off = size * 1e-6;
    let left_over = |p: Vec2| in_hatch(p) && !in_face(p);
    let mut kept: Vec<(Vec2, Vec2)> = Vec::new();
    let mut kept_at = PointGrid::new(snap);
    for (a, z) in cand {
        let d = (z - a).normalized();
        let m = (a + z) * 0.5;
        let nrm = Vec2::new(-d.y, d.x) * off;
        let (l, r) = (left_over(m + nrm), left_over(m - nrm));
        if l == r {
            continue;
        }
        let e = if l { (a, z) } else { (z, a) };
        // The same piece of boundary twice (a loop edge that is also a face edge).
        if kept_at.near(e.0).any(|i| kept.get(i).is_some_and(|(p, q)| p.near(e.0, snap) && q.near(e.1, snap))) {
            continue;
        }
        kept_at.insert(e.0, kept.len());
        kept.push(e);
    }
    let outlines = chain(kept, snap * 10.0);
    if outlines.is_empty() {
        return Ok((None, face));
    }
    let depth = |i: usize, l: &[Vec2]| {
        let Some(p) = l.first().zip(l.get(1)).map(|(a, z)| (*a + *z) * 0.5) else { return 0 };
        outlines.iter().enumerate().filter(|(j, o)| *j != i && point_in_polygon(o, p)).count()
    };
    let new_loops: Vec<HatchLoop> = outlines
        .iter()
        .enumerate()
        .map(|(i, l)| HatchLoop { vertices: l.iter().map(|p| PolyVertex::new(*p)).collect(), outer: depth(i, l) == 0, sources: Vec::new() })
        .collect();
    let out = Hatch { loops: new_loops, associative: false, style: 0, ..h.clone() };
    Ok((Some(EntityKind::Hatch(out)), face))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(lo: f64, hi: f64) -> HatchLoop {
        let vertices = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)].iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect();
        HatchLoop { vertices, outer: true, sources: Vec::new() }
    }

    fn hatch(loops: Vec<HatchLoop>) -> Hatch {
        Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops,
            scale: 1.0,
            angle: 0.0,
            associative: true,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        }
    }

    fn area(k: &EntityKind) -> f64 {
        let EntityKind::Hatch(h) = k else { return f64::NAN };
        let ls = cadcraft_render::hatch_fill_loops(h, 1e-3);
        // Odd-parity area by sampling.
        let mut n = 0;
        for i in 0..200 {
            for j in 0..200 {
                let p = Vec2::new(-1.0 + i as f64 * 0.06 + 0.03, -1.0 + j as f64 * 0.06 + 0.03);
                if ls.iter().filter(|l| point_in_polygon(l, p)).count() % 2 == 1 {
                    n += 1;
                }
            }
        }
        f64::from(n) * 0.06 * 0.06
    }

    #[test]
    fn a_line_across_a_hatch_trims_the_picked_half() {
        let h = hatch(vec![square(0.0, 10.0)]);
        let cut = [Segment::Line(Line::new(Vec2::new(4.0, -5.0), Vec2::new(4.0, 15.0)))];
        let (left, face) = trim_hatch(&h, &cut, Vec2::new(2.0, 5.0)).unwrap();
        let left = left.unwrap();
        assert!((area(&left) - 60.0).abs() < 1.0, "{}", area(&left));
        let EntityKind::Hatch(lh) = &left else { panic!() };
        assert!(!lh.associative);
        assert!(face.first().is_some_and(|f| f.iter().all(|p| p.x <= 4.0 + 1e-6)));
    }

    /// A round hatch kept as two half-circle arcs (bulge 1): a line across its top is found as a
    /// cutting edge and only the cap above it is taken away (#2034 review).
    #[test]
    fn a_line_across_a_circular_hatch_trims_the_cap() {
        let circle = HatchLoop {
            vertices: vec![
                PolyVertex { p: Vec2::new(10.0, 0.0), bulge: 1.0, ..PolyVertex::new(Vec2::ZERO) },
                PolyVertex { p: Vec2::new(-10.0, 0.0), bulge: 1.0, ..PolyVertex::new(Vec2::ZERO) },
            ],
            outer: true,
            sources: Vec::new(),
        };
        let h = hatch(vec![circle]);
        let cut = [Segment::Line(Line::new(Vec2::new(-20.0, 5.0), Vec2::new(20.0, 5.0)))];
        let (left, _) = trim_hatch(&h, &cut, Vec2::new(0.0, 8.0)).unwrap();
        let Some(EntityKind::Hatch(left)) = left else { panic!("the hatch below the line is left") };
        let pts: Vec<Vec2> = cadcraft_render::hatch_fill_loops(&left, 1e-2).into_iter().flatten().collect();
        let top = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        let bottom = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert!((top - 5.0).abs() < 1e-3, "the cap above y=5 is gone: top {top}");
        assert!((bottom + 10.0).abs() < 0.05, "the rest stays: bottom {bottom}");
    }

    #[test]
    fn a_hole_cut_out_of_a_hatch() {
        let h = hatch(vec![square(0.0, 10.0)]);
        // A closed square of cutting edges inside the hatch: picking inside it takes it out.
        let c = [(3.0, 3.0), (6.0, 3.0), (6.0, 6.0), (3.0, 6.0), (3.0, 3.0)];
        let cut: Vec<Segment> = c.windows(2).map(|w| Segment::Line(Line::new(Vec2::new(w[0].0, w[0].1), Vec2::new(w[1].0, w[1].1)))).collect();
        let (left, _) = trim_hatch(&h, &cut, Vec2::new(4.5, 4.5)).unwrap();
        assert!((area(&left.unwrap()) - 91.0).abs() < 1.0);
    }

    #[test]
    fn no_cutting_edge_no_trim() {
        let h = hatch(vec![square(0.0, 10.0)]);
        let cut = [Segment::Line(Line::new(Vec2::new(20.0, 0.0), Vec2::new(20.0, 10.0)))];
        assert!(trim_hatch(&h, &cut, Vec2::new(5.0, 5.0)).is_err());
    }

    #[test]
    fn hostile_hatches_do_not_panic() {
        let h = hatch(vec![HatchLoop { vertices: vec![PolyVertex::new(Vec2::new(f64::NAN, 0.0))], outer: true, sources: Vec::new() }]);
        let _ = trim_hatch(&h, &[], Vec2::ZERO);
        let h = hatch(Vec::new());
        let _ = trim_hatch(&h, &[], Vec2::ZERO);
    }

    /// A hatch with a many-sided boundary trims in about linear time (the hover preview runs
    /// this on every move; #2329 review).
    #[test]
    fn a_detailed_hatch_trims_quickly() {
        let n = 20_000;
        let c = Vec2::new(5.0, 5.0);
        let vertices: Vec<PolyVertex> =
            (0..n).map(|i| PolyVertex::new(c + Vec2::from_angle(std::f64::consts::TAU * f64::from(i) / f64::from(n)) * 5.0)).collect();
        let h = hatch(vec![HatchLoop { vertices, outer: true, sources: Vec::new() }]);
        let cut = [Segment::Line(Line::new(Vec2::new(7.0, -5.0), Vec2::new(7.0, 15.0)))];
        let t = std::time::Instant::now();
        let (left, _) = trim_hatch(&h, &cut, Vec2::new(9.0, 5.0)).unwrap();
        let took = t.elapsed();
        let a = area(&left.unwrap());
        // The circle (78.54) less the cap right of x = 7 (19.82).
        assert!((a - 58.72).abs() < 1.0, "{a}");
        assert!(took.as_secs_f64() < 1.0, "took {took:?}");
    }

    /// The banded inside test agrees with `point_in_polygon` (two loops, one inside the
    /// other, points on band edges and vertices included), with and without bands.
    #[test]
    fn banded_inside_test_matches_the_plain_one() {
        let star: Vec<Vec2> = (0..40).map(|i| Vec2::from_angle(f64::from(i) * 0.157) * if i % 2 == 0 { 10.0 } else { 4.0 }).collect();
        let inner: Vec<Vec2> = (0..12).map(|i| Vec2::from_angle(f64::from(i) * 0.5) * 2.0).collect();
        let edges: Vec<(Vec2, Vec2)> = edges_of(&star).into_iter().chain(edges_of(&inner)).collect();
        let rows = EdgeRows::new(edges);
        assert!(rows.rows.len() > 1);
        for i in -60..=60 {
            for j in -60..=60 {
                let p = Vec2::new(f64::from(i) * 0.19, f64::from(j) * 0.19);
                let plain = point_in_polygon(&star, p) != point_in_polygon(&inner, p);
                assert_eq!(rows.contains(p), plain, "{p:?}");
            }
        }
        for v in star.iter().chain(&inner) {
            assert_eq!(rows.contains(*v), point_in_polygon(&star, *v) != point_in_polygon(&inner, *v), "{v:?}");
        }
    }

    #[test]
    fn walls_along_the_hatch_boundary_do_not_stop_the_trim() {
        // The hatch fills a room whose walls run along its boundary and past its corners; a
        // line crosses the room.
        let h = hatch(vec![square(0.0, 10.0)]);
        let l = |a: (f64, f64), b: (f64, f64)| Segment::Line(Line::new(Vec2::new(a.0, a.1), Vec2::new(b.0, b.1)));
        let cut = [
            l((-5.0, 0.0), (15.0, 0.0)),
            l((10.0, -5.0), (10.0, 15.0)),
            l((15.0, 10.0), (-5.0, 10.0)),
            l((0.0, 15.0), (0.0, -5.0)),
            // A wall piece covering part of an edge only.
            l((2.0, 0.0), (6.0, 0.0)),
            l((4.0, -5.0), (4.0, 15.0)),
        ];
        let (left, _) = trim_hatch(&h, &cut, Vec2::new(2.0, 5.0)).unwrap();
        assert!((area(&left.unwrap()) - 60.0).abs() < 1.0);
    }

    #[test]
    fn a_pick_on_the_hatch_edge_trims_the_part_inside() {
        let h = hatch(vec![square(0.0, 10.0)]);
        let cut = [Segment::Line(Line::new(Vec2::new(4.0, -5.0), Vec2::new(4.0, 15.0)))];
        // On the left edge of the hatch.
        let (left, _) = trim_hatch(&h, &cut, Vec2::new(0.0, 5.0)).unwrap();
        assert!((area(&left.unwrap()) - 60.0).abs() < 1.0);
    }
}
