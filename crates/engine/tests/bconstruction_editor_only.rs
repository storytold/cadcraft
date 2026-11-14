//! The construction mark (ACAD_BCONSTRUCTION) only changes how an object is shown and edited inside
//! the Block Editor. In a drawing the mark is data an ordinary object carries, as AutoCAD treats it
//! (a line saved from the editor's <Current Drawing> is a normal line: its color, layer and
//! linetype can be changed).

use cadcraft_engine::Session;
use cadcraft_engine::blockedit::CURRENT_DRAWING;
use cadcraft_engine::doc::{Handle, RawTag};
use serde_json::json;

/// The extended data that marks an object as construction geometry.
fn mark() -> Vec<RawTag> {
    vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }]
}

/// A drawing with a plain line and a marked one, no editor open.
fn drawing_with_a_marked_line() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    for y in [0, 5] {
        s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
    }
    let all = s.doc().unwrap().model.handles();
    assert_eq!(all.len(), 2);
    let (plain, marked) = (all[0], all[1]);
    s.doc_mut().unwrap().modify_entity(marked, |e| e.common.xdata = mark()).unwrap();
    s.execute("layer.new", &json!({ "name": "Other" })).unwrap();
    (s, plain, marked)
}

/// A new drawing is not a block editor.
#[test]
fn a_new_drawing_is_not_a_block_editor() {
    let s = Session::new();
    assert!(!s.doc().unwrap().block_editor);
}

/// Outside the editor a marked object takes color layer and linetype changes.
#[test]
fn outside_the_editor_a_marked_object_takes_color_layer_and_linetype_changes() {
    let (mut s, plain, marked) = drawing_with_a_marked_line();
    s.execute("properties.set", &json!({ "handles": [marked.hex()], "color": 1 })).unwrap();
    s.execute("properties.set", &json!({ "handles": [marked.hex()], "layer": "Other" })).unwrap();
    s.execute("properties.set", &json!({ "handles": [marked.hex()], "linetype": "ByBlock" })).unwrap();
    let e = s.doc().unwrap().entity(marked).unwrap();
    assert_eq!(e.common.layer, "Other");
    assert_eq!(e.common.xdata, mark(), "changing properties keeps the mark in the data");
    // MATCHPROP and the layer commands too.
    s.execute("matchprop", &json!({ "source": plain.hex(), "targets": [marked.hex()] })).unwrap();
    assert_eq!(s.doc().unwrap().entity(marked).unwrap().common.layer, "0");
    s.execute("laycur", &json!({ "handles": [marked.hex()] })).unwrap();
    s.execute("laymch", &json!({ "handles": [marked.hex()], "layer": "Other" })).unwrap();
    assert_eq!(s.doc().unwrap().entity(marked).unwrap().common.layer, "Other");
}

/// A drawing whose block "Guided" holds two lines, the Block Editor open on it.
fn editor_on_a_block() -> (Session, Handle, Handle) {
    let (mut s, _, _) = drawing_with_a_marked_line();
    let all: Vec<String> = s.doc().unwrap().model.handles().iter().map(|h| h.hex()).collect();
    s.execute("block", &json!({ "name": "Guided", "base": [0, 0], "handles": all })).unwrap();
    s.execute("bedit", &json!({ "name": "Guided" })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 2);
    (s, handles[0], handles[1])
}

/// Inside the editor the mark still locks color layer and linetype.
#[test]
fn inside_the_editor_the_mark_still_locks_color_layer_and_linetype() {
    let (mut s, regular, guide) = editor_on_a_block();
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    assert!(s.doc().unwrap().block_editor);
    for params in [json!({ "color": 1 }), json!({ "layer": "Other" }), json!({ "linetype": "ByBlock" })] {
        let mut params = params;
        params["handles"] = json!([guide.hex()]);
        assert!(s.execute("properties.set", &params).is_err(), "accepted {params}");
    }
    assert!(s.execute("matchprop", &json!({ "source": regular.hex(), "targets": [guide.hex()] })).is_err());
    assert!(s.execute("laycur", &json!({ "handles": [guide.hex()] })).is_err());
}

/// The editor flag follows the editor and never reaches the drawing.
#[test]
fn the_editor_flag_follows_the_editor_and_never_reaches_the_drawing() {
    let (mut s, _, guide) = editor_on_a_block();
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    assert!(s.doc().unwrap().block_editor, "the editor's drawing");
    assert!(!s.state().unwrap().drawing().block_editor, "the drawing outside it");
    s.execute("bsave", &json!({})).unwrap();
    assert!(s.doc().unwrap().block_editor, "the editor is still the editor after BSAVE");
    assert!(!s.state().unwrap().drawing().block_editor, "BSAVE puts the block in the drawing, not the editor's flag");
    let recovery = s.state().unwrap().recovery_drawing().unwrap();
    assert!(!recovery.block_editor, "a recovery copy is the drawing");
    s.execute("bclose", &json!({})).unwrap();
    assert!(!s.doc().unwrap().block_editor, "closed: an ordinary drawing again");
}

/// The current drawing editor hands back an ordinary drawing.
#[test]
fn the_current_drawing_editor_hands_back_an_ordinary_drawing() {
    let (mut s, _, marked) = drawing_with_a_marked_line();
    s.execute("bedit", &json!({ "name": CURRENT_DRAWING })).unwrap();
    assert!(s.doc().unwrap().block_editor);
    assert!(!s.state().unwrap().drawing().block_editor);
    // Convert in the editor, save and close: the line in the drawing is an ordinary, marked one.
    s.doc_mut().unwrap().modify_entity(marked, |e| e.common.xdata = mark()).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    assert!(!s.state().unwrap().drawing().block_editor);
    s.execute("bclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(!d.block_editor);
    assert!(!d.is_editor_construction(d.entity(marked).unwrap()), "outside the editor the mark has no effect");
    assert_eq!(d.entity(marked).unwrap().common.xdata, mark(), "and it is kept for the next editor session and the file");
    s.execute("properties.set", &json!({ "handles": [marked.hex()], "color": 1 })).unwrap();
}

/// Inside the editor an ignored layer or linetype does not block other properties.
#[test]
fn inside_the_editor_an_ignored_layer_or_linetype_does_not_block_other_properties() {
    let (mut s, _, guide) = editor_on_a_block();
    s.doc_mut().unwrap().modify_entity(guide, |e| e.common.xdata = mark()).unwrap();
    let layer = s.doc().unwrap().entity(guide).unwrap().common.layer.clone();
    // A null layer or linetype is ignored by the writer, so thickness still applies to the marked object.
    s.execute("properties.set", &json!({ "handles": [guide.hex()], "layer": null, "linetype": null, "thickness": 2 })).unwrap();
    let e = s.doc().unwrap().entity(guide).unwrap();
    assert_eq!(e.common.thickness, 2.0);
    assert_eq!(e.common.layer, layer, "a null layer changes nothing");
    assert_eq!(e.common.xdata, mark());
    // Values the writer would apply, and any colour, are still refused.
    for params in [json!({ "layer": "Other" }), json!({ "linetype": "ByBlock" }), json!({ "color": 1 }), json!({ "color": null })] {
        let mut params = params;
        params["handles"] = json!([guide.hex()]);
        assert!(s.execute("properties.set", &params).is_err(), "accepted {params}");
    }
}
