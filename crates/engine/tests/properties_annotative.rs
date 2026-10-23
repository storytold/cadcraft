//! Annotative scaling in the Properties palette's rows: Annotative scale (the scales an
//! annotative object supports), paper and model text height and defined width, the hatch and
//! multileader rows that follow the palette's own, and `properties.set`'s `annoScales`.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn handle(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn last(s: &Session) -> String {
    s.doc().unwrap().model.last().unwrap().handle.hex()
}

fn describe(s: &mut Session, handles: &[&str]) -> Value {
    s.execute("properties.describe", &json!({ "handles": handles })).unwrap()
}

fn group<'a>(desc: &'a Value, name: &str) -> &'a Value {
    desc["groups"].as_array().unwrap().iter().find(|g| g["name"] == name).unwrap_or_else(|| panic!("no group {name}: {desc}"))
}

fn keys(desc: &Value, name: &str) -> Vec<String> {
    group(desc, name)["rows"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_string()).collect()
}

fn row<'a>(desc: &'a Value, name: &str, key: &str) -> &'a Value {
    group(desc, name)["rows"].as_array().unwrap().iter().find(|r| r["key"] == key).unwrap_or_else(|| panic!("no row {key}: {desc}"))
}

/// A session at annotation scale 1:10 with a text annotative at 1:10 and 1:2 (2.5 on paper).
fn annotative_text() -> (Session, String) {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "CANNOSCALE", "value": "1:10" })).unwrap();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 2.5 })).unwrap());
    s.execute("properties.set", &json!({ "handles": [t], "annotative": true })).unwrap();
    s.execute("objectscale", &json!({ "handles": [t], "add": "1:2" })).unwrap();
    (s, t)
}

#[test]
fn annotative_text_shows_its_scales_and_its_height_on_paper_and_in_the_model() {
    let (mut s, t) = annotative_text();
    let d = describe(&mut s, &[&t]);
    assert_eq!(
        keys(&d, "Text"),
        ["text", "style", "annotative", "annoScales", "justify", "height", "modelHeight", "rotation", "widthFactor", "oblique"]
    );
    let sc = row(&d, "Text", "annoScales");
    assert_eq!((sc["kind"].as_str(), sc["value"].clone(), sc["display"].as_str()), (Some("annoScales"), json!(["1:10", "1:2"]), Some("1:10, 1:2")));
    assert_eq!(row(&d, "Text", "height")["label"], "Paper text height");
    let model = row(&d, "Text", "modelHeight");
    assert_eq!((model["value"].as_f64(), model["editable"].as_bool()), (Some(25.0), Some(false)));
    // At a scale it supports, the row shows that scale and the model height follows it.
    s.execute("setvar", &json!({ "name": "CANNOSCALE", "value": "1:2" })).unwrap();
    assert_eq!(row(&describe(&mut s, &[&t]), "Text", "modelHeight")["value"].as_f64(), Some(5.0));
}

#[test]
fn plain_text_keeps_one_height_row_and_no_scales() {
    let mut s = Session::new();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 2.5 })).unwrap());
    let d = describe(&mut s, &[&t]);
    let k = keys(&d, "Text");
    assert!(k.contains(&"annotative".to_string()) && !k.contains(&"annoScales".to_string()) && !k.contains(&"modelHeight".to_string()), "{k:?}");
    assert_eq!(row(&d, "Text", "height")["label"], "Height");
    // Annotative and plain together: no scale row, one height row.
    let (mut s, a) = annotative_text();
    let b = handle(&s.execute("text", &json!({ "at": [0, 5], "text": "B", "height": 2.5 })).unwrap());
    let d = describe(&mut s, &[&a, &b]);
    assert!(row(&d, "Text", "annotative")["varies"].as_bool().unwrap());
    assert!(!keys(&d, "Text").contains(&"annoScales".to_string()));
}

#[test]
fn scales_that_differ_vary() {
    let (mut s, a) = annotative_text();
    let b = handle(&s.execute("text", &json!({ "at": [0, 5], "text": "B", "height": 2.5 })).unwrap());
    s.execute("properties.set", &json!({ "handles": [b], "annotative": true })).unwrap();
    let r = row(&describe(&mut s, &[&a, &b]), "Text", "annoScales").clone();
    assert_eq!((r["varies"].as_bool(), r["display"].as_str()), (Some(true), Some("*VARIES*")));
}

#[test]
fn annotative_mtext_has_paper_and_model_height_and_width() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "CANNOSCALE", "value": "1:4" })).unwrap();
    let m = handle(&s.execute("mtext", &json!({ "at": [0, 0], "text": "Note", "height": 2, "width": 30 })).unwrap());
    s.execute("properties.set", &json!({ "handles": [m], "annotative": true })).unwrap();
    let d = describe(&mut s, &[&m]);
    let k = keys(&d, "Text");
    for key in ["annotative", "annoScales", "height", "modelHeight", "width", "modelWidth"] {
        assert!(k.contains(&key.to_string()), "{key}: {k:?}");
    }
    assert_eq!(row(&d, "Text", "modelHeight")["value"].as_f64(), Some(8.0));
    assert_eq!(row(&d, "Text", "modelWidth")["value"].as_f64(), Some(120.0));
    assert_eq!(row(&d, "Text", "width")["label"], "Paper defined width");
}

