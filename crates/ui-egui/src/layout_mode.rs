//! Layout mode: in the Model tab, a rectangle for the part of the model each layout shows, named
//! after the layout. It works like the grips of any other object: a click selects a rectangle and
//! shows its grips (the four corners stretch it, the middle moves it); a click on a grip takes it, the
//! rectangle follows the pointer, and the next click puts it down (Esc or a right click cancels).
//! Double-clicking a rectangle opens its layout. On empty ground, with nothing selected, a click starts
//! a window and the next click closes it (or press, drag and let go): a new layout showing that area;
//! with a rectangle selected that click only deselects it. While the mode is on the drawing's own
//! objects are left alone.
//!
//! Positions snap to the drawing's objects with the running object snaps (OSNAP), as any point pick
//! does. An area may have any proportion: the viewport takes the area's shape.

use cadcraft_doc::{Handle, Space};
use cadcraft_engine::Session;
use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_geom::Vec2;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// One colour for every layout; the name tells them apart.
const COLOR: Color32 = Color32::from_rgb(0x1d, 0x9e, 0xe0);

/// A grip is taken from this many points away.
const GRAB: f32 = 8.0;

/// A press-and-drag shorter than this many points on empty ground is a click: the window waits for
/// its second corner.
const MIN_DRAG: f32 = 6.0;

/// The model area one layout viewport shows.
#[derive(Clone, Debug, PartialEq)]
struct Area {
    layout: String,
    handle: Handle,
    min: Vec2,
    max: Vec2,
    locked: bool,
}

impl Area {
    fn center(&self) -> Vec2 {
        Vec2::new((self.min.x + self.max.x) * 0.5, (self.min.y + self.max.y) * 0.5)
    }
    fn size(&self) -> Vec2 {
        Vec2::new(self.max.x - self.min.x, self.max.y - self.min.y)
    }
    fn rect(&self, xf: &Xf) -> Rect {
        Rect::from_two_pos(xf.to_screen(Vec2::new(self.min.x, self.max.y)), xf.to_screen(Vec2::new(self.max.x, self.min.y)))
    }
}

/// A grip of a selected rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grip {
    /// A corner (0 bottom-left, 1 bottom-right, 2 top-right, 3 top-left): the area grows or shrinks
    /// around the opposite corner, in any proportion.
    Corner(usize),
    /// The middle: the area moves with it.
    Center,
}

impl Grip {
    const ALL: [Grip; 5] = [Grip::Corner(0), Grip::Corner(1), Grip::Corner(2), Grip::Corner(3), Grip::Center];

    /// Where the grip is on screen.
    fn at(self, r: Rect) -> Pos2 {
        match self {
            Grip::Corner(0) => r.left_bottom(),
            Grip::Corner(1) => r.right_bottom(),
            Grip::Corner(2) => r.right_top(),
            Grip::Corner(_) => r.left_top(),
            Grip::Center => r.center(),
        }
    }
}

/// A grip that was clicked: its rectangle follows the pointer until the next click.
#[derive(Clone, Debug)]
struct Hot {
    grip: Grip,
    area: Area,
    /// The corners the rectangle has now, following the pointer.
    shown: (Vec2, Vec2),
}

/// A new layout's window, started by a click: its first corner (snapped), and whether the button is
/// still down from that press (letting go far away then closes it: press, drag, release).
#[derive(Clone, Copy, Debug)]
struct NewWindow {
    start: Vec2,
    start_px: Pos2,
    held: bool,
}

/// Layout mode's state.
#[derive(Default)]
pub struct LayoutMode {
    pub on: bool,
    selected: Option<Handle>,
    hot: Option<Hot>,
    window: Option<NewWindow>,
    /// The areas, and the drawing revision they were read at.
    areas: Vec<Area>,
    read_at: Option<(u64, u64)>,
}

impl LayoutMode {
    /// Whether the mode is working on the canvas now: on, in the Model tab, with no command asking
    /// for anything.
    pub fn active(&self, s: &Session) -> bool {
        self.on && s.layout_space() == Space::Model && !s.prompting() && s.running.is_none()
    }

    /// Whether a grip is taken or a window started (the canvas leaves the pointer alone meanwhile).
    pub fn busy(&self) -> bool {
        self.hot.is_some() || self.window.is_some()
    }

