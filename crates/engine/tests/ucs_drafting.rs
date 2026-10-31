//! The drafting aids follow the current UCS (#1106): ortho and direct distance entry along its
//! axes, grid snap in its XY plane (SNAPBASE from its origin, SNAPANG from its X axis), polar and
//! object snap tracking measured from its X axis, isometric axes turned with it, and DIMLINEAR's
//! horizontal and vertical along its axes. In the World UCS everything is as before.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimKind, EntityKind};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::snap::tracking::{CursorQuery, TrackConfig};
use cadcraft_engine::snap::{self, iso};
use serde_json::json;

const DEG: f64 = std::f64::consts::PI / 180.0;

fn session() -> Session {
    let mut s = Session::new();
    s.new_drawing(false);
    // Nothing running that would snap the cursor elsewhere.
    s.settings.osmode = 0;
    s.settings.otrack = false;
    s.settings.polarmode = false;
    s.settings.orthomode = false;
    s.settings.snapmode = false;
    s
}

/// The UCS at (5, 5) turned `deg` degrees.
fn turned(s: &mut Session, deg: f64) {
    s.execute("ucs", &json!({"option": "origin", "origin": [5, 5]})).unwrap();
    s.execute("ucs", &json!({"option": "z", "angle": deg})).unwrap();
}

fn query(base: Option<Vec2>) -> CursorQuery {
    CursorQuery { base, deferred: false, corner: false, aperture: 0.01, tol: 1.0, now: 0.0, shift: false }
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    a.dist(Vec2::new(x, y)) < 1e-9
}

fn ang_eq(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(std::f64::consts::TAU);
    d < 1e-9 || std::f64::consts::TAU - d < 1e-9
}

#[test]
fn ortho_follows_the_ucs_axes() {
    let mut s = session();
    s.settings.orthomode = true;
    // World UCS: horizontal.
    let p = s.snap_cursor(Vec2::new(10.0, 4.0), &query(Some(Vec2::ZERO))).point;
    assert!(near(p, 10.0, 0.0), "{p:?}");
    // Turned 30°: along the UCS X axis, as far as the cursor reaches along it.
    turned(&mut s, 30.0);
    let p = s.snap_cursor(Vec2::new(10.0, 4.0), &query(Some(Vec2::ZERO))).point;
    let along = 10.0 * (30.0 * DEG).cos() + 4.0 * (30.0 * DEG).sin();
    assert!(near(p, along * (30.0 * DEG).cos(), along * (30.0 * DEG).sin()), "{p:?}");
    // And along its Y axis when the cursor is nearer that.
    let p = s.snap_cursor(Vec2::new(-4.0, 9.0), &query(Some(Vec2::ZERO))).point;
    let d = p - Vec2::ZERO;
    assert!(ang_eq(d.angle(), 120.0 * DEG) || ang_eq(d.angle(), 300.0 * DEG), "{:?}", d.angle() / DEG);
}

/// Holding Shift turns ortho on (or off) while it is held, along the current UCS's axes too.
#[test]
fn shift_ortho_follows_the_ucs_axes() {
    let mut s = session();
    s.settings.orthomode = false;
    turned(&mut s, 30.0);
    let shifted = CursorQuery { shift: true, ..query(Some(Vec2::ZERO)) };
    let p = s.snap_cursor(Vec2::new(10.0, 4.0), &shifted).point;
    let along = 10.0 * (30.0 * DEG).cos() + 4.0 * (30.0 * DEG).sin();
    assert!(near(p, along * (30.0 * DEG).cos(), along * (30.0 * DEG).sin()), "Shift: ortho on, along the UCS X axis: {p:?}");
    // Without Shift the cursor goes where it is; with ORTHOMODE on, Shift turns it off.
    assert!(near(s.snap_cursor(Vec2::new(10.0, 4.0), &query(Some(Vec2::ZERO))).point, 10.0, 4.0));
    s.settings.orthomode = true;
    assert!(near(s.snap_cursor(Vec2::new(10.0, 4.0), &shifted).point, 10.0, 4.0), "Shift: ortho off");
    assert!(s.settings.orthomode, "ORTHOMODE itself is unchanged");
}

