//! `properties.describe`: the Properties palette's rows for text, mtext and dimensions, with
//! shared values, *VARIES*, units, and the reason a dimension's text height can't be edited.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn handle(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn last(s: &Session) -> String {
    s.doc().unwrap().model.last().unwrap().handle.hex()
}

fn describe(s: &mut Session, p: Value) -> Value {
    s.execute("properties.describe", &p).unwrap()
}

/// The row `key` in group `group`.
fn row<'a>(desc: &'a Value, group: &str, key: &str) -> &'a Value {
    let g = desc["groups"].as_array().unwrap().iter().find(|g| g["name"] == group).unwrap_or_else(|| panic!("no group {group}: {desc}"));
    g["rows"].as_array().unwrap().iter().find(|r| r["key"] == key).unwrap_or_else(|| panic!("no row {key} in {group}: {g}"))
}

fn group_names(desc: &Value) -> Vec<String> {
    desc["groups"].as_array().unwrap().iter().map(|g| g["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn text_rows_share_values_and_vary() {
    let mut s = Session::new();
    let a = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 2.5 })).unwrap());
    let b = handle(&s.execute("text", &json!({ "at": [0, 5], "text": "B", "height": 1, "justify": "MC" })).unwrap());
    let one = describe(&mut s, json!({ "handles": [a] }));
    assert_eq!(group_names(&one), ["General", "Text", "Geometry"]);
    let h = row(&one, "Text", "height");
    assert_eq!((h["value"].as_f64(), h["display"].as_str(), h["kind"].as_str()), (Some(2.5), Some("2.5000"), Some("distance")));
    assert_eq!(row(&one, "Text", "justify")["display"], "Left");
    assert_eq!(row(&one, "Text", "style")["value"], "Standard");
    assert!(row(&one, "Text", "style")["options"].as_array().unwrap().iter().any(|o| o["value"] == "Standard"));
    assert_eq!(row(&one, "Text", "text")["value"], "A");

    let both = describe(&mut s, json!({ "handles": [a, b] }));
    assert_eq!(both["count"], 2);
    let h = row(&both, "Text", "height");
    assert_eq!((h["varies"].as_bool(), h["display"].as_str(), h["value"].is_null()), (Some(true), Some("*VARIES*"), true));
    // Both are rotated 0 and in the same style.
    assert_eq!(row(&both, "Text", "rotation")["varies"], false);
    assert_eq!(row(&both, "Text", "style")["display"], "Standard");
    assert_eq!(row(&both, "Text", "justify")["varies"], true);
}

#[test]
fn mixed_selections_show_general_rows_unless_filtered() {
    let mut s = Session::new();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 1 })).unwrap());
    s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    let c = last(&s);
    let mixed = describe(&mut s, json!({ "handles": [t, c] }));
    assert_eq!(group_names(&mixed), ["General"]);
    assert_eq!(mixed["types"], json!([{ "type": "Text", "count": 1 }, { "type": "Circle", "count": 1 }]));
    assert_eq!(row(&mixed, "General", "layer")["value"], "0");
    let texts = describe(&mut s, json!({ "handles": [t, c], "type": "text" }));
    assert_eq!((texts["filter"].as_str(), texts["count"].as_u64()), (Some("Text"), Some(1)));
    assert_eq!(group_names(&texts), ["General", "Text", "Geometry"]);
    // An unknown filter describes everything.
    assert_eq!(describe(&mut s, json!({ "handles": [t, c], "type": "nope" }))["count"], 2);
    // Nothing selected, or handles that name nothing: no rows.
    assert_eq!(describe(&mut s, json!({ "handles": [] }))["groups"], json!([]));
    assert_eq!(describe(&mut s, json!({ "handles": ["FFFFFF"] }))["count"], 0);
}

#[test]
fn mtext_rows() {
    let mut s = Session::new();
    let r = s.execute("mtext", &json!({ "at": [0, 0], "text": "one\\Ptwo", "height": 3, "width": 40, "justify": "MC", "lineSpacing": 1.5 })).unwrap();
    let m = handle(&r);
    let d = describe(&mut s, json!({ "handles": [m] }));
    assert_eq!(group_names(&d), ["General", "Text", "Geometry"]);
    assert_eq!(row(&d, "Text", "justify")["value"], "MC");
    assert_eq!(row(&d, "Text", "justify")["display"], "Middle center");
    assert_eq!(row(&d, "Text", "text")["kind"], "contents");
    assert_eq!(row(&d, "Text", "lineSpacing")["value"], 1.5);
    // Line space distance: factor × height × 5/3.
    let dist = row(&d, "Text", "lineSpacingDistance")["value"].as_f64().unwrap();
    assert!((dist - 1.5 * 3.0 * 5.0 / 3.0).abs() < 1e-9, "{dist}");
    assert_eq!(row(&d, "Text", "lineSpacingStyle")["value"], "atLeast");
    assert_eq!(row(&d, "Text", "width")["value"], 40.0);
    let cols = row(&d, "Text", "columns");
    assert_eq!((cols["display"].as_str(), cols["editable"].as_bool()), (Some("None"), Some(false)));
}

