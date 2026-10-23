//! The colour drop-down every colour list opens (Ribbon, Properties palette, Quick Properties,
//! Layer Properties Manager), laid out as Windows CAD applications draw it: ByLayer and ByBlock (or
//! None for a background colour), a grid of shades (nine columns from dark to light, the RGB
//! value as a tooltip), the nine standard Index Colors, and More Colors… which opens the full
//! Select Color window (the colour index and true colour). The shades are CADCraft's own,
//! computed from hues.

use cadcraft_color::{Color, Rgb, aci_rgb};
use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};

use crate::theme::Tokens;

/// The grid's columns: grey, then hues in degrees with their saturation (olive is a dull yellow).
const COLUMNS: [(f32, f32); 9] =
    [(0.0, 0.0), (50.0, 0.85), (28.0, 0.85), (0.0, 0.85), (290.0, 0.55), (235.0, 0.65), (210.0, 0.75), (130.0, 0.6), (58.0, 0.55)];

/// The grid's rows, dark to light: (value, saturation factor).
const ROWS: [(f32, f32); 5] = [(0.55, 1.0), (0.72, 0.95), (0.88, 0.85), (0.96, 0.55), (1.0, 0.3)];

fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |f: f32| ((f + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb(to(r), to(g), to(b))
}

/// The shade at `row`, `col` of the grid.
pub fn shade(row: usize, col: usize) -> Rgb {
    let (h, s) = COLUMNS.get(col).copied().unwrap_or((0.0, 0.0));
    let (v, sf) = ROWS.get(row).copied().unwrap_or((1.0, 1.0));
    if s == 0.0 {
        // Black to white.
        let g = (row as f32 / (ROWS.len() - 1) as f32 * 255.0).round() as u8;
        return Rgb(g, g, g);
    }
    hsv(h, s * sf, v)
}

fn c32(c: Color) -> Color32 {
    let r = c.resolve(Color::Index(7), Color::Index(7));
    Color32::from_rgb(r.0, r.1, r.2)
}

/// What a colour list returns: a colour, or None (no background colour) when the list offers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Color(Color),
    None,
}

