//! The navigation overlays on the drawing area: the ViewCube (Top face, Home, compass ring and
//! the WCS menu) and the UCS icon.
//!
//! Every overlay registers a widget that senses clicks *and* drags, so egui hands the pointer to
//! the overlay instead of the canvas underneath (a click on the ViewCube must never also pick or
//! start a selection window in the drawing). What the overlays do goes through commands:
//! `zoom.extents` (Top face, Home), `ui.toggle.viewcube`, `ui.toggle.ucsicon`.
//!
//! CADCraft's model view is a 2D plan view of the World Coordinate System: there is no view
//! twist, no 3D view direction and no user coordinate system yet, so the compass ring explains
//! that instead of rotating, and the WCS menu lists the only coordinate system there is.

use egui::{CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

pub mod cube3d;
use cube3d::{CubeProjector, CubeZone};

/// Shown when the compass ring is clicked or dragged.
pub const NO_TWIST: &str = "ViewCube: rotating the view needs a view twist, which the 2D plan view doesn't support yet.";

/// Half-size (in screen pixels) of the unit 3D ViewCube before projection.
const CUBE_HALF_SIZE: f32 = 28.0;

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
    /// Compute the ViewCube layout rectangles anchored to the top-right of `canvas`.
    pub fn new(canvas: Rect) -> Self {
        let center = pos2(canvas.right() - 96.0, canvas.top() + 86.0);
        let ring = 64.0;
        Layout {
            center,
            ring,
            face: Rect::from_center_size(center, vec2(56.0, 56.0)),
            home: Rect::from_center_size(center + vec2(-ring + 2.0, -ring + 2.0), vec2(20.0, 20.0)),
            pill: Rect::from_center_size(center + vec2(0.0, ring + 26.0), vec2(56.0, 16.0)),
            bounds: Rect::from_center_size(center, vec2(2.0 * (ring + 14.0), 2.0 * (ring + 14.0))),
        }
    }

    /// The part of the cube at `p` in Top plan view (not the pill, which is its own widget).
    pub fn part_at(&self, p: Pos2) -> Option<Part> {
        self.part_at_in_orbit(p, cadcraft_render::Orbit3d::default()).0
    }

    /// The [`Part`] and optional [`CubeZone`] (face, edge, or corner) at `p` under `orbit`.
    pub fn part_at_in_orbit(&self, p: Pos2, orbit: cadcraft_render::Orbit3d) -> (Option<Part>, Option<CubeZone>) {
        if self.home.contains(p) {
            return (Some(Part::Home), None);
        }
        let proj = CubeProjector::new(self.center, CUBE_HALF_SIZE, orbit);
        if let Some(zone) = proj.zone_at(p) {
            return (Some(Part::Face), Some(zone));
        }
        if proj.on_compass_ring(p, self.ring) {
            return (Some(Part::Ring), None);
        }
        (None, None)
    }
}

/// Interactive region of the ViewCube widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Face,
    Home,
    Ring,
}

impl Part {
    /// Tooltip text shown when hovering over this part of the ViewCube.
    fn tip(self) -> &'static str {
        match self {
            Part::Face => "Click a face, edge, or corner to align the 3D view; drag to orbit",
            Part::Home => "Home: zoom to the drawing extents",
            Part::Ring => "Compass: drag to rotate the 3D view around the Z axis, or click N/E/S/W",
        }
    }
}

