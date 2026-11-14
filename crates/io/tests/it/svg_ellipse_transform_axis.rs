//! SVG import of an ellipse under a rotation, skew or non-uniform scale: the major axis of the
//! resulting `ELLIPSE` must point along the major axis of the transformed shape in the output plane.

use cadcraft_doc::EntityKind;
use cadcraft_io::read;

/// Reads `<ellipse rx=20 ry=10>` under `transform` and returns (major direction angle, ratio).
fn import(transform: &str) -> (f64, f64) {
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="{transform}"><ellipse cx="0" cy="0" rx="20" ry="10"/></g></svg>"#);
    let d = read(svg.as_bytes(), "a.svg").unwrap();
    let e = d
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Ellipse(el) => Some(el.clone()),
            _ => None,
        })
        .unwrap();
    let (dx, dy) = (e.major.x - e.center.x, e.major.y - e.center.y);
    (dy.atan2(dx), e.ratio)
}

/// Major-axis angle and ratio of the image of the ellipse under the 2x2 map `[a c; b d]`, with
/// the SVG y axis flipped (eigenvectors of `B·Bᵀ`, where B's columns are the transformed radii).
fn expected(a: f64, b: f64, c: f64, d: f64) -> (f64, f64) {
    let (bx, by) = ((20.0 * a, -20.0 * b), (10.0 * c, -10.0 * d));
    let m11 = bx.0 * bx.0 + by.0 * by.0;
    let m22 = bx.1 * bx.1 + by.1 * by.1;
    let m12 = bx.0 * bx.1 + by.0 * by.1;
    let angle = 0.5 * (2.0 * m12).atan2(m11 - m22);
    let tr = m11 + m22;
    let disc = ((m11 - m22).powi(2) + 4.0 * m12 * m12).sqrt();
    (angle, ((tr - disc) / (tr + disc)).sqrt())
}

/// Angles equal up to the sign of the axis (modulo π).
fn same_axis(got: f64, want: f64) -> bool {
    let diff = (got - want).rem_euclid(std::f64::consts::PI);
    diff < 1e-6 || std::f64::consts::PI - diff < 1e-6
}

fn check(transform: &str, m: (f64, f64, f64, f64)) {
    let (angle, ratio) = import(transform);
    let (want_angle, want_ratio) = expected(m.0, m.1, m.2, m.3);
    assert!(same_axis(angle, want_angle), "{transform}: major axis at {}°, want {}°", angle.to_degrees(), want_angle.to_degrees());
    assert!((ratio - want_ratio).abs() < 1e-9, "{transform}: ratio {ratio}, want {want_ratio}");
}

#[test]
fn rotated_ellipse_keeps_its_major_axis_direction() {
    let (s, c) = 30f64.to_radians().sin_cos();
    check("rotate(30)", (c, s, -s, c));
}

#[test]
fn skewed_ellipse_gets_the_output_space_major_axis() {
    check("skewX(30)", (1.0, 0.0, 30f64.to_radians().tan(), 1.0));
}

#[test]
fn scaled_after_rotation_ellipse_gets_the_output_space_major_axis() {
    let (s, c) = 30f64.to_radians().sin_cos();
    check("matrix(2 0 0 1 0 0) rotate(30)", (2.0 * c, s, -2.0 * s, c));
}
