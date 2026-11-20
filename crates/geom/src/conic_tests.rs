//! Tests for exact conic intersections and closest points (`conic.rs`): known answers (also at
//! 1e6–1e7 coordinates), tangencies, a brute-force comparison on random ellipses, and hostile input.

use crate::*;

struct Rng(u64);

impl Rng {
    /// Next xorshift64* value.
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in `[lo, hi]`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// A random full ellipse near `offset` with axes up to `size`.
    fn ellipse(&mut self, offset: Vec2, size: f64) -> Ellipse {
        let center = offset + Vec2::new(self.range(-size, size), self.range(-size, size));
        let ang = self.range(0.0, TAU);
        let len = self.range(0.2 * size, size);
        Ellipse::full(center, Vec2::new(ang.cos(), ang.sin()) * len, self.range(0.15, 1.0))
    }
}

/// True when two points are within `tol`.
fn close(a: Vec2, b: Vec2, tol: f64) -> bool {
    a.dist(b) <= tol
}

/// Points sorted by x, then y, for order-independent comparison.
fn sorted(mut v: Vec<Vec2>) -> Vec<Vec2> {
    v.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    v
}

/// Asserts that two point sets match one to one within `tol`.
fn assert_points(got: Vec<Vec2>, want: &[Vec2], tol: f64) {
    let got = sorted(got);
    let want = sorted(want.to_vec());
    assert_eq!(got.len(), want.len(), "got {got:?}, want {want:?}");
    for (g, w) in got.iter().zip(&want) {
        assert!(close(*g, *w, tol), "got {g:?}, want {w:?} (tol {tol})");
    }
}

/// Just the points of `(point, t1, t2)` results.
fn points(v: Vec<(Vec2, f64, f64)>) -> Vec<Vec2> {
    v.into_iter().map(|(p, _, _)| p).collect()
}

// ---------- known answers ----------

/// Secant, vertical, tangent and missing lines against a known ellipse, at the origin and far from it.
#[test]
fn line_ellipse_known_answer_and_params() {
    for off in [Vec2::ZERO, Vec2::new(3.0e6, -7.0e6)] {
        let e = Ellipse::full(off + Vec2::new(3.0, 4.0), Vec2::new(5.0, 0.0), 0.4);
        let l = Line::new(off + Vec2::new(-10.0, 4.0), off + Vec2::new(10.0, 4.0));
        let hits = line_ellipse(&l, &e);
        assert_points(points(hits.clone()), &[off + Vec2::new(-2.0, 4.0), off + Vec2::new(8.0, 4.0)], 1e-9 * (1.0 + off.len()));
        for (p, t, te) in hits {
            assert!(close(l.at(t), p, 1e-9 * (1.0 + off.len())));
            assert!(close(e.at_param(te), p, 1e-9 * (1.0 + off.len())));
        }
        // Vertical line through the centre: (3, 4 ± 2).
        let v = Line::new(off + Vec2::new(3.0, -100.0), off + Vec2::new(3.0, 100.0));
        assert_points(points(line_ellipse(&v, &e)), &[off + Vec2::new(3.0, 2.0), off + Vec2::new(3.0, 6.0)], 1e-9 * (1.0 + off.len()));
        // Tangent line at the top: one point.
        let t = Line::new(off + Vec2::new(-10.0, 6.0), off + Vec2::new(10.0, 6.0));
        assert_points(points(line_ellipse(&t, &e)), &[off + Vec2::new(3.0, 6.0)], 1e-6 * (1.0 + off.len()));
        // Missing line.
        let m = Line::new(off + Vec2::new(-10.0, 6.1), off + Vec2::new(10.0, 6.1));
        assert!(line_ellipse(&m, &e).is_empty());
    }
}

