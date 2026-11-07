//! A selected dimension's extension line offset (DIMEXO) and fixed extension lines (DIMFXLON,
//! DIMFXL) are edited in the Properties palette as overrides of that dimension (#2124).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services, theme};
use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::{Map, Value, json};

/// The Properties palette with one linear dimension selected.
fn palettes() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.session.script("dimlinear 0,0 10,0 5,-3\n").unwrap();
    let h = app.session.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).map(|e| e.handle.hex()).unwrap();
    app.run("select", json!({ "handles": [h] })).unwrap();
    let mut h = Harness::builder().with_size(vec2(400.0, 1600.0)).with_context_setup(|ctx| theme::install_fonts(ctx, "en")).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::right_palettes(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn overrides(app: &CadApp) -> Map<String, Value> {
    let d = app.session.doc().unwrap();
    d.model.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.overrides.clone()) } else { None }).unwrap()
}

/// The text field in the Properties row labelled `row`, and what it shows.
fn field<'h>(h: &'h Harness<'static, CadApp>, row: &str) -> (egui_kittest::Node<'h>, String) {
    let y = h.get_by_label(row).rect().center().y;
    let node = h.get_all(by().role(Role::TextInput)).find(|n| (n.rect().center().y - y).abs() < 4.0).unwrap();
    let shown = node.value().unwrap_or_default();
    (node, shown)
}

/// Replace the text of the field in the row labelled `row` with `text` and press Enter.
fn type_into(h: &mut Harness<'static, CadApp>, row: &str, text: &str) {
    field(h, row).0.focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(h, row).0.type_text(text);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn properties_set_a_dimensions_extension_line_offset_and_fixed_length() {
    let mut h = palettes();
    // The imperial Standard style: DIMEXO 0.0625, DIMFXL 1.
    assert_eq!(field(&h, "Ext line offset").1, "0.0625");
    assert_eq!(field(&h, "Ext line fixed length").1, "1.0000");
    type_into(&mut h, "Ext line offset", "0.5");
    assert_eq!(overrides(h.state()).get("extOffset"), Some(&json!(0.5)));
    h.get_by_role_and_label(Role::CheckBox, "No").click();
    h.run_steps(3);
    assert_eq!(overrides(h.state()).get("fixedExt"), Some(&json!(true)));
    type_into(&mut h, "Ext line fixed length", "0.25");
    assert_eq!(overrides(h.state()).get("fixedExtLength"), Some(&json!(0.25)));
    // The palette shows the dimension's own values now.
    assert_eq!(field(&h, "Ext line offset").1, "0.5000");
    h.get_by_role_and_label(Role::CheckBox, "Yes");
}
