//! Boolean operations on closed loops of lines and circular arcs. Split the boundaries at
//! crossings (and at coincident edges' ends), retain edges with filled area on just one side,
//! then walk the directed boundary graph. Arcs stay circular; no tessellation is involved.
//!
//! Coordinates are translated and scaled before geometric predicates. All pair searches,
//! classifications and graph visits share a work budget, and split storage is capped, so a
//! hostile drawing fails without leaving a partial result.

use std::collections::{HashMap, HashSet};

use crate::{Circle, PI, PolyVertex, Polyline, TAU, Vec2, circle_circle, line_line_infinite, norm_angle};

/// Maximum number of input edges, checked before allocating their storage.
pub const MAX_EDGES: usize = 60_000;
const MAX_PIECES: usize = 120_000;
const MAX_WORK: usize = 10_000_000;
// Relative to the combined drawing extent, after normalization.
const TOL: f64 = 1e-10;
const COMPLEX: &str = "These regions are too complex to combine.";
const INVALID: &str = "Region boundaries must contain finite, closed, non-degenerate curves.";
const PRECISION: &str = "Region boundaries are too close to combine reliably at this scale.";

/// Set operation applied to the filled sides of two region boundaries.
#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Union,
    Difference,
    Intersection,
}

impl Operation {
    /// Whether the result contains a point with the given membership in each input area.
    fn filled(self, a: bool, b: bool) -> bool {
        match self {
            Self::Union => a || b,
            Self::Difference => a && !b,
            Self::Intersection => a && b,
        }
    }
}

struct Budget(usize);

impl Budget {
    /// Charge geometric work before doing it; exhaustion aborts the whole operation.
    fn spend(&mut self, n: usize) -> Result<(), &'static str> {
        self.0 = self.0.checked_sub(n).ok_or(COMPLEX)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Round {
    center: Vec2,
    radius: f64,
    start: f64,
    sweep: f64,
}

#[derive(Clone, Copy)]
struct Edge {
    a: Vec2,
    b: Vec2,
    round: Option<Round>,
}

impl Edge {
    /// Build a normalized directed line or circular arc from its endpoints and signed bulge.
    fn new(a: Vec2, b: Vec2, bulge: f64) -> Result<Self, &'static str> {
        let round = if bulge.abs() < 1e-12 {
            None
        } else {
            let d = b - a;
            let center = a.mid(b) + d.perp() * ((1.0 / bulge - bulge) * 0.25);
            let radius = d.len() * (bulge.abs() + 1.0 / bulge.abs()) * 0.25;
            if !center.is_finite() || !radius.is_finite() || radius <= TOL {
                return Err(INVALID);
            }
            // Center/radius evaluation and intersection tests subtract numbers at this
            // magnitude to recover boundary coordinates. If their rounding uncertainty
            // exceeds our normalized tolerance, missed crossings can classify a real
            // overlap as empty and cause INTERSECT to erase both source regions.
            let uncertainty = 32.0 * f64::EPSILON * (center.x.abs().max(center.y.abs()) + radius);
            if uncertainty > TOL {
                return Err(PRECISION);
            }
            Some(Round { center, radius, start: center.angle_to(a), sweep: 4.0 * bulge.atan() })
        };
        Ok(Self { a, b, round })
    }

    /// Evaluate a fraction of the edge, retaining the supplied endpoint coordinates exactly.
    fn at(self, t: f64) -> Vec2 {
        if t == 0.0 {
            return self.a;
        }
        if t == 1.0 {
            return self.b;
        }
        match self.round {
            None => self.a.lerp(self.b, t),
            Some(r) => Vec2::polar(r.center, r.radius, r.start + r.sweep * t),
        }
    }

    /// Unit direction along the edge at a fraction of its length.
    fn tangent(self, t: f64) -> Vec2 {
        match self.round {
            None => (self.b - self.a) / self.a.dist(self.b),
            Some(r) => Vec2::from_angle(r.start + r.sweep * t).perp() * r.sweep.signum(),
        }
    }

    /// Length in normalized drawing coordinates, including the full circular sweep.
    fn len(self) -> f64 {
        self.round.map_or_else(|| self.a.dist(self.b), |r| r.radius * r.sweep.abs())
    }

    /// Parameter of the line projection or radial arc projection, possibly outside the edge.
    fn parameter(self, p: Vec2) -> f64 {
        match self.round {
            None => {
                let d = self.b - self.a;
                (p - self.a).dot(d) / d.len2()
            }
            Some(r) => {
                let angle = r.center.angle_to(p);
                let d = norm_angle((angle - r.start) * r.sweep.signum());
                if TAU - d < TOL { 0.0 } else { d / r.sweep.abs() }
            }
        }
    }

