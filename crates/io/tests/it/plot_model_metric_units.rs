//! #1828: a model-space plot at 1:1 in a drawing whose units are millimetres puts a 10 mm line
//! at 28.35 pt on an ISO A4 sheet, whatever MEASUREMENT says. A drawing made in millimetres by
//! changing the drawing units (UNITS sets INSUNITS, not MEASUREMENT) was plotted 25.4 times too
//! large, because model-space paper units were taken as inches.
//!
//! The model plot's units are the paper's, never the layouts' page setups': a layout whose page
//! setup is in inches still plots its paper space in inches (#1866).

use cadcraft_doc::{Common, Drawing, EntityKind, Layout, Line, PageSetup, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::plot;
use serde_json::{Value, json};

use crate::pdf_read;

/// The page size in points.
fn media_box(pdf: &[u8]) -> (f64, f64) {
    pdf_read::read(pdf).media()
}

/// The plotted length in points of the single horizontal line.
fn line_pt(pdf: &[u8]) -> f64 {
    let doc = pdf_read::read(pdf);
    let xs: Vec<f64> = pdf_read::drawn(doc.content()).iter().filter(|o| o.op == "m" || o.op == "l").filter_map(|o| o.nums.first().copied()).collect();
    assert_eq!(xs.len(), 2, "one line, two ends: {xs:?}");
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

/// Plot the model at 1:1 on the named paper, as the Plot dialog does.
fn plot_on(d: &Drawing, paper: &str) -> Vec<u8> {
    let opts: Value = json!({
        "paper": paper,
        "landscape": false,
        "fit": false,
        "scale": 1,
        "lineweights": false,
        "compress": false,
    });
    plot(d, &Space::Model, &opts).unwrap()
}

const A4_PT: (f64, f64) = (595.28, 841.89);
const ANSI_A_PT: (f64, f64) = (612.0, 792.0);
const TEN_MM_PT: f64 = 28.3465;
const TEN_IN_PT: f64 = 720.0;

fn assert_pt(actual: f64, want: f64) {
    assert!((actual - want).abs() < 0.01, "{actual} pt, want {want}");
}

/// The MediaBox is the sheet in points (A4 or ANSI A, portrait).
fn assert_media(pdf: &[u8], size: (f64, f64)) {
    let (w, h) = media_box(pdf);
    assert!((w - size.0).abs() < 0.01 && (h - size.1).abs() < 0.01, "{w} x {h} pt, want {size:?}");
}

#[test]
fn model_plot_in_a_metric_drawing_is_one_to_one() {
    let pdf = plot_on(&drawing(4, 1), "ISO A4");
    assert_media(&pdf, A4_PT);
    assert_pt(line_pt(&pdf), TEN_MM_PT);
}

#[test]
fn model_plot_follows_the_length_unit_when_measurement_says_inches() {
    // A drawing started imperial whose units the user changed to millimetres with UNITS:
    // MEASUREMENT stays 0, the length unit is what says the drawing is in millimetres.
    let pdf = plot_on(&drawing(4, 0), "ISO A4");
    assert_media(&pdf, A4_PT);
    assert_pt(line_pt(&pdf), TEN_MM_PT);
}

#[test]
fn model_plot_takes_units_from_the_paper() {
    // A millimetre sheet at 1:1 prints one drawing unit per millimetre, whatever the drawing's
    // length unit is.
    let pdf = plot_on(&drawing(1, 0), "ISO A4");
    assert_media(&pdf, A4_PT);
    assert_pt(line_pt(&pdf), TEN_MM_PT);
    // An inch sheet prints one drawing unit per inch, whatever the drawing's length unit is.
    let pdf = plot_on(&drawing(4, 0), "ANSI A");
    assert_media(&pdf, ANSI_A_PT);
    assert_pt(line_pt(&pdf), TEN_IN_PT);
}

#[test]
fn layout_plot_keeps_its_page_setup_units() {
    // The @echelon regression: a metric drawing whose layout has a page setup in inches. The
    // layout's paper space is in inches (25.4 mm per unit), not in the drawing's millimetres,
    // and its A4 sheet is still A4.
    let mut d = drawing(4, 0);
    let mut l = Layout::new("SHEET", 3);
    l.page = PageSetup {
        paper: "ISO A4".into(),
        width_mm: 210.0,
        height_mm: 297.0,
        margins_mm: [0.0; 4],
        landscape: false,
        paper_mm: false,
        ..PageSetup::default()
    };
    d.layouts.push(l);
    let line = EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) });
    d.add(&Space::Paper("SHEET".into()), Common::default(), line).unwrap();
    let opts = json!({"fit": false, "scale": 1, "lineweights": false, "compress": false});
    let pdf = plot(&d, &Space::Paper("SHEET".into()), &opts).unwrap();
    assert_media(&pdf, A4_PT);
    assert_pt(line_pt(&pdf), TEN_IN_PT);
}
