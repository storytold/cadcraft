//! Property-based and hostile-input tests for geometry numerics (gaps.md S6).
//!
//! Deterministic: a fixed-seed xorshift64* generator per test, no external crate. Each property
//! runs a few thousand cheap cases; the known-answer part of the suite works at 1e6–1e7
//! coordinates, where double precision starts to show (geometry-parity.md §Tests).

use crate::{
    Arc, Bounds2, Circle, Line, Segment, Vec2, circle_circle, intersect_ext, intersect_segments, line_circle, line_line, line_line_infinite,
};

// ---------- generator ----------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in `[-scale, scale]`.
    fn f64(&mut self, scale: f64) -> f64 {
        ((self.next() >> 11) as f64 / (1u64 << 53) as f64) * 2.0 * scale - scale
    }
    /// Uniform in `[lo, hi]`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn vec(&mut self, scale: f64) -> Vec2 {
        Vec2::new(self.f64(scale), self.f64(scale))
    }
    /// An f64 chosen to be nasty: non-finite, subnormal, extreme or raw bit patterns.
    fn hostile(&mut self) -> f64 {
        match self.next() % 10 {
            0 => 0.0,
            1 => -0.0,
            2 => f64::NAN,
            3 => f64::INFINITY,
            4 => f64::NEG_INFINITY,
            5 => 1.0e300,
            6 => -1.0e300,
            7 => 5.0e-324,
            8 => f64::MIN_POSITIVE,
            _ => f64::from_bits(self.next()),
        }
    }
}

// ---------- comparison helpers ----------

/// Relative closeness with a floor, so zero-scale cases stay meaningful.
fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}

fn pt_eq(a: Vec2, b: Vec2, rel: f64) -> bool {
    near(a.x, b.x, rel) && near(a.y, b.y, rel)
}

/// Every point of `a` matches some point of `b` and vice versa (multiset within tolerance).
fn same_set(a: &[Vec2], b: &[Vec2], rel: f64) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut used = vec![false; b.len()];
    'outer: for p in a {
        for (i, q) in b.iter().enumerate() {
            if !used[i] && pt_eq(*p, *q, rel) {
                used[i] = true;
                continue 'outer;
            }
        }
        return false;
    }
    true
}

/// `p` lies on the circle: |p - c| = r.
fn on_circle(p: Vec2, c: Vec2, r: f64, rel: f64) -> bool {
    near(p.dist(c), r.abs(), rel)
}

/// `p` lies on the line through `a` with direction `d`: cross(d, p - a) = 0.
fn on_line(p: Vec2, a: Vec2, d: Vec2, rel: f64) -> bool {
    (d.cross(p - a)).abs() <= rel * d.len() * (p - a).len().max(1.0)
}

// ---------- line/line ----------

/// The infinite-line intersection lies on both lines, and swapping the lines gives the same point.
#[test]
fn line_line_infinite_lies_on_both_and_is_symmetric() {
    let mut r = Rng::new(0xC0FFEE);
    let mut checked = 0;
    for _ in 0..4000 {
        let (a1, a2, b1, b2) = (r.vec(1000.0), r.vec(1000.0), r.vec(1000.0), r.vec(1000.0));
        let Some((p, t, u)) = line_line_infinite(a1, a2, b1, b2) else { continue };
        checked += 1;
        // The parameters put the point back on each line.
        assert!(pt_eq(a1 + (a2 - a1) * t, p, 1e-9), "t does not reproduce p: {t}");
        assert!(pt_eq(b1 + (b2 - b1) * u, p, 1e-9), "u does not reproduce p: {u}");
        assert!(on_line(p, a1, a2 - a1, 1e-9) && on_line(p, b1, b2 - b1, 1e-9), "p {p:?} not on both lines");
        // Symmetry.
        let swapped = line_line_infinite(b1, b2, a1, a2);
        assert!(swapped.is_some_and(|(q, _, _)| pt_eq(p, q, 1e-9)), "a×b {p:?} != b×a {swapped:?}");
    }
    assert!(checked > 1000, "almost-parallel filter ate everything ({checked} cases)");
}

