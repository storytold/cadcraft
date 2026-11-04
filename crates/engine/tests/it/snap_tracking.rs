//! Object snap tracking (OTRACK, F11) and the Extension, Parallel and Apparent Intersection
//! snaps, driven through `Session::snap_cursor` as the UI does, without the UI.

use cadcraft_doc::{Common, EntityKind, Insert, Line, Space};
use cadcraft_engine::Session;
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::snap::tracking::{Acquired, CursorQuery, CursorSnap, DWELL, MAX_ACQUIRED, PathKind, polar_angles};
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

const APERTURE: f64 = 0.5;
const TOL: f64 = 0.1;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn session(osmode: u32) -> Session {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": osmode })).unwrap();
    s
}

fn line(s: &mut Session, a: (f64, f64), b: (f64, f64)) {
    s.execute("line", &json!({ "points": [[a.0, a.1], [b.0, b.1]] })).unwrap();
}

fn insert_block(s: &mut Session, name: &str, at: Vec2) -> String {
    s.doc_mut()
        .unwrap()
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::Insert(Insert {
                block: name.into(),
                insert: Vec3::new(at.x, at.y, 0.0),
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            }),
        )
        .unwrap()
        .hex()
}

fn setvar(s: &mut Session, name: &str, value: serde_json::Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

fn query(base: Option<Vec2>, now: f64) -> CursorQuery {
    CursorQuery { base, deferred: false, corner: false, aperture: APERTURE, tol: TOL, now, shift: false }
}

fn at(s: &mut Session, cursor: Vec2, base: Option<Vec2>) -> CursorSnap {
    s.snap_cursor(cursor, &query(base, 100.0))
}

#[test]
fn default_cursor_tracking_acquires_nested_block_geometry() {
    let mut s = session(mode::END | mode::CEN | mode::INT | mode::EXT);
    assert!(s.settings.otrack);
    let line = s
        .doc_mut()
        .unwrap()
        .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) }))
        .unwrap()
        .hex();
    s.execute("block", &json!({ "name": "INNER", "base": [0, 0], "handles": [line], "keep": "delete" })).unwrap();
    let inner = insert_block(&mut s, "INNER", Vec2::ZERO);
    s.execute("block", &json!({ "name": "OUTER", "base": [0, 0], "handles": [inner], "keep": "delete" })).unwrap();
    insert_block(&mut s, "OUTER", Vec2::ZERO);

    rest(&mut s, v(10.02, 0.02), None);

    assert_eq!(s.tracking.acquired.len(), 1);
    assert!(matches!(
        s.tracking.acquired.first(),
        Some(Acquired::Point { at, name, ext: Some(_) }) if at.near(v(10.0, 0.0), 1e-9) && *name == "Endpoint"
    ));
}

#[test]
fn osnaphatch_geometry_is_not_used_for_tracking_extension() {
    let mut s = session(mode::END | mode::EXT);
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 5] })).unwrap();
    let boundary = s.doc().unwrap().model.iter().last().unwrap().handle.hex();
    s.execute("hatch", &json!({ "handles": [boundary] })).unwrap();
    s.execute("erase", &json!({ "handles": [boundary] })).unwrap();
    setvar(&mut s, "OSNAPHATCH", json!(1));

    rest(&mut s, v(10.02, 4.98), None);

    assert_eq!(s.tracking.acquired.len(), 1);
    assert!(matches!(s.tracking.acquired.first(), Some(Acquired::Point { ext: None, .. })));
}

/// Rest the cursor at `p` long enough to acquire (or release) what is there, then move off.
fn rest(s: &mut Session, p: Vec2, base: Option<Vec2>) {
    s.snap_cursor(p, &query(base, 0.0));
    s.snap_cursor(p, &query(base, DWELL + 0.01));
    s.snap_cursor(v(1e5, 1e5), &query(base, DWELL + 0.02));
}

