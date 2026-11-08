//! User shortcuts are checked however they arrive: `ui.set` drops what the preferences would,
//! and a malformed preference value leaves the shortcuts as they are.

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

fn call(app: &mut CadApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let (req, _reply) = ControlRequest::new(method, params);
    match handle(app, ctx, &req) {
        Outcome::Done(v) => v,
        _ => panic!("unexpected screenshot"),
    }
}

#[test]
fn ui_set_keeps_only_shortcuts_the_preferences_would() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    let v = call(
        &mut app,
        &ctx,
        "ui.set",
        json!({"shortcuts": {"line": "Ctrl+Shift+L", "circle": "not a key", "nosuchcommand": "Ctrl+Shift+K", "move": "Ctrl+Shift+L"}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    let ids: Vec<&String> = app.ui.shortcuts.keys().collect();
    assert_eq!(ids, vec!["line"], "a bad key, an unknown command and a key taken twice are dropped: {:?}", app.ui.shortcuts);
}

#[test]
fn a_malformed_preference_keeps_the_shortcuts() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.load_prefs(r#"{"shortcuts": {"line": "Ctrl+Shift+L"}}"#);
    assert_eq!(app.ui.shortcuts.len(), 1);
    app.load_prefs(r#"{"shortcuts": "Ctrl+Shift+L"}"#);
    assert_eq!(app.ui.shortcuts.get("line").map(String::as_str), Some("Ctrl+Shift+L"), "not cleared");
}