/// The finite-line result agrees with the parameters of the infinite one.
#[test]
fn line_line_matches_infinite_parameters() {
    let mut r = Rng::new(0xBAD5EED);
    for _ in 0..4000 {
        let a = Line::new(r.vec(100.0), r.vec(100.0));
        let b = Line::new(r.vec(100.0), r.vec(100.0));
        let finite = line_line(&a, &b);
        match line_line_infinite(a.a, a.b, b.a, b.b) {
            Some((p, t, u)) => {
                let inside = (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u);
                if let Some(q) = finite {
                    assert!(inside && pt_eq(p, q, 1e-9), "finite hit {q:?} at t={t} u={u} outside the segments");
                } else {
                    assert!(!inside, "no finite hit but t={t} u={u} are inside");
                }
            }
            None => assert!(finite.is_none(), "parallel lines report a finite intersection"),
        }
    }
}

// ---------- line/circle ----------

/// Every reported hit lies on the line and on the circle; a line through the centre hits at ±r
/// along its direction (known answer).
#[test]
fn line_circle_hits_lie_on_both() {
    let mut r = Rng::new(0x5EED1);
    for _ in 0..4000 {
        let l = Line::new(r.vec(1000.0), r.vec(1000.0));
        let c = Circle { center: r.vec(100.0), radius: r.range(0.01, 50.0) };
        for (p, t) in line_circle(&l, &c) {
            assert!(p.x.is_finite() && p.y.is_finite(), "non-finite hit {p:?}");
            assert!(on_circle(p, c.center, c.radius, 1e-9), "hit {p:?} not on the circle {c:?}");
            assert!(on_line(p, l.a, l.b - l.a, 1e-9), "hit {p:?} not on the line {l:?}");
            // The parameter reproduces the hit to within the coordinate scale's rounding.
            let scale = l.a.len().max(l.b.len()).max(c.center.len()).max(1.0);
            assert!(l.at(t).dist(p) <= 1e-9 * scale, "parameter {t} off the line");
        }
    }
    // Known answer: through the centre at a known angle.
    let dir = Vec2::new(3.0, 4.0).normalized();
    for &(cx, cy) in &[(0.0, 0.0), (1.0e6, -5.0e5), (8.0e6, 7.0e6)] {
        let c = Circle { center: Vec2::new(cx, cy), radius: 2.5 };
        let far = c.center + dir * 100.0;
        let hits = line_circle(&Line::new(c.center - dir, far), &c);
        assert_eq!(hits.len(), 2, "a line through the centre crosses twice at ({cx},{cy})");
        let want = [c.center - dir * c.radius, c.center + dir * c.radius];
        for h in hits.iter().map(|(p, _)| *p) {
            assert!(want.iter().any(|w| pt_eq(h, *w, 1e-9)), "hit {h:?} != expected {want:?} at ({cx},{cy})");
        }
    }
}

/// Hostile lines and circles never panic; guarded functions return only finite hits or none.
#[test]
fn line_circle_hostile_inputs_stay_finite() {
    let mut r = Rng::new(0xF00D);
    for _ in 0..3000 {
        let l = Line::new(Vec2::new(r.hostile(), r.hostile()), Vec2::new(r.hostile(), r.hostile()));
        let c = Circle { center: Vec2::new(r.hostile(), r.hostile()), radius: r.hostile() };
        for (p, _) in line_circle(&l, &c) {
            assert!(p.x.is_finite() && p.y.is_finite(), "non-finite hit from hostile input: {p:?}");
        }
        // Degenerate line (a == b) on a sane circle: no hits, no panic.
        let c2 = Circle { center: Vec2::new(1.0, 2.0), radius: 3.0 };
        assert!(line_circle(&Line::new(c2.center, c2.center), &c2).is_empty(), "zero-length line crosses nothing");
    }
}

// ---------- circle/circle ----------

/// Hits lie on both circles, the result is symmetric, and the closed-form answer matches.
#[test]
fn circle_circle_hits_lie_on_both_and_match_the_closed_form() {
    let mut r = Rng::new(0xC1C1E);
    for _ in 0..4000 {
        let c1 = Circle { center: r.vec(100.0), radius: r.range(1.0, 40.0) };
        let c2 = Circle { center: r.vec(100.0), radius: r.range(1.0, 40.0) };
        let hits = circle_circle(&c1, &c2);
        for p in &hits {
            assert!(on_circle(*p, c1.center, c1.radius, 1e-9), "hit {p:?} not on circle 1");
            assert!(on_circle(*p, c2.center, c2.radius, 1e-9), "hit {p:?} not on circle 2");
        }
        assert!(same_set(&hits, &circle_circle(&c2, &c1), 1e-9), "c1×c2 {hits:?} != c2×c1");
        // Closed form when they properly cross: x = (d² + r1² − r2²) / 2d, h = √(r1² − x²).
        let d = c1.center.dist(c2.center);
        if c1.radius.abs() < d && c2.radius.abs() < d && d < c1.radius + c2.radius && d > (c1.radius - c2.radius).abs() && d > 1e-9 {
            let dir = (c2.center - c1.center) / d;
            let x = (d * d + c1.radius * c1.radius - c2.radius * c2.radius) / (2.0 * d);
            let h2 = c1.radius * c1.radius - x * x;
            if h2 > 0.0 {
                let h = h2.sqrt();
                let want = [c1.center + dir * x + Vec2::new(-dir.y, dir.x) * h, c1.center + dir * x - Vec2::new(-dir.y, dir.x) * h];
                assert_eq!(hits.len(), 2, "properly crossing circles report {} hits: {c1:?} {c2:?}", hits.len());
                for p in &hits {
                    assert!(want.iter().any(|w| pt_eq(*p, *w, 1e-9)), "hit {p:?} != closed form {want:?}");
                }
            }
        }
    }
}