#[test]
fn direct_distance_with_ortho_goes_along_the_ucs_axis() {
    let mut s = session();
    turned(&mut s, 30.0);
    s.settings.orthomode = true;
    s.cmdline("line 0,0").unwrap();
    // The cursor a little off the UCS X axis: 10 along it.
    s.cursor = s.input_ucs().to_wcs(Vec2::new(8.0, 1.0));
    s.cmdline("10").unwrap();
    s.cmdline("").unwrap();
    let l = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).unwrap();
    let u = s.input_ucs();
    assert!(l.a.xy().dist(u.to_wcs(Vec2::ZERO)) < 1e-9 && l.b.xy().dist(u.to_wcs(Vec2::new(10.0, 0.0))) < 1e-9, "{l:?}");
}

#[test]
fn grid_snap_lies_in_the_ucs() {
    let mut s = session();
    cadcraft_engine::sysvars::set(&mut s, "SNAPUNIT", &json!([1, 1])).unwrap();
    s.settings.snapmode = true;
    let p = s.snap_cursor(Vec2::new(5.3, 6.8), &query(None)).point;
    assert!(near(p, 5.0, 7.0), "World: {p:?}");
    // Turned 90° at (5, 5): the lattice through (5, 5) along the turned axes; with SNAPBASE and
    // SNAPANG measured in the UCS.
    turned(&mut s, 90.0);
    let p = s.snap_cursor(Vec2::new(5.3, 6.8), &query(None)).point;
    assert!(near(p, 5.0, 7.0), "{p:?}");
    s.execute("ucs", &json!({"option": "z", "angle": -45})).unwrap();
    let u = s.input_ucs();
    let p = s.snap_cursor(u.to_wcs(Vec2::new(2.2, -0.9)), &query(None)).point;
    assert!(u.to_ucs(p).dist(Vec2::new(2.0, -1.0)) < 1e-9, "UCS {:?}", u.to_ucs(p));
    let d = s.doc().unwrap();
    let (o, a) = snap::grid_frame_in(d, &s.space());
    assert!(near(o, 5.0, 5.0) && ang_eq(a, 45.0 * DEG), "{o:?} {}", a / DEG);
    // SNAPBASE is a UCS point, SNAPANG an angle from its X axis.
    cadcraft_engine::sysvars::set(&mut s, "SNAPBASE", &json!([1, 0])).unwrap_or(());
    s.doc_mut().unwrap().header.set("SNAPBASE", cadcraft_engine::doc::HVal::Point(Vec3::new(1.0, 0.0, 0.0)));
    s.doc_mut().unwrap().header.set_f64("SNAPANG", 10.0);
    let (o, a) = snap::grid_frame_in(s.doc().unwrap(), &s.space());
    assert!(o.dist(u.to_wcs(Vec2::new(1.0, 0.0))) < 1e-9 && ang_eq(a, 55.0 * DEG), "{o:?} {}", a / DEG);
    // The stored values are unchanged (they are UCS values).
    assert_eq!(snap::grid_frame(s.doc().unwrap()), (Vec2::new(1.0, 0.0), 10.0 * DEG));
}

#[test]
fn polar_tracking_is_measured_from_the_ucs_x_axis() {
    let mut s = session();
    s.settings.polarmode = true;
    s.settings.polarang = 90.0 * DEG;
    let base = Vec2::ZERO;
    let at32 = Vec2::from_angle(32.0 * DEG) * 10.0;
    // World UCS, 90° steps: 32° is on no path.
    assert!(s.snap_cursor(at32, &query(Some(base))).track.is_none());
    // Turned 30°: the path at 30° catches it.
    turned(&mut s, 30.0);
    let snapped = s.snap_cursor(at32, &query(Some(base)));
    let t = snapped.track.expect("on the 30° path");
    let d = t.point - base;
    assert!(ang_eq(d.angle(), 30.0 * DEG), "{}", d.angle() / DEG);
    // The configuration itself: polar angles and the orthogonal tracking paths turn with the UCS.
    let st = s.drafting_settings();
    let cfg = TrackConfig::new(&st, 0.0);
    for want in [30.0, 120.0, 210.0, 300.0] {
        assert!(
            cfg.polar_angles.iter().any(|a| ang_eq(*a, want * DEG)),
            "{want}: {:?}",
            cfg.polar_angles.iter().map(|a| a / DEG).collect::<Vec<_>>()
        );
        assert!(cfg.track_angles.iter().any(|a| ang_eq(*a, want * DEG)), "{want}");
    }
    // ANGBASE adds to it.
    let cfg = TrackConfig::new(&st, 15.0 * DEG);
    assert!(cfg.polar_angles.iter().any(|a| ang_eq(*a, 45.0 * DEG)));
}

