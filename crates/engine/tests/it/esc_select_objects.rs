//! #2889: Esc at "Select objects" cancels the command and leaves nothing selected. Before, the
//! picks stayed selected (the prompt highlights them through the selection), and the next
//! command took them as a pickfirst selection without asking. A pickfirst selection the command
//! started with stays selected after Esc, as before.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// A drawing with one line and nothing selected, and the line's handle.
fn one_line() -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle;
    s.set_selection(Vec::new());
    (s, h)
}

/// Commands with their own select phase, and one that runs through `SelectRun` (ANNORESET).
#[test]
fn esc_at_select_objects_leaves_nothing_selected() {
    for cmd in ["move", "copy", "erase", "rotate", "annoreset"] {
        let (mut s, h) = one_line();
        s.start(cmd).unwrap();
        s.input(Input::Pick(vec![h])).unwrap();
        assert_eq!(s.selection(), vec![h], "{cmd}: the pick is highlighted while the prompt is up");
        s.cancel();
        assert!(!s.prompting(), "{cmd}: Esc cancels the command");
        assert!(s.selection().is_empty(), "{cmd}: Esc left the pick selected");
    }
}

/// The harm the leftover selection did: ERASE right after an Esc'd MOVE erased the line at once.
#[test]
fn the_next_command_asks_for_objects_after_an_esc() {
    let (mut s, h) = one_line();
    s.start("move").unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.cancel();
    s.start("erase").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "ERASE asks for objects: {}", s.prompt_text());
    assert_eq!(s.doc().unwrap().model.len(), 1, "the line is still there");
}

#[test]
fn esc_keeps_the_pickfirst_selection_a_command_started_with() {
    let (mut s, h) = one_line();
    s.set_selection(vec![h]);
    s.start("move").unwrap();
    assert!(s.prompting(), "MOVE takes the selection and asks for the base point");
    s.cancel();
    assert_eq!(s.selection(), vec![h]);
}

/// A program can switch to a layout while a command runs. The switch clears the selection, and
/// Esc must not bring back model-space objects on the layout, where the next command would take
/// them as its selection.
#[test]
fn esc_after_a_layout_switch_restores_nothing_from_model_space() {
    let (mut s, h) = one_line();
    s.set_selection(vec![h]);
    s.start("move").unwrap();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    s.cancel();
    assert!(s.selection().is_empty(), "a model-space line came back selected on the layout: {:?}", s.selection());
}