/// Degenerate and hostile circles: concentric, contained, negative/NaN radii, extreme centres.
#[test]
fn circle_circle_degenerate_and_hostile_never_panic() {
    let mut r = Rng::new(0xD15EA5E);
    for _ in 0..3000 {
        let c1 = Circle { center: Vec2::new(r.hostile(), r.hostile()), radius: r.hostile() };
        let c2 = Circle { center: Vec2::new(r.hostile(), r.hostile()), radius: r.hostile() };
        for p in circle_circle(&c1, &c2) {
            assert!(p.x.is_finite() && p.y.is_finite(), "non-finite hit from hostile circles: {p:?}");
        }
    }
    let o = Circle { center: Vec2::ZERO, radius: 5.0 };
    assert!(circle_circle(&o, &Circle { center: Vec2::ZERO, radius: 5.0 }).is_empty(), "concentric circles do not cross");
    assert!(circle_circle(&o, &Circle { center: Vec2::new(1.0, 0.0), radius: 1.0 }).is_empty(), "contained circle does not cross");
    assert!(circle_circle(&o, &Circle { center: Vec2::new(20.0, 0.0), radius: 1.0 }).is_empty(), "far circle does not cross");
    assert!(circle_circle(&o, &Circle { center: Vec2::ZERO, radius: -5.0 }).is_empty(), "negative radius is not a circle");
}

// ---------- segments ----------

/// Extending segments only adds hits: the finite result is a subset of the extended one.
#[test]
fn segment_extension_only_adds_hits() {
    let mut r = Rng::new(0x5E6C0DE);
    for _ in 0..3000 {
        let s1 = Segment::Line(Line::new(r.vec(100.0), r.vec(100.0)));
        let s2 = Segment::Line(Line::new(r.vec(100.0), r.vec(100.0)));
        let finite = intersect_segments(&s1, &s2);
        let extended = intersect_ext(&s1, &s2, true);
        for p in finite {
            assert!(extended.iter().any(|q| pt_eq(p, *q, 1e-9)), "finite hit {p:?} missing from the extended result");
        }
        for p in &extended {
            assert!(p.x.is_finite() && p.y.is_finite(), "non-finite segment hit {p:?}");
        }
    }
}

// ---------- closest points ----------

/// The closest point on a line is a fixed point of `closest`, and the offset is perpendicular.
#[test]
fn line_closest_is_a_fixed_point_and_perpendicular() {
    let mut r = Rng::new(0xC105E);
    for _ in 0..4000 {
        let l = Line::new(r.vec(1000.0), r.vec(1000.0));
        if (l.b - l.a).len() < 1e-9 {
            continue;
        }
        let p = r.vec(1000.0);
        let c = l.closest(p);
        assert!(pt_eq(l.closest(c), c, 1e-12), "closest is not idempotent at {c:?}");
        assert!(l.dist(p) >= 0.0, "negative distance");
        // `Line` is a finite segment: the offset is perpendicular only for interior projections;
        // otherwise the closest point is an endpoint.
        let t = l.param_of(p);
        let d = (l.b - l.a).normalized();
        if (1e-9..=1.0 - 1e-9).contains(&t) {
            assert!((c - p).dot(d).abs() <= 1e-9 * (c - p).len().max(1.0), "offset not perpendicular to the line");
        } else {
            assert!(pt_eq(c, l.a, 1e-12) || pt_eq(c, l.b, 1e-12), "closest outside the segment is not an endpoint: {c:?}");
        }
    }
}

