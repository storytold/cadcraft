//! Planar regions: find the closed boundary around a point from a soup of curves (HATCH pick
//! points, BOUNDARY, REGION).
//!
//! The curves arrive as polylines. Segments are split at every crossing, vertices merged within
//! a tolerance, and the face to the left of the edge hit by a ray from the point is traced by
//! always taking the sharpest left turn.
//!
//! Islands are the connected groups of curves that lie inside that face without touching its
//! boundary; each one contributes the outline of its outside (traced the same way, clockwise
//! around the group), so a square drawn as four separate lines is as much an island as a closed
//! polyline.

use std::collections::HashMap;

use crate::{Bounds2, Line, Vec2, line_line, point_in_polygon, shoelace};

/// Upper bound on input segments (keeps the quadratic split bounded on hostile input).
pub const MAX_SEGMENTS: usize = 60_000;

/// Upper bound on islands reported for one boundary (hostile input with many tiny shapes).
pub const MAX_ISLANDS: usize = 4096;

/// The closed boundary around a point and the islands inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    /// Counter-clockwise outer loop.
    pub outer: Vec<Vec2>,
    /// Groups of curves inside `outer` that don't touch it and don't surround the point.
    pub islands: Vec<Island>,
}

/// One island: the outside outline of a connected group of curves.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// Counter-clockwise outline (dangling ends removed).
    pub outline: Vec<Vec2>,
    /// Indices into the input polylines that make up the group, ascending.
    pub sources: Vec<usize>,
}

fn key(p: Vec2, q: f64) -> (i64, i64) {
    ((p.x / q).round() as i64, (p.y / q).round() as i64)
}

struct Graph {
    pts: Vec<Vec2>,
    adj: Vec<Vec<usize>>,
    /// (graph vertex, input polyline) for every input segment: which curve touches which group.
    src: Vec<(usize, usize)>,
}

fn build(polys: &[Vec<Vec2>], tol: f64) -> Option<Graph> {
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    let mut owner: Vec<usize> = Vec::new();
    for (k, pl) in polys.iter().enumerate() {
        for w in pl.windows(2) {
            if let [a, b] = w
                && a.is_finite()
                && b.is_finite()
                && !a.near(*b, tol)
            {
                segs.push((*a, *b));
                owner.push(k);
            }
        }
        if segs.len() > MAX_SEGMENTS {
            return None;
        }
    }
    if segs.is_empty() || segs.len() > MAX_SEGMENTS {
        return None;
    }
    // Split points per segment (parameters), found with a uniform grid.
    let bb = Bounds2::from_points(segs.iter().flat_map(|(a, b)| [*a, *b]));
    let n = segs.len();
    let cells = ((n as f64).sqrt().ceil() as usize).clamp(1, 512);
    let cw = (bb.width() / cells as f64).max(1e-12);
    let ch = (bb.height() / cells as f64).max(1e-12);
    let cell = |p: Vec2| {
        (
            ((p.x - bb.min.x) / cw).floor().clamp(0.0, (cells - 1) as f64) as usize,
            ((p.y - bb.min.y) / ch).floor().clamp(0.0, (cells - 1) as f64) as usize,
        )
    };
    let mut grid: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        let (x0, y0) = cell(a.min(*b));
        let (x1, y1) = cell(a.max(*b));
        if (x1 - x0 + 1) * (y1 - y0 + 1) > 4096 {
            // Very long segment: register in a coarse band.
            for gx in (x0..=x1).step_by(((x1 - x0) / 64).max(1)) {
                for gy in (y0..=y1).step_by(((y1 - y0) / 64).max(1)) {
                    grid.entry((gx, gy)).or_default().push(i);
                }
            }
            continue;
        }
        for gx in x0..=x1 {
            for gy in y0..=y1 {
                grid.entry((gx, gy)).or_default().push(i);
            }
        }
    }
    let mut splits: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
    let mut tested = std::collections::HashSet::new();
    for list in grid.values() {
        for (k, &i) in list.iter().enumerate() {
            for &j in list.iter().skip(k + 1) {
                if i == j || !tested.insert((i.min(j), i.max(j))) {
                    continue;
                }
                let (Some(si), Some(sj)) = (segs.get(i), segs.get(j)) else { continue };
                let li = Line::new(si.0, si.1);
                let lj = Line::new(sj.0, sj.1);
                if let Some(x) = line_line(&li, &lj) {
                    if let Some(v) = splits.get_mut(i) {
                        v.push(li.param_of(x).clamp(0.0, 1.0));
                    }
                    if let Some(v) = splits.get_mut(j) {
                        v.push(lj.param_of(x).clamp(0.0, 1.0));
                    }
                }
                if tested.len() > 20_000_000 {
                    return None;
                }
            }
        }
    }
    let mut g = Graph { pts: Vec::new(), adj: Vec::new(), src: Vec::with_capacity(n) };
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut vid = |p: Vec2, g: &mut Graph| -> usize {
        let k = key(p, tol);
        *index.entry(k).or_insert_with(|| {
            g.pts.push(p);
            g.adj.push(Vec::new());
            g.pts.len() - 1
        })
    };
    for (i, (a, b)) in segs.iter().enumerate() {
        let mut ts = splits.get(i).cloned().unwrap_or_default();
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        let line = Line::new(*a, *b);
        let first = vid(*a, &mut g);
        if let Some(k) = owner.get(i) {
            g.src.push((first, *k));
        }
        for w in ts.windows(2) {
            if let [t0, t1] = w {
                let u = vid(line.at(*t0), &mut g);
                let v = vid(line.at(*t1), &mut g);
                if u != v {
                    if let Some(l) = g.adj.get_mut(u)
                        && !l.contains(&v)
                    {
                        l.push(v);
                    }
                    if let Some(l) = g.adj.get_mut(v)
                        && !l.contains(&u)
                    {
                        l.push(u);
                    }
                }
            }
        }
    }
    Some(g)
}

