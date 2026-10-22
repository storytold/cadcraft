//! An engine error on the control channel keeps its text in `error` and adds `data`
//! `{code, command, param?}` (`docs/control-protocol.md`), like the headless MCP backend.

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

fn call(app: &mut CadApp, method: &str, params: Value) -> Value {
    let (req, _reply) = ControlRequest::new(method, params);
    match handle(app, &egui::Context::default(), &req) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => Value::Null,
    }
}

#[test]
fn engine_errors_carry_code_command_and_param() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let v = call(&mut app, "engine.execute", json!({"command": "frobnicate", "params": {}}));
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error"], "unknown command `frobnicate`", "{v}");
    assert_eq!(v["data"], json!({"code": "unknown_command", "command": "frobnicate"}), "{v}");

    let v = call(&mut app, "command", json!({"command": "circle", "params": {"center": [0, 0]}}));
    assert_eq!(v["data"]["code"], "bad_params", "{v}");
    assert_eq!(v["data"]["command"], "circle", "{v}");
    assert_eq!(v["data"]["param"], "radius", "{v}");
    assert!(v["error"].as_str().is_some_and(|e| e.contains("radius")), "{v}");
}

#[test]
fn errors_outside_the_engine_stay_text_only() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for (method, params) in [("nope", json!({})), ("engine.execute", json!({})), ("ui.pointer", json!({}))] {
        let v = call(&mut app, method, params);
        assert_eq!(v["ok"], false, "{v}");
        assert!(v["error"].is_string(), "{v}");
        assert!(v.get("data").is_none(), "{v}");
    }
}
