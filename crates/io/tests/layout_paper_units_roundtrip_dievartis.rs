//! Regression tests for explicit and inherited paper-space units across DXF saves.
//! The drawing's MEASUREMENT controls only the historical, inherited case.

use cadcraft_doc::{Drawing, Layout};
use cadcraft_io::{read, write_dxf};

fn round_trip(measurement: i64, paper_mm: bool, override_mm: Option<bool>) {
    let mut drawing = if measurement == 1 { Drawing::new_metric() } else { Drawing::new_imperial() };
    drawing.header.set_i64("MEASUREMENT", measurement);
    let mut layout = Layout::new("UNITS_TEST", 9);
    layout.page.paper_mm = paper_mm;
    layout.page.paper_unit_mm_override = override_mm;
    drawing.layouts.push(layout);

    let before = &drawing.layout("UNITS_TEST").unwrap().page;
    let expected_mm = cadcraft_render::paper::page_unit_mm(&drawing, before);

    let dxf = write_dxf(&drawing);
    let reopened = read(dxf.as_bytes(), "paper_units_test.dxf").unwrap();
    let after = &reopened.layout("UNITS_TEST").unwrap().page;

    assert_eq!(after.paper_mm, paper_mm);
    assert_eq!(after.paper_unit_mm_override, override_mm);
    assert_eq!(cadcraft_render::paper::page_unit_mm(&reopened, after), expected_mm);
}

#[test]
fn explicit_inches_survive_metric_dxf_round_trip() {
    // The bug: false used to inherit MEASUREMENT=1 and become millimetres.
    round_trip(1, false, Some(false));
}

#[test]
fn explicit_millimetres_survive_imperial_dxf_round_trip() {
    round_trip(0, true, Some(true));
}

#[test]
fn inherited_units_survive_metric_dxf_round_trip() {
    round_trip(1, false, None);
}

#[test]
fn inherited_units_survive_imperial_dxf_round_trip() {
    round_trip(0, false, None);
}

#[test]
fn explicit_inches_survive_imperial_dxf_round_trip() {
    round_trip(0, false, Some(false));
}

#[test]
fn explicit_millimetres_survive_metric_dxf_round_trip() {
    round_trip(1, true, Some(true));
}

#[test]
fn inherited_millimetres_flag_survives_dxf_round_trip() {
    // Older PageSetup data can set paper_mm=true without an override.
    round_trip(0, true, None);
}
