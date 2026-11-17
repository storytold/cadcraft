//! A prompt asking for a name started away from the drawing area (a layout tab's Copy, a ribbon
//! button) shows beside the pointer where it is, with what is typed.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

/// The texts painted with the pointer at `at`.
fn tooltip_texts(app: &mut CadApp, at: egui::Pos2) -> Vec<String> {
    let ctx = egui::Context::default();
    let mut all = Vec::new();
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
        all.clear();
        out.shapes.iter().for_each(|s| texts(&s.shape, &mut all));
        out.textures_delta.clear();
    }
    all
}

#[test]
fn a_name_prompt_shows_beside_the_pointer_outside_the_drawing_area() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.cmdline("layout");
    app.cmdline("copy");
    app.cmdline("Layout1");
    let message = app.session.current_prompt().map(|p| p.message).unwrap_or_default();
    assert!(message.contains("for copy"), "{message}");
    app.cmd.buffer = "Plan B".into();
    // Over the top of the window, away from the drawing.
    let all = tooltip_texts(&mut app, egui::pos2(640.0, 6.0));
    assert!(all.iter().any(|t| t == "Enter layout name for copy: Plan B"), "{all:?}");
}
