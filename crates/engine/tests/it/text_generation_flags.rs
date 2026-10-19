//! TEXT's own Backwards and Upside down (DXF 71 generation flags): new text and attribute
//! definitions take them from their style, later style changes leave existing text alone, the
//! Properties palette shows and sets them, and DXF keeps them.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Text};
use cadcraft_engine::properties::describe;
use serde_json::{Value, json};

fn texts(s: &Session) -> Vec<Text> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).collect()
}

fn handles(s: &Session) -> Vec<String> {
    s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect()
}

#[test]
fn new_text_takes_its_styles_effects_and_keeps_them() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Mirror", "backwards": true, "upsideDown": true })).unwrap();
    for l in ["TEXT", "0,0", "1", "0", "Typed", ""] {
        s.cmdline(l).unwrap();
    }
    s.execute("text", &json!({ "at": [0, 5], "text": "Json" })).unwrap();
    for t in texts(&s) {
        assert!(t.backwards && t.upside_down, "{}", t.value);
    }
    // The style changing doesn't change text already placed.
    s.execute("style", &json!({ "name": "Mirror", "backwards": false, "upsideDown": false })).unwrap();
    assert!(texts(&s).iter().all(|t| t.backwards && t.upside_down));
    s.execute("text", &json!({ "at": [0, 10], "text": "Plain" })).unwrap();
    let plain = &texts(&s)[2];
    assert!(!plain.backwards && !plain.upside_down);
}

#[test]
fn attribute_definitions_take_the_style_too() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Tag", "backwards": true, "widthFactor": 0.75 })).unwrap();
    s.execute("attdef", &json!({ "tag": "NO", "prompt": "Number", "at": [0, 0], "height": 1 })).unwrap();
    let d = s.doc().unwrap();
    let a = d.model.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).unwrap();
    assert!(a.text.backwards && !a.text.upside_down);
    assert_eq!(a.text.width_factor, 0.75);
}

#[test]
fn the_palette_shows_and_sets_them() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "text": "a", "height": 1 })).unwrap();
    s.execute("text", &json!({ "at": [0, 5], "text": "b", "height": 1 })).unwrap();
    let hs = handles(&s);
    let r = s.execute("properties.set", &json!({ "handles": [hs[0]], "backwards": true })).unwrap();
    assert_eq!(r["ignored"], json!([]));
    let d = s.doc().unwrap();
    let handles: Vec<_> = d.model.iter().map(|e| e.handle).collect();
    let desc = serde_json::to_value(describe(d, &handles, None)).unwrap();
    let misc = desc["groups"].as_array().unwrap().iter().find(|g| g["name"] == "Misc").expect("a Misc group");
    let row = |key: &str| misc["rows"].as_array().unwrap().iter().find(|r| r["key"] == key).cloned().unwrap_or(Value::Null);
    assert_eq!(row("backwards")["varies"], true);
    assert_eq!(row("upsideDown")["value"], false);
    s.execute("properties.set", &json!({ "handles": hs, "upsideDown": true, "backwards": false })).unwrap();
    assert!(texts(&s).iter().all(|t| t.upside_down && !t.backwards));
}

#[test]
fn dxf_keeps_them() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "text": "Plain", "height": 1 })).unwrap();
    s.execute("text", &json!({ "at": [0, 5], "text": "Both", "height": 1 })).unwrap();
    let both = handles(&s)[1].clone();
    s.execute("properties.set", &json!({ "handles": [both], "backwards": true, "upsideDown": true })).unwrap();
    let dxf = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let t: Vec<Text> = back.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).collect();
    let by = |v: &str| t.iter().find(|x| x.value == v).unwrap().clone();
    assert!(!by("Plain").backwards && !by("Plain").upside_down);
    assert!(by("Both").backwards && by("Both").upside_down);
    // Flags of 0 aren't written; 6 is both. A text's groups run from its value (group 1) to the
    // next entity (group 0).
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let flags = |v: &str| -> Option<String> {
        let at = lines.iter().position(|l| *l == v).unwrap();
        let mut i = at + 1;
        while i + 1 < lines.len() && lines[i] != "0" {
            if lines[i] == "71" {
                return Some(lines[i + 1].to_string());
            }
            i += 2;
        }
        None
    };
    assert_eq!(flags("Both").as_deref(), Some("6"));
    assert_eq!(flags("Plain"), None);
}

#[test]
fn dxf_starts_justified_text_by_its_own_flags() {
    // The same centered text twice, one backwards by its own flag (its style isn't): DXF group 10,
    // the start of the baseline, is worked out with each text's flags, so the two differ.
    let mut s = Session::new();
    for y in [0, 5] {
        s.execute("text", &json!({ "at": [10, y], "text": "Same", "height": 1, "justify": "MC" })).unwrap();
    }
    let back_one = handles(&s)[1].clone();
    s.execute("properties.set", &json!({ "handles": [back_one], "backwards": true })).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let starts: Vec<(bool, f64)> =
        back.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some((t.backwards, t.insert.x)) } else { None }).collect();
    let x = |b: bool| starts.iter().find(|(f, _)| *f == b).unwrap().1;
    assert!((x(true) - x(false)).abs() > 0.1, "{starts:?}");
}
