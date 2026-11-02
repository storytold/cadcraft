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

/// Upper bound on open curve ends considered for closing gaps, and on the pairs compared.
const MAX_GAP_ENDS: usize = 50_000;
const MAX_GAP_PAIRS: usize = 5_000_000;

/// The closed boundary around a point and the islands inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    /// Counter-clockwise outer loop.
    pub outer: Vec<Vec2>,
    /// Groups of curves inside `outer` that don't touch it and don't surround the point.
    pub islands: Vec<Island>,
    /// Indices into the input polylines that make up `outer`, ascending.
    pub outer_sources: Vec<usize>,
}

/// One island: the outside outline of a connected group of curves.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// Counter-clockwise outline (dangling ends removed).
    pub outline: Vec<Vec2>,
    /// Indices into the input polylines that make up the group, ascending.
    pub sources: Vec<usize>,
}

fn key(p: Vec2, origin: Vec2, q: f64) -> (i64, i64) {
    // The drawing may be georeferenced far from (0, 0). Quantizing absolute
    // coordinates saturates i64 and aliases distinct vertices into one key.
    // Shift to the bounds origin before converting to tolerance-sized bins.
    (((p.x - origin.x) / q).round() as i64, ((p.y - origin.y) / q).round() as i64)
}

struct Graph {
    pts: Vec<Vec2>,
    adj: Vec<Vec<usize>>,
    /// (graph vertex, input polyline) for every input segment: which curve touches which group.
    src: Vec<(usize, usize)>,
    /// The input polyline of every edge (lower vertex index first).
    edge_src: HashMap<(usize, usize), usize>,
}

