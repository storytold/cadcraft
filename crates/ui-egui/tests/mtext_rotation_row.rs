//! The Properties palette and Quick Properties show and set an MTEXT's Rotation (#900), as they
//! already do for TEXT. The engine side (`properties.set {rotation}`, ROTATE, DXF/DWG) was there.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::json;

/// A selected MTEXT turned 30 degrees.
fn app_with_mtext() -> CadApp {
    let mut app = CadApp::new(Session::default(), Services::default());
    let h = app.run("mtext", json!({ "at": [0, 0], "text": "Note", "height": 2.5, "rotation": 30 })).unwrap();
    app.run("select", json!({ "handles": [h["handle"]] })).unwrap();
    app
}

fn rotation(app: &CadApp) -> f64 {
    let d = app.session.doc().unwrap();
    d.model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.rotation.to_degrees()) } else { None }).unwrap()
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(egui::accesskit::Role::TextInput).value(shown))
}

fn type_into(h: &mut Harness<'static, CadApp>, shown: &str, text: &str) {
    field(h, shown).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(h, shown).type_text(text);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn the_properties_palette_has_a_rotation_row_for_mtext() {
    let mut h = Harness::builder().with_size(vec2(400.0, 1400.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::right_palettes(app, ui);
        },
        app_with_mtext(),
    );
    h.run_steps(4);
    type_into(&mut h, "30", "90");
    assert!((rotation(h.state()) - 90.0).abs() < 1e-9, "{}", rotation(h.state()));
    // The shown value follows, and typing 0 turns it back.
    type_into(&mut h, "90", "0");
    assert!(rotation(h.state()).abs() < 1e-9);
}

#[test]
fn quick_properties_has_it_too() {
    let mut app = app_with_mtext();
    app.session.settings.qpmode = true;
    let mut h = Harness::builder().with_size(vec2(800.0, 600.0)).with_accessibility_check(false).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::quick::quick_properties(app, &ui.ctx().clone());
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Rotation");
    type_into(&mut h, "30.0000", "45");
    assert!((rotation(h.state()) - 45.0).abs() < 1e-9, "{}", rotation(h.state()));
}
