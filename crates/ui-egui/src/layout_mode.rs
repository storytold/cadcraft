//! Layout mode: in the Model tab, a rectangle for the part of the model each layout shows, named
//! after the layout. Clicking one selects it, dragging moves it and dragging a corner resizes it
//! (the viewport keeps its shape), double-clicking opens the layout, and dragging on empty ground
//! adds a layout showing that area. While the mode is on the drawing's own objects are left alone.
//!
//! Positions snap to the drawing's objects with the running object snaps (OSNAP), as any point
//! pick does: a moved rectangle puts its nearest corner (or its centre) on the snap point, a
//! resized one its dragged corner, a new one both of its corners.

use cadcraft_doc::{Handle, Space};
use cadcraft_engine::Session;
use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_geom::Vec2;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::canvas::Xf;

/// One colour for every layout; the name tells them apart.
const COLOR: Color32 = Color32::from_rgb(0x1d, 0x9e, 0xe0);

/// The corner handles are this many points square (and grabbed within this many points).
const HANDLE: f32 = 8.0;

/// A drag shorter than this many points on empty ground adds no layout.
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
    /// The corners (bottom-left first, counter-clockwise) and the centre: the points a moved
    /// rectangle puts on a snap point.
    fn reference_points(&self) -> [Vec2; 5] {
        [self.min, Vec2::new(self.max.x, self.min.y), self.max, Vec2::new(self.min.x, self.max.y), self.center()]
    }
}

/// What a press on a rectangle started.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    /// The body: the area follows the pointer.
    Move,
    /// A corner (0 bottom-left, 1 bottom-right, 2 top-right, 3 top-left): the area grows or
    /// shrinks around the opposite corner.
    Corner(usize),
    /// Empty ground: a window for a new layout.
    New,
}

#[derive(Clone, Debug)]
struct Drag {
    grab: Grab,
    area: Option<Area>,
    /// Where the press was, snapped.
    start: Vec2,
    start_px: Pos2,
    /// For a move: the corner or centre of the area nearest the press, which lands on snap points.
    reference: Vec2,
    /// The area as it looks now, when it is being edited (not yet applied).
    shown: Option<(Vec2, f64)>,
}

/// Layout mode's state.
#[derive(Default)]
pub struct LayoutMode {
    pub on: bool,
    selected: Option<Handle>,
    drag: Option<Drag>,
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

