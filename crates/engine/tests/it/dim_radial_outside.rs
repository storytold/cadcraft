//! DIMRADIUS / DIMDIAMETER: a dimension line location picked outside the circle puts the text
//! at that point (group 11, flag 70 bit 128), as for a location inside; the leader stops short
//! of the text instead of running into it.

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

/// Group 70 of every DIMENSION in the saved DXF.
fn dxf_flags(s: &Session) -> Vec<i64> {
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut flags = Vec::new();
    let mut in_dim = false;
    for p in lines.chunks(2) {
        match p {
            ["0", v] => in_dim = *v == "DIMENSION",
            ["70", v] if in_dim => flags.push(v.parse().unwrap()),
            _ => {}
        }
    }
    flags
}

fn assert_text_at(s: &Session, at: Vec2) {
    assert!(s.running.is_none());
    let (dm, g) = last_dim(s);
    assert!(dm.user_text_pos);
    assert!(dm.text_mid.xy().dist(at) < 1e-9, "{:?}", dm.text_mid);
    assert!(g.text_pos.dist(at) < 1e-9, "{:?}", g.text_pos);
    let nearest = g.lines_of(LineRole::Dim).flatten().map(|p| p.dist(at)).fold(f64::INFINITY, f64::min);
    assert!(nearest > 0.05, "the leader runs into the text: {nearest}");
}

#[test]
fn radial_text_goes_to_a_location_outside_the_circle() {
    let mut s = Session::new();
    s.script("circle 50,50 25\n").unwrap();
    s.script("dimradius 75,50 100,80\n").unwrap();
    assert_text_at(&s, Vec2::new(100.0, 80.0));

    s.script("circle 300,50 25\n").unwrap();
    s.script("dimdiameter 325,50 350,80\n").unwrap();
    assert_text_at(&s, Vec2::new(350.0, 80.0));

    s.execute("arc", &json!({"center": [50, 200], "radius": 25, "start": 0, "end": 90})).unwrap();
    s.script("dimradius 75,200 100,230\n").unwrap();
    assert_text_at(&s, Vec2::new(100.0, 230.0));

    // Radius 4 | 32 | 128, diameter 3 | 32 | 128, as AutoCAD writes them.
    assert_eq!(dxf_flags(&s), vec![164, 163, 164]);
}
