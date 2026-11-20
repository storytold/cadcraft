//! Text and multiline text keep their Backwards / Upside down / Text frame settings, and the
//! drawing's missing fonts are reported.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

#[test]
fn backwards_upside_down_and_frame_are_properties() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "AB" })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [h], "backwards": true, "upsideDown": true })).unwrap();
    let EntityKind::Text(t) = &s.doc().unwrap().model.iter().next().unwrap().kind else { panic!() };
    assert!(t.backwards && t.upside_down);

    let r = s.execute("mtext", &json!({ "at": [0, 10], "height": 1, "width": 20, "text": "Line one\\Pline two" })).unwrap();
    let m = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [m], "frame": true })).unwrap();
    let d = s.doc().unwrap();
    let mt = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(t) => Some(t.clone()),
        _ => None,
    });
    let mt = mt.unwrap();
    assert!(mt.frame);
    let frame = cadcraft_render::mtext_frame(d, &mt).unwrap();
    assert_eq!(frame.len(), 5);
}

/// A missing big font is reported with the drawing's other missing fonts.
#[test]
fn a_missing_big_font_is_reported() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        let mut st = d.text_styles.first().cloned().unwrap();
        st.name = "ASIAN".into();
        st.font = "txt.shx".into();
        st.big_font = "cadcraft_no_such_bigfont.shx".into();
        d.text_styles.push(st);
    }
    let r = s.execute("fonts.missing", &json!({})).unwrap();
    let fonts: Vec<&str> = r.as_array().unwrap().iter().filter_map(|f| f["font"].as_str()).collect();
    assert!(fonts.contains(&"cadcraft_no_such_bigfont.shx"), "{fonts:?}");
    assert!(!fonts.contains(&"txt.shx"), "the stroke font stands in for txt.shx");
}
