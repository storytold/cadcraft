//! Interactive camera navigation overlays and the WCS/UCS menus.

use cadcraft_geom::Vec3;
use cadcraft_render::view3d::Camera3d;
use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// Where the ViewCube's parts are on the canvas.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub center: Pos2,
    /// Radius of the compass ring's centre line.
    pub ring: f32,
    pub face: Rect,
    pub home: Rect,
    /// The "WCS" coordinate-system menu under the cube.
    pub pill: Rect,
    /// Everything that belongs to the cube (the area kept away from the canvas).
    pub bounds: Rect,
}

impl Layout {
    pub fn new(canvas: Rect) -> Self {
        let center = pos2(canvas.right() - 92.0, canvas.top() + 82.0);
        let ring = 58.0;
        Layout {
            center,
            ring,
            // The cube projects to about 31×31 px at an isometric angle; this hit region includes
            // its projected edges and corners while staying well inside the compass ring.
            face: Rect::from_center_size(center, vec2(80.0, 80.0)),
            home: Rect::from_center_size(center + vec2(-ring + 2.0, -ring + 2.0), vec2(20.0, 20.0)),
            pill: Rect::from_center_size(center + vec2(0.0, ring + 26.0), vec2(56.0, 16.0)),
            bounds: Rect::from_center_size(center, vec2(2.0 * (ring + 10.0), 2.0 * (ring + 10.0))),
        }
    }

    /// The part of the cube at `p` (not the pill, which is its own widget).
    pub fn part_at(&self, p: Pos2) -> Option<Part> {
        if self.home.contains(p) {
            return Some(Part::Home);
        }
        if self.face.contains(p) {
            return Some(Part::Face);
        }
        let d = p.distance(self.center);
        ((self.ring - 10.0..=self.ring + 10.0).contains(&d)).then_some(Part::Ring)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Face,
    Home,
    Ring,
}

impl Part {
    fn tip(self) -> &'static str {
        match self {
            Part::Face => "Click a face for a standard view; drag to orbit",
            Part::Home => "Home: southeast isometric view",
            Part::Ring => "Compass: click a direction or drag to rotate",
        }
    }
}

/// A projected cube face and its standard camera preset.
struct Face {
    points: Vec<Pos2>,
    center: Pos2,
    depth: f64,
    preset: &'static str,
    label: &'static str,
}

fn camera(app: &CadApp) -> Camera3d {
    let (azimuth, elevation) =
        app.session.view3d.map(|v| (v.azimuth, v.elevation)).unwrap_or((-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2));
    Camera3d { azimuth, elevation }
}

fn project(camera: Camera3d, center: Pos2, v: Vec3, scale: f32) -> (Pos2, f64) {
    let (p, depth) = camera.project(v);
    (center + vec2(p.x as f32 * scale, -p.y as f32 * scale), depth)
}

fn faces(camera: Camera3d, center: Pos2) -> Vec<Face> {
    let specs = [
        ("right", "RIGHT", [1., 0., 0.], [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]]),
        ("left", "LEFT", [-1., 0., 0.], [[-1., 1., -1.], [-1., -1., -1.], [-1., -1., 1.], [-1., 1., 1.]]),
        ("back", "BACK", [0., 1., 0.], [[1., 1., -1.], [-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.]]),
        ("front", "FRONT", [0., -1., 0.], [[-1., -1., -1.], [1., -1., -1.], [1., -1., 1.], [-1., -1., 1.]]),
        ("top", "TOP", [0., 0., 1.], [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]]),
        ("bottom", "BOTTOM", [0., 0., -1.], [[-1., 1., -1.], [1., 1., -1.], [1., -1., -1.], [-1., -1., -1.]]),
    ];
    let mut result = Vec::new();
    for (preset, label, n, corners) in specs {
        let normal = Vec3::new(n[0], n[1], n[2]);
        let (_, depth) = camera.project(normal);
        if depth <= 1e-8 {
            continue;
        }
        let points: Vec<_> = corners.into_iter().map(|v| project(camera, center, Vec3::new(v[0], v[1], v[2]), 23.0).0).collect();
        let sum = points.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2());
        result.push(Face { points, center: pos2(sum.x / 4., sum.y / 4.), depth, preset, label });
    }
    result.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    result
}

fn contains(points: &[Pos2], p: Pos2) -> bool {
    let mut sign = 0.0_f32;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross.abs() > 0.001 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    sign != 0.0
}

