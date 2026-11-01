//! DIMPOST's suffix goes on the tolerance values as well as on the measurement (issue #390).

use cadcraft_doc::{DimKind, DimStyle, Dimension};
use cadcraft_geom::Vec3;
use cadcraft_render::dimension_geometry;

fn text(vars: serde_json::Value) -> (String, String) {
    let d = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let mut st = DimStyle::default();
    let rejected = st.apply_fields(vars.as_object().unwrap());
    assert!(rejected.is_empty(), "unknown variables {rejected:?}");
    let g = dimension_geometry(&d, &st, 1.0);
    (g.value, g.mtext)
}

#[test]
fn dimpost_suffix_applies_to_tolerances() {
    let sym = serde_json::json!({"DIMPOST": "mm", "DIMDEC": 1, "DIMTOL": 1, "DIMTP": 0.1, "DIMTM": 0.1, "DIMTDEC": 1});
    assert_eq!(text(sym).0, "10.0mm±0.1mm");
    // With `<>` only the part after it is the suffix; the prefix stays on the measurement.
    let dev = serde_json::json!({"DIMPOST": "L=<> mm", "DIMDEC": 1, "DIMTOL": 1, "DIMTP": 0.1, "DIMTM": 0.2, "DIMTDEC": 1});
    let (_, mtext) = text(dev);
    assert!(mtext.starts_with("L=10.0 mm{"), "{mtext}");
    assert!(mtext.contains("\\S+0.1 mm^-0.2 mm;"), "{mtext}");
    // No DIMPOST: unchanged.
    let plain = serde_json::json!({"DIMDEC": 1, "DIMTOL": 1, "DIMTP": 0.1, "DIMTM": 0.1, "DIMTDEC": 1});
    assert_eq!(text(plain).0, "10.0±0.1");
}