/// Two crossed ellipses meet in four known points, also rotated and at 1e6–1e7.
#[test]
fn crossed_ellipses_four_points_rotated_and_far_from_origin() {
    // x²/4 + y² = 1 and x² + y²/4 = 1 meet at (±2/√5, ±2/√5).
    let k = 2.0 / 5f64.sqrt();
    for (off, rot) in [(Vec2::ZERO, 0.0_f64), (Vec2::new(1.0e7, 2.0e6), 0.7), (Vec2::new(-4.0e6, 9.0e6), 2.3)] {
        let dir = Vec2::new(rot.cos(), rot.sin());
        let e1 = Ellipse::full(off, dir * 2.0, 0.5);
        let e2 = Ellipse::full(off, dir.perp() * 2.0, 0.5);
        let want: Vec<Vec2> = [(k, k), (k, -k), (-k, k), (-k, -k)].iter().map(|&(x, y)| off + Vec2::new(x, y).rotate(rot)).collect();
        let hits = ellipse_ellipse(&e1, &e2);
        assert_points(points(hits.clone()), &want, 1e-9 * (1.0 + off.len()));
        for (p, t1, t2) in hits {
            assert!(close(e1.at_param(t1), p, 1e-9 * (1.0 + off.len())));
            assert!(close(e2.at_param(t2), p, 1e-9 * (1.0 + off.len())));
        }
    }
}

/// Inner and outer tangencies, disjoint, nested and coincident curves.
#[test]
fn tangencies_give_single_points() {
    // Inner double tangency: x²/4 + y² = 1 and the unit circle touch at (0, ±1).
    let e = Ellipse::full(Vec2::ZERO, Vec2::new(2.0, 0.0), 0.5);
    let c = Circle::new(Vec2::ZERO, 1.0);
    assert_points(points(circle_ellipse(&c, &e)), &[Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0)], 1e-6);
    // Outer tangency at (2, 0).
    let c = Circle::new(Vec2::new(3.0, 0.0), 1.0);
    assert_points(points(circle_ellipse(&c, &e)), &[Vec2::new(2.0, 0.0)], 1e-6);
    // Disjoint and nested curves have no common points.
    assert!(circle_ellipse(&Circle::new(Vec2::new(10.0, 0.0), 1.0), &e).is_empty());
    assert!(circle_ellipse(&Circle::new(Vec2::ZERO, 0.5), &e).is_empty());
    // Coincident curves: no isolated points.
    assert!(ellipse_ellipse(&e, &e).is_empty());
    assert!(ellipse_ellipse(&e, &Ellipse::full(Vec2::ZERO, Vec2::new(-2.0, 0.0), 0.5)).is_empty());
}

/// A circle cutting an ellipse in four known points far from the origin.
#[test]
fn circle_ellipse_two_points_known_answer() {
    // x²/4 + y² = 1 and the circle of radius √2 at the origin: x² = 4/3, y² = 2/3.
    let e = Ellipse::full(Vec2::new(5.0e6, 5.0e6), Vec2::new(2.0, 0.0), 0.5);
    let c = Circle::new(e.center, 2f64.sqrt());
    let (x, y) = ((4.0f64 / 3.0).sqrt(), (2.0f64 / 3.0).sqrt());
    let want: Vec<Vec2> = [(x, y), (x, -y), (-x, y), (-x, -y)].iter().map(|&(a, b)| e.center + Vec2::new(a, b)).collect();
    assert_points(points(circle_ellipse(&c, &e)), &want, 1e-8);
}

/// Only the common points on both elliptical arcs are kept.
#[test]
fn arcs_filter_by_sweep() {
    let k = 2.0 / 5f64.sqrt();
    let mut e1 = Ellipse::full(Vec2::ZERO, Vec2::new(2.0, 0.0), 0.5);
    e1.start = 0.0;
    e1.end = PI * 0.5; // upper-right quarter only
    let e2 = Ellipse::full(Vec2::ZERO, Vec2::new(0.0, 2.0), 0.5);
    assert_points(points(ellipse_arcs(&e1, &e2)), &[Vec2::new(k, k)], 1e-9);
}

