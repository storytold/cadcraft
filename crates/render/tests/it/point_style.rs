//! POINT display: point style (PDMODE) and size (PDSIZE) (issue #261).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Point, Space};
use cadcraft_geom::{Bounds2, Vec3};
use cadcraft_render::{DisplayList, Kind, Options, build};

/// A point at (5,5) and a vertical line 100 units long (the extents are 100 high).
fn drawing(pdmode: i64, pdsize: f64) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.header.set_i64("PDMODE", pdmode);
    d.header.set_f64("PDSIZE", pdsize);
    d.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(5.0, 5.0, 0.0), angle: 0.0 })).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(0.0, 100.0, 0.0) })).unwrap();
    d
}

/// Dots and the bounds of the point's line work (the line along x = 0 is left out).
fn point_parts(l: &DisplayList) -> (usize, usize, Bounds2) {
    let dots = l.prims.iter().filter(|p| p.kind == Kind::Point).count();
    let lines: Vec<_> = l.prims.iter().filter(|p| p.kind == Kind::Polyline && l.points(p).iter().any(|q| q.x > 1.0)).collect();
    let b = Bounds2::from_points(lines.iter().flat_map(|p| l.points(p).iter().copied()));
    (dots, lines.len(), b)
}

fn close(b: &Bounds2, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    [b.min.x - x0, b.min.y - y0, b.max.x - x1, b.max.y - y1].iter().all(|v| v.abs() < 1e-3)
}

#[test]
fn cross_in_circle_with_absolute_size() {
    let l = build(&drawing(35, 1.0), &Space::Model, &Options::default());
    let (dots, lines, b) = point_parts(&l);
    assert_eq!((dots, lines), (0, 3), "an X (two lines) and a circle, no dot");
    assert!(close(&b, 4.5, 4.5, 5.5, 5.5), "{b:?}");
}

#[test]
fn dot_and_nothing() {
    let (dots, lines, _) = point_parts(&build(&drawing(0, 1.0), &Space::Model, &Options::default()));
    assert_eq!((dots, lines), (1, 0));
    let (dots, lines, _) = point_parts(&build(&drawing(1, 1.0), &Space::Model, &Options::default()));
    assert_eq!((dots, lines), (0, 0));
    // 96 = square and circle around a dot.
    let (dots, lines, b) = point_parts(&build(&drawing(96, 2.0), &Space::Model, &Options::default()));
    assert_eq!((dots, lines), (1, 2));
    assert!(close(&b, 4.0, 4.0, 6.0, 6.0), "{b:?}");
}

#[test]
fn relative_sizes_follow_the_view_height() {
    // PDSIZE -10: 10 % of a 50-unit view = 5 across.
    let opts = Options { view_height: 50.0, ..Options::default() };
    let (_, lines, b) = point_parts(&build(&drawing(2, -10.0), &Space::Model, &opts));
    assert_eq!(lines, 2);
    assert!(close(&b, 2.5, 2.5, 7.5, 7.5), "{b:?}");
    // PDSIZE 0 without a view height: 5 % of the 100-unit extents = 5 across.
    let (_, _, b) = point_parts(&build(&drawing(3, 0.0), &Space::Model, &Options::default()));
    assert!(close(&b, 2.5, 2.5, 7.5, 7.5), "{b:?}");
}

#[test]
fn hostile_point_settings_do_not_panic() {
    for mode in [i64::MIN, -1, 5, 31, 127, i64::MAX] {
        for size in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e300, 1e300, 0.0, -5.0] {
            let l = build(&drawing(mode, size), &Space::Model, &Options::default());
            assert!(l.verts.iter().all(|v| v.is_finite()), "PDMODE {mode} PDSIZE {size}");
        }
    }
}