/// Return a precise camera direction for a cube edge or corner hit. Face interiors use their
/// axis-aligned preset; edges and corners use the normalized world direction of that feature.
fn edge_or_corner(camera: Camera3d, center: Pos2, at: Pos2) -> Option<(f64, f64)> {
    let vertices = [-1.0_f64, 1.0];
    let mut nearest: Option<(f32, Vec3)> = None;
    for x in vertices {
        for y in vertices {
            for z in vertices {
                let v = Vec3::new(x, y, z);
                let (point, depth) = project(camera, center, v, 23.0);
                if depth <= 0.0 {
                    continue;
                }
                let distance = at.distance(point);
                if nearest.is_none_or(|(d, _)| distance < d) {
                    nearest = Some((distance, v));
                }
            }
        }
    }
    if let Some((_, v)) = nearest.filter(|(d, _)| *d <= 6.0) {
        let azimuth = v.y.atan2(v.x).to_degrees();
        let elevation = v.z.atan2(v.x.hypot(v.y)).to_degrees();
        return Some((azimuth, elevation));
    }

    // Twelve cube edges. Their midpoints have one zero coordinate and two signed coordinates.
    let mut nearest: Option<(f32, Vec3)> = None;
    for zero_axis in 0..3 {
        for a in vertices {
            for b in vertices {
                let coords = match zero_axis {
                    0 => [0.0, a, b],
                    1 => [a, 0.0, b],
                    _ => [a, b, 0.0],
                };
                let v = Vec3::new(coords[0], coords[1], coords[2]);
                let (point, depth) = project(camera, center, v, 23.0);
                if depth <= 0.0 {
                    continue;
                }
                let distance = at.distance(point);
                if nearest.is_none_or(|(d, _)| distance < d) {
                    nearest = Some((distance, v));
                }
            }
        }
    }
    let (_, v) = nearest.filter(|(d, _)| *d <= 4.5)?;
    Some((v.y.atan2(v.x).to_degrees(), v.z.atan2(v.x.hypot(v.y)).to_degrees()))
}

fn compass(camera: Camera3d, l: &Layout) -> Vec<(&'static str, &'static str, Pos2)> {
    // Compass describes the horizontal world axes; use the same azimuth as the cube,
    // viewed from above, so the ring remains readable even at a side view.
    let c = Camera3d { elevation: std::f64::consts::FRAC_PI_2, ..camera };
    [
        ("N", "back", Vec3::new(0., 1., 0.)),
        ("S", "front", Vec3::new(0., -1., 0.)),
        ("E", "right", Vec3::new(1., 0., 0.)),
        ("W", "left", Vec3::new(-1., 0., 0.)),
    ]
    .into_iter()
    .map(|(label, preset, v)| (label, preset, project(c, l.center, v, l.ring).0))
    .collect()
}

