//! Layer commands cannot bypass construction geometry's protected layer property.

use std::sync::Arc;

use cadcraft_engine::doc::{Handle, RawTag};
use cadcraft_engine::{DocState, Session};
use serde_json::json;

fn fixture() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    let mut handles = Vec::new();
    for y in [0, 5] {
        handles.push(s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap()["handles"][0].clone());
    }
    s.execute("block", &json!({ "name": "Guided", "base": [0, 0], "handles": handles })).unwrap();
    s.execute("bedit", &json!({ "name": "Guided" })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 2);
    let (regular, guide) = (handles[0], handles[1]);
    s.doc_mut()
        .unwrap()
        .modify_entity(guide, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }];
        })
        .unwrap();
    s.execute("layer.new", &json!({ "name": "Other", "current": true })).unwrap();
    s.set_selection(vec![regular, guide]);
    (s, regular, guide)
}

fn unchanged(before: &DocState, after: &DocState) {
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert!(Arc::ptr_eq(&before.saved, &after.saved));
    assert!(Arc::ptr_eq(before.drawing(), after.drawing()));
    assert_eq!((before.uid, before.revision), (after.uid, after.revision));
    assert_eq!((&before.selection, &before.previous_selection), (&after.selection, &after.previous_selection));
    assert_eq!(before.block_editor(), after.block_editor());
    assert_eq!(before.block_unsaved(), after.block_unsaved());
    for (old, new) in [(&before.undo, &after.undo), (&before.redo, &after.redo)] {
        assert_eq!(old.len(), new.len());
        for (old, new) in old.iter().zip(new) {
            assert_eq!((&old.label, &old.selection), (&new.label, &new.selection));
            assert!(Arc::ptr_eq(&old.doc, &new.doc));
        }
    }
}

fn layer(s: &Session, h: Handle) -> &str {
    &s.doc().unwrap().entity(h).unwrap().common.layer
}

fn unmark(s: &mut Session, h: Handle) {
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.xdata.clear()).unwrap();
}

#[test]
fn laycur_refuses_mixed_construction_targets_before_changing_any_object() {
    let (mut s, regular, guide) = fixture();
    let before = s.state().unwrap().clone();
    let error = s.execute("laycur", &json!({ "handles": [regular.hex(), guide.hex()] })).unwrap_err().to_string();
    assert!(error.to_ascii_lowercase().contains("construction"), "{error}");
    unchanged(&before, s.state().unwrap());
    assert_eq!((layer(&s, regular), layer(&s, guide)), ("0", "0"));

    assert_eq!(s.execute("laycur", &json!({ "handles": [regular.hex()] })).unwrap()["changed"], 1);
    assert_eq!((layer(&s, regular), layer(&s, guide)), ("Other", "0"));
    unmark(&mut s, guide);
    assert_eq!(s.execute("laycur", &json!({ "handles": [guide.hex()] })).unwrap()["changed"], 1);
    assert_eq!(layer(&s, guide), "Other");
}

#[test]
fn laymch_refuses_mixed_construction_targets_before_creating_the_destination_layer() {
    let (mut s, regular, guide) = fixture();
    let before = s.state().unwrap().clone();
    assert!(s.doc().unwrap().layer("NewDestination").is_none());
    let error = s.execute("laymch", &json!({ "handles": [regular.hex(), guide.hex()], "layer": "NewDestination" })).unwrap_err().to_string();
    assert!(error.to_ascii_lowercase().contains("construction"), "{error}");
    unchanged(&before, s.state().unwrap());
    assert!(s.doc().unwrap().layer("NewDestination").is_none());
    assert_eq!((layer(&s, regular), layer(&s, guide)), ("0", "0"));

    assert_eq!(s.execute("laymch", &json!({ "handles": [regular.hex()], "layer": "OnlyRegular" })).unwrap()["changed"], 1);
    assert_eq!((layer(&s, regular), layer(&s, guide)), ("OnlyRegular", "0"));
    unmark(&mut s, guide);
    assert_eq!(s.execute("laymch", &json!({ "handles": [regular.hex(), guide.hex()], "layer": "NewDestination" })).unwrap()["changed"], 2);
    assert_eq!((layer(&s, regular), layer(&s, guide)), ("NewDestination", "NewDestination"));
}
