//! The close button of an inactive file tab stays while the pointer is on it, and a click on it
//! closes that drawing. It used to take the tab's hover, so the next frame dropped it and the one
//! after drew it again: the tab and its X flickered.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[test]
fn an_inactive_tabs_close_button_stays_under_the_pointer_and_closes_it() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).unwrap();
    app.run("new", serde_json::json!({})).unwrap();
    app.ui.start_tab = false;
    assert_ne!(app.session.active, 0, "the first drawing's tab is inactive");
    let first = app.session.docs[0].title.clone();
    let open = app.session.docs.len();
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    // Pointing at the tab shows its close button.
    h.get_by_role_and_label(Role::Tab, &first).hover();
    h.run_steps(3);
    let close = format!("Close {first}");
    h.get_by_label(&close).hover();
    for frame in 0..6 {
        h.step();
        assert!(h.query_by_label(&close).is_some(), "the close button is gone on frame {frame} under the pointer");
    }
    h.get_by_label(&close).click();
    h.run_steps(3);
    let titles: Vec<&str> = h.state().session.docs.iter().map(|d| d.title.as_str()).collect();
    assert_eq!(titles.len(), open - 1, "one drawing closed: {titles:?}");
    assert!(!titles.contains(&first.as_str()), "the clicked tab's drawing is the one closed: {titles:?}");
}