/// Paint the 3D projected ViewCube, tilted compass ring, and Home button, and handle the pointer.
/// The canvas paints the WCS pill itself.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout) {
    let current_orbit = app.session.state().map(|s| s.orbit).unwrap_or_default();
    let proj = CubeProjector::new(l.center, CUBE_HALF_SIZE, current_orbit);
    let resp = ui.interact(l.bounds, ui.scope_id().with("viewcube"), Sense::click_and_drag());
    let (part, hover_zone) = resp.hover_pos().map_or((None, None), |pos| l.part_at_in_orbit(pos, current_orbit));

    let p = ui.painter_at(l.bounds.union(l.pill));
    proj.paint(&p, l.ring, part == Some(Part::Ring), hover_zone);
    paint_home(&p, l.home, part == Some(Part::Home));

    let resp = match (part, hover_zone) {
        (Some(Part::Face), Some(zone)) => resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(zone.tooltip()),
        (Some(part), _) => resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(part.tip()),
        (None, _) => resp,
    };
    let press_pos = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
    let (pressed_on, pressed_zone) = press_pos.map_or((None, None), |o| l.part_at_in_orbit(o, current_orbit));
    if resp.clicked() {
        match pressed_on {
            Some(Part::Face) => {
                let zone = pressed_zone.or(hover_zone).unwrap_or(CubeZone::new(0, 0, 1));
                let (yaw_deg, pitch_deg) = zone.target_angles_deg(current_orbit.yaw.to_degrees());
                if zone == CubeZone::new(0, 0, 1) && current_orbit.is_top() {
                    let _ = app.run("zoom.extents", json!({}));
                } else {
                    let _ = app.run("3dorbit", json!({ "yaw": yaw_deg, "pitch": pitch_deg }));
                    let _ = app.run("zoom.extents", json!({}));
                }
            }
            Some(Part::Home) => {
                let _ = app.run("zoom.extents", json!({}));
            }
            Some(Part::Ring) => {
                if current_orbit.is_top() {
                    app.session.echo(NO_TWIST);
                } else if let Some(click_pos) = resp.interact_pointer_pos() {
                    // Clicking near N / S / E / W on the compass ring in 3D snaps yaw to that cardinal direction.
                    for (dir, target_yaw) in [
                        (cadcraft_geom::Vec3::new(0.0, 1.0, 0.0), 180.0_f64),
                        (cadcraft_geom::Vec3::new(0.0, -1.0, 0.0), 0.0_f64),
                        (cadcraft_geom::Vec3::new(1.0, 0.0, 0.0), -90.0_f64),
                        (cadcraft_geom::Vec3::new(-1.0, 0.0, 0.0), 90.0_f64),
                    ] {
                        if proj.project_ring_point(dir, l.ring).distance(click_pos) <= 14.0 {
                            let _ = app.run(
                                "3dorbit",
                                json!({ "yaw": target_yaw, "pitch": current_orbit.pitch.to_degrees() }),
                            );
                            break;
                        }
                    }
                }
            }
            None => {}
        }
    }
    if resp.dragged() && pressed_on == Some(Part::Face) {
        let d = resp.drag_delta();
        let _ = app.session.orbit_drag(f64::from(d.x), f64::from(d.y));
    }
    if resp.dragged() && pressed_on == Some(Part::Ring) {
        if current_orbit.is_top() {
            if resp.drag_started() {
                app.session.echo(NO_TWIST);
            }
        } else {
            let d = resp.drag_delta();
            let _ = app.session.orbit_drag(f64::from(d.x), 0.0);
        }
    }
    let mut action = None;
    let mut start_orbit = false;
    resp.context_menu(|ui| {
        if ui.button("Home (Zoom Extents)").clicked() {
            action = Some(("zoom.extents", json!({})));
            ui.close();
        }
        if ui.button("3D Orbit (3DO)").clicked() {
            start_orbit = true;
            ui.close();
        }
        ui.separator();
        for (lbl, cmd) in [
            ("Top (2D Plan)", "view.top"),
            ("Front", "view.front"),
            ("Right", "view.right"),
            ("Left", "view.left"),
            ("Back", "view.back"),
            ("SE Isometric", "view.seiso"),
            ("SW Isometric", "view.swiso"),
        ] {
            if ui.button(lbl).clicked() {
                action = Some((cmd, json!({})));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Hide ViewCube").clicked() {
            action = Some(("ui.toggle.viewcube", json!({"on": false})));
            ui.close();
        }
    });
    if start_orbit {
        app.start("3dorbit");
    }
    if let Some((id, params)) = action {
        let _ = app.run(id, params);
    }
    // The WCS pill (painted by the canvas) opens the coordinate-system menu.
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

/// The coordinate-system menu: the WCS, 3D View presets, 3D Orbit and the icon switch.
fn ucs_menu(app: &mut CadApp, popup: egui::Popup<'_>) {
    let mut hide = false;
    let mut preset: Option<&'static str> = None;
    let mut start_orbit = false;
    popup.show(|ui| {
        ui.radio(true, "World (WCS)").on_hover_text("CADCraft draws in the World Coordinate System; user coordinate systems aren't available yet.");
        ui.separator();
        if ui.button("3D Orbit (3DO)").clicked() {
            start_orbit = true;
            ui.close();
        }
        for (lbl, cmd) in [
            ("Top (2D Plan)", "view.top"),
            ("Front", "view.front"),
            ("Right", "view.right"),
            ("SE Isometric", "view.seiso"),
            ("SW Isometric", "view.swiso"),
        ] {
            if ui.button(lbl).clicked() {
                preset = Some(cmd);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Hide UCS Icon").clicked() {
            hide = true;
            ui.close();
        }
    });
    if start_orbit {
        app.start("3dorbit");
    }
    if let Some(cmd) = preset {
        let _ = app.run(cmd, json!({}));
    }
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

    fn view(app: &CadApp) -> (f64, f64, f64) {
        let v = app.session.state().map(|s| s.view()).unwrap_or_default();
        (v.center.x, v.center.y, v.height)
    }

    #[test]
    fn parts_are_found_where_they_are_drawn() {
        let l = Layout::new(Rect::from_min_size(pos2(200.0, 80.0), vec2(1000.0, 800.0)));
        assert_eq!(l.part_at(l.center), Some(Part::Face));
        assert_eq!(l.part_at(l.home.center()), Some(Part::Home));
        assert_eq!(l.part_at(l.center + vec2(0.0, -l.ring)), Some(Part::Ring), "N");
        assert_eq!(l.part_at(l.center + vec2(l.ring + 3.0, 0.0)), Some(Part::Ring), "E");
        assert_eq!(l.part_at(l.center + vec2(34.0, 34.0)), None, "between the face and the ring");
        assert!(l.bounds.contains_rect(l.home) && l.bounds.contains_rect(l.face));
    }

    #[test]
    fn viewcube_clicks_reach_the_cube_not_the_drawing() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let fitted = view(&app);
        // Zoomed in, the Top face fits the drawing again (zoom extents) and nothing is picked.
        let _ = app.run("zoom.in", json!({}));
        assert_ne!(view(&app), fitted);
        click(&mut app, &ctx, l.center, egui::PointerButton::Primary);
        assert_eq!(view(&app), fitted, "Top face zooms to extents");
        assert!(app.session.pending_window.is_none(), "the click didn't start a selection window");
        assert!(app.session.selection().is_empty());
        // Home does the same.
        let _ = app.run("zoom.in", json!({}));
        click(&mut app, &ctx, l.home.center(), egui::PointerButton::Primary);
        assert_eq!(view(&app), fitted, "Home zooms to extents");
        // The ring explains that the 2D view can't rotate, and doesn't touch the drawing.
        click(&mut app, &ctx, l.center + vec2(0.0, -l.ring), egui::PointerButton::Primary);
        assert_eq!(app.session.log.last().map(String::as_str), Some(NO_TWIST));
        assert!(app.session.pending_window.is_none());
        assert_eq!(view(&app), fitted);
        // A click in the drawing still reaches the drawing.
        let empty = canvas(&app).center() + vec2(0.0, -300.0);
        click(&mut app, &ctx, empty, egui::PointerButton::Primary);
        assert!(app.session.pending_window.is_some(), "an empty-space click starts a selection window");
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

    #[test]
    fn cube3d_faces_edges_and_corners_align_view_orientation() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        // Clicking the bottom-right corner of the Top face in 2D plan view selects the Top-Front-Right (SE Isometric) corner.
        let corner_pos = l.center + vec2(22.0, 22.0);
        let (part, zone) = l.part_at_in_orbit(corner_pos, cadcraft_render::Orbit3d::default());
        assert_eq!(part, Some(Part::Face));
        assert_eq!(zone, Some(CubeZone::new(1, -1, 1)), "Top-Front-Right corner");
        click(&mut app, &ctx, corner_pos, egui::PointerButton::Primary);
        let orbit = app.session.state().map(|s| s.orbit).unwrap_or_default();
        assert!(!orbit.is_top(), "clicking corner rotates into 3D isometric view");
        assert!((orbit.yaw.to_degrees() - -45.0).abs() < 1e-3);
        assert!((orbit.pitch.to_degrees() - 54.7356).abs() < 0.05);

        // In 3D SE Isometric view, the Front, Right, and Top faces are all projected on the 3D cube and clickable.
        let proj = CubeProjector::new(l.center, CUBE_HALF_SIZE, orbit);
        let (front_center, _) = proj.project_unit(cadcraft_geom::Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(proj.zone_at(front_center), Some(CubeZone::new(0, -1, 0)), "Front face center");
        click(&mut app, &ctx, front_center, egui::PointerButton::Primary);
        let orbit_after_front = app.session.state().map(|s| s.orbit).unwrap_or_default();
        assert_eq!(orbit_after_front.label(), "FRONT");
    }
}
