use crate::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn angles_normalize() {
    assert!(close(norm_angle(-PI / 2.0), 3.0 * PI / 2.0));
    assert!(close(ccw_sweep(0.0, 0.0), TAU));
    assert!(angle_in_sweep(0.1, 6.0, 0.5));
    assert!(!angle_in_sweep(3.0, 6.0, 0.5));
}

#[test]
fn mat_inverse_roundtrip() {
    let m = Mat3::rotate_about(Vec2::new(3.0, 4.0), 0.7).then(Mat3::scale(2.0, 2.0));
    let inv = m.inverse().unwrap();
    let p = Vec2::new(10.0, -2.0);
    let q = inv.apply(m.apply(p));
    assert!(q.near(p, 1e-9));
}

#[test]
fn mirror_matrix() {
    let m = Mat3::mirror(Vec2::ZERO, Vec2::new(1.0, 1.0));
    assert!(m.apply(Vec2::new(1.0, 0.0)).near(Vec2::new(0.0, 1.0), 1e-12));
    assert!(m.is_mirroring());
}

#[test]
fn circle_through_three_points() {
    let c = Circle::from_3_points(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0)).unwrap();
    assert!(c.center.near(Vec2::ZERO, 1e-9));
    assert!(close(c.radius, 1.0));
    assert!(Circle::from_3_points(Vec2::ZERO, Vec2::X, Vec2::X * 2.0).is_none());
}

#[test]
fn arc_three_points_direction() {
    let a = Arc::from_3_points(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0)).unwrap();
    assert!(close(a.sweep(), PI));
    assert!(a.mid_point().near(Vec2::new(0.0, 1.0), 1e-9));
    let b = Arc::from_3_points(Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0), Vec2::new(-1.0, 0.0)).unwrap();
    assert!(b.mid_point().near(Vec2::new(0.0, -1.0), 1e-9));
}

#[test]
fn arc_bounds_include_quadrants() {
    let a = Arc::new(Vec2::ZERO, 2.0, 0.0, PI);
    let b = a.bounds();
    assert!(close(b.max.y, 2.0));
    assert!(close(b.min.y, 0.0));
}

#[test]
fn bulge_semicircle() {
    let (arc, ccw) = bulge_to_arc(Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), 1.0).unwrap();
    assert!(ccw);
    assert!(arc.center.near(Vec2::new(1.0, 0.0), 1e-9));
    assert!(close(arc.radius, 1.0));
    // CCW from (0,0) to (2,0) around (1,0) passes below.
    assert!(arc.mid_point().near(Vec2::new(1.0, -1.0), 1e-9));
    let (arc2, ccw2) = bulge_to_arc(Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), -1.0).unwrap();
    assert!(!ccw2);
    assert!(arc2.mid_point().near(Vec2::new(1.0, 1.0), 1e-9));
}

#[test]
fn bulge_quarter_arc() {
    let b = arc_to_bulge(PI / 2.0);
    let (arc, _) = bulge_to_arc(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), b).unwrap();
    assert!(arc.center.near(Vec2::ZERO, 1e-9));
    assert!(close(arc.sweep(), PI / 2.0));
}

#[test]
fn polyline_area_and_length() {
    let p = Polyline::from_points(&[Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0)], true);
    assert!(close(p.area(), 12.0));
    assert!(close(p.len(), 14.0));
    assert!(p.contains(Vec2::new(1.0, 1.0)));
    assert!(!p.contains(Vec2::new(5.0, 1.0)));
}

#[test]
fn line_intersections() {
    let a = Line::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
    let b = Line::new(Vec2::new(0.0, 10.0), Vec2::new(10.0, 0.0));
    assert!(line_line(&a, &b).unwrap().near(Vec2::new(5.0, 5.0), 1e-9));
    let c = Line::new(Vec2::new(20.0, 0.0), Vec2::new(30.0, -10.0));
    assert!(line_line(&a, &c).is_none());
}

#[test]
fn line_circle_hits() {
    let l = Line::new(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0));
    let hits = line_circle(&l, &Circle::new(Vec2::ZERO, 2.0));
    assert_eq!(hits.len(), 2);
    let segs = intersect_segments(&Segment::Line(l), &Segment::Arc { arc: Arc::new(Vec2::ZERO, 2.0, 0.0, PI / 2.0), ccw: true });
    assert_eq!(segs.len(), 1);
    assert!(segs[0].near(Vec2::new(2.0, 0.0), 1e-9));
}

#[test]
fn circle_circle_hits() {
    let h = circle_circle(&Circle::new(Vec2::ZERO, 5.0), &Circle::new(Vec2::new(8.0, 0.0), 5.0));
    assert_eq!(h.len(), 2);
    assert!(h.iter().all(|p| close(p.x, 4.0) && close(p.y.abs(), 3.0)));
}

