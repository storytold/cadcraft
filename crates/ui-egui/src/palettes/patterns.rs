//! The hatch pattern picker: a swatch of every pattern offered for new hatches, beside HATCH's
//! and HATCHEDIT's prompts and in the Properties palette, so a pattern is chosen by its look
//! instead of by typing its name.

use std::cell::RefCell;
use std::sync::Arc;

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, PatternChoice, Picker, pattern_choices};
use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// A swatch's side, in points.
const CELL: f32 = 56.0;
/// Room under a swatch for the pattern's name.
const LABEL_H: f32 = 16.0;
/// Space between swatches.
const GAP: f32 = 6.0;
/// Swatches per row in the Properties palette's menu.
const MENU_COLUMNS: f32 = 6.0;

/// A pattern as the picker draws it: its runs in the unit square (y up), or a solid fill.
struct Swatch {
    name: String,
    description: String,
    runs: Vec<Vec<Vec2>>,
    solid: bool,
}

/// The patterns' names and line family counts, and their swatches.
type Swatches = (Vec<(String, usize)>, Arc<Vec<Swatch>>);

thread_local! {
    static SWATCHES: RefCell<Swatches> = RefCell::new((Vec::new(), Arc::new(Vec::new())));
}

/// The swatches of the patterns offered now, made again only when the list changes (a `.pat`
/// file loaded more patterns).
fn swatches() -> Arc<Vec<Swatch>> {
    let choices = pattern_choices();
    let key: Vec<(String, usize)> = choices.iter().map(|c| (c.name.clone(), c.lines.len())).collect();
    SWATCHES.with(|cell| {
        let mut cache = cell.borrow_mut();
        if cache.0 != key {
            *cache = (key, Arc::new(choices.into_iter().map(swatch).collect()));
        }
        cache.1.clone()
    })
}

fn swatch(c: PatternChoice) -> Swatch {
    let solid = c.lines.is_empty();
    Swatch { runs: cadcraft_engine::render::pattern_swatch(&c.lines, 1.0), name: c.name, description: c.description, solid }
}

/// Draw one swatch in `r` (the square and the name under it).
fn paint(p: &egui::Painter, r: Rect, s: &Swatch, selected: bool, hovered: bool) {
    let t = Tokens::get();
    let square = Rect::from_min_size(r.min, vec2(CELL, CELL));
    p.rect_filled(square, 3.0, t.field);
    let inner = square.shrink(3.0);
    if s.solid {
        p.rect_filled(inner, 1.0, t.text);
    } else {
        let at = |v: &Vec2| pos2(inner.min.x + v.x as f32 * inner.width(), inner.max.y - v.y as f32 * inner.height());
        let stroke = Stroke::new(1.0, t.text);
        for run in &s.runs {
            match run.as_slice() {
                [] => {}
                [dot] => {
                    p.circle_filled(at(dot), 0.7, t.text);
                }
                pts => {
                    p.add(egui::Shape::line(pts.iter().map(at).collect(), stroke));
                }
            }
        }
    }
    let border = match (selected, hovered) {
        (true, _) => Stroke::new(2.0, t.accent),
        (false, true) => Stroke::new(1.0, t.accent),
        _ => Stroke::new(1.0, t.border),
    };
    p.rect_stroke(square, 3.0, border, egui::StrokeKind::Inside);
    let label = Rect::from_min_max(pos2(r.min.x, square.max.y), r.max);
    let color = if selected { t.accent } else { t.text_dim };
    p.with_clip_rect(label).text(label.center_top() + vec2(0.0, 2.0), Align2::CENTER_TOP, &s.name, crate::theme::small(), color);
}

