//! Construction geometry is visible while the Block Editor is open and is saved invisible (the
//! entity's visibility flag, group 60), as AutoCAD does: the invisible flag is what keeps it out
//! of the block's references and out of a drawing saved from `<Current Drawing>`; reopening the
//! editor shows it again. Without the flag a reader that honours only the flag (AutoCAD does)
//! draws the construction geometry in every reference.

use cadcraft_engine::Session;
use cadcraft_engine::blockedit::CURRENT_DRAWING;
use cadcraft_engine::doc::{Drawing, Handle, RawTag};
use serde_json::json;

fn mark() -> Vec<RawTag> {
    vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }]
}

/// (marked, visible) of each object of block `block`, in the block's order.
fn flags(d: &Drawing, block: &str) -> Vec<(bool, bool)> {
    d.block(block).unwrap().entities.iter().map(|e| (e.common.is_construction(), e.common.visible)).collect()
}

/// A drawing whose block "Door" (base point `base`) holds two lines, the Block Editor open on it.
/// The second line is the one a test marks as construction geometry.
fn editor_on_door(base: [i32; 2]) -> (Session, Handle) {
    let mut s = Session::new();
    for y in [0, 5] {
        s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
    }
    let all: Vec<String> = s.doc().unwrap().model.handles().iter().map(|h| h.hex()).collect();
    s.execute("block", &json!({ "name": "Door", "base": base, "handles": all })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 2);
    (s, handles[1])
}

#[test]
fn bsave_keeps_construction_visible_in_the_editor_and_saves_it_invisible() {
    let (mut s, guide) = editor_on_door([0, 0]);
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    assert_eq!(
        flags(s.state().unwrap().drawing(), "Door"),
        vec![(false, true), (true, false)],
        "the plain line stays visible, the marked one is saved invisible"
    );
    assert!(s.doc().unwrap().entity(guide).unwrap().common.visible, "the editor still shows it");
    // Unsaved changes kept for recovery are saved the same way.
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    let recovery = s.state().unwrap().recovery_drawing().unwrap();
    assert_eq!(flags(&recovery, "Door"), vec![(false, true), (true, false)]);
}

#[test]
fn reopening_the_editor_shows_the_saved_invisible_geometry_again() {
    let (mut s, guide) = editor_on_door([0, 0]);
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let saved = flags(s.doc().unwrap(), "Door");
    assert_eq!(saved, vec![(false, true), (true, false)]);

    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    assert!(s.doc().unwrap().model.iter().all(|e| e.common.visible), "the editor shows every object");
    assert_eq!(flags(s.state().unwrap().drawing(), "Door"), saved, "the definition is still the saved one");
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(flags(s.doc().unwrap(), "Door"), saved, "closing without saving leaves it as it was");
}

#[test]
fn an_unedited_construction_object_keeps_its_exact_coordinates_with_a_base_point() {
    let mut s = Session::new();
    // A base point whose coordinates don't survive x - b + b exactly, so an object the editor
    // mistook for edited would come back with different numbers.
    s.execute("line", &json!({ "points": [[0.2, 0.4], [0.9, 0.6]] })).unwrap();
    s.execute("line", &json!({ "points": [[0.1, 0.7], [0.3, 0.9]] })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    s.doc_mut().unwrap().modify_entity(handles[1], |e| e.common.xdata = mark()).unwrap();
    let all: Vec<String> = handles.iter().map(|h| h.hex()).collect();
    s.execute("block", &json!({ "name": "B", "base": [0.7, 0.1], "handles": all })).unwrap();
    // Nothing is edited: the saved definition keeps its exact numbers and only the flag changes.
    s.execute("bedit", &json!({ "name": "B" })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let saved = s.doc().unwrap().block("B").unwrap().as_ref().clone();
    assert_eq!(flags(s.doc().unwrap(), "B"), vec![(false, true), (true, false)]);

    // The saved definition holds the construction object invisible, the editor shows it visible:
    // that difference alone must not make it look edited.
    s.execute("bedit", &json!({ "name": "B" })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    assert_eq!(s.state().unwrap().drawing().block("B").unwrap().as_ref(), &saved, "nothing changed, so every number and flag is the saved one");
}

#[test]
fn bsaveas_saves_the_copy_invisible_too() {
    let (mut s, guide) = editor_on_door([0, 0]);
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    s.execute("bsaveas", &json!({ "name": "Copy" })).unwrap();
    assert_eq!(flags(s.state().unwrap().drawing(), "Copy"), vec![(false, true), (true, false)]);
}

#[test]
fn the_current_drawing_editor_saves_construction_objects_invisible() {
    let mut s = Session::new();
    for y in [0, 5] {
        s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
    }
    let all = s.doc().unwrap().model.handles();
    let (plain, marked) = (all[0], all[1]);
    s.execute("bedit", &json!({ "name": CURRENT_DRAWING })).unwrap();
    s.doc_mut().unwrap().modify_entity(marked, |e| e.common.xdata = mark()).unwrap();
    assert!(s.doc().unwrap().entity(marked).unwrap().common.visible, "visible inside the editor");

    s.execute("bsave", &json!({})).unwrap();
    {
        let host = s.state().unwrap().drawing();
        assert!(host.entity(plain).unwrap().common.visible);
        assert!(!host.entity(marked).unwrap().common.visible, "saved into the drawing it is invisible");
    }
    s.execute("bclose", &json!({})).unwrap();
    assert!(s.doc().unwrap().entity(plain).unwrap().common.visible);
    assert!(!s.doc().unwrap().entity(marked).unwrap().common.visible, "and stays so outside the editor");
    assert!(s.doc().unwrap().entity(marked).unwrap().common.is_construction(), "with its mark");

    // The editor shows it again; closing without saving keeps the drawing as it was.
    s.execute("bedit", &json!({ "name": CURRENT_DRAWING })).unwrap();
    assert!(s.doc().unwrap().entity(marked).unwrap().common.visible);
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert!(!s.doc().unwrap().entity(marked).unwrap().common.visible);
}
