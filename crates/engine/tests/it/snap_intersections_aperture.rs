//! Finite and apparent intersection behavior around the cursor aperture.

use cadcraft_doc::{Common, Drawing, Ellipse, EntityKind, Line, Space};
use cadcraft_engine::snap::{self, SnapHit, mode};
use cadcraft_geom::{Vec2, Vec3};

fn add_line(d: &mut Drawing, a: Vec2, b: Vec2) {
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) })).unwrap();
}

fn add_ellipse(d: &mut Drawing, center: Vec2, major: Vec2, ratio: f64) {
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Ellipse(Ellipse { center: center.to3(0.0), major: major.to3(0.0), ratio, start: 0.0, end: std::f64::consts::TAU }),
    )
    .unwrap();
}

fn hit(d: &Drawing, cursor: Vec2, aperture: f64, osmode: u32) -> Option<SnapHit> {
    snap::osnap(d, &Space::Model, cursor, aperture, osmode, None, false)
}

#[test]
fn finite_intersection_on_the_aperture_boundary_is_included() {
    let mut d = Drawing::new_imperial();
    add_line(&mut d, Vec2::new(-1.0, 0.4), Vec2::new(1.0, 0.4));
    add_line(&mut d, Vec2::new(0.3, -1.0), Vec2::new(0.3, 1.0));

    let h = hit(&d, Vec2::ZERO, 0.5, mode::INT).expect("intersection on aperture boundary");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(Vec2::new(0.3, 0.4), 1e-12), "{h:?}");
}

#[test]
fn finite_intersection_just_outside_the_aperture_is_rejected() {
    let mut d = Drawing::new_imperial();
    let x = 0.300001;
    add_line(&mut d, Vec2::new(-1.0, 0.4), Vec2::new(1.0, 0.4));
    add_line(&mut d, Vec2::new(x, -1.0), Vec2::new(x, 1.0));

    assert!(hit(&d, Vec2::ZERO, 0.5, mode::INT).is_none());
}

#[test]
fn finite_line_parameter_tolerance_is_preserved() {
    let mut d = Drawing::new_imperial();
    let x = 1.0 + 5e-10;
    add_line(&mut d, Vec2::ZERO, Vec2::new(1.0, 0.0));
    add_line(&mut d, Vec2::new(x, -1.0), Vec2::new(x, 1.0));

    let h = hit(&d, Vec2::new(x, 0.0), 1e-9, mode::INT).expect("intersection within the existing line parameter tolerance");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(Vec2::new(x, 0.0), 1e-15), "{h:?}");
}

#[test]
fn tessellated_line_parameter_tolerance_is_preserved() {
    let mut d = Drawing::new_imperial();
    add_ellipse(&mut d, Vec2::ZERO, Vec2::new(10.0, 0.0), 0.5);
    let x = 10.0 + 5e-12;
    add_line(&mut d, Vec2::new(x, -1.0), Vec2::new(x, 1.0));

    let h = hit(&d, Vec2::new(x, 0.0), 1e-10, mode::INT).expect("finite intersection within a tessellated chord's parameter tolerance");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(Vec2::new(x, 0.0), 1e-10), "{h:?}");
}

#[test]
fn large_tessellated_chord_parameter_slack_is_not_pruned() {
    let mut d = Drawing::new_imperial();
    let radius = 1e12;
    let step = std::f64::consts::TAU / cadcraft_geom::arc_segments(radius, std::f64::consts::TAU, radius * 1e-3) as f64;
    let next = Vec2::new(-radius + radius * step.cos(), radius * 0.5 * step.sin());
    let t = 1.0 / next.x;
    assert!(t.abs() < 1e-9);
    let cursor = Vec2::new(1.0, t * next.y);
    add_ellipse(&mut d, Vec2::new(-radius, 0.0), Vec2::new(radius, 0.0), 0.5);
    add_line(&mut d, Vec2::new(1.0, cursor.y - 100.0), Vec2::new(1.0, cursor.y + 100.0));

    let h = hit(&d, cursor, 0.5, mode::INT).expect("finite intersection within a long tessellated chord's parameter tolerance");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(cursor, 1e-6), "{h:?}");
}

#[test]
fn tessellated_ellipse_intersection_near_cursor_is_kept() {
    let mut d = Drawing::new_imperial();
    add_ellipse(&mut d, Vec2::ZERO, Vec2::new(10.0, 0.0), 0.5);
    add_line(&mut d, Vec2::new(9.0, 0.0), Vec2::new(11.0, 0.0));

    let h = hit(&d, Vec2::new(10.0, 0.0), 0.05, mode::INT).expect("ellipse and line intersection");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(Vec2::new(10.0, 0.0), 1e-9), "{h:?}");
}

#[test]
fn ellipse_bounds_covering_cursor_do_not_create_a_curve_hit() {
    let mut d = Drawing::new_imperial();
    for i in 0..12usize {
        let radius = 50.0 + i as f64 * 10.0;
        let major = Vec2::from_angle(i as f64 * 0.17) * radius;
        add_ellipse(&mut d, Vec2::ZERO, major, 0.75 + (i % 5) as f64 * 0.04);
    }

    assert!(hit(&d, Vec2::ZERO, 0.1, mode::INT).is_none());
}

#[test]
fn apparent_intersection_keeps_segments_outside_the_finite_aperture() {
    let mut d = Drawing::new_imperial();
    add_line(&mut d, Vec2::new(0.15, 0.0), Vec2::new(0.18, 0.0));
    add_line(&mut d, Vec2::new(0.0, 0.15), Vec2::new(0.0, 0.18));

    let h = hit(&d, Vec2::ZERO, 0.1, mode::APP).expect("apparent intersection of extended lines");
    assert_eq!(h.mode, mode::APP);
    assert!(h.point.near(Vec2::ZERO, 1e-12), "{h:?}");
}

#[test]
fn real_intersection_keeps_precedence_when_app_is_also_enabled() {
    let mut d = Drawing::new_imperial();
    add_line(&mut d, Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0));
    add_line(&mut d, Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0));
    add_line(&mut d, Vec2::new(0.15, 0.04), Vec2::new(0.18, 0.04));
    add_line(&mut d, Vec2::new(0.04, 0.15), Vec2::new(0.04, 0.18));

    let h = hit(&d, Vec2::ZERO, 0.1, mode::INT | mode::APP).expect("real and apparent intersections");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(Vec2::ZERO, 1e-12), "{h:?}");
}

#[test]
fn line_and_circle_intersections_remain_available() {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::ZERO, radius: 2.0 })).unwrap();
    add_line(&mut d, Vec2::new(-3.0, 0.0), Vec2::new(3.0, 0.0));
    add_line(&mut d, Vec2::new(0.0, -3.0), Vec2::new(0.0, 3.0));

    let h = hit(&d, Vec2::ZERO, 0.1, mode::INT).expect("crossing lines inside a circle drawing");
    assert_eq!(h.mode, mode::INT);
    assert!(h.point.near(Vec2::ZERO, 1e-12), "{h:?}");
}

#[test]
fn non_finite_query_inputs_return_no_hit() {
    let mut d = Drawing::new_imperial();
    add_line(&mut d, Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0));
    add_line(&mut d, Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0));

    assert!(hit(&d, Vec2::ZERO, f64::NAN, mode::INT).is_none());
    assert!(hit(&d, Vec2::new(f64::INFINITY, 0.0), 0.1, mode::INT).is_none());
}