#[test]
fn two_acquired_points_snap_where_their_paths_cross() {
    let mut s = session(mode::END);
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    line(&mut s, (20.0, 5.0), (30.0, 5.0));
    rest(&mut s, v(10.02, 0.03), None);
    rest(&mut s, v(20.03, 5.02), None);
    let acquired: Vec<Vec2> = s.tracking.acquired.iter().map(Acquired::at).collect();
    assert_eq!(acquired, vec![v(10.0, 0.0), v(20.0, 5.0)]);
    // Vertical from (10,0) meets horizontal from (20,5).
    let r = at(&mut s, v(10.05, 4.97), None);
    let t = r.track.expect("a tracking point");
    assert!(t.point.near(v(10.0, 5.0), 1e-9), "{:?}", t.point);
    assert_eq!(t.paths.len(), 2);
    assert!(t.paths.iter().all(|p| p.kind == PathKind::Alignment && p.name == "Endpoint"));
    // Away from the crossing the cursor slides along the one path it is near.
    let t = at(&mut s, v(10.04, 2.0), None).track.unwrap();
    assert!(t.point.near(v(10.0, 2.0), 1e-9) && t.paths.len() == 1 && t.paths[0].from == v(10.0, 0.0));
    // Too far from every path: nothing.
    assert!(at(&mut s, v(13.0, 2.0), None).track.is_none());
    // OTRACK off (F11): the acquired points send no paths.
    s.execute("otrack", &json!({ "on": false })).unwrap();
    assert!(at(&mut s, v(10.05, 4.97), None).track.is_none());
}

#[test]
fn a_tracking_path_snaps_where_it_crosses_an_object() {
    let mut s = session(mode::END | mode::INT);
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    line(&mut s, (5.0, 8.0), (15.0, 8.0));
    rest(&mut s, v(10.0, 0.02), None);
    let t = at(&mut s, v(10.2, 8.1), None).track.expect("path ∩ object");
    assert!(t.point.near(v(10.0, 8.0), 1e-9), "{:?}", t.point);
    assert_eq!(t.object, Some("Intersection"));
    // Without Intersection running the path only slides.
    setvar(&mut s, "OSMODE", json!(mode::END));
    assert!(at(&mut s, v(10.2, 8.1), None).track.is_none(), "0.2 off the path is outside the tolerance");
    let t = at(&mut s, v(10.05, 8.1), None).track.unwrap();
    assert!(t.object.is_none() && t.point.near(v(10.0, 8.1), 1e-9));
}

#[test]
fn polar_angles_increment_additional_and_tracking() {
    let mut s = session(mode::END);
    setvar(&mut s, "POLARANG", json!(30));
    let base = Some(Vec2::ZERO);
    // 60° from the base point.
    let c = Vec2::from_angle(60f64.to_radians()) * 10.0 + v(0.03, -0.02);
    let t = at(&mut s, c, base).track.expect("polar");
    assert_eq!((t.paths[0].kind, t.paths[0].name), (PathKind::Polar, "Polar"));
    assert!((t.point.angle().to_degrees() - 60.0).abs() < 1e-9);
    // An additional angle tracks only with POLARMODE bit 4.
    setvar(&mut s, "POLARADDANG", json!("17"));
    let c17 = Vec2::from_angle(17f64.to_radians()) * 10.0 + v(0.0, 0.03);
    assert!(at(&mut s, c17, base).track.is_none());
    setvar(&mut s, "POLARMODE", json!(4));
    let t = at(&mut s, c17, base).track.expect("additional angle");
    assert!((t.point.angle().to_degrees() - 17.0).abs() < 1e-9);
    // Ortho on: no polar paths.
    s.execute("ortho", &json!({ "on": true })).unwrap();
    assert!(at(&mut s, c, base).track.is_none());
    s.execute("ortho", &json!({ "on": false })).unwrap();
    // Object snap tracking along polar angles needs POLARMODE bit 2; orthogonal otherwise.
    line(&mut s, (40.0, 0.0), (50.0, 0.0));
    rest(&mut s, v(50.02, 0.02), None);
    let c30 = v(50.0, 0.0) + Vec2::from_angle(30f64.to_radians()) * 5.0 + v(0.02, -0.02);
    assert!(at(&mut s, c30, None).track.is_none());
    let up = at(&mut s, v(50.03, 4.0), None).track.unwrap();
    assert!(up.point.near(v(50.0, 4.0), 1e-9));
    setvar(&mut s, "POLARMODE", json!(2));
    let t = at(&mut s, c30, None).track.expect("tracking at 30°");
    assert_eq!(t.paths[0].kind, PathKind::Alignment);
    assert!(((t.point - v(50.0, 0.0)).angle().to_degrees() - 30.0).abs() < 1e-9);
    // Measured from ANGBASE.
    let a = polar_angles(90f64.to_radians(), &[], 10f64.to_radians());
    assert_eq!(a.len(), 4);
    assert!((a[0].to_degrees() - 10.0).abs() < 1e-9 && (a[1].to_degrees() - 100.0).abs() < 1e-9);
}

