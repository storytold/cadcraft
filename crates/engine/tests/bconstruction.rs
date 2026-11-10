//! BCONSTRUCTION changes block geometry without changing its identity or ordinary properties.

use std::sync::Arc;

use cadcraft_engine::doc::{Handle, RawTag};
use cadcraft_engine::{DocState, Input, Session};
use serde_json::{Value, json};

fn marked(s: &Session, h: Handle) -> bool {
    s.doc().unwrap().model.get(h).unwrap().common.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case("ACAD_BCONSTRUCTION"))
}

/// Two lines in a named block with a nonzero base point, opened for editing.
fn editor() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    let mut handles = Vec::new();
    for y in [5, 9] {
        let out = s.execute("line", &json!({ "points": [[5, y], [15, y]] })).unwrap();
        handles.push(out["handles"][0].clone());
    }
    s.execute("block", &json!({ "name": "Frame", "base": [5, 5], "handles": handles, "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "Frame", "at": [100, 20] })).unwrap();
    s.execute("bedit", &json!({ "name": "Frame" })).unwrap();
    let handles = s.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 2);
    (s, handles)
}

fn run(s: &mut Session, mode: &str, handles: &[Handle]) -> Value {
    s.execute("bconstruction", &json!({ "mode": mode, "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap()
}

fn unchanged(before: &DocState, after: &DocState) {
    assert!(Arc::ptr_eq(&before.doc, &after.doc), "editor drawing retained");
    assert!(Arc::ptr_eq(&before.saved, &after.saved), "saved drawing retained");
    assert!(Arc::ptr_eq(before.drawing(), after.drawing()), "host drawing retained");
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

#[test]
fn convert_revert_and_undo_preserve_identity_geometry_properties_and_other_xdata() {
    let (mut s, hs) = editor();
    let extra = vec![RawTag { code: 1001, value: "CADCRAFT_TEST".into() }, RawTag { code: 1000, value: "keep this".into() }];
    s.doc_mut()
        .unwrap()
        .modify_entity(hs[0], |e| {
            e.common.xdata = extra.clone();
            e.common.ltscale = 2.5;
            e.common.thickness = 0.75;
        })
        .unwrap();
    s.execute("layer.new", &json!({ "name": "Different", "current": true })).unwrap();
    let original = s.doc().unwrap().model.get(hs[0]).unwrap().as_ref().clone();
    let undo = s.state().unwrap().undo.len();
    let out = run(&mut s, "convert", &[hs[0], hs[0]]);
    assert_eq!((out["changed"].as_u64(), out["construction"].as_u64()), (Some(1), Some(1)));
    assert!(marked(&s, hs[0]));
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one conversion step");
    let converted = s.doc().unwrap().model.get(hs[0]).unwrap();
    assert_eq!(converted.handle, original.handle);
    assert_eq!(converted.kind, original.kind);
    let mut common = converted.common.clone();
    common.xdata = original.common.xdata.clone();
    assert_eq!(common, original.common);

    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.get(hs[0]).unwrap().as_ref(), &original);
    s.redo().unwrap();
    assert!(marked(&s, hs[0]));
    assert_eq!(run(&mut s, "revert", &[hs[0]])["changed"], 1);
    let mut reverted = original.clone();
    reverted.common.layer = "Different".into();
    assert_eq!(s.doc().unwrap().model.get(hs[0]).unwrap().as_ref(), &reverted, "Revert changes only construction metadata and the current layer");
    s.undo().unwrap();
    assert!(marked(&s, hs[0]));
    assert_eq!(s.doc().unwrap().model.get(hs[0]).unwrap().common.layer, original.common.layer, "Undo restores the original layer");
    s.redo().unwrap();
    assert!(!marked(&s, hs[0]));
    assert_eq!(s.doc().unwrap().model.get(hs[0]).unwrap().as_ref(), &reverted);
}

#[test]
fn repeated_conversion_and_revert_are_noops_and_keep_redo_history() {
    let (mut s, hs) = editor();
    run(&mut s, "convert", &[hs[0]]);
    s.execute("line", &json!({ "points": [[0, 2], [3, 2]] })).unwrap();
    s.undo().unwrap();
    let before = s.state().unwrap().clone();
    assert!(!before.redo.is_empty());
    assert_eq!(run(&mut s, "convert", &[hs[0]])["changed"], 0);
    unchanged(&before, s.state().unwrap());
    assert_eq!(run(&mut s, "revert", &[hs[1]])["changed"], 0);
    unchanged(&before, s.state().unwrap());
}

#[test]
fn malformed_or_foreign_targets_fail_atomically_without_history_changes() {
    let (mut s, hs) = editor();
    s.set_selection(vec![hs[0]]);
    let mut cases = vec![
        json!({}),
        json!({ "mode": "sideways", "handles": [hs[0].hex()] }),
        json!({ "mode": "convert", "handles": [hs[0].hex(), "FFFFFFFFFFFFFFFE"] }),
        json!({ "mode": "convert", "handles": [hs[0].hex(), {}] }),
        json!({ "mode": "convert", "handles": "bad" }),
        json!({ "mode": "convert", "handles": [hs[0].hex(), -1] }),
        json!({ "mode": "convert", "handles": [] }),
    ];
    // Bounds are checked before allocating or mutating a caller-sized selection.
    cases.push(json!({ "mode": "convert", "handles": vec![hs[0].hex(); 65_537] }));
    for params in cases {
        let before = s.state().unwrap().clone();
        assert!(s.execute("bconstruction", &params).is_err(), "accepted {params}");
        unchanged(&before, s.state().unwrap());
        assert!(!marked(&s, hs[0]));
    }
    let before = s.state().unwrap().clone();
    assert_eq!(s.execute("bconstruction", &json!({ "mode": "convert" })).unwrap()["changed"], 1, "absent handles use the selection");
    assert!(marked(&s, hs[0]));
    assert_eq!(s.state().unwrap().undo.len(), before.undo.len() + 1);
}

#[test]
fn outside_the_editor_every_mode_is_refused_by_the_shared_enabled_check() {
    let mut s = Session::new();
    let before = s.state().unwrap().clone();
    for mode in ["convert", "revert", "show", "hide"] {
        let error = s.execute("bconstruction", &json!({ "mode": mode })).unwrap_err().to_string();
        assert!(error.to_ascii_lowercase().contains("block editor"), "{error}");
        unchanged(&before, s.state().unwrap());
    }
    for name in ["BCONSTRUCTION", "_.BCONSTRUCTION"] {
        let error = s.start(name).unwrap_err().to_string();
        assert!(error.to_ascii_lowercase().contains("block editor"), "{error}");
        assert!(s.running.is_none());
        unchanged(&before, s.state().unwrap());
    }
}

#[test]
fn typed_convert_revert_and_cancel_use_the_existing_selection_prompt() {
    let (mut s, hs) = editor();
    let before = s.state().unwrap().clone();
    s.start("BCONSTRUCTION").unwrap();
    assert!(s.prompt_text().contains("Convert"));
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
    unchanged(&before, s.state().unwrap());

    s.start("BCONSTRUCTION").unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.prompt_text().contains("Select objects"));
    s.input(Input::Pick(vec![hs[0]])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert!(marked(&s, hs[0]));
    assert!(!marked(&s, hs[1]));

    s.set_selection(vec![hs[0]]);
    s.start("_.BCONSTRUCTION").unwrap();
    s.input(Input::Text("Revert".into())).unwrap();
    if s.running.is_some() {
        s.input(Input::Enter).unwrap();
    }
    assert!(s.running.is_none());
    assert!(!marked(&s, hs[0]));
}

#[test]
fn nonzero_base_save_copy_close_and_reopen_keep_construction_metadata() {
    let (mut s, hs) = editor();
    let editor_kind = s.doc().unwrap().model.get(hs[0]).unwrap().kind.clone();
    run(&mut s, "convert", &[hs[0]]);
    s.execute("bsave", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.get(hs[0]).unwrap().kind, editor_kind);
    let original_definition = s.state().unwrap().drawing().block("Frame").unwrap().as_ref().clone();
    assert!(original_definition.entities.get(hs[0]).unwrap().common.xdata.iter().any(|t| t.value == "ACAD_BCONSTRUCTION"));
    s.execute("bsaveas", &json!({ "name": "Copy" })).unwrap();
    assert_eq!(s.state().unwrap().block_editor(), Some("Copy"));
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().block("Frame").unwrap().as_ref(), &original_definition);
    s.execute("bedit", &json!({ "name": "Frame" })).unwrap();
    assert!(marked(&s, hs[0]));
    assert_eq!(s.doc().unwrap().model.get(hs[0]).unwrap().kind, editor_kind);
    run(&mut s, "revert", &[hs[0]]);
    s.execute("bclose", &json!({ "save": false })).unwrap();
    s.execute("bedit", &json!({ "name": "Frame" })).unwrap();
    assert!(marked(&s, hs[0]), "discarded Revert leaves saved definition intact");
}

fn hidden(s: &Session, h: Handle) -> bool {
    s.doc().unwrap().hidden_objects.contains(&h)
}

#[test]
fn hide_show_and_history_restore_the_same_view_mode() {
    let (mut s, hs) = editor();
    assert_eq!(s.execute("bconstruction", &json!({ "mode": "hide" })).unwrap()["construction"], 0);
    run(&mut s, "convert", &[hs[0]]);
    assert!(hidden(&s, hs[0]), "objects converted while Hide All is active are hidden");
    assert!(!hidden(&s, hs[1]));
    s.undo().unwrap();
    assert!(!marked(&s, hs[0]) && !hidden(&s, hs[0]));
    s.redo().unwrap();
    assert!(marked(&s, hs[0]) && hidden(&s, hs[0]));
    let before = s.state().unwrap().clone();
    s.execute("bconstruction", &json!({ "mode": "hide" })).unwrap();
    unchanged(&before, s.state().unwrap());

    s.execute("bconstruction", &json!({ "mode": "show" })).unwrap();
    assert!(!hidden(&s, hs[0]));
    s.undo().unwrap();
    assert!(hidden(&s, hs[0]));
    s.redo().unwrap();
    assert!(!hidden(&s, hs[0]), "the restored Show All mode");
    let before = s.state().unwrap().clone();
    s.execute("bconstruction", &json!({ "mode": "show" })).unwrap();
    unchanged(&before, s.state().unwrap());
}

#[test]
fn current_drawing_save_keeps_construction_but_does_not_publish_the_editor_hide_cache() {
    let mut s = Session::new();
    let out = s.execute("line", &json!({ "points": [[1, 2], [6, 2]] })).unwrap();
    let h = Handle::parse_hex(out["handles"][0].as_str().unwrap()).unwrap();
    let geometry = s.doc().unwrap().model.get(h).unwrap().kind.clone();
    s.execute("bedit", &json!({ "name": "<Current Drawing>" })).unwrap();
    run(&mut s, "convert", &[h]);
    s.execute("bconstruction", &json!({ "mode": "hide" })).unwrap();
    assert!(hidden(&s, h));
    s.execute("bsave", &json!({})).unwrap();
    assert!(marked(&s, h) && hidden(&s, h), "the editor keeps its view mode after saving");
    assert!(!s.state().unwrap().drawing().hidden_objects.contains(&h), "the saved drawing has no temporary construction hide cache");
    s.execute("bclose", &json!({})).unwrap();
    assert!(s.state().unwrap().block_editor().is_none());
    assert!(marked(&s, h) && !hidden(&s, h));
    assert_eq!(s.doc().unwrap().model.get(h).unwrap().kind, geometry);
    s.execute("bedit", &json!({ "name": "<Current Drawing>" })).unwrap();
    assert!(marked(&s, h) && !hidden(&s, h), "a fresh editor starts with construction shown");
}
