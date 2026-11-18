//! OFFSET of a full ellipse goes to the side of the picked point (issue #501): an outside point
//! gave an inward copy and an inside point an outward one.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn last_handle(s: &Session) -> String {
    let d = s.doc().unwrap();
    let Some(e) = d.model.iter().last() else { panic!("empty drawing") };
    e.handle.hex()
}

/// The largest (x, y) over the vertices of the last polyline.
fn last_poly_max(s: &Session) -> (f64, f64) {
    let d = s.doc().unwrap();
    let Some(e) = d.model.iter().last() else { panic!("empty drawing") };
    let EntityKind::LwPolyline(p) = &e.kind else { panic!("not a polyline: {:?}", e.kind) };
    p.vertices.iter().fold((f64::MIN, f64::MIN), |(x, y), v| (x.max(v.p.x), y.max(v.p.y)))
}

#[test]
fn full_ellipse_offset_follows_side_point() {
    // Horizontal and vertical major axis; the far end of the major axis lands at 12 outside, 8 inside.
    let cases = [([10.0, 0.0], [20.0, 0.0], 12.0), ([10.0, 0.0], [0.0, 0.0], 8.0), ([0.0, 10.0], [0.0, 20.0], 12.0), ([0.0, 10.0], [0.0, 0.0], 8.0)];
    for (major, side, expected) in cases {
        let mut s = Session::new();
        s.execute("ellipse", &json!({ "center": [0, 0], "major": major, "ratio": 0.5 })).unwrap();
        let h = last_handle(&s);
        s.execute("offset", &json!({ "handle": h, "distance": 2, "side": side })).unwrap();
        let (mx, my) = last_poly_max(&s);
        let got = if major[0] > 0.0 { mx } else { my };
        assert!((got - expected).abs() < 0.05, "major {major:?}, side {side:?}: far end at {got}, expected {expected}");
    }
}
