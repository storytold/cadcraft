//! The "other corner" of RECTANG and of ARRAYRECT's unit cell ignores ortho: the rectangle keeps
//! its area (#1113).

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

/// Draw a rectangle from 0,0 to a click at 10,3 and return its extents and the preview's.
fn click_rectangle(ortho: bool) -> ((Vec2, Vec2), (Vec2, Vec2)) {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
    s.execute("setvar", &json!({ "name": "ORTHOMODE", "value": ortho as i32 })).unwrap();
    s.cmdline("rectang 0,0").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify other corner point");
    let c = hover(&mut s, Vec2::new(10.0, 3.0));
    let mut pv = cadcraft_geom::Bounds2::EMPTY;
    for e in s.preview(c) {
        pv = pv.union(&cadcraft_doc::entity_bounds(s.doc().unwrap(), &e, 0));
    }
    s.input(Input::Point(c)).unwrap();
    assert!(s.current_prompt().is_none(), "RECTANG is done, still at {:?}", s.current_prompt().map(|p| p.message));
    let b = s.doc().unwrap().extents(&s.space());
    ((b.min, b.max), (pv.min, pv.max))
}

#[test]
fn rectang_other_corner_ignores_ortho() {
    let want = (Vec2::ZERO, Vec2::new(10.0, 3.0));
    let on = click_rectangle(true);
    assert_eq!(on.0, want, "the rectangle is 10 by 3 with ortho on");
    assert_eq!(on.1, want, "the preview is 10 by 3 with ortho on");
    assert_eq!(click_rectangle(false), on, "ortho changes nothing");
}

/// ARRAYRECT's unit cell from 0,0 to a click at 4,2 on a 2 by 2 array of a circle: the extents.
fn click_unit_cell(ortho: bool) -> (Vec2, Vec2) {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
    s.execute("setvar", &json!({ "name": "ORTHOMODE", "value": ortho as i32 })).unwrap();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 0.5 })).unwrap();
    for t in ["arrayrect", "l", "", "cou", "2", "2", "s", "u", "0,0"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(s.current_prompt().unwrap().message, "Specify other corner");
    let c = hover(&mut s, Vec2::new(4.0, 2.0));
    s.input(Input::Point(c)).unwrap();
    s.cmdline("x").unwrap();
    assert!(s.current_prompt().is_none(), "ARRAYRECT is done, still at {:?}", s.current_prompt().map(|p| p.message));
    let b = s.doc().unwrap().extents(&s.space());
    (b.min, b.max)
}

#[test]
fn array_unit_cell_ignores_ortho() {
    let on = click_unit_cell(true);
    assert_eq!(on, (Vec2::new(-0.5, -0.5), Vec2::new(4.5, 2.5)), "columns 4 apart, rows 2 apart with ortho on");
    assert_eq!(click_unit_cell(false), on, "ortho changes nothing");
}
