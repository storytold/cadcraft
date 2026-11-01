//! Zooming far in with the pointer on an object (#2299): its rollover highlight is dashed, and
//! dashing the whole object made one shape per 10 points of it on screen. Zoomed to the limit
//! that was billions of shapes, so the app froze and then ran out of memory.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

/// One frame of the app; the number of shapes it painted.
fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>) -> usize {
    let raw =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out.shapes.len()
}

#[test]
fn zooming_far_in_on_a_hovered_line_paints_what_shows() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("circle", json!({"center": [50, 50], "radius": 20})).unwrap();
    app.run("line", json!({"points": [[0, 0], [100, 100]]})).unwrap();
    app.run("line", json!({"points": [[0, 100], [100, 0]]})).unwrap();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, Vec::new());
    let over = app.canvas.rect.unwrap().center();
    // The smaller zoom first, so a regression fails there instead of filling memory at the limit.
    for height in [1e-2, 1e-9] {
        // Centred on a point of the first line, which the pointer, at the canvas centre, hovers.
        app.run("view.set", json!({"center": [25, 25], "height": height})).unwrap();
        let shapes = (0..3).map(|_| frame(&mut app, &ctx, vec![egui::Event::PointerMoved(over)])).max().unwrap_or(0);
        // Without a hovered object there is no dashed highlight, and the count says nothing.
        assert!(app.canvas.hover.is_some(), "view height {height}: the line is hovered");
        assert!(shapes < 5_000, "view height {height}: {shapes} shapes painted");
        assert_eq!(app.session.state().unwrap().view().height, height);
    }
}