#[test]
fn dimension_rows_come_from_the_style_and_its_overrides() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    let d = describe(&mut s, json!({ "handles": [dm] }));
    assert_eq!(group_names(&d), ["General", "Misc", "Lines & Arrows", "Text", "Fit", "Primary Units", "Alternate Units", "Tolerances"]);
    let th = row(&d, "Text", "textHeight");
    assert_eq!((th["value"].as_f64(), th["editable"].as_bool(), th.get("overridden")), (Some(0.18), Some(true), None));
    assert_eq!(row(&d, "Misc", "dimStyle")["value"], "Standard");
    assert_eq!(row(&d, "Text", "textAbove")["display"], "Centered");
    assert_eq!(row(&d, "Primary Units", "decimals")["display"], "0.0000");
    assert_eq!(row(&d, "Lines & Arrows", "arrowBlock1")["display"], "Closed filled");
    assert_eq!(row(&d, "Lines & Arrows", "dimLineColor")["value"], "ByBlock");
    let m = row(&d, "Text", "measurement");
    assert_eq!((m["editable"].as_bool(), m["display"].as_str()), (Some(false), Some("10.0000")));
    assert_eq!(row(&d, "Text", "textOverride")["value"], "");

    // A Properties edit is an override of this dimension; the row says so.
    let r = s.execute("properties.set", &json!({ "handles": [dm], "textHeight": 0.3 })).unwrap();
    assert_eq!(r["ignored"], json!([]));
    let d = describe(&mut s, json!({ "handles": [dm] }));
    let th = row(&d, "Text", "textHeight");
    assert_eq!((th["value"].as_f64(), th["overridden"].as_bool()), (Some(0.3), Some(true)));
    // The style itself is unchanged.
    let st = s.execute("dimstyle.list", &json!({ "name": "Standard" })).unwrap();
    assert_eq!(st["variables"]["DIMTXT"], 0.18);
}

#[test]
fn a_fixed_height_text_style_decides_dimension_text_height() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Fixed", "height": 0.5, "current": false })).unwrap();
    s.execute("dimstyle", &json!({ "name": "FixedDims", "textStyle": "Fixed" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    let d = describe(&mut s, json!({ "handles": [dm] }));
    let th = row(&d, "Text", "textHeight");
    assert_eq!((th["editable"].as_bool(), th["display"].as_str()), (Some(false), Some("0.5000")));
    assert!(th["note"].as_str().unwrap().contains("Fixed"), "{th}");
    // Fixed heights that differ vary, whatever their DIMTXT says.
    s.execute("style", &json!({ "name": "Large", "height": 0.75, "current": false })).unwrap();
    s.execute("dimstyle", &json!({ "name": "LargeDims", "textStyle": "Large" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 10], "p2": [10, 10], "at": [5, 13] })).unwrap();
    let large = last(&s);
    let d = describe(&mut s, json!({ "handles": [dm, large] }));
    let th = row(&d, "Text", "textHeight");
    assert_eq!((th["editable"].as_bool(), th["display"].as_str()), (Some(false), Some("*VARIES*")));
}

#[test]
fn measurements_read_as_each_dimension_writes_them() {
    let mut s = Session::new();
    let shown =
        |s: &mut Session, hs: Value| row(&describe(s, json!({ "handles": hs })), "Text", "measurement")["display"].as_str().unwrap().to_string();
    s.execute("dimangular", &json!({ "vertex": [0, 0], "p1": [10, 0], "p2": [0, 10], "at": [3, 3] })).unwrap();
    let angle = last(&s);
    assert_eq!(shown(&mut s, json!([angle])), "90°");
    // In the dimension's angle format.
    s.execute("properties.set", &json!({ "handles": [angle], "angularUnit": 3, "angularDecimals": 2 })).unwrap();
    assert_eq!(shown(&mut s, json!([angle])), "1.57r");
    // The same length written at two precisions varies.
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let a = last(&s);
    s.execute("dimlinear", &json!({ "p1": [0, 20], "p2": [10, 20], "at": [5, 23] })).unwrap();
    let b = last(&s);
    assert_eq!(shown(&mut s, json!([a, b])), "10.0000");
    s.execute("properties.set", &json!({ "handles": [b], "precision": 1 })).unwrap();
    assert_eq!(shown(&mut s, json!([a, b])), "*VARIES*");
}

#[test]
fn rows_show_lengths_in_the_drawing_units() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "LUNITS", "value": 4 })).unwrap();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 18 })).unwrap());
    let d = describe(&mut s, json!({ "handles": [t] }));
    assert_eq!(row(&d, "Text", "height")["display"], "1'-6\"");
    assert_eq!(row(&d, "Text", "height")["value"], 18.0);
}

#[test]
fn hostile_requests_never_panic() {
    let mut s = Session::new();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 1 })).unwrap());
    for p in [json!({}), json!({ "handles": "x" }), json!({ "handles": [t, t, t] }), json!({ "handles": [t], "type": 5 }), json!({ "type": "Text" })]
    {
        let _ = s.execute("properties.describe", &p);
    }
    // Duplicates count once.
    assert_eq!(describe(&mut s, json!({ "handles": [t, t] }))["count"], 1);
}
