//! Drafting Settings reads drawing spacing and commits edits through SETVAR (#607).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, HVal};
use cadcraft_engine::geom::Vec3;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui::accesskit::{Action, ActionData, ActionRequest, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

fn harness(d: Drawing, render: bool) -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(d, "Drawing", None);
    let mut app = CadApp::new(session, Services::default());
    app.ui.dialog = Some("dsettings".into());
    let builder = Harness::builder().with_size(egui::vec2(800.0, 600.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    h
}

fn set_spacing(h: &mut Harness<'_, CadApp>, index: usize, value: f64) {
    let (target_node, target_tree) = h.get_all_by_role(Role::SpinButton).nth(index).expect("spacing input").accesskit_node().locate();
    h.event(egui::Event::AccessKitActionRequest(ActionRequest {
        action: Action::SetValue,
        target_node,
        target_tree,
        data: Some(ActionData::NumericValue(value)),
    }));
    h.run_steps(3);
}

#[test]
fn spacing_controls_update_the_drawing_and_mark_it_dirty() {
    let directory = std::env::var_os("CADCRAFT_DRAFTING_SHOTS").map(std::path::PathBuf::from);
    let mut h = harness(Drawing::new_imperial(), directory.is_some());
    assert!(!h.state().session.state().unwrap().is_dirty());
    set_spacing(&mut h, 0, 0.25);
    set_spacing(&mut h, 1, 0.125);
    // The fourth spin box: Polar distance (#450) sits between snap and grid spacing.
    set_spacing(&mut h, 3, 0.75);
    let app = h.state_mut();
    assert_eq!(app.run("getvar", json!({"name": "SNAPUNIT"})).unwrap()["value"], json!([0.25, 0.125]));
    assert_eq!(app.run("getvar", json!({"name": "GRIDUNIT"})).unwrap()["value"], json!([0.75, 0.5]));
    assert!(app.session.state().unwrap().is_dirty());
    if let Some(directory) = directory {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(directory.join("drafting-settings-spacing.png")).unwrap();
    }
}

#[test]
fn opening_the_dialog_preserves_loaded_spacing_outside_drag_ranges() {
    let mut d = Drawing::new_imperial();
    d.header.set("SNAPUNIT", HVal::Point(Vec3::new(1e-6, 2e-6, 0.0)));
    d.header.set("GRIDUNIT", HVal::Point(Vec3::new(1e8, 1e8, 0.0)));
    let h = harness(d, false);
    let header = &h.state().session.doc().unwrap().header;
    assert_eq!(header.point("SNAPUNIT"), Some(Vec3::new(1e-6, 2e-6, 0.0)));
    assert_eq!(header.point("GRIDUNIT"), Some(Vec3::new(1e8, 1e8, 0.0)));
    assert!(!h.state().session.state().unwrap().is_dirty(), "opening a dialog must not change the drawing");
}
