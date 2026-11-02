//! Library linetypes load scaled for the drawing's units (issue #294): a metric drawing gets the
//! patterns in millimetres (inch lengths × 25.4), an imperial one keeps the inch lengths.

use cadcraft_engine::Session;
use serde_json::json;

/// Dash and gap lengths of a linetype loaded with the LINETYPE command.
fn loaded(metric: bool, name: &str) -> Vec<f64> {
    let mut s = Session::empty();
    s.new_drawing(metric);
    s.execute("linetype", &json!({ "load": name })).unwrap();
    let d = s.doc().unwrap();
    d.linetype(name).unwrap().pattern.iter().map(|e| e.length).collect()
}

fn near(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn metric_drawings_load_millimetre_patterns() {
    assert!(near(&loaded(false, "DASHED"), &[0.5, -0.25]));
    let mm = loaded(true, "DASHED");
    assert!(near(&mm, &[12.7, -6.35]), "{mm:?}");
    assert!(near(&loaded(true, "CENTER"), &[31.75, -6.35, 6.35, -6.35]));
}

#[test]
fn embedded_text_scales_with_the_pattern() {
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("linetype", &json!({ "load": "*" })).unwrap();
    let d = s.doc().unwrap();
    let fence = d.linetype("FENCELINE1").unwrap();
    let text = fence.pattern.iter().find(|e| e.text.is_some()).unwrap();
    assert!((text.scale - 2.54).abs() < 1e-9 && (text.length + 2.54).abs() < 1e-9, "{text:?}");
}

#[test]
fn centerline_loads_the_metric_center_linetype() {
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 20], [100, 20]] })).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    let (h1, h2) = (&hs[0], &hs[1]);
    s.execute("centerline", &json!({ "h1": h1, "h2": h2 })).unwrap();
    let lt = s.doc().unwrap().linetype("CENTER").unwrap().clone();
    assert!((lt.pattern[0].length - 31.75).abs() < 1e-9, "{lt:?}");
}