/// Trace the face left of half-edge u→v (vertex indices), giving up after `max_steps` edges.
fn trace(g: &Graph, u0: usize, v0: usize, max_steps: usize) -> Option<Vec<usize>> {
    let (mut u, mut v) = (u0, v0);
    let mut out = vec![u];
    for _ in 0..max_steps {
        out.push(v);
        let pv = *g.pts.get(v)?;
        let back = (*g.pts.get(u)? - pv).angle();
        let mut best: Option<(f64, usize)> = None;
        for &w in g.adj.get(v)? {
            if w == u && g.adj.get(v).is_some_and(|l| l.len() > 1) {
                continue;
            }
            let a = (*g.pts.get(w)? - pv).angle();
            let mut cw = crate::norm_angle(back - a);
            if cw < 1e-12 {
                cw = crate::TAU;
            }
            if best.is_none_or(|(b, _)| cw < b) {
                best = Some((cw, w));
            }
        }
        let (_, w) = best?;
        u = v;
        v = w;
        if u == u0 && v == v0 {
            out.pop();
            return Some(out);
        }
    }
    None
}

fn points(g: &Graph, ids: &[usize]) -> Option<Vec<Vec2>> {
    ids.iter().map(|&i| g.pts.get(i).copied()).collect()
}

/// The face around `p`, as vertex indices (counter-clockwise).
fn outer_face(g: &Graph, p: Vec2) -> Option<Vec<usize>> {
    // Try hits along a ray to +X from nearest outward, so nested faces resolve to the innermost.
    let mut hits: Vec<(f64, usize, usize)> = Vec::new();
    for (u, ns) in g.adj.iter().enumerate() {
        for &v in ns {
            if u >= v {
                continue;
            }
            let (Some(a), Some(b)) = (g.pts.get(u), g.pts.get(v)) else { continue };
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y);
                if x > p.x {
                    hits.push((x, u, v));
                }
            }
        }
    }
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    let max_steps = g.pts.len().saturating_mul(2).max(16);
    for (_, a, b) in hits.into_iter().take(64) {
        let (pa, pb) = (*g.pts.get(a)?, *g.pts.get(b)?);
        let (u, v) = if (pb - pa).cross(p - pa) > 0.0 { (a, b) } else { (b, a) };
        if let Some(ids) = trace(g, u, v, max_steps)
            && ids.len() >= 3
            && let Some(lp) = points(g, &ids)
            && shoelace(&lp) > 0.0
            && point_in_polygon(&lp, p)
        {
            return Some(ids);
        }
    }
    None
}

