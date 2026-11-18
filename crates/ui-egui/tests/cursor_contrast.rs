//! The thin crosshair and pickbox follow the background under the pointer, including in MSPACE.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Space, Viewport};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_ui_egui::{CadApp, Services, theme};
use egui::{Color32, Context, Pos2, Rect, Shape, Stroke, pos2, vec2};
use serde_json::json;

const DARK_INK: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const LIGHT_INK: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xe8);

struct Canvas {
    app: CadApp,
    ctx: Context,
    previous_theme: egui::Theme,
}

impl Canvas {
    fn new(paper: bool) -> Self {
        let previous_theme = theme::active();
        let mut session = Session::new();
        session.new_drawing(true);
        if paper {
            session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        }
        session.execute("view.set", &json!({"center": [100, 100], "height": 400})).unwrap();
        session.settings.gridmode = false;
        session.settings.dynmode = false;
        let mut app = CadApp::new(session, Services::default());
        app.canvas.zoom_pending = false;
        app.ui.show_command_line = false;
        app.ui.show_ucs_icon = false;
        app.ui.show_viewcube = false;
        app.run("options", json!({"theme": "dark", "systemCursor": false, "PICKBOX": 4, "SELECTIONPREVIEW": 1})).unwrap();
        let mut canvas = Self { app, ctx: Context::default(), previous_theme };
        canvas.paint(pos2(640.0, 400.0));
        canvas
    }

    fn screen(&self, point: Vec2) -> Pos2 {
        self.app.canvas.xf.unwrap().to_screen(point)
    }

    fn paint(&mut self, at: Pos2) -> Vec<egui::epaint::ClippedShape> {
        let mut shapes = Vec::new();
        // Let layout and hover state settle, as in system_cursor.rs.
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
                events: vec![egui::Event::PointerMoved(at)],
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.app.logic(ui.ctx());
                self.app.ui(ui);
            });
            shapes = std::mem::take(&mut output.shapes);
            output.textures_delta.clear();
        }
        shapes
    }

    fn assert_ink(&mut self, at: Pos2, expected: Color32, label: &str) {
        let shapes = self.paint(at);
        assert!(self.app.canvas.rect.unwrap().contains(at), "{label}: pointer must be on the canvas");
        let mut arms = Vec::new();
        let mut boxes = Vec::new();
        for shape in &shapes {
            cursor_strokes(&shape.shape, at, &mut arms, &mut boxes);
        }
        assert_eq!(arms.len(), 4, "{label}: four cursor arms must be painted");
        assert_eq!(boxes.len(), 1, "{label}: the pickbox must be painted");
        for stroke in arms.iter().chain(&boxes) {
            assert_eq!(stroke.width, 1.0, "{label}: keep the thin cursor");
            assert_eq!(stroke.color, expected, "{label}: cursor ink");
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        theme::set_canvas(theme::Canvas::default());
        theme::set_active(self.previous_theme);
    }
}

/// Identify the actual cursor shapes by their position, independently of their colour.
/// The drawings are empty around the pointer, with grids and UCS icons hidden.
fn cursor_strokes(shape: &Shape, at: Pos2, arms: &mut Vec<Stroke>, boxes: &mut Vec<Stroke>) {
    match shape {
        Shape::LineSegment { points: [a, b], stroke } => {
            let direction = *b - *a;
            let from_pointer = *a - at;
            let cross = direction.x * from_pointer.y - direction.y * from_pointer.x;
            if direction.length() > 1.0 && cross.abs() / direction.length() < 0.01 && a.distance(at).min(b.distance(at)) < 16.0 {
                arms.push(*stroke);
            }
        }
        Shape::Rect(rect) if rect.rect.center().distance(at) < 0.01 && rect.rect.size() == vec2(12.0, 12.0) => {
            boxes.push(rect.stroke);
        }
        Shape::Vec(shapes) => shapes.iter().for_each(|s| cursor_strokes(s, at, arms, boxes)),
        _ => {}
    }
}

