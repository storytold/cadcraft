//! Temporary tracking and polar options (#441): the `TT` temporary tracking point and the `TK`
//! tracking modifier typed at any point prompt, PolarSnap, polar angles relative to the last
//! segment and Extension on elliptical arcs and splines, all driven without the UI.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap;
use cadcraft_engine::snap::tracking::{Acquired, CursorQuery, DWELL, PathKind, PathShape};
use serde_json::json;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// The last line drawn as (start, end).
fn last_line(s: &Session) -> (Vec2, Vec2) {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a.xy(), l.b.xy())) } else { None }).last().unwrap()
}

fn near(p: Vec2, x: f64, y: f64) -> bool {
    p.near(v(x, y), 1e-9)
}

/// Move the cursor to `raw` as the canvas does: snap it and remember where it ended up.
fn hover(s: &mut Session, raw: Vec2, base: Option<Vec2>) -> Vec2 {
    let q = CursorQuery { base, deferred: false, corner: false, aperture: 0.5, tol: 0.1, now: 100.0, shift: false };
    let r = s.snap_cursor(raw, &q);
    s.cursor = r.point;
    r.point
}

#[test]
fn tt_acquires_a_point_that_tracks_until_the_next_point() {
    let mut s = Session::new();
    // Even with object snap tracking off, a temporary tracking point sends its paths.
    s.execute("otrack", &json!({ "on": false })).unwrap();
    s.cmdline("line tt").unwrap();
    assert!(s.prompt_text().ends_with("Specify temporary OTRACK point:"), "{}", s.prompt_text());
    s.cmdline("5,5").unwrap();
    assert!(s.prompt_text().ends_with("Specify first point:"), "{}", s.prompt_text());
    assert_eq!(s.tracking.acquired, vec![Acquired::Temporary { at: v(5.0, 5.0) }]);
    // The cursor slides along the horizontal path from 5,5 …
    let p = hover(&mut s, v(8.0, 5.04), None);
    assert!(near(p, 8.0, 5.0), "{p:?}");
    let t = s.tracking.last.clone().unwrap();
    assert_eq!((t.paths[0].kind, t.paths[0].name), (PathKind::Alignment, "Temporary track point"));
    // … and a distance typed there is measured from the tracking point, with no base point.
    s.cmdline("4").unwrap();
    assert!(near(s.last_point, 9.0, 5.0), "{:?}", s.last_point);
    assert!(s.tracking.acquired.is_empty(), "released once the command took a point");
    s.cmdline("@0,3").unwrap();
    s.cmdline("").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 9.0, 5.0) && near(b, 9.0, 8.0), "{a:?} {b:?}");
}

#[test]
fn tt_point_is_the_last_point_and_takes_snap_overrides() {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0\n\n").unwrap();
    s.cmdline("line 0,5 _tt end 9.97,0.04").unwrap();
    assert_eq!(s.tracking.acquired, vec![Acquired::Temporary { at: v(10.0, 0.0) }]);
    // `@` is measured from the temporary point.
    s.cmdline("@0,2").unwrap();
    s.cmdline("").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 0.0, 5.0) && near(b, 10.0, 2.0), "{a:?} {b:?}");
}

