//! #1056: a leader's annotation is drawn at the dimension text size, DIMTXT × DIMSCALE, like the
//! dimensions beside it.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

#[test]
fn leader_text_takes_dimscale() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({"name": "S25", "current": true, "DIMSCALE": 25, "DIMTXT": 0.25, "DIMASZ": 0.2})).unwrap();
    s.execute("leader", &json!({"points": [[0, 0], [20, 20], [30, 20]], "text": "NOTE"})).unwrap();
    let h = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(m) = &e.kind { Some(m.height) } else { None }).unwrap();
    assert!((h - 6.25).abs() < 1e-9, "{h}");
}

/// An annotative dimension style: the annotation scale takes DIMSCALE's place, as for an
/// annotative dimension, so the text is DIMTXT high and annotative, scaled with the dimension.
#[test]
fn leader_text_of_an_annotative_style_is_annotative() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({"name": "A", "current": true, "DIMSCALE": 2, "DIMTXT": 0.25, "annotative": true})).unwrap();
    s.execute("leader", &json!({"points": [[0, 0], [20, 20], [30, 20]], "text": "NOTE"})).unwrap();
    let d = s.doc().unwrap();
    let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::MText(_))).unwrap();
    let EntityKind::MText(m) = &e.kind else { unreachable!() };
    assert!((m.height - 0.25).abs() < 1e-9, "{}", m.height);
    assert!(cadcraft_engine::doc::is_annotative(e), "annotative like the style");
    // Drawn at an annotation scale of 1:50, it is as high as the dimension text: DIMTXT × 50.
    let scaled = cadcraft_engine::doc::anno_scaled(e, 50.0);
    let EntityKind::MText(m) = &scaled.kind else { unreachable!() };
    assert!((m.height - 12.5).abs() < 1e-9, "{}", m.height);
}
