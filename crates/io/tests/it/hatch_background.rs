//! A hatch's background colour is written as AutoCAD keeps it (HATCHBACKGROUNDCOLOR xdata) and
//! read back from it.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2};

fn hatch(bg: Option<Color>) -> EntityKind {
    let sq = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    EntityKind::Hatch(Hatch {
        pattern: "ANSI31".into(),
        solid: false,
        loops: vec![HatchLoop { vertices: sq.iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(), outer: true, sources: Vec::new() }],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: bg,
        pattern_lines: Vec::new(),
    })
}

#[test]
fn background_colour_round_trips_and_is_written_for_autocad() {
    for bg in [Color::Index(3), Color::True(Rgb(250, 240, 10))] {
        let mut d = Drawing::new_metric();
        d.add(&Space::Model, Common::default(), hatch(Some(bg))).unwrap();
        let text = cadcraft_io::write_dxf(&d);
        assert!(text.contains("HATCHBACKGROUNDCOLOR"));
        let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
        let h = back.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
        assert_eq!(h.background, Some(bg));
    }
}

#[test]
fn autocads_background_xdata_alone_gives_the_colour() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), hatch(Some(Color::Index(5)))).unwrap();
    // Without CADCraft's own xdata (a file AutoCAD wrote): only HATCHBACKGROUNDCOLOR is left.
    let text = cadcraft_io::write_dxf(&d).replace("1000\r\nHATCH\r\n", "1000\r\nOTHER\r\n").replace("1000\nHATCH\n", "1000\nOTHER\n");
    assert!(text.contains("OTHER"), "CADCraft's own background is out of the way");
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let h = back.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
    assert_eq!(h.background, Some(Color::Index(5)));
}