/// Records the crossing of segments `i` and `j`, if any, as split parameters on both.
fn split_pair(segs: &[(Vec2, Vec2)], splits: &mut [Vec<f64>], i: usize, j: usize) {
    let (Some(si), Some(sj)) = (segs.get(i), segs.get(j)) else { return };
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
    let mut long: Vec<usize> = Vec::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        let (x0, y0) = cell(a.min(*b));
        let (x1, y1) = cell(a.max(*b));
        if (x1 - x0 + 1) * (y1 - y0 + 1) > 4096 {
            // Very long segment: too many cells to register in; it is tested against every segment below.
            long.push(i);
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
    // Count every candidate-pair *visit*, including repeats of a pair that shares several grid
    // cells, not only the distinct pairs actually tested. A long segment replicated across many
    // cells makes the same pair reappear in each, so a budget on distinct pairs alone leaves the
    // repeated hashing unbounded (CWE-407).
    let mut visits: u64 = 0;
    const MAX_PAIR_VISITS: u64 = 60_000_000;
    for list in grid.values() {
        for (k, &i) in list.iter().enumerate() {
            for &j in list.iter().skip(k + 1) {
                visits += 1;
                if visits > MAX_PAIR_VISITS {
                    return None;
                }
                if i == j || !tested.insert((i.min(j), i.max(j))) {
                    continue;
                }
                split_pair(&segs, &mut splits, i, j);
                if tested.len() > 20_000_000 {
                    return None;
                }
            }
        }
    }
    for &i in &long {
        for j in 0..n {
            if i == j || !tested.insert((i.min(j), i.max(j))) {
                continue;
            }
            split_pair(&segs, &mut splits, i, j);
            if tested.len() > 20_000_000 {
                return None;
            }
        }
    }
    let mut g = Graph { pts: Vec::new(), adj: Vec::new(), src: Vec::with_capacity(n), edge_src: HashMap::new() };
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut vid = |p: Vec2, g: &mut Graph| -> usize {
        let k = key(p, bb.min, tol);
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
                    if let Some(k) = owner.get(i) {
                        g.edge_src.entry((u.min(v), u.max(v))).or_insert(*k);
                    }
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

/// The graph of the curves with gaps of up to `gap` between open ends bridged: each open end
/// joins the nearest other open end within reach.
fn build_gap(polys: &[Vec<Vec2>], tol: f64, gap: f64) -> Option<Graph> {
    let mut g = build(polys, tol.max(1e-12))?;
    if !(gap.is_finite() && gap > tol) {
        return Some(g);
    }
    let mut ends: Vec<(Vec2, usize)> =
        g.adj.iter().enumerate().filter(|(_, ns)| ns.len() == 1).filter_map(|(u, _)| Some((*g.pts.get(u)?, u))).take(MAX_GAP_ENDS).collect();
    ends.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    let mut joined = vec![false; ends.len()];
    let mut budget = MAX_GAP_PAIRS;
    for i in 0..ends.len() {
        let (Some(&(pu, u)), Some(false)) = (ends.get(i), joined.get(i).copied()) else { continue };
        let lo = ends.partition_point(|e| e.0.x < pu.x - gap);
        let mut best: Option<(f64, usize)> = None;
        for (j, &(pv, v)) in ends.iter().enumerate().skip(lo) {
            if pv.x > pu.x + gap || budget == 0 {
                break;
            }
            budget -= 1;
            if j == i || joined.get(j).copied().unwrap_or(true) || g.adj.get(u).is_some_and(|l| l.contains(&v)) {
                continue;
            }
            let d = pu.dist(pv);
            if d <= gap && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, j));
            }
        }
        let Some((_, j)) = best else { continue };
        let Some(&(_, v)) = ends.get(j) else { continue };
        let owner = g.adj.get(u).and_then(|l| l.first()).and_then(|w| g.edge_src.get(&(u.min(*w), u.max(*w))).copied());
        if let Some(l) = g.adj.get_mut(u) {
            l.push(v);
        }
        if let Some(l) = g.adj.get_mut(v) {
            l.push(u);
        }
        if let Some(k) = owner {
            g.edge_src.insert((u.min(v), u.max(v)), k);
        }
        if let Some(f) = joined.get_mut(i) {
            *f = true;
        }
        if let Some(f) = joined.get_mut(j) {
            *f = true;
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
    Graph { pts: g.pts.clone(), adj, src: Vec::new(), edge_src: HashMap::new() }
}

/// The outline of the outside of a group of curves on cycles, traced counter-clockwise from its
/// leftmost (then lowest) vertex `u`, giving up after `steps` edges.
fn outside(core: &Graph, u: usize, steps: usize) -> Option<Vec<Vec2>> {
    let pu = *core.pts.get(u)?;
    // Every neighbour lies to the right of the leftmost vertex; the one turned furthest
    // counter-clockwise has the outside of the group on its left.
    let signed = |w: &usize| {
        core.pts.get(*w).map(|q| {
            let a = (*q - pu).angle();
            if a > crate::PI { a - crate::TAU } else { a }
        })
    };
    let w = core.adj.get(u).into_iter().flatten().filter_map(|w| Some((signed(w)?, *w))).max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, w)| w)?;
    let mut lp = trace(core, u, w, steps).and_then(|ids| points(core, &ids))?;
    lp.reverse();
    Some(lp)
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
        let steps = edges.get(&c).copied().unwrap_or(0).saturating_add(4);
        let Some(lp) = outside(&core, u, steps) else { continue };
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

/// The point of the nearest other curve within `snap` of the end `e` of curve `i`, as (curve,
/// segment, parameter, point). `None` when the end touches a curve, or none is near. `Err` when
/// the budget of segment checks runs out.
fn nearest_curve(polys: &[Vec<Vec2>], i: usize, e: Vec2, snap: f64, tol: f64, budget: &mut usize) -> Result<Option<(usize, usize, f64, Vec2)>, ()> {
    let mut best: Option<(f64, usize, usize, f64, Vec2)> = None;
    let mut touching = false;
    for (j, other) in polys.iter().enumerate() {
        if j == i {
            continue;
        }
        for (k, w) in other.windows(2).enumerate() {
            if *budget == 0 {
                return Err(());
            }
            *budget -= 1;
            let [a, b] = w else { continue };
            if !(a.is_finite() && b.is_finite())
                || e.x < a.x.min(b.x) - snap
                || e.x > a.x.max(b.x) + snap
                || e.y < a.y.min(b.y) - snap
                || e.y > a.y.max(b.y) + snap
            {
                continue;
            }
            let line = Line::new(*a, *b);
            let t = line.param_of(e).clamp(0.0, 1.0);
            let q = line.at(t);
            let d = q.dist(e);
            if d <= tol {
                touching = true;
            } else if d <= snap && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, j, k, t, q));
            }
        }
    }
    Ok(best.filter(|_| !touching).map(|(_, j, k, t, q)| (j, k, t, q)))
}

