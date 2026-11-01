//! Property tests for `vlax-curve-*`: hundreds of random curves of every kind, each function
//! checked against the others and against independent oracles (CADCraft's own curve lengths and
//! MEASURE points, AREA, dense sampling and numeric integration).

#![allow(clippy::unwrap_used)]

use cadcraft_doc::{Arc, Circle, Ellipse, Entity, EntityKind, Line, LwPolyline, Polyline3d};
use cadcraft_geom::{PolyVertex, Polyline, Spline, Vec2, Vec3};

use super::curve::Curve;
use crate::Session;
use crate::cmd::curves::Chain;

type P3 = [f64; 3];

/// A small deterministic generator (xorshift64*), so a failure reproduces.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in `lo..hi`.
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn n(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo + 1) as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        self.f(0.0, 1.0) < p
    }
    fn v2(&mut self, r: f64) -> Vec2 {
        Vec2::new(self.f(-r, r), self.f(-r, r))
    }
    fn v3(&mut self, r: f64) -> Vec3 {
        Vec3::new(self.f(-r, r), self.f(-r, r), self.f(-r, r))
    }
}

fn dist(a: &P3, b: &P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A random curve entity of kind `k` (0 line … 6 3D polyline).
fn random_kind(r: &mut Rng, k: usize) -> EntityKind {
    let tau = std::f64::consts::TAU;
    match k {
        0 => EntityKind::Line(Line { a: r.v3(100.0), b: r.v3(100.0) }),
        1 => {
            let c = r.v2(100.0);
            // Angles on either side of 0 and beyond 2π: the arc may cross the 0° direction.
            EntityKind::Arc(Arc { center: c.to3(r.f(-5.0, 5.0)), radius: r.f(0.05, 80.0), start: r.f(-7.0, 7.0), end: r.f(-7.0, 7.0) })
        }
        2 => EntityKind::Circle(Circle { center: r.v2(100.0).to3(r.f(-5.0, 5.0)), radius: r.f(0.05, 80.0) }),
        3 => {
            let n = r.n(2, 12);
            let mut vertices = Vec::new();
            let mut p = r.v2(50.0);
            for _ in 0..n {
                // Some repeated vertices (zero-length segments), bulges both ways and none.
                if !r.chance(0.1) {
                    p += r.v2(30.0);
                }
                let bulge = if r.chance(0.4) { 0.0 } else { r.f(-2.5, 2.5) };
                vertices.push(PolyVertex::with_bulge(p, bulge));
            }
            EntityKind::LwPolyline(LwPolyline { vertices, closed: r.chance(0.5), const_width: 0.0, elevation: r.f(-5.0, 5.0), plinegen: false })
        }
        4 => {
            let major = Vec2::from_angle(r.f(0.0, tau)) * r.f(0.5, 50.0);
            let full = r.chance(0.3);
            let (start, end) = if full { (0.0, tau) } else { (r.f(-1.0, 7.0), r.f(-1.0, 7.0)) };
            EntityKind::Ellipse(Ellipse { center: r.v2(100.0).to3(r.f(-5.0, 5.0)), major: major.to3(0.0), ratio: r.f(0.05, 1.0), start, end })
        }
        5 => {
            let n = r.n(3, 8);
            let mut fit = Vec::new();
            let mut p = r.v2(50.0);
            for _ in 0..n {
                p = p + r.v2(20.0) + Vec2::new(5.0, 0.0);
                fit.push(p);
            }
            EntityKind::Spline(if r.chance(0.3) { Spline::from_fit_points_closed(&fit) } else { Spline::from_fit_points(&fit) })
        }
        _ => {
            let n = r.n(2, 8);
            EntityKind::Polyline3d(Polyline3d { points: (0..n).map(|_| r.v3(50.0)).collect(), closed: r.chance(0.4) })
        }
    }
}

fn entity_of(s: &mut Session, kind: EntityKind) -> Entity {
    let h = s.add_entity(kind).unwrap();
    (**s.doc().unwrap().entity(h).unwrap()).clone()
}

/// `n + 1` points on the curve, evenly spaced by parameter (straight from the exact evaluation,
/// not through the distance functions under test).
fn samples(c: &Curve, n: usize) -> Vec<P3> {
    let (t0, t1) = (c.start_param(), c.end_param());
    (0..=n).filter_map(|i| c.point_at_param(t0 + (t1 - t0) * i as f64 / n as f64)).collect()
}

#[test]
fn every_function_agrees_with_the_others_on_random_curves() {
    let mut r = Rng(0x5EED_CAD0_1335);
    let mut s = Session::new();
    let mut checked = 0;
    for round in 0..70 {
        for k in 0..7 {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let Some(c) = Curve::of(&e) else { panic!("round {round} kind {k}: not a curve: {kind:?}") };
            let ctx = format!("round {round} kind {k}: {kind:?}");
            let len = c.length();
            let scale = 1.0 + len;
            let tol = 1e-9 * scale;
            assert!(len.is_finite() && len >= 0.0, "{ctx}");
            let (t0, t1) = (c.start_param(), c.end_param());
            assert!(t1 >= t0, "{ctx}");
            // Ends: distance 0 and the whole length.
            assert!(c.dist_at_param(t0).unwrap().abs() <= tol, "{ctx}");
            assert!((c.dist_at_param(t1).unwrap() - len).abs() <= tol, "{ctx}");
            let mut last_d = -1.0;
            for i in 0..=24 {
                let t = t0 + (t1 - t0) * i as f64 / 24.0;
                let p = c.point_at_param(t).unwrap();
                let d = c.dist_at_param(t).unwrap();
                // Distance never goes back as the parameter grows.
                assert!(d >= last_d - tol, "{ctx}: t {t}");
                last_d = d;
                // Distance → parameter → point comes back to the same point.
                let t2 = c.param_at_dist(d).unwrap();
                assert!(dist(&c.point_at_param(t2).unwrap(), &p) <= 1e3 * tol, "{ctx}: t {t} t2 {t2}");
                assert!(dist(&c.point_at_dist(d).unwrap(), &p) <= 1e3 * tol, "{ctx}: d {d}");
                // A point on the curve is its own closest point, and has a parameter.
                let (_, q) = c.closest(&p).unwrap();
                assert!(dist(&q, &p) <= 1e3 * tol, "{ctx}: closest of on-curve point {p:?} gave {q:?}");
                let tp = c.param_at_point(&p).unwrap_or_else(|| panic!("{ctx}: no parameter at its own point {p:?}"));
                assert!(dist(&c.point_at_param(tp).unwrap(), &p) <= 1e3 * tol, "{ctx}");
                checked += 1;
            }
            // Past either end: nil.
            assert!(c.point_at_param(t1 + 1e-3 * (1.0 + (t1 - t0))).is_none(), "{ctx}");
            assert!(c.point_at_dist(len + 1e-3 * scale).is_none(), "{ctx}");
            assert!(c.point_at_dist(-1e-3 * scale).is_none(), "{ctx}");
        }
    }
    assert!(checked > 10_000);
}

#[test]
fn lengths_and_measure_points_match_cadcraft_s_own_curves() {
    // `Chain` is what MEASURE, DIVIDE, LENGTHEN and BREAK walk along.
    let mut r = Rng(0xC4A1_0001);
    let mut s = Session::new();
    for round in 0..150 {
        for k in [0, 1, 2, 3] {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap_or_else(|| panic!("not a curve: {kind:?}"));
            // CADCraft's chain leaves out a polyline with no length at all.
            let Some(chain) = Chain::of(&kind) else {
                assert!(c.length() <= 1e-9, "{kind:?}");
                continue;
            };
            let ctx = format!("round {round}: {kind:?}");
            // A line's z is its own; the chain is flat. Compare in plan for the others.
            if k == 0 {
                continue;
            }
            let len = chain.len();
            assert!((c.length() - len).abs() <= 1e-9 * (1.0 + len), "{ctx}: {} vs {len}", c.length());
            for i in 0..=16 {
                let d = len * i as f64 / 16.0;
                let (want, _) = chain.at_length(d).unwrap();
                let got = c.point_at_dist(d).unwrap();
                assert!(Vec2::new(got[0], got[1]).dist(want) <= 1e-7 * (1.0 + len), "{ctx}: at {d}: {got:?} vs {want:?}");
            }
        }
    }
}

#[test]
fn ellipse_and_spline_lengths_match_numeric_integration() {
    let mut r = Rng(0xE111_9505);
    let mut s = Session::new();
    for round in 0..40 {
        for k in [4, 5] {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap();
            let (t0, t1) = (c.start_param(), c.end_param());
            // Simpson's rule on |C'(t)|, 4000 steps between each pair of knots (a spline is smooth
            // only within a span) and 20 000 over an ellipse, the speed from points on the exact
            // curve a millionth of the parameter range apart (a wider step's chord falls short).
            let dt = 1e-6 * (t1 - t0);
            let speed = |t: f64| {
                let (lo, hi) = ((t - dt).max(t0), (t + dt).min(t1));
                dist(&c.point_at_param(lo).unwrap(), &c.point_at_param(hi).unwrap()) / (hi - lo)
            };
            let mut cuts = vec![t0];
            if let EntityKind::Spline(sp) = &kind {
                cuts.extend(sp.knots.iter().copied().filter(|k| *k > t0 && *k < t1));
            }
            cuts.push(t1);
            cuts.dedup();
            let n = if cuts.len() > 2 { 4000 } else { 20_000 };
            let mut want = 0.0;
            for w in cuts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let h = (b - a) / n as f64;
                let mut sum = speed(a) + speed(b);
                for i in 1..n {
                    sum += speed(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
                }
                want += sum * h / 3.0;
            }
            let got = c.length();
            assert!((got - want).abs() <= 1e-8 * (1.0 + want), "round {round}: {kind:?}: {got} vs {want}");
        }
    }
}

#[test]
fn ellipse_and_spline_lengths_are_lengthen_s_own() {
    // LENGTHEN measures with the geometry crate's arc lengths: a routine and the command agree.
    let mut r = Rng(0x1E57_0335);
    let mut s = Session::new();
    for round in 0..60 {
        for k in [4, 5] {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap();
            let (t0, t1) = (c.start_param(), c.end_param());
            let ctx = format!("round {round}: {kind:?}");
            let (len, at) = match &kind {
                EntityKind::Spline(sp) => (sp.length(), sp.length_between(t0, (t0 + t1) / 2.0)),
                EntityKind::Ellipse(el) => {
                    let g = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
                    let full = if g.is_full() { g.arc_length_from(0.0, std::f64::consts::TAU) } else { g.length() };
                    (full, g.arc_length_from(t0, (t1 - t0) / 2.0))
                }
                _ => continue,
            };
            assert!((c.length() - len).abs() <= 1e-12 * (1.0 + len), "{ctx}: {} vs {len}", c.length());
            assert!((c.dist_at_param((t0 + t1) / 2.0).unwrap() - at).abs() <= 1e-12 * (1.0 + len), "{ctx}");
            // Distance → parameter → distance comes back to the same distance.
            for i in 0..=20 {
                let d = len * i as f64 / 20.0;
                let t = c.param_at_dist(d).unwrap();
                assert!((c.dist_at_param(t).unwrap() - d).abs() <= 1e-9 * (1.0 + len), "{ctx}: at {d}");
            }
        }
    }
}

#[test]
fn closest_points_are_never_beaten_by_a_sampled_point() {
    let mut r = Rng(0xC105_E570);
    let mut s = Session::new();
    for round in 0..40 {
        for k in 0..7 {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap();
            let pts = samples(&c, 4000);
            for _ in 0..8 {
                let q3 = r.v3(150.0);
                // Plan curves are compared in their own plane.
                let q = if k == 6 || k == 0 { [q3.x, q3.y, q3.z] } else { [q3.x, q3.y, pts.first().map_or(0.0, |p| p[2])] };
                let (_, best) = c.closest(&q).unwrap();
                let got = dist(&best, &q);
                let sampled = pts.iter().map(|p| dist(p, &q)).fold(f64::INFINITY, f64::min);
                assert!(got <= sampled + 1e-7 * (1.0 + c.length()), "round {round} kind {k}: {got} > {sampled} for {q:?}: {kind:?}");
                // Extending never gives a farther point.
                let ext = c.closest_point(&q, true).unwrap();
                assert!(dist(&ext, &q) <= got + 1e-9, "round {round} kind {k}");
            }
        }
    }
}

#[test]
fn derivatives_match_finite_differences() {
    let mut r = Rng(0xDE21_7A7E);
    let mut s = Session::new();
    for round in 0..60 {
        for k in [0, 1, 2, 3, 4, 5, 6] {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap();
            let (t0, t1) = (c.start_param(), c.end_param());
            if t1 - t0 <= 1e-9 {
                continue;
            }
            for i in 0..10 {
                // Inside a polyline segment (derivatives jump at its vertices).
                let t = if matches!(k, 3 | 6) { (t0 + i as f64).min(t1 - 1.0) + 0.5 } else { t0 + (t1 - t0) * (0.05 + 0.9 * i as f64 / 9.0) };
                if t >= t1 {
                    continue;
                }
                let h = 1e-6 * (t1 - t0).max(1.0);
                let (a, b) = (c.point_at_param(t - h).unwrap(), c.point_at_param(t + h).unwrap());
                let fd = [(b[0] - a[0]) / (2.0 * h), (b[1] - a[1]) / (2.0 * h), (b[2] - a[2]) / (2.0 * h)];
                let (d1, d2) = c.derivs(t).unwrap();
                let mag = dist(&fd, &[0.0; 3]).max(1.0);
                assert!(dist(&d1, &fd) <= 1e-4 * mag, "round {round} kind {k} t {t}: {d1:?} vs {fd:?}: {kind:?}");
                if k != 5 {
                    let (da, db) = (c.derivs(t - h).unwrap().0, c.derivs(t + h).unwrap().0);
                    let fd2 = [(db[0] - da[0]) / (2.0 * h), (db[1] - da[1]) / (2.0 * h), (db[2] - da[2]) / (2.0 * h)];
                    let mag2 = dist(&fd2, &[0.0; 3]).max(1.0);
                    assert!(dist(&d2, &fd2) <= 1e-3 * mag2, "round {round} kind {k} t {t}: {d2:?} vs {fd2:?}");
                }
            }
            // A line's parameter is its length: unit speed. An arc's or a circle's is the angle: speed r.
            match &kind {
                EntityKind::Line(_) if c.length() > 1e-9 => assert!((dist(&c.derivs(t0).unwrap().0, &[0.0; 3]) - 1.0).abs() <= 1e-9),
                EntityKind::Arc(a) => assert!((dist(&c.derivs(t0).unwrap().0, &[0.0; 3]) - a.radius).abs() <= 1e-9 * a.radius),
                EntityKind::Circle(ci) => assert!((dist(&c.derivs(t0).unwrap().0, &[0.0; 3]) - ci.radius).abs() <= 1e-9 * ci.radius),
                _ => {}
            }
        }
    }
}

/// The area of a closed polygon through dense samples of the curve.
fn sampled_area(c: &Curve, n: usize) -> f64 {
    let pts: Vec<Vec2> = samples(c, n).into_iter().map(|p| Vec2::new(p[0], p[1])).collect();
    cadcraft_geom::shoelace(&pts).abs()
}

#[test]
fn areas_match_the_area_command_and_dense_sampling() {
    let mut r = Rng(0xA2EA_0042);
    let mut s = Session::new();
    for round in 0..80 {
        for k in [1, 2, 3, 4, 5] {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap();
            let got = c.area();
            // Dense sampling, closed by the chord as an open curve is.
            let want = sampled_area(&c, 40_000);
            let size = c.length().powi(2).max(1.0);
            // A polygon through 40 000 points is itself ~1e-9 of the size short on a bend.
            assert!((got - want).abs() <= 1e-8 * size, "round {round} kind {k}: {got} vs {want}: {kind:?}");
            // AREA's Object option, for what it accepts.
            match &kind {
                EntityKind::Circle(_) | EntityKind::LwPolyline(LwPolyline { closed: true, .. }) => {
                    let a = s.execute("area", &serde_json::json!({"handle": e.handle.hex()})).unwrap();
                    let area = a["area"].as_f64().unwrap();
                    assert!((got - area).abs() <= 1e-9 * size, "round {round}: {got} vs AREA {area}: {kind:?}");
                }
                EntityKind::Ellipse(el) if (el.end - el.start - std::f64::consts::TAU).abs() < 1e-12 => {
                    let a = s.execute("area", &serde_json::json!({"handle": e.handle.hex()})).unwrap();
                    assert!((got - a["area"].as_f64().unwrap()).abs() <= 1e-9 * size, "round {round}: {kind:?}");
                }
                _ => {}
            }
            if let EntityKind::LwPolyline(pl) = &kind
                && pl.closed
            {
                let g = Polyline { vertices: pl.vertices.clone(), closed: true };
                assert!((got - g.area().abs()).abs() <= 1e-9 * size, "round {round}");
                assert!((c.length() - g.len()).abs() <= 1e-9 * (1.0 + g.len()), "round {round}");
            }
        }
    }
}

#[test]
fn ends_are_the_entity_s_own_points() {
    let mut r = Rng(0xE2D5_0001);
    let mut s = Session::new();
    for _ in 0..100 {
        for k in [0, 1, 3, 6] {
            let kind = random_kind(&mut r, k);
            let e = entity_of(&mut s, kind.clone());
            let c = Curve::of(&e).unwrap();
            let (a, b) = (c.point_at_param(c.start_param()).unwrap(), c.point_at_param(c.end_param()).unwrap());
            let (wa, wb): (P3, P3) = match &kind {
                EntityKind::Line(l) => ([l.a.x, l.a.y, l.a.z], [l.b.x, l.b.y, l.b.z]),
                EntityKind::Arc(ar) => {
                    let (s0, s1) = (Vec2::polar(ar.center.xy(), ar.radius, ar.start), Vec2::polar(ar.center.xy(), ar.radius, ar.end));
                    ([s0.x, s0.y, ar.center.z], [s1.x, s1.y, ar.center.z])
                }
                EntityKind::LwPolyline(pl) => {
                    let f = pl.vertices.first().unwrap().p;
                    let l = if pl.closed { f } else { pl.vertices.last().unwrap().p };
                    ([f.x, f.y, pl.elevation], [l.x, l.y, pl.elevation])
                }
                EntityKind::Polyline3d(pl) => {
                    let f = pl.points.first().unwrap();
                    let l = if pl.closed { f } else { pl.points.last().unwrap() };
                    ([f.x, f.y, f.z], [l.x, l.y, l.z])
                }
                _ => continue,
            };
            assert!(dist(&a, &wa) <= 1e-9 * (1.0 + c.length()), "{kind:?}: start {a:?} vs {wa:?}");
            assert!(dist(&b, &wb) <= 1e-9 * (1.0 + c.length()), "{kind:?}: end {b:?} vs {wb:?}");
        }
    }
}

#[test]
fn hostile_geometry_never_panics() {
    let mut s = Session::new();
    let big = 1e300;
    let kinds = vec![
        EntityKind::Line(Line { a: Vec3::new(big, -big, big), b: Vec3::new(-big, big, -big) }),
        EntityKind::Line(Line { a: Vec3::new(1.0, 1.0, 1.0), b: Vec3::new(1.0, 1.0, 1.0) }),
        EntityKind::Arc(Arc { center: Vec3::new(0.0, 0.0, 0.0), radius: big, start: 0.0, end: 1.0 }),
        EntityKind::Arc(Arc { center: Vec3::new(0.0, 0.0, 0.0), radius: 0.0, start: 0.0, end: 1.0 }),
        EntityKind::Arc(Arc { center: Vec3::new(0.0, 0.0, 0.0), radius: 1.0, start: 1.0, end: 1.0 }),
        EntityKind::Circle(Circle { center: Vec3::new(big, big, 0.0), radius: big }),
        EntityKind::Ellipse(Ellipse { center: Vec3::new(0.0, 0.0, 0.0), major: Vec3::new(0.0, 0.0, 0.0), ratio: 0.0, start: 0.0, end: 1.0 }),
        EntityKind::Ellipse(Ellipse { center: Vec3::new(0.0, 0.0, 0.0), major: Vec3::new(big, 0.0, 0.0), ratio: 1.0, start: 0.0, end: 7.0 }),
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![
                PolyVertex::with_bulge(Vec2::new(0.0, 0.0), 1e12),
                PolyVertex::with_bulge(Vec2::new(1e-300, 0.0), -1e12),
                PolyVertex::new(Vec2::new(5.0, 5.0)),
            ],
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
        EntityKind::LwPolyline(LwPolyline { vertices: Vec::new(), closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }),
        EntityKind::Polyline3d(Polyline3d { points: vec![Vec3::new(0.0, 0.0, 0.0)], closed: true }),
        EntityKind::Polyline3d(Polyline3d { points: Vec::new(), closed: false }),
        EntityKind::Spline(Spline::from_fit_points(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0)])),
        EntityKind::Spline(Spline::from_control(vec![Vec2::new(big, 0.0), Vec2::new(-big, big), Vec2::new(0.0, -big)], 2)),
        EntityKind::Spline(Spline::from_control(vec![Vec2::new(0.0, 0.0)], 3)),
    ];
    for kind in kinds {
        let Ok(h) = s.add_entity(kind.clone()) else { continue };
        let e = (**s.doc().unwrap().entity(h).unwrap()).clone();
        let Some(c) = Curve::of(&e) else { continue };
        for t in [f64::NAN, f64::INFINITY, -f64::INFINITY, 0.0, 0.5, 1.0, 1e300, -1e300] {
            let _ = (c.point_at_param(t), c.dist_at_param(t), c.param_at_dist(t), c.point_at_dist(t), c.derivs(t));
        }
        for q in [[0.0, 0.0, 0.0], [f64::NAN, 0.0, 0.0], [big, -big, big], [f64::INFINITY, 1.0, 1.0]] {
            let _ = (c.closest(&q), c.closest_point(&q, true), c.param_at_point(&q));
        }
        let _ = (c.area(), c.length(), c.is_closed(), c.is_planar());
    }
}
