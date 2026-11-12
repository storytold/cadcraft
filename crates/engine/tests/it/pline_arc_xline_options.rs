//! PLINE arc-mode options (Angle, CEnter, Direction, Radius, Second pt, Line), line mode's
//! Length, and the XLINE options (Hor, Ang with Reference, Bisect, Offset with Through) (#397).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use serde_json::json;

/// tan(22.5°): the bulge of a quarter circle.
const QUARTER: f64 = 0.414_213_562_373_095_1;

/// The vertices (x, y, bulge) of the last polyline.
fn pline(s: &Session) -> Vec<(f64, f64, f64)> {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (v.p.x, v.p.y, v.bulge)).collect(),
        k => panic!("expected a polyline, got {k:?}"),
    }
}

fn run(script: &str) -> Session {
    let mut s = Session::new();
    s.script(script).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    s
}

#[track_caller]
fn assert_pline(s: &Session, want: &[(f64, f64, f64)]) {
    let got = pline(s);
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9 && (g.2 - w.2).abs() < 1e-9, "{got:?} != {want:?}");
    }
}

#[test]
fn angle_then_endpoint_center_or_radius() {
    assert_pline(&run("PLINE 0,0 A A 90 10,0\n\n"), &[(0.0, 0.0, QUARTER), (10.0, 0.0, 0.0)]);
    // Center (5,0): a quarter turn counterclockwise from (0,0) ends at (5,-5).
    assert_pline(&run("PLINE 0,0 A A 90 CE 5,0\n\n"), &[(0.0, 0.0, QUARTER), (5.0, -5.0, 0.0)]);
    // Radius 5 and a half circle: the chord is 10 long, here pointing up.
    assert_pline(&run("PLINE 0,0 A A 180 R 5 90\n\n"), &[(0.0, 0.0, 1.0), (0.0, 10.0, 0.0)]);
    // Enter takes the default chord direction: the tangent (+X at the start) turned by half the angle.
    assert_pline(&run("PLINE 0,0 A A 180 R 5\n\n\n"), &[(0.0, 0.0, 1.0), (0.0, 10.0, 0.0)]);
}

#[test]
fn included_angle_follows_angdir() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "ANGDIR", "value": 1 })).unwrap();
    s.script("PLINE 0,0 A A 90 10,0\n\n").unwrap();
    assert_pline(&s, &[(0.0, 0.0, -QUARTER), (10.0, 0.0, 0.0)]);
}

#[test]
fn center_then_endpoint_angle_or_length() {
    assert_pline(&run("PLINE 0,0 A CE 5,0 10,0\n\n"), &[(0.0, 0.0, 1.0), (10.0, 0.0, 0.0)]);
    assert_pline(&run("PLINE 0,0 A CE 5,0 A -90\n\n"), &[(0.0, 0.0, -QUARTER), (5.0, 5.0, 0.0)]);
    assert_pline(&run("PLINE 0,0 A CE 5,0 L 7.0710678118654755\n\n"), &[(0.0, 0.0, QUARTER), (5.0, -5.0, 0.0)]);
}

#[test]
fn direction_radius_and_second_point() {
    // Leaving upwards to end at (10,0): a half circle turning clockwise.
    assert_pline(&run("PLINE 0,0 A D 90 10,0\n\n"), &[(0.0, 0.0, -1.0), (10.0, 0.0, 0.0)]);
    // Radius 10 over a chord of 10: a 60° counterclockwise arc.
    let tan15 = (15f64).to_radians().tan();
    assert_pline(&run("PLINE 0,0 A R 10 10,0\n\n"), &[(0.0, 0.0, tan15), (10.0, 0.0, 0.0)]);
    assert_pline(&run("PLINE 0,0 A R 5 A 180 0\n\n"), &[(0.0, 0.0, 1.0), (10.0, 0.0, 0.0)]);
    // A negative radius: the 300° major arc.
    let tan75 = (75f64).to_radians().tan();
    assert_pline(&run("PLINE 0,0 A R -10 10,0\n\n"), &[(0.0, 0.0, tan75), (10.0, 0.0, 0.0)]);
    // Through (5,5) on the way to (10,0): over the top, clockwise.
    assert_pline(&run("PLINE 0,0 A S 5,5 10,0\n\n"), &[(0.0, 0.0, -1.0), (10.0, 0.0, 0.0)]);
}

