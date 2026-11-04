//! PNG rasterising of POINT markers must never panic, wherever the point lands on the pixel grid
//! (issue #270).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Point, Space};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{Options, build};

fn add_point(d: &mut Drawing, x: f64, y: f64) {
    d.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(x, y, 0.0), angle: 0.0 })).unwrap();
}

/// Points at every 1/16-pixel offset (and on the image border) used to trip a tiny-skia
/// `debug_assert!` in its anti-aliased rectangle filler (debug builds only).
#[test]
fn points_at_subpixel_positions_do_not_panic() {
    let mut d = Drawing::new_imperial();
    for i in 0..16 {
        for j in 0..16 {
            add_point(&mut d, 10.0 + f64::from(i) * (1.0 + 1.0 / 16.0), 10.0 + f64::from(j) * (1.0 + 1.0 / 16.0));
        }
    }
    for (x, y) in [(0.0, 0.0), (0.3, 0.7), (-0.6, 5.0), (99.6, 99.6), (100.0, 100.0), (50.0, -0.4)] {
        add_point(&mut d, x, y);
    }
    let list = build(&d, &Space::Model, &Options::default());
    let view = View { center: Vec2::new(50.0, 50.0), scale: 1.0, width: 100, height: 100 };
    for antialias in [true, false] {
        let pm = render(&list, &view, &RasterOptions { antialias, ..Default::default() }).unwrap();
        // The marker at (10, 10) is drawn in the foreground colour.
        let px = pm.pixel(10, 90).unwrap();
        assert!(px.red() > 128 && px.green() > 128, "{px:?}");
    }
}

/// The reported case: a POINT at (3,0) in a drawing exported to a 2400 x 1600 PNG.
#[test]
fn point_in_large_png_export_does_not_panic() {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 7.0, 0.0) })).unwrap();
    add_point(&mut d, 3.0, 0.0);
    let list = build(&d, &Space::Model, &Options::default());
    for (w, h) in [(2400, 1600), (1601, 999), (333, 77)] {
        let view = View::fit(&list.bounds, w, h, 0.05);
        let png = cadcraft_render::raster::render_png(&list, &view, &RasterOptions::default()).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
