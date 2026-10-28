//! Associative array grips on the canvas: on a selected array, an arrow at the last column, row
//! or item changes how many there are, an arrow at the second one the distance (or angle)
//! between them, and a square at a rectangular array's far corner both counts. Count arrows sit
//! just beyond their item, so they stay apart from the spacing arrows of a two-item array.
//! Clicking one starts the engine's grip edit ([`cadcraft_engine::Session::start_array_grip`]):
//! the array follows the cursor, and a click or a typed value sets it.

use std::sync::Arc;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;
use cadcraft_engine::doc::{ArrayGrip, ArrayGripKind, EntityKind, Handle};

/// Most selected arrays whose grips are shown.
const MAX_ARRAYS: usize = 200;

/// The array grips of the selection, worked out once per drawing revision and selection.
#[derive(Clone, Default)]
pub struct Cache {
    key: (u64, u64, Vec<Handle>),
    grips: Arc<Vec<(Handle, ArrayGrip)>>,
}

/// The selected arrays' grips (cached).
pub(crate) fn of_selection(app: &mut CadApp) -> Arc<Vec<(Handle, ArrayGrip)>> {
    let sel = app.session.selection();
    let Ok(st) = app.session.state() else { return Arc::default() };
    let key = (st.uid, st.revision, sel.clone());
    if app.canvas.array_grips.key != key {
        let grips: Vec<(Handle, ArrayGrip)> = sel
            .iter()
            .filter(|h| matches!(st.doc.entity(**h).map(|e| &e.kind), Some(EntityKind::Insert(_))))
            .filter(|h| st.doc.entity(**h).is_some_and(|e| !st.doc.layer(&e.common.layer).is_some_and(|l| l.locked)))
            .take(MAX_ARRAYS)
            .flat_map(|h| st.doc.array_grips(*h).into_iter().map(move |g| (*h, g)))
            .collect();
        app.canvas.array_grips = Cache { key, grips: Arc::new(grips) };
    }
    app.canvas.array_grips.grips.clone()
}

/// The screen outline of grip `g` (size `gs`, half the side of a square grip).
fn outline(xf: &Xf, g: &ArrayGrip, gs: f32) -> Vec<Pos2> {
    let c = xf.to_screen(g.at);
    let ahead = xf.to_screen(g.at + g.dir) - c;
    if g.kind == ArrayGripKind::RowColumnCount || ahead.length() <= 1e-6 {
        return vec![c + vec2(-gs, -gs), c + vec2(gs, -gs), c + vec2(gs, gs), c + vec2(-gs, gs)];
    }
    // The arrow points the way the count or the distance grows, on screen.
    let u = ahead / ahead.length();
    let n = vec2(-u.y, u.x);
    let c = if g.kind.is_count() { c + u * (gs * 2.6) } else { c };
    let (len, half) = (gs * 1.6, gs * 1.1);
    vec![c - u * (len * 0.5) + n * half, c + u * (len * 0.8), c - u * (len * 0.5) - n * half]
}

/// Whether `p` is on grip `g` (its box, a little larger, as for other grips).
fn hit(xf: &Xf, g: &ArrayGrip, gs: f32, p: Pos2) -> bool {
    Rect::from_points(&outline(xf, g, gs)).expand(3.0).contains(p)
}

/// Draw the grips of the selected arrays; the one under the cursor warm, the one being dragged
/// hot.
pub(crate) fn draw(app: &mut CadApp, painter: &Painter, xf: &Xf, rect: Rect, hover: Option<Pos2>) {
    let t = Tokens::get();
    let gs = app.session.settings.gripsize as f32;
    let hot = app.canvas.array_hot;
    for (h, g) in of_selection(app).iter() {
        if !rect.contains(xf.to_screen(g.at)) {
            continue;
        }
        let pts = outline(xf, g, gs);
        let is_hot = hot.is_some_and(|(hh, k)| hh == *h && k == g.kind);
        let warm = !is_hot && hover.is_some_and(|p| hit(xf, g, gs, p));
        let fill = if is_hot {
            t.grip_hot
        } else if warm {
            Color32::from_rgb(0xff, 0x7f, 0x9f)
        } else {
            t.grip
        };
        painter.add(Shape::convex_polygon(pts, fill, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80))));
    }
}

/// The array grip under `hover`, if any: (array, grip).
pub(crate) fn at(app: &mut CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Handle, ArrayGrip)> {
    let p = hover?;
    let gs = app.session.settings.gripsize as f32;
    of_selection(app).iter().find(|(_, g)| hit(xf, g, gs, p)).copied()
}

/// A click on grip `g` of array `h`: start dragging it.
pub(crate) fn click(app: &mut CadApp, (h, g): (Handle, ArrayGrip)) {
    app.canvas.hot_grips.clear();
    match app.session.start_array_grip(h, g.kind) {
        Ok(()) => app.canvas.array_hot = Some((h, g.kind)),
        Err(e) => app.session.echo(e.to_string()),
    }
}
