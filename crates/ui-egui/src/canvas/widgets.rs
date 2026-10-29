use super::draw::draw_list;
use super::grid::draw_grid;
use super::{CadApp, Color32, Pos2, Rect, Rgb, Shape, Stroke, Tokens, Xf, pos2, vec2};

pub(super) fn tile_rects(app: &CadApp, full: Rect) -> Vec<(Rect, cadcraft_engine::ModelTile)> {
    let x = |f: f64| full.left() + full.width() * f.clamp(0.0, 1.0) as f32;
    let y = |f: f64| full.bottom() - full.height() * f.clamp(0.0, 1.0) as f32;
    app.session
        .model_tiles()
        .into_iter()
        .map(|t| (Rect::from_min_max(pos2(x(t.min.x), y(t.max.y)), pos2(x(t.max.x), y(t.min.y))).shrink(1.0), t))
        .collect()
}

/// The world ↔ screen transform of a model tile drawn in `r`.
pub(super) fn tile_xf(r: Rect, tile: &cadcraft_engine::ModelTile) -> Xf {
    Xf { rect: r, center: tile.view.center, scale: f64::from(r.height()) / tile.view.height.max(1e-12) }
}

/// The tiles other than the current one (each with its own view, clipped to it), then the
/// tile borders: thin, and heavy around the current tile.
pub(super) fn draw_tiles(app: &CadApp, ui: &egui::Ui, tiles: &[(Rect, cadcraft_engine::ModelTile)]) {
    let t = Tokens::get();
    let bg = Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b());
    for (r, tile) in tiles.iter().filter(|(r, tile)| !tile.active && r.width() >= 1.0 && r.height() >= 1.0) {
        let p = ui.painter_at(*r);
        let xf = tile_xf(*r, tile);
        p.rect_filled(*r, 0.0, t.canvas);
        draw_grid(app, &p, &xf);
        if let Some(list) = &app.canvas.list {
            draw_list(&p, &xf, list, bg, app.session.settings.lwdisplay);
        }
        viewport_label(&p, *r);
    }
    let p = ui.painter();
    for (r, tile) in tiles {
        let (w, c) = if tile.active { (2.0, t.accent) } else { (1.0, t.border) };
        p.rect_stroke(*r, 0.0, Stroke::new(w, c), egui::StrokeKind::Outside);
    }
}

/// A small padlock, drawn in code: body plus shackle.
pub(super) fn draw_lock(p: &egui::Painter, c: Pos2, color: Color32) {
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, 1.5), vec2(7.0, 5.0)), 1.0, color);
    p.add(Shape::line(
        vec![pos2(c.x - 2.0, c.y - 1.0), pos2(c.x - 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 1.0)],
        Stroke::new(1.2, color),
    ));
}

pub(super) fn draw_ucs_icon(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    let o = pos2(rect.left() + 36.0, rect.bottom() - 34.0);
    let st = Stroke::new(1.0, t.canvas_ink);
    p.line_segment([o, o + vec2(60.0, 0.0)], st);
    p.line_segment([o, o + vec2(0.0, -60.0)], st);
    p.rect_stroke(Rect::from_center_size(o, vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
    let f = egui::FontId::proportional(13.0);
    p.text(o + vec2(70.0, 0.0), egui::Align2::LEFT_CENTER, crate::tl!("X"), f.clone(), t.canvas_ink);
    p.text(o + vec2(0.0, -70.0), egui::Align2::CENTER_BOTTOM, crate::tl!("Y"), f, t.canvas_ink);
}

pub(super) fn viewport_label(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    let at = pos2(rect.left() + 10.0, rect.top() + 8.0);
    p.text(at, egui::Align2::LEFT_TOP, crate::tl!("+  |  Top  |  2D Wireframe"), crate::theme::small(), t.canvas_ink);
}

pub(super) fn draw_viewcube(app: &mut CadApp, ui: &mut egui::Ui, rect: Rect) {
    let l = crate::viewcube::Layout::new(rect);
    let (c, ring) = (l.center, l.ring);
    let p = ui.painter_at(rect);
    crate::viewcube::show(app, ui, &l);
    // WCS pill: the label and a down chevron, drawn as lines rather than a "⌄" glyph that a UI
    // font may lack (it showed as a box, #1541).
    let pill = Rect::from_center_size(c + vec2(0.0, ring + 26.0), vec2(56.0, 16.0));
    p.rect_filled(pill, 8.0, Color32::from_rgb(0x48, 0x50, 0x5c));
    let ink = Tokens::DARK.canvas_ink;
    let label = p.layout_no_wrap(crate::tl!("WCS").to_string(), crate::theme::small(), ink);
    let (gap, chevron) = (4.0, 6.0);
    let left = pill.center().x - (label.size().x + gap + chevron) / 2.0;
    let mid = pill.center().y;
    p.galley(pos2(left, mid - label.size().y / 2.0), label.clone(), ink);
    let x = left + label.size().x + gap;
    p.line(vec![pos2(x, mid - 1.5), pos2(x + chevron / 2.0, mid + 1.5), pos2(x + chevron, mid - 1.5)], Stroke::new(1.2, ink));
}
