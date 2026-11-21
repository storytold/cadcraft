//! Command-line behaviour of the SPLINE, POLYGON and CIRCLE prompt machines.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()).unwrap()
}

fn logged(s: &Session, text: &str) -> bool {
    s.log.iter().any(|l| l.contains(text))
}

#[test]
fn spline_close_makes_a_smooth_periodic_curve() {
    let mut s = Session::new();
    s.cmdline("spline 0,0 4,0 4,3 0,3 c").unwrap();
    assert!(s.running.is_none());
    let EntityKind::Spline(sp) = last(&s) else { panic!("no spline") };
    assert!(sp.closed && sp.is_periodic());
    assert_eq!(sp.fit.len(), 4, "the first point is not repeated");
    let (lo, hi) = sp.domain();
    assert!(sp.eval(lo).near(Vec2::ZERO, 1e-9) && sp.eval(hi).near(Vec2::ZERO, 1e-9));
    let h = 1e-6;
    let out = (sp.eval(lo + h) - sp.eval(lo)).normalized();
    let back = (sp.eval(hi) - sp.eval(hi - h)).normalized();
    assert!(out.near(back, 1e-4), "no corner at the seam");
}

#[test]
fn spline_rejects_text_and_switches_method() {
    let mut s = Session::new();
    s.cmdline("spline 0,0 5,5 10,0 xyz").unwrap();
    assert!(logged(&s, "Point or option keyword required."));
    s.cmdline("").unwrap();
    let EntityKind::Spline(sp) = last(&s) else { panic!("no spline") };
    assert_eq!(sp.fit, [Vec2::ZERO, Vec2::new(5.0, 5.0), Vec2::new(10.0, 0.0)], "no point was swallowed");

    // Method > CV switches to drawing by control vertices.
    s.cmdline("spline m cv 0,0 1,1 2,0 3,1").unwrap();
    s.cmdline("").unwrap();
    let EntityKind::Spline(sp) = last(&s) else { panic!("no spline") };
    assert!(sp.fit.is_empty() && sp.control.len() == 4);
}

#[test]
fn circle_radius_point_on_the_centre_is_refused() {
    for input in ["circle 2,3 2,3", "circle 2,3 d 2,3"] {
        let mut s = Session::new();
        s.cmdline(input).unwrap();
        assert_eq!(s.doc().unwrap().model.len(), 0, "{input}: no circle");
        assert!(logged(&s, "Value must be positive and nonzero."), "{input}: {:?}", s.log);
        assert!(s.running.is_some(), "{input}: still at the radius prompt");
        s.cmdline("4").unwrap();
        assert!(s.running.is_none(), "{input}: a positive value finishes");
        assert_eq!(s.doc().unwrap().model.len(), 1, "{input}: one circle");
        let want = if input.contains(" d ") { 2.0 } else { 4.0 };
        assert!(
            matches!(last(&s), EntityKind::Circle(c) if c.center.xy().near(Vec2::new(2.0, 3.0), 1e-9) && (c.radius - want).abs() < 1e-9),
            "{input}: the centre is kept"
        );
    }
}

/// The two lowest vertices of a closed polyline share their y (the bottom edge is horizontal).
fn bottom_y(k: &EntityKind) -> Option<f64> {
    let EntityKind::LwPolyline(p) = k else { return None };
    let mut ys: Vec<f64> = p.vertices.iter().map(|v| v.p.y).collect();
    ys.sort_by(f64::total_cmp);
    ((ys[0] - ys[1]).abs() < 1e-9).then_some(ys[0])
}

#[test]
fn polygon_typed_radius_has_a_horizontal_bottom_edge() {
    let mut s = Session::new();
    let pentagon = -2.0 * (std::f64::consts::PI / 5.0).cos();
    for (input, low) in
        [("polygon 6 0,0 i 2", -(3f64.sqrt())), ("polygon 6 0,0 c 2", -2.0), ("polygon 4 0,0 i 2", -(2f64.sqrt())), ("polygon 5 0,0 i 2", pentagon)]
    {
        s.cmdline(input).unwrap();
        let y = bottom_y(&last(&s));
        assert!(y.is_some_and(|y| (y - low).abs() < 1e-9), "{input}: bottom edge at {y:?}, want {low}");
    }
}

#[test]
fn polygon_edge_endpoints_must_differ() {
    let mut s = Session::new();
    s.cmdline("polygon 4 e 1,1 1,1").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0, "no polygon");
    assert!(logged(&s, "Edge endpoints must differ."), "{:?}", s.log);
    assert!(s.running.is_some(), "still at the second endpoint prompt");
    s.cmdline("3,1").unwrap();
    assert!(s.running.is_none(), "a distinct endpoint finishes");
    let EntityKind::LwPolyline(p) = last(&s) else { panic!("no polygon") };
    assert!(p.closed && p.vertices.len() == 4);
    assert!(p.vertices.iter().any(|v| v.p.near(Vec2::new(3.0, 1.0), 1e-9)), "the edge runs from 1,1 to 3,1");
}

#[test]
fn circle_center_prompt_rejects_garbage() {
    let mut s = Session::new();
    s.cmdline("circle d").unwrap();
    assert!(logged(&s, "Point or option keyword required."));
    assert!(s.running.is_some(), "still at the centre prompt");
    s.cmdline("0,0 5").unwrap();
    assert!(matches!(last(&s), EntityKind::Circle(c) if (c.radius - 5.0).abs() < 1e-9));
}

#[test]
fn polygon_radius_must_be_positive() {
    for input in ["polygon 4 0,0 i 0", "polygon 4 0,0 c 0", "polygon 5 0,0 i -5", "polygon 5 0,0 c -5", "polygon 4 0,0 i 0,0", "polygon 4 0,0 c 0,0"]
    {
        let mut s = Session::new();
        s.cmdline(input).unwrap();
        assert_eq!(s.doc().unwrap().model.len(), 0, "{input}: no polygon");
        assert!(logged(&s, "Value must be positive and nonzero."), "{input}: {:?}", s.log);
        assert!(s.running.is_some(), "{input}: still at the radius prompt");
        s.cmdline("2").unwrap();
        assert!(s.running.is_none(), "{input}: a positive radius finishes");
        assert_eq!(s.doc().unwrap().model.len(), 1, "{input}: one polygon");
        assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.closed), "{input}");
    }
}
