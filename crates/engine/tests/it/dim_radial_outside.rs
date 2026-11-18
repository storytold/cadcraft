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

#[test]
fn radius_text_grip_moves_the_arrow_across_the_circle() {
    let mut s = Session::new();
    let circle = s.execute("circle", &json!({"center": [0, 0], "radius": 10})).unwrap();
    s.execute("dimradius", &json!({"handle": circle["handle"], "at": [-18, 0]})).unwrap();
    let h = s.doc().unwrap().model.last().unwrap().handle;
    let (original, _) = last_dim(&s);
    assert_eq!(original.p15.xy(), Vec2::new(-10.0, 0.0));
    let text_grip = s.grips_of(h).unwrap().iter().position(|p| *p == original.text_mid.xy()).unwrap();
    let at = Vec2::new(16.0, 12.0);
    let tip = Vec2::new(8.0, 6.0);

    s.start_grip_edit(&[(h, text_grip)]).unwrap();
    let preview = s.preview(at);
    let preview_dim = preview
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm),
            _ => None,
        })
        .unwrap();
    assert!(preview_dim.p15.xy().near(tip, 1e-9), "{:?}", preview_dim.p15);

    s.input(cadcraft_engine::Input::Point(at)).unwrap();
    let (placed, geometry) = last_dim(&s);
    assert!(placed.p15.xy().near(tip, 1e-9));
    assert_eq!(placed.defpt, original.defpt);
    assert_eq!(placed.text_mid.xy(), at);
    assert!((geometry.measurement - 10.0).abs() < 1e-9);
    let doc = s.doc().unwrap();
    let attachment = placed.assoc.iter().find(|a| a.point == "p15").unwrap();
    assert!(cadcraft_engine::assoc::snap_point(doc, attachment).unwrap().near(tip, 1e-9));
}
