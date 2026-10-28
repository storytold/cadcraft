//! Dynamic block grips on the canvas, as AutoCAD shows them on a selected dynamic block
//! reference: an arrow at a distance's end (drag it to change the distance), a square at a
//! position (drag it to move it) and a down-pointing triangle for a list (click it for the
//! visibility states or lookup values). Dragging runs the engine's dynamic grip edit
//! ([`cadcraft_engine::Session::start_dyn_grip`]); a list choice runs `blocks.reference.set`.

use std::sync::Arc;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};
use serde_json::json;

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;
use cadcraft_engine::doc::Handle;
use cadcraft_engine::doc::dynblock::{DynGrip, GripShape};

/// Most selected references whose grips are shown.
const MAX_REFERENCES: usize = 200;

/// The dynamic grips of the selection, worked out once per drawing revision and selection.
#[derive(Clone, Default)]
pub struct Cache {
    key: (u64, u64, Vec<Handle>),
    grips: Arc<Vec<(Handle, DynGrip)>>,
}

/// The selected references' dynamic grips (cached).
pub(crate) fn of_selection(app: &mut CadApp) -> Arc<Vec<(Handle, DynGrip)>> {
    let sel = app.session.selection();
    let Ok(st) = app.session.state() else { return Arc::default() };
    let key = (st.uid, st.revision, sel.clone());
    if app.canvas.dyn_grips.key != key {
        let grips: Vec<(Handle, DynGrip)> = sel
            .iter()
            .filter(|h| matches!(st.doc.entity(**h).map(|e| &e.kind), Some(cadcraft_engine::doc::EntityKind::Insert(_))))
            .filter(|h| st.doc.entity(**h).is_some_and(|e| !st.doc.layer(&e.common.layer).is_some_and(|l| l.locked)))
            .take(MAX_REFERENCES)
            .flat_map(|h| cadcraft_engine::doc::dynblock::grips(&st.doc, *h).into_iter().map(move |g| (*h, g)))
            .collect();
        app.canvas.dyn_grips = Cache { key, grips: Arc::new(grips) };
    }
    app.canvas.dyn_grips.grips.clone()
}

/// The screen outline of grip `g` (size `gs`, half the side of a square grip).
fn outline(xf: &Xf, g: &DynGrip, gs: f32) -> Vec<Pos2> {
    let c = xf.to_screen(g.at);
    match g.shape {
        GripShape::Linear => {
            // The arrow points the way the distance grows, on screen.
            let ahead = xf.to_screen(g.at + g.dir) - c;
            let u = if ahead.length() > 1e-6 { ahead / ahead.length() } else { vec2(1.0, 0.0) };
            let n = vec2(-u.y, u.x);
            let (len, half) = (gs * 1.6, gs * 1.1);
            vec![c - u * (len * 0.5) + n * half, c + u * (len * 0.8), c - u * (len * 0.5) - n * half]
        }
        GripShape::Point => {
            vec![c + vec2(-gs, -gs), c + vec2(gs, -gs), c + vec2(gs, gs), c + vec2(-gs, gs)]
        }
        GripShape::List => {
            let w = gs * 1.3;
            vec![c + vec2(-w, -w * 0.6), c + vec2(w, -w * 0.6), c + vec2(0.0, w * 0.9)]
        }
    }
}

/// Whether `p` is on grip `g` (its box, a little larger, as for other grips).
fn hit(xf: &Xf, g: &DynGrip, gs: f32, p: Pos2) -> bool {
    let r = Rect::from_points(&outline(xf, g, gs)).expand(3.0);
    r.contains(p)
}

/// Draw the grips of the selected dynamic references; the one under the cursor warm, the one
/// being dragged hot.
pub(crate) fn draw(app: &mut CadApp, painter: &Painter, xf: &Xf, rect: Rect, hover: Option<Pos2>) {
    let t = Tokens::get();
    let gs = app.session.settings.gripsize as f32;
    let hot = app.canvas.dyn_hot;
    for (h, g) in of_selection(app).iter() {
        if !rect.contains(xf.to_screen(g.at)) {
            continue;
        }
        let pts = outline(xf, g, gs);
        let is_hot = hot.as_ref().is_some_and(|(hh, n, p)| hh == h && *n == g.node && *p == g.part);
        let warm = !is_hot && hover.is_some_and(|p| hit(xf, g, gs, p));
        let fill = if is_hot {
            t.grip_hot
        } else if warm {
            Color32::from_rgb(0xff, 0x7f, 0x9f)
        } else {
            t.grip_dyn
        };
        painter.add(Shape::convex_polygon(pts, fill, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80))));
    }
}

/// The dynamic grip under `hover`, if any: (reference, grip).
pub(crate) fn at(app: &mut CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Handle, DynGrip)> {
    let p = hover?;
    let gs = app.session.settings.gripsize as f32;
    of_selection(app).iter().find(|(_, g)| hit(xf, g, gs, p)).cloned()
}

/// A click on dynamic grip `g` of reference `h`: drag it, or open its list.
pub(crate) fn click(app: &mut CadApp, (h, g): (Handle, DynGrip)) {
    if g.shape == GripShape::List {
        app.canvas.dyn_menu = Some((h, g));
        app.canvas.dyn_menu_fresh = true;
        return;
    }
    app.canvas.hot_grips.clear();
    match app.session.start_dyn_grip(h, g.node, g.part) {
        Ok(()) => app.canvas.dyn_hot = Some((h, g.node, g.part)),
        Err(e) => app.session.echo(e.to_string()),
    }
}

/// The open list of a list grip: its choices under the grip, the current one checked. A choice
/// sets the property (one undo step); a click elsewhere closes it.
pub(crate) fn menu(app: &mut CadApp, ctx: &egui::Context, xf: &Xf) {
    let Some((h, g)) = app.canvas.dyn_menu.clone() else { return };
    let current = app
        .session
        .state()
        .ok()
        .and_then(|st| cadcraft_engine::doc::dynblock::custom_properties(&st.doc, h))
        .and_then(|(_, ps)| ps.into_iter().find(|p| p.name.eq_ignore_ascii_case(&g.property)))
        .map(|p| p.value)
        .unwrap_or_default();
    let gs = app.session.settings.gripsize as f32;
    let at = xf.to_screen(g.at) + vec2(-gs, gs * 1.5);
    let mut chosen: Option<String> = None;
    let area = egui::Area::new(egui::Id::unique(("dyn-grip-list", h, g.node))).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            for c in &g.choices {
                if ui.selectable_label(c.eq_ignore_ascii_case(&current), c.as_str()).clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
    });
    // The click that opened the list isn't a click elsewhere.
    let fresh = std::mem::take(&mut app.canvas.dyn_menu_fresh);
    let clicked_elsewhere = !fresh && ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer();
    if let Some(value) = chosen {
        app.canvas.dyn_menu = None;
        if let Err(e) = app.run("blocks.reference.set", json!({ "handle": h.hex(), "name": g.property, "value": value })) {
            app.session.echo(e);
        }
    } else if clicked_elsewhere || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.canvas.dyn_menu = None;
    }
}