#[test]
fn extension_runs_on_from_a_line_end_and_round_an_arc() {
    // Extension alone, tracking off: resting on the end acquires its extension.
    let mut s = session(mode::EXT);
    s.execute("otrack", &json!({ "on": false })).unwrap();
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    rest(&mut s, v(10.02, 0.02), None);
    assert_eq!(s.tracking.acquired.len(), 1);
    let t = at(&mut s, v(14.0, 0.05), None).track.expect("on the extension");
    assert!(t.point.near(v(14.0, 0.0), 1e-9));
    assert_eq!((t.paths[0].kind, t.paths[0].name), (PathKind::Extension, "Extension"));
    // Only beyond the end, not back along the line.
    assert!(at(&mut s, v(5.0, 0.05), None).track.is_none());
    // Direct distance entry along the extension measures from the end.
    s.cmdline("line 30,30").unwrap();
    let r = at(&mut s, v(14.0, 0.05), Some(v(30.0, 30.0)));
    s.cursor = r.point;
    s.cmdline("5").unwrap();
    s.cmdline("").unwrap();
    let EntityKind::Line(l) = s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()).unwrap() else { panic!("no line") };
    assert!(l.b.xy().near(v(15.0, 0.0), 1e-9), "{:?}", l.b);

    // An arc's extension follows its circle, outside the arc.
    let mut s = session(mode::EXT);
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    rest(&mut s, v(5.02, 0.02), None);
    let c = Vec2::from_angle(300f64.to_radians()) * 5.05;
    let t = at(&mut s, c, None).track.expect("on the arc's extension");
    assert!((t.point.len() - 5.0).abs() < 1e-9 && t.paths[0].kind == PathKind::Extension);
    assert!(at(&mut s, Vec2::from_angle(45f64.to_radians()) * 5.05, None).track.is_none(), "not on the arc itself");
}

#[test]
fn parallel_path_through_the_base_point() {
    let mut s = session(mode::PAR);
    line(&mut s, (0.0, 0.0), (10.0, 10.0));
    let base = Some(v(20.0, 0.0));
    rest(&mut s, v(5.0, 5.02), base);
    assert!(matches!(s.tracking.acquired.first(), Some(Acquired::Parallel { .. })));
    let t = at(&mut s, v(25.05, 4.98), base).track.expect("parallel");
    assert_eq!((t.paths[0].kind, t.paths[0].name), (PathKind::Parallel, "Parallel"));
    let d = t.point - v(20.0, 0.0);
    assert!(d.cross(v(1.0, 1.0)).abs() < 1e-9 && d.x > 0.0);
    // And the other way.
    let t = at(&mut s, v(15.05, -4.98), base).track.expect("parallel, backwards");
    assert!((t.point - v(20.0, 0.0)).cross(v(1.0, 1.0)).abs() < 1e-9);
    // No base point, no parallel path.
    assert!(at(&mut s, v(25.05, 4.98), None).track.is_none());
}

