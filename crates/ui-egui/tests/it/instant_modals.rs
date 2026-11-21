//! "Save changes?" is fully drawn, dim included, in the frame of the Close that asks it.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::epaint::Shape;
use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

/// The opacity of each painted text containing `needle`.
fn text_opacity(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Vec<f32> {
    fn walk(shape: &Shape, needle: &str, out: &mut Vec<f32>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, needle, out)),
            Shape::Text(text) if text.galley.text().contains(needle) => out.push(text.opacity_factor),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, needle, &mut out));
    out
}

#[test]
fn close_prompt_is_fully_drawn_in_the_frame_of_the_menu_choice() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h.state_mut().run("line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    h.run_steps(2);
    menus::activate(h.state_mut(), "close");
    h.step();
    assert_eq!(text_opacity(&h.output().shapes, "Save changes to"), [1.0]);
}
