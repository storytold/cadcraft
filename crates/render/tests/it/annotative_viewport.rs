//! Annotative objects seen through layout viewports (#468): they draw at their paper size in
//! every viewport whose scale they support, and not at all in others when ANNOALLVISIBLE is 0.

use cadcraft_doc::{AnnoScale, Common, Drawing, EntityKind, Space, Text, Viewport};
use cadcraft_geom::{Bounds2, Vec3};
use cadcraft_render::{Options, VIEWPORT_CONTENT, build};

/// A viewport on Layout1, 100 × 100 paper units, showing a model window `100 × scale`.
fn viewport(x: f64, scale: f64) -> Viewport {
    serde_json::from_value(serde_json::json!({
        "center": [x, 50.0, 0.0],
        "width": 100.0,
        "height": 100.0,
        "viewCenter": [0.0, 0.0],
        "viewHeight": 100.0 * scale,
        "id": 2,
    }))
    .unwrap()
}

/// A metric drawing with annotative text 2.5 high on paper supporting 1:50 and 1:100, seen
/// through one viewport at `scale`.
fn drawing(scale: f64, all_visible: bool) -> Drawing {
    let mut d = Drawing::new_metric();
    d.header.set_i64("ANNOALLVISIBLE", i64::from(all_visible));
    let anno = Common { anno_scales: vec![AnnoScale::new("1:50", 1.0, 50.0), AnnoScale::new("1:100", 1.0, 100.0)], ..Default::default() };
    let text = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.5,
        value: "ROOM".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(&Space::Model, anno, EntityKind::Text(text)).unwrap();
    d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(viewport(50.0, scale))).unwrap();
    d
}

/// Bounds of the model-space content drawn in the viewport, in paper units.
fn content(d: &Drawing) -> Bounds2 {
    let l = build(d, &Space::Paper("Layout1".into()), &Options::default());
    let mut b = Bounds2::EMPTY;
    for p in l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT) {
        for q in l.points(p) {
            b.add(*q);
        }
    }
    b
}

#[test]
fn same_paper_height_at_every_supported_scale() {
    let at_50 = content(&drawing(50.0, false));
    let at_100 = content(&drawing(100.0, false));
    assert!(!at_50.is_empty() && !at_100.is_empty());
    assert!((at_50.height() / at_100.height() - 1.0).abs() < 1e-9, "{} vs {}", at_50.height(), at_100.height());
    assert!((at_50.width() / at_100.width() - 1.0).abs() < 1e-9);
    // Glyphs span most of the 2.5 text height on paper.
    assert!(at_50.height() > 1.5 && at_50.height() < 3.5, "{}", at_50.height());
}

#[test]
fn unsupported_viewport_scale_hides_or_shows_at_the_objects_scale() {
    assert!(content(&drawing(20.0, false)).is_empty());
    // ANNOALLVISIBLE 1: drawn at its first scale (1:50), so 50/20 = 2.5× its paper size here.
    let shown = content(&drawing(20.0, true));
    let at_50 = content(&drawing(50.0, false));
    assert!((shown.height() / at_50.height() - 2.5).abs() < 1e-9, "{} vs {}", shown.height(), at_50.height());
}

#[test]
fn model_space_uses_cannoscale() {
    let mut d = drawing(50.0, false);
    let list = |d: &Drawing| build(d, &Space::Model, &Options::default()).prims.len();
    // CANNOSCALE 1:1 isn't supported: hidden.
    assert_eq!(list(&d), 0);
    d.header.set_str("CANNOSCALE", "1:100");
    assert!(list(&d) > 0);
    let b = build(&d, &Space::Model, &Options::default()).bounds;
    assert!(b.height() > 150.0 && b.height() < 350.0, "{}", b.height());
}
