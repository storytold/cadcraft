//! A dimension's text height, arrow size, overall scale and precision set from Properties
//! (`properties.set`) are per-dimension overrides, as in AutoCAD's Properties palette (#1259).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind};
use cadcraft_engine::render::DimGeometry;
use serde_json::json;

fn dims(s: &Session) -> Vec<(String, Dimension, DimGeometry)> {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some((e.handle.hex(), dm.clone(), cadcraft_engine::render::dimension_in(d, dm))),
            _ => None,
        })
        .collect()
}

#[test]
fn properties_override_one_dimensions_text_height_arrows_scale_and_precision() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,-3\ndimlinear 0,10 20,10 5,13\n").unwrap();
    let before = dims(&s);
    let (h, _, g0) = &before[0];
    let base_height = g0.text_height;
    s.execute("properties.set", &json!({ "handles": [h], "textHeight": 5.0, "arrowSize": 0.0, "dimScale": 2.0, "precision": 3 })).unwrap();
    let after = dims(&s);
    let (_, dm, g) = &after[0];
    assert_eq!(dm.overrides.get("textHeight"), Some(&json!(5.0)));
    assert_eq!(dm.overrides.get("arrowSize"), Some(&json!(0.0)));
    assert_eq!(dm.overrides.get("scale"), Some(&json!(2.0)));
    assert_eq!(dm.overrides.get("decimals"), Some(&json!(3)));
    // The text is drawn 5 × the overall scale high and shows three decimals.
    assert!((g.text_height - 10.0).abs() < 1e-9, "{} (was {base_height})", g.text_height);
    assert_eq!(g.value, "10.000");
    // The other dimension is left alone.
    assert!(after[1].1.overrides.is_empty());
    assert!((after[1].2.text_height - before[1].2.text_height).abs() < 1e-12);
}

#[test]
fn bad_values_change_nothing() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,-3\n").unwrap();
    let h = dims(&s)[0].0.clone();
    for p in [json!({ "textHeight": 0.0 }), json!({ "dimScale": -1.0 }), json!({ "arrowSize": "big" }), json!({ "precision": 9 })] {
        let mut p = p;
        p["handles"] = json!([h]);
        assert!(s.execute("properties.set", &p).is_err(), "{p}");
    }
    assert!(dims(&s)[0].1.overrides.is_empty());
}
