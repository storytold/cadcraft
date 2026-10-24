//! #1081: a multileader's text can be edited: TEXTEDIT (what a double-click starts) takes it and
//! hands it to the in-place editor, and the Properties palette's Contents field changes it.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn text_of(s: &Session) -> String {
    s.doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.as_ref().map(|t| t.contents.clone()) } else { None })
        .unwrap()
}

#[test]
fn textedit_and_properties_change_a_multileaders_text() {
    let mut s = Session::new();
    s.execute("mleader", &json!({"points": [[0, 0], [20, 20]], "text": "example text"})).unwrap();
    let (h, at) = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.as_ref().map(|t| (e.handle, t.insert.xy())) } else { None })
        .unwrap();

    // TEXTEDIT on the text: the current contents are the default, and the in-place editor gets
    // the multileader's MTEXT.
    s.cmdline("TEXTEDIT").unwrap();
    s.input(Input::Point(at + Vec2::new(1.0, -0.05))).unwrap();
    let p = s.current_prompt().unwrap();
    assert_eq!(p.message, "Enter new text");
    assert_eq!(p.default.as_deref(), Some("example text"));
    assert_eq!(s.editing_mtext().map(|m| m.contents), Some("example text".to_string()));
    s.input(Input::Text("edited\nsecond line".into())).unwrap();
    assert_eq!(text_of(&s), "edited\\Psecond line");
    s.cancel();

    // The Properties palette's Contents field.
    s.execute("properties.set", &json!({"handles": [h.hex()], "text": "from properties"})).unwrap();
    assert_eq!(text_of(&s), "from properties");
}
