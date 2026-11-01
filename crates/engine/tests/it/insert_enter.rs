//! Enter or Space at INSERT's insertion point prompt ends the command, as in AutoCAD, instead of
//! asking again (the Blocks palette's Repeat Placement stops this way).

use cadcraft_engine::{Input, Session};
use serde_json::json;

#[test]
fn enter_at_the_insertion_point_ends_insert() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "A", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();

    s.start("insert").unwrap();
    s.input(Input::Text("A".into())).unwrap();
    assert!(s.prompt_text().starts_with("INSERT Specify insertion point"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap();
    assert_eq!(s.prompt_text(), "Command:");
    assert_eq!(s.doc().unwrap().model.iter().count(), 0, "nothing is placed");
}