#[test]
fn apparent_intersection_where_extensions_meet() {
    // Two objects near the cursor that would meet if extended.
    let mut s = session(mode::APP);
    line(&mut s, (0.0, 0.0), (11.7, 0.0));
    line(&mut s, (12.0, -5.0), (12.0, -0.3));
    let r = at(&mut s, v(11.9, 0.05), None);
    let h = r.snap.expect("apparent intersection");
    assert_eq!((h.mode, h.name), (mode::APP, "Apparent Intersection"));
    assert!(h.point.near(v(12.0, 0.0), 1e-9));
    // Intersection alone needs them to really cross.
    assert!(snap::osnap(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, v(11.9, 0.05), APERTURE, mode::INT, None, false).is_none());

    // A far object rested on first: its extension meets the one under the cursor.
    let mut s = session(mode::APP);
    line(&mut s, (0.0, 0.0), (5.0, 0.0));
    line(&mut s, (20.0, -5.0), (20.0, 5.0));
    rest(&mut s, v(2.0, 0.05), None);
    assert!(matches!(s.tracking.acquired.first(), Some(Acquired::Apparent { .. })));
    let t = at(&mut s, v(20.1, 0.1), None).track.expect("extended apparent intersection");
    assert!(t.point.near(v(20.0, 0.0), 1e-9));
    assert_eq!(t.object, Some("Apparent Intersection"));
    // Its extension alone doesn't pull the cursor.
    assert!(at(&mut s, v(30.0, 0.05), None).track.is_none());
}

#[test]
fn resting_again_releases_and_seven_points_at_most() {
    let mut s = session(mode::END);
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let p = v(10.02, 0.02);
    // Not before the dwell time.
    s.snap_cursor(p, &query(None, 0.0));
    let r = s.snap_cursor(p, &query(None, DWELL / 2.0));
    assert!(r.wait && s.tracking.acquired.is_empty());
    s.snap_cursor(p, &query(None, DWELL + 0.01));
    assert_eq!(s.tracking.acquired.len(), 1);
    // Staying on it doesn't release it.
    s.snap_cursor(p, &query(None, 5.0));
    s.snap_cursor(p, &query(None, 10.0));
    assert_eq!(s.tracking.acquired.len(), 1);
    // Leaving and resting on it again does.
    s.snap_cursor(v(1e5, 1e5), &query(None, 11.0));
    rest(&mut s, p, None);
    assert!(s.tracking.acquired.is_empty());
    // At most seven, the oldest dropped.
    for i in 0..10 {
        s.tracking.toggle(Acquired::Point { at: v(f64::from(i), 0.0), name: "Endpoint", ext: None });
    }
    assert_eq!(s.tracking.acquired.len(), MAX_ACQUIRED);
    assert_eq!(s.tracking.acquired[0].at(), v(3.0, 0.0));
    s.tracking.clear();
    assert!(s.tracking.acquired.is_empty());
}

#[test]
fn shift_to_acquire() {
    let mut s = session(mode::END);
    setvar(&mut s, "POLARMODE", json!(8));
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let p = v(10.02, 0.02);
    s.snap_cursor(p, &query(None, 0.0));
    s.snap_cursor(p, &query(None, 5.0));
    assert!(s.tracking.acquired.is_empty(), "resting doesn't acquire");
    s.snap_cursor(p, &CursorQuery { shift: true, ..query(None, 5.1) });
    assert_eq!(s.tracking.acquired.len(), 1);
}

#[test]
fn shift_overrides_ortho_while_held() {
    let mut s = session(0);
    let base = Some(v(0.0, 0.0));
    let held = |base| CursorQuery { shift: true, ..query(base, 100.0) };
    // Ortho off: Shift turns it on.
    assert_eq!(at(&mut s, v(5.0, 1.0), base).point, v(5.0, 1.0));
    assert_eq!(s.snap_cursor(v(5.0, 1.0), &held(base)).point, v(5.0, 0.0));
    // Ortho on: Shift turns it off.
    s.execute("ortho", &json!({ "on": true })).unwrap();
    assert_eq!(at(&mut s, v(5.0, 1.0), base).point, v(5.0, 0.0));
    assert_eq!(s.snap_cursor(v(5.0, 1.0), &held(base)).point, v(5.0, 1.0));
    assert!(s.settings.orthomode, "the override leaves ORTHOMODE as it was");
    // No base point, or a window's first corner: nothing to constrain.
    assert_eq!(s.snap_cursor(v(5.0, 1.0), &held(None)).point, v(5.0, 1.0));
    s.execute("ortho", &json!({ "on": false })).unwrap();
    let corner = CursorQuery { corner: true, ..held(base) };
    assert_eq!(s.snap_cursor(v(5.0, 1.0), &corner).point, v(5.0, 1.0));
}

