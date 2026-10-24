//! #1828: a model-space plot at 1:1 in a drawing whose units are millimetres puts a 10 mm line
//! at 28.35 pt on an ISO A4 sheet, whatever MEASUREMENT says. A drawing made in millimetres by
//! changing the drawing units (UNITS sets INSUNITS, not MEASUREMENT) was plotted 25.4 times too
//! large, because model-space paper units were taken as inches.

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::plot;
use serde_json::{Value, json};

/// The page size in points from the PDF's `/MediaBox [0 0 W H]`.
fn media_box(pdf: &str) -> (f64, f64) {
    let line = pdf.lines().find(|l| l.contains("/MediaBox [")).unwrap_or_default();
    let inside = line.split("/MediaBox [").nth(1).and_then(|r| r.split(']').next()).unwrap_or_default();
    let v: Vec<f64> = inside.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    assert_eq!(v.len(), 4, "{line}");
    (v[2], v[3])
}

/// The plotted length in points of the single horizontal line.
fn line_pt(pdf: &str) -> f64 {
    let xs: Vec<f64> =
        pdf.lines().filter(|l| l.ends_with(" m") || l.ends_with(" l")).filter_map(|l| l.split_whitespace().next()?.parse().ok()).collect();
    assert_eq!(xs.len(), 2, "{pdf}");
    (xs[1] - xs[0]).abs()
}

/// A drawing with one 10-unit line in model space and the given drawing units.
fn drawing(insunits: i64, measurement: i64) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.header.set_i64("INSUNITS", insunits);
    d.header.set_i64("MEASUREMENT", measurement);
    let line = EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) });
    d.add(&Space::Model, Common::default(), line).unwrap();
    d
}

/// Plot the model on ISO A4 portrait at 1:1, as the Plot dialog does.
fn plot_a4(d: &Drawing) -> String {
    let opts: Value = json!({
        "paper": "ISO A4",
        "landscape": false,
        "fit": false,
        "scale": 1,
        "lineweights": false,
        "compress": false,
    });
    String::from_utf8_lossy(&plot(d, &Space::Model, &opts).unwrap()).into_owned()
}

const A4_PT: (f64, f64) = (595.28, 841.89);
const TEN_MM_PT: f64 = 28.3465;

#[test]
fn model_plot_in_a_metric_drawing_is_one_to_one() {
    let pdf = plot_a4(&drawing(4, 1));
    let (w, h) = media_box(&pdf);
    assert!((w - A4_PT.0).abs() < 0.01 && (h - A4_PT.1).abs() < 0.01, "A4 portrait is {w} x {h} pt");
    let pt = line_pt(&pdf);
    assert!((pt - TEN_MM_PT).abs() < 0.01, "10 mm is {pt} pt, want {TEN_MM_PT}");
}

#[test]
fn model_plot_in_millimetres_follows_the_drawing_units_when_measurement_says_inches() {
    // A drawing started imperial whose units the user changed to millimetres with UNITS:
    // MEASUREMENT stays 0, the length unit is what says the drawing is in millimetres.
    let pdf = plot_a4(&drawing(4, 0));
    let (w, h) = media_box(&pdf);
    assert!((w - A4_PT.0).abs() < 0.01 && (h - A4_PT.1).abs() < 0.01, "A4 portrait is {w} x {h} pt");
    let pt = line_pt(&pdf);
    assert!((pt - TEN_MM_PT).abs() < 0.01, "10 mm is {pt} pt, want {TEN_MM_PT}");
}

#[test]
fn model_plot_in_inches_stays_at_one_unit_per_inch() {
    // An imperial drawing (units inches) at 1:1: a 10-unit line is 10 inches wide (720 pt).
    let pdf = plot_a4(&drawing(1, 0));
    let pt = line_pt(&pdf);
    assert!((pt - 720.0).abs() < 0.01, "10 in is {pt} pt, want 720");
}

#[test]
fn layout_plot_in_millimetres_follows_the_drawing_units() {
    // The same drawing's layout, with an ISO A4 page setup but plot paper units 0 (what
    // CADCraft writes for a page setup it did not take from a file): its paper space is in
    // millimetres because the drawing is.
    let mut d = drawing(4, 0);
    let name = d.layouts[0].name.clone();
    if let Some(l) = d.layouts.iter_mut().find(|l| l.name == name) {
        l.page = cadcraft_doc::PageSetup {
            paper: "ISO A4".into(),
            width_mm: 210.0,
            height_mm: 297.0,
            margins_mm: [0.0; 4],
            landscape: false,
            ..cadcraft_doc::PageSetup::default()
        };
    }
    let line = EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) });
    d.add(&Space::Paper(name.clone()), Common::default(), line).unwrap();
    let opts = json!({"fit": false, "scale": 1, "lineweights": false, "compress": false});
    let pdf = String::from_utf8_lossy(&plot(&d, &Space::Paper(name), &opts).unwrap()).into_owned();
    let (w, h) = media_box(&pdf);
    assert!((w - A4_PT.0).abs() < 0.01 && (h - A4_PT.1).abs() < 0.01, "A4 portrait is {w} x {h} pt");
    let pt = line_pt(&pdf);
    assert!((pt - TEN_MM_PT).abs() < 0.01, "10 mm is {pt} pt, want {TEN_MM_PT}");
}
