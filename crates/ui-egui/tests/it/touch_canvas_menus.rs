//! A touch on a menu painted on the canvas (Dynamic Input options, selection cycling) is a touch
//! on that menu: the virtual mouse leaves it alone, so it reaches the row under the finger.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Pos2, Rect, TouchDeviceId, TouchId, TouchPhase, pos2};

fn touch_start(at: Pos2) -> egui::RawInput {
    egui::RawInput {
        time: Some(10.0),
        events: vec![Event::Touch { device_id: TouchDeviceId(0), id: TouchId(1), phase: TouchPhase::Start, pos: at, force: None }],
        ..Default::default()
    }
}

fn app() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.rect = Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0)));
    app.cmd.option_rows = vec![(Rect::from_min_max(pos2(420.0, 320.0), pos2(520.0, 340.0)), "Close".into())];
    app
}

#[test]
fn a_touch_on_a_dynamic_input_option_row_passes_through() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut raw = touch_start(pos2(450.0, 330.0));
    app.touch_hook(&ctx, &mut raw);
    assert!(app.touch.passing(), "the option row owns the touch");
    assert_eq!(app.touch.crosshair(), None, "no crosshair is shown for it");
}

#[test]
fn a_touch_on_the_bare_canvas_drives_the_crosshair() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut raw = touch_start(pos2(100.0, 100.0));
    app.touch_hook(&ctx, &mut raw);
    assert!(!app.touch.passing());
    assert!(app.touch.crosshair().is_some());
}
