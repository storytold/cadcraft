//! Text and multiline text take another justification in place (the letters don't move), and
//! keep their Backwards / Upside down / Text frame settings.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn bounds(s: &Session, h: &str) -> cadcraft_engine::geom::Bounds2 {
    let d = s.doc().unwrap();
    let e = d.model.iter().find(|e| e.handle.hex() == h).unwrap();
    match &e.kind {
        EntityKind::Text(t) => cadcraft_render::place_text_entity(d, t, &t.value).1,
        EntityKind::MText(t) => cadcraft_render::layout_mtext_entity(d, t).bounds,
        _ => panic!("not text"),
    }
}

fn near(a: cadcraft_engine::geom::Bounds2, b: cadcraft_engine::geom::Bounds2) -> bool {
    a.min.dist(b.min) < 1e-6 && a.max.dist(b.max) < 1e-6
}

#[test]
fn text_justification_changes_keep_the_letters_in_place() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [10, 5], "height": 2.5, "text": "Hello 20", "rotation": 30 })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    let b0 = bounds(&s, &h);
    for j in ["MC", "TR", "BL", "C", "R", "M", "L"] {
        s.execute("properties.set", &json!({ "handles": [h], "justify": j })).unwrap();
        assert!(near(bounds(&s, &h), b0), "{j} moved the text");
    }
    let d = s.doc().unwrap();
    let EntityKind::Text(t) = &d.model.iter().next().unwrap().kind else { panic!() };
    assert_eq!(t.halign, cadcraft_engine::doc::HAlign::Left);
    assert!(s.execute("properties.set", &json!({ "handles": [h], "justify": "XX" })).is_err());
}

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
    let b0 = bounds(&s, &m);
    s.execute("properties.set", &json!({ "handles": [m], "justify": "MC", "frame": true })).unwrap();
    assert!(near(bounds(&s, &m), b0), "multiline text stays in place");
    let d = s.doc().unwrap();
    let mt = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(t) => Some(t.clone()),
        _ => None,
    });
    let mt = mt.unwrap();
    assert!(mt.frame && mt.attach == 5);
    let frame = cadcraft_render::mtext_frame(d, &mt).unwrap();
    assert_eq!(frame.len(), 5);
}

/// properties.set refuses a justification the selected object type does not have (multiline
/// text has no baseline justifications) instead of reporting a change it did not make.
#[test]
fn a_justification_must_suit_the_selected_objects() {
    let mut s = Session::new();
    let r = s.execute("mtext", &json!({ "at": [0, 10], "height": 1, "width": 20, "text": "abc" })).unwrap();
    let m = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    assert!(s.execute("properties.set", &json!({ "handles": [m], "justify": "C" })).is_err());
    assert!(s.execute("properties.set", &json!({ "handles": [m], "justify": "TL" })).is_ok());
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