/// Conic from an ellipse and back, circles, line pairs and pencil members.
#[test]
fn conic_round_trips_and_line_pairs() {
    let e = Ellipse::full(Vec2::new(12.0, -3.0), Vec2::new(3.0, 4.0), 0.3);
    let q = Conic::from_ellipse(&e).unwrap();
    assert!(q.discriminant() < 0.0 && q.det().abs() > 1e-12);
    for i in 0..16 {
        assert!(q.eval(e.at_param(i as f64 * 0.4)).abs() < 1e-9);
    }
    let back = q.to_ellipse().unwrap();
    assert!(close(back.center, e.center, 1e-9));
    assert!((back.major.len() - 5.0).abs() < 1e-9 && (back.ratio - 0.3).abs() < 1e-9);
    assert!(back.major.normalized().cross(e.major.normalized()).abs() < 1e-9);
    // A circle is an ellipse with ratio 1.
    let c = Conic::from_circle(&Circle::new(Vec2::new(1.0, 2.0), 3.0)).unwrap().to_ellipse().unwrap();
    assert!(close(c.center, Vec2::new(1.0, 2.0), 1e-12) && (c.major.len() - 3.0).abs() < 1e-12 && (c.ratio - 1.0).abs() < 1e-12);
    // A line pair is degenerate and vanishes on both lines.
    let lp = Conic::from_line_pair(Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 2.0), Vec2::new(2.0, 0.0));
    assert!(lp.det().abs() < 1e-12 && lp.to_ellipse().is_none());
    assert!(lp.eval(Vec2::new(5.0, 5.0)).abs() < 1e-12 && lp.eval(Vec2::new(-1.0, 3.0)).abs() < 1e-12);
    // The pencil member halfway between two concentric circles is the circle in between.
    let c1 = Conic::from_circle(&Circle::new(Vec2::ZERO, 1.0)).unwrap();
    let c3 = Conic::from_circle(&Circle::new(Vec2::ZERO, 3.0)).unwrap();
    let mid = c1.add_scaled(&c3, 1.0).to_ellipse().unwrap();
    assert!((mid.major.len() - 5f64.sqrt()).abs() < 1e-12);
}

// ---------- brute force on random ellipses ----------

/// Roots of `q2(e1(t))` by sign changes on a fine sampling plus bisection. Cases with a near
/// tangency (a local minimum of |f| close to zero without a sign change) are reported as `None`.
fn brute_roots(e1: &Ellipse, q2: &Conic) -> Option<Vec<Vec2>> {
    let n = 20_000;
    let f = |t: f64| q2.eval(e1.at_param(t));
    let scale = (0..64).map(|i| f(i as f64 * TAU / 64.0).abs()).fold(0.0, f64::max).max(1e-300);
    let mut roots = Vec::new();
    let mut prev = f(0.0);
    let mut prev2 = prev;
    for i in 1..=n {
        let t = TAU * i as f64 / n as f64;
        let v = f(t);
        if prev == 0.0 || prev.signum() != v.signum() {
            let (mut lo, mut hi) = (TAU * (i - 1) as f64 / n as f64, t);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if f(lo).signum() == f(mid).signum() { lo = mid } else { hi = mid }
            }
            roots.push(e1.at_param(0.5 * (lo + hi)));
        } else if prev.abs() < v.abs() && prev.abs() < prev2.abs() && prev.abs() < 1e-6 * scale {
            return None; // ambiguous near-tangency
        }
        prev2 = prev;
        prev = v;
    }
    // A root exactly at t = 0 is seen twice (start and end).
    let mut out: Vec<Vec2> = Vec::new();
    for r in roots {
        if out.iter().all(|o| o.dist(r) > 1e-9 * (1.0 + r.len())) {
            out.push(r);
        }
    }
    Some(out)
}

/// Random ellipse pairs agree with sign-change sampling plus bisection.
#[test]
fn random_ellipse_pairs_match_brute_force() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut checked = 0;
    let mut histogram = [0usize; 5];
    for case in 0..1500 {
        let off = if case % 3 == 0 { Vec2::new(4.0e6, 6.0e6) } else { Vec2::ZERO };
        let size = if case % 5 == 0 { 1000.0 } else { 10.0 };
        let e1 = rng.ellipse(off, size);
        let e2 = if case % 4 == 0 {
            // Crossing pairs: e1 turned about a nearby centre, usually four common points.
            let turn = rng.range(0.4, 2.7);
            let nudge = Vec2::new(rng.range(-0.1, 0.1), rng.range(-0.1, 0.1)) * e1.major.len() * e1.ratio;
            Ellipse::full(e1.center + nudge, e1.major.rotate(turn) * rng.range(0.9, 1.1), e1.ratio * rng.range(0.8, 1.2))
        } else {
            rng.ellipse(off, size)
        };
        // Brute force in e1's own local frame so far-off coordinates don't spoil the reference.
        let local = |e: &Ellipse| Ellipse::full(e.center - off, e.major, e.ratio);
        let q2 = Conic::from_ellipse(&local(&e2)).unwrap().normalized();
        let Some(want) = brute_roots(&local(&e1), &q2) else { continue };
        let want: Vec<Vec2> = want.into_iter().map(|p| p + off).collect();
        let got = points(ellipse_ellipse(&e1, &e2));
        let tol = 1e-7 * size;
        assert_eq!(got.len(), want.len(), "case {case}: {e1:?} {e2:?}\n got {got:?}\n want {want:?}");
        for w in &want {
            assert!(got.iter().any(|g| g.dist(*w) <= tol), "case {case}: missing {w:?} in {got:?}");
        }
        checked += 1;
        if let Some(h) = histogram.get_mut(got.len()) {
            *h += 1;
        }
    }
    assert!(checked > 1200, "too many ambiguous cases: {checked}");
    // Every count of common points actually occurs (0, 1–2, 3–4 are all exercised).
    eprintln!("ellipse pairs by number of common points: {histogram:?}");
    assert!(histogram[0] > 100 && histogram[2] > 100 && histogram[4] > 50, "{histogram:?}");
}

