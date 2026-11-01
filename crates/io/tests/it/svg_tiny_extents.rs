//! SVG export of drawings with tiny extents: the viewBox, stroke width and point radius must
//! stay non-zero, and coordinates must keep their significant digits.

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Point, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::svg;

/// The numbers of attribute `name="…"` (whitespace separated) in the first element carrying it.
fn attr(s: &str, name: &str) -> Vec<f64> {
    let key = format!("{name}=\"");
    let rest = s.split(&key).nth(1).unwrap_or_default();
    let val = rest.split('"').next().unwrap_or_default();
    val.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect()
}

fn export(kind: EntityKind) -> String {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), kind).unwrap();
    svg::export(&d, &Space::Model)
}

#[test]
fn single_point_has_a_visible_view_box_and_radius() {
    let s = export(EntityKind::Point(Point { p: Vec3::new(5.0, 5.0, 0.0), angle: 0.0 }));
    let vb = attr(&s, "viewBox");
    assert_eq!(vb.len(), 4, "{s}");
    assert!(vb[2] > 0.0 && vb[3] > 0.0, "zero-size viewBox: {s}");
    assert!(attr(&s, "r")[0] > 0.0, "zero radius: {s}");
}

#[test]
fn small_drawing_has_a_non_zero_stroke_and_keeps_precision() {
    let e = 0.000123456;
    let s = export(EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(e, e, 0.0) }));
    assert!(attr(&s, "stroke-width")[0] > 0.0, "zero stroke width: {s}");
    // The 2% margin is 2.46912e-6: at 6 decimals it would round to 0.000002.
    let pts = attr(&s, "points");
    assert_eq!(pts.len(), 4, "{s}");
    assert!((pts[0] - 0.02 * e).abs() < 1e-10, "coordinates lost precision: {s}");
    assert!((pts[1] - 1.02 * e).abs() < 1e-10, "coordinates lost precision: {s}");
}

#[test]
fn normal_drawing_keeps_six_decimals() {
    let s = export(EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 50.0, 0.0) }));
    assert!(s.contains(r#"viewBox="0 0 104.000000 54.000000""#), "{s}");
    assert!(s.contains(r#"stroke-width="0.069333""#), "{s}");
}
