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

/// Split the edge a→b at the points of `pts` lying on it (within `tol`).
fn split_at(a: Vec2, b: Vec2, pts: &[Vec2], tol: f64, out: &mut Vec<(Vec2, Vec2)>) {
    let ln = Line::new(a, b);
    let len = a.dist(b);
    let mut ts: Vec<f64> =
        pts.iter().filter(|p| ln.dist(**p) <= tol).map(|p| ln.param_of(*p)).filter(|t| *t * len > tol && (1.0 - *t) * len > tol).collect();
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

/// Chain directed edges into closed loops (start of one = end of the last).
fn chain(mut edges: Vec<(Vec2, Vec2)>, tol: f64) -> Vec<Vec<Vec2>> {
    let mut loops = Vec::new();
    while let Some((a, b)) = edges.pop() {
        let mut pts = vec![a];
        let mut end = b;
        let mut guard = 0usize;
        while !end.near(a, tol) && guard < MAX_EDGES {
            guard += 1;
            let Some(i) = edges.iter().position(|(p, _)| p.near(end, tol)) else { break };
            let (p, q) = edges.swap_remove(i);
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
    let raw: Vec<Vec<Vec2>> =
        h.loops.iter().map(|l| cadcraft_geom::Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(1e300)).collect();
    let bounds = Bounds2::from_points(raw.iter().flatten().copied());
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
    let in_hatch = |p: Vec2| loops.iter().filter(|l| point_in_polygon(l, p)).count() % 2 == 1;
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
    let Some(outer) = face.first().cloned() else { return Err(no_cut()) };
    let in_face = |p: Vec2| point_in_polygon(&outer, p) && !face.iter().skip(1).any(|i| point_in_polygon(i, p));
    // A face made of the hatch's own boundary only: no cutting edge crosses this area.
    let on_hatch = |a: Vec2, z: Vec2| {
        let m = (a + z) * 0.5;
        loops.iter().any(|l| edges_of(l).iter().any(|(p, q)| Line::new(*p, *q).dist(m) <= snap * 10.0))
    };
    if face.iter().all(|f| edges_of(f).iter().all(|(a, z)| on_hatch(*a, *z))) {
        return Err(no_cut());
    }
    // Every candidate edge: the hatch's loops split where the face's corners touch them, and the
    // face's own edges.
    let corners: Vec<Vec2> = face.iter().flatten().copied().collect();
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
        if kept.iter().any(|(p, q)| p.near(e.0, snap) && q.near(e.1, snap)) {
            continue;
        }
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