#[test]
fn shift_ortho_override_wins_over_polar() {
    let mut s = session(0);
    s.execute("polar", &json!({ "on": true })).unwrap();
    setvar(&mut s, "POLARANG", json!(45));
    let base = Some(v(0.0, 0.0));
    // Near the 45° polar path the cursor follows it; with Shift held ortho does instead.
    let free = at(&mut s, v(5.0, 5.05), base);
    assert!(free.track.is_some() && free.point.near(v(5.025, 5.025), 1e-9), "{free:?}");
    let held = s.snap_cursor(v(5.0, 5.05), &CursorQuery { shift: true, ..query(base, 100.0) });
    assert_eq!((held.point, held.track), (v(0.0, 5.05), None));
}

#[test]
fn sysvars_polarmode_polaraddang_autosnap() {
    let mut s = Session::new();
    let get = |s: &Session, n: &str| cadcraft_engine::sysvars::get(s, n).unwrap();
    setvar(&mut s, "POLARMODE", json!(6));
    assert_eq!(get(&s, "POLARMODE"), json!(6));
    setvar(&mut s, "POLARADDANG", json!("15;22.5"));
    assert_eq!(get(&s, "POLARADDANG"), json!("15;22.5"));
    assert_eq!(get(&s, "AUTOSNAP"), json!(61));
    s.execute("otrack", &json!({})).unwrap();
    assert!(!s.settings.otrack, "F11 toggles tracking");
    assert_eq!(get(&s, "AUTOSNAP"), json!(45));
    setvar(&mut s, "AUTOSNAP", json!(63));
    assert!(s.settings.otrack && s.settings.polarmode);
    for (n, bad) in [
        ("POLARMODE", json!(16)),
        ("POLARMODE", json!(-1)),
        ("POLARADDANG", json!("abc")),
        ("POLARADDANG", json!("1;2;3;4;5;6;7;8;9;10;11")),
        ("AUTOSNAP", json!(99)),
    ] {
        assert!(s.execute("setvar", &json!({ "name": n, "value": bad })).is_err(), "{n} = {bad}");
    }
}

#[test]
fn hostile_inputs_never_panic() {
    let mut s = session(mode::END | mode::INT | mode::EXT | mode::PAR | mode::APP);
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    s.execute("circle", &json!({ "center": [5, 5], "radius": 3 })).unwrap();
    setvar(&mut s, "POLARANG", json!(0.0001));
    setvar(&mut s, "POLARMODE", json!(15));
    setvar(&mut s, "POLARADDANG", json!("1e300;-5;0"));
    for i in 0..9 {
        let a = [f64::NAN, f64::INFINITY, 1e300, -1e300, 0.0, 1e-300, 5.0, 10.0, -0.0][i];
        s.tracking.toggle(Acquired::Point { at: v(a, a), name: "Endpoint", ext: None });
    }
    rest(&mut s, v(10.0, 0.0), Some(v(1.0, 1.0)));
    rest(&mut s, v(5.0, 8.0), Some(v(1.0, 1.0)));
    let nums = [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e308, -1e308, 0.0, 1e-300];
    for x in nums {
        for ap in [APERTURE, 0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
            for base in [None, Some(v(x, 0.0)), Some(Vec2::ZERO)] {
                let q = CursorQuery { base, deferred: true, corner: false, aperture: ap, tol: ap, now: x, shift: x.is_nan() };
                let _ = s.snap_cursor(v(x, 0.5), &q);
                let r = s.snap_cursor(v(10.0, x), &q);
                assert!(r.track.is_none_or(|t| t.point.is_finite()));
            }
        }
    }
    assert!(polar_angles(0.0, &[], 0.0).len() == 1);
    assert!(polar_angles(1e-9, &[f64::NAN], f64::NAN).len() <= 360);
    assert!(polar_angles(1e-4, &[], 0.0).len() <= 360);
}
