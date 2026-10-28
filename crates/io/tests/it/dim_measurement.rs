//! A dimension's measured value is worked out from its definition points when the file does not
//! carry one. DXF group code 42 (the actual measurement) is optional: many writers leave it out
//! (or write 0), and a dimension read from such a file then reports no measurement at all,
//! although its definition points describe one perfectly well.
//!
//! AutoCAD measures a dimension from the same points, and the value it wrote into the
//! dimension's anonymous block is the one a reader sees. A linear dimension measures the run
//! along its own direction, an aligned one the distance between its two points.

use cadcraft_doc::{Common, DimKind, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read, write_dxf};

/// A dimension with the given points and no measurement of its own.
fn dim(kind: DimKind, defpt: (f64, f64), p13: (f64, f64), p14: (f64, f64), p15: (f64, f64)) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind,
        defpt: Vec3::new(defpt.0, defpt.1, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::new(p13.0, p13.1, 0.0),
        p14: Vec3::new(p14.0, p14.1, 0.0),
        p15: Vec3::new(p15.0, p15.1, 0.0),
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    })
}

/// The DXF of a drawing with one dimension, as a writer that leaves out group 42 writes it: the
/// group 42 line is taken out of the written file.
fn dxf_without_measurement(kind: DimKind, defpt: (f64, f64), p13: (f64, f64), p14: (f64, f64), p15: (f64, f64)) -> String {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), dim(kind, defpt, p13, p14, p15)).unwrap();
    let dxf = write_dxf(&d);
    let lines: Vec<&str> = dxf.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut skip = false;
    for line in lines {
        if skip {
            skip = false;
            continue;
        }
        // The measurement is written as group 42 with its value on the next line.
        if line.trim() == "42" {
            skip = true;
            continue;
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

/// The measurements of the drawing's dimensions, in order.
fn measurements(d: &Drawing) -> Vec<f64> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.measurement),
            _ => None,
        })
        .collect()
}

/// A linear dimension whose file carries no group 42 reports the run along its direction.
#[test]
fn a_linear_dimension_without_a_stored_measurement_is_measured_from_its_points() {
    let dxf = dxf_without_measurement(DimKind::Linear { rotation: 0.0 }, (0.0, 5.0), (0.0, 0.0), (7500.0, 0.0), (0.0, 0.0));
    let d = read(dxf.as_bytes(), "d.dxf").expect("read");
    assert_eq!(measurements(&d), vec![7500.0]);
}

/// An aligned dimension measures the distance between its two points, whatever angle they are at.
#[test]
fn an_aligned_dimension_without_a_stored_measurement_measures_the_distance() {
    // The two points are 1200 apart at 5.7°: a dimension measures 1200, not its horizontal run.
    let a = 5.7106f64.to_radians();
    let dxf = dxf_without_measurement(DimKind::Aligned, (-14000.0, 24000.0), (0.0, 0.0), (1200.0 * a.cos(), 1200.0 * a.sin()), (0.0, 0.0));
    let d = read(dxf.as_bytes(), "d.dxf").expect("read");
    let m = measurements(&d);
    assert_eq!(m.len(), 1);
    assert!((m[0] - 1200.0).abs() < 1e-6, "{}", m[0]);
}

/// A radius dimension measures the distance from the centre to the point on the circle.
#[test]
fn a_radius_dimension_without_a_stored_measurement_measures_the_radius() {
    let dxf = dxf_without_measurement(DimKind::Radius, (100.0, 100.0), (0.0, 0.0), (0.0, 0.0), (130.0, 100.0));
    let d = read(dxf.as_bytes(), "d.dxf").expect("read");
    assert_eq!(measurements(&d), vec![30.0]);
}

/// A file that carries group 42 keeps the value the file holds.
#[test]
fn a_stored_measurement_is_kept() {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), dim(DimKind::Linear { rotation: 0.0 }, (0.0, 5.0), (0.0, 0.0), (7500.0, 0.0), (0.0, 0.0))).unwrap();
    let dxf = write_dxf(&d);
    let back = read(dxf.as_bytes(), "d.dxf").expect("read");
    // The written file holds the measurement the renderer computed, and it is kept.
    let m = measurements(&back);
    assert_eq!(m.len(), 1);
    assert!((m[0] - 7500.0).abs() < 1e-6, "{}", m[0]);
}

/// A dimension whose points do not describe a measurement reports none, not a wrong one.
#[test]
fn a_dimension_without_usable_points_reports_no_measurement() {
    // An angular dimension from three points with no arc point: there is no angle to measure.
    let dxf = dxf_without_measurement(DimKind::Angular3P, (0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (0.0, 0.0));
    let d = read(dxf.as_bytes(), "d.dxf").expect("read");
    assert_eq!(measurements(&d), vec![0.0]);
}
