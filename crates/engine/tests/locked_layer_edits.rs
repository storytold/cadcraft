//! Property, table, block and array edits must respect source-layer locks (#936).

use std::sync::Arc;

use cadcraft_engine::doc::{Drawing, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

struct Snapshot {
    drawing: Arc<Drawing>,
    undo: usize,
    redo: usize,
    revision: u64,
}

impl Snapshot {
    fn new(s: &Session) -> Self {
        let st = s.state().unwrap();
        Self { drawing: st.doc.clone(), undo: st.undo.len(), redo: st.redo.len(), revision: st.revision }
    }

    fn assert_unchanged(&self, s: &Session) {
        let st = s.state().unwrap();
        assert!(Arc::ptr_eq(&self.drawing, &st.doc), "the drawing was touched by a refused edit");
        assert_eq!((st.undo.len(), st.redo.len(), st.revision), (self.undo, self.redo, self.revision));
    }
}

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.as_str().unwrap()).unwrap()
}

fn lock(s: &mut Session, name: &str, locked: bool) {
    s.execute("layer.set", &json!({"name": name, "locked": locked})).unwrap();
}

fn circle(s: &mut Session, x: f64) -> Value {
    s.execute("circle", &json!({"center": [x, 0], "radius": 1})).unwrap()["handle"].clone()
}

fn mixed_circles(s: &mut Session) -> (Value, Value) {
    let protected = circle(s, 0.0);
    s.execute("layer.new", &json!({"name": "Editable"})).unwrap();
    s.execute("layer.current", &json!({"name": "Editable"})).unwrap();
    let editable = circle(s, 10.0);
    lock(s, "0", true);
    (protected, editable)
}

fn refuse(s: &mut Session, id: &str, p: &Value) {
    let before = Snapshot::new(s);
    let selection = s.selection();
    let e = s.execute(id, p).unwrap_err();
    assert!(e.to_string().contains("locked layer"), "{id}: {e}");
    before.assert_unchanged(s);
    assert_eq!(s.selection(), selection);
}