    /// Distance to the finite edge, using its ends when the projection falls outside it.
    fn distance(self, p: Vec2) -> f64 {
        let t = self.parameter(p);
        if (0.0..=1.0).contains(&t) { p.dist(self.at(t)) } else { p.dist(self.a).min(p.dist(self.b)) }
    }

    /// Return a split parameter only when the point lies on this finite edge within `TOL`.
    fn on(self, p: Vec2) -> Option<f64> {
        if p.dist(self.a) <= TOL {
            return Some(0.0);
        }
        if p.dist(self.b) <= TOL {
            return Some(1.0);
        }
        let t = self.parameter(p);
        ((0.0..=1.0).contains(&t) && p.dist(self.at(t)) <= TOL).then_some(t)
    }

    /// Restrict the edge to two ordered parameters without approximating its circular arc.
    fn part(self, t0: f64, t1: f64) -> Self {
        Self { a: self.at(t0), b: self.at(t1), round: self.round.map(|r| Round { start: r.start + r.sweep * t0, sweep: r.sweep * (t1 - t0), ..r }) }
    }

    /// Reverse travel, including the sign and starting angle of an arc's sweep.
    fn reversed(self) -> Self {
        Self { a: self.b, b: self.a, round: self.round.map(|r| Round { start: r.start + r.sweep, sweep: -r.sweep, ..r }) }
    }

    /// Polyline bulge for this edge's signed sweep, or zero for a line.
    fn bulge(self) -> f64 {
        self.round.map_or(0.0, |r| (r.sweep * 0.25).tan())
    }

    /// Horizontal ray crossings. Split arcs at their Y extrema so each half-open interval
    /// contributes at most once; shared vertices and tangencies then have the usual parity.
    fn crossings(self, p: Vec2) -> usize {
        let Some(r) = self.round else {
            return usize::from(
                (self.a.y > p.y) != (self.b.y > p.y) && self.a.x + (p.y - self.a.y) * (self.b.x - self.a.x) / (self.b.y - self.a.y) > p.x,
            );
        };
        let mut ts = vec![0.0, 1.0];
        for angle in [PI * 0.5, PI * 1.5] {
            let t = norm_angle((angle - r.start) * r.sweep.signum()) / r.sweep.abs();
            if t > 0.0 && t < 1.0 {
                ts.push(t);
            }
        }
        ts.sort_by(f64::total_cmp);
        let dx = ((r.radius - (p.y - r.center.y).abs()) * (r.radius + (p.y - r.center.y).abs())).max(0.0).sqrt();
        ts.windows(2)
            .filter(|w| {
                let &[t0, t1] = *w else { return false };
                let (a, b) = (self.at(t0), self.at(t1));
                let side = (r.start + r.sweep * (t0 + t1) * 0.5).cos().signum();
                (a.y > p.y) != (b.y > p.y) && r.center.x + side * dx > p.x
            })
            .count()
    }
}

/// Validate and normalize closed input loops before constructing their directed edges.
fn edges(polys: &[Polyline], origin: Vec2, scale: f64) -> Result<Vec<Edge>, &'static str> {
    let mut out = Vec::new();
    for p in polys {
        if !p.closed || p.vertices.len() < 2 {
            return Err(INVALID);
        }
        for (i, v) in p.vertices.iter().enumerate() {
            let w = p.vertices.get((i + 1) % p.vertices.len()).ok_or(INVALID)?;
            let (a, b) = ((v.p - origin) / scale, (w.p - origin) / scale);
            if a.dist(b) <= TOL {
                // Repeated straight vertices are harmless, but losing a short curved edge
                // would silently change the area of the drawing.
                if v.bulge.abs() >= 1e-12 || a != b {
                    return Err(PRECISION);
                }
                continue;
            }
            out.push(Edge::new(a, b, v.bulge)?);
        }
    }
    Ok(out)
}

