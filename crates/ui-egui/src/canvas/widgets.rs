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
    let orbit = app.session.state().map(|s| s.orbit).unwrap_or_default();
    for (r, tile) in tiles.iter().filter(|(r, tile)| !tile.active && r.width() >= 1.0 && r.height() >= 1.0) {
        let p = ui.painter_at(*r);
        let xf = tile_xf(*r, tile);
        p.rect_filled(*r, 0.0, t.canvas);
        draw_grid(app, &p, &xf);
        if let Some(list) = &app.canvas.list {
            draw_list(&p, &xf, list, bg, app.session.settings.lwdisplay);
        }
        viewport_label(&p, *r, orbit);
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

/// Draw the UCS coordinate icon in the bottom-left corner of the viewport (2D XY or 3D RGB triad).
pub(super) fn draw_ucs_icon(p: &egui::Painter, rect: Rect, orbit: cadcraft_render::Orbit3d) {
    let t = Tokens::get();
    let o = pos2(rect.left() + 36.0, rect.bottom() - 34.0);
    let f = egui::FontId::proportional(13.0);
    if orbit.is_top() {
        let st = Stroke::new(1.0, t.canvas_ink);
        p.line_segment([o, o + vec2(60.0, 0.0)], st);
        p.line_segment([o, o + vec2(0.0, -60.0)], st);
        p.rect_stroke(Rect::from_center_size(o, vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
        p.text(o + vec2(70.0, 0.0), egui::Align2::LEFT_CENTER, crate::tl!("X"), f.clone(), t.canvas_ink);
        p.text(o + vec2(0.0, -70.0), egui::Align2::CENTER_BOTTOM, crate::tl!("Y"), f, t.canvas_ink);
        return;
    }
    // 3D UCS Triad: X (Red), Y (Green), Z (Blue).
    let len = 56.0_f64;
    let to_scr = |v: cadcraft_geom::Vec3| -> Pos2 {
        let pr = orbit.project_vec(v);
        pos2(o.x + pr.x as f32, o.y - pr.y as f32)
    };
    let px = to_scr(cadcraft_geom::Vec3::new(len, 0.0, 0.0));
    let py = to_scr(cadcraft_geom::Vec3::new(0.0, len, 0.0));
    let pz = to_scr(cadcraft_geom::Vec3::new(0.0, 0.0, len));
    let col_x = t.ucs_x;
    let col_y = t.ucs_y;
    let col_z = t.ucs_z;
    p.line_segment([o, px], Stroke::new(2.0, col_x));
    p.line_segment([o, py], Stroke::new(2.0, col_y));
    p.line_segment([o, pz], Stroke::new(2.0, col_z));
    p.text(px + (px - o).normalized() * 10.0, egui::Align2::CENTER_CENTER, "X", f.clone(), col_x);
    p.text(py + (py - o).normalized() * 10.0, egui::Align2::CENTER_CENTER, "Y", f.clone(), col_y);
    p.text(pz + (pz - o).normalized() * 10.0, egui::Align2::CENTER_CENTER, "Z", f, col_z);
}

/// Draw the green 3D Orbit arcball overlay and pivot triad while `3DORBIT` is active.
pub(super) fn draw_orbit_arcball(p: &egui::Painter, rect: Rect, orbit: cadcraft_render::Orbit3d) {
    let t = Tokens::get();
    let c = rect.center();
    let r = (rect.width().min(rect.height()) * 0.34).clamp(90.0, 260.0);
    let green = t.orbit_arcball;
    p.circle_stroke(c, r, Stroke::new(1.8, green));
    for d in [vec2(0.0, -r), vec2(0.0, r), vec2(-r, 0.0), vec2(r, 0.0)] {
        p.circle_stroke(c + d, 6.5, Stroke::new(1.4, green));
    }
    // Pivot crosshair + 3D mini triad at center.
    let len = 28.0_f64;
    let to_scr = |v: cadcraft_geom::Vec3| -> Pos2 {
        let pr = orbit.project_vec(v);
        pos2(c.x + pr.x as f32, c.y - pr.y as f32)
    };
    p.line_segment([c, to_scr(cadcraft_geom::Vec3::new(len, 0.0, 0.0))], Stroke::new(1.8, t.ucs_x));
    p.line_segment([c, to_scr(cadcraft_geom::Vec3::new(0.0, len, 0.0))], Stroke::new(1.8, t.ucs_y));
    p.line_segment([c, to_scr(cadcraft_geom::Vec3::new(0.0, 0.0, len))], Stroke::new(1.8, t.ucs_z));
}

/// Draw the top-left viewport controls label showing current view orientation and visual style.
pub(super) fn viewport_label(p: &egui::Painter, rect: Rect, orbit: cadcraft_render::Orbit3d) {
    let t = Tokens::get();
    let at = pos2(rect.left() + 10.0, rect.top() + 8.0);
    if orbit.is_top() {
        p.text(at, egui::Align2::LEFT_TOP, crate::tl!("+  |  Top  |  2D Wireframe"), crate::theme::small(), t.canvas_ink);
    } else {
        let txt = crate::tf!(
            "+  |  {view} ({yaw}°, {pitch}°)  |  3D Orbit Wireframe",
            view = crate::i18n::t(orbit.label()),
            yaw = format!("{:.0}", orbit.yaw.to_degrees().rem_euclid(360.0)),
            pitch = format!("{:.0}", orbit.pitch.to_degrees())
        );
        p.text(at, egui::Align2::LEFT_TOP, txt, crate::theme::small(), t.canvas_ink);
    }
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
