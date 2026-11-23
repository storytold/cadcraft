//! Construction view operations preserve independent object hiding and reference boundaries.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, RawTag};
use serde_json::json;

/// Draws a horizontal line at height `y` and returns its handle.
fn line(s: &mut Session, y: i32) -> Handle {
    let result = s.execute("line", &json!({ "points": [[0, y], [100, y]] })).unwrap();
    Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap()
}

/// A Block Editor on a block of two lines. Returns the session and the two lines.
fn editor() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    let a = line(&mut s, 0);
    let b = line(&mut s, 10);
    s.execute("block", &json!({ "name": "ConstructionView", "base": [0, 0], "handles": [a.hex(), b.hex()], "keep": "delete" })).unwrap();
    s.execute("bedit", &json!({ "name": "ConstructionView" })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 2);
    (s, handles[0], handles[1])
}

/// Runs BCONSTRUCTION in `mode` on `h` alone.
fn construction(s: &mut Session, mode: &str, h: Handle) {
    s.execute("bconstruction", &json!({ "mode": mode, "handles": [h.hex()] })).unwrap();
}

/// Asserts that `h` is still hidden by HIDEOBJECTS after `operation`.
fn remains_hidden(s: &Session, h: Handle, operation: &str) {
    let d = s.doc().unwrap();
    assert!(d.hidden_objects.contains(&h), "{operation} removed unrelated HIDEOBJECTS state");
    assert!(!d.is_visible(d.model.get(h).unwrap()), "{operation} revealed an independently hidden object");
}

/// Construction operations preserve an independently hidden regular object.
#[test]
fn construction_operations_preserve_an_independently_hidden_regular_object() {
    let (mut s, guide, regular) = editor();
    s.execute("hideobjects", &json!({ "handles": [regular.hex()] })).unwrap();
    remains_hidden(&s, regular, "setup");
    for operation in ["convert", "show", "hide", "revert"] {
        construction(&mut s, operation, guide);
        remains_hidden(&s, regular, operation);
    }
    let d = s.doc().unwrap();
    assert!(d.is_visible(d.model.get(guide).unwrap()), "Revert reveals the object formerly hidden only by construction mode");
}

/// A construction noop preserves independent hiding document identity and history.
#[test]
fn a_construction_noop_preserves_independent_hiding_document_identity_and_history() {
    let (mut s, guide, regular) = editor();
    construction(&mut s, "convert", guide);
    s.execute("hideobjects", &json!({ "handles": [regular.hex()] })).unwrap();
    line(&mut s, 20);
    s.undo().unwrap();
    let before = s.state().unwrap().clone();
    assert!(!before.redo.is_empty());
    construction(&mut s, "convert", guide);
    let after = s.state().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc), "a repeated conversion must not rewrite unrelated hidden state");
    assert!(Arc::ptr_eq(before.drawing(), after.drawing()));
    assert_eq!((before.undo.len(), before.redo.len()), (after.undo.len(), after.redo.len()));
    assert_eq!(before.revision, after.revision);
    remains_hidden(&s, regular, "no-op Convert");
}

/// Exploding an associative array does not materialize construction sources.
#[test]
fn exploding_an_associative_array_does_not_materialize_construction_sources() {
    let mut s = Session::new();
    let regular = line(&mut s, 0);
    let guide = line(&mut s, 10);
    s.doc_mut()
        .unwrap()
        .modify_entity(guide, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }];
        })
        .unwrap();
    let made = s
        .execute("arrayrect", &json!({ "handles": [regular.hex(), guide.hex()], "rows": 1, "cols": 2, "colSpacing": 20, "associative": true }))
        .unwrap();
    assert_eq!(made["associative"], true);
    s.execute("explode", &json!({ "handles": [made["handle"]] })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.len(), 2, "only the two ordinary source copies may become model geometry");
    for e in d.model.iter() {
        assert!(!e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "ACAD_BCONSTRUCTION"));
        let EntityKind::Line(segment) = &e.kind else { panic!("the ordinary lines must remain") };
        assert_eq!((segment.a.y, segment.b.y), (0.0, 0.0));
    }
    assert!(
        d.blocks.values().any(|b| b.entities.iter().any(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "ACAD_BCONSTRUCTION"))),
        "the source definition keeps its construction metadata"
    );
}

/// Whether HIDEOBJECTS hides `h`.
fn user_hid(s: &Session, h: Handle) -> bool {
    s.doc().unwrap().hidden_objects.contains(&h)
}

/// Hide all and show all never reveal a construction object the user hid.
#[test]
fn hide_all_and_show_all_never_reveal_a_construction_object_the_user_hid() {
    // Hidden by HIDEOBJECTS first, then Hide All and Show All.
    let (mut s, guide, _) = editor();
    construction(&mut s, "convert", guide);
    s.execute("hideobjects", &json!({ "handles": [guide.hex()] })).unwrap();
    for operation in ["hide", "show", "hide", "show"] {
        construction(&mut s, operation, guide);
        remains_hidden(&s, guide, operation);
    }
    // Hidden by HIDEOBJECTS while Hide All is already on.
    let (mut s, guide, _) = editor();
    construction(&mut s, "convert", guide);
    construction(&mut s, "hide", guide);
    s.execute("hideobjects", &json!({ "handles": [guide.hex()] })).unwrap();
    construction(&mut s, "show", guide);
    remains_hidden(&s, guide, "Show all after HIDEOBJECTS under Hide all");
    // Ending the isolation brings it back, and Hide All still hides it.
    s.execute("unisolateobjects", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.is_visible(d.model.get(guide).unwrap()), "UNISOLATE shows the construction object again while Show all is on");
    construction(&mut s, "hide", guide);
    let d = s.doc().unwrap();
    assert!(!d.is_visible(d.model.get(guide).unwrap()));
    assert!(!user_hid(&s, guide), "Hide all alone is not object isolation");
}
