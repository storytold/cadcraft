//! Midpoint, Quadrant, Perpendicular and Tangent snaps on elliptical arcs and splines (#332).

use cadcraft_doc::{EntityKind, Space};
use cadcraft_engine::Session;
use cadcraft_engine::snap::{self, mode};
use cadcraft_geom::Vec2;

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()).unwrap()
}

/// The snap of `modes` near `p` (a hair off it, inside a 0.3 aperture).
fn snap_at(s: &Session, p: Vec2, modes: u32, base: Option<Vec2>) -> Option<(Vec2, u32)> {
    snap::osnap(s.doc().unwrap(), &Space::Model, p + Vec2::new(0.03, -0.02), 0.3, modes, base, false).map(|h| (h.point, h.mode))
}

/// A quarter of an ellipse (semi-axes 4 and 2) from (4,0) to (0,2), typed at the command line.
fn quarter_ellipse() -> Session {
    let mut s = Session::new();
    s.cmdline("ellipse a c 0,0 4,0 2 0 90").unwrap();
    assert!(s.running.is_none());
    let EntityKind::Ellipse(e) = last(&s) else { panic!("no elliptical arc") };
    assert!((e.end - e.start - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{e:?}");
    s
}

#[test]
fn elliptical_arc_midpoint_and_quadrants() {
    let s = quarter_ellipse();
    // The midpoint by length: both halves of the arc as long.
    let (m, k) = along_quarter(&s, mode::MID, None).expect("a midpoint");
    assert_eq!(k, mode::MID);
    assert!(((m.x / 4.0).powi(2) + (m.y / 2.0).powi(2) - 1.0).abs() < 1e-9, "on the ellipse: {m:?}");
    let len = |a: f64, b: f64| {
        (0..10_000)
            .map(|i| {
                let t = |j: f64| a + (b - a) * j / 10_000.0;
                let p = |t: f64| Vec2::new(4.0 * t.cos(), 2.0 * t.sin());
                p(t(i as f64)).dist(p(t(i as f64 + 1.0)))
            })
            .sum::<f64>()
    };
    let tm = (m.y / 2.0).atan2(m.x / 4.0);
    assert!((len(0.0, tm) - len(tm, std::f64::consts::FRAC_PI_2)).abs() < 1e-4);
    // Quadrants: the two the arc passes through, not the far ones.
    assert_eq!(snap_at(&s, Vec2::new(4.0, 0.0), mode::QUA, None).map(|h| h.0), Some(Vec2::new(4.0, 0.0)));
    assert!(snap_at(&s, Vec2::new(0.0, 2.0), mode::QUA, None).is_some_and(|h| h.0.near(Vec2::new(0.0, 2.0), 1e-9)));
    assert_eq!(snap_at(&s, Vec2::new(-4.0, 0.0), mode::QUA, None), None);
    assert_eq!(snap_at(&s, Vec2::new(0.0, -2.0), mode::QUA, None), None);
}

/// The first snap of `modes` (from `base`) met while moving the cursor along the quarter ellipse.
fn along_quarter(s: &Session, modes: u32, base: Option<Vec2>) -> Option<(Vec2, u32)> {
    (0..=200).find_map(|k| {
        let a = std::f64::consts::FRAC_PI_2 * f64::from(k) / 200.0;
        snap_at(s, Vec2::new(4.0 * a.cos(), 2.0 * a.sin()), modes, base)
    })
}

fn on_quarter(p: Vec2) -> bool {
    p.x >= -1e-9 && p.y >= -1e-9 && ((p.x / 4.0).powi(2) + (p.y / 2.0).powi(2) - 1.0).abs() < 1e-9
}

#[test]
fn elliptical_arc_perpendicular_and_tangent_from_a_base_point() {
    let s = quarter_ellipse();
    // Tangent from (6,1): the line from the base is perpendicular to the ellipse's normal
    // (the gradient of x²/16 + y²/4) where it touches.
    let base = Vec2::new(6.0, 1.0);
    let (q, m) = along_quarter(&s, mode::TAN, Some(base)).expect("a tangent point on the arc");
    assert_eq!(m, mode::TAN);
    assert!(on_quarter(q) && Vec2::new(q.x / 16.0, q.y / 4.0).dot(q - base).abs() < 1e-9, "{q:?}");
    // Perpendicular from (10,10): the line from the base runs along the normal.
    let base = Vec2::new(10.0, 10.0);
    let (q, m) = along_quarter(&s, mode::PER, Some(base)).expect("a perpendicular foot on the arc");
    assert_eq!(m, mode::PER);
    assert!(on_quarter(q) && Vec2::new(q.x / 16.0, q.y / 4.0).cross(q - base).abs() < 1e-6, "{q:?}");
    // The other tangent from (6,1) touches the ellipse below the axis, off the arc.
    assert!((0..=200).all(|k| {
        let a = -std::f64::consts::FRAC_PI_2 * f64::from(k) / 200.0;
        snap_at(&s, Vec2::new(4.0 * a.cos(), 2.0 * a.sin()), mode::TAN, Some(Vec2::new(6.0, 1.0))).is_none_or(|(p, _)| on_quarter(p))
    }));
}

#[test]
fn spline_midpoint_perpendicular_and_tangent() {
    let mut s = Session::new();
    // A symmetric arch through (0,0), (2,2), (4,0).
    s.cmdline("spline 0,0 2,2 4,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let EntityKind::Spline(sp) = last(&s) else { panic!("no spline") };
    let top = sp.eval({
        let (lo, hi) = sp.domain();
        (lo + hi) / 2.0
    });
    assert!((top.x - 2.0).abs() < 1e-9, "{top:?}");
    let (m, k) = snap_at(&s, top, mode::MID, None).expect("a midpoint");
    assert_eq!(k, mode::MID);
    assert!(m.near(top, 1e-6), "{m:?} vs {top:?}");
    // Perpendicular from straight below the top: the top.
    let (p, k) = snap_at(&s, top, mode::PER, Some(Vec2::new(2.0, -5.0))).expect("a perpendicular foot");
    assert_eq!(k, mode::PER);
    assert!(p.near(top, 1e-6), "{p:?}");
    // Tangent from above the top: the line touches the arch where it runs straight at the base.
    let base = Vec2::new(2.0, top.y + 1.0);
    let (lo, hi) = sp.domain();
    let mut hit = None;
    for i in 0..=400 {
        let q = sp.eval(lo + (hi - lo) * i as f64 / 400.0);
        if let Some(h) = snap_at(&s, q, mode::TAN, Some(base)) {
            hit = Some(h);
            break;
        }
    }
    let (q, k) = hit.expect("a tangent point");
    assert_eq!(k, mode::TAN);
    let i = (0..=10_000).map(|i| lo + (hi - lo) * i as f64 / 10_000.0).min_by(|a, b| sp.eval(*a).dist(q).total_cmp(&sp.eval(*b).dist(q))).unwrap();
    let d = (sp.eval(i + 1e-6) - sp.eval(i - 1e-6)).normalized();
    assert!(d.cross((q - base).normalized()).abs() < 1e-3, "the line from the base runs along the curve at {q:?}");
}
