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

#[test]
fn a_cleared_background_stays_cleared() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), hatch(Some(Color::Index(3)))).unwrap();
    let mut back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let h = back.model.iter().next().unwrap().handle;
    back.modify_entity(h, |e| {
        if let EntityKind::Hatch(x) = &mut e.kind {
            x.background = None;
        }
    })
    .unwrap();
    let text = cadcraft_io::write_dxf(&back);
    // Only the application's registration is left; no hatch carries the colour any more.
    let groups = text.lines().map(str::trim).collect::<Vec<_>>();
    assert!(!groups.windows(2).any(|w| w[0] == "1001" && w[1] == "HATCHBACKGROUNDCOLOR"), "the old background is not written again");
    let again = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let h = again.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
    assert_eq!(h.background, None);
}

/// Another program changed HATCHBACKGROUNDCOLOR but kept CADCraft's own copy: the standard
/// value wins (#2323 review).
#[test]
fn autocads_background_wins_over_a_stale_private_copy() {
    // The 1071 value AutoCAD's xdata holds for a colour.
    let acad_value = |c: Color| {
        let mut d = Drawing::new_metric();
        d.add(&Space::Model, Common::default(), hatch(Some(c))).unwrap();
        let text = cadcraft_io::write_dxf(&d);
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let at = lines.windows(2).rposition(|w| w[0] == "1001" && w[1] == "HATCHBACKGROUNDCOLOR").unwrap();
        let v = lines[at + 2..].windows(2).find(|w| w[0] == "1071").unwrap()[1].to_string();
        (text, v)
    };
    let (text, old) = acad_value(Color::Index(3));
    let (_, new) = acad_value(Color::Index(5));
    let edited = text.replacen(&format!("1071\n{old}\n"), &format!("1071\n{new}\n"), 1).replacen(
        &format!("1071\r\n{old}\r\n"),
        &format!("1071\r\n{new}\r\n"),
        1,
    );
    assert_ne!(edited, text);
    let back = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
    let h = back.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
    assert_eq!(h.background, Some(Color::Index(5)));
}