/// The swatches whose name or description holds `filter`, `current` marked; returns the
/// pattern clicked.
pub fn grid(ui: &mut egui::Ui, current: &str, filter: &str) -> Option<String> {
    let filter = filter.trim().to_lowercase();
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        let shown = swatches();
        for s in
            shown.iter().filter(|s| filter.is_empty() || s.name.to_lowercase().contains(&filter) || s.description.to_lowercase().contains(&filter))
        {
            let (r, resp) = ui.allocate_exact_size(vec2(CELL, CELL + LABEL_H), Sense::click());
            if ui.is_rect_visible(r) {
                paint(ui.painter(), r, s, s.name.eq_ignore_ascii_case(current), resp.hovered());
            }
            let tip = if s.description.is_empty() { s.name.clone() } else { format!("{}\n{}", s.name, s.description) };
            if resp.on_hover_text(tip).clicked() {
                chosen = Some(s.name.clone());
            }
        }
    });
    chosen
}

/// The Properties palette's pattern menu: the swatch grid, scrolling; returns the pattern
/// clicked.
pub fn menu(ui: &mut egui::Ui, current: &str) -> Option<String> {
    ui.set_width(MENU_COLUMNS * (CELL + GAP));
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| grid(ui, current, "")).inner
}

/// The picker window while the prompt offers one (HATCH's boundary and pattern prompts,
/// HATCHEDIT's pattern prompt): a click enters the pattern's name at the prompt.
pub fn prompt_picker(app: &mut CadApp, ctx: &egui::Context) {
    let Some(Picker::HatchPattern(current)) = app.session.current_prompt().and_then(|p| p.picker) else { return };
    // With the Ribbon, the Hatch Creation / Hatch Editor tab's gallery is the picker.
    if app.ui.show_ribbon {
        return;
    }
    let filter_id = egui::Id::new("hatch_pattern_filter");
    let mut filter: String = ctx.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
    let mut chosen = None;
    let area = ctx.content_rect();
    egui::Window::new(crate::tl!("Hatch Pattern"))
        .id(egui::Id::new("Hatch Pattern"))
        .collapsible(true)
        .resizable(true)
        .default_size(vec2(5.0 * (CELL + GAP) + 12.0, 5.0 * (CELL + LABEL_H + GAP) + 40.0))
        .default_pos(pos2(area.max.x - 5.0 * (CELL + GAP) - 330.0, area.min.y + 120.0))
        .show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text(crate::tl!("Search for pattern")).desired_width(f32::INFINITY));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                chosen = grid(ui, &current, &filter);
            });
        });
    ctx.data_mut(|d| d.insert_temp(filter_id, filter));
    if let Some(name) = chosen {
        app.cmd.buffer.clear();
        if let Err(e) = app.session.input(Input::Text(name)) {
            app.session.echo(e.to_string());
        }
    }
}

/// The gradient fills as the Hatch Editor's gallery names them, with the engine's names.
pub const GRADIENTS: [(&str, &str); 9] = [
    ("GR_LINEAR", "LINEAR"),
    ("GR_CYLIN", "CYLINDER"),
    ("GR_INVCYL", "INVCYLINDER"),
    ("GR_SPHER", "SPHERICAL"),
    ("GR_HEMISP", "HEMISPHERICAL"),
    ("GR_CURVED", "CURVED"),
    ("GR_INVSPH", "INVSPHERICAL"),
    ("GR_INVHEM", "INVHEMISPHERICAL"),
    ("GR_INVCUR", "INVCURVED"),
];

/// The engine's gradient name of a gallery tile (`GR_LINEAR` → `LINEAR`).
pub fn gradient_of(tile: &str) -> Option<&'static str> {
    GRADIENTS.iter().find(|(t, _)| t.eq_ignore_ascii_case(tile)).map(|(_, g)| *g)
}

/// The gallery tile of a gradient fill (`LINEAR` → `GR_LINEAR`).
pub fn tile_of_gradient(name: &str) -> Option<&'static str> {
    GRADIENTS.iter().find(|(_, g)| g.eq_ignore_ascii_case(name)).map(|(t, _)| *t)
}

