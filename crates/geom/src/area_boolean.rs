//! Boolean operations on closed loops of lines and circular arcs. Split the boundaries at
//! crossings (and at coincident edges' ends), retain edges with filled area on just one side,
//! then walk the directed boundary graph. Resolvable arcs stay circular; a sagitta below the
//! relative resolution is treated as straight. No tessellation is involved.
//!
//! Coordinates are translated and scaled before geometric predicates. All pair searches,
//! classifications and graph visits share a work budget, and split storage is capped, so a
//! hostile drawing fails without leaving a partial result.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use crate::{Bounds2, Circle, PI, PolyVertex, Polyline, TAU, Vec2, circle_circle, line_line_infinite, norm_angle};

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
    sweep: f64,
    bulge: f64,
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
        // Imported round-off bulges whose entire deviation from the chord is below TOL
        // are straight at this resolution; a huge implied radius must not reject them.
        let round = if bulge.abs() * a.dist(b) * 0.5 <= TOL {
            None
        } else {
            let d = b - a;
            let center = a.mid(b) + d.perp() * ((1.0 / bulge - bulge) * 0.25);
            let radius = d.len() * (bulge.abs() + 1.0 / bulge.abs()) * 0.25;
            if !center.is_finite() || !radius.is_finite() || radius <= TOL {
                return Err(INVALID);
            }
            Some(Round { center, radius, sweep: 4.0 * bulge.atan(), bulge })
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
            Some(r) => {
                // Rotate the start radius relative to the chord. Using 2*sin(phi/2)^2
                // for 1-cos(phi) avoids subtracting a huge center from a huge radius.
                let phi = r.sweep * t;
                let half_sin = (phi * 0.5).sin();
                let one_minus_cos = 2.0 * half_sin * half_sin;
                let k = (1.0 / r.bulge - r.bulge) * 0.25;
                let d = self.b - self.a;
                self.a + d * (one_minus_cos * 0.5 + k * phi.sin()) + d.perp() * (k * one_minus_cos - phi.sin() * 0.5)
            }
        }
    }

    /// Unit direction along the edge at a fraction of its length.
    fn tangent(self, t: f64) -> Vec2 {
        match self.round {
            None => (self.b - self.a) / self.a.dist(self.b),
            Some(r) => ((self.b - self.a) / self.a.dist(self.b)).rotate(r.sweep * (t - 0.5)),
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
                let (s, c) = half_angle(r.bulge);
                let unit = (self.b - self.a) / self.a.dist(self.b);
                let start = unit * -s.abs() + unit.perp() * (-c * r.bulge.signum());
                let delta = (p - self.a) * (2.0 * s.abs() / self.a.dist(self.b));
                let angle = start.cross(delta).atan2(1.0 + start.dot(delta));
                let d = norm_angle(angle * r.sweep.signum());
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
        Self {
            a: self.at(t0),
            b: self.at(t1),
            round: self.round.map(|r| Round { sweep: r.sweep * (t1 - t0), bulge: (r.sweep * (t1 - t0) * 0.25).tan(), ..r }),
        }
    }

    /// Reverse travel, including the sign and starting angle of an arc's sweep.
    fn reversed(self) -> Self {
        Self { a: self.b, b: self.a, round: self.round.map(|r| Round { sweep: -r.sweep, bulge: -r.bulge, ..r }) }
    }

    /// Polyline bulge for this edge's signed sweep, or zero for a line.
    fn bulge(self) -> f64 {
        self.round.map_or(0.0, |r| r.bulge)
    }

    /// Conservative bounds use the sagitta for minor arcs, without cancellation at a
    /// distant center. Major arcs use their whole circle; bounds include contact tolerance.
    fn bounds(self) -> Result<Bounds2, &'static str> {
        let b = match self.round {
            None => Bounds2::new(self.a, self.b),
            Some(r) if r.bulge.abs() <= 1.0 => Bounds2::new(self.a, self.b).expand(r.bulge.abs() * self.a.dist(self.b) * 0.5),
            Some(r) => Circle::new(r.center, r.radius).bounds(),
        }
        .expand(TOL);
        if b.min.is_finite() && b.max.is_finite() { Ok(b) } else { Err(PRECISION) }
    }

    /// Minor arcs stay within a sagitta of their chords. Project both chords onto this
    /// chord's normal to reject distant curves even when rotated axis-aligned boxes overlap.
    fn separated(self, other: Self) -> bool {
        if self.round.is_some_and(|r| r.bulge.abs() > 1.0) || other.round.is_some_and(|r| r.bulge.abs() > 1.0) {
            return false;
        }
        let normal = (self.b - self.a).perp() / self.a.dist(self.b);
        let (lo, hi) = ((other.a - self.a).dot(normal), (other.b - self.a).dot(normal));
        let sag = self.bulge().abs() * self.a.dist(self.b) * 0.5 + other.bulge().abs() * other.a.dist(other.b) * 0.5 + TOL;
        lo.min(hi) > sag || lo.max(hi) < -sag
    }

    /// Only intersecting edge boxes need center/radius intersection arithmetic. A distant
    /// shallow arc must not fail the operation; a missed local crossing must not erase it.
    fn check_precision(self) -> Result<(), &'static str> {
        if self.round.is_some_and(|r| 32.0 * f64::EPSILON * (r.center.x.abs().max(r.center.y.abs()) + r.radius) > TOL) {
            Err(PRECISION)
        } else {
            Ok(())
        }
    }

    /// Infinite-line/circle roots in the chord's frame. The implicit circle equation is
    /// s*(u*u + v*v - u) - c*v = 0, so no large squared radii or centers are subtracted.
    fn line_roots(self, origin: Vec2, dir: Vec2) -> Result<Vec<f64>, &'static str> {
        let Some(r) = self.round else { return Err(INVALID) };
        let chord = self.b - self.a;
        let len2 = chord.len2();
        let q = origin - self.a;
        let (u, v) = (q.dot(chord) / len2, q.dot(chord.perp()) / len2);
        let (du, dv) = (dir.dot(chord) / len2, dir.dot(chord.perp()) / len2);
        let (s, c) = half_angle(r.bulge);
        let a = s * (du * du + dv * dv);
        let b = s * (2.0 * (u * du + v * dv) - du) - c * dv;
        let c = s * (u * u + v * v - u) - c * v;
        let scale = a.abs().max(b.abs()).max(c.abs());
        if !scale.is_finite() || scale == 0.0 {
            return Err(PRECISION);
        }
        let (a, b, c) = (a / scale, b / scale, c / scale);
        let roots = if a == 0.0 {
            if b == 0.0 {
                return Err(PRECISION);
            }
            vec![-c / b]
        } else {
            let disc = b * b - 4.0 * a * c;
            let rounding = 32.0 * f64::EPSILON * (b * b + (4.0 * a * c).abs());
            if disc < -rounding {
                return Ok(Vec::new());
            }
            let root = if disc.abs() <= rounding { 0.0 } else { disc.sqrt() };
            let q = -0.5 * (b + root.copysign(b));
            if q == 0.0 { vec![-b / (2.0 * a)] } else { vec![q / a, c / q] }
        };
        if roots.iter().all(|t| t.is_finite()) { Ok(roots) } else { Err(PRECISION) }
    }

    /// Horizontal ray crossings. Split arcs at their Y extrema so each half-open interval
    /// contributes at most once; shared vertices and tangencies then have the usual parity.
    fn crossings(self, p: Vec2) -> Result<usize, &'static str> {
        let Some(r) = self.round else {
            return Ok(usize::from(
                (self.a.y > p.y) != (self.b.y > p.y) && self.a.x + (p.y - self.a.y) * (self.b.x - self.a.x) / (self.b.y - self.a.y) > p.x,
            ));
        };
        let mut ts = vec![0.0, 1.0];
        let start_tangent = (self.b - self.a).angle() - r.sweep * 0.5;
        for angle in [0.0, PI] {
            let t = norm_angle((angle - start_tangent) * r.sweep.signum()) / r.sweep.abs();
            if t > 0.0 && t < 1.0 {
                ts.push(t);
            }
        }
        ts.sort_by(f64::total_cmp);
        let roots = self.line_roots(p, Vec2::X)?;
        Ok(ts
            .windows(2)
            .filter(|w| {
                let &[t0, t1] = *w else { return false };
                let (a, b) = (self.at(t0), self.at(t1));
                (a.y > p.y) != (b.y > p.y)
                    && roots.iter().any(|&x| x > 0.0 && self.on(p + Vec2::X * x).is_some_and(|t| t >= t0 - 1e-12 && t <= t1 + 1e-12))
            })
            .count())
    }
}

