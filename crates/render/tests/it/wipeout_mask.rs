//! Wipeouts hide what is drawn before them and show their frame per WIPEOUTFRAME (issue #291).

use cadcraft_color::Rgb;
use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space, Wipeout};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{DisplayList, Kind, Options, build, build_plot};

fn line() -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 10.0, 0.0) })
}

fn wipeout() -> EntityKind {
    let b = [(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)];
    EntityKind::Wipeout(Wipeout { boundary: b.iter().map(|&(x, y)| Vec2::new(x, y)).collect() })
}

/// A diagonal line and a square wipeout over its middle, in the given draw order.
fn drawing(line_on_top: bool) -> Drawing {
    let mut d = Drawing::new_imperial();
    let order = if line_on_top { [wipeout(), line()] } else { [line(), wipeout()] };
    for k in order {
        d.add(&Space::Model, Common::default(), k).unwrap();
    }
    d
}

/// Dark (drawn) pixels inside and outside the wipeout, on white paper (colour 7 prints black).
fn dark_pixels(list: &DisplayList) -> (usize, usize) {
    let view = View { center: Vec2::new(5.0, 5.0), scale: 10.0, width: 120, height: 120 };
    let o = RasterOptions { background: Rgb(255, 255, 255), antialias: false, ..Default::default() };
    let pm = render(list, &view, &o).unwrap();
    let (mut inside, mut outside) = (0, 0);
    for y in 0..120u32 {
        for x in 0..120u32 {
            let c = pm.pixel(x, y).unwrap();
            if c.red() > 128 {
                continue;
            }
            // Screen (60, 60) is world (5, 5); 10 px per unit. Inside = clear of the frame.
            let (wx, wy) = (5.0 + (f64::from(x) + 0.5 - 60.0) / 10.0, 5.0 - (f64::from(y) + 0.5 - 60.0) / 10.0);
            if (2.5..7.5).contains(&wx) && (2.5..7.5).contains(&wy) {
                inside += 1;
            } else {
                outside += 1;
            }
        }
    }
    (inside, outside)
}

#[test]
fn wipeout_hides_what_is_drawn_before_it() {
    let (inside, outside) = dark_pixels(&build(&drawing(false), &Space::Model, &Options::default()));
    assert_eq!(inside, 0, "the line is hidden under the wipeout");
    assert!(outside > 0, "the line shows outside it");
    // Brought to front (drawn after the wipeout), the line shows on top.
    let (inside, _) = dark_pixels(&build(&drawing(true), &Space::Model, &Options::default()));
    assert!(inside > 0);
}

#[test]
fn implicit_and_explicit_closure_draw_the_same_mask_and_frame() {
    for reversed in [false, true] {
        let mut implicit = drawing(false);
        let handle = implicit.model.iter().find(|e| matches!(e.kind, EntityKind::Wipeout(_))).unwrap().handle;
        if reversed {
            implicit.model.modify(handle, |e| {
                let EntityKind::Wipeout(w) = &mut e.kind else { panic!("wipeout") };
                w.boundary.reverse();
            });
        }
        let mut explicit = implicit.clone();
        explicit.model.modify(handle, |e| {
            let EntityKind::Wipeout(w) = &mut e.kind else { panic!("wipeout") };
            w.boundary.push(w.boundary[0]);
        });
        for frame in [0, 1, 2] {
            implicit.header.set_i64("WIPEOUTFRAME", frame);
            explicit.header.set_i64("WIPEOUTFRAME", frame);
            for plot in [false, true] {
                let draw = if plot { build_plot } else { build };
                let a = draw(&implicit, &Space::Model, &Options::default());
                let b = draw(&explicit, &Space::Model, &Options::default());
                assert_eq!(a.prims, b.prims);
                assert_eq!(a.verts, b.verts);
                assert_eq!(a.tris, b.tris);
                for primitive in b.prims.iter().filter(|p| p.handle == handle && p.kind == Kind::Polyline) {
                    let points = b.points(primitive);
                    assert_eq!(points.len(), 5, "four corners and exactly one closing point");
                    assert_eq!(points.first(), points.last());
                }
                assert_eq!(dark_pixels(&a), dark_pixels(&b));
            }
        }
    }
}

#[test]
fn wipeoutframe_controls_the_frame() {
    // Polylines in the list: the line, plus the frame when it is drawn.
    let polylines = |d: &Drawing, plot: bool| {
        let l = if plot { build_plot(d, &Space::Model, &Options::default()) } else { build(d, &Space::Model, &Options::default()) };
        assert_eq!(l.prims.iter().filter(|p| p.kind == Kind::Mask).count(), 1);
        l.prims.iter().filter(|p| p.kind == Kind::Polyline).count()
    };
    let mut d = drawing(false);
    assert_eq!((polylines(&d, false), polylines(&d, true)), (2, 2), "default 1: shown and plotted");
    d.header.set_i64("WIPEOUTFRAME", 0);
    assert_eq!((polylines(&d, false), polylines(&d, true)), (1, 1), "0: no frame");
    d.header.set_i64("WIPEOUTFRAME", 2);
    assert_eq!((polylines(&d, false), polylines(&d, true)), (2, 1), "2: shown, not plotted");
}
