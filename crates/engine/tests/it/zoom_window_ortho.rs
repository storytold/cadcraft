//! ZOOM Window's opposite corner ignores ortho: the window stays a rectangle (#787).

use cadcraft_engine::Session;
use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_geom::Vec2;
use serde_json::json;

/// Move the cursor to `raw` at the current prompt, as the canvas does.
fn hover(s: &mut Session, raw: Vec2) -> Vec2 {
    let p = s.current_prompt().unwrap();
    let q = CursorQuery { base: p.base, deferred: p.deferred, corner: p.corner, aperture: 1e-6, tol: 1e-6, now: 0.0, shift: false };
    s.snap_cursor(raw, &q).point
}

#[test]
fn zoom_window_corner_ignores_ortho() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
    s.execute("setvar", &json!({ "name": "ORTHOMODE", "value": 1 })).unwrap();
    s.cmdline("zoom w 0,0").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify opposite corner");
    let c = hover(&mut s, Vec2::new(10.0, 4.0));
    assert!(c.near(Vec2::new(10.0, 4.0), 1e-9), "the corner is where the cursor is: {c:?}");
    // A typed distance goes toward the cursor too, not along the nearest axis.
    s.cursor = c;
    s.cmdline("5").unwrap();
    let far = Vec2::new(10.0, 4.0).normalized() * 5.0;
    let v = s.state().unwrap().view();
    assert!(v.center.near(far * 0.5, 1e-9), "the window is 0,0 to {far:?}: {v:?}");

    // A line's rubber band still follows ortho.
    s.cmdline("line 0,0").unwrap();
    let c = hover(&mut s, Vec2::new(10.0, 4.0));
    assert!(c.near(Vec2::new(10.0, 0.0), 1e-9), "ortho keeps the line horizontal: {c:?}");
}
