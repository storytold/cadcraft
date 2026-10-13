//! The OS cursor over the drawing area (#39): hidden under the drawn crosshair by default, kept
//! visible with `systemCursor` so screen magnifiers and other assistive tech can follow it.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

/// The cursor icon the app asks for with the pointer at `at`.
fn cursor_at(app: &mut CadApp, at: egui::Pos2) -> egui::CursorIcon {
    let ctx = egui::Context::default();
    let mut icon = egui::CursorIcon::Default;
    // A few frames: layout settles and the pointer hover is known from the second frame on.
    for _ in 0..3 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            events: vec![egui::Event::PointerMoved(at)],
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

/// The cursor icon the app asks for with the pointer in the middle of the drawing area.
fn canvas_cursor(app: &mut CadApp) -> egui::CursorIcon {
    cursor_at(app, egui::pos2(640.0, 400.0))
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

/// The host's default (the desktop app's `CADCRAFT_SYSTEM_CURSOR`) turns it on; a saved `false`
/// and the resets keep it.
#[test]
fn the_host_default_survives_saved_profiles_and_resets() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("options", serde_json::json!({ "reset": true })).unwrap();
    assert!(!app.ui.system_cursor, "off without a host default");

    app.set_system_cursor_default(true);
    assert!(app.ui.system_cursor);
    app.load_profile(r#"{"systemCursor": false}"#);
    assert!(app.ui.system_cursor);
    app.ui.system_cursor = false;
    app.run("options", serde_json::json!({ "reset": true })).unwrap();
    assert!(app.ui.system_cursor);
    app.ui.system_cursor = false;
    app.run("ui.resetpalettes", serde_json::json!({})).unwrap();
    assert!(app.ui.system_cursor);
}

#[test]
fn drawing_area_hides_the_system_cursor_over_its_overlays() {
    // The software draws the pointer across the drawing area, including the ViewCube (its corner and a part) and
    // the command line, so the system cursor stays hidden on all of them.
    let mut app = CadApp::new(Session::new(), Services::default());
    canvas_cursor(&mut app);
    let canvas = app.canvas.rect.expect("the drawing area is laid out");
    let spots = [
        ("centre", canvas.center()),
        ("ViewCube corner", egui::pos2(canvas.right() - 30.0, canvas.top() + 14.0)),
        ("ViewCube part", egui::pos2(canvas.right() - 60.0, canvas.top() + 30.0)),
        ("command line", egui::pos2(canvas.center().x, canvas.bottom() - 22.0)),
    ];
    for (name, at) in spots {
        assert_eq!(cursor_at(&mut app, at), egui::CursorIcon::None, "{name}: the system cursor stays hidden");
    }
}

#[test]
fn above_the_drawing_area_the_system_cursor_stays_visible() {
    let mut app = CadApp::new(Session::new(), Services::default());
    canvas_cursor(&mut app);
    let canvas = app.canvas.rect.expect("the drawing area is laid out");
    let above = egui::pos2(canvas.center().x, canvas.top() - 8.0);
    assert_ne!(cursor_at(&mut app, above), egui::CursorIcon::None, "outside the drawing area the system cursor is shown");
}

#[test]
fn the_app_hides_the_system_cursor_only_over_the_drawing_area() {
    let mut app = CadApp::new(Session::new(), Services::default());
    canvas_cursor(&mut app);
    assert!(app.canvas.hide_system_cursor, "over the drawing area the app hides the system cursor");
    let canvas = app.canvas.rect.expect("the drawing area is laid out");
    cursor_at(&mut app, egui::pos2(canvas.center().x, canvas.top() - 8.0));
    assert!(!app.canvas.hide_system_cursor, "above the drawing area the system cursor is shown");

    app.run("ui.toggle.systemcursor", serde_json::json!({ "on": true })).ok();
    canvas_cursor(&mut app);
    assert!(!app.canvas.hide_system_cursor, "with systemCursor the OS crosshair stays visible for magnifiers");
}