#[test]
fn line_back_and_length() {
    assert_pline(&run("PLINE 0,0 A A 90 10,0 L 10,-5\n\n"), &[(0.0, 0.0, QUARTER), (10.0, 0.0, 0.0), (10.0, -5.0, 0.0)]);
    // Length continues the last segment's direction.
    assert_pline(&run("PLINE 0,0 3,4 L 5\n\n"), &[(0.0, 0.0, 0.0), (3.0, 4.0, 0.0), (6.0, 8.0, 0.0)]);
    // After a half circle from (0,-5) to (10,-5) round the bottom, the tangent points up.
    assert_pline(&run("PLINE 0,0 0,-5 A 10,-5 L L 4\n\n"), &[(0.0, 0.0, 0.0), (0.0, -5.0, 1.0), (10.0, -5.0, 0.0), (10.0, -1.0, 0.0)]);
}

#[test]
fn bad_arc_answers_reprompt() {
    // A radius too small for the chord is refused and the radius kept for another endpoint.
    let mut s = Session::new();
    s.script("PLINE 0,0 A R 2 10,0").unwrap();
    assert!(s.prompt_text().contains("Specify endpoint of arc"), "{}", s.prompt_text());
    s.script("4,0\n\n").unwrap();
    assert_pline(&s, &[(0.0, 0.0, 1.0), (4.0, 0.0, 0.0)]);
    // Enter at a sub-prompt goes back to the arc prompt.
    let mut s = Session::new();
    s.script("PLINE 0,0 A CE\n\n").unwrap();
    assert!(s.prompt_text().contains("Specify endpoint of arc or [Angle/CEnter"), "{}", s.prompt_text());
}

/// Base and unit direction of the last construction line or ray.
fn xline(s: &Session) -> (Vec2, Vec2) {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::XLine(r) | EntityKind::Ray(r) => (r.base.xy(), r.dir.xy()),
        k => panic!("expected an xline, got {k:?}"),
    }
}

#[track_caller]
fn assert_xline(s: &Session, base: (f64, f64), angle_deg: f64) {
    let (b, d) = xline(s);
    let want = Vec2::from_angle(angle_deg.to_radians());
    assert!(b.near(Vec2::new(base.0, base.1), 1e-9), "base {b:?}");
    // An xline runs both ways.
    assert!(d.cross(want).abs() < 1e-9, "direction {d:?}");
}

#[test]
fn xline_options() {
    assert_xline(&run("XLINE H 0,5\n\n"), (0.0, 5.0), 0.0);
    assert_xline(&run("XLINE A 30 0,5\n\n"), (0.0, 5.0), 30.0);
    assert_xline(&run("XLINE B 0,0 10,0 0,10\n\n"), (0.0, 0.0), 45.0);
    // Reference: 30° from a line at 45°.
    let mut s = Session::new();
    s.script("LINE 0,0 10,10\n\nXLINE A R 5,5 30 0,5\n\n").unwrap();
    assert_xline(&s, (0.0, 5.0), 75.0);
    // Offset through a point, and by a distance from the polyline segment picked (not the first).
    let mut s = Session::new();
    s.script("PLINE 0,0 10,0 10,10\n\nXLINE O T 5,0 3,7\n\n").unwrap();
    assert_xline(&s, (3.0, 7.0), 0.0);
    s.script("XLINE O 2 10,5 12,5\n\n").unwrap();
    let (b, _) = xline(&s);
    assert!((b.x - 12.0).abs() < 1e-9, "{b:?}");
    assert_xline(&s, (b.x, b.y), 90.0);
    // RAY: start point, then through points.
    let s = run("RAY 0,0 0,10\n\n");
    assert_xline(&s, (0.0, 0.0), 90.0);
}