/// Random line–ellipse hits lie on both curves with consistent parameters.
#[test]
fn random_lines_match_closed_form_residuals() {
    let mut rng = Rng(0xD1B54A32D192ED03);
    for case in 0..2000 {
        let off = if case % 2 == 0 { Vec2::new(-3.0e6, 8.0e6) } else { Vec2::ZERO };
        let e = rng.ellipse(off, 50.0);
        let q = Conic::from_ellipse(&Ellipse::full(e.center - off, e.major, e.ratio)).unwrap().normalized();
        let a = off + Vec2::new(rng.range(-80.0, 80.0), rng.range(-80.0, 80.0));
        let b = off + Vec2::new(rng.range(-80.0, 80.0), rng.range(-80.0, 80.0));
        for (p, t, te) in line_ellipse(&Line::new(a, b), &e) {
            assert!(q.eval(p - off).abs() < 1e-9, "case {case}");
            let on_line = a + (b - a) * t;
            assert!(on_line.dist(p) < 1e-6, "case {case}: {on_line:?} vs {p:?}");
            assert!(e.at_param(te).dist(p) < 1e-9 * (1.0 + off.len()));
        }
    }
}

// ---------- closest point ----------

/// Reference distance from `p` to the curve by dense sampling.
fn sampled_closest(e: &Ellipse, p: Vec2) -> f64 {
    let n = 40_000;
    let sweep = e.sweep();
    let mut best = f64::INFINITY;
    for i in 0..=n {
        best = best.min(e.at_param(e.start + sweep * i as f64 / n as f64).dist(p));
    }
    best
}

/// Closest points on the axes and the off-axis feet from inside near the centre.
#[test]
fn closest_point_known_answers() {
    let e = Ellipse::full(Vec2::new(1.0e6, 1.0e6), Vec2::new(4.0, 0.0), 0.5);
    // Points on the axes, inside and outside, including the evolute's cusp region.
    for (q, want) in [
        (Vec2::new(10.0, 0.0), Vec2::new(4.0, 0.0)),
        (Vec2::new(0.0, 7.0), Vec2::new(0.0, 2.0)),
        (Vec2::new(0.0, -0.5), Vec2::new(0.0, -2.0)),
        (Vec2::new(-10.0, 0.0), Vec2::new(-4.0, 0.0)),
    ] {
        let got = e.closest(e.center + q);
        assert!(close(got, e.center + want, 1e-9 * 1.0e6), "{q:?}: {got:?}");
    }
    // Inside near the centre on the major axis: the nearest points are off-axis (x = a²x₀/(a²−b²)).
    let got = e.closest(e.center + Vec2::new(1.5, 0.0)) - e.center;
    assert!((got.x - 2.0).abs() < 1e-9 && (got.y.abs() - 3f64.sqrt()).abs() < 1e-9, "{got:?}");
}

/// The exact closest point is never farther than dense sampling, on ellipses and arcs.
#[test]
fn closest_point_beats_dense_sampling_on_ellipses_and_arcs() {
    let mut rng = Rng(0xA0761D6478BD642F);
    for case in 0..600 {
        let mut e = rng.ellipse(Vec2::ZERO, 20.0);
        if case % 2 == 1 {
            e.start = rng.range(0.0, TAU);
            e.end = e.start + rng.range(0.2, 5.5);
        }
        let p = Vec2::new(rng.range(-40.0, 40.0), rng.range(-40.0, 40.0));
        let got = e.closest(p);
        let t = ellipse_arc_closest_param(&e, p).unwrap();
        assert!(on_ellipse_arc(&e, t), "case {case}");
        assert!(close(e.at_param(t), got, 1e-12));
        let sampled = sampled_closest(&e, p);
        assert!(got.dist(p) <= sampled + 1e-9, "case {case}: exact {} > sampled {sampled}", got.dist(p));
        assert!(got.dist(p) >= sampled - 1e-3, "case {case}: exact {} far below sampled {sampled}", got.dist(p));
    }
}