#[test]
fn tk_runs_orthogonal_tracking_points_until_enter() {
    let mut s = Session::new();
    s.cmdline("line 0,0 tk").unwrap();
    assert!(s.prompt_text().ends_with("First tracking point:"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    assert!(s.prompt_text().ends_with("Next point (Press ENTER to end tracking):"), "{}", s.prompt_text());
    assert_eq!(s.current_prompt().and_then(|p| p.base), Some(v(10.0, 0.0)));
    // Picked off-axis, the point goes straight up from the last tracking point.
    s.cmdline("12,3").unwrap();
    assert!(near(s.last_point, 10.0, 3.0), "{:?}", s.last_point);
    s.cmdline("@5,0").unwrap();
    // A distance follows the cursor's axis from the last tracking point.
    s.cursor = v(15.2, 9.0);
    s.cmdline("2").unwrap();
    assert!(near(s.last_point, 15.0, 5.0), "{:?}", s.last_point);
    // Enter ends tracking: the line runs to the last tracking point.
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("Specify next point or [Undo]:"), "{}", s.prompt_text());
    let (a, b) = last_line(&s);
    assert!(near(a, 0.0, 0.0) && near(b, 15.0, 5.0), "{a:?} {b:?}");
}

#[test]
fn tk_takes_snaps_and_nests_under_from() {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0\n\n").unwrap();
    // FROM, base point by tracking from the snapped endpoint, then the offset.
    s.cmdline("line from tracking endp 9.96,0.03 @0,4").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("<Offset>:"), "{}", s.prompt_text());
    s.cmdline("@1,1").unwrap();
    s.cmdline("@1,0").unwrap();
    s.cmdline("").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 11.0, 5.0) && near(b, 12.0, 5.0), "{a:?} {b:?}");
}

#[test]
fn enter_at_the_first_tracking_point_and_hostile_input_are_harmless() {
    let mut s = Session::new();
    s.cmdline("line tk").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("Specify first point:"), "{}", s.prompt_text());
    for _ in 0..40 {
        s.cmdline("tk").unwrap();
        s.cmdline("tt").unwrap();
    }
    s.cmdline("nan,1").unwrap();
    s.cmdline("1e308").unwrap();
    s.cancel();
    assert!(s.running.is_none());
    assert!(s.tracking.acquired.len() <= cadcraft_engine::snap::tracking::MAX_ACQUIRED);
}

// PolarSnap, polar angles relative to the last segment, Extension on ellipse and spline ends.

