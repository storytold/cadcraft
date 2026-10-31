//! TEXTEDIT with a single-line text picked exposes that text (`Session::editing_text`) for the
//! UI's in-place field, and drops it once the value is given or the pick is left.

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

#[test]
fn a_picked_text_or_attribute_definition_is_the_text_being_edited() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "text": "HELLO", "height": 2.5 })).unwrap();
    s.execute("attdef", &json!({ "tag": "NO", "default": "1", "at": [0, 10], "height": 2.5 })).unwrap();
    s.execute("mtext", &json!({ "at": [0, 20], "text": "first\\Psecond", "height": 2.5, "width": 20 })).unwrap();
    assert!(s.editing_text().is_none());
    s.start("textedit").unwrap();
    assert!(s.editing_text().is_none(), "nothing picked yet");
    s.input(Input::Point(Vec2::new(4.0, 1.2))).unwrap();
    assert_eq!(s.prompt_text(), s.prompt_text());
    assert_eq!(s.editing_text().map(|t| t.value), Some("HELLO".to_string()), "{}", s.prompt_text());
    s.input(Input::Text("HELLO WORLD".into())).unwrap();
    assert!(s.editing_text().is_none(), "given");

    s.input(Input::Point(Vec2::new(1.0, 11.2))).unwrap();
    assert_eq!(s.editing_text().map(|t| (t.value, t.height)), Some(("1".to_string(), 2.5)), "an attribute definition's default");
    s.input(Input::Enter).unwrap();
    assert!(s.editing_text().is_none(), "left as it was");

    s.input(Input::Point(Vec2::new(3.0, 18.8))).unwrap();
    assert!(s.editing_text().is_none(), "multiline text is not single-line text: {}", s.prompt_text());
}