/// Sine and cosine of half the signed arc sweep, avoiding overflow for large bulges.
fn half_angle(b: f64) -> (f64, f64) {
    if b.abs() <= 1.0 {
        (2.0 * b / (1.0 + b * b), (1.0 - b * b) / (1.0 + b * b))
    } else {
        let inv = 1.0 / b;
        (2.0 * inv / (1.0 + inv * inv), (inv * inv - 1.0) / (1.0 + inv * inv))
    }
}

/// Normalized edges retain their loop ranges and conservative boxes for the sweep.
struct Input {
    edges: Vec<Edge>,
    loops: Vec<Range<usize>>,
    boxes: Vec<Bounds2>,
}

/// Validate and normalize closed input loops before constructing their directed edges.
fn edges(polys: &[Polyline], origin: Vec2, scale: f64) -> Result<Input, &'static str> {
    let mut out = Vec::new();
    let mut loops = Vec::new();
    for p in polys {
        if !p.closed || p.vertices.len() < 2 {
            return Err(INVALID);
        }
        let start = out.len();
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
        loops.push(start..out.len());
    }
    let boxes = out.iter().map(|e| e.bounds()).collect::<Result<Vec<_>, _>>()?;
    Ok(Input { edges: out, loops, boxes })
}

/// Paired parameters for finite line/arc crossings and coincident edges' shared endpoints.
fn cuts(a: Edge, b: Edge) -> Result<Vec<(f64, f64)>, &'static str> {
    a.check_precision()?;
    b.check_precision()?;
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
    Ok(points.into_iter().filter_map(|p| Some((a.on(p)?, b.on(p)?))).collect())
}