/// Connected component id of every vertex.
fn components(g: &Graph) -> Vec<usize> {
    let mut comp = vec![usize::MAX; g.pts.len()];
    let mut next = 0;
    let mut stack = Vec::new();
    for start in 0..g.pts.len() {
        if comp.get(start).is_none_or(|c| *c != usize::MAX) {
            continue;
        }
        stack.push(start);
        while let Some(u) = stack.pop() {
            match comp.get_mut(u) {
                Some(c) if *c == usize::MAX => *c = next,
                _ => continue,
            }
            stack.extend(g.adj.get(u).into_iter().flatten().copied());
        }
        next += 1;
    }
    comp
}

/// The graph without dangling ends: vertices of degree one are removed until none remain, so
/// only edges on cycles are left.
fn prune(g: &Graph) -> Graph {
    let mut adj = g.adj.clone();
    let mut stack: Vec<usize> = (0..adj.len()).filter(|&u| adj.get(u).is_some_and(|l| l.len() == 1)).collect();
    while let Some(u) = stack.pop() {
        let Some(l) = adj.get_mut(u) else { continue };
        if l.len() != 1 {
            continue;
        }
        let ns = std::mem::take(l);
        for v in ns {
            if let Some(lv) = adj.get_mut(v) {
                lv.retain(|&w| w != u);
                if lv.len() == 1 {
                    stack.push(v);
                }
            }
        }
    }
    Graph { pts: g.pts.clone(), adj, src: Vec::new() }
}

/// Outlines of the groups of curves inside the face `outer` (vertex indices into `g`).
fn islands(g: &Graph, outer: &[usize], p: Vec2) -> Vec<Island> {
    let comp = components(g);
    let outer_comp: std::collections::HashSet<usize> = outer.iter().filter_map(|&i| comp.get(i).copied()).collect();
    let Some(outer_pts) = points(g, outer) else { return Vec::new() };
    let ob = Bounds2::from_points(outer_pts.iter().copied());
    let core = prune(g);
    // Leftmost (then lowest) vertex still on a cycle, per component.
    let mut start: HashMap<usize, usize> = HashMap::new();
    let mut edges: HashMap<usize, usize> = HashMap::new();
    for (u, ns) in core.adj.iter().enumerate() {
        let (Some(&c), Some(q)) = (comp.get(u), core.pts.get(u)) else { continue };
        if ns.is_empty() || outer_comp.contains(&c) {
            continue;
        }
        *edges.entry(c).or_default() += ns.len();
        let e = start.entry(c).or_insert(u);
        if let Some(b) = core.pts.get(*e)
            && (q.x < b.x || (q.x == b.x && q.y < b.y))
        {
            *e = u;
        }
    }
    let mut sources: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(v, k) in &g.src {
        if let Some(&c) = comp.get(v)
            && start.contains_key(&c)
        {
            sources.entry(c).or_default().push(k);
        }
    }
    let mut starts: Vec<(usize, usize)> = start.into_iter().collect();
    starts.sort_unstable();
    let mut out = Vec::new();
    for (c, u) in starts {
        if out.len() >= MAX_ISLANDS {
            break;
        }
        let Some(&pu) = core.pts.get(u) else { continue };
        if !ob.contains(pu) || !point_in_polygon(&outer_pts, pu) {
            continue;
        }
        // Every neighbour lies to the right of the leftmost vertex; the one turned furthest
        // counter-clockwise has the outside of the group on its left.
        let signed = |w: &usize| {
            core.pts.get(*w).map(|q| {
                let a = (*q - pu).angle();
                if a > crate::PI { a - crate::TAU } else { a }
            })
        };
        let Some(w) = core.adj.get(u).into_iter().flatten().filter_map(|w| Some((signed(w)?, *w))).max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, w)| w)
        else {
            continue;
        };
        let steps = edges.get(&c).copied().unwrap_or(0).saturating_add(4);
        let Some(mut lp) = trace(&core, u, w, steps).and_then(|ids| points(&core, &ids)) else { continue };
        lp.reverse();
        let area = shoelace(&lp);
        let ib = Bounds2::from_points(lp.iter().copied());
        if lp.len() < 3 || !area.is_finite() || area <= 0.0 || !ob.contains_box(&ib) || point_in_polygon(&lp, p) {
            continue;
        }
        let mut src = sources.remove(&c).unwrap_or_default();
        src.sort_unstable();
        src.dedup();
        out.push(Island { outline: lp, sources: src });
    }
    out
}