/// Paired parameters for finite line/arc crossings and coincident edges' shared endpoints.
fn cuts(a: Edge, b: Edge) -> Vec<(f64, f64)> {
    // Endpoints split collinear overlaps and coincident circular arcs, as well as T junctions.
    let mut points = vec![a.a, a.b, b.a, b.b];
    match (a.round, b.round) {
        (None, None) => {
            if let Some((p, _, _)) = line_line_infinite(a.a, a.b, b.a, b.b) {
                points.push(p);
            }
        }
        (Some(x), Some(y)) => points.extend(circle_circle(&Circle::new(x.center, x.radius), &Circle::new(y.center, y.radius))),
        _ => {
            let (line, arc) = if a.round.is_none() { (a, b.round) } else { (b, a.round) };
            if let Some(r) = arc {
                // Project to the infinite line without a world-unit length threshold.
                let d = line.b - line.a;
                let t = (r.center - line.a).dot(d) / d.len2();
                let foot = line.a + d * t;
                let h2 = (r.radius - foot.dist(r.center)) * (r.radius + foot.dist(r.center));
                // A rotated tangent can land a few ulps either side of the circle. Treat
                // that as one contact, rather than miss it or create two spurious tiny arcs.
                let roundoff = 32.0 * f64::EPSILON * r.radius * r.radius;
                if h2 >= -roundoff {
                    let h2 = if h2.abs() <= roundoff { 0.0 } else { h2 };
                    let step = d * (h2 / d.len2()).sqrt();
                    points.extend([foot - step, foot + step]);
                }
            }
        }
    }
    points.into_iter().filter_map(|p| Some((a.on(p)?, b.on(p)?))).collect()
}

/// Odd-parity containment across all input loops, charging each ray/edge test to the budget.
fn contains(es: &[Edge], p: Vec2, budget: &mut Budget) -> Result<bool, &'static str> {
    budget.spend(es.len())?;
    Ok(es.iter().map(|e| e.crossings(p)).sum::<usize>() % 2 == 1)
}

/// Keep a boundary piece only if its two sides differ in result membership; orient it so
/// filled area lies to the left. Probes stay clear of nearby noncoincident edges.
fn retain(piece: Edge, a: &[Edge], b: &[Edge], op: Operation, budget: &mut Budget) -> Result<Option<Edge>, &'static str> {
    let mid = piece.at(0.5);
    let normal = piece.tangent(0.5).perp();
    let mut gap = (piece.len() * 0.05).min(1e-6);
    budget.spend(a.len() + b.len())?;
    for e in a.iter().chain(b) {
        let d = e.distance(mid);
        if d > TOL {
            gap = gap.min(d * 0.25);
        }
    }
    if gap <= TOL * 2.0 || !mid.is_finite() {
        return Err(PRECISION);
    }
    let (left, right) = (mid + normal * gap, mid - normal * gap);
    let l = op.filled(contains(a, left, budget)?, contains(b, left, budget)?);
    let r = op.filled(contains(a, right, budget)?, contains(b, right, budget)?);
    Ok(match (l, r) {
        (true, false) => Some(piece),
        (false, true) => Some(piece.reversed()),
        _ => None,
    })
}

#[derive(Default)]
struct Nodes {
    points: Vec<Vec2>,
    bins: HashMap<(i64, i64), Vec<usize>>,
}

impl Nodes {
    /// Reuse a node within `TOL`, checking adjacent spatial bins to avoid bin-edge cracks.
    fn insert(&mut self, p: Vec2, budget: &mut Budget) -> Result<usize, &'static str> {
        let q = p / TOL;
        // Leave room for neighbour-bin arithmetic; non-finite/saturating casts would alias
        // unrelated vertices, making the output topology depend on an overflow.
        if !q.is_finite() || q.x.abs().max(q.y.abs()) > 1e18 {
            return Err(PRECISION);
        }
        let (x, y) = (q.x.floor() as i64, q.y.floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.bins.get(&(x + dx, y + dy)) {
                    budget.spend(list.len())?;
                    for &i in list {
                        if self.points.get(i).is_some_and(|v| v.dist(p) <= TOL) {
                            return Ok(i);
                        }
                    }
                }
            }
        }
        let i = self.points.len();
        self.points.push(p);
        self.bins.entry((x, y)).or_default().push(i);
        Ok(i)
    }
}