#[test]
fn model_cursor_keeps_following_live_background_preferences() {
    let mut c = Canvas::new(false);
    let at = c.screen(Vec2::new(100.0, 100.0));
    c.assert_ink(at, LIGHT_INK, "default model");
    c.app.run("options", json!({"modelBackground": "#ffffff"})).unwrap();
    c.assert_ink(at, DARK_INK, "white model");
    c.app.run("options", json!({"modelBackground": null, "theme": "light"})).unwrap();
    c.assert_ink(at, LIGHT_INK, "reset model with light interface");
}

#[test]
fn paper_and_surround_follow_the_sheet_after_pan_and_zoom() {
    let mut c = Canvas::new(true);
    let at = c.screen(Vec2::new(30.0, 40.0));
    c.assert_ink(at, DARK_INK, "white paper");
    let outside = c.screen(Vec2::new(-20.0, 40.0));
    c.assert_ink(outside, LIGHT_INK, "gray surround");

    // Keep the pointer still while navigation moves the sheet out from underneath it.
    c.app.run("view.set", json!({"center": [500, 100], "height": 400})).unwrap();
    c.assert_ink(at, LIGHT_INK, "paper panned away");
    c.app.run("view.set", json!({"center": [100, 100], "height": 1000})).unwrap();
    c.assert_ink(at, LIGHT_INK, "paper zoomed away");
    c.app.run("view.set", json!({"center": [100, 100], "height": 400})).unwrap();
    c.assert_ink(at, DARK_INK, "paper returned");

    // One colour follows the centre even when long arms cross the sheet's boundary.
    c.app.run("options", json!({"CURSORSIZE": 100})).unwrap();
    let edge = c.screen(Vec2::new(0.0, 40.0));
    c.assert_ink(edge + vec2(2.0, 0.0), DARK_INK, "just inside paper");
    c.assert_ink(edge - vec2(2.0, 0.0), LIGHT_INK, "just outside paper");
}

#[test]
fn paper_preferences_are_independent_of_model_preferences() {
    let mut c = Canvas::new(true);
    let at = c.screen(Vec2::new(30.0, 40.0));
    c.app.run("options", json!({"modelBackground": "#ffffff", "paperBackground": "#202020"})).unwrap();
    c.assert_ink(at, LIGHT_INK, "dark paper with white model preference");
    c.assert_ink(c.screen(Vec2::new(-20.0, 40.0)), LIGHT_INK, "surround with white model preference");
    c.app.run("options", json!({"paperBackground": "#ffffff", "modelBackground": "#202020", "theme": "light"})).unwrap();
    c.assert_ink(at, DARK_INK, "white paper with dark model preference and light interface");
    c.app.run("layout.set", json!({"name": "Model"})).unwrap();
    c.assert_ink(at, LIGHT_INK, "switch to dark model");
    c.app.run("layout.set", json!({"name": "Layout1"})).unwrap();
    c.app.run("view.set", json!({"center": [100, 100], "height": 400})).unwrap();
    c.assert_ink(at, DARK_INK, "switch back to white paper");
}

#[test]
fn mspace_and_isometric_cursor_use_the_paper_display_transform() {
    let mut c = Canvas::new(true);
    c.app
        .session
        .doc_mut()
        .unwrap()
        .add(
            &Space::Paper("Layout1".into()),
            Common::default(),
            EntityKind::Viewport(Viewport {
                center: Vec3::new(70.0, 60.0, 0.0),
                width: 100.0,
                height: 80.0,
                view_center: Vec2::new(10000.0, 20000.0),
                view_height: 1000.0,
                id: 2,
                on: true,
                ..Default::default()
            }),
        )
        .unwrap();
    let at = c.screen(Vec2::new(70.0, 60.0));
    let outside = c.screen(Vec2::new(-20.0, 60.0));
    c.app.run("mspace", json!({})).unwrap();
    assert!(c.app.session.state().unwrap().mspace.is_some());
    c.app.session.settings.isodraft = true;
    c.app.run("options", json!({"CURSORSIZE": 100})).unwrap();
    c.assert_ink(at, DARK_INK, "MSPACE on paper");
    c.assert_ink(outside, LIGHT_INK, "MSPACE outside paper");
    c.app.run("view.set", json!({"center": [30000, -40000], "height": 4000})).unwrap();
    c.assert_ink(at, DARK_INK, "MSPACE after model pan and zoom");
    c.app.run("pspace", json!({})).unwrap();
    c.assert_ink(at, DARK_INK, "back in paper space");
}