    fn cancel(&mut self) {
        self.hot = None;
        self.window = None;
    }

    fn refresh(&mut self, s: &mut Session) {
        let key = s.state().map(|st| (st.uid, st.revision)).ok();
        if key.is_some() && self.read_at == key {
            return;
        }
        self.read_at = key;
        self.areas = read_areas(s);
        if self.selected.is_some_and(|h| !self.areas.iter().any(|a| a.handle == h)) {
            self.selected = None;
        }
        if self.hot.as_ref().is_some_and(|h| !self.areas.iter().any(|a| a.handle == h.area.handle)) {
            self.hot = None;
        }
    }

    fn at(&self, xf: &Xf, p: Pos2) -> Option<&Area> {
        let w = xf.to_world(p);
        // Of the rectangles under the pointer the smallest wins, so one inside another stays reachable.
        self.areas
            .iter()
            .filter(|a| w.x >= a.min.x && w.x <= a.max.x && w.y >= a.min.y && w.y <= a.max.y)
            .min_by(|a, b| (a.size().x * a.size().y).total_cmp(&(b.size().x * b.size().y)))
    }

    /// The selected rectangle (not locked) and the grip of it within reach of `p`.
    fn grip_at(&self, xf: &Xf, p: Pos2) -> Option<(Area, Grip)> {
        let a = self.selected.and_then(|h| self.areas.iter().find(|a| a.handle == h)).filter(|a| !a.locked)?;
        let r = a.rect(xf);
        // The corners win over the middle, so a small rectangle can still be stretched.
        Grip::ALL.into_iter().find(|g| g.at(r).distance(p) <= GRAB).map(|g| (a.clone(), g))
    }
}