#[test]
fn the_world_ucs_drafts_as_before() {
    let s = session();
    let st = s.drafting_settings();
    assert_eq!(st.ucs_angle, 0.0);
    for (b, p) in [(Vec2::ZERO, Vec2::new(10.0, 4.0)), (Vec2::new(3.0, 3.0), Vec2::new(1.0, 20.0)), (Vec2::ZERO, Vec2::new(-5.0, 5.0))] {
        assert_eq!(snap::ortho_at(b, p, 0.0), snap::ortho(b, p));
        assert_eq!(iso::constrain(&st, b, p), snap::ortho(b, p));
    }
    let cfg = TrackConfig::new(&st, 0.0);
    for want in [0.0, 90.0, 180.0, 270.0] {
        assert!(cfg.track_angles.iter().any(|a| ang_eq(*a, want * DEG)));
    }
    assert_eq!(snap::grid_frame_in(s.doc().unwrap(), &s.space()), snap::grid_frame(s.doc().unwrap()));
    // Hostile angles fall back to the world axes.
    assert_eq!(snap::ortho_at(Vec2::ZERO, Vec2::new(3.0, 1.0), f64::NAN), Vec2::new(3.0, 0.0));
    assert_eq!(snap::ortho_at(Vec2::ZERO, Vec2::new(3.0, 1.0), f64::INFINITY), Vec2::new(3.0, 0.0));
}

#[test]
fn isometric_axes_turn_with_the_ucs() {
    let mut s = session();
    s.settings.isodraft = true;
    let st0 = s.drafting_settings();
    turned(&mut s, 40.0);
    let st = s.drafting_settings();
    for v in [Vec2::new(10.0, 3.0), Vec2::new(-2.0, 9.0), Vec2::new(4.0, -7.0)] {
        let base = Vec2::new(1.0, 2.0);
        let r = iso::constrain(&st, base, base + v.rotate(40.0 * DEG)) - base;
        let want = iso::constrain(&st0, Vec2::ZERO, v).rotate(40.0 * DEG);
        assert!(r.dist(want) < 1e-9, "{v:?}: {r:?} vs {want:?}");
    }
}

fn last_dim_rotation(s: &Session) -> f64 {
    let d = s.doc().unwrap();
    let r = d.model.iter().filter_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.kind) } else { None }).last();
    match r {
        Some(DimKind::Linear { rotation }) => rotation,
        other => panic!("not a linear dimension: {other:?}"),
    }
}

#[test]
fn dimlinear_horizontal_and_vertical_follow_the_ucs() {
    let mut s = session();
    turned(&mut s, 30.0);
    // Placed above the points (in the UCS): horizontal along the UCS X axis.
    s.cmdline("dimlinear 0,0 10,0 5,5").unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 30.0 * DEG), "{}", last_dim_rotation(&s) / DEG);
    // Beside them: vertical along its Y axis.
    s.cmdline("dimlinear 0,0 0,10 5,5").unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 120.0 * DEG), "{}", last_dim_rotation(&s) / DEG);
    // The keywords.
    s.cmdline("dimlinear 0,0 10,10 h 5,20").unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 30.0 * DEG));
    s.cmdline("dimlinear 0,0 10,10 v 20,5").unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 120.0 * DEG));
    // JSON stays in world coordinates: horizontal is world horizontal.
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 5]})).unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 0.0));
    // World UCS: as before.
    s.execute("ucs", &json!({"option": "world"})).unwrap();
    s.cmdline("dimlinear 0,0 10,0 5,5").unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 0.0));
    s.cmdline("dimlinear 0,0 0,10 5,5").unwrap();
    assert!(ang_eq(last_dim_rotation(&s), 90.0 * DEG));
}
