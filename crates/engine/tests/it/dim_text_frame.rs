//! A negative DIMGAP draws a frame around the dimension text: SETVAR takes the negative value, a
//! new dimension draws the frame, and EXPLODE turns it into four lines.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::json;

/// Draw a linear dimension with DIMGAP `gap`, explode it, and return the number of lines the
/// dimension drew and the number of LINEs EXPLODE left.
fn explode_with_gap(gap: f64) -> (usize, usize) {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "DIMGAP", "value": gap })).unwrap();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [30, 0], "at": [15, 5] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let drawn = match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(dm) => cadcraft_engine::render::dimension_in(s.doc().unwrap(), dm).lines.len(),
        k => panic!("not a dimension: {k:?}"),
    };
    s.execute("explode", &json!({ "handles": [h.hex()] })).unwrap();
    let lines = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    (drawn, lines)
}

#[test]
fn negative_dimgap_frame_is_drawn_and_exploded() {
    let (plain_drawn, plain_lines) = explode_with_gap(0.09);
    let (drawn, lines) = explode_with_gap(-0.09);
    assert_eq!(drawn, plain_drawn + 4);
    assert_eq!(lines, plain_lines + 4);
}
