//! Undo and redo keep the session on a layout that exists (#371): undoing a rename of the current
//! layout stays on that tab under its old name; undoing its creation returns to the Model tab.

use cadcraft_doc::Space;
use cadcraft_engine::Session;
use serde_json::json;

fn paper(n: &str) -> Space {
    Space::Paper(n.into())
}

#[test]
fn undo_and_redo_of_a_rename_follow_the_current_layout() {
    let mut s = Session::new();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.execute("mview", &json!({"p1": [1, 1], "p2": [5, 4]})).unwrap();
    let paper_view = s.state().unwrap().paper_view();
    s.execute("layout.rename", &json!({"to": "Sheet"})).unwrap();
    assert_eq!(s.layout_space(), paper("Sheet"));

    s.cmdline("u").unwrap();
    assert_eq!(s.layout_space(), paper("Layout1"));
    assert_eq!(s.state().unwrap().paper_view(), paper_view, "the sheet view is kept");
    // The layout is shown again: its sheet is drawn.
    let list = cadcraft_render::build(s.doc().unwrap(), &s.layout_space(), &cadcraft_render::Options::default());
    assert!(list.sheet.is_some());

    s.redo().unwrap();
    assert_eq!(s.layout_space(), paper("Sheet"));
    s.execute("line", &json!({"points": [[1, 1], [2, 2]]})).unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.layout_space(), paper("Layout1"));
    s.execute("mview", &json!({"p1": [6, 1], "p2": [8, 4]})).unwrap();
}

#[test]
fn undoing_the_current_layouts_creation_returns_to_model() {
    let mut s = Session::new();
    s.execute("layout.new", &json!({"name": "New"})).unwrap();
    s.execute("layout.set", &json!({"name": "New"})).unwrap();
    s.execute("mspace", &json!({})).unwrap();
    s.execute("line", &json!({"points": [[1, 1], [5, 5]]})).unwrap();
    s.undo().unwrap();
    assert_eq!(s.layout_space(), paper("New"), "the layout still exists");
    s.undo().unwrap();
    assert_eq!(s.layout_space(), Space::Model);
    assert_eq!(s.space(), Space::Model);
    assert!(s.state().unwrap().mspace.is_none());
    s.execute("line", &json!({"points": [[1, 1], [5, 5]]})).unwrap();

    // Other layouts are left alone: undoing a rename of a layout that is not current.
    let mut s = Session::new();
    s.execute("layout.set", &json!({"name": "Layout2"})).unwrap();
    s.execute("layout.rename", &json!({"from": "Layout1", "to": "A"})).unwrap();
    s.undo().unwrap();
    assert_eq!(s.layout_space(), paper("Layout2"));
}
