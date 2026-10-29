//! The measured value of a dimension worked out from its definition points.
//!
//! DXF group 42 holds a dimension's measured value, but many writers leave it at 0 (or write a
//! stale one), so a dimension read from a file often carries no usable measurement. The
//! geometric measurement is what AutoCAD measures, and what the renderer draws.

use crate::{DimKind, Dimension};
use cadcraft_geom::Vec3;

fn dim(kind: DimKind, p13: (f64, f64), p14: (f64, f64), defpt: (f64, f64), p15: (f64, f64), p16: (f64, f64)) -> Dimension {
    Dimension {
        kind,
        defpt: Vec3::new(defpt.0, defpt.1, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::new(p13.0, p13.1, 0.0),
        p14: Vec3::new(p14.0, p14.1, 0.0),
        p15: Vec3::new(p15.0, p15.1, 0.0),
        p16: Vec3::new(p16.0, p16.1, 0.0),
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: vec![],
        marks: Default::default(),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A horizontal linear dimension measures the horizontal run of its two points.
#[test]
fn a_horizontal_linear_dimension_measures_its_run() {
    let d = dim(DimKind::Linear { rotation: 0.0 }, (1500.0, 9300.0), (9000.0, 9300.0), (1500.0, 9100.0), (0.0, 0.0), (0.0, 0.0));
    assert_eq!(d.geometric_measurement(), Some(7500.0));
}

/// A vertical linear dimension measures the vertical run.
#[test]
fn a_vertical_linear_dimension_measures_its_run() {
    let d = dim(
        DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 },
        (8000.0, 10150.0),
        (8000.0, 14150.0),
        (7700.0, 10150.0),
        (0.0, 0.0),
        (0.0, 0.0),
    );
    assert_eq!(d.geometric_measurement(), Some(4000.0));
}

/// A rotated linear dimension measures the run along its own direction, not the slope: the
/// horizontal run of a pair 10 apart and 5 up, measured at 30°, is 10·cos(30°) + 5·sin(30°),
/// the projection of the whole vector on the dimension line.
#[test]
fn a_rotated_linear_dimension_measures_along_its_own_direction() {
    let angle = 30f64.to_radians();
    let d = dim(DimKind::Linear { rotation: angle }, (0.0, 0.0), (10.0, 5.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let m = d.geometric_measurement().unwrap();
    let want = 10.0 * angle.cos() + 5.0 * angle.sin();
    assert!(near(m, want), "{m} != {want}");
}

/// An aligned dimension measures the straight distance between its two points.
#[test]
fn an_aligned_dimension_measures_the_distance() {
    let d = dim(DimKind::Aligned, (0.0, 0.0), (3.0, 4.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    assert_eq!(d.geometric_measurement(), Some(5.0));
}

/// A radius dimension measures the distance from the centre to the point on the circle.
#[test]
fn a_radius_dimension_measures_the_radius() {
    let d = dim(DimKind::Radius, (0.0, 0.0), (0.0, 0.0), (100.0, 100.0), (130.0, 100.0), (0.0, 0.0));
    assert_eq!(d.geometric_measurement(), Some(30.0));
}

/// A diameter dimension measures the whole span, defpt to p15.
#[test]
fn a_diameter_dimension_measures_the_span() {
    let d = dim(DimKind::Diameter, (0.0, 0.0), (0.0, 0.0), (100.0, 100.0), (130.0, 100.0), (0.0, 0.0));
    assert_eq!(d.geometric_measurement(), Some(30.0));
}

/// An ordinate dimension measures the distance from the origin to the feature along one axis.
#[test]
fn an_ordinate_dimension_measures_from_the_origin() {
    let x = dim(DimKind::Ordinate { x_type: true }, (40.0, 0.0), (40.0, 0.0), (10.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    assert_eq!(x.geometric_measurement(), Some(30.0));
    let y = dim(DimKind::Ordinate { x_type: false }, (0.0, 40.0), (0.0, 40.0), (0.0, 10.0), (0.0, 0.0), (0.0, 0.0));
    assert_eq!(y.geometric_measurement(), Some(30.0));
}

/// An angular dimension from three points measures the angle at p15, in radians.
#[test]
fn an_angular_dimension_measures_the_angle_in_radians() {
    let d =
        dim(DimKind::Angular3P, (10.0, 0.0), (10.0 * 60f64.to_radians().cos(), 10.0 * 60f64.to_radians().sin()), (5.0, 5.0), (0.0, 0.0), (0.0, 0.0));
    let m = d.geometric_measurement().unwrap();
    assert!(near(m, 60f64.to_radians()), "{m}");
}

/// An angular dimension from two lines measures the angle between them, in radians: the first
/// line runs p13 → p14, the second defpt → p15, and the vertex is where the two meet.
#[test]
fn an_angular_dimension_from_two_lines_measures_the_angle() {
    // The first line is the X axis; the second runs from the origin at 120°. The arc point
    // p16 sits between them, so the measured angle is the 120° one.
    let a = 120f64.to_radians();
    let half = 60f64.to_radians();
    let d = dim(DimKind::Angular, (-10.0, 0.0), (10.0, 0.0), (0.0, 0.0), (10.0 * a.cos(), 10.0 * a.sin()), (10.0 * half.cos(), 10.0 * half.sin()));
    let m = d.geometric_measurement().unwrap();
    assert!(near(m, a), "{m}");
}

/// An arc length dimension measures the length of the arc, not the chord.
#[test]
fn an_arc_length_dimension_measures_the_arc() {
    // A quarter circle of radius 10 about the origin: from (10, 0) to (0, 10).
    let d = dim(DimKind::ArcLength, (10.0, 0.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let m = d.geometric_measurement().unwrap();
    assert!(near(m, 10.0 * std::f64::consts::FRAC_PI_2), "{m}");
}

/// A dimension whose definition points do not describe a measurement has none.
#[test]
fn a_dimension_without_usable_points_has_no_measurement() {
    let d = dim(DimKind::Angular3P, (0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    assert_eq!(d.geometric_measurement(), None);
}

/// The stored measurement wins when the file holds one; the geometric one fills the gap.
#[test]
fn the_stored_measurement_is_kept_and_the_geometric_one_fills_the_gap() {
    let mut d = dim(DimKind::Linear { rotation: 0.0 }, (0.0, 0.0), (10.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    assert_eq!(d.effective_measurement(), Some(10.0));
    d.measurement = 10.0;
    assert_eq!(d.effective_measurement(), Some(10.0));
    // A stored value that disagrees with the geometry is kept: the file said so.
    d.measurement = 999.0;
    assert_eq!(d.effective_measurement(), Some(999.0));
    // A stored 0 is no measurement at all: the geometric one is used.
    d.measurement = 0.0;
    assert_eq!(d.effective_measurement(), Some(10.0));
}
