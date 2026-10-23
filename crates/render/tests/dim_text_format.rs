//! Dimension text formatting (issue #373): DIMRND rounds the measurement only, tolerances use
//! DIMTZIN, alternate units DIMALTRND/DIMALTU/DIMALTZ, and DIMPOST wraps angles too (#1653).

use cadcraft_doc::{DimKind, DimStyle, Dimension};
use cadcraft_geom::Vec3;
use cadcraft_render::dimension_geometry;

fn linear(len: f64) -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(len, 0.0, 0.0),
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
    }
}

fn text(d: &Dimension, vars: serde_json::Value) -> String {
    let mut st = DimStyle::default();
    let rejected = st.apply_fields(vars.as_object().unwrap());
    assert!(rejected.is_empty(), "unknown variables {rejected:?}");
    dimension_geometry(d, &st, 1.0).value
}

#[test]
fn dimrnd_rounds_neither_tolerances_nor_alternate_units() {
    let d = linear(10.3);
    let tol = serde_json::json!({"DIMRND": 0.5, "DIMDEC": 2, "DIMTOL": 1, "DIMTP": 0.1, "DIMTM": 0.1, "DIMTDEC": 2});
    assert_eq!(text(&d, tol), "10.50±0.10");
    let dev = serde_json::json!({"DIMRND": 0.5, "DIMDEC": 2, "DIMTOL": 1, "DIMTP": 0.1, "DIMTM": 0.2, "DIMTDEC": 2});
    assert_eq!(text(&d, dev), "10.50+0.10/-0.20");
    // Tolerance zeros follow DIMTZIN, not DIMZIN.
    let tzin = serde_json::json!({"DIMZIN": 8, "DIMDEC": 2, "DIMTOL": 1, "DIMTP": 0.1, "DIMTM": 0.1, "DIMTDEC": 2});
    assert_eq!(text(&d, tzin.clone()), "10.3±0.10");
    let mut tzin8 = tzin;
    tzin8["DIMTZIN"] = 8.into();
    assert_eq!(text(&d, tzin8), "10.3±0.1");

    // 10.3 in = 261.62 mm: DIMRND 0.5 rounds the primary value only.
    assert_eq!(text(&d, serde_json::json!({"DIMRND": 0.5, "DIMDEC": 1, "DIMALT": 1})), "10.5 [261.62]");
    // DIMALTRND rounds the alternate value.
    assert_eq!(text(&d, serde_json::json!({"DIMDEC": 1, "DIMALT": 1, "DIMALTRND": 0.5})), "10.3 [261.50]");
    // DIMALTZ, not DIMZIN, suppresses alternate zeros.
    assert_eq!(text(&linear(10.0), serde_json::json!({"DIMZIN": 8, "DIMALT": 1})), "10 [254.00]");
    assert_eq!(text(&linear(10.0), serde_json::json!({"DIMALT": 1, "DIMALTZ": 8})), "10.0000 [254]");
    // DIMALTU: alternate units in their own format (7 = fractional, not stacked).
    assert_eq!(text(&linear(2.0), serde_json::json!({"DIMALT": 1, "DIMALTF": 0.25, "DIMALTU": 7, "DIMALTD": 2})), "2.0000 [1/2]");
}

#[test]
fn dimpost_applies_to_angular_dimensions_too() {
    let mut a = linear(0.0);
    a.kind = DimKind::Angular3P;
    a.p15 = Vec3::ZERO;
    a.p13 = Vec3::new(10.0, 0.0, 0.0);
    a.p14 = Vec3::new(0.0, 10.0, 0.0);
    a.defpt = Vec3::new(3.0, 3.0, 0.0);
    // AutoCAD 2026 applies the style's DIMPOST to angular dimensions it builds: with "<> m"
    // its own 2-line and 3-point dimensions read "36.9° m" (#1653, follow-up to #373/#376).
    assert_eq!(text(&a, serde_json::json!({"DIMPOST": "<>mm"})), "90°mm");
    // A prefix before `<>` works as on a linear dimension.
    assert_eq!(text(&a, serde_json::json!({"DIMPOST": "A=<>"})), "A=90°");
    // Linear dimensions keep it, and an angular text override wraps the posted angle, as for
    // a linear dimension.
    assert_eq!(text(&linear(10.0), serde_json::json!({"DIMPOST": "<>mm", "DIMDEC": 1})), "10.0mm");
    a.text = "<> TYP".into();
    assert_eq!(text(&a, serde_json::json!({"DIMPOST": "<>mm"})), "90°mm TYP");
}
