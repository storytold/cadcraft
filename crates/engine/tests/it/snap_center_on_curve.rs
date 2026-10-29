//! Center snaps from the curve, as in AutoCAD: with the cursor over a circle, an arc, an
//! ellipse or a polyline arc (also inside a block reference) the running Center snap finds its
//! center, however far away it is. Before, the center was only found with the cursor on the
//! center itself, so moving onto a circle with the default running snaps (END, CEN, INT, EXT)
//! showed nothing.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Insert, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::pointmod::snap_candidates;
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

/// AutoCAD's default running snaps (OSMODE 4133).
const DEFAULT: u32 = 4133;

fn snap_at(s: &Session, cursor: Vec2, aperture: f64, osmode: u32) -> Option<(Vec2, u32)> {
    snap::osnap(s.doc().unwrap(), &Space::Model, cursor, aperture, osmode, None, false).map(|h| (h.point, h.mode))
}

fn polar(c: Vec2, r: f64, deg: f64) -> Vec2 {
    let a = deg.to_radians();
    c + Vec2::new(r * a.cos(), r * a.sin())
}

#[test]
fn cursor_on_a_circle_snaps_to_its_center() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [100, 50], "radius": 20 })).unwrap();
    let c = Vec2::new(100.0, 50.0);
    for deg in [10.0, 45.0, 130.0, 250.0, 300.0] {
        // A hair off the circle, well inside the aperture; the center is 20 units away.
        let cursor = polar(c, 20.3, deg);
        let (p, m) = snap_at(&s, cursor, 1.0, DEFAULT).unwrap_or_else(|| panic!("no snap on the circle at {deg}°"));
        assert_eq!(m, mode::CEN, "at {deg}°");
        assert!(p.near(c, 1e-9), "at {deg}°: {p:?}");
    }
    // Away from the circle (outside the aperture) there is nothing.
    assert_eq!(snap_at(&s, polar(c, 23.0, 45.0), 1.0, DEFAULT), None);
    // Center off: no center.
    assert_eq!(snap_at(&s, polar(c, 20.3, 45.0), 1.0, mode::END), None);
    // On the center itself, as before.
    assert_eq!(snap_at(&s, c + Vec2::new(0.2, 0.1), 1.0, DEFAULT).map(|h| h.1), Some(mode::CEN));
}

#[test]
fn arcs_ellipses_and_polyline_arcs_snap_to_their_centers() {
    let mut s = Session::new();
    s.execute("arc", &json!({ "center": [0, 0], "radius": 10, "start": 0, "end": 90 })).unwrap();
    s.execute("ellipse", &json!({ "center": [50, 0], "major": [8, 0], "ratio": 0.5 })).unwrap();
    // A polyline with a half-circle bulge from (100,0) to (120,0): center (110,0), radius 10.
    s.execute("pline", &json!({ "vertices": [{"p": [100, 0], "bulge": 1.0}, [120, 0]] })).unwrap();
    let arc = snap_at(&s, polar(Vec2::ZERO, 10.2, 45.0), 1.0, DEFAULT).unwrap();
    assert_eq!(arc.1, mode::CEN);
    assert!(arc.0.near(Vec2::ZERO, 1e-9));
    // The arc runs 0°-90°: on its circle at 200° (not on the arc) there is no center.
    assert_eq!(snap_at(&s, polar(Vec2::ZERO, 10.2, 200.0), 1.0, DEFAULT), None);
    let el = snap_at(&s, Vec2::new(50.0, 4.2), 1.0, DEFAULT).unwrap();
    assert_eq!(el.1, mode::CEN);
    assert!(el.0.near(Vec2::new(50.0, 0.0), 1e-9));
    let pl = snap_at(&s, polar(Vec2::new(110.0, 0.0), 10.2, 270.0), 1.0, DEFAULT).unwrap();
    assert_eq!(pl.1, mode::CEN, "the bulge's arc");
    assert!(pl.0.near(Vec2::new(110.0, 0.0), 1e-9), "{:?}", pl.0);
}

#[test]
fn a_nearer_endpoint_or_intersection_still_wins() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap();
    s.execute("line", &json!({ "points": [[10, 0], [30, 0]] })).unwrap();
    // At the line's end on the circle: Endpoint/Intersection, not the far center.
    let (p, m) = snap_at(&s, Vec2::new(10.2, 0.1), 1.0, DEFAULT).unwrap();
    assert!(m == mode::END || m == mode::INT, "{m}");
    assert!(p.near(Vec2::new(10.0, 0.0), 1e-9));
}

#[test]
fn circles_inside_block_references_snap_to_their_centers() {
    let mut s = Session::new();
    let c = s.execute("circle", &json!({ "center": [5, 0], "radius": 2 })).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("block", &json!({ "name": "H", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();
    let kind = EntityKind::Insert(Insert {
        block: "H".into(),
        insert: Vec3::new(100.0, 100.0, 0.0),
        scale: Vec3::new(3.0, 3.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), kind).unwrap();
    // The circle lands at (115,100) with radius 6.
    let (p, m) = snap_at(&s, polar(Vec2::new(115.0, 100.0), 6.2, 60.0), 1.0, DEFAULT).unwrap();
    assert_eq!(m, mode::CEN);
    assert!(p.near(Vec2::new(115.0, 100.0), 1e-9), "{p:?}");
}

#[test]
fn tab_cycling_offers_the_center_from_the_curve() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap();
    let all = snap_candidates(s.doc().unwrap(), &Space::Model, polar(Vec2::ZERO, 10.2, 30.0), 1.0, DEFAULT, None, false);
    assert_eq!(all.iter().filter(|h| h.mode == mode::CEN && h.point.near(Vec2::ZERO, 1e-9)).count(), 1, "{all:?}");
}

#[test]
fn the_cursor_query_snaps_to_the_center_from_the_curve() {
    // The path the drawing area uses (Session::snap_cursor): the point a click would take.
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 50 })).unwrap();
    s.start("line").unwrap();
    let q = snap::tracking::CursorQuery { base: None, deferred: true, corner: false, aperture: 1.0, tol: 0.5, now: 0.0, shift: false };
    let r = s.snap_cursor(polar(Vec2::ZERO, 50.4, 33.0), &q);
    assert_eq!(r.snap.map(|h| h.mode), Some(mode::CEN));
    assert!(r.point.near(Vec2::ZERO, 1e-9), "{:?}", r.point);
}

