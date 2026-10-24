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
        clip: None,
        on: true,
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

#[test]
fn a_viewport_looks_at_its_view_target_plus_its_view_centre() {
    // AutoCAD often writes the view centre (12/22) near zero and the view target (17/27) where
    // the drawing is: the model point at the viewport's middle is their sum.
    let mut d = Drawing::new_metric();
    let sheet = Space::Paper(d.layouts[0].name.clone());
    d.add(&sheet, Common::default(), viewport(2, vec![], vec![])).unwrap();
    let text = cadcraft_io::write_dxf(&d);
    // The viewport's view centre 10,10 becomes 2,3 with the target at 8,7: rewrite the group
    // values after "12" and "22" in the viewport whose centre (10) is 50.
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let start = lines.windows(2).position(|w| w[0].trim() == "10" && w[1].trim().parse::<f64>().ok() == Some(50.0)).expect("the viewport");
    let mut done = false;
    let mut i = start;
    while i + 1 < lines.len() && !done {
        if lines[i].trim() == "12" {
            out[i + 1] = "2.0".into();
        }
        if lines[i].trim() == "22" {
            out[i + 1] = "3.0".into();
            out[i + 1].push_str("\n 17\n8.0\n 27\n7.0");
            done = true;
        }
        i += 2;
    }
    assert!(done, "the writer's view centre groups");
    let edited = out.join("\n");
    let back = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
    let v = viewports(&back.layouts[0]).into_iter().find(|v| v.id == 2).unwrap();
    assert!((v.view_center.x - 10.0).abs() < 1e-9 && (v.view_center.y - 10.0).abs() < 1e-9, "{:?}", v.view_center);
}
