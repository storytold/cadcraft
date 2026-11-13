//! The app's own title bar (Windows): the Tool Bar in the title row after the logo and the app's
//! name, the drawing's name at the right, caption buttons at the right edge.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportCommand, ViewportId, pos2, vec2};

const W: f32 = 1400.0;

fn texts(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), Rect::from_min_size(t.pos, t.galley.size()))),
        egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

fn frame(app: &mut CadApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> (Vec<(String, Rect)>, Vec<ViewportCommand>) {
    let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(W, 900.0))), time: Some(time), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    let mut all = Vec::new();
    out.shapes.iter().for_each(|s| texts(&s.shape, &mut all));
    (all, out.viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default())
}

#[test]
fn the_tool_bar_sits_in_the_title_row_after_the_logo_and_name() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.custom_titlebar = true;
    app.ui.show_toolbar = true;
    let ctx = egui::Context::default();
    let mut shown = Vec::new();
    for i in 0..3 {
        shown = frame(&mut app, &ctx, f64::from(i) * 0.1, vec![]).0;
    }
    let brand = shown.iter().find(|(s, _)| s == "CADCraft").map(|(_, r)| *r);
    let Some(brand) = brand else { panic!("no app name: {shown:?}") };
    assert!(brand.center().y < 34.0 && brand.left() < 80.0, "the name follows the logo at the left of the top row: {brand:?}");
    // Nothing of the window's own title row above it: the Tool Bar is the top row.
    assert!(!shown.iter().any(|(s, _)| s.starts_with("CADCraft ")), "{shown:?}");

    // Close (the rightmost caption button) asks the window to close; the app then asks about
    // unsaved drawings.
    let close = pos2(W - 23.0, 17.0);
    let mut cmds = Vec::new();
    let button = |pressed| Event::PointerButton { pos: close, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    for (i, e) in [vec![Event::PointerMoved(close)], vec![button(true)], vec![button(false)]].into_iter().enumerate() {
        cmds.extend(frame(&mut app, &ctx, 1.0 + i as f64 * 0.05, e).1);
    }
    assert!(cmds.contains(&ViewportCommand::Close), "{cmds:?}");
}
