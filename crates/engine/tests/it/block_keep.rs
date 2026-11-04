//! BLOCK's `keep` parameter takes convert, retain or delete, in any case. Anything else is an
//! error that changes nothing (issue #935: a misspelt value erased the objects without making a
//! reference to the new block).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn line(s: &mut Session) -> Value {
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone()
}

fn references(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Insert(_))).count()
}

/// Everything a refused BLOCK must leave alone: the objects, the blocks, the selection and the
/// undo history.
fn state(s: &Session) -> (Vec<String>, Vec<String>, Vec<String>, usize) {
    let d = s.doc().unwrap();
    let objects = d.model.iter().map(|e| format!("{:?}", e.kind)).collect();
    let blocks = d.blocks.keys().cloned().collect();
    let selection = s.selection().iter().map(|h| h.hex()).collect();
    (objects, blocks, selection, s.state().unwrap().undo.len())
}

#[test]
fn a_value_that_is_not_documented_is_an_error_and_changes_nothing() {
    let mut s = Session::new();
    let h = line(&mut s);
    s.set_selection(vec![Handle::parse_hex(h.as_str().unwrap()).unwrap()]);
    for (cmd, keep) in [("block", "retian"), ("block", ""), ("block", "erase"), ("-block", "convert-all"), ("-block", "keep")] {
        let before = state(&s);
        let err = s.execute(cmd, &json!({ "name": "B", "base": [0, 0], "handles": [h], "keep": keep })).unwrap_err();
        assert!(err.to_string().contains("`keep`"), "{cmd} {keep:?}: {err}");
        assert_eq!(state(&s), before, "{cmd} {keep:?} changed the drawing");
        assert!(s.doc().unwrap().block("B").is_none(), "{cmd} {keep:?} defined the block");
    }
    assert_eq!(s.doc().unwrap().model.len(), 1, "the line is still there");
    assert_eq!(references(&s), 0);
    assert_eq!(s.selection().len(), 1, "the line is still selected");
}

#[test]
fn the_documented_values_work_in_any_case() {
    let mut s = Session::new();
    let h = line(&mut s);
    let r = s.execute("block", &json!({ "name": "R", "base": [0, 0], "handles": [h], "keep": "Retain" })).unwrap();
    assert!(r["insert"].is_null());
    assert_eq!(s.doc().unwrap().model.len(), 1, "the line stays");
    assert!(s.doc().unwrap().block("R").is_some());

    let h = line(&mut s);
    let r = s.execute("block", &json!({ "name": "D", "base": [0, 0], "handles": [h], "keep": "DELETE" })).unwrap();
    assert!(r["insert"].is_null());
    assert_eq!(s.doc().unwrap().model.len(), 1, "the second line is erased");

    let h = line(&mut s);
    let r = s.execute("-block", &json!({ "name": "C", "base": [0, 0], "handles": [h], "keep": " convert " })).unwrap();
    assert!(r["insert"].is_string());
    assert_eq!(references(&s), 1);
    assert_eq!(s.doc().unwrap().model.len(), 2, "the retained line and C's reference");
}
