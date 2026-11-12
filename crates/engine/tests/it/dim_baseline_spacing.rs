//! DIMBASELINE spaces dimension lines by DIMDLI × the overall scale of the dimension it
//! continues: SETVAR DIM* overrides count, and DIMSCALE 0 falls back to the drawing's DIMSCALE
//! (issue #364). DIMSPACE's default spacing uses the same scale.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn dim_line_y(s: &Session, h: &Value) -> f64 {
    let h = Handle::parse_hex(h.as_str().unwrap()).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(dm) => dm.defpt.y,
        k => panic!("not a dimension: {k:?}"),
    }
}

/// Spacing between a horizontal dimension with its line at y = 5 and the baseline one after it.
fn baseline_spacing(setup: &[(&str, Value)]) -> f64 {
    let mut s = Session::new();
    for (cmd, p) in setup {
        s.execute(cmd, p).unwrap();
    }
    let first = s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 5]})).unwrap();
    let next = s.execute("dimbaseline", &json!({"points": [[20, 0]]})).unwrap();
    dim_line_y(&s, &next["handles"][0]) - dim_line_y(&s, &first["handle"])
}

#[test]
fn baseline_spacing_follows_overrides_and_dimscale() {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let plain = baseline_spacing(&[]);
    assert!(near(plain, 0.38), "{plain}");
    let scaled = baseline_spacing(&[("setvar", json!({"name": "DIMSCALE", "value": 10}))]);
    assert!(near(scaled, 3.8), "SETVAR DIMSCALE scales the spacing: {scaled}");
    let dli = baseline_spacing(&[("setvar", json!({"name": "DIMDLI", "value": 1}))]);
    assert!(near(dli, 1.0), "SETVAR DIMDLI sets the spacing: {dli}");
    let zero = baseline_spacing(&[("dimstyle", json!({"name": "Standard", "DIMSCALE": 0}))]);
    assert!(near(zero, 0.38), "DIMSCALE 0 in model space is scale 1, not a collapsed spacing: {zero}");

    // DIMSPACE's default spacing (twice DIMDLI) uses the same scale.
    let mut s = Session::new();
    s.execute("dimstyle", &json!({"name": "Standard", "DIMSCALE": 0})).unwrap();
    let a = s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [30, 0], "at": [15, 5]})).unwrap();
    let b = s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [50, 0], "at": [25, 6]})).unwrap();
    s.execute("dimspace", &json!({"base": a["handle"], "handles": [b["handle"]]})).unwrap();
    let gap = dim_line_y(&s, &b["handle"]) - dim_line_y(&s, &a["handle"]);
    assert!(near(gap, 0.76), "{gap}");
}