#[test]
fn spline_interpolates_fit_points() {
    let fit = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 1.0), Vec2::new(4.0, 4.0), Vec2::new(6.0, 0.0)];
    let s = Spline::from_fit_points(&fit);
    assert!(s.is_valid());
    let pts = s.tessellate(1e-4);
    for f in fit {
        let d = pts.iter().map(|p| p.dist(f)).fold(f64::INFINITY, f64::min);
        assert!(d < 0.05, "fit point {f:?} missed by {d}");
    }
    assert!(s.eval(0.0).near(fit[0], 1e-9));
    assert!(s.eval(1.0).near(fit[4], 1e-9));
}

#[test]
fn ellipse_points() {
    let e = Ellipse::full(Vec2::ZERO, Vec2::new(4.0, 0.0), 0.5);
    assert!(e.at_param(PI / 2.0).near(Vec2::new(0.0, 2.0), 1e-9));
    let b = e.bounds();
    assert!((b.width() - 8.0).abs() < 1e-3);
}

#[test]
fn segment_offset() {
    let s = Segment::Arc { arc: Arc::new(Vec2::ZERO, 5.0, 0.0, PI), ccw: true };
    let o = s.offset(1.0).unwrap();
    if let Segment::Arc { arc, .. } = o {
        assert!(close(arc.radius, 4.0))
    } else {
        panic!()
    }
    assert!(s.offset(6.0).is_none());
}

#[test]
fn arc_segments_bounded() {
    assert_eq!(arc_segments(0.0, 1.0, 0.1), 1);
    assert!(arc_segments(1e9, TAU, 1e-12) <= 4096);
    assert!(arc_segments(f64::NAN, TAU, 0.1) == 1);
}

#[test]
fn ocs_identity_for_z() {
    assert_eq!(Mat4::ocs(Vec3::Z), Mat4::IDENTITY);
    let m = Mat4::ocs(Vec3::new(0.0, 0.0, -1.0));
    let p = m.apply(Vec3::new(1.0, 0.0, 0.0));
    assert!((p.x + 1.0).abs() < 1e-9);
}

#[test]
fn common_tangents_of_two_circles() {
    // Distance from `c` to the infinite line through `p`, `q`.
    let dist = |c: Vec2, p: Vec2, q: Vec2| ((q - p).cross(c - p) / p.dist(q)).abs();
    let (a, b) = (Circle::new(Vec2::ZERO, 10.0), Circle::new(Vec2::new(30.0, 0.0), 4.0));
    let ts = a.common_tangents(&b);
    assert_eq!(ts.len(), 4, "separate circles: 2 outer + 2 inner tangents");
    for (p, q) in &ts {
        assert!(close(p.dist(a.center), 10.0) && close(q.dist(b.center), 4.0));
        assert!(close(dist(a.center, *p, *q), 10.0) && close(dist(b.center, *p, *q), 4.0));
    }
    // Overlapping circles only have the two outer tangents.
    assert_eq!(a.common_tangents(&Circle::new(Vec2::new(12.0, 0.0), 4.0)).len(), 2);
    // Concentric, nested, touching inside, non-finite: none, and no panic.
    assert!(a.common_tangents(&Circle::new(Vec2::ZERO, 3.0)).is_empty());
    assert!(a.common_tangents(&Circle::new(Vec2::new(1.0, 0.0), 3.0)).is_empty());
    assert!(a.common_tangents(&Circle::new(Vec2::new(6.0, 0.0), 4.0)).is_empty());
    assert!(a.common_tangents(&Circle::new(Vec2::new(f64::NAN, 0.0), 4.0)).is_empty());
    assert!(a.common_tangents(&Circle::new(Vec2::new(f64::INFINITY, 0.0), 4.0)).is_empty());
    // A zero-radius circle (a point): the tangents from that point.
    let z = a.common_tangents(&Circle::new(Vec2::new(30.0, 0.0), 0.0));
    assert!(!z.is_empty() && z.iter().all(|(p, q)| close(dist(a.center, *p, *q), 10.0) && q.near(Vec2::new(30.0, 0.0), 1e-9)));
}

#[test]
fn circle_three_points_preserves_large_world_coordinates() {
    // A unit circle near a survey coordinate. The absolute-square formula suffers
    // catastrophic cancellation and returns a center tens of millions of units away.
    let origin = 1.0e12;
    let c = Circle::from_3_points(Vec2::new(origin + 1.0, origin), Vec2::new(origin, origin + 1.0), Vec2::new(origin - 1.0, origin)).unwrap();
    assert!(c.center.near(Vec2::new(origin, origin), 1e-3), "{c:?}");
    assert!((c.radius - 1.0).abs() < 1e-3, "{c:?}");
}

