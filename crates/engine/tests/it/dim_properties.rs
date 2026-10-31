//! A dimension's text height, arrow size, overall scale and precision set from Properties
//! (`properties.set`) are per-dimension overrides, as in AutoCAD's Properties palette (#1259).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::render::{DimGeometry, LineRole};
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

/// Where the extension lines of the first dimension start (their end at the measured points).
fn ext_line_starts(s: &Session) -> Vec<Vec2> {
    let (_, _, g) = &dims(s)[0];
    g.lines.iter().zip(&g.line_roles).filter(|(_, role)| **role == LineRole::Ext).filter_map(|(line, _)| line.first().copied()).collect()
}

/// The extension lines' offset from the measured points (DIMEXO) and a fixed length (DIMFXLON,
/// DIMFXL) are per-dimension overrides too (#2124).
#[test]
fn properties_override_one_dimensions_extension_line_offset_and_fixed_length() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,-3\ndimlinear 0,10 20,10 5,13\n").unwrap();
    let h = dims(&s)[0].0.clone();
    // The extension lines run from the measured points (y = 0) down to the dimension line (y = -3).
    s.execute("properties.set", &json!({ "handles": [h], "extOffset": 1.0 })).unwrap();
    assert_eq!(dims(&s)[0].1.overrides.get("extOffset"), Some(&json!(1.0)));
    let starts = ext_line_starts(&s);
    assert_eq!(starts.len(), 2);
    assert!(starts.iter().all(|p| (p.y + 1.0).abs() < 1e-9), "a 1-unit gap below the points: {starts:?}");
    // A fixed length measures from the dimension line instead.
    s.execute("properties.set", &json!({ "handles": [h], "fixedExt": true, "fixedExtLength": 0.5 })).unwrap();
    let dm = &dims(&s)[0].1;
    assert_eq!(dm.overrides.get("fixedExt"), Some(&json!(true)));
    assert_eq!(dm.overrides.get("fixedExtLength"), Some(&json!(0.5)));
    let starts = ext_line_starts(&s);
    assert!(starts.iter().all(|p| (p.y + 2.5).abs() < 1e-9), "0.5 long from the dimension line: {starts:?}");
    // The other dimension is left alone.
    assert!(dims(&s)[1].1.overrides.is_empty());
}

#[test]
fn bad_values_change_nothing() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,-3\n").unwrap();
    let h = dims(&s)[0].0.clone();
    for p in [
        json!({ "textHeight": 0.0 }),
        json!({ "dimScale": -1.0 }),
        json!({ "arrowSize": "big" }),
        json!({ "precision": 9 }),
        json!({ "extOffset": -1.0 }),
        json!({ "fixedExtLength": -1.0 }),
        json!({ "fixedExt": "yes" }),
    ] {
        let mut p = p;
        p["handles"] = json!([h]);
        assert!(s.execute("properties.set", &p).is_err(), "{p}");
    }
    assert!(dims(&s)[0].1.overrides.is_empty());
}