/// The areas of every layout viewport (`layout.areas`).
fn read_areas(s: &mut Session) -> Vec<Area> {
    let Ok(Value::Array(list)) = s.execute("layout.areas", &json!({})) else { return Vec::new() };
    list.iter()
        .filter_map(|a| {
            let w = a.get("window")?.as_array()?;
            let corner = |i: usize| -> Option<Vec2> {
                let c = w.get(i)?.as_array()?;
                Some(Vec2::new(c.first()?.as_f64()?, c.get(1)?.as_f64()?))
            };
            Some(Area {
                layout: a.get("layout")?.as_str()?.to_string(),
                handle: Handle::parse_hex(a.get("handle")?.as_str()?)?,
                min: corner(0)?,
                max: corner(1)?,
                locked: a.get("locked").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// The corners of `area` with `grip` put at `to`: the middle moves the area, a corner keeps the
/// opposite one and takes any proportion.
fn grip_moved(area: &Area, grip: Grip, to: Vec2) -> (Vec2, Vec2) {
    match grip {
        Grip::Corner(i) => {
            let anchor = match i {
                0 => area.max,
                1 => Vec2::new(area.min.x, area.max.y),
                2 => area.min,
                _ => Vec2::new(area.max.x, area.min.y),
            };
            // An edge that collapses onto the anchor stays a hair wide, so the area keeps an extent.
            let eps = 1e-6 * area.size().x.max(area.size().y).max(1e-9);
            let apart = |a: f64, b: f64| if (b - a).abs() < eps { a + eps } else { b };
            let to = Vec2::new(apart(anchor.x, to.x), apart(anchor.y, to.y));
            (Vec2::new(anchor.x.min(to.x), anchor.y.min(to.y)), Vec2::new(anchor.x.max(to.x), anchor.y.max(to.y)))
        }
        Grip::Center => {
            let c = area.center();
            let d = Vec2::new(to.x - c.x, to.y - c.y);
            (Vec2::new(area.min.x + d.x, area.min.y + d.y), Vec2::new(area.max.x + d.x, area.max.y + d.y))
        }
    }
}

/// `raw` moved onto the drawing's object snaps (and the snap grid), as a point pick would be. The
/// snap marker shows where it landed.
fn snapped(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, raw: Vec2) -> Vec2 {
    let now = ui.input(|i| i.time);
    let q = CursorQuery {
        base: None,
        deferred: false,
        corner: false,
        aperture: app.session.settings.aperture / xf.scale,
        tol: 6.0 / xf.scale,
        now,
        shift: false,
    };
    let r = app.session.snap_cursor(raw, &q);
    app.canvas.snap = r.snap;
    app.canvas.track = r.track;
    app.canvas.cursor = Some(r.point);
    r.point
}

/// Tell the user something in the status bar (and in the command history).
fn say(app: &mut CadApp, message: String) {
    app.session.echo(message.clone());
    app.status = Some((message, crate::now_ms()));
}

/// Handles the pointer over the canvas. Returns whether layout mode took the input, so the
/// canvas does not also select or pick.
pub fn interact(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, inside: bool) -> bool {
    if !app.canvas.layout.active(&app.session) && !app.canvas.layout.busy() {
        app.canvas.layout.cancel();
        return false;
    }
    app.canvas.layout.refresh(&mut app.session);
    let (pressed, released, double, secondary, hover, origin, esc) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
            i.pointer.secondary_pressed(),
            i.pointer.interact_pos(),
            i.pointer.press_origin(),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let Some(pos) = hover else { return true };
    if esc || secondary {
        app.canvas.layout.cancel();
        return true;
    }
    if double && let Some(a) = app.canvas.layout.at(xf, pos).cloned() {
        app.canvas.layout.cancel();
        let _ = app.run("layout.set", json!({ "name": a.layout }));
        return true;
    }
    let raw = xf.to_world(pos);
    // Where a grip being moved, or the window's second corner, is now.
    let target = if app.canvas.layout.busy() { Some(snapped(app, ui, xf, raw)) } else { None };
    if let (Some(h), Some(t)) = (app.canvas.layout.hot.as_mut(), target) {
        h.shown = grip_moved(&h.area, h.grip, t);
    }
    if pressed && inside {
        press(app, ui, xf, origin.unwrap_or(pos), target);
    } else if released
        && let Some(w) = app.canvas.layout.window
        && w.held
    {
        // Press, drag and let go: far from the press it closes the window, near it it was a click.
        match target {
            Some(t) if w.start_px.distance(pos) >= MIN_DRAG => finish_window(app, w, t),
            _ => {
                if let Some(w) = app.canvas.layout.window.as_mut() {
                    w.held = false;
                }
            }
        }
    }
    true
}

/// A press at `at` (the pointer may have moved on within the frame; `target` is where it is now, when
/// something is in progress): puts down a grip or closes a window in progress; otherwise takes a grip,
/// selects a rectangle, deselects, or starts a window.
fn press(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, at: Pos2, target: Option<Vec2>) {
    if let Some(h) = app.canvas.layout.hot.take() {
        // Put the grip down where the pointer is.
        let to = target.unwrap_or_else(|| xf.to_world(at));
        let (lo, hi) = grip_moved(&h.area, h.grip, to);
        let params = match h.grip {
            Grip::Center => {
                let c = Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
                json!({ "handle": h.area.handle.hex(), "center": [c.x, c.y] })
            }
            Grip::Corner(_) => json!({ "handle": h.area.handle.hex(), "window": [[lo.x, lo.y], [hi.x, hi.y]] }),
        };
        match app.run("layout.setarea", params) {
            Ok(_) => app.canvas.layout.read_at = None,
            Err(e) => say(app, e),
        }
        return;
    }
    if let Some(w) = app.canvas.layout.window {
        // The window's second corner.
        finish_window(app, w, target.unwrap_or_else(|| xf.to_world(at)));
        return;
    }
    if let Some((area, grip)) = app.canvas.layout.grip_at(xf, at) {
        let shown = (area.min, area.max);
        app.canvas.layout.hot = Some(Hot { grip, area, shown });
        return;
    }
    let under = app.canvas.layout.at(xf, at).cloned();
    match under {
        Some(a) => {
            app.canvas.layout.selected = Some(a.handle);
            if a.locked {
                say(app, crate::tf!("The viewport of layout {name} is locked: unlock it to change its area", name = a.layout));
            }
        }
        None if app.canvas.layout.selected.is_some() => app.canvas.layout.selected = None,
        None => {
            let start = snapped(app, ui, xf, xf.to_world(at));
            app.canvas.layout.window = Some(NewWindow { start, start_px: at, held: true });
        }
    }
}

/// Close a new layout's window at `end`: a layout that shows that area, in the area's proportion.
fn finish_window(app: &mut CadApp, w: NewWindow, end: Vec2) {
    app.canvas.layout.window = None;
    let window = json!([[w.start.x, w.start.y], [end.x, end.y]]);
    match app.run("layout.new", json!({ "window": window, "adapt": true })) {
        Ok(v) => {
            if let Some(n) = v.get("name").and_then(Value::as_str) {
                app.session.echo(format!("Layout {n} added."));
            }
            app.canvas.layout.read_at = None;
        }
        Err(e) => say(app, e),
    }
}

/// Draws the rectangles over the model.
pub fn paint(app: &mut CadApp, painter: &egui::Painter, xf: &Xf) {
    if !app.canvas.layout.active(&app.session) && !app.canvas.layout.busy() {
        return;
    }
    app.canvas.layout.refresh(&mut app.session);
    let t = Tokens::get();
    let lm = &app.canvas.layout;
    let clip = painter.clip_rect();
    let hover = painter.ctx().input(|i| i.pointer.interact_pos());
    let gs = app.session.settings.gripsize as f32;
    for a in &lm.areas {
        let selected = lm.selected == Some(a.handle);
        let hot = lm.hot.as_ref().filter(|h| h.area.handle == a.handle);
        let r = match hot {
            Some(h) => Rect::from_two_pos(xf.to_screen(Vec2::new(h.shown.0.x, h.shown.1.y)), xf.to_screen(Vec2::new(h.shown.1.x, h.shown.0.y))),
            None => a.rect(xf),
        };
        if !r.intersects(clip) {
            continue;
        }
        // A locked viewport can't be changed: it is drawn fainter, with "locked" after its name.
        let color = if a.locked { COLOR.gamma_multiply(0.55) } else { COLOR };
        painter.rect_filled(r, 0.0, color.gamma_multiply(if selected { 0.16 } else { 0.07 }));
        painter.rect_stroke(r, 0.0, Stroke::new(if selected { 2.5 } else { 1.5 }, color), StrokeKind::Middle);
        let name = if a.locked { format!("{} ({})", a.layout, crate::tl!("locked")) } else { a.layout.clone() };
        let label = painter.layout_no_wrap(name, FontId::proportional(13.0), Color32::WHITE);
        let tag = Rect::from_min_size(r.left_top(), label.size() + vec2(10.0, 4.0));
        painter.rect_filled(tag, 0.0, color);
        painter.galley(tag.min + vec2(5.0, 2.0), label, Color32::WHITE);
        if selected && !a.locked {
            // Grips as on any selected object: cold, warm under the pointer, hot once taken.
            for g in Grip::ALL {
                let at = g.at(r);
                let grip = Rect::from_center_size(at, vec2(gs * 2.0, gs * 2.0));
                let taken = hot.is_some_and(|h| h.grip == g);
                let warm = !taken && lm.hot.is_none() && hover.is_some_and(|p| grip.expand(2.0).contains(p));
                let fill = if taken {
                    t.grip_hot
                } else if warm {
                    Color32::from_rgb(0xff, 0x7f, 0x9f)
                } else {
                    t.grip
                };
                painter.rect_filled(grip, 0.0, fill);
                painter.rect_stroke(grip, 0.0, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80)), StrokeKind::Middle);
            }
        }
    }
    if let (Some(w), Some(end)) = (lm.window, app.canvas.cursor) {
        let r = Rect::from_two_pos(xf.to_screen(w.start), xf.to_screen(end));
        painter.rect_filled(r, 0.0, COLOR.gamma_multiply(0.1));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, COLOR), StrokeKind::Middle);
        let hint = painter.layout_no_wrap(crate::tl!("New layout").to_string(), FontId::proportional(13.0), COLOR);
        painter.galley(pos2(r.left(), r.top() - hint.size().y - 3.0), hint, COLOR);
    }
    painter.text(
        clip.left_top() + vec2(10.0, 30.0),
        Align2::LEFT_TOP,
        crate::tl!("Layout mode: click a rectangle to select it, click a grip and then where to put it, click twice on empty ground to add a layout, double-click to open"),
        FontId::proportional(12.0),
        COLOR,
    );
}
