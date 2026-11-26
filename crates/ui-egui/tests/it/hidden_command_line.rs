//! Escape and typed command names reach the command line whether or not its overlay is shown.

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

fn call(app: &mut CadApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let (req, _reply) = ControlRequest::new(method, params);
    match handle(app, ctx, &req) {
        Outcome::Done(v) => {
            assert_eq!(v["ok"], true, "{v}");
            v["result"].clone()
        }
        _ => panic!("unexpected screenshot"),
    }
}

fn frame(app: &mut CadApp, ctx: &egui::Context) {
    let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), ..Default::default() };
    app.raw_input_hook(ctx, &mut raw);
    app.before_input(ctx);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    // No renderer here: drop the texture updates deliberately.
    out.textures_delta.clear();
    assert!(!ctx.text_edit_focused(), "a text field must not hold focus");
}

fn pump(app: &mut CadApp, ctx: &egui::Context) {
    for _ in 0..3 {
        frame(app, ctx);
    }
    assert!(app.synthetic.is_empty());
    assert!(app.canvas.rect.is_some());
}

#[test]
fn hidden_command_line_keyboard() {
    for visible in [true, false] {
        let ctx = egui::Context::default();
        cadcraft_ui_egui::framework::install_headless(&ctx);
        let mut app = CadApp::new(Session::new(), Services::default());
        pump(&mut app, &ctx);
        call(&mut app, &ctx, "ui.set", json!({"showCommandLine": visible}));

        call(&mut app, &ctx, "cmdline.input", json!({"text": "LINE"}));
        assert_eq!(call(&mut app, &ctx, "cmdline.state", json!({}))["running"], "line");
        call(&mut app, &ctx, "ui.key", json!({"key": "escape"}));
        pump(&mut app, &ctx);
        assert!(call(&mut app, &ctx, "cmdline.state", json!({}))["running"].is_null(), "escape, visible={visible}");

        call(&mut app, &ctx, "ui.text", json!({"text": "LINE"}));
        call(&mut app, &ctx, "ui.key", json!({"key": "enter"}));
        pump(&mut app, &ctx);
        assert_eq!(call(&mut app, &ctx, "cmdline.state", json!({}))["running"], "line", "typed LINE, visible={visible}");
        assert_eq!(app.cmd.history, ["LINE"], "visible={visible}");
    }
}
