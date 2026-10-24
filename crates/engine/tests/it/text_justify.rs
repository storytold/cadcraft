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

#[test]
fn text_formulas_show_their_result_and_edit_as_formulas() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "=2400-2*18" })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    let get = |s: &Session| match &s.doc().unwrap().model.iter().next().unwrap().kind {
        EntityKind::Text(t) => (t.value.clone(), t.formula.clone()),
        _ => panic!(),
    };
    assert_eq!(get(&s), ("2364".to_string(), Some("=2400-2*18".to_string())));
    // Edited as the formula; plain text drops it.
    s.execute("textedit", &json!({ "handle": h, "text": "=ROUND(10/3,2)" })).unwrap();
    assert_eq!(get(&s).0, "3.33");
    s.execute("textedit", &json!({ "handle": h, "text": "plain" })).unwrap();
    assert_eq!(get(&s), ("plain".to_string(), None));
    // Dates follow the clock when fields are updated.
    s.execute("textedit", &json!({ "handle": h, "text": "=TEXT(TODAY(),\"yyyy\")" })).unwrap();
    let year = get(&s).0;
    assert!(year.len() == 4 && year.starts_with("20"), "{year}");
    assert_eq!(s.execute("updatefield", &json!({})).unwrap()["updated"], 1);
}

#[test]
fn formulas_work_when_editing_multiline_text_dimensions_and_leaders() {
    let mut s = Session::new();
    let r = s.execute("mtext", &json!({ "at": [0, 10], "height": 1, "width": 20, "text": "abc" })).unwrap();
    let m = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": m, "text": "{\\fArial|b0;=1200+350*2}" })).unwrap();
    let mt = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(t) => Some(t.clone()),
        _ => None,
    });
    let mt = mt.unwrap();
    assert!(cadcraft_fonts::plain_mtext(&mt.contents).contains("1900"), "{}", mt.contents);
    // Two lines: not a formula.
    s.execute("textedit", &json!({ "handle": m, "text": "=1+1\\Psecond" })).unwrap();
    let mt = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(t) => Some(t.clone()),
        _ => None,
    });
    assert!(mt.unwrap().formula.is_none());
}

#[test]
fn dimension_and_leader_text_take_formulas() {
    let mut s = Session::new();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": h, "text": "=<>/2" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!(dm.marks.formula.as_deref(), Some("=<>/2"));
    assert_eq!(dm.text, "625");
    assert_eq!(cadcraft_render::dimension_in(d, dm).value, "625");
    let r = s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
    let m = r["handle"].as_str().unwrap().to_string();
    s.execute("textedit", &json!({ "handle": m, "text": "=2*21" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::MLeader(ml) = &d.model.iter().find(|e| e.handle.hex() == m).unwrap().kind else { panic!() };
    assert_eq!(ml.text.as_ref().unwrap().contents, "42");
}