/// Ratios above 1 and negative ratios give correct closest points.
#[test]
fn ratio_above_one_and_negative_ratio_are_handled() {
    let p = Vec2::new(0.3, 9.0);
    for ratio in [2.0, -0.5] {
        let e = Ellipse::full(Vec2::ZERO, Vec2::new(4.0, 0.0), ratio);
        let got = e.closest(p);
        assert!(got.dist(p) <= sampled_closest(&e, p) + 1e-9);
        let t = ellipse_closest_param(&e, p).unwrap();
        assert!(close(e.at_param(t), got, 1e-12));
    }
}

// ---------- hostile input ----------

/// NaN, infinities, zeros and extreme values never panic and yield only finite points.
#[test]
fn hostile_input_never_panics() {
    let bad = [f64::NAN, f64::INFINITY, -f64::INFINITY, 0.0, -0.0, 1e308, -1e308, 1e-310, f64::MIN_POSITIVE];
    let ok = Ellipse::full(Vec2::ZERO, Vec2::new(2.0, 0.0), 0.5);
    for &x in &bad {
        for &y in &bad {
            let e = Ellipse::full(Vec2::new(x, 1.0), Vec2::new(y, x), y);
            let l = Line::new(Vec2::new(x, y), Vec2::new(y, x));
            for (p, _, _) in ellipse_ellipse(&e, &ok).into_iter().chain(ellipse_ellipse(&ok, &e)).chain(line_ellipse(&l, &ok)) {
                assert!(p.is_finite());
            }
            for (p, _, _) in line_ellipse(&l, &e).into_iter().chain(circle_ellipse(&Circle::new(Vec2::new(x, y), y), &ok)) {
                assert!(p.is_finite());
            }
            let _ = e.closest(Vec2::new(x, y));
            let _ = ok.closest(Vec2::new(x, y));
            if let Some(t) = ellipse_arc_closest_param(&ok, Vec2::new(x, y)) {
                assert!(t.is_finite());
            }
            let _ = Conic::new(x, y, x, y, x, y).to_ellipse();
        }
    }
}

/// Points almost on the major axis (inside the evolute) keep their exact off-axis feet: the root
/// search works with `u = s + 1`, which would otherwise cancel to zero (CodeRabbit on #1754).
#[test]
fn closest_point_near_major_axis_stays_exact() {
    let e = Ellipse::full(Vec2::ZERO, Vec2::new(4.0, 0.0), 0.5);
    // Foot from (1.5, 0⁺): x = a²·x₀/(a² − b²) = 2, y = b·√(1 − (x/a)²) = √3.
    for y in [1e-300, 1e-20, 1e-16, 1e-12, 1e-9, 1e-6] {
        // Off the axis the foot moves by O(y), so allow for that.
        let tol = 1e-9 + y;
        let got = e.closest(Vec2::new(1.5, y));
        assert!((got.x - 2.0).abs() < tol && (got.y - 3f64.sqrt()).abs() < tol, "y = {y:e}: {got:?}");
        let mirrored = e.closest(Vec2::new(-1.5, -y));
        assert!((mirrored.x + 2.0).abs() < tol && (mirrored.y + 3f64.sqrt()).abs() < tol, "y = -{y:e}: {mirrored:?}");
    }
    // Ratio above 1 (`major` is the short axis, here along y; the long axis of length 4 is along x)
    // and far from the origin: near the long axis the feet are still (±2, ±√3).
    let far = Ellipse::full(Vec2::new(3.0e6, -2.0e6), Vec2::new(0.0, 2.0), 2.0);
    let got = far.closest(far.center + Vec2::new(1.5, 1e-20)) - far.center;
    assert!((got.x - 2.0).abs() < 1e-6 && (got.y.abs() - 3f64.sqrt()).abs() < 1e-6, "{got:?}");
}
