//! The DXF reader drops the paper-space viewport the writer adds (marked `1000 PAPERVIEW` in
//! CADCraft xdata) but keeps a real viewport whose frozen or recoloured layer is named PAPERVIEW.

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};

fn viewport(id: u32, frozen_layers: Vec<String>, layer_colors: Vec<(String, Color)>) -> EntityKind {
    EntityKind::Viewport(Viewport {
        center: Vec3::new(50.0, 40.0, 0.0),
        width: 80.0,
        height: 60.0,
        view_center: Vec2::new(10.0, 10.0),
        view_height: 50.0,
        id,
        locked: false,
        frozen_layers,
        layer_colors,
    })
}

fn viewports(l: &Layout) -> Vec<Viewport> {
    l.entities.iter().filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some(v.clone()) } else { None }).collect()
}

#[test]
fn viewport_with_a_layer_named_paperview_survives_dxf_roundtrip() {
    let mut d = Drawing::new_metric();
    d.ensure_layer("PAPERVIEW");
    let sheet = Space::Paper(d.layouts[0].name.clone());
    d.add(&sheet, Common::default(), viewport(2, vec!["PAPERVIEW".into()], vec![])).unwrap();
    d.add(&sheet, Common::default(), viewport(3, vec![], vec![("PAPERVIEW".into(), Color::Index(1))])).unwrap();

    let text = cadcraft_io::write_dxf(&d);
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let vps = viewports(&back.layouts[0]);
    assert_eq!(vps.iter().map(|v| v.id).collect::<Vec<_>>(), [2, 3], "only the added sheet viewport is dropped");
    assert_eq!(vps[0].frozen_layers, ["PAPERVIEW"]);
    assert_eq!(vps[1].layer_colors, [("PAPERVIEW".to_string(), Color::Index(1))]);
}
