//! CURSORSIZE 100 runs the crosshair out to the edges of the drawing area (#951); smaller sizes
//! are as long as they were.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Pos2, Rect, Shape, pos2, vec2};

fn screen() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))
}

fn lines(shape: &Shape, out: &mut Vec<(Pos2, Pos2, egui::Stroke)>) {
    match shape {
        Shape::LineSegment { points, stroke } => out.push((points[0], points[1], *stroke)),
        Shape::Vec(v) => v.iter().for_each(|s| lines(s, out)),
        _ => {}
    }
}

/// The crosshair's line segments (horizontal ones and vertical ones, 1 point, the crosshair
/// colour) with the pointer at `at`, and the drawing area.
fn crosshair(percent: f64, at: Pos2) -> (Vec<(Pos2, Pos2)>, Rect) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.session.settings.cursorsize = percent;
    let ctx = egui::Context::default();
    let mut found = Vec::new();
    for _ in 0..4 {
        let raw = egui::RawInput { screen_rect: Some(screen()), events: vec![egui::Event::PointerMoved(at)], ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        let mut all = Vec::new();
        out.shapes.iter().for_each(|c| lines(&c.shape, &mut all));
        let colour = cadcraft_ui_egui::theme::Tokens::get().crosshair;
        found = all
            .into_iter()
            .filter(|(a, b, s)| {
                s.color == colour && s.width == 1.0 && ((a.y == b.y && a.x != b.x) || (a.x == b.x && a.y != b.y)) && (a.x == at.x || a.y == at.y)
            })
            .map(|(a, b, _)| (a, b))
            .collect();
    }
    (found, app.canvas.rect.expect("the drawing area was laid out"))
}

fn horizontal(lines: &[(Pos2, Pos2)]) -> (f32, f32) {
    let h = lines.iter().filter(|(a, b)| a.y == b.y);
    (h.clone().map(|(a, b)| a.x.min(b.x)).fold(f32::MAX, f32::min), h.map(|(a, b)| a.x.max(b.x)).fold(f32::MIN, f32::max))
}

fn vertical(lines: &[(Pos2, Pos2)]) -> (f32, f32) {
    let v = lines.iter().filter(|(a, b)| a.x == b.x);
    (v.clone().map(|(a, b)| a.y.min(b.y)).fold(f32::MAX, f32::min), v.map(|(a, b)| a.y.max(b.y)).fold(f32::MIN, f32::max))
}

#[test]
fn at_100_percent_the_arms_reach_every_edge_of_the_drawing_area() {
    // Near each side and corner of the area (the bottom is the command line's), and in the middle.
    for (fx, fy) in [(0.02, 0.5), (0.98, 0.5), (0.5, 0.05), (0.5, 0.8), (0.05, 0.1), (0.95, 0.75), (0.5, 0.5)] {
        let probe = crosshair(100.0, pos2(640.0, 400.0)).1;
        let at = pos2(probe.min.x + probe.width() * fx, probe.min.y + probe.height() * fy);
        let (found, rect) = crosshair(100.0, at);
        let (left, right) = horizontal(&found);
        let (top, bottom) = vertical(&found);
        assert!(left <= rect.min.x && right >= rect.max.x, "at {at:?} in {rect:?}: horizontal arm {left}..{right}");
        assert!(top <= rect.min.y && bottom >= rect.max.y, "at {at:?} in {rect:?}: vertical arm {top}..{bottom}");
    }
}

#[test]
fn below_100_percent_the_arms_are_a_share_of_the_height_as_before() {
    let probe = crosshair(5.0, pos2(640.0, 400.0)).1;
    let at = probe.center();
    for percent in [5.0, 25.0, 99.0] {
        let (found, rect) = crosshair(percent, at);
        let want = (rect.height() * percent as f32 / 100.0).max(12.0);
        let (left, right) = horizontal(&found);
        let (top, bottom) = vertical(&found);
        for (got, label) in [(at.x - left, "left"), (right - at.x, "right"), (at.y - top, "up"), (bottom - at.y, "down")] {
            assert!((got - want).abs() < 0.01, "{percent}% {label} arm {got}, want {want}");
        }
    }
}
