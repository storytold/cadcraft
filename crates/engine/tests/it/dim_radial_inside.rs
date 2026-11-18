//! DIMRADIUS / DIMDIAMETER: a dimension line location picked inside the circle puts the text
//! there, on the dimension line; outside, the text stays beyond the arrowhead.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::render::{DimGeometry, LineRole};
use serde_json::json;

fn last_dim(s: &Session) -> (Dimension, DimGeometry) {
    let d = s.doc().unwrap();
    match &d.model.last().unwrap().kind {
        EntityKind::Dimension(dm) => (dm.clone(), cadcraft_engine::render::dimension_in(d, dm)),
        other => panic!("not a dimension: {other:?}"),
    }
}

/// Whether a dimension line passes over `p` (on the x axis).
fn covers(g: &DimGeometry, x: f64) -> bool {
    g.lines_of(LineRole::Dim)
        .any(|l| l.windows(2).any(|w| w[0].y.abs() < 1e-9 && w[1].y.abs() < 1e-9 && w[0].x.min(w[1].x) < x && x < w[0].x.max(w[1].x)))
}

#[test]
fn radial_text_goes_inside_when_the_location_is_inside() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 10})).unwrap();

    // Typed: pick the circle, then a location inside.
    s.script("dimradius 10,0 4,0\n").unwrap();
    assert!(s.running.is_none());
    let (dm, g) = last_dim(&s);
    assert!(dm.user_text_pos);
    assert!(g.text_pos.dist(Vec2::new(4.0, 0.0)) < 1e-9, "{:?}", g.text_pos);
    // The dimension line runs from beyond the text to the arrowhead and stays inside the circle.
    assert!(covers(&g, 9.0) && !covers(&g, 4.0) && !covers(&g, 1.0));
    assert!(g.lines_of(LineRole::Dim).flatten().all(|p| p.len() <= 10.0 + 1e-9));

    // Outside: the text goes to the point too (dim_radial_outside.rs), beyond the arrowhead.
    s.script("dimradius 0,10 0,15\n").unwrap();
    assert!(s.running.is_none());
    let (_, g) = last_dim(&s);
    assert!(g.text_pos.y > 10.0, "{:?}", g.text_pos);

    // JSON form, diameter: the line spans the circle, broken around the text.
    let c = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    s.execute("dimdiameter", &json!({"handle": c, "at": [-3, 0]})).unwrap();
    let (_, g) = last_dim(&s);
    assert!(g.text_pos.dist(Vec2::new(-3.0, 0.0)) < 1e-9, "{:?}", g.text_pos);
    assert!(covers(&g, -9.5) && covers(&g, 9.5) && !covers(&g, -3.0));
}