fn setvar(s: &mut Session, name: &str, value: serde_json::Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

/// Rest the cursor on `p` long enough to acquire what is there, then move off.
fn rest(s: &mut Session, p: Vec2) {
    for (at, now) in [(p, 0.0), (p, DWELL + 0.01), (v(1e5, 1e5), DWELL + 0.02)] {
        s.snap_cursor(at, &CursorQuery { base: None, deferred: false, corner: false, aperture: 0.5, tol: 0.1, now, shift: false });
    }
}

#[test]
fn polarsnap_steps_along_polar_paths_by_polardist() {
    let mut s = Session::new();
    setvar(&mut s, "OSMODE", json!(0));
    setvar(&mut s, "SNAPMODE", json!(1));
    setvar(&mut s, "SNAPTYPE", json!(1));
    setvar(&mut s, "POLARDIST", json!(2.5));
    assert_eq!(s.execute("getvar", &json!({ "name": "SNAPTYPE" })).unwrap()["value"], json!(1));
    s.cmdline("line 0,0").unwrap();
    let base = Some(v(0.0, 0.0));
    assert!(near(hover(&mut s, v(6.1, 0.03), base), 5.0, 0.0));
    assert!(near(hover(&mut s, v(6.4, -0.04), base), 7.5, 0.0));
    assert!(near(hover(&mut s, v(0.02, -8.9), base), 0.0, -10.0));
    // A distance typed there still runs along the path.
    s.cmdline("3").unwrap();
    assert!(near(s.last_point, 0.0, -3.0), "{:?}", s.last_point);
    // Off every path the cursor is free: PolarSnap replaces the grid snap.
    assert!(near(hover(&mut s, v(3.3, 2.2), Some(v(0.0, -3.0))), 3.3, 2.2));
    // POLARDIST 0: steps of the snap X spacing.
    setvar(&mut s, "POLARDIST", json!(0));
    setvar(&mut s, "SNAPUNIT", json!([0.4, 0.4]));
    assert!(near(hover(&mut s, v(2.9, -3.03), Some(v(0.0, -3.0))), 2.8, -3.0));
    // Grid snap again with SNAPTYPE 0.
    setvar(&mut s, "SNAPTYPE", json!(0));
    assert!(near(hover(&mut s, v(3.3, 2.1), Some(v(0.0, -3.0))), 3.2, 2.0));
    for bad in [json!(2), json!(-1), json!("x")] {
        assert!(s.execute("setvar", &json!({ "name": "SNAPTYPE", "value": bad })).is_err());
    }
    assert!(s.execute("setvar", &json!({ "name": "POLARDIST", "value": -1 })).is_err());
}

#[test]
fn polar_angles_relative_to_the_last_segment() {
    let mut s = Session::new();
    setvar(&mut s, "OSMODE", json!(0));
    s.cmdline("line 0,0 10,10").unwrap();
    let base = Some(v(10.0, 10.0));
    // Absolute: 45° is no polar angle with the 90° increment.
    hover(&mut s, v(15.0, 15.05), base);
    assert!(s.tracking.last.is_none());
    setvar(&mut s, "POLARMODE", json!(1));
    // Relative: straight on and at right angles to the 45° segment.
    let p = hover(&mut s, v(15.0, 15.05), base);
    assert!(near(p, 15.025, 15.025), "{p:?}");
    assert_eq!(s.tracking.last.as_ref().unwrap().paths[0].kind, PathKind::Polar);
    let p = hover(&mut s, v(7.0, 13.04), base);
    assert!(near(p, 6.98, 13.02), "{p:?}");
    // Elsewhere than the end of the last segment the angles are absolute again.
    let p = hover(&mut s, v(25.0, 0.03), Some(v(20.0, 0.0)));
    assert!(near(p, 25.0, 0.0), "{p:?}");
}

#[test]
fn extension_runs_around_an_elliptical_arc_and_off_a_spline_end() {
    let mut s = Session::new();
    setvar(&mut s, "OSMODE", json!(snap::mode::END | snap::mode::EXT));
    // A quarter ellipse from 10,0 to 0,5.
    s.execute("ellipse", &json!({ "center": [0, 0], "major": [10, 0], "ratio": 0.5, "start": 0, "end": 90 })).unwrap();
    rest(&mut s, v(0.02, 5.02));
    let a = 135f64.to_radians();
    let p = hover(&mut s, v(10.0 * a.cos(), 5.0 * a.sin() + 0.05), None);
    let t = s.tracking.last.clone().expect("on the ellipse's extension");
    assert_eq!(t.paths[0].kind, PathKind::Extension);
    assert!(matches!(t.paths[0].shape, PathShape::Ellipse { .. }));
    assert!(((p.x / 10.0).powi(2) + (p.y / 5.0).powi(2) - 1.0).abs() < 1e-3, "{p:?}");
    // Not along the arc itself.
    let a = 45f64.to_radians();
    hover(&mut s, v(10.0 * a.cos(), 5.0 * a.sin() + 0.05), None);
    assert!(s.tracking.last.is_none());

    // An open spline runs on along its end tangent (here 45° down from 15,0).
    let mut s = Session::new();
    setvar(&mut s, "OSMODE", json!(snap::mode::END | snap::mode::EXT));
    s.execute("spline", &json!({ "control": [[0, 0], [5, 5], [10, 5], [15, 0]], "degree": 3 })).unwrap();
    rest(&mut s, v(15.02, 0.02));
    let p = hover(&mut s, v(18.0, -2.96), None);
    let t = s.tracking.last.clone().expect("on the spline's extension");
    assert_eq!((t.paths[0].kind, t.paths[0].from), (PathKind::Extension, v(15.0, 0.0)));
    assert!(near(p, 17.98, -2.98), "{p:?}");
    // Hostile cursors near the acquired end.
    for x in [f64::NAN, f64::INFINITY, -1e308, 1e-300] {
        let r = s.snap_cursor(
            v(15.0 + x, x),
            &CursorQuery { base: Some(v(x, 0.0)), deferred: false, corner: false, aperture: 0.5, tol: 0.1, now: x, shift: false },
        );
        assert!(r.track.is_none_or(|t| t.point.is_finite()));
    }
}
