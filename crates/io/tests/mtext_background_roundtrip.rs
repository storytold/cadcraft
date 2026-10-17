//! MTEXT background colours and fill mask round-trip through DXF (groups 90, 63, 421, 45, 441, 430).

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, MText, MTextBackground, Space};
use cadcraft_geom::Vec3;

#[test]
fn mtext_background_color_roundtrips() {
    let mut d = Drawing::new_metric();
    let bg = MTextBackground {
        fill: 1,
        color: Some(Color::True(Rgb(10, 20, 30))),
        scale: 1.25,
        transparency: Some(40),
        color_book: Some("Pantone 123 C".into()),
    };
    let t = MText {
        insert: Vec3::new(10.0, 20.0, 0.0),
        height: 2.5,
        width: 30.0,
        attach: 1,
        rotation: 0.5,
        style: "Standard".into(),
        contents: "Custom background".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
        background: Some(bg.clone()),
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    let bytes = cadcraft_io::write(&d, "bg.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "bg.dxf").unwrap();
    let Some(EntityKind::MText(r)) = back.model.iter().map(|e| e.kind.clone()).next() else {
        panic!("no mtext found");
    };
    assert_eq!(r.background, Some(bg));
}

#[test]
fn mtext_background_mask_roundtrips() {
    let mut d = Drawing::new_metric();
    let bg = MTextBackground { fill: 2, color: None, scale: 1.5, transparency: None, color_book: None };
    let t = MText {
        insert: Vec3::new(5.0, 15.0, 0.0),
        height: 1.8,
        width: 25.0,
        attach: 4,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "Mask background".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
        background: Some(bg.clone()),
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    let bytes = cadcraft_io::write(&d, "mask.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "mask.dxf").unwrap();
    let Some(EntityKind::MText(r)) = back.model.iter().map(|e| e.kind.clone()).next() else {
        panic!("no mtext found");
    };
    assert_eq!(r.background, Some(bg));
}