struct Splitter<'a> {
    a: &'a Input,
    b: &'a Input,
    sa: Vec<Vec<f64>>,
    sb: Vec<Vec<f64>>,
    stored: usize,
}

impl Splitter<'_> {
    /// Only overlapping edge boxes need geometric predicates or a precision check.
    fn pair(&mut self, i: usize, j: usize, budget: &mut Budget) -> Result<(), &'static str> {
        if !self.a.boxes.get(i).ok_or(INVALID)?.intersects(self.b.boxes.get(j).ok_or(INVALID)?) {
            return Ok(());
        }
        budget.spend(1)?;
        let (a, b) = (*self.a.edges.get(i).ok_or(INVALID)?, *self.b.edges.get(j).ok_or(INVALID)?);
        if a.separated(b) || b.separated(a) {
            return Ok(());
        }
        for (t, u) in cuts(a, b)? {
            self.stored += 2;
            if self.stored > MAX_PIECES {
                return Err(COMPLEX);
            }
            self.sa.get_mut(i).ok_or(INVALID)?.push(t);
            self.sb.get_mut(j).ok_or(INVALID)?.push(u);
        }
        Ok(())
    }
}

/// Sweep finite edge boxes by X. Start events precede ends at the same coordinate so shared
/// endpoints are tested. Charge active-pair visits as well as actual intersections: stacked
/// boxes with no Y overlap must not turn into an unbounded quadratic scan.
fn split_inputs(a: &Input, b: &Input, budget: &mut Budget) -> Result<(Vec<Vec<f64>>, Vec<Vec<f64>>), &'static str> {
    let mut events = Vec::new();
    for (side, input) in [(false, a), (true, b)] {
        for (i, bounds) in input.boxes.iter().enumerate() {
            events.extend([(bounds.min.x, false, side, i), (bounds.max.x, true, side, i)]);
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
    let mut splits =
        Splitter { a, b, sa: vec![Vec::new(); a.edges.len()], sb: vec![Vec::new(); b.edges.len()], stored: 2 * (a.edges.len() + b.edges.len()) };
    let (mut active_a, mut active_b) = (BTreeSet::new(), BTreeSet::new());
    for (_, end, side, i) in events {
        budget.spend(1)?;
        let (other, own) = if side { (&active_a, &mut active_b) } else { (&active_b, &mut active_a) };
        if end {
            own.remove(&i);
            continue;
        }
        budget.spend(other.len())?;
        for &j in other {
            if side {
                splits.pair(j, i, budget)?;
            } else {
                splits.pair(i, j, budget)?;
            }
        }
        own.insert(i);
    }
    Ok((splits.sa, splits.sb))
}

/// Odd-parity containment across all input loops, charging each ray/edge test to the budget.
fn contains(es: &[Edge], p: Vec2, budget: &mut Budget) -> Result<bool, &'static str> {
    budget.spend(es.len())?;
    let n = es.iter().try_fold(0usize, |n, e| e.crossings(p).map(|count| n + count))?;
    Ok(n % 2 == 1)
}

