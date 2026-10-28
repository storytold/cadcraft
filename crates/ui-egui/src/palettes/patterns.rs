//! The hatch pattern picker: a swatch of every pattern offered for new hatches, beside HATCH's
//! and HATCHEDIT's prompts and in the Properties palette, so a pattern is chosen by its look
//! instead of by typing its name.

use std::cell::RefCell;
use std::sync::Arc;

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, PatternChoice, Picker, pattern_choices};
use egui::{Align2, Rect, Sense, Stroke, pos2, vec2};

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
    let filter_id = egui::Id::unique("hatch_pattern_filter");
    let mut filter: String = ctx.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
    let mut chosen = None;
    let area = ctx.content_rect();
    egui::Window::new(crate::tl!("Hatch Pattern"))
        .id(egui::Id::unique("Hatch Pattern"))
        .collapsible(true)
        .resizable(true)
        .default_size(vec2(5.0 * (CELL + GAP) + 12.0, 5.0 * (CELL + LABEL_H + GAP) + 40.0))
        .default_pos(pos2(area.max.x - 5.0 * (CELL + GAP) - 330.0, area.min.y + 120.0))
        .show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text(crate::tl!("Search for pattern")).desired_width(f32::INFINITY))
                .on_hover_text(crate::tl!("Search for pattern"));
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
