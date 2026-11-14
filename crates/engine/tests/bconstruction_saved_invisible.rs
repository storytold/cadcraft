//! Construction geometry is visible while the Block Editor is open and is saved invisible (the
//! entity's visibility flag, group 60), as AutoCAD does: the invisible flag is what keeps it out
//! of the block's references and out of a drawing saved from `<Current Drawing>`; reopening the
//! editor shows it again. Without the flag a reader that honours only the flag (AutoCAD does)
//! draws the construction geometry in every reference.

use cadcraft_engine::Session;
use cadcraft_engine::blockedit::CURRENT_DRAWING;
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{Drawing, EntityKind, Handle, RawTag};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

/// The extended data that marks an object as construction geometry.
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

/// Bsave keeps construction visible in the editor and saves it invisible.
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

/// Reopening the editor shows the saved invisible geometry again.
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

/// An unedited construction object keeps its exact coordinates with a base point.
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

/// The geometry of each object of block `block`, in the block's order.
fn geometry(d: &Drawing, block: &str) -> Vec<EntityKind> {
    d.block(block).unwrap().entities.iter().map(|e| e.kind.clone()).collect()
}

/// A drawing whose block "B" (base point (0.7, 0.1)) holds two lines. The second one starts at
/// x = 0.1, and `0.1 - 0.7 + 0.7` is not `0.1`: sent to the editor's origin and back, that line
/// comes out with a different number.
fn fractional_block() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0.2, 0.4], [0.9, 0.6]] })).unwrap();
    s.execute("line", &json!({ "points": [[0.1, 0.7], [0.3, 0.9]] })).unwrap();
    let all: Vec<String> = s.doc().unwrap().model.handles().iter().map(|h| h.hex()).collect();
    s.execute("block", &json!({ "name": "B", "base": [0.7, 0.1], "handles": all })).unwrap();
    s
}

/// Converting an object moves nothing: the block keeps its exact coordinates with a base point.
#[test]
fn converting_an_object_keeps_its_exact_coordinates_with_a_base_point() {
    let mut s = fractional_block();
    let before = geometry(s.doc().unwrap(), "B");
    s.execute("bedit", &json!({ "name": "B" })).unwrap();
    let guide = s.doc().unwrap().model.handles()[1];
    s.execute("bconstruction", &json!({ "mode": "convert", "handles": [guide.hex()] })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(flags(d, "B"), vec![(false, true), (true, false)], "the second line is saved as construction geometry");
    assert_eq!(geometry(d, "B"), before, "Convert moves nothing, so every coordinate is the one the block had");
}

/// A property changed in the editor is saved with the block's exact coordinates, and an object
/// moved in the editor is still saved where it was moved to.
#[test]
fn a_property_change_keeps_exact_coordinates_and_a_move_is_still_saved() {
    let mut s = fractional_block();
    let before = geometry(s.doc().unwrap(), "B");
    s.execute("bedit", &json!({ "name": "B" })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    s.execute("move", &json!({ "handles": [handles[0].hex()], "delta": [1, 0] })).unwrap();
    s.execute("properties.set", &json!({ "handles": [handles[1].hex()], "color": 1 })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let after = geometry(d, "B");
    assert_eq!(d.block("B").unwrap().entities.iter().nth(1).unwrap().common.color, Color::Index(1), "the new color is saved");
    assert_eq!(after[1], before[1], "a change of color moves nothing, so every coordinate is the one the block had");
    let (EntityKind::Line(moved), EntityKind::Line(was)) = (&after[0], &before[0]) else { panic!("not lines: {after:?}") };
    assert!(moved.a.xy().near(was.a.xy() + Vec2::new(1.0, 0.0), 1e-12) && moved.b.xy().near(was.b.xy() + Vec2::new(1.0, 0.0), 1e-12), "{moved:?}");
}

/// Bsaveas saves the copy invisible too.
#[test]
fn bsaveas_saves_the_copy_invisible_too() {
    let (mut s, guide) = editor_on_door([0, 0]);
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    s.execute("bsaveas", &json!({ "name": "Copy" })).unwrap();
    assert_eq!(flags(s.state().unwrap().drawing(), "Copy"), vec![(false, true), (true, false)]);
}

/// The current drawing editor saves construction objects invisible.
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
