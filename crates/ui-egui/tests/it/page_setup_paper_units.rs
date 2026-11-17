//! File ▸ Page Setup Manager: a layout's paper units can be millimetres on an inch-family sheet
//! (an ARCH D sheet in a metric drawing, #3249), so the sheet measures 914.4 x 609.6 units.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

#[test]
fn page_setup_switches_an_arch_d_sheet_to_millimetres() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("units", json!({ "insunits": 4 })).unwrap();
    app.run("pagesetup", json!({ "layout": "Layout1", "paper": "ARCH D", "landscape": true })).unwrap();
    let inch = cadcraft_render::sheet(app.session.doc().unwrap(), "Layout1").unwrap();
    assert!((inch.size.x - 36.0).abs() < 1e-9, "{:?}", inch.size);
    app.start("pagesetup");
    // The dialog's existing combo boxes (device, paper, plot area, plot style) have no
    // accessible name yet; that check is not what this test is about.
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_accessibility_check(false).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h.get_by_label("Millimeters").click();
    h.run_steps(5);
    let d = h.state().session.doc().unwrap();
    assert!(d.layout("Layout1").unwrap().page.paper_mm);
    let mm = cadcraft_render::sheet(d, "Layout1").unwrap();
    assert!((mm.size.x - 914.4).abs() < 1e-9 && (mm.size.y - 609.6).abs() < 1e-9, "{:?}", mm.size);
}
