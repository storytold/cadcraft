//! The measurement a dimension reports and the measurement it is drawn with are the same number.
//!
//! The renderer measures a dimension to draw its text, and the reader measures it to report the
//! value. They are two implementations of the same rule, so a change to one that does not reach
//! the other would show a number that differs from the drawing. This pins them together.

use cadcraft_doc::{DimKind, Dimension, Drawing};
use cadcraft_geom::Vec3;
use cadcraft_render::dimension_in;

fn dim(kind: DimKind, defpt: (f64, f64), p13: (f64, f64), p14: (f64, f64), p15: (f64, f64), p16: (f64, f64)) -> Dimension {
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
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

/// The drawn measurement and the reported one, for a dimension.
fn pair(dm: &Dimension) -> (f64, Option<f64>) {
    let d = Drawing::new_imperial();
    let drawn = dimension_in(&d, dm).measurement;
    (drawn, dm.geometric_measurement())
}

/// A linear dimension measures the same run as it is drawn with.
#[test]
fn a_linear_dimension_is_drawn_with_the_measurement_it_reports() {
    let dm = dim(DimKind::Linear { rotation: 0.0 }, (0.0, 5.0), (0.0, 0.0), (7500.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// A vertical linear dimension measures the same run as it is drawn with.
#[test]
fn a_vertical_linear_dimension_is_drawn_with_the_measurement_it_reports() {
    let dm = dim(
        DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 },
        (8000.0, 10150.0),
        (8000.0, 14150.0),
        (7700.0, 10150.0),
        (0.0, 0.0),
        (0.0, 0.0),
    );
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// An aligned dimension measures the same distance as it is drawn with.
#[test]
fn an_aligned_dimension_is_drawn_with_the_measurement_it_reports() {
    // The two points are 1200 apart at 5.7°: the dimension follows the line between them.
    let a = 5.7106f64.to_radians();
    let dm = dim(DimKind::Aligned, (-14000.0, 24000.0), (0.0, 0.0), (1200.0 * a.cos(), 1200.0 * a.sin()), (0.0, 0.0), (0.0, 0.0));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// A radius dimension measures the same radius as it is drawn with.
#[test]
fn a_radius_dimension_is_drawn_with_the_measurement_it_reports() {
    let dm = dim(DimKind::Radius, (100.0, 100.0), (0.0, 0.0), (0.0, 0.0), (130.0, 100.0), (0.0, 0.0));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// A diameter dimension measures the same span as it is drawn with.
#[test]
fn a_diameter_dimension_is_drawn_with_the_measurement_it_reports() {
    let dm = dim(DimKind::Diameter, (100.0, 100.0), (0.0, 0.0), (0.0, 0.0), (130.0, 100.0), (0.0, 0.0));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// An ordinate dimension measures the same distance as it is drawn with.
#[test]
fn an_ordinate_dimension_is_drawn_with_the_measurement_it_reports() {
    let x = dim(DimKind::Ordinate { x_type: true }, (10.0, 0.0), (40.0, 0.0), (40.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let (drawn, reported) = pair(&x);
    assert_eq!(reported, Some(drawn));
    let y = dim(DimKind::Ordinate { x_type: false }, (0.0, 10.0), (0.0, 40.0), (0.0, 40.0), (0.0, 0.0), (0.0, 0.0));
    let (drawn, reported) = pair(&y);
    assert_eq!(reported, Some(drawn));
}

/// An angular dimension measures the same angle as it is drawn with.
#[test]
fn an_angular_dimension_is_drawn_with_the_measurement_it_reports() {
    let a = 120f64.to_radians();
    let half = 60f64.to_radians();
    let dm = dim(DimKind::Angular, (-10.0, 0.0), (10.0, 0.0), (0.0, 0.0), (10.0 * a.cos(), 10.0 * a.sin()), (10.0 * half.cos(), 10.0 * half.sin()));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// An angular dimension from three points measures the same angle as it is drawn with.
#[test]
fn an_angular_dimension_from_three_points_is_drawn_with_the_measurement_it_reports() {
    let dm =
        dim(DimKind::Angular3P, (10.0, 0.0), (10.0 * 60f64.to_radians().cos(), 10.0 * 60f64.to_radians().sin()), (5.0, 5.0), (0.0, 0.0), (0.0, 0.0));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}

/// An arc length dimension measures the same arc as it is drawn with.
#[test]
fn an_arc_length_dimension_is_drawn_with_the_measurement_it_reports() {
    let dm = dim(DimKind::ArcLength, (10.0, 0.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let (drawn, reported) = pair(&dm);
    assert_eq!(reported, Some(drawn));
}