fn edit_undo_redo(s: &mut Session, id: &str, p: &Value) {
    let before = s.state().unwrap().doc.clone();
    s.execute(id, p).unwrap();
    let after = s.state().unwrap().doc.clone();
    assert!(!Arc::ptr_eq(&before, &after), "{id} must edit the unlocked object");
    s.execute("undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc), "undo {id}");
    s.execute("redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&after, &s.state().unwrap().doc), "redo {id}");
}

#[test]
fn properties_refuse_locked_and_mixed_targets_before_creating_a_destination_layer() {
    for selected in [false, true] {
        for reversed in [false, true] {
            let mut s = Session::new();
            let (a, b) = mixed_circles(&mut s);
            let hs = if reversed { vec![b, a.clone()] } else { vec![a.clone(), b] };
            let mut p = json!({"layer": "NewDestination", "radius": 4, "color": "red"});
            if selected {
                s.set_selection(hs.iter().map(handle).collect());
            } else {
                p["handles"] = json!(hs);
            }
            refuse(&mut s, "properties.set", &p);
            assert!(s.doc().unwrap().layer("NewDestination").is_none());
            refuse(&mut s, "properties.set", &json!({"handles": [a], "visible": false}));
        }
    }
}

#[test]
fn unlocked_properties_edit_and_undo_redo_still_work() {
    let mut s = Session::new();
    let (_, b) = mixed_circles(&mut s);
    edit_undo_redo(&mut s, "properties.set", &json!({"handles": [b], "radius": 3, "color": "green"}));
    assert!(matches!(&s.doc().unwrap().entity(handle(&b)).unwrap().kind, EntityKind::Circle(c) if c.radius == 3.0));
}

fn table(s: &mut Session) -> Value {
    s.execute("table", &json!({"at": [0, 0], "rows": 3, "cols": 3, "cells": [["1", "2", "3"]]})).unwrap()["handle"].clone()
}

#[test]
fn table_mutations_refuse_explicit_and_selected_locked_tables() {
    let cases = [
        ("table.set", json!({"row": 0, "col": 0, "text": "changed"})),
        ("table.set", json!({"row": 0, "col": 1, "formula": "=A1*2"})),
        ("table.insertrow", json!({"row": 1})),
        ("table.deleterow", json!({"row": 1})),
        ("table.insertcol", json!({"col": 1})),
        ("table.deletecol", json!({"col": 1})),
        ("table.merge", json!({"row": 0, "col": 0, "rows": 2, "cols": 2})),
        ("table.unmerge", json!({"row": 0, "col": 0})),
        ("table.cellstyle", json!({"row": 0, "col": 0, "style": "Header"})),
        ("table.formula", json!({"row": 0, "col": 1, "function": "cell", "cell": "A1"})),
        ("table.break", json!({"enabled": true, "height": 1})),
        ("datalink", json!({"remove": true})),
    ];
    for selected in [false, true] {
        let mut s = Session::new();
        let h = table(&mut s);
        s.execute("table.merge", &json!({"handle": h, "row": 0, "col": 0, "rows": 2, "cols": 2})).unwrap();
        lock(&mut s, "0", true);
        for (id, params) in &cases {
            let mut p = params.clone();
            if selected {
                s.set_selection(vec![handle(&h)]);
            } else {
                // Numeric handles are accepted by the shared table editor too.
                p["handle"] = json!(handle(&h).0);
            }
            refuse(&mut s, id, &p);
        }
        lock(&mut s, "0", false);
        edit_undo_redo(&mut s, "table.set", &json!({"handle": h, "row": 0, "col": 0, "text": "unlocked"}));
    }
}

#[test]
fn interactive_tabledit_refuses_a_locked_cell() {
    let mut s = Session::new();
    table(&mut s);
    lock(&mut s, "0", true);
    let before = Snapshot::new(&s);
    s.start("tabledit").unwrap();
    s.input(Input::Point(Vec2::new(0.5, -0.1))).unwrap();
    s.input(Input::Text("changed".into())).unwrap();
    assert!(s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
    before.assert_unchanged(&s);
}

#[test]
fn block_conversion_and_deletion_refuse_mixed_sources_atomically() {
    for id in ["block", "-block"] {
        for keep in ["convert", "delete"] {
            for selected in [false, true] {
                for reversed in [false, true] {
                    let mut s = Session::new();
                    let (a, b) = mixed_circles(&mut s);
                    let hs = if reversed { vec![b, a] } else { vec![a, b] };
                    let mut p = json!({"name": "Protected", "base": [0, 0], "keep": keep, "annotative": true});
                    if selected {
                        s.set_selection(hs.iter().map(handle).collect());
                    } else {
                        p["handles"] = json!(hs);
                    }
                    refuse(&mut s, id, &p);
                    assert!(s.doc().unwrap().block("Protected").is_none());
                    lock(&mut s, "0", false);
                    edit_undo_redo(&mut s, id, &p);
                }
            }
        }
    }
}

#[test]
fn block_retain_can_copy_a_locked_source_without_changing_it() {
    let mut s = Session::new();
    let (a, b) = mixed_circles(&mut s);
    let original = s.doc().unwrap().entity(handle(&a)).unwrap().clone();
    edit_undo_redo(&mut s, "block", &json!({"name": "Copy", "handles": [a, b], "keep": "retain"}));
    assert!(Arc::ptr_eq(&original, s.doc().unwrap().entity(handle(&a)).unwrap()));
    assert_eq!(s.doc().unwrap().model.len(), 2);
    assert_eq!(s.doc().unwrap().block("Copy").unwrap().entities.len(), 2);
}

#[test]
fn interactive_block_and_dash_block_refuse_locked_preselection() {
    for id in ["block", "-block"] {
        let mut s = Session::new();
        let (a, b) = mixed_circles(&mut s);
        s.set_selection(vec![handle(&a), handle(&b)]);
        let before = Snapshot::new(&s);
        s.start(id).unwrap();
        s.input(Input::Text("Protected".into())).unwrap();
        s.input(Input::Point(Vec2::ZERO)).unwrap();
        assert!(s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
        before.assert_unchanged(&s);
    }
}

fn array(s: &mut Session) -> Value {
    let h = circle(s, 0.0);
    s.execute("arrayrect", &json!({"handles": [h], "rows": 1, "cols": 3, "colSpacing": 2})).unwrap()["handle"].clone()
}

#[test]
fn arrayedit_refuses_every_locked_mutation_without_creating_blocks_or_source_objects() {
    let mut s = Session::new();
    let a = array(&mut s);
    let replacement = circle(&mut s, 20.0);
    lock(&mut s, "0", true);
    for p in [
        json!({"handle": a, "cols": 4}),
        json!({"handle": a, "base": [5, 5]}),
        json!({"handle": a, "reset": true}),
        json!({"handle": a, "replace": {"handles": [replacement], "items": [[1, 0, 0]]}}),
        json!({"handle": a, "source": {}}),
    ] {
        refuse(&mut s, "arrayedit", &p);
    }
    lock(&mut s, "0", false);
    edit_undo_redo(&mut s, "arrayedit", &json!({"handle": a, "cols": 4}));
}

#[test]
fn unlocked_array_can_copy_locked_replacement_objects() {
    let mut s = Session::new();
    let a = array(&mut s);
    s.execute("properties.set", &json!({"handles": [a], "layer": "Editable"})).unwrap();
    let replacement = circle(&mut s, 20.0);
    let original = s.doc().unwrap().entity(handle(&replacement)).unwrap().clone();
    lock(&mut s, "0", true);
    edit_undo_redo(&mut s, "arrayedit", &json!({"handle": a, "replace": {"handles": [replacement], "items": [[1, 0, 0]]}}));
    assert!(Arc::ptr_eq(&original, s.doc().unwrap().entity(handle(&replacement)).unwrap()));
}

#[test]
fn arrayclose_refuses_saving_if_the_array_was_locked_after_source_editing_began() {
    let mut s = Session::new();
    let a = array(&mut s);
    s.execute("properties.set", &json!({"handles": [a], "layer": "Protected"})).unwrap();
    s.execute("arrayedit", &json!({"handle": a, "source": {}})).unwrap();
    lock(&mut s, "Protected", true);
    refuse(&mut s, "arrayclose", &json!({"save": true}));
    // Discarding is still possible: the temporary source objects are on unlocked layer 0.
    s.execute("arrayclose", &json!({"save": false})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
}

#[test]
fn arrayclose_validates_every_source_object_before_saving_or_discarding() {
    for save in [false, true] {
        let mut s = Session::new();
        let a = array(&mut s);
        s.execute("properties.set", &json!({"handles": [a], "layer": "Editable"})).unwrap();
        s.execute("arrayedit", &json!({"handle": a, "source": {}})).unwrap();
        // An extra object made during editing is also removed on close.
        circle(&mut s, 10.0);
        lock(&mut s, "0", true);
        refuse(&mut s, "arrayclose", &json!({"save": save}));
        lock(&mut s, "0", false);
        edit_undo_redo(&mut s, "arrayclose", &json!({"save": save}));
    }
}

#[test]
fn interactive_arrayedit_refuses_locked_preselection_and_picks() {
    for preselected in [false, true] {
        let mut s = Session::new();
        let a = array(&mut s);
        lock(&mut s, "0", true);
        if preselected {
            s.set_selection(vec![handle(&a)]);
        }
        let before = Snapshot::new(&s);
        let _ = s.start("arrayedit");
        if !preselected {
            let _ = s.input(Input::Pick(vec![handle(&a)]));
        }
        assert!(s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
        before.assert_unchanged(&s);
    }
}

#[test]
fn refused_edits_preserve_an_existing_redo_step() {
    let mut s = Session::new();
    let a = array(&mut s);
    let t = table(&mut s);
    let c = circle(&mut s, 20.0);
    lock(&mut s, "0", true);
    lock(&mut s, "0", false);
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.state().unwrap().redo.is_empty());
    for (id, p) in [
        ("properties.set", json!({"handles": [c], "radius": 2})),
        ("table.set", json!({"handle": t, "row": 0, "col": 0, "text": "changed"})),
        ("block", json!({"name": "Protected", "handles": [c]})),
        ("arrayedit", json!({"handle": a, "cols": 4})),
    ] {
        refuse(&mut s, id, &p);
    }
    s.execute("redo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().layer("0").unwrap().locked);
}
