//! Dimension text formatting (issue #373): DIMRND rounds the measurement only, tolerances use
//! DIMTZIN, alternate units DIMALTRND/DIMALTU/DIMALTZ, and DIMPOST is not added to angles.

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
fn dimpost_is_not_added_to_angular_dimensions() {
    let mut a = linear(0.0);
    a.kind = DimKind::Angular3P;
    a.p15 = Vec3::ZERO;
    a.p13 = Vec3::new(10.0, 0.0, 0.0);
    a.p14 = Vec3::new(0.0, 10.0, 0.0);
    a.defpt = Vec3::new(3.0, 3.0, 0.0);
    assert_eq!(text(&a, serde_json::json!({"DIMPOST": "<>mm"})), "90°");
    // Linear dimensions keep it, and an angular text override still wraps the angle.
    assert_eq!(text(&linear(10.0), serde_json::json!({"DIMPOST": "<>mm", "DIMDEC": 1})), "10.0mm");
    a.text = "<> TYP".into();
    assert_eq!(text(&a, serde_json::json!({"DIMPOST": "<>mm"})), "90° TYP");
}

fn angular(radians: f64) -> Dimension {
    let mut d = linear(0.0);
    d.kind = DimKind::Angular3P;
    d.p13 = Vec3::new(10.0, 0.0, 0.0);
    d.p14 = Vec3::new(10.0 * radians.cos(), 10.0 * radians.sin(), 0.0);
    d.defpt = Vec3::new(5.0 * (radians / 2.0).cos(), 5.0 * (radians / 2.0).sin(), 0.0);
    d
}

#[test]
fn angular_dimensions_use_the_style_decimal_separator() {
    for (unit, radians, decimals, dot, comma) in
        [(0, 39.4_f64.to_radians(), 1, "39.4°", "39,4°"), (2, std::f64::consts::FRAC_PI_4, 1, "50.0g", "50,0g"), (3, 1.25, 2, "1.25r", "1,25r")]
    {
        let three_point = angular(radians);
        let mut two_line = three_point.clone();
        two_line.kind = DimKind::Angular;
        two_line.p16 = three_point.defpt;
        two_line.p15 = three_point.p14;
        two_line.p14 = three_point.p13;
        two_line.p13 = Vec3::ZERO;
        two_line.defpt = Vec3::ZERO;
        for d in [three_point, two_line] {
            let vars = serde_json::json!({"DIMAUNIT": unit, "DIMADEC": decimals});
            assert_eq!(text(&d, vars.clone()), dot);
            let mut vars = vars;
            vars["DIMDSEP"] = serde_json::json!(",");
            assert_eq!(text(&d, vars), comma);
        }
    }
}

#[test]
fn angular_decimal_separator_respects_overrides_and_literal_text() {
    let mut d = angular(39.4_f64.to_radians());
    d.text = "<> at 1.5".into();
    d.overrides.insert("DIMDSEP".into(), serde_json::json!(","));
    assert_eq!(text(&d, serde_json::json!({"DIMADEC": 1, "DIMDSEP": "."})), "39,4° at 1.5");
    d.overrides.insert("DIMDSEP".into(), serde_json::json!("."));
    assert_eq!(text(&d, serde_json::json!({"DIMADEC": 1, "DIMDSEP": ","})), "39.4° at 1.5");
}

#[test]
fn angular_zero_suppression_works_with_a_decimal_comma() {
    let d = angular(0.5_f64.to_radians());
    for (zin, expected) in [(0, "0,500°"), (1, ",500°"), (2, "0,5°"), (3, ",5°")] {
        assert_eq!(text(&d, serde_json::json!({"DIMADEC": 3, "DIMAZIN": zin, "DIMDSEP": ","})), expected);
    }
}

#[test]
fn angular_decimal_separator_keeps_dms_notation() {
    let d = angular(45.5125_f64.to_radians());
    assert_eq!(text(&d, serde_json::json!({"DIMAUNIT": 1, "DIMADEC": 3, "DIMDSEP": ","})), "45°30'45\"");
}
