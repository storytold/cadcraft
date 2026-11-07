//! The tool bar's Measure button drops down MEASUREGEOM's options, as AutoCAD's Measure
//! drop-down does: Quick, Distance, Radius, Angle, Area and Volume.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.in_window_menu = false;
    app.ui.show_toolbar = true;
    Harness::builder().with_size(vec2(1600.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    )
}

#[test]
fn the_measure_button_lists_the_measurements_and_starts_the_one_chosen() {
    for (entry, prompt) in [("Quick", "Move cursor"), ("Distance", "first point"), ("Radius", "Select arc or circle")] {
        let mut h = harness();
        h.run_steps(3);
        let button = h.get_by_label("Measure").rect();
        h.get_by_label("Measure").click();
        h.run_steps(3);
        // Other panels have buttons of these names too: the drop-down's hang below the button.
        let under = |r: egui::Rect| r.top() >= button.bottom() - 1.0 && (r.left() - button.left()).abs() < 200.0;
        for listed in ["Quick", "Distance", "Radius", "Angle", "Area", "Volume"] {
            assert!(h.get_all_by_label(listed).any(|n| under(n.rect())), "{listed} is listed under the button");
        }
        let target = h.get_all_by_label(entry).map(|n| n.rect()).filter(|r| under(*r)).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
        h.get_all_by_label(entry).find(|n| n.rect() == target).unwrap().click();
        h.run_steps(3);
        let text = h.state().session.prompt_text();
        assert!(text.contains(prompt), "{entry}: {text} {:?}", h.state().session.log);
        h.state_mut().session.cancel();
    }
}