/// Deduplicate coincident pieces, trace closed result loops with filled area to the left,
/// and restore drawing coordinates. An open or ambiguous boundary returns a precision error.
fn stitch(pieces: Vec<Edge>, origin: Vec2, scale: f64, budget: &mut Budget) -> Result<Vec<Polyline>, &'static str> {
    let mut nodes = Nodes::default();
    let mut graph: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut edges: Vec<(usize, usize, Edge)> = Vec::new();
    let mut pairs: HashMap<(usize, usize), Vec<Vec2>> = HashMap::new();
    for e in pieces {
        let (u, v) = (nodes.insert(e.a, budget)?, nodes.insert(e.b, budget)?);
        if u == v {
            return Err(PRECISION);
        }
        let mids = pairs.entry((u, v)).or_default();
        budget.spend(mids.len())?;
        if mids.iter().any(|p| p.dist(e.at(0.5)) <= TOL) {
            continue;
        }
        mids.push(e.at(0.5));
        graph.entry(u).or_default().push(edges.len());
        edges.push((u, v, e));
    }
    let mut used = HashSet::new();
    let mut loops = Vec::new();
    for first in 0..edges.len() {
        if used.contains(&first) {
            continue;
        }
        let start = edges.get(first).ok_or(PRECISION)?.0;
        let mut next = first;
        let mut vertices = Vec::new();
        loop {
            budget.spend(1)?;
            if !used.insert(next) {
                return Err(PRECISION);
            }
            let &(u, v, e) = edges.get(next).ok_or(PRECISION)?;
            let p = *nodes.points.get(u).ok_or(PRECISION)? * scale + origin;
            if !p.is_finite() {
                return Err(PRECISION);
            }
            vertices.push(PolyVertex::with_bulge(p, e.bulge()));
            if v == start {
                break;
            }
            let candidates = graph.get(&v).ok_or(PRECISION)?;
            budget.spend(candidates.len())?;
            // The first clockwise edge from the reverse incoming direction bounds the face
            // on the left. Chord directions infinitesimally inside each arc also distinguish
            // tangent circles, which should stay separate loops rather than a figure eight.
            let reverse = (e.at(1.0 - 1e-4) - e.b).angle();
            next = candidates
                .iter()
                .copied()
                .filter(|i| !used.contains(i))
                .min_by(|i, j| {
                    let angle = |k: usize| edges.get(k).map_or(f64::INFINITY, |(_, _, e)| norm_angle(reverse - (e.at(1e-4) - e.a).angle()));
                    angle(*i).total_cmp(&angle(*j)).then(i.cmp(j))
                })
                .ok_or(PRECISION)?;
        }
        if vertices.len() < 2 {
            return Err(PRECISION);
        }
        loops.push(Polyline { vertices, closed: true });
    }
    Ok(loops)
}

/// Combine two areas, each represented by closed, non-self-crossing line/arc loops.
/// Containment uses odd parity, independent of input winding. Output outer loops run CCW,
/// holes CW. Empty results have no loops. No partial result is returned on invalid input,
/// exhausted work/storage limits, or geometry below the relative resolution (1e-10).
pub fn combine(a: &[Polyline], b: &[Polyline], op: Operation) -> Result<Vec<Polyline>, &'static str> {
    let mut count = 0usize;
    let mut min = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut max = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in a.iter().chain(b) {
        count = count.checked_add(p.vertices.len()).filter(|n| *n <= MAX_EDGES).ok_or(COMPLEX)?;
        for v in &p.vertices {
            if !v.p.is_finite() || !v.bulge.is_finite() {
                return Err(INVALID);
            }
            min = min.min(v.p);
            max = max.max(v.p);
        }
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let size = max - min;
    let scale = size.x.max(size.y);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(INVALID);
    }
    let (a, b) = (edges(a, min, scale)?, edges(b, min, scale)?);
    let mut budget = Budget(MAX_WORK);
    let mut splits_a = vec![vec![0.0, 1.0]; a.len()];
    let mut splits_b = vec![vec![0.0, 1.0]; b.len()];
    let mut stored = 2 * (a.len() + b.len());
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            budget.spend(1)?;
            for (t, u) in cuts(x, y) {
                stored += 2;
                if stored > MAX_PIECES {
                    return Err(COMPLEX);
                }
                splits_a.get_mut(i).ok_or(INVALID)?.push(t);
                splits_b.get_mut(j).ok_or(INVALID)?.push(u);
            }
        }
    }
    let mut pieces = Vec::new();
    for (&e, mut ts) in a.iter().zip(splits_a).chain(b.iter().zip(splits_b)) {
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|t, u| (*t - *u).abs() * e.len() <= TOL);
        for w in ts.windows(2) {
            let [t, u] = *w else { continue };
            if let Some(piece) = retain(e.part(t, u), &a, &b, op, &mut budget)? {
                pieces.push(piece);
            }
        }
    }
    stitch(pieces, min, scale, &mut budget)
}

#[cfg(test)]
#[path = "area_boolean_tests.rs"]
mod tests;