/// Paint and operate the cube. Its widget owns both clicks and drags over the overlay.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout) {
    let resp = ui.interact(l.bounds, ui.scope_id().with("viewcube"), Sense::click_and_drag());
    let part = resp.hover_pos().and_then(|p| l.part_at(p));
    let cam = camera(app);
    let faces = faces(cam, l.center);
    let directions = compass(cam, l);
    let p = ui.painter_at(l.bounds.union(l.pill));
    let ring_col = if part == Some(Part::Ring) { Color32::from_rgb(0x5c, 0x67, 0x7c) } else { Color32::from_rgb(0x48, 0x50, 0x5c) };
    p.circle_stroke(l.center, l.ring, Stroke::new(9.0, ring_col));
    p.circle_stroke(l.center, l.ring + 4.5, Stroke::new(1.0, Color32::from_rgb(0x5c, 0x65, 0x72)));
    for (label, _, at) in &directions {
        p.text(*at, egui::Align2::CENTER_CENTER, *label, egui::FontId::proportional(17.0), Color32::from_rgb(0xc8, 0xcc, 0xd2));
    }
    for face in &faces {
        let hot = resp.hover_pos().is_some_and(|pt| contains(&face.points, pt));
        let color = if hot { Color32::from_rgb(0xb8, 0xbc, 0xc2) } else { Color32::from_rgb(0x9a, 0x9e, 0xa4) };
        p.add(egui::Shape::convex_polygon(face.points.clone(), color, Stroke::new(1.0, Color32::from_rgb(0x6c, 0x70, 0x76))));
        if face.depth > 0.35 {
            p.text(face.center, egui::Align2::CENTER_CENTER, face.label, egui::FontId::proportional(9.0), Color32::from_rgb(0x50, 0x54, 0x5a));
        }
    }
    paint_home(&p, l.home, part == Some(Part::Home));
    let resp = match part {
        Some(part) => resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(part.tip()),
        None => resp,
    };
    let origin = ui.input(|i| i.pointer.press_origin());
    let pressed_on = origin.and_then(|o| l.part_at(o));
    if resp.dragged_by(egui::PointerButton::Primary) && matches!(pressed_on, Some(Part::Face | Part::Ring)) {
        let delta = ui.input(|i| i.pointer.delta());
        if delta.is_finite() {
            let target = if pressed_on == Some(Part::Ring) {
                resp.interact_pointer_pos().map(|at| {
                    let previous = at - delta;
                    let from = (previous.y - l.center.y).atan2(previous.x - l.center.x);
                    let to = (at.y - l.center.y).atan2(at.x - l.center.x);
                    let turn = (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                    (cam.azimuth.to_degrees() + f64::from(turn.to_degrees()), cam.elevation.to_degrees())
                })
            } else {
                Some((
                    cam.azimuth.to_degrees() - f64::from(delta.x) * 0.6,
                    (cam.elevation.to_degrees() - f64::from(delta.y) * 0.6).clamp(-90.0, 90.0),
                ))
            };
            if let Some((azimuth, elevation)) = target {
                let azimuth = (azimuth + 180.0).rem_euclid(360.0) - 180.0;
                let _ = app.run("view3d", json!({"azimuth": azimuth, "elevation": elevation, "orbit": false}));
                app.canvas.orbit_dragged = true;
                ui.ctx().request_repaint();
            }
        }
    }
    if resp.clicked() {
        let at = resp.interact_pointer_pos().unwrap_or(l.center);
        let preset = match l.part_at(at) {
            Some(Part::Home) => Some("seiso"),
            Some(Part::Face) => {
                if let Some((azimuth, elevation)) = edge_or_corner(cam, l.center, at) {
                    let _ = app.run("view3d", json!({"azimuth": azimuth, "elevation": elevation, "orbit": false}));
                    None
                } else {
                    faces.iter().rev().find(|f| contains(&f.points, at)).map(|f| f.preset)
                }
            }
            Some(Part::Ring) => directions.iter().min_by(|a, b| a.2.distance_sq(at).total_cmp(&b.2.distance_sq(at))).map(|d| d.1),
            None => None,
        };
        if let Some(preset) = preset {
            let _ = app.run("view3d", json!({"preset": preset, "orbit": false}));
        }
    }
    let mut action = None;
    resp.context_menu(|ui| {
        for (label, preset) in [
            ("Home", "seiso"),
            ("Top", "top"),
            ("Bottom", "bottom"),
            ("Front", "front"),
            ("Back", "back"),
            ("Left", "left"),
            ("Right", "right"),
            ("SW Isometric", "swiso"),
            ("SE Isometric", "seiso"),
            ("NE Isometric", "neiso"),
            ("NW Isometric", "nwiso"),
        ] {
            if ui.button(label).clicked() {
                action = Some(("view3d", json!({"preset": preset, "orbit": false})));
                ui.close();
            }
        }
        if ui.button("Plan").clicked() {
            action = Some(("plan", json!({})));
            ui.close();
        }
        if ui.button("Hide ViewCube").clicked() {
            action = Some(("ui.toggle.viewcube", json!({"on": false})));
            ui.close();
        }
    });
    if let Some((id, params)) = action {
        let _ = app.run(id, params);
    }
    let pill = ui.interact(l.pill, ui.scope_id().with("viewcube_wcs"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
    let pill = pill.on_hover_text("Coordinate system");
    ucs_menu(app, egui::Popup::menu(&pill));
}

/// A small house, drawn in code.
fn paint_home(p: &egui::Painter, r: Rect, hot: bool) {
    let t = Tokens::get();
    if hot {
        p.rect_filled(r, 3.0, t.control_hover);
    }
    let st = Stroke::new(1.3, if hot { t.text } else { t.text_dim });
    let c = r.center();
    let roof = [c + vec2(-6.5, 0.5), c + vec2(0.0, -6.0), c + vec2(6.5, 0.5)];
    p.line(roof.to_vec(), st);
    p.line(vec![c + vec2(-4.5, -1.0), c + vec2(-4.5, 6.0), c + vec2(4.5, 6.0), c + vec2(4.5, -1.0)], st);
    p.line_segment([c + vec2(-1.2, 6.0), c + vec2(-1.2, 2.5)], st);
    p.line_segment([c + vec2(1.2, 6.0), c + vec2(1.2, 2.5)], st);
    p.line_segment([c + vec2(-1.2, 2.5), c + vec2(1.2, 2.5)], st);
}

/// The UCS icon's arms (as drawn by the canvas at the lower-left corner): the X arm and the Y arm.
pub fn ucs_arms(canvas: Rect) -> [Rect; 2] {
    let o = pos2(canvas.left() + 36.0, canvas.bottom() - 34.0);
    [Rect::from_min_max(o + vec2(-8.0, -8.0), o + vec2(80.0, 8.0)), Rect::from_min_max(o + vec2(-8.0, -82.0), o + vec2(8.0, 8.0))]
}

/// Make the UCS icon clickable: hovering highlights it, a click (either button) opens the
/// coordinate-system menu.
pub fn ucs_icon(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    let t = Tokens::get();
    let [x, y] = ucs_arms(canvas);
    let rx = ui.interact(x, ui.scope_id().with("ucs_x"), Sense::click_and_drag());
    let ry = ui.interact(y, ui.scope_id().with("ucs_y"), Sense::click_and_drag());
    let resp = (rx | ry).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("UCS icon: World Coordinate System");
    if resp.hovered() {
        let o = pos2(x.left() + 8.0, x.center().y);
        let st = Stroke::new(2.0, t.hover);
        let p = ui.painter_at(canvas);
        p.line_segment([o, o + vec2(60.0, 0.0)], st);
        p.line_segment([o, o + vec2(0.0, -60.0)], st);
    }
    // Opened with `Bool(true)`: a pointer-anchored popup records its position only then.
    let open = (resp.clicked() || resp.secondary_clicked()).then_some(egui::SetOpenCommand::Bool(true));
    ucs_menu(app, egui::Popup::menu(&resp).open_memory(open).at_pointer_fixed());
}

/// The coordinate-system menu: the WCS (the only coordinate system so far) and the icon switch.
fn ucs_menu(app: &mut CadApp, popup: egui::Popup<'_>) {
    let mut hide = false;
    popup.show(|ui| {
        ui.radio(true, "World (WCS)").on_hover_text("CADCraft draws in the World Coordinate System; user coordinate systems aren't available yet.");
        ui.separator();
        if ui.button("Hide UCS Icon").clicked() {
            hide = true;
            ui.close();
        }
    });
    if hide {
        let _ = app.run("ui.toggle.ucsicon", json!({"on": false}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    /// The app with the sample drawing, laid out once in a 1400×900 window.
    fn app_and_ctx() -> (CadApp, egui::Context) {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.ui.in_window_menu = true;
        app.start("ui.sample");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        (app, ctx)
    }

    fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    /// A real pointer click (move, press, release in separate frames, like the control channel).
    pub(crate) fn click(app: &mut CadApp, ctx: &egui::Context, at: Pos2, button: egui::PointerButton) {
        let m = egui::Modifiers::default();
        frame(app, ctx, vec![egui::Event::PointerMoved(at)]);
        frame(app, ctx, vec![egui::Event::PointerButton { pos: at, button, pressed: true, modifiers: m }]);
        frame(app, ctx, vec![egui::Event::PointerButton { pos: at, button, pressed: false, modifiers: m }]);
        frame(app, ctx, Vec::new());
    }

    /// A menu popup was shown in the last frame.
    fn menu_shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground))
    }

    fn canvas(app: &CadApp) -> Rect {
        app.canvas.rect.unwrap_or(Rect::NOTHING)
    }

    #[test]
    fn cube_navigation_changes_camera_and_keeps_selection_clear() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        click(&mut app, &ctx, l.home.center(), egui::PointerButton::Primary);
        let v = app.session.view3d.unwrap();
        assert!((v.azimuth.to_degrees() + 45.0).abs() < 0.001);
        click(&mut app, &ctx, l.center, egui::PointerButton::Primary);
        assert!(app.session.view3d.is_some());
        let _ = app.run("plan", json!({}));
        let m = egui::Modifiers::default();
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(l.center)]);
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: l.center, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(l.center + vec2(20.0, 20.0))]);
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton { pos: l.center + vec2(20.0, 20.0), button: egui::PointerButton::Primary, pressed: false, modifiers: m }],
        );
        let v = app.session.view3d.expect("drag starts 3D view");
        assert!(v.elevation.to_degrees() < 90.0);
        assert!(app.session.selection().is_empty());
        assert!(app.session.pending_window.is_none());
        assert_ne!(
            faces(camera(&app), l.center)[0].points,
            faces(Camera3d { azimuth: -std::f64::consts::FRAC_PI_2, elevation: std::f64::consts::FRAC_PI_2 }, l.center)[0].points
        );
    }

    #[test]
    fn face_edge_corner_and_compass_clicks_only_change_the_camera() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let state = app.session.state().unwrap();
        let revision = state.revision;
        let undo_len = state.undo.len();
        let drawing = state.doc.clone();

        // The projected center of an exposed side face snaps to its corresponding orthographic view.
        let _ = app.run("view3d", json!({"preset": "seiso", "orbit": false}));
        let cam = camera(&app);
        let front = faces(cam, l.center).into_iter().find(|f| f.preset == "front").unwrap();
        click(&mut app, &ctx, front.center, egui::PointerButton::Primary);
        assert!((app.session.view3d.unwrap().azimuth.to_degrees() + 90.0).abs() < 0.001);
        assert!(app.session.view3d.unwrap().elevation.abs() < 0.001);

        // A visible corner sets the diagonal direction and elevation from that cube corner.
        let cam = camera(&app);
        let corner = project(cam, l.center, Vec3::new(1.0, -1.0, 1.0), 23.0).0;
        click(&mut app, &ctx, corner, egui::PointerButton::Primary);
        let v = app.session.view3d.unwrap();
        assert!((v.azimuth.to_degrees() + 45.0).abs() < 0.001);
        assert!((v.elevation.to_degrees() - 35.264_389_682_754_654).abs() < 0.001);

        // A visible cube edge snaps to a horizontal diagonal.
        let cam = camera(&app);
        let edge = project(cam, l.center, Vec3::new(1.0, -1.0, 0.0), 23.0).0;
        click(&mut app, &ctx, edge, egui::PointerButton::Primary);
        let v = app.session.view3d.unwrap();
        assert!((v.azimuth.to_degrees() + 45.0).abs() < 0.001);
        assert!(v.elevation.abs() < 0.001);

        // Ring cardinal click selects its horizontal standard view.
        let north = compass(camera(&app), &l).into_iter().find(|d| d.0 == "N").unwrap().2;
        click(&mut app, &ctx, north, egui::PointerButton::Primary);
        assert!((app.session.view3d.unwrap().azimuth.to_degrees() - 90.0).abs() < 0.001);
        assert!(app.session.view3d.unwrap().elevation.abs() < 0.001);

        let state = app.session.state().unwrap();
        assert_eq!(state.revision, revision);
        assert_eq!(state.undo.len(), undo_len);
        assert!(std::sync::Arc::ptr_eq(&state.doc, &drawing));
        assert!(state.selection.is_empty());
        assert!(app.session.pending_window.is_none());
    }

    #[test]
    fn dragging_compass_changes_azimuth_without_touching_document_state() {
        let (mut app, ctx) = app_and_ctx();
        let _ = app.run("view3d", json!({"preset": "seiso", "orbit": false}));
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let l = Layout::new(canvas(&app));
        let start = l.center + vec2(l.ring, 0.0);
        let end = l.center + vec2(0.0, l.ring);
        let before = app.session.view3d.unwrap();
        let revision = app.session.state().unwrap().revision;
        let undo_len = app.session.state().unwrap().undo.len();
        let zoom = app.session.state().unwrap().view();
        let m = egui::Modifiers::default();
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(start)]);
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: m }]);
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let after = app.session.view3d.unwrap();
        assert!((after.azimuth - before.azimuth).abs() > 0.01);
        assert_eq!(after.elevation, before.elevation);
        assert_eq!(app.session.state().unwrap().revision, revision);
        assert_eq!(app.session.state().unwrap().undo.len(), undo_len);
        assert_eq!(app.session.state().unwrap().view(), zoom);
        assert!(app.session.state().unwrap().selection.is_empty());
        assert!(app.session.pending_window.is_none());
    }

    #[test]
    fn ucs_icon_and_wcs_pill_open_the_coordinate_system_menu() {
        let (mut app, ctx) = app_and_ctx();
        let [x, _] = ucs_arms(canvas(&app));
        click(&mut app, &ctx, x.center(), egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "UCS icon menu");
        assert!(app.session.pending_window.is_none(), "the click stayed on the icon");
        let l = Layout::new(canvas(&app));
        click(&mut app, &ctx, l.pill.center(), egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "WCS menu");
        assert!(app.session.pending_window.is_none());
    }
}
