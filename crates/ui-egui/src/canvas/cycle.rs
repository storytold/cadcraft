use super::grips::grip_at;
use super::{CadApp, Handle, Pos2, Rect, Stroke, Tokens, Vec2, Xf, pos2, vec2};

/// The objects under a click that selection cycling lets the user choose between: where the click
/// was, each object with its row's label (type and layer), Shift held, and the rows as drawn.
#[derive(Clone, Debug)]
pub(super) struct CycleList {
    at: Pos2,
    pub(super) items: Vec<(Handle, String, String)>,
    shift: bool,
    pub(super) rows: Vec<Rect>,
}

impl CycleList {
    pub(super) fn row_at(&self, p: Pos2) -> Option<Handle> {
        self.rows.iter().zip(&self.items).find(|(r, _)| r.contains(p)).map(|(_, (h, _, _))| *h)
    }
}

/// A click while selection cycling's list is open picks the row under it (anywhere else just
/// closes the list); otherwise a click over overlapping objects that a pick would choose between
/// opens the list (SELECTIONCYCLING 2). True when the click was used up here.
pub(super) fn cycle_click(app: &mut CadApp, xf: &Xf, hover: Option<Pos2>, world: Option<Vec2>, shift: bool) -> bool {
    // Only where a click picks objects; a click that places a point goes through.
    let picks = app.session.click_picks();
    if let Some((_, hs, i)) = app.canvas.hover_cycle.take()
        && picks
        && let Some(h) = hs.get(i % hs.len().max(1))
    {
        if let Err(e) = app.session.pick_cycled(*h, shift) {
            app.session.echo(e.to_string());
        }
        return true;
    }
    if let Some(list) = app.canvas.cycle.take() {
        let Some(h) = hover.and_then(|hp| list.row_at(hp)).filter(|_| picks) else { return picks };
        if let Err(e) = app.session.pick_cycled(h, list.shift) {
            app.session.echo(e.to_string());
        }
        return true;
    }
    let (Some(hp), Some(w)) = (hover, world) else { return false };
    if crate::dyngrips::at(app, xf, hover).is_some() || grip_at(app, xf, hover).is_some() {
        return false;
    }
    let hs = app.session.cycling_candidates(w);
    let Ok(d) = app.session.doc() else { return false };
    let items: Vec<_> =
        hs.into_iter().filter_map(|h| d.entity(h).map(|e| (h, crate::i18n::t(e.kind.type_name()).to_string(), e.common.layer.clone()))).collect();
    if items.len() < 2 {
        return false;
    }
    app.canvas.cycle = Some(CycleList { at: hp, items, shift, rows: Vec::new() });
    true
}

/// Shift+Space over overlapping objects highlights the next one under the cursor, and a click
/// picks the highlighted one, whether selection cycling is on or not; moving the cursor off the
/// pick box where it started ends it.
pub(super) fn shift_space_cycle(app: &mut CadApp, ui: &egui::Ui, hover: Option<Pos2>, world: Option<Vec2>) {
    let pick_px = (app.session.settings.pickbox.max(1.0) * 1.5) as f32;
    let (Some(hp), Some(w)) = (hover, world) else {
        app.canvas.hover_cycle = None;
        return;
    };
    if app.canvas.hover_cycle.as_ref().is_some_and(|(a, _, _)| a.distance(hp) > pick_px) {
        app.canvas.hover_cycle = None;
    }
    let pressed = ui
        .input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Space, pressed: true, modifiers, .. } if modifiers.shift)));
    if pressed {
        // With the list open Shift+Space does nothing, and isn't Enter either.
        let next = match app.canvas.hover_cycle.take() {
            _ if app.canvas.cycle.is_some() => None,
            Some((a, hs, i)) => Some((a, hs, i + 1)),
            None => Some((hp, app.session.pick_candidates(w), 1)).filter(|(_, hs, _)| hs.len() > 1),
        };
        if next.is_some() || app.canvas.cycle.is_some() {
            // Space is Enter at the command line; this one only steps the highlight.
            ui.ctx().input_mut(|i| {
                i.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Space, .. }) && !matches!(e, egui::Event::Text(t) if t == " "))
            });
        }
        app.canvas.hover_cycle = next;
    }
    if let Some((_, hs, i)) = &app.canvas.hover_cycle {
        app.canvas.hover = hs.get(i % hs.len().max(1)).copied();
        app.canvas.hover_at = None;
    }
}

/// Selection cycling's badge beside the cursor: two overlapping squares, objects overlap here.
pub(super) fn cycle_badge(p: &egui::Painter, at: Pos2) {
    let t = Tokens::get();
    for off in [vec2(14.0, -22.0), vec2(19.0, -17.0)] {
        let r = Rect::from_min_size(at + off, vec2(9.0, 9.0));
        p.rect_filled(r, 1.0, t.cmd_bg);
        p.rect_stroke(r, 1.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
    }
}

/// Whether `p` lies on a menu the canvas paints itself — the selection cycling list's rows or the
/// Dynamic Input options menu's rows. They are menus, so the OS cursor stays visible over them, but
/// they share the canvas's layer: `layer_id_at` reports the drawing, and only their rectangles can
/// tell them apart from it (#1701).
pub(super) fn over_canvas_menu(app: &CadApp, p: Pos2) -> bool {
    app.canvas.cycle.as_ref().is_some_and(|c| c.rows.iter().any(|r| r.expand(2.0).contains(p)))
        || app.cmd.option_rows.iter().any(|(r, _)| r.expand(2.0).contains(p))
}

/// The selection cycling list beside the click that opened it: a row per object, its type and
/// layer, the row under the pointer highlighted; the rows are kept for clicks.
pub(super) fn cycle_list(app: &mut CadApp, p: &egui::Painter) {
    let Some(list) = app.canvas.cycle.as_mut() else { return };
    let t = Tokens::get();
    let font = crate::theme::small();
    let galleys: Vec<_> = list
        .items
        .iter()
        .map(|(_, ty, layer)| (p.layout_no_wrap(ty.clone(), font.clone(), t.text), p.layout_no_wrap(layer.clone(), font.clone(), t.text_dim)))
        .collect();
    let row_h = galleys.iter().map(|(a, _)| a.size().y).fold(12.0, f32::max) + 6.0;
    let ty_w = galleys.iter().map(|(a, _)| a.size().x).fold(40.0, f32::max);
    let layer_w = galleys.iter().map(|(_, b)| b.size().x).fold(20.0, f32::max);
    let w = ty_w + layer_w + 36.0;
    let m = Rect::from_min_size(list.at + vec2(14.0, 14.0), vec2(w, row_h * galleys.len() as f32 + 4.0));
    p.rect_filled(m, 3.0, t.cmd_bg);
    p.rect_stroke(m, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    let hover = app.canvas.hover;
    list.rows.clear();
    for (i, ((ty, layer), (h, _, _))) in galleys.into_iter().zip(&list.items).enumerate() {
        let row = Rect::from_min_size(pos2(m.left() + 2.0, m.top() + 2.0 + row_h * i as f32), vec2(w - 4.0, row_h));
        if hover == Some(*h) {
            p.rect_filled(row, 2.0, t.cmd_keyword_hover);
        }
        p.galley(pos2(row.left() + 8.0, row.center().y - ty.size().y / 2.0), ty, t.text);
        p.galley(pos2(row.left() + 20.0 + ty_w, row.center().y - layer.size().y / 2.0), layer, t.text_dim);
        list.rows.push(row);
    }
}
