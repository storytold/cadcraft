//! MTEXT's opposite corner ignores ortho: the text box keeps its width and proportion (#3388).

use cadcraft_doc::{EntityKind, MText};
use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_engine::{Input, Session};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::json;

/// Move the cursor to `raw` at the current prompt, as the canvas does.
fn hover(s: &mut Session, raw: Vec2) -> Vec2 {
    let p = s.current_prompt().unwrap();
    let q = CursorQuery { base: p.base, deferred: p.deferred, corner: p.corner, aperture: 1e-6, tol: 1e-6, now: 0.0, shift: false };
    let c = s.snap_cursor(raw, &q).point;
    s.cursor = c;
    c
}

fn session(ortho: bool) -> Session {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
    s.execute("setvar", &json!({ "name": "ORTHOMODE", "value": i32::from(ortho) })).unwrap();
    s
}

fn last_mtext(s: &Session) -> MText {
    match s.doc().unwrap().model.last().map(|e| e.kind.clone()) {
        Some(EntityKind::MText(t)) => t,
        k => panic!("no mtext: {k:?}"),
    }
}

/// Place an MTEXT from `first` to a cursor click at `second`, returning the preview bounds,
/// in-place editor box `(insert, width)` and created entity `(insert, width)`.
fn click_mtext(ortho: bool, first: Vec2, second: Vec2) -> ((Vec2, Vec2), (Vec2, f64), (Vec2, f64)) {
    let mut s = session(ortho);
    s.cmdline(&format!("mtext {},{}", first.x, first.y)).unwrap();
    let p = s.current_prompt().unwrap();
    assert_eq!(p.message, "Specify opposite corner");
    assert!(p.corner, "MTEXT opposite corner ignores ortho");
    let c = hover(&mut s, second);
    assert!(c.near(second, 1e-9), "opposite corner stays under the cursor: {c:?}");
    let mut pv = Bounds2::EMPTY;
    for e in s.preview(c) {
        pv = pv.union(&cadcraft_doc::entity_bounds(s.doc().unwrap(), &e, 0));
    }
    s.input(Input::Point(c)).unwrap();
    let ed = s.editing_mtext().unwrap();
    s.input(Input::Text("Hello".to_string())).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.current_prompt().is_none(), "MTEXT is done, still at {:?}", s.current_prompt().map(|p| p.message));
    let t = last_mtext(&s);
    assert_eq!(t.contents, "Hello");
    ((pv.min, pv.max), (ed.insert.xy(), ed.width), (t.insert.xy(), t.width))
}

#[test]
fn mtext_opposite_corner_ignores_ortho() {
    let cases = [
        // Wide box (dx > dy)
        (Vec2::ZERO, Vec2::new(10.0, 3.0), Vec2::ZERO, Vec2::new(10.0, 3.0), Vec2::new(0.0, 3.0), 10.0),
        // Tall box (dy > dx)
        (Vec2::ZERO, Vec2::new(3.0, 10.0), Vec2::ZERO, Vec2::new(3.0, 10.0), Vec2::new(0.0, 10.0), 3.0),
        // Opposite quadrant (negative dx and dy)
        (Vec2::new(10.0, 10.0), Vec2::new(2.0, 4.0), Vec2::new(2.0, 4.0), Vec2::new(10.0, 10.0), Vec2::new(2.0, 10.0), 8.0),
    ];
    for (first, second, min, max, insert, width) in cases {
        let on = click_mtext(true, first, second);
        assert!(on.0.0.near(min, 1e-9) && on.0.1.near(max, 1e-9), "preview bounds with ortho on: {:?}", on.0);
        assert!(on.1.0.near(insert, 1e-9) && (on.1.1 - width).abs() < 1e-9, "editor box with ortho on: {:?}", on.1);
        assert!(on.2.0.near(insert, 1e-9) && (on.2.1 - width).abs() < 1e-9, "entity box with ortho on: {:?}", on.2);
        let off = click_mtext(false, first, second);
        assert_eq!(off, on, "ortho changes nothing");
    }
}