/// The curves with each open end that stops within `snap` of another curve moved onto that
/// curve (which gains a vertex there), so the two meet exactly. A curve that is flattened
/// into chords (an ellipse, a spline) lies up to the flattening tolerance off the true curve,
/// so a line that ends on the true curve can stop just short of the chords.
pub fn snap_ends(polys: &[Vec<Vec2>], snap: f64, tol: f64) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = polys.to_vec();
    if !(snap.is_finite() && snap > tol) {
        return out;
    }
    // build() refuses this input anyway; don't spend ends x segments work on it.
    let total: usize = polys.iter().map(|p| p.len().saturating_sub(1)).sum();
    if total > MAX_SEGMENTS {
        return out;
    }
    // (target polyline, segment index, parameter on it, the point) for every vertex to insert.
    let mut inserts: Vec<(usize, usize, f64, Vec2)> = Vec::new();
    let mut budget = MAX_GAP_PAIRS;
    'ends: for (i, pl) in polys.iter().enumerate() {
        let (Some(&first), Some(&last)) = (pl.first(), pl.last()) else { continue };
        if pl.len() < 2 || first.near(last, tol) {
            continue;
        }
        for at_end in [false, true] {
            let e = if at_end { last } else { first };
            if !e.is_finite() {
                continue;
            }
            let Ok(near) = nearest_curve(polys, i, e, snap, tol, &mut budget) else { break 'ends };
            let Some((j, k, t, q)) = near else { continue };
            let Some(slot) = out.get_mut(i).and_then(|v| if at_end { v.last_mut() } else { v.first_mut() }) else { continue };
            *slot = q;
            let ends = polys.get(j).and_then(|o| o.get(k).zip(o.get(k + 1)));
            if let Some((&a, &b)) = ends
                && !q.near(a, tol)
                && !q.near(b, tol)
            {
                inserts.push((j, k, t, q));
            }
        }
    }
    // Later segments and larger parameters first, so earlier indices stay valid.
    inserts.sort_by(|x, y| y.0.cmp(&x.0).then(y.1.cmp(&x.1)).then(y.2.total_cmp(&x.2)));
    for (j, k, _, q) in inserts {
        if let Some(v) = out.get_mut(j)
            && k < v.len()
        {
            v.insert(k + 1, q);
        }
    }
    out
}

/// The closed loop enclosing `p`, counter-clockwise, or `None`.
pub fn enclosing_loop(polys: &[Vec<Vec2>], p: Vec2, tol: f64) -> Option<Vec<Vec2>> {
    enclosing_loop_gap(polys, p, tol, 0.0)
}

/// [`enclosing_loop`], with gaps of up to `gap` between curve ends closed (HPGAPTOL).
pub fn enclosing_loop_gap(polys: &[Vec<Vec2>], p: Vec2, tol: f64, gap: f64) -> Option<Vec<Vec2>> {
    let g = build_gap(polys, tol, gap)?;
    points(&g, &outer_face(&g, p)?)
}

/// The closed loop enclosing `p` plus the islands inside it, or `None` when `p` isn't enclosed.
pub fn boundary_at(polys: &[Vec<Vec2>], p: Vec2, tol: f64) -> Option<Boundary> {
    boundary_at_gap(polys, p, tol, 0.0)
}

/// [`boundary_at`], with gaps of up to `gap` between curve ends closed (HPGAPTOL).
pub fn boundary_at_gap(polys: &[Vec<Vec2>], p: Vec2, tol: f64, gap: f64) -> Option<Boundary> {
    let g = build_gap(polys, tol, gap)?;
    let ids = outer_face(&g, p)?;
    let outer = points(&g, &ids)?;
    let islands = islands(&g, &ids, p);
    let mut outer_sources: Vec<usize> =
        ids.iter().zip(ids.iter().cycle().skip(1)).filter_map(|(&u, &v)| g.edge_src.get(&(u.min(v), u.max(v))).copied()).collect();
    outer_sources.sort_unstable();
    outer_sources.dedup();
    Some(Boundary { outer, islands, outer_sources })
}

