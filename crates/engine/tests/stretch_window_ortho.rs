//! STRETCH's crossing window ignores ortho: the opposite corner is where the cursor is, so the
//! window keeps its area and catches what it crosses. The same holds for every rectangle picked by
//! two corners (PLOTWINDOW, MVIEW, -VIEW Window, REVCLOUD Rectangular).

use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_engine::{Input, Session};
use cadcraft_geom::Vec2;
use serde_json::json;

/// Move the cursor to `raw` at the current prompt, as the canvas does.
fn hover(s: &mut Session, raw: Vec2) -> Vec2 {
    let p = s.current_prompt().unwrap();
    let q = CursorQuery { base: p.base, deferred: p.deferred, corner: p.corner, aperture: 1e-6, tol: 1e-6, now: 0.0, shift: false };
    let c = s.snap_cursor(raw, &q).point;
    s.cursor = c;
    c
}

/// A session with ortho on, object snaps off and a line from 0,0 to 10,0.
fn ortho_session() -> Session {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
    s.execute("setvar", &json!({ "name": "ORTHOMODE", "value": 1 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s
}

/// Pick the window's opposite corner at `to`, stretch by 5,0 and return the extents.
fn stretch_from_corner(mut s: Session, to: Vec2) -> (Vec2, Vec2) {
    assert_eq!(s.current_prompt().unwrap().message, "Specify opposite corner");
    let c = hover(&mut s, to);
    assert!(c.near(to, 1e-9), "the corner is where the cursor is: {c:?}");
    s.input(Input::Point(c)).unwrap();
    s.input(Input::Enter).unwrap();
    for t in ["0,0", "5,0"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.current_prompt().is_none(), "STRETCH is done, still at {:?}", s.current_prompt().map(|p| p.message));
    let b = s.doc().unwrap().extents(&s.space());
    (b.min, b.max)
}

#[test]
fn stretch_implied_window_ignores_ortho() {
    let mut s = ortho_session();
    s.cmdline("stretch").unwrap();
    // Right to left: a crossing window.
    s.input(Input::Point(Vec2::new(12.0, 1.0))).unwrap();
    assert_eq!(stretch_from_corner(s, Vec2::new(8.0, -1.0)), (Vec2::ZERO, Vec2::new(15.0, 0.0)), "the line's end moved by 5");
}

#[test]
fn stretch_crossing_mode_ignores_ortho() {
    let mut s = ortho_session();
    for t in ["stretch", "c", "8,-1"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(stretch_from_corner(s, Vec2::new(12.0, 1.0)), (Vec2::ZERO, Vec2::new(15.0, 0.0)), "the line's end moved by 5");
}

#[test]
fn rectangle_corner_prompts_ignore_ortho() {
    for script in [&["plotwindow", "0,0"][..], &["mview", "0,0"], &["-view", "w", "v1", "0,0"], &["revcloud", "r", "0,0"]] {
        let mut s = Session::new();
        s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
        s.execute("setvar", &json!({ "name": "ORTHOMODE", "value": 1 })).unwrap();
        if script[0] == "mview" {
            s.cmdline("layout s Layout1").unwrap();
        }
        for t in script {
            s.cmdline(t).unwrap();
        }
        let p = s.current_prompt().unwrap();
        assert_eq!(p.message, "Specify opposite corner", "{script:?}");
        assert!(p.corner, "{script:?}: the opposite corner ignores ortho");
        let c = hover(&mut s, Vec2::new(10.0, 4.0));
        assert!(c.near(Vec2::new(10.0, 4.0), 1e-9), "{script:?}: the corner is where the cursor is: {c:?}");
    }
}
