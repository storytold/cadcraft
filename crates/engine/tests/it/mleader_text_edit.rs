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

/// An edited multileader text is undone in one step and is what a DXF save keeps.
#[test]
fn an_edited_multileader_text_undoes_and_survives_a_dxf_save() {
    let mut s = Session::new();
    s.execute("mleader", &json!({"points": [[0, 0], [20, 20]], "text": "example text"})).unwrap();
    let at = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.as_ref().map(|t| t.insert.xy()) } else { None })
        .unwrap();
    s.cmdline("TEXTEDIT").unwrap();
    s.input(Input::Point(at + Vec2::new(1.0, -0.05))).unwrap();
    s.input(Input::Text("edited\nsecond line".into())).unwrap();
    s.cancel();
    // 2007 DXF: older versions keep a multileader as a LEADER and an MTEXT.
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2007), binary: false };
    let reopened = cadcraft_io::read_dxf(&cadcraft_io::write_with(s.doc().unwrap(), "x.dxf", &opts).unwrap()).unwrap();
    let saved =
        reopened.model.iter().find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.as_ref().map(|t| t.contents.clone()) } else { None });
    assert_eq!(saved.as_deref(), Some("edited\\Psecond line"));
    s.undo().unwrap();
    assert_eq!(text_of(&s), "example text");
}

/// A multileader on a locked layer is refused when it is picked, before the editor opens, and
/// Properties leaves it alone.
#[test]
fn a_multileader_on_a_locked_layer_is_refused_when_picked() {
    let mut s = Session::new();
    s.execute("mleader", &json!({"points": [[0, 0], [20, 20]], "text": "example text"})).unwrap();
    let (h, at) = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.as_ref().map(|t| (e.handle, t.insert.xy())) } else { None })
        .unwrap();
    s.execute("layer.set", &json!({"name": "0", "locked": true})).unwrap();
    s.cmdline("TEXTEDIT").unwrap();
    s.input(Input::Point(at + Vec2::new(1.0, -0.05))).unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("The object is on a locked layer."));
    assert!(s.editing_mtext().is_none(), "the editor doesn't open");
    assert_eq!(s.current_prompt().unwrap().message, "Select an annotation object");
    s.cancel();
    let r = s.execute("properties.set", &json!({"handles": [h.hex()], "text": "from properties"})).unwrap();
    assert_eq!((r["changed"].as_u64(), r["locked"].as_u64()), (Some(0), Some(1)), "{r}");
    assert_eq!(text_of(&s), "example text");
}

#[test]
fn a_text_too_long_for_the_editor_is_asked_for_at_the_command_line() {
    let mut s = Session::new();
    s.execute("mleader", &json!({"points": [[0, 0], [20, 20]], "text": "example text"})).unwrap();
    let (h, at) = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.as_ref().map(|t| (e.handle, t.insert.xy())) } else { None })
        .unwrap();
    let long = format!("example text {}", "x".repeat(cadcraft_engine::mtext_edit::MAX_CHARS));
    s.execute("properties.set", &json!({"handles": [h.hex()], "text": long})).unwrap();
    assert_eq!(text_of(&s), long);
    s.cmdline("TEXTEDIT").unwrap();
    s.input(Input::Point(at + Vec2::new(1.0, -0.05))).unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("This text is too long for the editor; enter the new text at the command line."));
    assert!(s.editing_mtext().is_none(), "the editor doesn't open");
    assert_eq!(s.current_prompt().unwrap().message, "Enter new text");
    // Enter keeps it whole; a typed answer still replaces it, as for any other text.
    s.input(Input::Enter).unwrap();
    assert_eq!(text_of(&s), long, "the text is not shortened");
    s.input(Input::Point(at + Vec2::new(1.0, -0.05))).unwrap();
    s.input(Input::Text("short".into())).unwrap();
    assert_eq!(text_of(&s), "short");
    s.cancel();
}
