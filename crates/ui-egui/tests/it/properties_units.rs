//! Properties palette numeric fields (#849): typed values are read as the command line reads
//! them (feet and inches included), distances are shown in the drawing's units, and text that
//! isn't a number is reported instead of being dropped.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services, theme};
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::json;

/// A selected TEXT 2.5 high, with lengths in `lunits`.
fn palettes(lunits: i64) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("setvar", json!({ "name": "LUNITS", "value": lunits })).unwrap();
    let h = app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 2.5 })).unwrap();
    app.run("select", json!({ "handles": [h["handle"]] })).unwrap();
    // Install the production font chain before the first pass; the fixture draws palettes
    // directly and would otherwise fall back to egui's fonts instead of the app's chain.
    let mut h = Harness::builder().with_size(vec2(400.0, 1400.0)).with_context_setup(|ctx| theme::install_fonts(ctx, "en")).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::right_palettes(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn height(app: &CadApp) -> f64 {
    let d = app.session.doc().unwrap();
    d.model.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.height) } else { None }).unwrap()
}

/// The text box showing `shown` (its text run child carries the same value).
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
fn text_height_takes_feet_and_inches() {
    let mut h = palettes(2);
    type_into(&mut h, "2.5000", "3'");
    assert_eq!(height(h.state()), 36.0);
    type_into(&mut h, "36.0000", "1'6 1/2\"");
    assert_eq!(height(h.state()), 18.5);
}

#[test]
fn text_height_is_shown_in_the_drawing_units() {
    let mut h = palettes(4);
    type_into(&mut h, "0'-2 1/2\"", "3'6\"");
    assert_eq!(height(h.state()), 42.0);
    field(&h, "3'-6\"");
}

#[test]
fn unreadable_text_is_reported_not_dropped() {
    let mut h = palettes(2);
    let before = h.state().session.log.len();
    type_into(&mut h, "2.5000", "abc");
    assert_eq!(height(h.state()), 2.5);
    let log = &h.state().session.log[before..];
    assert!(log.iter().any(|l| l.contains("abc")), "{log:?}");
    field(&h, "2.5000");
}
