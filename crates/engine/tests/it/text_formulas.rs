//! Text formulas: a text, multiline text, multileader or dimension text that starts with `=`
//! keeps the formula and shows its result; UPDATEFIELD works them out again.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

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
fn dimension_text_takes_formulas() {
    let mut s = Session::new();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": h, "text": "=<>/2" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!(dm.marks.formula.as_deref(), Some("=<>/2"));
    assert_eq!(dm.text, "625");
    assert_eq!(cadcraft_render::dimension_in(d, dm).value, "625");
}

/// properties.set replacing a text drops the formula it had (or takes the new one), like
/// typing over it; UPDATEFIELD does not bring the old result back (#2034 review).
#[test]
fn replacing_text_in_properties_updates_its_formula() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "=1+1" })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    let r = s.execute("mtext", &json!({ "at": [0, 10], "height": 1, "width": 20, "text": "=2+2" })).unwrap();
    let m = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [h, m], "text": "replacement" })).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    for e in s.doc().unwrap().model.iter() {
        match &e.kind {
            EntityKind::Text(t) => assert_eq!((t.value.as_str(), t.formula.as_deref()), ("replacement", None)),
            EntityKind::MText(t) => assert_eq!((t.contents.as_str(), t.formula.as_deref()), ("replacement", None)),
            _ => {}
        }
    }
    // A formula given there is worked out.
    s.execute("properties.set", &json!({ "handles": [h], "text": "=3*3" })).unwrap();
    let t = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!((t.value.as_str(), t.formula.as_deref()), ("9", Some("=3*3")));
}

/// SHEET() in a layout is that layout's name, in model space "Model", whichever is active when
/// fields are updated (#2034 review).
#[test]
fn sheet_is_each_texts_own_layout() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "=SHEET()" })).unwrap();
    let layout = s.doc().unwrap().layouts.first().map(|l| l.name.clone()).unwrap();
    s.execute("layout.set", &json!({ "name": layout })).unwrap();
    s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "=SHEET()" })).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let value = |e: &std::sync::Arc<cadcraft_engine::doc::Entity>| if let EntityKind::Text(t) = &e.kind { t.value.clone() } else { String::new() };
    assert_eq!(d.model.iter().map(value).collect::<Vec<_>>(), ["Model"]);
    let l = d.layouts.iter().find(|l| l.name == layout).unwrap();
    assert_eq!(l.entities.iter().map(value).collect::<Vec<_>>(), std::slice::from_ref(&layout));
}

/// A new multileader's text that is a formula is worked out as when it is edited; a multiline
/// result keeps `\`, `{` and `}` as letters (#2034 review).
#[test]
fn new_multileader_formulas_and_literal_results() {
    let mut s = Session::new();
    s.execute("mleader", &json!({ "points": [[0, 0], [10, 10]], "text": "=2*3" })).unwrap();
    let ml = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MLeader(m) = &e.kind { m.text.clone() } else { None }).unwrap();
    assert_eq!((cadcraft_fonts::plain_mtext(&ml.contents).as_str(), ml.formula.as_deref()), ("6", Some("=2*3")));
    // In multiline text a brace is written `\{`: the formula is =UPPER("{a}").
    let r = s.execute("mtext", &json!({ "at": [0, 20], "height": 1, "width": 20, "text": "=UPPER(\"\\{a\\}\")" })).unwrap();
    let m = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    let mt = s.doc().unwrap().entity(cadcraft_engine::doc::Handle::parse_hex(&m).unwrap()).map(|e| e.kind.clone());
    let Some(EntityKind::MText(mt)) = mt else { panic!() };
    assert_eq!(mt.formula.as_deref(), Some("=UPPER(\"{a}\")"));
    assert_eq!(cadcraft_fonts::plain_mtext(&mt.contents), "{A}", "{}", mt.contents);
}

/// A dimension formula's `<>` is the measurement itself, not the first number in the shown text:
/// a DIMPOST prefix with a digit (`2X<>`) does not become the value (#2034 review).
#[test]
fn a_dimension_formula_uses_the_measurement_not_the_shown_text() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "DIMPOST", "value": "2X<>" })).unwrap();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": h, "text": "=<>/2" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!(cadcraft_render::dimension_in(d, dm).value, "625");
}

/// The stored text of a `=<>/2` dimension with a DIMPOST prefix is the formula's result
/// (properties and DXF group 1 show it), not a number read from the prefix (#2318 review).
#[test]
fn a_dimension_formula_stores_its_result_with_a_prefix() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "DIMPOST", "value": "2X<>" })).unwrap();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": h, "text": "=<>/2" })).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!(dm.text, "625");
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(d).as_bytes()).unwrap();
    let dm = back.model.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.clone()) } else { None }).unwrap();
    assert_eq!((dm.text.as_str(), dm.marks.formula.as_deref()), ("625", Some("=<>/2")));
}