#[test]
fn circle_three_points_accepts_small_valid_triangles() {
    let r = 1.0e-7;
    let c = Circle::from_3_points(Vec2::new(r, 0.0), Vec2::new(0.0, r), Vec2::new(-r, 0.0)).unwrap();
    assert!(c.center.near(Vec2::ZERO, 1e-12), "{c:?}");
    assert!((c.radius - r).abs() < 1e-12, "{c:?}");
    assert!(Circle::from_3_points(Vec2::ZERO, Vec2::new(r, 0.0), Vec2::new(2.0 * r, 0.0)).is_none());
    assert!(Circle::from_3_points(Vec2::ZERO, Vec2::X, Vec2::new(f64::NAN, 0.0)).is_none());
}

#[test]
fn ellipse_bounds_find_analytic_extrema() {
    // A tilted ellipse's extrema generally fall between tessellated samples.
    let center = Vec2::new(7.0, -11.0);
    let e = Ellipse::full(center, Vec2::new(3.0, 4.0), 0.5);
    let bounds = e.bounds();
    let dx = 13.0_f64.sqrt(); // hypot(major.x, minor.x)
    let dy = 18.25_f64.sqrt(); // hypot(major.y, minor.y)
    assert!((bounds.min.x - (center.x - dx)).abs() < 1e-12);
    assert!((bounds.max.x - (center.x + dx)).abs() < 1e-12);
    assert!((bounds.min.y - (center.y - dy)).abs() < 1e-12);
    assert!((bounds.max.y - (center.y + dy)).abs() < 1e-12);
}

#[test]
fn elliptical_arc_bounds_exclude_extrema_outside_the_sweep() {
    let e = Ellipse { center: Vec2::ZERO, major: Vec2::new(3.0, 4.0), ratio: 0.5, start: 0.0, end: PI / 4.0 };
    let bounds = e.bounds();
    assert!((bounds.max.x - 3.0).abs() < 1e-12);
    assert!((bounds.min.x - e.at_param(e.end).x).abs() < 1e-12);
    assert!((bounds.min.y - e.at_param(e.end).y).abs() < 1e-12);
    assert!((bounds.max.y - 18.25_f64.sqrt()).abs() < 1e-12);
}

#[test]
fn polyline_area_is_exact_for_bulge_segments() {
    // One semicircular arc from left to right plus a straight closing edge.
    let mut p = Polyline::from_points(&[Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0)], true);
    p.vertices[0].bulge = 1.0;
    assert!((p.area() - PI / 2.0).abs() < 1e-12, "{}", p.area());
    p.vertices[0].bulge = -1.0;
    assert!((p.area() + PI / 2.0).abs() < 1e-12, "{}", p.area());
}

#[test]
fn polyline_area_is_stable_far_from_the_origin() {
    let o = 1.0e12;
    let p = Polyline::from_points(&[Vec2::new(o, o), Vec2::new(o + 10.0, o), Vec2::new(o + 10.0, o + 10.0), Vec2::new(o, o + 10.0)], true);
    assert!((p.area() - 100.0).abs() < 1e-12, "{}", p.area());
    // An open polyline's measured area retains the implicit straight closing edge.
    let mut open = p;
    open.closed = false;
    assert!((open.area() - 100.0).abs() < 1e-12);
}

#[test]
fn shoelace_preserves_small_polygon_area_at_large_coordinates() {
    let o = 1.0e12;
    let corners = [Vec2::new(o, o), Vec2::new(o + 10.0, o), Vec2::new(o + 10.0, o + 10.0), Vec2::new(o, o + 10.0)];
    assert_eq!(shoelace(&corners), 100.0);
    assert_eq!(shoelace(&corners.iter().rev().copied().collect::<Vec<_>>()), -100.0);
}

#[test]
fn near_tangent_long_line_does_not_invent_a_circle_hit() {
    let c = Circle::new(Vec2::ZERO, 1.0);
    let far = 1.0e8;
    let outside = Line::new(Vec2::new(-far, 1.0 + 1e-9), Vec2::new(far, 1.0 + 1e-9));
    assert!(line_circle(&outside, &c).is_empty());
    let tangent = Line::new(Vec2::new(-far, 1.0), Vec2::new(far, 1.0));
    let hits = line_circle(&tangent, &c);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].0.near(Vec2::new(0.0, 1.0), 1e-9));
}

#[test]
fn long_secant_keeps_both_intersections() {
    let c = Circle::new(Vec2::ZERO, 2.0);
    let l = Line::new(Vec2::new(-1.0e8, 0.0), Vec2::new(1.0e8, 0.0));
    let hits = line_circle(&l, &c);
    assert_eq!(hits.len(), 2);
    assert!(hits[0].0.near(Vec2::new(-2.0, 0.0), 1e-8), "{hits:?}");
    assert!(hits[1].0.near(Vec2::new(2.0, 0.0), 1e-8), "{hits:?}");
}
