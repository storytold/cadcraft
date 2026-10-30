//! A layout viewport with a view twist (DVIEW TWist inside it; DXF VIEWPORT group 51) shows model
//! space turned on the sheet, on screen, when plotting and in the canvas's active-viewport list.

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space, Viewport};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use cadcraft_render::{DisplayList, Options, VIEWPORT_CONTENT, build, build_plot, viewport_content};

fn drawing(twist_deg: f64) -> (Drawing, cadcraft_doc::Handle) {
    let mut d = Drawing::new_imperial();
    // A model line along +X, 2 long, from the view centre.
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(2.0, 0.0, 0.0) })).unwrap();
    let vp = Viewport {
        center: Vec3::new(5.0, 4.0, 0.0),
        width: 8.0,
        height: 6.0,
        view_center: Vec2::ZERO,
        view_height: 12.0,
        id: 2,
        twist: twist_deg.to_radians(),
        ..Viewport::default()
    };
    let h = d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
    (d, h)
}

/// The bounds of the viewport's contents in a list.
fn contents(l: &DisplayList) -> Bounds2 {
    Bounds2::from_points(l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).flat_map(|p| l.points(p).to_vec()))
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < 1e-6
}

#[test]
fn a_twisted_viewport_turns_its_contents_on_screen_and_in_plots() {
    let space = Space::Paper("Layout1".into());
    // Untwisted: the line runs right from the viewport centre (scale 1/2).
    let (d, _) = drawing(0.0);
    let b = contents(&build(&d, &space, &Options::default()));
    assert!(near(b.min, Vec2::new(5.0, 4.0)) && near(b.max, Vec2::new(6.0, 4.0)), "{b:?}");
    // Turned 90° counterclockwise: it runs up.
    let (d, h) = drawing(90.0);
    for l in [build(&d, &space, &Options::default()), build_plot(&d, &space, &Options::default())] {
        let b = contents(&l);
        assert!(near(b.min, Vec2::new(5.0, 4.0)) && near(b.max, Vec2::new(5.0, 5.0)), "{b:?}");
    }
    // The active viewport's own list, built for an area around it, agrees.
    let area = Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 8.0));
    let l = viewport_content(&d, &space, h, area, &Options::default());
    let b = Bounds2::from_points(l.prims.iter().flat_map(|p| l.points(p).to_vec()));
    assert!(near(b.min, Vec2::new(5.0, 4.0)) && near(b.max, Vec2::new(5.0, 5.0)), "{b:?}");
}

#[test]
fn model_and_paper_points_map_both_ways_through_a_twist() {
    let (d, h) = drawing(30.0);
    let Some(EntityKind::Viewport(vp)) = d.entity(h).map(|e| e.kind.clone()) else { panic!() };
    let m = Vec2::new(3.0, -1.5);
    let p = vp.model_to_paper(m);
    assert!(near(vp.paper_to_model(p), m));
    assert!(near(vp.model_to_paper_matrix().apply(m), p));
    // +X in model space points 30° above the sheet's +X.
    let dir = vp.model_to_paper(Vec2::new(1.0, 0.0)) - vp.model_to_paper(Vec2::ZERO);
    assert!((dir.y.atan2(dir.x).to_degrees() - 30.0).abs() < 1e-9);
}