#[test]
fn a_dimension_shows_its_scales_with_its_style() {
    let mut s = Session::new();
    let dm = handle(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 5] })).unwrap());
    s.execute("properties.set", &json!({ "handles": [dm], "annotative": true })).unwrap();
    assert_eq!(keys(&describe(&mut s, &[&dm]), "Misc")[..3], ["dimStyle", "annotative", "annoScales"]);
}

#[test]
fn hatches_and_multileaders_get_their_annotative_rows_after_the_palette_s_own() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let hatch = handle(&s.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap());
    s.execute("mleader", &json!({ "points": [[20, 0], [30, 10]], "text": "Note" })).unwrap();
    let ml = last(&s);
    for (h, what) in [(&hatch, "hatch"), (&ml, "multileader")] {
        let d = describe(&mut s, &[h]);
        let g = group(&d, "Misc");
        assert_eq!(g["extra"], true, "{what}: {g}");
        assert_eq!(keys(&d, "Misc"), ["annotative"], "{what}");
        s.execute("properties.set", &json!({ "handles": [h], "annotative": true })).unwrap();
        assert_eq!(keys(&describe(&mut s, &[h]), "Misc"), ["annotative", "annoScales"], "{what}");
    }
    // Both together share the rows.
    let d = describe(&mut s, &[&hatch, &ml]);
    assert_eq!(row(&d, "Misc", "annoScales")["display"], "1:1");
    // Text groups have no extra flag.
    let (mut s, t) = annotative_text();
    assert!(group(&describe(&mut s, &[&t]), "Text").get("extra").is_none());
}

#[test]
fn anno_scales_sets_exactly_the_scales_given() {
    let (mut s, t) = annotative_text();
    let r = s.execute("properties.set", &json!({ "handles": [t], "annoScales": ["1:4", "1:2", "1:4"] })).unwrap();
    assert!(r["ignored"].as_array().is_none_or(|a| a.is_empty()), "{r}");
    assert_eq!(row(&describe(&mut s, &[&t]), "Text", "annoScales")["value"], json!(["1:4", "1:2"]));
    // A name the list lacks, and an empty list, are errors that change nothing.
    assert!(s.execute("properties.set", &json!({ "handles": [t], "annoScales": ["1:3"] })).is_err());
    assert!(s.execute("properties.set", &json!({ "handles": [t], "annoScales": [] })).is_err());
    assert!(s.execute("properties.set", &json!({ "handles": [t], "annoScales": 7 })).is_err());
    assert_eq!(row(&describe(&mut s, &[&t]), "Text", "annoScales")["value"], json!(["1:4", "1:2"]));
}

#[test]
fn anno_scales_leave_plain_objects_plain() {
    let (mut s, t) = annotative_text();
    let plain = handle(&s.execute("text", &json!({ "at": [0, 5], "text": "B", "height": 2.5 })).unwrap());
    // A plain text among the selection takes no scales (it isn't annotative).
    s.execute("properties.set", &json!({ "handles": [t, plain], "annoScales": ["1:2"] })).unwrap();
    assert!(!keys(&describe(&mut s, &[&plain]), "Text").contains(&"annoScales".to_string()));
    assert_eq!(row(&describe(&mut s, &[&t]), "Text", "annoScales")["value"], json!(["1:2"]));
    // `annotative: false` with a scale list makes the object plain.
    s.execute("properties.set", &json!({ "handles": [t], "annotative": false, "annoScales": ["1:10"] })).unwrap();
    assert!(!keys(&describe(&mut s, &[&t]), "Text").contains(&"annoScales".to_string()));
    // `annotative: true` with a list: annotative at exactly those scales.
    s.execute("properties.set", &json!({ "handles": [plain], "annotative": true, "annoScales": ["1:2", "1:10"] })).unwrap();
    assert_eq!(row(&describe(&mut s, &[&plain]), "Text", "annoScales")["value"], json!(["1:2", "1:10"]));
}

#[test]
fn a_solid_pattern_replaces_a_gradient() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let h = handle(&s.execute("gradient", &json!({ "points": [[5, 5]] })).unwrap());
    s.execute("properties.set", &json!({ "handles": [h], "pattern": "SOLID" })).unwrap();
    let d = s.doc().unwrap();
    let e = d.entity(cadcraft_engine::doc::Handle::parse_hex(&h).unwrap()).unwrap();
    let cadcraft_engine::doc::EntityKind::Hatch(hatch) = &e.kind else { panic!("hatch") };
    assert!(hatch.solid && hatch.gradient.is_none() && hatch.pattern_lines.is_empty(), "{hatch:?}");
}
