//! BLOCK redefinition: no self-referencing blocks, names are case-insensitive (#250).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::{Value, json};

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    s.execute("line", &json!({ "points": [a, b] })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

/// Kinds of the entities in block `name`, as "line"/"insert:<block>".
fn contents(s: &Session, name: &str) -> Vec<String> {
    let b = s.doc().unwrap().block(name).unwrap().clone();
    b.entities
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Insert(i) => format!("insert:{}", i.block),
            EntityKind::Line(_) => "line".to_string(),
            _ => "other".to_string(),
        })
        .collect()
}

#[test]
fn a_block_cannot_be_redefined_to_reference_itself() {
    let mut s = Session::new();
    // Command line: the second -BLOCK selects the converted reference of B plus a new line.
    s.script("LINE 0,0 10,0\n\n-BLOCK B 0,0 L\n\nLINE 0,0 0,10\n\n-BLOCK B 0,0 ALL\n\n").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l.contains("references itself")), "{:?}", s.log);
    assert_eq!(contents(&s, "B"), ["line"], "B keeps its original definition");
    assert_eq!(s.doc().unwrap().model.len(), 2, "the reference and the new line stay in the drawing");

    // Through a nested block: A holds B's reference, B holds A's.
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let a = s.execute("block", &json!({ "name": "A", "base": [0, 0], "handles": [l] })).unwrap()["insert"].clone();
    let l = line(&mut s, [0.0, 0.0], [0.0, 10.0]);
    let b = s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [a, l] })).unwrap()["insert"].clone();
    let r = s.execute("block", &json!({ "name": "a", "base": [0, 0], "handles": [b] }));
    assert!(r.is_err_and(|e| e.to_string().contains("references itself")));
    assert_eq!(contents(&s, "A"), ["line"]);
}

#[test]
fn redefining_with_a_different_case_replaces_the_block() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("block", &json!({ "name": "Door", "base": [0, 0], "handles": [l] })).unwrap();
    let l = line(&mut s, [0.0, 0.0], [0.0, 10.0]);
    s.execute("block", &json!({ "name": "DOOR", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
    let list = s.execute("blocks.list", &json!({})).unwrap();
    let names: Vec<&str> = list["blocks"].as_array().unwrap().iter().filter_map(|b| b["name"].as_str()).collect();
    assert_eq!(names, ["Door"]);
    assert_eq!(s.doc().unwrap().blocks.values().map(|b| b.entities.len()).collect::<Vec<_>>(), [1]);
    let v: Vec<Value> = s.doc().unwrap().block("Door").unwrap().entities.iter().map(|e| serde_json::to_value(&e.kind).unwrap()).collect();
    assert_eq!(v[0]["b"]["y"], json!(10.0), "the new definition replaced the old one");
}