/// The arc of the review of #2347 (and #1114's test): center (100,50), radius 80, 30°-150°.
fn review_arc() -> Session {
    let mut s = Session::new();
    s.execute("arc", &json!({ "center": [100, 50], "radius": 80, "start": 30, "end": 150 })).unwrap();
    s
}

#[test]
fn a_midpoint_under_the_cursor_beats_the_center_from_the_curve() {
    let s = review_arc();
    // On the arc's midpoint (100,130) with Center and Midpoint running: Midpoint, as in AutoCAD.
    for cursor in [Vec2::new(100.05, 130.0), Vec2::new(100.2, 130.1)] {
        assert_eq!(snap_at(&s, cursor, 0.5, mode::CEN | mode::MID), Some((Vec2::new(100.0, 130.0), mode::MID)), "at {cursor:?}");
        assert_eq!(snap_at(&s, cursor, 0.5, mode::MID), Some((Vec2::new(100.0, 130.0), mode::MID)), "at {cursor:?}");
    }
    // Elsewhere on the arc, with no snap point in the aperture, the center from the curve.
    let (p, m) = snap_at(&s, polar(Vec2::new(100.0, 50.0), 80.1, 70.0), 0.5, mode::CEN | mode::MID).unwrap();
    assert_eq!(m, mode::CEN);
    assert!(p.near(Vec2::new(100.0, 50.0), 1e-9), "{p:?}");
}

#[test]
fn a_quadrant_under_the_cursor_beats_the_center_from_the_curve() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [20, 0], "radius": 2 })).unwrap();
    let cursor = Vec2::new(20.1, 2.0);
    assert_eq!(snap_at(&s, cursor, 0.5, mode::CEN | mode::QUA), Some((Vec2::new(20.0, 2.0), mode::QUA)));
    assert_eq!(snap_at(&s, cursor, 0.5, mode::QUA), Some((Vec2::new(20.0, 2.0), mode::QUA)));
    // Away from the quadrants the center from the curve.
    let (p, m) = snap_at(&s, polar(Vec2::new(20.0, 0.0), 2.1, 45.0), 0.5, mode::CEN | mode::QUA).unwrap();
    assert_eq!(m, mode::CEN);
    assert!(p.near(Vec2::new(20.0, 0.0), 1e-9), "{p:?}");
}

#[test]
fn many_straight_segments_do_not_hide_a_circle_from_center() {
    // A polyline of 2,500 straight segments zigzagging over the cursor, then a circle: the
    // work limit counts circles and arcs only, so the circle's center is still found.
    let mut s = Session::new();
    let vertices: Vec<[f64; 2]> = (0..=2500).map(|i| [i as f64 * 0.04, if i % 2 == 0 { 0.0 } else { 100.0 }]).collect();
    s.execute("pline", &json!({ "vertices": vertices })).unwrap();
    s.execute("circle", &json!({ "center": [50, 50], "radius": 20 })).unwrap();
    let (p, m) = snap_at(&s, polar(Vec2::new(50.0, 50.0), 20.2, 100.0), 0.5, mode::CEN).unwrap();
    assert_eq!(m, mode::CEN);
    assert!(p.near(Vec2::new(50.0, 50.0), 1e-9), "{p:?}");
}

#[test]
fn a_viewport_center_keeps_its_curve_distance_against_paper_space() {
    use cadcraft_engine::doc::{Circle, Drawing, Viewport};
    let mut d = Drawing::new_imperial();
    let paper = Space::Paper("Layout1".into());
    // Model circle (10,0) r16 seen through a viewport at half scale: paper center (100,50), radius 8.
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(10.0, 0.0, 0.0), radius: 16.0 })).unwrap();
    let vp = Viewport {
        center: Vec3::new(100.0, 50.0, 0.0),
        width: 20.0,
        height: 20.0,
        view_center: Vec2::new(10.0, 0.0),
        view_height: 40.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    // A small paper-space circle whose curve passes 0.3 from the cursor and whose center is
    // nearer the cursor (3.3) than the model circle's (8).
    d.add(&paper, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(108.0, 53.3, 0.0), radius: 3.0 })).unwrap();
    // The cursor is on the model circle (0.1 off in paper units): its center wins.
    let h = snap::osnap(&d, &paper, Vec2::new(108.1, 50.0), 0.5, mode::CEN, None, false).unwrap();
    assert_eq!(h.mode, mode::CEN);
    assert!(h.point.near(Vec2::new(100.0, 50.0), 1e-9), "{h:?}");
    assert!(h.viewport.is_some());
    // On the paper circle, away from the model one: its center.
    let h = snap::osnap(&d, &paper, Vec2::new(108.0, 56.35), 0.5, mode::CEN, None, false).unwrap();
    assert!(h.point.near(Vec2::new(108.0, 53.3), 1e-9), "{h:?}");
    assert!(h.viewport.is_none());
}
