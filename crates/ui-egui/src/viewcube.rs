//! The navigation overlays on the drawing area: the ViewCube (Top face, Home, compass ring and
//! the UCS menu) and the UCS icon.
//!
//! Every overlay registers a widget that senses clicks *and* drags, so egui hands the pointer to
//! the overlay instead of the canvas underneath (a click on the ViewCube must never also pick or
//! start a selection window in the drawing). What the overlays do goes through commands:
//! `zoom.extents` (Top face, Home), `ui.toggle.viewcube`, `ui.toggle.ucsicon`, `ucs`.
//!
//! CADCraft's model view is a 2D plan view: there is no view twist and no 3D view direction yet,
//! so the compass ring explains that instead of rotating. The UCS menu lists World and the
//! drawing's named UCSs.

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// Shown when the compass ring is clicked or dragged.
pub const NO_TWIST: &str = "ViewCube: rotating the view needs a view twist, which the 2D plan view doesn't support yet.";

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
            face: Rect::from_center_size(center, vec2(44.0, 44.0)),
            home: Rect::from_center_size(center + vec2(-ring + 2.0, -ring + 2.0), vec2(20.0, 20.0)),
            pill: Rect::from_center_size(center + vec2(0.0, ring + 26.0), vec2(84.0, 16.0)),
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
            Part::Face => "Top: plan view of the drawing (zoom extents)",
            Part::Home => "Home: zoom to the drawing extents",
            Part::Ring => "Compass: view rotation isn't available in the 2D plan view yet",
        }
    }
}