/// The closed loop enclosing `p`, counter-clockwise, or `None`.
pub fn enclosing_loop(polys: &[Vec<Vec2>], p: Vec2, tol: f64) -> Option<Vec<Vec2>> {
    let g = build(polys, tol.max(1e-12))?;
    points(&g, &outer_face(&g, p)?)
}

/// The closed loop enclosing `p` plus the islands inside it, or `None` when `p` isn't enclosed.
pub fn boundary_at(polys: &[Vec<Vec2>], p: Vec2, tol: f64) -> Option<Boundary> {
    let g = build(polys, tol.max(1e-12))?;
    let ids = outer_face(&g, p)?;
    let outer = points(&g, &ids)?;
    let islands = islands(&g, &ids, p);
    Some(Boundary { outer, islands })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, s: f64) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s), Vec2::new(x0, y0)]
    }

    #[test]
    fn finds_square_around_point() {
        let l = enclosing_loop(&[square(0.0, 0.0, 10.0)], Vec2::new(5.0, 5.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn crossing_lines_make_faces() {
        // A square cut by a diagonal: the face is a triangle.
        let polys = vec![square(0.0, 0.0, 10.0), vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)]];
        let l = enclosing_loop(&polys, Vec2::new(7.0, 2.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn overlapping_separate_lines() {
        // Four separate lines that overshoot each other (typical hand drafting).
        let polys = vec![
            vec![Vec2::new(-1.0, 0.0), Vec2::new(11.0, 0.0)],
            vec![Vec2::new(10.0, -1.0), Vec2::new(10.0, 6.0)],
            vec![Vec2::new(11.0, 5.0), Vec2::new(-1.0, 5.0)],
            vec![Vec2::new(0.0, 6.0), Vec2::new(0.0, -1.0)],
        ];
        let l = enclosing_loop(&polys, Vec2::new(3.0, 3.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn open_shape_has_no_boundary() {
        let polys = vec![vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)]];
        assert!(enclosing_loop(&polys, Vec2::new(5.0, 2.0), 1e-9).is_none());
        assert!(enclosing_loop(&[], Vec2::ZERO, 1e-9).is_none());
    }

    #[test]
    fn innermost_of_nested() {
        let polys = vec![square(0.0, 0.0, 10.0), square(2.0, 2.0, 2.0)];
        let l = enclosing_loop(&polys, Vec2::new(3.0, 3.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 4.0).abs() < 1e-9);
        let outer = enclosing_loop(&polys, Vec2::new(8.0, 8.0), 1e-9).unwrap();
        assert!((shoelace(&outer) - 100.0).abs() < 1e-9);
    }

    fn lines_of(poly: &[Vec2]) -> Vec<Vec<Vec2>> {
        poly.windows(2).map(|w| w.to_vec()).collect()
    }

    #[test]
    fn island_from_separate_lines() {
        // Issue #106: a square drawn as four separate lines inside a rectangle is an island.
        let mut polys = vec![square(0.0, 0.0, 20.0)];
        polys.extend(lines_of(&square(5.0, 5.0, 5.0)));
        let b = boundary_at(&polys, Vec2::new(2.0, 2.0), 1e-9).unwrap();
        assert!((shoelace(&b.outer) - 400.0).abs() < 1e-9);
        assert_eq!(b.islands.len(), 1);
        let isl = &b.islands[0];
        assert!((shoelace(&isl.outline) - 25.0).abs() < 1e-9, "counter-clockwise square");
        assert_eq!(isl.outline.len(), 4);
        assert_eq!(isl.sources, vec![1, 2, 3, 4]);
        // The same square as one closed polyline: same island.
        let b2 = boundary_at(&[square(0.0, 0.0, 20.0), square(5.0, 5.0, 5.0)], Vec2::new(2.0, 2.0), 1e-9).unwrap();
        assert_eq!(b2.islands.len(), 1);
        assert_eq!(b2.islands[0].sources, vec![1]);
        // Picking inside the square gives the square, with no islands.
        let inner = boundary_at(&polys, Vec2::new(7.0, 7.0), 1e-9).unwrap();
        assert!((shoelace(&inner.outer) - 25.0).abs() < 1e-9);
        assert!(inner.islands.is_empty());
    }

    #[test]
    fn island_outline_ignores_dangling_ends_and_inner_edges() {
        let mut polys = vec![square(0.0, 0.0, 20.0)];
        polys.extend(lines_of(&square(5.0, 5.0, 5.0)));
        polys.push(vec![Vec2::new(10.0, 10.0), Vec2::new(13.0, 12.0)]); // tail sticking out
        polys.push(vec![Vec2::new(5.0, 5.0), Vec2::new(10.0, 10.0)]); // diagonal inside
        let b = boundary_at(&polys, Vec2::new(2.0, 2.0), 1e-9).unwrap();
        assert_eq!(b.islands.len(), 1);
        assert!((shoelace(&b.islands[0].outline) - 25.0).abs() < 1e-9);
        assert_eq!(b.islands[0].sources, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn open_curves_and_attached_lines_are_not_islands() {
        let polys = vec![
            square(0.0, 0.0, 20.0),
            // An open U inside the region.
            vec![Vec2::new(5.0, 10.0), Vec2::new(5.0, 5.0), Vec2::new(10.0, 5.0), Vec2::new(10.0, 10.0)],
            // A closed shape touching the outer boundary is part of the boundary, not an island.
            vec![Vec2::new(20.0, 15.0), Vec2::new(15.0, 15.0), Vec2::new(15.0, 20.0)],
        ];
        let b = boundary_at(&polys, Vec2::new(2.0, 2.0), 1e-9).unwrap();
        assert!(b.islands.is_empty());
        assert!((shoelace(&b.outer) - 375.0).abs() < 1e-9);
    }

    #[test]
    fn nested_islands_and_outside_curves() {
        let mut polys = vec![square(0.0, 0.0, 20.0)];
        polys.extend(lines_of(&square(4.0, 4.0, 10.0)));
        polys.extend(lines_of(&square(6.0, 6.0, 2.0)));
        polys.extend(lines_of(&square(30.0, 0.0, 5.0))); // outside the region
        let b = boundary_at(&polys, Vec2::new(2.0, 2.0), 1e-9).unwrap();
        let mut areas: Vec<f64> = b.islands.iter().map(|i| shoelace(&i.outline)).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2);
        assert!((areas[0] - 4.0).abs() < 1e-9 && (areas[1] - 100.0).abs() < 1e-9);
        // Between the island and the nested one: the island is the boundary, the nested square
        // its island.
        let mid = boundary_at(&polys, Vec2::new(5.0, 5.0), 1e-9).unwrap();
        assert!((shoelace(&mid.outer) - 100.0).abs() < 1e-9);
        assert_eq!(mid.islands.len(), 1);
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let nan = Vec2::new(f64::NAN, 0.0);
        let inf = Vec2::new(f64::INFINITY, 1.0);
        let mut polys = vec![square(0.0, 0.0, 20.0), vec![nan, inf, Vec2::new(5.0, 5.0)], vec![Vec2::new(3.0, 3.0); 3]];
        polys.extend(lines_of(&square(5.0, 5.0, 5.0)));
        let b = boundary_at(&polys, Vec2::new(2.0, 2.0), 1e-9).unwrap();
        assert_eq!(b.islands.len(), 1);
        assert!(boundary_at(&polys, Vec2::new(f64::NAN, 2.0), 1e-9).is_none());
        // Many islands stay bounded.
        let mut many = vec![square(-1.0, -1.0, 200.0)];
        for i in 0..80 {
            for j in 0..80 {
                many.push(square(f64::from(i) * 2.0, f64::from(j) * 2.0, 1.0));
            }
        }
        let b = boundary_at(&many, Vec2::new(-0.5, -0.5), 1e-9).unwrap();
        assert!(b.islands.len() <= MAX_ISLANDS);
    }
}
