//! The OS cursor over the drawing area (#39): hidden under the drawn crosshair by default, kept
//! visible with `systemCursor` so screen magnifiers and other assistive tech can follow it.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

/// The cursor icon the app asks for with the pointer in the middle of the drawing area.
fn canvas_cursor(app: &mut CadApp) -> egui::CursorIcon {
    let ctx = egui::Context::default();
    let mut icon = egui::CursorIcon::Default;
    // A few frames: layout settles and the pointer hover is known from the second frame on.
    for _ in 0..3 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            events: vec![egui::Event::PointerMoved(egui::pos2(640.0, 400.0))],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        icon = out.platform_output.cursor_icon;
        out.textures_delta.clear(); // no renderer here
    }
    icon
}

#[test]
fn system_cursor_stays_visible_over_the_canvas_when_requested() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.system_cursor = false;
    assert_eq!(canvas_cursor(&mut app), egui::CursorIcon::None, "default: the drawn crosshair replaces the OS cursor");

    app.run("ui.toggle.systemcursor", serde_json::json!({ "on": true })).ok();
    assert!(app.ui.system_cursor);
    assert_eq!(canvas_cursor(&mut app), egui::CursorIcon::Crosshair, "systemCursor keeps an OS cursor for magnifiers");
}