/// The Hatch Editor gallery's tiles in order: SOLID first, then the patterns by name with the
/// gradients in their place among them (after the G… patterns that sort before `GR_`).
pub fn gallery() -> Vec<String> {
    // The usual CAD order: `_` sorts before letters (GOST_WOOD, GR_LINEAR, GRASS).
    let key = |n: &str| n.to_ascii_uppercase().replace('_', " ");
    let mut names: Vec<String> = swatches().iter().map(|s| s.name.clone()).filter(|n| n != "SOLID").collect();
    names.sort_by_key(|n| key(n));
    let at = names.iter().position(|n| key(n) > key("GR_")).unwrap_or(names.len());
    let mut out = vec!["SOLID".to_string()];
    out.extend(names.iter().take(at).cloned());
    out.extend(GRADIENTS.iter().map(|(t, _)| (*t).to_string()));
    out.extend(names.iter().skip(at).cloned());
    out
}

/// One gallery tile in `r`: the pattern or gradient in a square, its name under it.
pub fn paint_tile(p: &egui::Painter, r: Rect, name: &str, selected: bool, hovered: bool) {
    let t = Tokens::get();
    if selected || hovered {
        p.rect_filled(r, 2.0, if selected { t.control_hover } else { t.control_hover.gamma_multiply(0.6) });
    }
    let side = (r.width() - 8.0).min(r.height() - LABEL_H - 4.0).max(4.0);
    let square = Rect::from_center_size(pos2(r.center().x, r.top() + 3.0 + side / 2.0), vec2(side, side));
    let inner = square.shrink(1.0);
    if let Some(g) = gradient_of(name) {
        gradient_mesh(p, inner, g);
    } else if let Some(s) = swatches().iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
        p.rect_filled(inner, 0.0, Color32::WHITE);
        if s.solid {
            p.rect_filled(inner, 0.0, Color32::BLACK);
        } else {
            let at = |v: &Vec2| pos2(inner.min.x + v.x as f32 * inner.width(), inner.max.y - v.y as f32 * inner.height());
            // One physical pixel, as a line of global width 0 in the drawing.
            let px = 1.0 / p.ctx().pixels_per_point();
            let stroke = Stroke::new(px, Color32::BLACK);
            for run in &s.runs {
                match run.as_slice() {
                    [] => {}
                    [dot] => {
                        p.rect_filled(Rect::from_center_size(at(dot), vec2(px, px)), 0.0, Color32::BLACK);
                    }
                    pts => {
                        p.add(egui::Shape::line(pts.iter().map(at).collect(), stroke));
                    }
                }
            }
        }
    }
    p.rect_stroke(square, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let label = Rect::from_min_max(pos2(r.min.x, square.max.y + 1.0), r.max);
    let shown = crate::i18n::ribbon(name);
    p.with_clip_rect(label).text(label.center_top() + vec2(0.0, 1.0), Align2::CENTER_TOP, shown, egui::FontId::proportional(10.5), t.text);
}

/// A gradient's preview, black to white, in the shape the renderer draws it.
fn gradient_mesh(p: &egui::Painter, r: Rect, name: &str) {
    const N: usize = 16;
    let inv = name.starts_with("INV");
    let base = name.trim_start_matches("INV");
    let shade = |u: f32, v: f32| -> f32 {
        let x = match base {
            "CYLINDER" => 1.0 - (2.0 * u - 1.0).abs(),
            "SPHERICAL" => 1.0 - (((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0).min(1.0),
            "HEMISPHERICAL" => 1.0 - (((u - 0.3).powi(2) + (v - 0.15).powi(2)).sqrt() * 1.3).min(1.0),
            "CURVED" => 1.0 - v * v,
            _ => u,
        };
        if inv { 1.0 - x } else { x }
    };
    let mut mesh = egui::Mesh::default();
    for j in 0..=N {
        for i in 0..=N {
            let (u, v) = (i as f32 / N as f32, j as f32 / N as f32);
            let g = (shade(u, v).clamp(0.0, 1.0) * 255.0) as u8;
            mesh.colored_vertex(pos2(r.min.x + u * r.width(), r.min.y + v * r.height()), Color32::from_gray(g));
        }
    }
    let w = (N + 1) as u32;
    for j in 0..N as u32 {
        for i in 0..N as u32 {
            let k = j * w + i;
            mesh.add_triangle(k, k + 1, k + w);
            mesh.add_triangle(k + 1, k + w + 1, k + w);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}
