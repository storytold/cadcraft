//! Saving an edited block as a copy must retain its source's last saved definition.

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn save_as_preserves_the_last_saved_source_definition() {
    let mut s = Session::new();
    let mut handles = Vec::new();
    for y in [5, 9] {
        let line = s.execute("line", &json!({ "points": [[5, y], [15, y]] })).unwrap();
        handles.push(line["handles"][0].clone());
    }
    s.execute("block", &json!({ "name": "Frame", "base": [5, 5], "handles": handles, "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "Frame", "at": [100, 20] })).unwrap();
    s.execute("bedit", &json!({ "name": "Frame" })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2);

    s.execute("line", &json!({ "points": [[0, 3], [3, 3]] })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    let saved_source = s.state().unwrap().drawing().block("Frame").unwrap().as_ref().clone();
    assert_eq!(saved_source.entities.len(), 3);
    s.execute("bsaveas", &json!({ "name": "Copy" })).unwrap();
    assert_eq!(s.state().unwrap().block_editor(), Some("Copy"));
    assert_eq!(s.state().unwrap().drawing().block("Frame").unwrap().as_ref(), &saved_source, "copy composition keeps the saved source");

    // A later save of Copy must not reintroduce the editor table's older source definition.
    s.execute("line", &json!({ "points": [[0, 6], [4, 6]] })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(s.doc().unwrap().block("Frame").unwrap().as_ref(), &saved_source, "editor reconciliation keeps the saved source");
    assert_eq!(s.doc().unwrap().block("Copy").unwrap().entities.len(), 4, "the copy keeps its own later edit");
    s.execute("bedit", &json!({ "name": "Frame" })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 3);
}