/// The outside outline of curves that together enclose an area (counter-clockwise, dangling
/// ends removed), or `None` when they enclose nothing or form more than one separate group.
/// Gaps of up to `gap` between curve ends are closed (HPGAPTOL).
pub fn outline(polys: &[Vec<Vec2>], tol: f64, gap: f64) -> Option<Vec<Vec2>> {
    let g = build_gap(polys, tol, gap)?;
    let core = prune(&g);
    let comp = components(&core);
    let mut start: Option<usize> = None;
    let mut group: Option<usize> = None;
    let mut edges = 0usize;
    for (u, ns) in core.adj.iter().enumerate() {
        if ns.is_empty() {
            continue;
        }
        let c = comp.get(u).copied()?;
        if group.is_some_and(|g| g != c) {
            return None;
        }
        group = Some(c);
        edges = edges.saturating_add(ns.len());
        let q = core.pts.get(u)?;
        if start.and_then(|s| core.pts.get(s)).is_none_or(|b| q.x < b.x || (q.x == b.x && q.y < b.y)) {
            start = Some(u);
        }
    }
    let lp = outside(&core, start?, edges.saturating_add(4))?;
    (lp.len() >= 3 && shoelace(&lp) > 0.0).then_some(lp)
}

/// A point strictly inside a closed polygon (either orientation), for re-finding the face a
/// loop encloses: the middle of the widest span of a horizontal line through it.
pub fn interior_point(poly: &[Vec2]) -> Option<Vec2> {
    let b = Bounds2::from_points(poly.iter().copied().filter(|p| p.is_finite()));
    if poly.len() < 3 || b.is_empty() || b.height() <= 0.0 {
        return None;
    }
    let mut best: Option<(f64, Vec2)> = None;
    for f in [0.5, 0.381_966, 0.618_034, 0.25, 0.75] {
        let y = b.min.y + b.height() * f;
        let mut xs: Vec<f64> = Vec::new();
        for (a, c) in poly.iter().zip(poly.iter().cycle().skip(1)) {
            if (a.y > y) != (c.y > y) {
                xs.push(a.x + (c.x - a.x) * (y - a.y) / (c.y - a.y));
            }
        }
        xs.sort_by(f64::total_cmp);
        for [x0, x1] in xs.as_chunks::<2>().0 {
            let width = x1 - x0;
            if width.is_finite() && best.is_none_or(|(bw, _)| width > bw) {
                best = Some((width, Vec2::new((x0 + x1) / 2.0, y)));
            }
        }
    }
    best.filter(|(w, _)| *w > 0.0).map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, s: f64) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s), Vec2::new(x0, y0)]
    }

    #[test]
    fn long_segment_crossing_in_unsampled_cell_is_split() {
        // A diagonal long enough to skip the grid, and a short segment crossing it in a cell the old coarse sampling missed.
        let mut polys = vec![vec![Vec2::new(0.0, 0.0), Vec2::new(1000.0, 1000.0)], vec![Vec2::new(11.0, 12.0), Vec2::new(12.0, 11.0)]];
        for i in 0..17_000usize {
            let x = 600.0 + (i % 130) as f64 * 2.0;
            let y = 100.0 + (i / 130) as f64 * 2.0;
            polys.push(vec![Vec2::new(x, y), Vec2::new(x + 1.0, y)]);
        }
        let g = build(&polys, 1e-6).unwrap();
        let hit = g.pts.iter().position(|p| p.near(Vec2::new(11.5, 11.5), 1e-6)).unwrap();
        assert_eq!(g.adj[hit].len(), 4);
    }

    #[test]
    fn finds_square_around_point() {
        let l = enclosing_loop(&[square(0.0, 0.0, 10.0)], Vec2::new(5.0, 5.0), 1e-9).unwrap();
        assert!((shoelace(&l) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn boundaries_at_large_world_coordinates_keep_distinct_vertices() {
        // Absolute-position / tolerance is beyond i64::MAX for this normal
        // georeferenced drawing; the old key collapsed every square corner.
        let origin = 1e12;
        let outline = square(origin, origin, 100.0);
        let picked = Vec2::new(origin + 50.0, origin + 50.0);
        let found = enclosing_loop(&[outline], picked, 1e-9).expect("large-coordinate boundary");
        assert_eq!(found.len(), 4);
        assert!((shoelace(&found) - 10_000.0).abs() < 1e-5);

        // Island detection uses the same graph and must preserve its loop.
        let result = boundary_at(&[square(origin, origin, 100.0)], picked, 1e-9).expect("boundary with islands");
        assert_eq!(result.outer.len(), 4);
        assert!(result.islands.is_empty());
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

    #[test]
    fn outer_sources_outline_and_interior_point() {
        // A square cut by a diagonal: the triangle picked is made of the square and the diagonal;
        // a far-away line takes no part.
        let polys = vec![square(0.0, 0.0, 10.0), vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)], vec![Vec2::new(30.0, 0.0), Vec2::new(40.0, 0.0)]];
        let b = boundary_at(&polys, Vec2::new(7.0, 2.0), 1e-9).unwrap();
        assert_eq!(b.outer_sources, vec![0, 1]);
        // Four separate lines: one outline; an open U and two separate squares have none.
        let l = outline(&lines_of(&square(5.0, 5.0, 5.0)), 1e-9, 0.0).unwrap();
        assert!((shoelace(&l) - 25.0).abs() < 1e-9);
        assert!(outline(&[vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)]], 1e-9, 0.0).is_none());
        assert!(outline(&[square(0.0, 0.0, 1.0), square(5.0, 5.0, 1.0)], 1e-9, 0.0).is_none());
        // An L shape: the point is inside, also for a clockwise loop.
        let l_shape =
            vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 10.0), Vec2::new(0.0, 10.0)];
        let p = interior_point(&l_shape).unwrap();
        assert!(point_in_polygon(&l_shape, p));
        let rev: Vec<Vec2> = l_shape.iter().rev().copied().collect();
        assert!(point_in_polygon(&rev, interior_point(&rev).unwrap()));
        assert!(interior_point(&[Vec2::ZERO, Vec2::new(1.0, 0.0)]).is_none());
        assert!(interior_point(&[Vec2::new(f64::NAN, 0.0); 4]).is_none());
    }

    #[test]
    fn gap_tolerance_closes_small_gaps() {
        // Four lines with 0.5 gaps at two corners.
        let polys = vec![
            vec![Vec2::new(0.0, 0.0), Vec2::new(9.5, 0.0)],
            vec![Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)],
            vec![Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)],
            vec![Vec2::new(0.0, 10.0), Vec2::new(0.0, 0.5)],
        ];
        let p = Vec2::new(5.0, 5.0);
        assert!(enclosing_loop(&polys, p, 1e-9).is_none());
        assert!(enclosing_loop_gap(&polys, p, 1e-9, 0.25).is_none(), "gap larger than the tolerance");
        let l = enclosing_loop_gap(&polys, p, 1e-9, 0.6).unwrap();
        assert!((shoelace(&l) - 100.0).abs() < 1e-9, "{}", shoelace(&l));
        let b = boundary_at_gap(&polys, p, 1e-9, 0.6).unwrap();
        assert_eq!(b.outer_sources, vec![0, 1, 2, 3]);
        assert!(outline(&polys, 1e-9, 0.6).is_some());
        // Hostile gaps.
        assert!(enclosing_loop_gap(&polys, p, 1e-9, f64::NAN).is_none());
        assert!(enclosing_loop_gap(&polys, p, 1e-9, f64::INFINITY).is_none());
    }

    #[test]
    fn line_ending_just_short_of_a_chord_is_joined_to_it() {
        // A strip between two verticals and a flattened arc that bows below the ends of the
        // verticals by 0.05 (the verticals stop on the true curve, the chord is lower).
        let top = vec![Vec2::new(0.0, 10.0), Vec2::new(2.0, 10.0)];
        let left = vec![Vec2::new(0.0, 10.0), Vec2::new(0.0, 0.05)];
        let right = vec![Vec2::new(2.0, 10.0), Vec2::new(2.0, 0.05)];
        let chord = vec![Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)];
        let polys = vec![top, left, right, chord];
        let p = Vec2::new(1.0, 5.0);
        assert!(enclosing_loop(&polys, p, 1e-9).is_none());
        let snapped = snap_ends(&polys, 0.1, 1e-9);
        let l = enclosing_loop(&snapped, p, 1e-9).unwrap();
        assert!((shoelace(&l) - 20.0).abs() < 1e-6, "{}", shoelace(&l));
        // Too far away: left alone.
        assert_eq!(snap_ends(&polys, 0.01, 1e-9), polys);
    }

    #[test]
    fn snap_ends_skips_input_over_the_segment_cap() {
        let mut polys: Vec<Vec<Vec2>> = (0..=MAX_SEGMENTS)
            .map(|i| {
                let x = i as f64 * 10.0;
                vec![Vec2::new(x, 0.0), Vec2::new(x, 1.0)]
            })
            .collect();
        polys.push(vec![Vec2::new(0.0, 1.05), Vec2::new(5.0, 1.05)]);
        assert_eq!(snap_ends(&polys, 0.1, 1e-9), polys);
    }
}
