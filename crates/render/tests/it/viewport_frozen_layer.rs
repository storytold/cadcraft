//! A layout viewport whose own layer is frozen shows neither its border nor its contents, on
//! screen and when plotting; turned off, the layer hides only the border (issue #395).

use cadcraft_doc::{Circle, Common, Drawing, EntityKind, Space, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::{DisplayList, Options, VIEWPORT_CONTENT, build, build_plot};

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 5.0 })).unwrap();
    d.ensure_layer("VPORTS");
    let vp = Viewport {
        center: Vec3::new(5.0, 4.0, 0.0),
        width: 8.0,
        height: 6.0,
        view_center: Vec2::ZERO,
        view_height: 12.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    d.add(&Space::Paper("Layout1".into()), Common { layer: "VPORTS".into(), ..Default::default() }, EntityKind::Viewport(vp)).unwrap();
    d
}

/// (contents drawn, border drawn) on screen and in a plot.
fn shown(d: &Drawing) -> [(bool, bool); 2] {
    let space = Space::Paper("Layout1".into());
    let parts = |l: DisplayList| (l.prims.iter().any(|p| p.handle == VIEWPORT_CONTENT), l.prims.iter().any(|p| p.handle != VIEWPORT_CONTENT));
    [parts(build(d, &space, &Options::default())), parts(build_plot(d, &space, &Options::default()))]
}

#[test]
fn frozen_viewport_layer_hides_the_contents_off_only_the_border() {
    let mut d = drawing();
    assert_eq!(shown(&d), [(true, true); 2]);
    d.layer_mut("VPORTS").unwrap().on = false;
    assert_eq!(shown(&d), [(true, false); 2], "off: contents stay, border hidden");
    let l = d.layer_mut("VPORTS").unwrap();
    l.on = true;
    l.frozen = true;
    assert_eq!(shown(&d), [(false, false); 2], "frozen: contents hidden too");
}