/// Typing a plain override in the properties drops a dimension's formula; UPDATEFIELD keeps
/// the typed text (#2318 review).
#[test]
fn a_dimension_override_in_properties_replaces_its_formula() {
    let mut s = Session::new();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": h, "text": "=<>/2" })).unwrap();
    s.execute("properties.set", &json!({ "handles": [h], "textOverride": "ABC" })).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!((dm.text.as_str(), dm.marks.formula.as_deref()), ("ABC", None));
    assert_eq!(cadcraft_render::dimension_in(d, dm).value, "ABC");
    // A formula typed there becomes the formula, `<>` the measurement.
    s.execute("properties.set", &json!({ "handles": [h], "textOverride": "=<>*2" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!((dm.text.as_str(), dm.marks.formula.as_deref()), ("2500", Some("=<>*2")));
}

/// TXT2MTXT of one text keeps its formula, which still recalculates (#2318 review).
#[test]
fn txt2mtxt_keeps_a_texts_formula() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "=2+3" })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("txt2mtxt", &json!({ "handles": [h], "combine": false })).unwrap();
    let mt = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!((cadcraft_fonts::plain_mtext(&mt.contents).as_str(), mt.formula.as_deref()), ("5", Some("=2+3")));
}

/// One-line text keeps braces and backslashes in its formula: they are letters there, not
/// multiline formatting (#2318 review).
#[test]
fn one_line_text_formulas_keep_braces() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "height": 1, "text": "=LEN(\"a{b}\")" })).unwrap();
    s.execute("text", &json!({ "at": [0, 5], "height": 1, "text": "=UPPER(\"{a}\\x\")" })).unwrap();
    let got: Vec<(String, Option<String>)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some((t.value.clone(), t.formula.clone())) } else { None })
        .collect();
    assert_eq!(got, [("4".to_string(), Some("=LEN(\"a{b}\")".to_string())), ("{A}\\X".to_string(), Some("=UPPER(\"{a}\\x\")".to_string()))]);
}

/// The in-place editor shows what was typed: an attribute definition's formula, a multiline
/// formula with its braces as letters, and a dimension's own measurement for `<>` (#2320 review).
#[test]
fn the_editor_shows_formulas_as_typed() {
    use cadcraft_engine::geom::Vec2;
    use cadcraft_engine::{EditorKind, Input};
    let open = |s: &mut Session, at: Vec2| {
        s.start("textedit").unwrap();
        // Single or Multiple may be asked first; a point picks the object.
        s.input(Input::Point(at)).unwrap();
        s.text_editor().expect("the editor opens")
    };
    let mut s = Session::new();
    s.execute("attdef", &json!({ "tag": "T", "default": "=1+1", "at": [0, 0], "height": 1.0 })).unwrap();
    let ed = open(&mut s, Vec2::new(0.3, 0.4));
    assert_eq!((ed.kind, ed.mtext.contents.as_str()), (EditorKind::Line, "=1+1"));
    // Accepted unchanged, the formula stays.
    s.input(Input::Text("=1+1".into())).unwrap();
    let a = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.text.clone()) } else { None }).unwrap();
    assert_eq!(a.formula.as_deref(), Some("=1+1"));
    s.cancel();

    let mut s = Session::new();
    s.execute("mtext", &json!({ "at": [0, 10], "height": 1, "width": 20, "text": "=UPPER(\"\\{a\\}\")" })).unwrap();
    let ed = open(&mut s, Vec2::new(0.5, 9.6));
    assert_eq!(ed.mtext.contents, "=UPPER(\"\\{a\\}\")", "braces written as letters");
    s.input(Input::Text(ed.mtext.contents.clone())).unwrap();
    let mt = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!((mt.formula.as_deref(), cadcraft_fonts::plain_mtext(&mt.contents).as_str()), (Some("=UPPER(\"{a}\")"), "{A}"));
    s.cancel();

    let mut s = Session::new();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("textedit", &json!({ "handle": h, "text": "=<>/2" })).unwrap();
    let at = {
        let d = s.doc().unwrap();
        let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
        cadcraft_render::dimension_in(d, dm).text_pos
    };
    let ed = open(&mut s, at);
    match ed.kind {
        EditorKind::Dimension { measured } => assert_eq!(measured, "1250.0000"),
        k => panic!("{k:?}"),
    }
}

/// Multiline text sent back by the editor in its own form of the codes, unchanged otherwise,
/// keeps its formula (#2326 review).
#[test]
fn an_unchanged_multiline_formula_in_the_editors_form_is_kept() {
    let mut s = Session::new();
    let r = s.execute("mtext", &json!({ "at": [0, 10], "height": 1, "width": 20, "text": "=UPPER(\"\\{a\\}\")&\"\\\\x\"" })).unwrap();
    let m = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    let before = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert!(before.formula.is_some());
    let shown = cadcraft_doc::mtext_literal(before.formula.as_deref().unwrap());
    let sent = cadcraft_engine::mtext_edit::Formatted::from_contents(&shown).to_contents();
    s.execute("textedit", &json!({ "handle": m, "text": sent })).unwrap();
    let after = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!(after.formula, before.formula);
}