#[test]
fn mtext_coordinate_and_direct_distance_input_with_ortho() {
    let mut s = session(true);
    // Absolute coordinate input for the opposite corner.
    s.cmdline("mtext 0,0").unwrap();
    s.cmdline("10,3").unwrap();
    let ed = s.editing_mtext().unwrap();
    assert!(ed.insert.xy().near(Vec2::new(0.0, 3.0), 1e-9));
    assert!((ed.width - 10.0).abs() < 1e-9);
    s.input(Input::Text("Abs".to_string())).unwrap();
    s.input(Input::Enter).unwrap();
    let t = last_mtext(&s);
    assert!(t.insert.xy().near(Vec2::new(0.0, 3.0), 1e-9));
    assert!((t.width - 10.0).abs() < 1e-9);

    // Relative coordinate input for the opposite corner.
    s.cmdline("mtext 2,3").unwrap();
    s.cmdline("@4,9").unwrap();
    let ed = s.editing_mtext().unwrap();
    assert!(ed.insert.xy().near(Vec2::new(2.0, 12.0), 1e-9));
    assert!((ed.width - 4.0).abs() < 1e-9);
    s.input(Input::Text("Rel".to_string())).unwrap();
    s.input(Input::Enter).unwrap();
    let t = last_mtext(&s);
    assert!(t.insert.xy().near(Vec2::new(2.0, 12.0), 1e-9));
    assert!((t.width - 4.0).abs() < 1e-9);

    // Direct distance entry goes toward the cursor rather than along an axis.
    s.cmdline("mtext 0,0").unwrap();
    let c = hover(&mut s, Vec2::new(3.0, 4.0));
    assert!(c.near(Vec2::new(3.0, 4.0), 1e-9));
    s.cmdline("5").unwrap();
    let ed = s.editing_mtext().unwrap();
    assert!(ed.insert.xy().near(Vec2::new(0.0, 4.0), 1e-9));
    assert!((ed.width - 3.0).abs() < 1e-9);
    s.input(Input::Text("Dir".to_string())).unwrap();
    s.input(Input::Enter).unwrap();
    let t = last_mtext(&s);
    assert!(t.insert.xy().near(Vec2::new(0.0, 4.0), 1e-9));
    assert!((t.width - 3.0).abs() < 1e-9);
}

#[test]
fn mtext_escape_cancels_cleanly_with_ortho() {
    let mut s = session(true);
    // Cancel at the opposite-corner prompt.
    s.cmdline("mtext 0,0").unwrap();
    let c = hover(&mut s, Vec2::new(10.0, 3.0));
    assert!(c.near(Vec2::new(10.0, 3.0), 1e-9));
    s.cancel();
    assert!(s.current_prompt().is_none());
    assert!(s.editing_mtext().is_none());
    assert!(s.doc().unwrap().model.is_empty());

    // Cancel after picking the opposite corner (at the text prompt).
    s.cmdline("mtext 0,0").unwrap();
    let c = hover(&mut s, Vec2::new(10.0, 3.0));
    s.input(Input::Point(c)).unwrap();
    assert!(s.editing_mtext().is_some());
    s.cancel();
    assert!(s.current_prompt().is_none());
    assert!(s.editing_mtext().is_none());
    assert!(s.doc().unwrap().model.is_empty());
}

#[test]
fn line_and_pline_still_obey_ortho() {
    let mut s = session(true);
    s.cmdline("line 0,0").unwrap();
    let p = s.current_prompt().unwrap();
    assert!(!p.corner, "LINE next point is constrained by ortho");
    let c = hover(&mut s, Vec2::new(10.0, 3.0));
    assert!(c.near(Vec2::new(10.0, 0.0), 1e-9), "ortho keeps LINE horizontal: {c:?}");
    s.cancel();

    s.cmdline("pline 0,0").unwrap();
    let p = s.current_prompt().unwrap();
    assert!(!p.corner, "PLINE next point is constrained by ortho");
    let c = hover(&mut s, Vec2::new(3.0, 10.0));
    assert!(c.near(Vec2::new(0.0, 10.0), 1e-9), "ortho keeps PLINE vertical: {c:?}");
    s.cancel();
}