/// The drop-down under `resp` (as wide as the list box), and the More Colors window while it is
/// open: call it every frame after drawing the list box. `by`: ByLayer and ByBlock first;
/// `none`: the None entry first (a background colour).
pub fn popup(resp: &egui::Response, id: egui::Id, current: Option<Color>, by: bool, none: bool) -> Option<Pick> {
    let t = Tokens::get();
    let mut out = None;
    let more_id = id.with("more");
    egui::Popup::from_toggle_button_response(resp)
        .id(id)
        .width(resp.rect.width().max(180.0))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
            let w = ui.available_width();
            let row = |ui: &mut egui::Ui, label: &str, sw: Option<Color32>, selected: bool| -> bool {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                if resp.hovered() || selected {
                    ui.painter().rect_filled(r, 2.0, if resp.hovered() { t.control_hover } else { t.control });
                }
                let s = Rect::from_min_size(pos2(r.left() + 6.0, r.center().y - 7.0), vec2(14.0, 14.0));
                match sw {
                    Some(c) => {
                        ui.painter().rect_filled(s, 1.0, c);
                        ui.painter().rect_stroke(s, 1.0, Stroke::new(1.0, Color32::from_gray(25)), egui::StrokeKind::Inside);
                    }
                    None => {
                        ui.painter().line_segment([s.left_bottom(), s.right_top()], Stroke::new(1.0, t.text));
                        ui.painter().rect_stroke(s, 1.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
                    }
                }
                ui.painter().text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, crate::theme::body(), t.text);
                resp.clicked()
            };
            if none && row(ui, crate::tl!("None"), None, current.is_none()) {
                out = Some(Pick::None);
            }
            if by {
                if row(ui, crate::tl!("ByLayer"), Some(Color32::WHITE), current == Some(Color::ByLayer)) {
                    out = Some(Pick::Color(Color::ByLayer));
                }
                if row(ui, crate::tl!("ByBlock"), Some(Color32::WHITE), current == Some(Color::ByBlock)) {
                    out = Some(Pick::Color(Color::ByBlock));
                }
            }
            ui.add_space(4.0);
            // The shades: nine columns, dark to light.
            let cell = ((w - 8.0) / COLUMNS.len() as f32).floor().clamp(14.0, 26.0);
            let swatch = |ui: &mut egui::Ui, r: Rect, fill: Color32, selected: bool, tip: String| -> bool {
                let resp = ui.interact(r, ui.id().with(("cl", r.min.x as i32, r.min.y as i32)), Sense::click());
                ui.painter().rect_filled(r.shrink(2.0), 1.0, fill);
                if selected || resp.hovered() {
                    ui.painter().rect_stroke(
                        r.shrink(1.0),
                        1.0,
                        Stroke::new(if selected { 2.0 } else { 1.0 }, Color32::WHITE),
                        egui::StrokeKind::Inside,
                    );
                }
                resp.on_hover_text(tip).clicked()
            };
            for rr in 0..ROWS.len() {
                let (line, _) = ui.allocate_exact_size(vec2(w, cell), Sense::hover());
                for cc in 0..COLUMNS.len() {
                    let rgb = shade(rr, cc);
                    let r = Rect::from_min_size(pos2(line.left() + 4.0 + cc as f32 * cell, line.top()), vec2(cell, cell));
                    let fill = Color32::from_rgb(rgb.0, rgb.1, rgb.2);
                    if swatch(ui, r, fill, current == Some(Color::True(rgb)), format!("{},{},{}", rgb.0, rgb.1, rgb.2)) {
                        out = Some(Pick::Color(Color::True(rgb)));
                    }
                }
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(crate::tl!("Index Colors")).color(t.text_dim));
            let (line, _) = ui.allocate_exact_size(vec2(w, cell), Sense::hover());
            for i in 1..=9u8 {
                let c = aci_rgb(i);
                let r = Rect::from_min_size(pos2(line.left() + 4.0 + f32::from(i - 1) * cell, line.top()), vec2(cell, cell));
                if swatch(ui, r, Color32::from_rgb(c.0, c.1, c.2), current == Some(Color::Index(i)), crate::tf!("Index color: {index}", index = i)) {
                    out = Some(Pick::Color(Color::Index(i)));
                }
            }
            ui.add_space(4.0);
            let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 2.0, t.control_hover);
            }
            // A colour wheel drawn in code.
            let centre = pos2(r.left() + 13.0, r.center().y);
            for k in 0..12 {
                let a0 = k as f32 / 12.0 * std::f32::consts::TAU;
                let rgb = hsv(k as f32 * 30.0, 0.8, 1.0);
                let pts = vec![centre, centre + 7.0 * vec2(a0.cos(), a0.sin()), centre + 7.0 * vec2((a0 + 0.53).cos(), (a0 + 0.53).sin())];
                ui.painter().add(egui::Shape::convex_polygon(pts, Color32::from_rgb(rgb.0, rgb.1, rgb.2), Stroke::NONE));
            }
            ui.painter().text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, crate::tl!("More Colors..."), crate::theme::body(), t.text);
            if resp.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(more_id, true));
            }
            if out.is_some() {
                ui.close();
            }
        });
    // More Colors…: the full Select Color window.
    if resp.ctx.data(|d| d.get_temp::<bool>(more_id)).unwrap_or(false) {
        let mut open = true;
        egui::Window::new(crate::tl!("Select Color")).id(more_id.with("window")).collapsible(false).resizable(false).open(&mut open).show(
            &resp.ctx,
            |ui| {
                if let Some(c) = crate::layers::color_palette(ui, more_id, current.unwrap_or(Color::Index(7)), by) {
                    out = Some(Pick::Color(c));
                }
            },
        );
        if !open || out.is_some() {
            resp.ctx.data_mut(|d| d.remove::<bool>(more_id));
        }
    }
    out
}

/// A colour's swatch colour for list boxes.
pub fn swatch_color(c: Color) -> Color32 {
    c32(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shades_go_from_dark_to_light_in_every_column() {
        let lum = |c: Rgb| 0.299 * f32::from(c.0) + 0.587 * f32::from(c.1) + 0.114 * f32::from(c.2);
        for col in 0..COLUMNS.len() {
            for row in 1..ROWS.len() {
                assert!(lum(shade(row, col)) > lum(shade(row - 1, col)), "column {col}, row {row}");
            }
        }
        assert_eq!(shade(0, 0), Rgb(0, 0, 0));
        assert_eq!(shade(4, 0), Rgb(255, 255, 255));
    }
}
