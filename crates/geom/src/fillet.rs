//! Circles tangent to two curves (FILLET) and the points where two curves cross, for lines,
//! circles, ellipses and splines.
//!
//! The centre of a radius-`r` circle tangent to two curves lies on an offset of each, at
//! distance `r` along the normal. Sampled offset curves are intersected to find every candidate,
//! then Newton's method on the exact offset curves (finite-difference Jacobian, capped
//! iterations) refines each one. `r = 0` gives the crossings of the curves themselves. Standard
//! numerical methods; degenerate input yields no solutions, never a panic or NaN.

use crate::{Bounds2, Circle, Ellipse, Line, Spline, TAU, Vec2};

/// A curve as a tangency target. Lines are infinite, circles and ellipses whole (parameter =
/// angle, eccentric angle for ellipses), splines limited to their domain.
#[derive(Clone, Debug, PartialEq)]
pub enum TanCurve {
    Line(Line),
    Circle(Circle),
    Ellipse(Ellipse),
    Spline(Spline),
}

/// One solution: curve parameters and points where the circle touches each curve, and its
/// centre (equal to the touch points when the radius is zero).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerFit {
    pub t1: f64,
    pub t2: f64,
    pub p1: Vec2,
    pub p2: Vec2,
    pub center: Vec2,
}

/// Samples round a whole circle or ellipse.
const ROUND_SAMPLES: usize = 720;
/// Most samples along one spline.
const MAX_SPLINE_SAMPLES: usize = 2048;
/// Newton iterations per candidate.
const NEWTON_STEPS: usize = 40;
/// Most solutions reported.
const MAX_SOLUTIONS: usize = 64;

/// Parameter range of a curve for sampling; `bounded` ranges clamp (splines end).
#[derive(Clone, Copy, Debug)]
struct Range {
    lo: f64,
    hi: f64,
    bounded: bool,
}

impl TanCurve {
    /// Point at parameter `t`.
    pub fn point(&self, t: f64) -> Vec2 {
        match self {
            TanCurve::Line(l) => l.a + (l.b - l.a) * t,
            TanCurve::Circle(c) => c.center + Vec2::from_angle(t) * c.radius,
            TanCurve::Ellipse(e) => e.at_param(t),
            TanCurve::Spline(s) => s.eval(t),
        }
    }

    /// First derivative at parameter `t`.
    pub fn derivative(&self, t: f64) -> Vec2 {
        match self {
            TanCurve::Line(l) => l.b - l.a,
            TanCurve::Circle(c) => Vec2::from_angle(t).perp() * c.radius,
            TanCurve::Ellipse(e) => e.derivative(t),
            TanCurve::Spline(s) => s.derivative(t),
        }
    }

    /// The point at distance `d` along the left normal at `t` (`None` where the curve stops).
    fn offset(&self, t: f64, d: f64) -> Option<Vec2> {
        let p = self.point(t);
        if d == 0.0 {
            return p.is_finite().then_some(p);
        }
        let n = self.derivative(t).perp().normalized();
        let q = p + n * d;
        (n != Vec2::ZERO && q.is_finite()).then_some(q)
    }

    /// A rough box round a bounded curve.
    fn rough_bounds(&self) -> Option<Bounds2> {
        let b = match self {
            TanCurve::Line(_) => return None,
            TanCurve::Circle(c) => Bounds2::new(c.center - Vec2::new(c.radius, c.radius), c.center + Vec2::new(c.radius, c.radius)),
            TanCurve::Ellipse(e) => Ellipse { start: 0.0, end: TAU, ..*e }.bounds(),
            TanCurve::Spline(s) => Bounds2::from_points(s.control.iter().copied()),
        };
        (!b.is_empty()).then_some(b)
    }

    /// Parameter range to search; infinite lines are cut to `window`.
    fn range(&self, window: &Bounds2) -> Option<Range> {
        match self {
            TanCurve::Line(l) => {
                let d = l.b - l.a;
                let len2 = d.len2();
                if !(len2 > 1e-300 && len2.is_finite()) {
                    return None;
                }
                let ts = window.corners().map(|c| (c - l.a).dot(d) / len2);
                let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                (lo.is_finite() && hi.is_finite() && hi > lo).then_some(Range { lo, hi, bounded: false })
            }
            TanCurve::Circle(c) => (c.radius > 1e-300 && c.radius.is_finite()).then_some(Range { lo: 0.0, hi: TAU, bounded: false }),
            TanCurve::Ellipse(e) => {
                (e.major.len() > 1e-300 && e.ratio > 0.0 && e.ratio.is_finite()).then_some(Range { lo: 0.0, hi: TAU, bounded: false })
            }
            TanCurve::Spline(s) => {
                let (lo, hi) = s.domain();
                (s.is_valid() && lo.is_finite() && hi > lo).then_some(Range { lo, hi, bounded: true })
            }
        }
    }

