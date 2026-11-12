//! -BLOCK erases the selected objects, and asks before redefining a block (issue #307). BLOCK
//! (the menu's Make...) and the JSON form still convert the selection to a block reference.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn prompt(s: &Session) -> String {
    s.current_prompt().map(|p| p.message).unwrap_or_default()
}

fn references(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Insert(_))).count()
}

#[test]
fn dash_block_erases_the_selection_and_asks_before_redefining() {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0\n\n-BLOCK B 0,0 L\n\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.len(), 0, "the line is erased, not converted");
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 1);
    s.execute("insert", &json!({ "name": "B", "at": [20, 0] })).unwrap();
    assert_eq!(references(&s), 1, "INSERT after -BLOCK leaves one reference");

    // An existing name (in any case): No, the default, asks for the name again.
    s.script("LINE 0,0 0,10\n\n").unwrap();
    s.cmdline("-block").unwrap();
    s.cmdline("b").unwrap();
    assert_eq!(prompt(&s), "Block \"b\" already exists. Redefine it?");
    s.cmdline("").unwrap();
    assert_eq!(prompt(&s), "Enter block name");
    s.cmdline("b").unwrap();
    s.cmdline("y").unwrap();
    assert_eq!(prompt(&s), "Specify insertion base point");
    s.script("5,5\nL\n\n").unwrap();
    assert!(s.running.is_none());
    let d = s.doc().unwrap();
    assert_eq!(d.blocks.keys().collect::<Vec<_>>(), ["B"]);
    let b = d.block("B").unwrap();
    assert!((b.base.x - 5.0).abs() < 1e-12 && (b.base.y - 5.0).abs() < 1e-12);
    assert!(matches!(b.entities.iter().next().map(|e| &e.kind), Some(EntityKind::Line(l)) if (l.b.y - 10.0).abs() < 1e-12), "B was redefined");
    assert_eq!(d.model.len(), 1, "the vertical line is erased; only the reference remains");
}

#[test]
fn block_and_the_json_forms_keep_their_defaults() {
    let mut s = Session::new();
    // BLOCK converts the selection to a reference.
    s.script("LINE 0,0 10,0\n\nBLOCK A 0,0 L\n\n").unwrap();
    assert_eq!(references(&s), 1);
    // JSON: block converts unless `keep` says otherwise; -block deletes unless it says otherwise.
    let l = |s: &mut Session| s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].clone();
    let h = l(&mut s);
    let r = s.execute("block", &json!({ "name": "C", "base": [0, 0], "handles": [h] })).unwrap();
    assert!(r["insert"].is_string());
    let h = l(&mut s);
    let r = s.execute("-block", &json!({ "name": "D", "base": [0, 0], "handles": [h] })).unwrap();
    assert!(r["insert"].is_null());
    let h = l(&mut s);
    s.execute("-block", &json!({ "name": "E", "base": [0, 0], "handles": [h], "keep": "retain" })).unwrap();
    assert_eq!(references(&s), 2);
    assert_eq!(s.doc().unwrap().model.len(), 3, "A and C references, E's retained line");
}
