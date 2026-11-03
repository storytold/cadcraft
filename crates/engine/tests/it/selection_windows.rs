//! One selection for every command (#1116, #1001 follow-up): STRETCH selects through the
//! session's "Select objects" like MOVE or ERASE, and every selection window waiting for its
//! opposite corner, whoever opened it, is reported alike (`Session::selection_window`), so the UI
//! draws it and lets a drag close it the same way everywhere.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::prompt::Input;
use cadcraft_engine::select::{PickArea, SelectArea};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.viewport_px = (1000.0, 1000.0);
    s.execute("view.set", &json!({"center": [10, 5], "height": 40})).unwrap();
    s.settings.osmode = 0;
    s
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let h = s.execute("line", &json!({ "points": [a, b] })).unwrap()["handles"][0].as_str().unwrap().to_string();
    Handle::parse_hex(&h).unwrap()
}

fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        _ => panic!("not a line"),
    }
}

fn point(s: &mut Session, x: f64, y: f64) {
    s.input(Input::Point(Vec2::new(x, y))).unwrap();
}

#[test]
fn stretch_selects_at_the_sessions_select_objects_prompt() {
    let mut s = session();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.start("stretch").unwrap();
    let p = s.current_prompt().unwrap();
    assert!(p.accept.select, "STRETCH asks for objects like every other command: {p:?}");
    // A click on empty space opens the session's window; right to left it is a crossing.
    point(&mut s, 12.0, 2.0);
    let w = s.selection_window().expect("a window waits for its opposite corner");
    assert_eq!(w.corner, Vec2::new(12.0, 2.0));
    assert!(w.crossing_to(Vec2::new(8.0, -2.0)));
    point(&mut s, 8.0, -2.0);
    assert!(s.selection_window().is_none());
    assert_eq!(s.selection(), vec![a]);
    s.input(Input::Enter).unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 5.0, 0.0);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(ends(&s, a), (Vec2::new(0.0, 0.0), Vec2::new(15.0, 0.0)), "only the end inside the crossing moves");
}

#[test]
fn stretch_moves_picked_objects_whole_and_removes_with_r() {
    let mut s = session();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    s.start("stretch").unwrap();
    // A pick, then a crossing over both right ends, then remove the second line with a pick.
    point(&mut s, 5.0, 0.0);
    point(&mut s, 12.0, 7.0);
    point(&mut s, 8.0, -2.0);
    s.input(Input::Text("R".into())).unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Remove objects");
    point(&mut s, 5.0, 5.0);
    s.input(Input::Enter).unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 0.0, 3.0);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    // Picked first, so it moves whole even though the crossing also took it.
    assert_eq!(ends(&s, a), (Vec2::new(0.0, 3.0), Vec2::new(10.0, 3.0)));
    assert_eq!(ends(&s, b), (Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0)), "removed");
}

#[test]
fn the_area_a_pick_was_made_with_is_read_once() {
    let mut s = session();
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.start("erase").unwrap();
    point(&mut s, 5.0, 0.0);
    assert_eq!(s.take_pick_area(), None, "a single pick marks no area");
    point(&mut s, -2.0, -2.0);
    point(&mut s, 12.0, 2.0);
    let bx = match s.take_pick_area() {
        Some(PickArea { area: SelectArea::Box(bx), crossing: false }) => bx,
        other => panic!("left to right is a window: {other:?}"),
    };
    assert!(bx.contains(Vec2::new(0.0, 0.0)) && bx.contains(Vec2::new(10.0, 0.0)));
    assert_eq!(s.take_pick_area(), None);
    s.input(Input::Text("CP".into())).unwrap();
    for (x, y) in [(-1.0, -1.0), (11.0, -1.0), (5.0, 3.0)] {
        point(&mut s, x, y);
    }
    s.input(Input::Enter).unwrap();
    assert!(matches!(s.take_pick_area(), Some(PickArea { area: SelectArea::Polygon(p), crossing: true }) if p.len() == 3));
}

#[test]
fn typed_window_and_crossing_corners_are_selection_windows() {
    let mut s = session();
    s.start("erase").unwrap();
    for (kw, crossing) in [("W", Some(false)), ("C", Some(true)), ("BOX", None)] {
        s.input(Input::Text(kw.into())).unwrap();
        assert!(s.selection_window().is_none(), "no corner yet");
        point(&mut s, 0.0, 0.0);
        let w = s.selection_window().expect("the opposite corner is a selection window");
        assert_eq!((w.corner, w.crossing), (Vec2::ZERO, crossing), "{kw}");
        let p = s.current_prompt().unwrap();
        assert!(!cadcraft_engine::pointmod::accepts(&p), "{kw}: a window corner picks objects, no object snaps");
        point(&mut s, 3.0, 3.0);
        assert!(s.selection_window().is_none());
    }
}

#[test]
fn trims_crossing_window_is_a_selection_window() {
    let mut s = session();
    line(&mut s, [0.0, 0.0], [20.0, 0.0]);
    line(&mut s, [10.0, -5.0], [10.0, 5.0]);
    s.start("trim").unwrap();
    // A click on empty space at "Select object to trim" opens TRIM's crossing window.
    point(&mut s, 15.0, 2.0);
    let w = s.selection_window().expect("TRIM's crossing waits for its opposite corner");
    assert_eq!((w.corner, w.crossing), (Vec2::new(15.0, 2.0), Some(true)), "always a crossing");
    assert!(!cadcraft_engine::pointmod::accepts(&s.current_prompt().unwrap()), "no object snaps at the corner");
    point(&mut s, 18.0, -2.0);
    assert!(s.selection_window().is_none());
}

#[test]
fn a_typed_fence_shows_its_lines_at_any_select_objects() {
    let mut s = session();
    s.start("erase").unwrap();
    s.input(Input::Text("F".into())).unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 5.0, 0.0);
    let pv = s.preview(Vec2::new(5.0, 5.0));
    assert_eq!(pv.len(), 2, "the fence so far and on to the cursor: {pv:?}");
}

/// A crossing made at STRETCH's "Select objects" moves only the multiline vertex inside it (#1739),
/// whether clicked or typed (`CP`).
#[test]
fn a_stretch_crossing_takes_a_multiline_by_its_vertex() {
    let vertices = |s: &Session| -> Vec<(f64, f64)> {
        s.doc()
            .unwrap()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLine(m) => Some(m.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()),
                _ => None,
            })
            .unwrap()
    };
    let mut s = Session::new();
    s.viewport_px = (1000.0, 1000.0);
    // Zoomed in so the clicks 5 from the vertex are on empty space, not a pick of the multiline.
    s.execute("view.set", &json!({"center": [3000, 300], "height": 500})).unwrap();
    s.settings.osmode = 0;
    // The element lines run 100 away from the middle vertex at (3000, 0).
    s.execute("mline", &json!({ "points": [[0, 0], [3000, 0], [3000, 2000]], "scale": 200 })).unwrap();
    s.start("stretch").unwrap();
    point(&mut s, 3005.0, 5.0);
    point(&mut s, 2995.0, -5.0);
    s.input(Input::Enter).unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 0.0, 610.0);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(vertices(&s), vec![(0.0, 0.0), (3000.0, 610.0), (3000.0, 2000.0)]);
    s.start("stretch").unwrap();
    s.input(Input::Text("CP".into())).unwrap();
    for (x, y) in [(2995.0, 605.0), (3005.0, 605.0), (3000.0, 615.0)] {
        point(&mut s, x, y);
    }
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 0.0, -610.0);
    assert_eq!(vertices(&s), vec![(0.0, 0.0), (3000.0, 0.0), (3000.0, 2000.0)]);
}
