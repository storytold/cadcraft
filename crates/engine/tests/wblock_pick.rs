//! WBLOCKPICK: the Write Block dialog's canvas picks (objects, base point) and their cancel.

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn session_with_line() -> (Session, cadcraft_engine::doc::Handle) {
    let mut s = Session::new();
    let r = s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
    s.set_selection(Vec::new());
    (s, h)
}

#[test]
fn objects_keeps_the_selection() {
    let (mut s, h) = session_with_line();
    s.start("wblockpick").unwrap();
    s.input(Input::Keyword("Objects".into())).unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    let p = s.wblock_picks.take().unwrap();
    assert_eq!((p.handles, p.base), (vec![h], None));
}

#[test]
fn base_keeps_the_point() {
    let (mut s, _) = session_with_line();
    s.start("wblockpick").unwrap();
    s.input(Input::Keyword("Base".into())).unwrap();
    s.input(Input::Point(Vec2::new(3.0, 4.0))).unwrap();
    assert!(s.running.is_none());
    let p = s.wblock_picks.take().unwrap();
    assert_eq!((p.handles.len(), p.base), (0, Some(Vec2::new(3.0, 4.0))));
}

#[test]
fn escape_keeps_nothing() {
    let (mut s, h) = session_with_line();
    s.start("wblockpick").unwrap();
    s.input(Input::Keyword("Objects".into())).unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
    assert!(s.wblock_picks.is_none());
}

#[test]
fn preexisting_selection_still_prompts() {
    // Even if there is a pickfirst selection, WBLOCKPICK Objects must keep prompting
    // so the dialog drives picking explicitly — it never bypasses the "Select objects" step.
    let (mut s, h) = session_with_line();
    s.set_selection(vec![h]);
    s.start("wblockpick").unwrap();
    s.input(Input::Keyword("Objects".into())).unwrap();
    // Must still be running — "Select objects" prompt expected.
    assert!(s.running.is_some());
    assert!(s.wblock_picks.is_none());
}