    /// Whether a rectangle is being dragged (the canvas leaves the pointer alone meanwhile).
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
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
    }

    fn at(&self, xf: &Xf, p: Pos2) -> Option<&Area> {
        let w = xf.to_world(p);
        // Of the rectangles under the pointer the smallest wins, so one inside another stays reachable.
        self.areas
            .iter()
            .filter(|a| w.x >= a.min.x && w.x <= a.max.x && w.y >= a.min.y && w.y <= a.max.y)
            .min_by(|a, b| (a.size().x * a.size().y).total_cmp(&(b.size().x * b.size().y)))
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

/// The corner `i` of `rect`'s screen rectangle.
fn corner_px(r: Rect, i: usize) -> Pos2 {
    match i {
        0 => r.left_bottom(),
        1 => r.right_bottom(),
        2 => r.right_top(),
        _ => r.left_top(),
    }
}

/// The new centre and height of `area` after a drag by `delta` (drawing units).
fn dragged(area: &Area, grab: Grab, delta: Vec2, to: Vec2) -> (Vec2, f64) {
    let c = area.center();
    let size = area.size();
    match grab {
        Grab::Corner(i) => {
            // The opposite corner stays; the pointer sets the new size, held to the viewport's shape.
            let anchor = match i {
                0 => Vec2::new(area.max.x, area.max.y),
                1 => Vec2::new(area.min.x, area.max.y),
                2 => Vec2::new(area.min.x, area.min.y),
                _ => Vec2::new(area.max.x, area.min.y),
            };
            let aspect = size.x / size.y.max(1e-12);
            let h = (to.y - anchor.y).abs().max((to.x - anchor.x).abs() / aspect.max(1e-12)).max(1e-9);
            let w = h * aspect;
            let sx = if to.x >= anchor.x { 1.0 } else { -1.0 };
            let sy = if to.y >= anchor.y { 1.0 } else { -1.0 };
            (Vec2::new(anchor.x + sx * w * 0.5, anchor.y + sy * h * 0.5), h)
        }
        _ => (Vec2::new(c.x + delta.x, c.y + delta.y), size.y),
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
    if !app.canvas.layout.active(&app.session) && !app.canvas.layout.dragging() {
        app.canvas.layout.drag = None;
        return false;
    }
    app.canvas.layout.refresh(&mut app.session);
    let (pressed, released, double, hover, origin, esc) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
            i.pointer.interact_pos(),
            i.pointer.press_origin(),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let Some(pos) = hover else { return true };
    if esc {
        app.canvas.layout.drag = None;
        return true;
    }
    if pressed && inside {
        // Where the button went down (the pointer may already have moved on within this frame).
        begin(app, ui, xf, origin.unwrap_or(pos));
    }
    if double && let Some(a) = app.canvas.layout.at(xf, pos).cloned() {
        app.canvas.layout.drag = None;
        let _ = app.run("layout.set", json!({ "name": a.layout }));
        return true;
    }
    let Some(drag) = app.canvas.layout.drag.clone() else { return true };
    let raw_to = xf.to_world(pos);
    // Where the dragged point lands, and the move it makes: a moved rectangle puts its reference
    // point on the snap point, so the delta is from that point; a corner or a window's second corner
    // snaps itself.
    let (to, delta) = match drag.grab {
        Grab::Move => {
            let target = Vec2::new(drag.reference.x + raw_to.x - drag.start.x, drag.reference.y + raw_to.y - drag.start.y);
            let p = snapped(app, ui, xf, target);
            (p, Vec2::new(p.x - drag.reference.x, p.y - drag.reference.y))
        }
        _ => {
            let p = snapped(app, ui, xf, raw_to);
            (p, Vec2::new(p.x - drag.start.x, p.y - drag.start.y))
        }
    };
    if let (Some(a), Grab::Move | Grab::Corner(_)) = (&drag.area, drag.grab) {
        let moved = pos.distance(drag.start_px) >= 1.0;
        if let Some(d) = app.canvas.layout.drag.as_mut() {
            d.shown = moved.then(|| dragged(a, drag.grab, delta, to));
        }
    }
    if released {
        app.canvas.layout.drag = None;
        finish(app, &drag, pos, to, delta);
    }
    true
}

/// A press at `at`: selects the rectangle under it and starts the drag it allows (a move, a corner
/// resize, or a new window on empty ground); a locked viewport only says why it can't change.
fn begin(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, at: Pos2) {
    let lm = &mut app.canvas.layout;
    let selected = lm.selected.and_then(|h| lm.areas.iter().find(|a| a.handle == h)).cloned();
    let corner = selected.as_ref().filter(|a| !a.locked).and_then(|a| {
        let r = a.rect(xf);
        (0..4).find(|&i| corner_px(r, i).distance(at) <= HANDLE).map(|i| (a.clone(), i))
    });
    let raw = xf.to_world(at);
    let under = lm.at(xf, at).cloned();
    if let Some(a) = under.as_ref().filter(|a| a.locked && corner.is_none()) {
        lm.selected = Some(a.handle);
        lm.drag = None;
        let name = a.layout.clone();
        say(app, crate::tf!("The viewport of layout {name} is locked: unlock it to change its area", name = name));
    } else {
        let (grab, area) = match (corner, under) {
            (Some((a, i)), _) => (Grab::Corner(i), Some(a)),
            (None, Some(a)) => {
                lm.selected = Some(a.handle);
                (Grab::Move, Some(a))
            }
            (None, None) => {
                lm.selected = None;
                (Grab::New, None)
            }
        };
        // A new window's first corner snaps; a move keeps the press point and snaps a corner.
        let reference = area
            .as_ref()
            .map(|a| {
                let mut pts = a.reference_points();
                pts.sort_by(|p, q| xf.to_screen(*p).distance(at).total_cmp(&xf.to_screen(*q).distance(at)));
                pts[0]
            })
            .unwrap_or(raw);
        let start = if grab == Grab::New { snapped(app, ui, xf, raw) } else { raw };
        app.canvas.layout.drag = Some(Drag { grab, area, start, start_px: at, reference, shown: None });
    }
}

/// A drag ended at `pos`: apply the edit, or add the layout.
fn finish(app: &mut CadApp, drag: &Drag, pos: Pos2, to: Vec2, delta: Vec2) {
    let far = pos.distance(drag.start_px) >= MIN_DRAG;
    let result = match (&drag.area, drag.grab) {
        (Some(a), Grab::Move | Grab::Corner(_)) if far => {
            let (c, h) = dragged(a, drag.grab, delta, to);
            app.run("layout.setarea", json!({ "handle": a.handle.hex(), "center": [c.x, c.y], "height": h }))
        }
        (None, Grab::New) if far => {
            let w = json!([[drag.start.x, drag.start.y], [to.x, to.y]]);
            app.run("layout.new", json!({ "window": w }))
        }
        _ => return,
    };
    match result {
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
    if !app.canvas.layout.active(&app.session) && !app.canvas.layout.dragging() {
        return;
    }
    app.canvas.layout.refresh(&mut app.session);
    let lm = &app.canvas.layout;
    let clip = painter.clip_rect();
    for a in &lm.areas {
        let selected = lm.selected == Some(a.handle);
        let shown = lm.drag.as_ref().and_then(|d| d.area.as_ref().filter(|x| x.handle == a.handle).and(d.shown));
        let r = match shown {
            Some((c, h)) => {
                let size = a.size();
                let w = h * size.x / size.y.max(1e-12);
                Rect::from_two_pos(xf.to_screen(Vec2::new(c.x - w * 0.5, c.y + h * 0.5)), xf.to_screen(Vec2::new(c.x + w * 0.5, c.y - h * 0.5)))
            }
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
            for i in 0..4 {
                let h = Rect::from_center_size(corner_px(r, i), vec2(HANDLE, HANDLE));
                painter.rect_filled(h, 0.0, Color32::WHITE);
                painter.rect_stroke(h, 0.0, Stroke::new(1.5, COLOR), StrokeKind::Middle);
            }
        }
    }
    if let Some(d) = &lm.drag
        && d.grab == Grab::New
        && let Some(end) = app.canvas.cursor
    {
        let r = Rect::from_two_pos(xf.to_screen(d.start), xf.to_screen(end));
        painter.rect_filled(r, 0.0, COLOR.gamma_multiply(0.1));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, COLOR), StrokeKind::Middle);
        let hint = painter.layout_no_wrap(crate::tl!("New layout").to_string(), FontId::proportional(13.0), COLOR);
        painter.galley(pos2(r.left(), r.top() - hint.size().y - 3.0), hint, COLOR);
    }
    painter.text(
        clip.left_top() + vec2(10.0, 30.0),
        Align2::LEFT_TOP,
        crate::tl!("Layout mode: drag to move, drag a corner to resize, drag on empty ground to add a layout, double-click to open"),
        FontId::proportional(12.0),
        COLOR,
    );
}
