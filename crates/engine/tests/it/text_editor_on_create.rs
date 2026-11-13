//! TEXT and MLEADER hand their text to the in-place editor once it is asked for, and show the
//! text's frame / the leader segment while points are picked.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{EditorKind, Input, Session};

fn texts(s: &Session) -> Vec<String> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn text_opens_a_line_editor_for_each_line_and_an_empty_line_ends_it() {
    let mut s = Session::new();
    s.start("text").unwrap();
    // Before the start point: the frame of a line of text follows the cursor.
    assert!(!s.preview(Vec2::new(1.0, 1.0)).is_empty());
    assert!(s.text_editor().is_none());
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(!s.preview(Vec2::new(0.0, 2.0)).is_empty(), "the frame grows with the height");
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    let ed = s.text_editor().expect("the editor opens at the text prompt");
    assert_eq!(ed.kind, EditorKind::Line);
    let first_key = ed.key.clone();
    s.input(Input::Text("first".into())).unwrap();
    let ed = s.text_editor().expect("the next line gets an editor");
    assert_ne!(ed.key, first_key);
    assert!(ed.mtext.insert.y < 0.0, "the next line is below the first");
    s.input(Input::Text("second".into())).unwrap();
    s.input(Input::Text(String::new())).unwrap();
    assert!(s.running.is_none());
    assert_eq!(texts(&s), ["first", "second"]);
}

#[test]
fn mleader_opens_the_editor_after_its_leader_and_previews_the_segment() {
    let mut s = Session::new();
    s.start("mleader").unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(!s.preview(Vec2::new(5.0, 5.0)).is_empty(), "the segment to the cursor shows");
    assert!(s.editing_mtext().is_none());
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    let m = s.editing_mtext().expect("the editor opens at the landing");
    assert!(m.insert.x > 5.0 && m.contents.is_empty());
    s.input(Input::Text("NOTE".into())).unwrap();
    assert!(s.running.is_none());
    let found =
        s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::MLeader(m) if m.text.as_ref().is_some_and(|t| t.contents == "NOTE")));
    assert!(found);
}
