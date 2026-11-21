//! Selection cycling (SELECTIONCYCLING, #1143): the objects under the pick box a click chooses
//! between, and picking one of them as the click would have.

use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Session, select};
use serde_json::json;

/// Two lines that overlap from x = 0 to x = 10 (the second drawn on top), and a line apart.
fn drawing() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [20, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 50], [10, 50]] })).unwrap();
    let hs: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    s.set_selection(Vec::new());
    (s, hs)
}

#[test]
fn pick_all_lists_every_object_under_the_pick_box() {
    let (s, hs) = drawing();
    let d = s.doc().unwrap();
    // Topmost first, as a plain pick chooses it.
    assert_eq!(select::pick_all(d, &Space::Model, Vec2::new(5.0, 0.0), 0.5), vec![hs[1], hs[0]]);
    assert_eq!(select::pick(d, &Space::Model, Vec2::new(5.0, 0.0), 0.5), Some(hs[1]));
    // Where only one object is, only that one; the far line never.
    assert_eq!(select::pick_all(d, &Space::Model, Vec2::new(15.0, 0.0), 0.5), vec![hs[1]]);
    assert!(select::pick_all(d, &Space::Model, Vec2::new(5.0, 25.0), 0.5).is_empty());
}

#[test]
fn cycling_candidates_follow_selectioncycling() {
    let (mut s, hs) = drawing();
    s.viewport_px = (1000.0, 1000.0);
    let at = Vec2::new(5.0, 0.0);
    // Off (AutoCAD's initial -2): a click picks as it always did.
    assert_eq!(s.execute("getvar", &json!({ "name": "SELECTIONCYCLING" })).unwrap()["value"], -2);
    assert!(s.cycling_candidates(at).is_empty());
    // 2: the list of both lines; a single object under the cursor needs no list.
    s.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": 2 })).unwrap();
    assert_eq!(s.cycling_candidates(at), vec![hs[1], hs[0]]);
    assert!(s.cycling_candidates(Vec2::new(15.0, 0.0)).is_empty());
    // 1: the badge only, no list; the toggle keeps the display mode it turns off.
    s.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": 1 })).unwrap();
    assert!(s.cycling_candidates(at).is_empty());
    assert!(s.cycling_badge(at));
    s.execute("selectioncycling", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "SELECTIONCYCLING" })).unwrap()["value"], -1);
    assert!(!s.cycling_badge(at));
    s.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": 0 })).unwrap();
    assert!(s.cycling_candidates(at).is_empty() && !s.cycling_badge(at));
    for v in [json!(3), json!(-3), json!("x")] {
        assert!(s.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": v })).is_err(), "{v}");
    }
}

#[test]
fn a_cycled_pick_selects_like_a_click() {
    let (mut s, hs) = drawing();
    s.pick_cycled(hs[0], false).unwrap();
    assert_eq!(s.selection(), vec![hs[0]]);
    // Shift removes it again (PICKADD on).
    s.pick_cycled(hs[0], true).unwrap();
    assert!(s.selection().is_empty());
    // At "Select objects" the chosen object goes to the command.
    s.start("erase").unwrap();
    s.pick_cycled(hs[0], false).unwrap();
    s.input(cadcraft_engine::Input::Enter).unwrap();
    assert!(s.running.is_none());
    let left: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    assert_eq!(left, vec![hs[1], hs[2]]);
}

#[test]
fn a_cycled_pick_of_an_erased_object_selects_nothing() {
    let (mut s, hs) = drawing();
    s.execute("erase", &json!({ "handles": [hs[0].hex()] })).unwrap();
    s.pick_cycled(hs[0], false).unwrap();
    assert!(s.selection().is_empty(), "a handle no longer in the drawing is never selected");
}