/// The closest point on an arc is on its circle, or one of its endpoints.
#[test]
fn arc_closest_is_on_the_circle_or_an_endpoint() {
    let mut r = Rng::new(0xA2C10);
    for _ in 0..4000 {
        let a = Arc::new(r.vec(100.0), r.range(0.1, 50.0), r.f64(std::f64::consts::TAU), r.f64(std::f64::consts::TAU));
        let p = r.vec(200.0);
        let c = a.closest(p);
        assert!(
            on_circle(c, a.center, a.radius, 1e-9) || pt_eq(c, a.start_point(), 1e-9) || pt_eq(c, a.end_point(), 1e-9),
            "closest {c:?} neither on the circle nor an endpoint of {a:?}"
        );
    }
}

// ---------- arcs ----------

/// Parameter and constructor consistency of arcs, from tiny to huge radii.
#[test]
fn arc_endpoints_and_sampling_are_consistent() {
    let mut r = Rng::new(0xA2C_5EED_7101 & 0x7FFF_FFFF_FFFF_FFFF);
    for _ in 0..3000 {
        let radius = r.range(1e-6, 1e6);
        let a = Arc::new(r.vec(1000.0), radius, r.f64(100.0), r.f64(100.0));
        assert!(pt_eq(a.at(0.0), a.start_point(), 1e-9), "at(0) != start_point");
        assert!(pt_eq(a.at(1.0), a.end_point(), 1e-9), "at(1) != end_point");
        for k in 0..=10 {
            let t = k as f64 / 10.0;
            let p = a.at(t);
            assert!(p.x.is_finite() && p.y.is_finite(), "arc sample not finite at t={t}: {p:?}");
            assert!(on_circle(p, a.center, a.radius, 1e-9), "arc sample at t={t} off the circle: {p:?}");
        }
        assert!(a.len() >= 0.0, "negative arc length");
    }
}

/// Three points rebuild an arc through all three; collinear points cannot.
#[test]
fn arc_from_3_points_passes_through_the_inputs() {
    let mut r = Rng::new(0x3BC1);
    for _ in 0..3000 {
        let p1 = r.vec(100.0);
        let p3 = r.vec(100.0);
        if p1.dist(p3) < 1.0 {
            continue;
        }
        // A point clearly off the p1-p3 line.
        let mid = (p1 + p3) * 0.5;
        let perp = Vec2::new(-(p3 - p1).y, (p3 - p1).x).normalized() * r.range(1.0, 50.0);
        let p2 = mid + perp;
        let Some(a) = Arc::from_3_points(p1, p2, p3) else {
            assert!(perp.len() < 1e-9, "non-collinear points {p1:?} {p2:?} {p3:?} rejected");
            continue;
        };
        assert!(
            on_circle(p1, a.center, a.radius, 1e-9) && on_circle(p2, a.center, a.radius, 1e-9) && on_circle(p3, a.center, a.radius, 1e-9),
            "rebuilt arc misses its inputs: {a:?}"
        );
        // The arc type is CCW-only: when p2 lies on the clockwise side of p1→p3, the same point
        // set is stored with start/end swapped (as DXF arcs are; the direction lives in Segment).
        assert!(
            (pt_eq(a.start_point(), p1, 1e-9) && pt_eq(a.end_point(), p3, 1e-9))
                || (pt_eq(a.start_point(), p3, 1e-9) && pt_eq(a.end_point(), p1, 1e-9)),
            "arc endpoints are not {{p1, p3}}: {a:?}"
        );
        // Whichever way it is stored, p2 lies on the returned arc.
        assert!(a.contains_angle(a.center.angle_to(p2)), "p2 is not on the returned arc: {a:?}");
        // The radius matches the circumradius closed form.
        let ((ax, ay), (bx, by), (cx2, cy2)) = ((p1.x, p1.y), (p2.x, p2.y), (p3.x, p3.y));
        let d = 2.0 * (ax * (by - cy2) + bx * (cy2 - ay) + cx2 * (ay - by));
        let ux = ((ax * ax + ay * ay) * (by - cy2) + (bx * bx + by * by) * (cy2 - ay) + (cx2 * cx2 + cy2 * cy2) * (ay - by)) / d;
        let uy = ((ax * ax + ay * ay) * (cx2 - bx) + (bx * bx + by * by) * (ax - cx2) + (cx2 * cx2 + cy2 * cy2) * (bx - ax)) / d;
        let circum = (Vec2::new(ux, uy) - p1).len();
        assert!(near(a.radius, circum, 1e-9), "radius {} != circumradius {circum}", a.radius);
    }
    // Exactly collinear points (axis-aligned integers) cannot define a circle.
    assert!(Arc::from_3_points(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)).is_none(), "collinear points have no circumcircle");
    assert!(
        Arc::from_3_points(Vec2::new(5.0, 7.0), Vec2::new(5.0, 9.0), Vec2::new(5.0, 12.0)).is_none(),
        "vertical collinear points have no circumcircle"
    );
}

