//! The Properties palette's Measurement row: the measurement, not the text that replaces it.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// The Measurement property is the measurement, not the text that replaces it (#2322 review).
#[test]
fn the_measurement_property_ignores_the_override() {
    let mut s = Session::new();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1250, 0], "at": [600, 100] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [h], "textOverride": "TYP" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().find(|e| e.handle.hex() == h).unwrap().kind else { panic!() };
    assert_eq!(cadcraft_render::dimension_in(d, dm).value, "TYP");
    assert_eq!(cadcraft_render::measurement_text(d, dm), "1250.0000");
}
