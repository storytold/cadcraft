//! Properties moves justified TEXT by its alignment point: a TL text (placed by its alignment
//! point, #342) lands where its new position says.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, entity_bounds};
use serde_json::json;

#[test]
fn moving_top_left_text_through_properties_moves_it() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [5, 5], "text": "TL", "height": 1, "justify": "TL" })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [h], "position": [20, 20] })).unwrap();
    let d = s.doc().unwrap();
    let b = d.model.iter().find_map(|e| matches!(e.kind, EntityKind::Text(_)).then(|| entity_bounds(d, e, 0))).unwrap();
    assert!((b.max.y - 20.0).abs() < 1e-9 && (b.min.x - 20.0).abs() < 1e-9, "moved text {b:?}");
}
