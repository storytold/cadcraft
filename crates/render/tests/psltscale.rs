//! PSLTSCALE: model-space linetypes seen through a layout viewport (issue #257).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space, Viewport, library};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::{Kind, Options, VIEWPORT_CONTENT, build};

/// A metric drawing with a 1000-unit DASHED line (pattern 0.5 on, 0.25 off) in model space,
/// seen through a 1:100 viewport on Layout1.
fn drawing(psltscale: i64) -> Drawing {
    let mut d = Drawing::new_metric();
    d.header.set_i64("PSLTSCALE", psltscale);
    d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
    let dashed = Common { linetype: "DASHED".into(), ..Default::default() };
    d.add(&Space::Model, dashed, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1000.0, 0.0, 0.0) })).unwrap();
    // 190 x 90 paper units showing a model window 9000 high: scale 1:100.
    let vp = Viewport {
        center: Vec3::new(105.0, 55.0, 0.0),
        width: 190.0,
        height: 90.0,
        view_center: Vec2::new(500.0, 0.0),
        view_height: 9000.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
    };
    d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
    d
}

/// Lengths (paper units) of the dashes drawn inside the viewport.
fn dash_lengths(d: &Drawing) -> Vec<f64> {
    let l = build(d, &Space::Paper("Layout1".into()), &Options::default());
    l.prims
        .iter()
        .filter(|p| p.handle == VIEWPORT_CONTENT && p.kind == Kind::Polyline)
        .map(|p| l.points(p).windows(2).map(|w| w[0].dist(w[1])).sum())
        .collect()
}

#[test]
fn psltscale_on_keeps_paper_dash_length() {
    let dashes = dash_lengths(&drawing(1));
    // 1000 model units = 10 paper units; 0.75-unit pattern on paper → about 13 dashes of 0.5.
    assert!((12..=15).contains(&dashes.len()), "{} dashes", dashes.len());
    assert!(dashes.iter().take(dashes.len() - 1).all(|l| (l - 0.5).abs() < 1e-6), "{dashes:?}");
}

#[test]
fn psltscale_off_scales_dashes_with_the_viewport() {
    let dashes = dash_lengths(&drawing(0));
    // Model-space dash length 0.5, shrunk 100x by the viewport.
    assert!(dashes.len() > 1000, "{} dashes", dashes.len());
    assert!(dashes.iter().take(dashes.len() - 1).all(|l| (l - 0.005).abs() < 1e-6));
}

#[test]
fn model_space_ignores_psltscale() {
    let d = drawing(1);
    let l = build(&d, &Space::Model, &Options::default());
    let first = l.prims.iter().find(|p| p.kind == Kind::Polyline).map(|p| l.points(p).to_vec()).unwrap();
    assert!((first[0].dist(first[1]) - 0.5).abs() < 1e-9);
}