    fn sample_count(&self) -> usize {
        match self {
            TanCurve::Line(_) => 1,
            TanCurve::Circle(_) | TanCurve::Ellipse(_) => ROUND_SAMPLES,
            TanCurve::Spline(s) => s.control.len().saturating_mul(16).clamp(256, MAX_SPLINE_SAMPLES),
        }
    }
}

/// A sampled offset curve: runs of consecutive (parameter, point) samples.
fn sample(c: &TanCurve, r: &Range, d: f64) -> Vec<Vec<(f64, Vec2)>> {
    let n = c.sample_count();
    let mut runs: Vec<Vec<(f64, Vec2)>> = vec![Vec::new()];
    for i in 0..=n {
        let t = r.lo + (r.hi - r.lo) * i as f64 / n as f64;
        match c.offset(t, d) {
            Some(p) => {
                if let Some(run) = runs.last_mut() {
                    run.push((t, p));
                }
            }
            None => runs.push(Vec::new()),
        }
    }
    runs.retain(|r| r.len() >= 2);
    runs
}

/// Where segments `a0-a1` and `b0-b1` cross: fractions along each.
fn seg_cross(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> Option<(f64, f64)> {
    let (da, db) = (a1 - a0, b1 - b0);
    let den = da.cross(db);
    if den.abs() <= 1e-300 {
        return None;
    }
    let w = b0 - a0;
    let u = w.cross(db) / den;
    let v = w.cross(da) / den;
    let tol = 1e-9;
    ((-tol..=1.0 + tol).contains(&u) && (-tol..=1.0 + tol).contains(&v)).then_some((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
}

/// Crossings of two sampled polylines, as parameter seeds, with chunked box culling.
fn crossings(a: &[(f64, Vec2)], b: &[(f64, Vec2)], out: &mut Vec<(f64, f64)>, work: &mut usize) {
    const CHUNK: usize = 16;
    let boxes = |s: &[(f64, Vec2)]| -> Vec<(usize, Bounds2)> {
        (0..s.len().saturating_sub(1))
            .step_by(CHUNK)
            .map(|i| (i, Bounds2::from_points(s.iter().skip(i).take(CHUNK + 1).map(|x| x.1)).expand(1e-9)))
            .collect()
    };
    let (ba, bb) = (boxes(a), boxes(b));
    for (ia, xa) in &ba {
        for (ib, xb) in &bb {
            let Some(left) = work.checked_sub(1) else { return };
            *work = left;
            if !xa.intersects(xb) {
                continue;
            }
            for i in *ia..(*ia + CHUNK).min(a.len().saturating_sub(1)) {
                let (Some(&(ta0, a0)), Some(&(ta1, a1))) = (a.get(i), a.get(i + 1)) else { continue };
                for j in *ib..(*ib + CHUNK).min(b.len().saturating_sub(1)) {
                    let Some(left) = work.checked_sub(1) else { return };
                    *work = left;
                    let (Some(&(tb0, b0)), Some(&(tb1, b1))) = (b.get(j), b.get(j + 1)) else { continue };
                    if let Some((u, v)) = seg_cross(a0, a1, b0, b1) {
                        out.push((ta0 + (ta1 - ta0) * u, tb0 + (tb1 - tb0) * v));
                        if out.len() > MAX_SOLUTIONS * 8 {
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Newton's method on `offset1(t1) = offset2(t2)`.
#[allow(clippy::too_many_arguments)]
fn refine(c1: &TanCurve, r1: &Range, d1: f64, c2: &TanCurve, r2: &Range, d2: f64, seed: (f64, f64), tol: f64) -> Option<(f64, f64)> {
    let clamp = |r: &Range, t: f64| if r.bounded { t.clamp(r.lo, r.hi) } else { t };
    let (h1, h2) = (((r1.hi - r1.lo) * 1e-7).max(1e-12), ((r2.hi - r2.lo) * 1e-7).max(1e-12));
    let (mut a, mut b) = seed;
    for _ in 0..NEWTON_STEPS {
        let f = c1.offset(a, d1)? - c2.offset(b, d2)?;
        if f.len() <= tol * 1e-6 {
            break;
        }
        // One-sided differences at a bounded end.
        let diff = |c: &TanCurve, r: &Range, t: f64, d: f64, h: f64| -> Option<Vec2> {
            let (lo, hi) = (clamp(r, t - h), clamp(r, t + h));
            if hi - lo <= 0.0 {
                return None;
            }
            Some((c.offset(hi, d)? - c.offset(lo, d)?) / (hi - lo))
        };
        let ja = diff(c1, r1, a, d1, h1)?;
        let jb = -diff(c2, r2, b, d2, h2)?;
        let det = ja.cross(jb);
        if det.abs() <= 1e-300 {
            break;
        }
        let rhs = -f;
        let (da, db) = (rhs.cross(jb) / det, ja.cross(rhs) / det);
        if !(da.is_finite() && db.is_finite()) {
            return None;
        }
        a = clamp(r1, a + da);
        b = clamp(r2, b + db);
    }
    let f = c1.offset(a, d1)? - c2.offset(b, d2)?;
    (f.len() <= tol && a.is_finite() && b.is_finite()).then_some((a, b))
}

/// Circles of radius `r` tangent to both curves (`r = 0`: the points where they cross). `near`
/// (the pick points) widens the search window for lines. Every distinct solution is returned;
/// callers choose one, usually the nearest the picks.
pub fn tangent_circles(c1: &TanCurve, c2: &TanCurve, r: f64, near: &[Vec2]) -> Vec<CornerFit> {
    if !(r.is_finite() && r >= 0.0) {
        return Vec::new();
    }
    let Some(win) = search_window(c1, c2, near) else { return Vec::new() };
    let size = win.width().max(win.height()).max(r).max(1e-9);
    let win = win.expand(2.0 * r + 0.25 * size);
    let tol = 1e-9 * size.max(1.0);
    let (Some(r1), Some(r2)) = (c1.range(&win), c2.range(&win)) else { return Vec::new() };
    let signs: &[(f64, f64)] = if r > 0.0 { &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] } else { &[(0.0, 0.0)] };
    let mut out: Vec<CornerFit> = Vec::new();
    let mut work = 16_000_000usize;
    for (s1, s2) in signs {
        let (d1, d2) = (s1 * r, s2 * r);
        let (o1, o2) = (sample(c1, &r1, d1), sample(c2, &r2, d2));
        let search = TanSearch { c1, r1: &r1, d1, c2, r2: &r2, d2, tol };
        // Refine each pair before visiting the next: raw duplicate seeds in an early run must
        // not consume all candidate slots. A separate request-wide budget bounds actual work.
        for a in &o1 {
            for b in &o2 {
                if !refine_pair(&search, a, b, &mut work, &mut out) {
                    return out;
                }
            }
        }
    }
    out
}

/// The two curves of one tangent search for one sign choice: their ranges and offsets, and the
/// tolerance.
struct TanSearch<'a> {
    c1: &'a TanCurve,
    r1: &'a Range,
    d1: f64,
    c2: &'a TanCurve,
    r2: &'a Range,
    d2: f64,
    tol: f64,
}

/// The window the tangent search looks in: the pick points and the extents of the curves that
/// are not lines. `None` when it is empty or not finite.
fn search_window(c1: &TanCurve, c2: &TanCurve, near: &[Vec2]) -> Option<Bounds2> {
    let mut win = Bounds2::from_points(near.iter().copied().filter(|p| p.is_finite()));
    for c in [c1, c2] {
        match c {
            TanCurve::Line(l) => {
                win.add(l.a);
                win.add(l.b);
            }
            _ => {
                if let Some(b) = c.rough_bounds() {
                    win = win.union(&b);
                }
            }
        }
    }
    if win.is_empty() || !(win.width().is_finite() && win.height().is_finite()) {
        return None;
    }
    Some(win)
}

/// Refine the crossings of the samples `a` and `b` into circles, adding the distinct ones to
/// `out`. `false` when the work budget or the solution cap ends the search.
fn refine_pair(search: &TanSearch, a: &[(f64, Vec2)], b: &[(f64, Vec2)], work: &mut usize, out: &mut Vec<CornerFit>) -> bool {
    let mut seeds = Vec::new();
    crossings(a, b, &mut seeds, work);
    seeds.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
    seeds.dedup_by(|x, y| (x.0 - y.0).abs() < 1e-9 && (x.1 - y.1).abs() < 1e-9);
    for seed in seeds {
        // Newton refinement has a fixed iteration cap and several curve evaluations.
        let Some(left) = work.checked_sub(128) else { return false };
        *work = left;
        let TanSearch { c1, r1, d1, c2, r2, d2, tol } = *search;
        let Some((t1, t2)) = refine(c1, r1, d1, c2, r2, d2, seed, tol) else { continue };
        let (Some(center), p1, p2) = (c1.offset(t1, d1), c1.point(t1), c2.point(t2)) else { continue };
        let near_dup = |f: &CornerFit| f.center.near(center, tol * 1e3) && f.p1.near(p1, tol * 1e3) && f.p2.near(p2, tol * 1e3);
        if !out.iter().any(near_dup) {
            out.push(CornerFit { t1, t2, p1, p2, center });
        }
        if out.len() >= MAX_SOLUTIONS {
            return false;
        }
    }
    *work != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s_curve() -> Spline {
        Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(3.0, 3.0), Vec2::new(6.0, -3.0), Vec2::new(9.0, 0.0)], 3)
    }

    /// A true fillet: both touch points at distance r from the centre, along each normal.
    fn check(c1: &TanCurve, c2: &TanCurve, f: &CornerFit, r: f64) {
        for (c, t, p) in [(c1, f.t1, f.p1), (c2, f.t2, f.p2)] {
            assert!(c.point(t).near(p, 1e-12));
            assert!((p.dist(f.center) - r).abs() < 1e-7, "{f:?}");
            if r > 0.0 {
                assert!((p - f.center).normalized().dot(c.derivative(t).normalized()).abs() < 1e-6, "{f:?}");
            }
        }
    }

    #[test]
    fn fillet_line_and_ellipse() {
        let e = TanCurve::Ellipse(Ellipse::full(Vec2::ZERO, Vec2::new(4.0, 0.0), 0.5));
        let l = TanCurve::Line(Line::new(Vec2::new(-10.0, 1.0), Vec2::new(10.0, 1.0)));
        let fits = tangent_circles(&e, &l, 0.5, &[Vec2::new(3.0, 1.0)]);
        assert!(fits.len() >= 4, "{fits:?}");
        for f in &fits {
            check(&e, &l, f, 0.5);
        }
        // The crossings: y = 1 meets the ellipse at x = ±4·√(3)/2.
        let x = tangent_circles(&e, &l, 0.0, &[]);
        assert_eq!(x.len(), 2, "{x:?}");
        for f in &x {
            assert!((f.p1.x.abs() - 2.0 * 3f64.sqrt()).abs() < 1e-9 && (f.p1.y - 1.0).abs() < 1e-9);
            assert!(f.p1.near(f.p2, 1e-9));
        }
    }

    #[test]
    fn fillet_spline_with_line_and_circle() {
        let s = TanCurve::Spline(s_curve());
        let l = TanCurve::Line(Line::new(Vec2::new(4.5, -5.0), Vec2::new(4.5, 5.0)));
        let fits = tangent_circles(&s, &l, 0.4, &[]);
        assert!(!fits.is_empty());
        for f in &fits {
            check(&s, &l, f, 0.4);
        }
        let c = TanCurve::Circle(Circle::new(Vec2::new(4.5, 0.0), 1.0));
        let fits = tangent_circles(&s, &c, 0.5, &[]);
        assert!(!fits.is_empty());
        for f in &fits {
            check(&s, &c, f, 0.5);
        }
        // Spline with spline.
        let mirrored = TanCurve::Spline(Spline::from_control(s_curve().control.iter().map(|p| Vec2::new(p.x, 1.0 - p.y)).collect(), 3));
        for f in &tangent_circles(&s, &mirrored, 0.3, &[]) {
            check(&s, &mirrored, f, 0.3);
        }
    }

    #[test]
    fn degenerate_input_gives_nothing() {
        let s = TanCurve::Spline(s_curve());
        let dot = TanCurve::Line(Line::new(Vec2::ZERO, Vec2::ZERO));
        assert!(tangent_circles(&s, &dot, 1.0, &[]).is_empty());
        assert!(tangent_circles(&s, &s, f64::NAN, &[]).is_empty());
        assert!(tangent_circles(&s, &s, -1.0, &[]).is_empty());
        let flat = TanCurve::Ellipse(Ellipse::full(Vec2::ZERO, Vec2::ZERO, 0.5));
        assert!(tangent_circles(&flat, &s, 1.0, &[]).is_empty());
        let huge = TanCurve::Circle(Circle::new(Vec2::new(1e308, 1e308), 1e308));
        let _ = tangent_circles(&huge, &s, 1e308, &[Vec2::new(f64::INFINITY, 0.0)]);
        let bad = TanCurve::Spline(Spline { degree: 3, knots: vec![0.0, 1.0], control: vec![Vec2::ZERO], ..Spline::default() });
        assert!(tangent_circles(&bad, &s, 1.0, &[]).is_empty());
    }
}

#[cfg(test)]
mod seed_budget_tests {
    use super::*;
    #[test]
    fn crossing_visits_have_an_independent_work_limit() {
        let a = [(0.0, Vec2::new(0.0, 0.0)), (1.0, Vec2::new(1.0, 1.0))];
        let b = [(0.0, Vec2::new(0.0, 1.0)), (1.0, Vec2::new(1.0, 0.0))];
        let mut seeds = Vec::new();
        crossings(&a, &b, &mut seeds, &mut 0);
        assert!(seeds.is_empty());
        crossings(&a, &b, &mut seeds, &mut 10);
        assert_eq!(seeds, vec![(0.5, 0.5)]);
    }
}