/// Paint the ViewCube's ring, compass letters, Top face and Home button, and handle the pointer.
/// The canvas paints the WCS pill itself; this returns the pointer's part for its highlight.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout) {
    let resp = ui.interact(l.bounds, ui.scope_id().with("viewcube"), Sense::click_and_drag());
    let part = resp.hover_pos().and_then(|p| l.part_at(p));
    let p = ui.painter_at(l.bounds.union(l.pill));
    let ring_col = if part == Some(Part::Ring) { Color32::from_rgb(0x5c, 0x67, 0x7c) } else { Color32::from_rgb(0x48, 0x50, 0x5c) };
    p.circle_stroke(l.center, l.ring, Stroke::new(9.0, ring_col));
    p.circle_stroke(l.center, l.ring + 4.5, Stroke::new(1.0, Color32::from_rgb(0x5c, 0x65, 0x72)));
    let f = egui::FontId::proportional(17.0);
    let lc = Color32::from_rgb(0xc8, 0xcc, 0xd2);
    let c = l.center;
    let ring = l.ring;
    p.text(c + vec2(0.0, -ring - 1.0), egui::Align2::CENTER_CENTER, crate::tl!("N"), f.clone(), lc);
    p.text(c + vec2(0.0, ring + 1.0), egui::Align2::CENTER_CENTER, crate::tl!("S"), f.clone(), lc);
    p.text(c + vec2(ring + 1.0, 0.0), egui::Align2::CENTER_CENTER, crate::tl!("E"), f.clone(), lc);
    p.text(c + vec2(-ring - 1.0, 0.0), egui::Align2::CENTER_CENTER, crate::tl!("W"), f, lc);
    let face_col = if part == Some(Part::Face) { Color32::from_rgb(0xb8, 0xbc, 0xc2) } else { Color32::from_rgb(0x9a, 0x9e, 0xa4) };
    p.rect_filled(l.face, 2.0, face_col);
    p.rect_stroke(l.face, 2.0, Stroke::new(1.0, Color32::from_rgb(0x6c, 0x70, 0x76)), egui::StrokeKind::Inside);
    p.text(c, egui::Align2::CENTER_CENTER, crate::tl!("TOP"), egui::FontId::proportional(13.0), Color32::from_rgb(0x50, 0x54, 0x5a));
    paint_home(&p, l.home, part == Some(Part::Home));

    let resp = match part {
        Some(part) => resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(part.tip()),
        None => resp,
    };
    let pressed_on = resp.interact_pointer_pos().and_then(|o| l.part_at(o));
    if resp.clicked() {
        match pressed_on {
            Some(Part::Face | Part::Home) => {
                let _ = app.run("zoom.extents", json!({}));
            }
            Some(Part::Ring) => app.session.echo(NO_TWIST),
            None => {}
        }
    }
    if resp.drag_started() && pressed_on == Some(Part::Ring) {
        app.session.echo(NO_TWIST);
    }
    let mut action = None;
    resp.context_menu(|ui| {
        if ui.button("Home").clicked() {
            action = Some(("zoom.extents", json!({})));
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
    // The UCS pill (painted by the canvas, wide enough for a cut-short UCS name) opens the
    // coordinate-system menu.
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

/// How long the UCS icon's arms are, in points.
pub const ARM: f32 = 60.0;
/// How far past an arm's end its X or Y label reaches, in points.
const LABEL: f32 = 16.0;
/// How close to an arm a pointer counts as on the icon, in points.
const HIT: f32 = 8.0;

/// Where the UCS icon is drawn: the point its arms start from, the screen directions of its X
/// and Y arms (unit vectors: the current UCS's axes seen in the plan view) and whether the UCS is
/// the World UCS (the icon then has a square at its corner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconPlace {
    pub origin: Pos2,
    pub x: egui::Vec2,
    pub y: egui::Vec2,
    pub world: bool,
}

impl IconPlace {
    /// The World UCS's icon at the lower-left corner of the drawing area.
    pub fn corner(canvas: Rect) -> IconPlace {
        IconPlace { origin: pos2(canvas.left() + 36.0, canvas.bottom() - 34.0), x: vec2(1.0, 0.0), y: vec2(0.0, -1.0), world: true }
    }

    /// The icon for the current UCS: at its origin when UCSICON asks for that (ORigin) and the
    /// whole icon fits there, else at the corner. `to_screen` maps a point of the space shown.
    pub fn of(app: &CadApp, canvas: Rect, to_screen: impl Fn(cadcraft_geom::Vec2) -> Pos2) -> IconPlace {
        let u = app.session.input_ucs();
        let dir = |v: cadcraft_geom::Vec3, or: egui::Vec2| {
            let d = vec2(v.x as f32, -(v.y as f32));
            let l = d.length();
            if l.is_finite() && l > 1e-6 { d / l } else { or }
        };
        let mut place = IconPlace::corner_turned(canvas, dir(u.x, vec2(1.0, 0.0)), dir(u.y, vec2(0.0, -1.0)), u.is_world());
        // At the origin only where the screen shows the space the UCS belongs to (not from paper
        // space into an active viewport).
        let same_space = app.session.space() == app.session.layout_space();
        if app.session.ucsicon_bits() & 2 != 0 && same_space {
            let at = to_screen(u.origin.xy());
            let reach = ARM + LABEL;
            let inner = canvas.shrink(12.0);
            if [at, at + place.x * reach, at + place.y * reach].iter().all(|p| p.x.is_finite() && p.y.is_finite() && inner.contains(*p)) {
                place.origin = at;
            }
        }
        place
    }

    /// The icon with arms along `x` and `y` at the lower-left corner: placed so the arms and their
    /// labels take the corner the World UCS's icon does, whichever way they point.
    pub fn corner_turned(canvas: Rect, x: egui::Vec2, y: egui::Vec2, world: bool) -> IconPlace {
        let home = IconPlace::corner(canvas).origin;
        let reach = ARM + LABEL;
        let (ends_x, ends_y) = ([0.0, x.x * reach, y.x * reach], [0.0, x.y * reach, y.y * reach]);
        // The World icon's arms run right and up from `home`: shift so the turned arms' left and
        // bottom edges sit where those do.
        let left = ends_x.iter().copied().fold(0.0f32, f32::min);
        let bottom = ends_y.iter().copied().fold(0.0f32, f32::max);
        let origin = pos2(home.x - left, home.y - bottom);
        IconPlace { origin: if origin.x.is_finite() && origin.y.is_finite() { origin } else { home }, x, y, world }
    }

    /// The icon's arms out to their labels: the X arm and the Y arm, as screen segments.
    pub fn arms(&self) -> [[Pos2; 2]; 2] {
        let o = self.origin;
        [[o, o + self.x * (ARM + 20.0)], [o, o + self.y * (ARM + 22.0)]]
    }

    /// Whether `p` is on the icon: within [`HIT`] of an arm (labels included), not merely in the
    /// box around a turned arm.
    pub fn hit(&self, p: Pos2) -> bool {
        self.arms().iter().any(|[a, b]| {
            let ab = *b - *a;
            let t = if ab.length_sq() > 0.0 { ((p - *a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
            (*a + ab * t - p).length() <= HIT
        })
    }
}

/// How the current UCS is named on screen.
fn ucs_label(app: &CadApp) -> String {
    let (u, name) = app.session.ucs_named();
    if u.is_world() && name.is_empty() {
        "World Coordinate System".into()
    } else if name.is_empty() {
        "Unnamed UCS".into()
    } else {
        name
    }
}

/// Make the UCS icon clickable: hovering highlights it, a click (either button) opens the
/// coordinate-system menu.
pub fn ucs_icon(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect, place: &IconPlace) {
    let t = Tokens::get();
    // The arms' boxes, but only a pointer near an arm takes clicks (a turned arm's box is mostly
    // drawing). The widget stays every frame so its menu, once open, keeps its anchor.
    let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| place.hit(p));
    let sense = if near { Sense::click_and_drag() } else { Sense::hover() };
    let [x, y] = place.arms().map(|[a, b]| Rect::from_two_pos(a, b).expand(HIT));
    let rx = ui.interact(x, ui.scope_id().with("ucs_x"), sense);
    let ry = ui.interact(y, ui.scope_id().with("ucs_y"), sense);
    let mut resp = rx | ry;
    if near {
        resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(format!("UCS icon: {}", ucs_label(app)));
    }
    if near && resp.hovered() {
        let o = place.origin;
        let st = Stroke::new(2.0, t.hover);
        let p = ui.painter_at(canvas);
        p.line_segment([o, o + place.x * ARM], st);
        p.line_segment([o, o + place.y * ARM], st);
    }
    // Opened with `Bool(true)`: a pointer-anchored popup records its position only then.
    let open = (resp.clicked() || resp.secondary_clicked()).then_some(egui::SetOpenCommand::Bool(true));
    ucs_menu(app, egui::Popup::menu(&resp).open_memory(open).at_pointer_fixed());
}

/// The coordinate-system menu: World, the drawing's named UCSs (picking one makes it current,
/// through the UCS command) and the icon switch.
fn ucs_menu(app: &mut CadApp, popup: egui::Popup<'_>) {
    let (u, current) = app.session.ucs_named();
    let names: Vec<String> = app.session.doc().map(|d| d.ucss.iter().map(|u| u.name.clone()).collect()).unwrap_or_default();
    let mut action: Option<(&str, serde_json::Value)> = None;
    popup.show(|ui| {
        if ui.radio(u.is_world() && current.is_empty(), "World (WCS)").clicked() {
            action = Some(("ucs.world", json!({})));
            ui.close();
        }
        for n in &names {
            if ui.radio(current.eq_ignore_ascii_case(n), n.as_str()).clicked() {
                action = Some(("ucs", json!({"option": "named", "action": "restore", "name": n})));
                ui.close();
            }
        }
        if !u.is_world() && current.is_empty() {
            let _ = ui.radio(true, "Unnamed");
        }
        ui.separator();
        if ui.button("Hide UCS Icon").clicked() {
            action = Some(("ui.toggle.ucsicon", json!({"on": false})));
            ui.close();
        }
    });
    if let Some((id, params)) = action {
        menu_action(app, id, params);
    }
}

/// Run a coordinate-system menu pick. A failure is already in the command history:
/// [`CadApp::run`] echoes it (and the icon switch runs UCSICON through it).
fn menu_action(app: &mut CadApp, id: &str, params: serde_json::Value) {
    let _ = app.run(id, params);
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

    /// A menu pick that fails (a tilted named UCS) tells the user once, not twice.
    #[test]
    fn a_failed_menu_pick_is_reported_once() {
        let (mut app, _ctx) = app_and_ctx();
        let tilted = cadcraft_engine::doc::Ucs {
            name: "T".into(),
            origin: cadcraft_engine::geom::Vec3::ZERO,
            x_axis: cadcraft_engine::geom::Vec3::new(1.0, 0.0, 0.0),
            y_axis: cadcraft_engine::geom::Vec3::new(0.0, 0.0, 1.0),
        };
        app.session.doc_mut().unwrap().ucss.push(tilted);
        menu_action(&mut app, "ucs", json!({"option": "named", "action": "restore", "name": "T"}));
        let told = app.session.log.iter().filter(|l| l.contains("tilted out of the XY plane")).count();
        assert_eq!(told, 1, "{:?}", app.session.log);
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
        assert_eq!(l.part_at(l.center + vec2(30.0, 30.0)), None, "between the face and the ring");
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
    fn the_ucs_icon_follows_the_ucs() {
        let (mut app, _ctx) = app_and_ctx();
        let c = canvas(&app);
        // A plain mapping: the canvas centre is world (0, 0), one point per unit.
        let to_screen = |p: cadcraft_geom::Vec2| pos2(c.center().x + p.x as f32, c.center().y - p.y as f32);
        let world = IconPlace::of(&app, c, to_screen);
        assert!(world.world && (world.x - vec2(1.0, 0.0)).length() < 1e-6 && (world.y - vec2(0.0, -1.0)).length() < 1e-6);
        // At the World origin (on screen, with ORigin on by default).
        assert!((world.origin - c.center()).length() < 1e-3, "{:?} vs {:?}", world.origin, c.center());
        // A UCS turned 90° at (20, 10): the X arm points up the screen, the Y arm left.
        app.run("ucs", json!({"option": "origin", "origin": [20, 10]})).unwrap();
        app.run("ucs", json!({"option": "z", "angle": 90})).unwrap();
        let p = IconPlace::of(&app, c, to_screen);
        assert!(!p.world);
        assert!((p.x - vec2(0.0, -1.0)).length() < 1e-6 && (p.y - vec2(-1.0, 0.0)).length() < 1e-6, "{p:?}");
        assert!((p.origin - pos2(c.center().x + 20.0, c.center().y - 10.0)).length() < 1e-3);
        // Its click areas lie along the turned arms.
        assert!(p.hit(p.origin + p.x * 40.0) && p.hit(p.origin + p.y * 40.0));
        // Noorigin, or an origin off screen: the corner, the turned icon in the World icon's place.
        let corner = IconPlace::corner_turned(c, p.x, p.y, false).origin;
        app.run("ucsicon", json!({"origin": false})).unwrap();
        assert_eq!(IconPlace::of(&app, c, to_screen).origin, corner);
        app.run("ucsicon", json!({"origin": true})).unwrap();
        app.run("ucs", json!({"option": "origin", "origin": [1e6, 0]})).unwrap();
        assert_eq!(IconPlace::of(&app, c, to_screen).origin, corner);
        app.run("ucs", json!({"option": "world"})).unwrap();
        app.run("ucs", json!({"option": "origin", "origin": [1e6, 0]})).unwrap();
        assert_eq!(IconPlace::of(&app, c, to_screen).origin, IconPlace::corner(c).origin, "unturned: where it always was");
        // The menu switch turns UCSICON off through its command.
        app.run("ui.toggle.ucsicon", json!({"on": false})).unwrap();
        assert_eq!(app.session.ucsicon_bits() & 1, 0);
        app.run("ui.toggle.ucsicon", json!({})).unwrap();
        assert_eq!(app.session.ucsicon_bits() & 1, 1, "a toggle without `on` flips it");
        app.run("ui.toggle.ucsicon.origin", json!({"on": false})).unwrap();
        assert_eq!(app.session.ucsicon_bits(), 1);
    }

    /// At the corner, a turned icon's arms and labels stay on the canvas at every angle, inside
    /// the World icon's corner box.
    #[test]
    fn a_turned_corner_icon_stays_on_the_canvas() {
        let c = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0));
        let home = IconPlace::corner(c).origin;
        let reach = ARM + LABEL;
        let (box_left, box_bottom) = (home.x, home.y);
        for deg in (0..360).step_by(15) {
            let a = (deg as f32).to_radians();
            // Screen y runs down: the Y arm is the X arm turned a quarter counterclockwise on screen.
            let (x, y) = (vec2(a.cos(), -a.sin()), vec2(-a.sin(), -a.cos()));
            let p = IconPlace::corner_turned(c, x, y, false);
            for end in [p.origin, p.origin + x * reach, p.origin + y * reach] {
                assert!(c.contains(end), "{deg}°: {end:?} off {c:?}");
                assert!(end.x >= box_left - 1e-3 && end.y <= box_bottom + 1e-3, "{deg}°: {end:?}");
            }
        }
        // Unturned, exactly where the World icon always was.
        assert_eq!(IconPlace::corner_turned(c, vec2(1.0, 0.0), vec2(0.0, -1.0), true).origin, home);
    }

    /// Only a pointer near an arm is on the icon: on a 45° UCS the corner of the arm's box, well
    /// away from the arm, is drawing.
    #[test]
    fn ucs_icon_hits_follow_the_arms_not_their_boxes() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let p = IconPlace { origin: pos2(200.0, 200.0), x: vec2(s, -s), y: vec2(-s, -s), world: false };
        assert!(p.hit(p.origin) && p.hit(p.origin + p.x * 50.0) && p.hit(p.origin + p.y * (ARM + 20.0)));
        assert!(p.hit(p.origin + p.x * 30.0 + p.y * 7.0), "within reach of the X arm");
        // The X arm's box spans 200..257 across; its far corner is 40 points from the arm.
        let box_corner = pos2(200.0 + (ARM + 20.0) * s, 200.0);
        assert!(!p.hit(box_corner), "{box_corner:?}");
        assert!(!p.hit(p.origin + p.x * (ARM + 40.0)), "past the label");
        assert!(!p.hit(pos2(f32::NAN, 0.0)));
    }

    #[test]
    fn ucs_icon_and_wcs_pill_open_the_coordinate_system_menu() {
        let (mut app, ctx) = app_and_ctx();
        // Where the canvas drew it: at the World origin when that is on screen (UCSICON 3).
        let xf = app.canvas.xf.expect("laid out");
        let place = IconPlace::of(&app, canvas(&app), |p| xf.to_screen(p));
        click(&mut app, &ctx, place.origin + place.x * 40.0, egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "UCS icon menu");
        assert!(app.session.pending_window.is_none(), "the click stayed on the icon");
        let l = Layout::new(canvas(&app));
        click(&mut app, &ctx, l.pill.center(), egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "WCS menu");
        assert!(app.session.pending_window.is_none());
    }
}