#[derive(Clone, Copy)]
enum Verdict {
    Keep,
    Reverse,
    Drop,
}

/// Keep a boundary piece only if its two sides differ in result membership; orient it so
/// filled area lies to the left. Probes stay clear of nearby noncoincident edges.
fn classify(piece: Edge, a: &[Edge], b: &[Edge], op: Operation, budget: &mut Budget) -> Result<Verdict, &'static str> {
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
        (true, false) => Verdict::Keep,
        (false, true) => Verdict::Reverse,
        _ => Verdict::Drop,
    })
}

/// A verdict can change only at a cut against the other input. Original vertices do not
/// end a run, and a run can wrap around the loop's first vertex. Keep these cut flags even
/// at t=0 or t=1 so crossings at original vertices reset the verdict on both sides.
fn classify_runs(input: &Input, splits: &[Vec<f64>], a: &[Edge], b: &[Edge], op: Operation, budget: &mut Budget) -> Result<Vec<Edge>, &'static str> {
    let mut out = Vec::new();
    for range in &input.loops {
        let mut pieces: Vec<(Edge, bool)> = Vec::new();
        let mut previous_cut = false;
        for i in range.clone() {
            let e = *input.edges.get(i).ok_or(INVALID)?;
            let mut ts: Vec<(f64, bool)> = splits.get(i).ok_or(INVALID)?.iter().map(|t| (*t, true)).collect();
            ts.extend([(0.0, false), (1.0, false)]);
            ts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut unique: Vec<(f64, bool)> = Vec::new();
            for (t, cut) in ts {
                if let Some(last) = unique.last_mut()
                    && (t - last.0).abs() * e.len() <= TOL
                {
                    last.1 |= cut;
                } else {
                    unique.push((t, cut));
                }
            }
            for w in unique.windows(2) {
                let &[(t, cut), (u, _)] = w else { continue };
                pieces.push((e.part(t, u), cut || (t == 0.0 && previous_cut)));
            }
            previous_cut = unique.last().is_some_and(|p| p.1);
        }
        if let Some(first) = pieces.first_mut() {
            first.1 |= previous_cut;
        }
        let start = pieces.iter().position(|p| p.1).unwrap_or(0);
        let mut verdict = None;
        for &(piece, cut) in pieces.iter().cycle().skip(start).take(pieces.len()) {
            let keep = match verdict {
                Some(v) if !cut => v,
                _ => classify(piece, a, b, op, budget)?,
            };
            verdict = Some(keep);
            match keep {
                Verdict::Keep => out.push(piece),
                Verdict::Reverse => out.push(piece.reversed()),
                Verdict::Drop => {}
            }
        }
    }
    Ok(out)
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
/// holes CW. Empty results have no loops. Arc sagitta at or below the relative resolution
/// (1e-10) is treated as straight. Invalid input, exhausted work/storage limits or unresolved
/// intersecting geometry return an error without a partial result.
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
    let (splits_a, splits_b) = split_inputs(&a, &b, &mut budget)?;
    let mut pieces = classify_runs(&a, &splits_a, &a.edges, &b.edges, op, &mut budget)?;
    pieces.extend(classify_runs(&b, &splits_b, &a.edges, &b.edges, op, &mut budget)?);
    stitch(pieces, min, scale, &mut budget)
}

#[cfg(test)]
#[path = "area_boolean_tests.rs"]
mod tests;
