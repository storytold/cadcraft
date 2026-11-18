use cadcraft_geom::{Camera3D, Vec2};

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
    Xf { rect: r, center: tile.view.center, scale: f64::from(r.height()) / tile.view.height.max(1e-12), twist: tile.view.twist, target_z: tile.view.target_z, cam: Camera3D::TOP }
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
        viewport_label(&p, *r, &Camera3D::TOP, false, t.canvas);
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

/// Draw the 3-axis X/Y/Z CADCross / UCS icon, rotating with `cam` (ported from MATCAD `synchronize_cadcross.m`),
/// anchored and clipped inside the active viewport (`vp_clip`) and scaled for `vp_full`.
pub(super) fn draw_ucs_icon(p: &egui::Painter, vp_full: Rect, vp_clip: Rect, cam: &Camera3D) {
    let t = Tokens::get();
    let s = crate::viewcube::ucs_arm_scale(vp_full);
    let p = p.with_clip_rect(vp_clip);
    let o = pos2(vp_clip.left() + 36.0 * s, vp_clip.bottom() - 34.0 * s);
    let f = egui::FontId::proportional((13.0 * s).clamp(8.5, 13.0));
    let box_sz = 9.0 * s;
    if cam.is_top_plan() {
        let st = Stroke::new(1.0, t.canvas_ink);
        let arm = 60.0 * s;
        let lbl = 70.0 * s;
        p.line_segment([o, o + vec2(arm, 0.0)], st);
        p.line_segment([o, o + vec2(0.0, -arm)], st);
        p.rect_stroke(Rect::from_center_size(o, vec2(box_sz, box_sz)), 0.0, st, egui::StrokeKind::Middle);
        p.text(o + vec2(lbl, 0.0), egui::Align2::LEFT_CENTER, crate::tl!("X"), f.clone(), t.canvas_ink);
        p.text(o + vec2(0.0, -lbl), egui::Align2::CENTER_BOTTOM, crate::tl!("Y"), f, t.canvas_ink);
        return;
    }
    // 3D CADCross: X (red), Y (green), Z (blue) projected by the 3D camera.
    let arm_3d = 56.0 * s;
    let lbl_3d = 68.0 * s;
    for (label, axis, col) in super::axis_colors() {
        let uv = cam.project_point(axis, Vec2::ZERO);
        let dir = vec2(uv.x as f32, -(uv.y as f32));
        if dir.length() < 0.08 {
            continue;
        }
        let end = o + dir * arm_3d;
        p.line_segment([o, end], Stroke::new((1.8 * s).clamp(1.1, 1.8), col));
        p.text(o + dir * lbl_3d, egui::Align2::CENTER_CENTER, label, f.clone(), col);
    }
    p.rect_stroke(Rect::from_center_size(o, vec2(8.0 * s, 8.0 * s)), 0.0, Stroke::new(1.0, t.canvas_ink), egui::StrokeKind::Middle);
}

/// Format the top-left viewport label, switching between `2D Wireframe` in plan view and
/// `Wireframe` in 3D or active `3DORBIT` (matching AutoCAD `AcVMTools.crx`).
pub(super) fn viewport_label_text(cam: &Camera3D, orbit_active: bool) -> String {
    let top_label = crate::tl!("+  |  Top  |  2D Wireframe");
    if cam.is_top_plan() && !orbit_active {
        return top_label.to_string();
    }
    let view_name = crate::i18n::t(cam.view_name());
    let style = crate::tl!("Wireframe");
    let mut parts = top_label.splitn(3, "  |  ");
    match (parts.next(), parts.next(), parts.next()) {
        (Some(prefix), Some(_), Some(_)) => format!("{prefix}  |  {view_name}  |  {style}"),
        _ => format!("+  |  {view_name}  |  {style}"),
    }
}

/// Paint the top-left viewport label showing the active view name (`Top`, `SW Isometric`, etc.)
/// and visual style (`2D Wireframe` vs. `Wireframe`), clipped inside `rect`.
pub(super) fn viewport_label(p: &egui::Painter, rect: Rect, cam: &Camera3D, orbit_active: bool, bg: Color32) {
    let ink = if crate::theme::luminance(bg) > 0.5 { Color32::from_rgb(0x47, 0x50, 0x5c) } else { Tokens::DARK.canvas_ink };
    let p = p.with_clip_rect(rect);
    let min_dim = rect.width().min(rect.height());
    let pad_x = (min_dim * 0.06).clamp(4.0, 10.0);
    let pad_y = (min_dim * 0.05).clamp(3.0, 8.0);
    let at = pos2(rect.left() + pad_x, rect.top() + pad_y);
    p.text(at, egui::Align2::LEFT_TOP, viewport_label_text(cam, orbit_active), crate::theme::small(), ink);
}

/// Paint the interactive 3D ViewCube widget and the WCS coordinate-system pill beneath it,
/// scaled to `vp_full` and clipped inside `vp_clip`.
pub(super) fn draw_viewcube(app: &mut CadApp, ui: &mut egui::Ui, vp_full: Rect, vp_clip: Rect, bg: Color32) {
    let l = crate::viewcube::Layout::for_viewport(vp_full, vp_clip);
    crate::viewcube::show_with_bg(app, ui, &l, bg);
    let pal = crate::viewcube::ViewCubePalette::for_background(bg);
    let p = ui.painter_at(l.clip);
    // WCS pill: the label and a down chevron, drawn as lines rather than a "⌄" glyph that a UI
    // font may lack (it showed as a box, #1541).
    let ps = l.scale.clamp(0.55, 1.0);
    let pill = l.pill;
    let radius = 8.0 * ps;
    p.rect_filled(pill, radius, pal.wcs_bg);
    if let Some(border) = pal.wcs_border {
        p.rect_stroke(pill, radius, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    }
    let ink = pal.wcs_ink;
    let mut font = crate::theme::small();
    font.size = (font.size * ps).clamp(7.0, font.size);
    let label = p.layout_no_wrap(crate::tl!("WCS").to_string(), font, ink);
    let (gap, chevron) = (4.0 * ps, 6.0 * ps);
    let left = pill.center().x - (label.size().x + gap + chevron) / 2.0;
    let mid = pill.center().y;
    p.galley(pos2(left, mid - label.size().y / 2.0), label.clone(), ink);
    let x = left + label.size().x + gap;
    p.line(vec![pos2(x, mid - 1.5 * ps), pos2(x + chevron / 2.0, mid + 1.5 * ps), pos2(x + chevron, mid - 1.5 * ps)], Stroke::new((1.2 * ps).clamp(0.8, 1.2), ink));
}

#[cfg(test)]
mod widget_tests {
    use cadcraft_geom::Camera3D;

    #[test]
    fn viewport_label_switches_between_2d_and_3d_wireframe() {
        let top = super::viewport_label_text(&Camera3D::TOP, false);
        assert!(top.contains("2D"), "Top plan label should be 2D Wireframe: {top}");

        let orbiting = super::viewport_label_text(&Camera3D::TOP, true);
        assert!(!orbiting.contains("2D"), "Active 3DORBIT must switch from 2D Wireframe to Wireframe: {orbiting}");
        assert!(orbiting.ends_with(crate::tl!("Wireframe")));

        let iso = super::viewport_label_text(&Camera3D::preset("se_isometric").unwrap(), false);
        assert!(!iso.contains("2D"), "3D view must show Wireframe rather than 2D Wireframe: {iso}");
        assert!(iso.ends_with(crate::tl!("Wireframe")));
    }
}