// ---------- bounds ----------

#[test]
fn bounds_union_contains_both_boxes() {
    let mut r = Rng::new(0xB0D4D);
    for _ in 0..3000 {
        let b1 = Bounds2::from_points([r.vec(100.0), r.vec(100.0)]);
        let b2 = Bounds2::from_points([r.vec(100.0), r.vec(100.0)]);
        let u = b1.union(&b2);
        for b in [&b1, &b2] {
            assert!(u.contains(b.min) && u.contains(b.max), "union does not contain {b:?}");
        }
        assert!(u.width() >= b1.width() - 1e-12 && u.width() >= b2.width() - 1e-12, "union narrower than its parts");
        assert!(u.height() >= b1.height() - 1e-12 && u.height() >= b2.height() - 1e-12, "union shorter than its parts");
        assert!(
            !u.intersects(&Bounds2::from_points([Vec2::new(1e6, 1e6), Vec2::new(2e6, 2e6)])) || u.max.x >= 1e6,
            "intersects disagrees with containment"
        );
    }
    // A box contains itself and contains_box agrees with contains on corners.
    let b = Bounds2::from_points([Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)]);
    assert!(b.contains_box(&b));
}

// ---------- vectors ----------

#[test]
fn vector_identities_hold_for_finite_inputs() {
    let mut r = Rng::new(0xEEC1 & 0x7FFF);
    for _ in 0..4000 {
        let a = r.vec(100.0);
        let b = r.vec(100.0);
        assert!(near(a.dot(b), b.dot(a), 1e-12), "dot is not commutative");
        assert!(pt_eq(a + b - b, a, 1e-12), "(a + b) - b != a");
        assert!(near(a.len2(), a.dot(a), 1e-12), "len2 != dot(a, a)");
        if a.len() > 1e-9 {
            let n = a.normalized();
            assert!(near(n.len(), 1.0, 1e-12), "normalized length is not 1: {n:?}");
        }
        let ang = r.f64(1e6);
        assert!(near(Vec2::from_angle(ang).len(), 1.0, 1e-12), "from_angle is not unit at {ang}");
        assert!(near(Vec2::polar(a, b.len(), ang).dist(a), b.len(), 1e-12), "polar is not at the given distance");
    }
}

/// Known answers at 1e6–1e7 coordinates, where ulp grows to ~2e-9…2e-7 (geometry-parity.md).
#[test]
fn large_coordinate_known_answers_stay_accurate() {
    for &shift in &[1.0e6, 3.0e6, 1.0e7] {
        let base = Vec2::new(shift, -shift * 0.4);
        // Two lines crossing at a known point.
        let target = base + Vec2::new(123.456, -78.9);
        let d1 = Vec2::new(1.0, 0.25);
        let d2 = Vec2::new(-0.5, 1.75);
        let crossed = line_line_infinite(target - d1 * 250.0, target + d1 * 250.0, target - d2 * 250.0, target + d2 * 250.0);
        assert!(crossed.is_some(), "orthogonal-ish lines failed to cross at {shift}");
        let (p, _, _) = crossed.unwrap_or((target, 0.0, 0.0));
        assert!(p.dist(target) <= 1e-9 * shift, "crossing drifted {} at shift {shift}", p.dist(target));
        // A circle pair with a closed-form crossing.
        let c1 = Circle { center: base, radius: 10.0 };
        let c2 = Circle { center: base + Vec2::new(12.0, 0.0), radius: 10.0 };
        let hits = circle_circle(&c1, &c2);
        assert_eq!(hits.len(), 2, "symmetric circles cross twice at {shift}: {hits:?}");
        let x = (12.0f64 * 12.0 + 10.0 * 10.0 - 10.0 * 10.0) / (2.0 * 12.0);
        let h = (10.0 * 10.0 - x * x).sqrt();
        for p in &hits {
            let want1 = Vec2::new(base.x + x, base.y + h);
            let want2 = Vec2::new(base.x + x, base.y - h);
            assert!(pt_eq(*p, want1, 1e-9) || pt_eq(*p, want2, 1e-9), "hit {p:?} != closed form ({want1:?} / {want2:?}) at {shift}");
        }
    }
}
