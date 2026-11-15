//! Regression tests for page-setup paper-space units in CADCraft.
//! Supplements the earlier PageSetup.paper_mm work from #1085.
//! Based on Dievartis/cadcraft commit 0cf789be; no production changes.

use cadcraft_engine::Session;
use serde_json::json;

fn session_with_measurement(measurement: i64) -> Session {
    let mut session = Session::new();
    session.doc_mut().unwrap().header.set_i64("MEASUREMENT", measurement);
    session
}

fn new_layout(session: &mut Session, name: &str) {
    session.execute("layout.new", &json!({ "name": name, "viewport": false })).unwrap();
}

fn paper_mm(session: &Session, name: &str) -> bool {
    session.doc().unwrap().layout(name).unwrap().page.paper_mm
}

#[test]
fn new_metric_layout_uses_millimetre_paper_space() {
    let mut session = session_with_measurement(1);
    new_layout(&mut session, "Metric");
    let page = &session.doc().unwrap().layout("Metric").unwrap().page;
    assert!(page.paper_mm, "new metric layouts must use millimetre paper-space units");
}

#[test]
fn new_imperial_layout_does_not_force_millimetres() {
    let mut session = session_with_measurement(0);
    new_layout(&mut session, "Imperial");
    assert!(!paper_mm(&session, "Imperial"));
}

#[test]
fn choosing_iso_and_ansi_papers_updates_paper_space_units() {
    let mut session = session_with_measurement(0);
    new_layout(&mut session, "Sheet");

    session.execute("pagesetup", &json!({ "layout": "Sheet", "paper": "A3" })).unwrap();
    assert!(paper_mm(&session, "Sheet"), "ISO sheets must use millimetres even when MEASUREMENT=0");

    session.execute("pagesetup", &json!({ "layout": "Sheet", "paper": "Letter" })).unwrap();
    assert!(!paper_mm(&session, "Sheet"), "ANSI Letter sheets must use inches");

    session.execute("pagesetup", &json!({ "layout": "Sheet", "paper": "A4" })).unwrap();
    assert!(paper_mm(&session, "Sheet"), "reselecting an ISO sheet must restore millimetres");
}

#[test]
fn explicit_paper_mm_override_wins_over_paper_preset() {
    let mut session = session_with_measurement(1);
    new_layout(&mut session, "Mixed");

    session.execute("pagesetup", &json!({ "layout": "Mixed", "paper": "Letter", "paperMm": true })).unwrap();
    assert!(paper_mm(&session, "Mixed"), "explicit mm override must win over an imperial preset");

    session.execute("pagesetup", &json!({ "layout": "Mixed", "paper": "A4", "paperMm": false })).unwrap();
    assert!(!paper_mm(&session, "Mixed"), "explicit inch override must win over a metric preset");
}

#[test]
fn invalid_paper_mm_is_rejected_without_changing_layout() {
    let mut session = session_with_measurement(1);
    new_layout(&mut session, "Reject");
    let before = paper_mm(&session, "Reject");
    assert!(session.execute("pagesetup", &json!({ "layout": "Reject", "paperMm": "yes" })).is_err());
    assert_eq!(paper_mm(&session, "Reject"), before);
}

#[test]
fn inch_paper_in_metric_drawing_uses_25_4_mm_per_unit() {
    let mut session = session_with_measurement(1);
    new_layout(&mut session, "InchSheet");

    session.execute(
        "pagesetup",
        &json!({ "layout": "InchSheet", "paper": "Letter" }),
    ).unwrap();

    let drawing = session.doc().unwrap();
    let page = &drawing.layout("InchSheet").unwrap().page;

    assert_eq!(
        cadcraft_render::paper::page_unit_mm(drawing, page),
        25.4,
        "Letter must use inches even when MEASUREMENT=1"
    );
}
