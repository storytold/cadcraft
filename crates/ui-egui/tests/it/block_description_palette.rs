//! Properties in the Block Editor: with nothing selected it shows the edited block's name and
//! description, and a description typed there is saved into the block by BSAVE.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::json;

/// Block "Door", described "Hinged door", open in the Block Editor.
fn palettes() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    let l = app.run("line", json!({ "points": [[0, 0], [2, 0]] })).unwrap();
    app.run("block", json!({ "name": "Door", "base": [0, 0], "handles": l["handles"], "description": "Hinged door" })).unwrap();
    app.run("bedit", json!({ "name": "Door" })).unwrap();
    let mut h = Harness::builder().with_size(vec2(400.0, 1400.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::right_palettes(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(egui::accesskit::Role::TextInput).value(shown))
}

#[test]
fn the_block_editor_shows_and_changes_the_block_description() {
    let mut h = palettes();
    field(&h, "Hinged door").focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(&h, "Hinged door").type_text("Sliding door");
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    field(&h, "Sliding door");

    let app = h.state_mut();
    assert!(app.session.state().unwrap().block_unsaved());
    app.run("bclose", json!({ "save": true })).unwrap();
    assert_eq!(app.session.doc().unwrap().block("Door").unwrap().description, "Sliding door");
}
