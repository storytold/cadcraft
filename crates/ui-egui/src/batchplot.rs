//! Batch Plot: a print manager. The sheets to plot in a list that can be reordered (drag, or the
//! Move buttons), with sheets repeated or removed; sheets of any open drawing can be added; the
//! result is one PDF with every sheet or one PDF per sheet (PUBLISH with `sheets`).

use egui::{Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// One sheet in the list: the drawing it belongs to (its title) and the layout (or `Model`).
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub drawing: String,
    pub layout: String,
}

/// The window's state between frames.
#[derive(Clone, Default)]
struct State {
    sheets: Vec<Sheet>,
    /// The drawing the list was filled from when the window opened.
    filled_for: Option<String>,
    /// The picked rows (Ctrl and Shift pick several).
    selected: Vec<usize>,
    /// Where Shift+click extends from.
    anchor: Option<usize>,
    /// The picked rows are being dragged.
    dragging: bool,
    /// Where a selection rectangle started (in the list's empty space).
    band_from: Option<egui::Pos2>,
    /// The drawing whose sheets the Add section shows.
    add_from: Option<String>,
    separate: bool,
    note: Option<String>,
}

fn state_id() -> egui::Id {
    egui::Id::new("cc_batch_plot")
}

/// The sheets of drawing `d` in tab order: Model when it has objects, then the layouts.
pub fn sheets_of(d: &cadcraft_doc::Drawing, title: &str) -> Vec<Sheet> {
    let mut layouts: Vec<_> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    let model = (!d.model.is_empty()).then(|| "Model".to_string());
    model.into_iter().chain(layouts.iter().map(|l| l.name.clone())).map(|layout| Sheet { drawing: title.to_string(), layout }).collect()
}

/// Move the sheet at `from` to stand before `slot` (0..=len).
pub fn move_sheet(v: &mut Vec<Sheet>, from: usize, slot: usize) -> Option<usize> {
    if from >= v.len() {
        return None;
    }
    let s = v.remove(from);
    let to = if slot > from { slot - 1 } else { slot }.min(v.len());
    v.insert(to, s);
    Some(to)
}

/// The PUBLISH parameters for `sheets` (drawing titles resolved to open drawings).
pub fn publish_params(app: &CadApp, sheets: &[Sheet], combine: bool, path: Option<&str>) -> Result<Value, String> {
    let list: Vec<Value> = sheets
        .iter()
        .map(|s| {
            let i = app
                .session
                .docs
                .iter()
                .position(|d| d.title == s.drawing)
                .ok_or_else(|| format!("{}: {}", crate::tl!("The drawing is closed"), s.drawing))?;
            Ok(json!({ "drawing": i, "layout": s.layout }))
        })
        .collect::<Result<_, String>>()?;
    let mut p = json!({ "sheets": list, "combine": combine });
    if let Some(path) = path {
        p["path"] = json!(path);
    }
    Ok(p)
}

/// Move the sheets at `picked` (any order) as one block to stand before `slot` (0..=len), keeping
/// their order; the indices they end up at.
pub fn move_sheets(v: &mut Vec<Sheet>, picked: &[usize], slot: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = picked.iter().copied().filter(|i| *i < v.len()).collect();
    idx.sort_unstable();
    idx.dedup();
    if idx.is_empty() {
        return Vec::new();
    }
    let before = idx.iter().filter(|i| **i < slot).count();
    let moved: Vec<Sheet> = idx.iter().filter_map(|i| v.get(*i).cloned()).collect();
    for i in idx.iter().rev() {
        v.remove(*i);
    }
    let at = slot.saturating_sub(before).min(v.len());
    for (k, sh) in moved.into_iter().enumerate() {
        v.insert(at + k, sh);
    }
    (at..at + idx.len()).collect()
}

/// What a click asked for, applied once the list is drawn.
enum Act {
    /// A click on row `i`: with Ctrl it toggles, with Shift it extends from the anchor.
    Click(usize, egui::Modifiers),
    /// A rubber-band rectangle picked these rows (added to the selection with Ctrl).
    Band(Vec<usize>, bool),
    SelectAll,
    /// The selected rows to stand before this slot.
    MoveTo(usize),
    Up,
    Down,
    Duplicate,
    Remove,
    Add(Sheet),
    AddAll(String),
    Clear,
    Plot,
}

/// File ▸ Batch Plot: the print manager window. While it is open the rest of the program waits.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let current = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    // Opened for a drawing: its sheets, in tab order (others are added by hand).
    if st.filled_for.as_deref() != Some(current.as_str()) {
        st.sheets = app.session.doc().map(|d| sheets_of(d, &current)).unwrap_or_default();
        st.filled_for = Some(current.clone());
        st.selected.clear();
        st.add_from = Some(current.clone());
        st.note = None;
    }
    st.selected.retain(|i| *i < st.sheets.len());
    let titles: Vec<String> = app.session.docs.iter().map(|d| d.title.clone()).collect();
    let paper = |s: &Sheet| -> String {
        let Some(d) = app.session.docs.iter().find(|d| d.title == s.drawing) else { return crate::tl!("closed").to_string() };
        if s.layout == "Model" {
            return String::new();
        }
        d.doc.layout(&s.layout).map(|l| l.page.paper.clone()).unwrap_or_default()
    };
    let rows: Vec<(String, String, String)> = st.sheets.iter().map(|s| (s.layout.clone(), s.drawing.clone(), paper(s))).collect();
    let can_plot = app.services.pick_save.is_some();
    let mut act: Option<Act> = None;
    let mut close = false;
    // Modal: a dimmed backdrop over the whole program takes every click outside the window.
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("cc_batch_plot_backdrop")).order(egui::Order::Middle).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(110));
    });
    egui::Window::new(crate::tl!("Batch Plot"))
        .id(egui::Id::new("cc_batch_plot_window"))
        .order(egui::Order::Foreground)
        .open(open)
        .default_size(vec2(700.0, 520.0))
        .min_size(vec2(520.0, 380.0))
        .resizable(true)
        .show(ctx, |ui| {
            let t = Tokens::get();
            ui.label(RichText::new(crate::tl!("Sheets to plot, in order")).strong());
            ui.horizontal_top(|ui| {
                let list_w = (ui.available_width() - 190.0).max(260.0);
                let list_h = (ui.available_height() - 190.0).max(140.0);
                ui.allocate_ui_with_layout(vec2(list_w, list_h), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(2.0).show(ui, |ui| {
                        let w = list_w - 10.0;
                        // Columns: number, sheet, drawing, paper, with a separator between each.
                        let cols = [34.0_f32, (w - 34.0) * 0.36, (w - 34.0) * 0.34, (w - 34.0) * 0.30];
                        let col_x = |k: usize| cols.iter().take(k).sum::<f32>();
                        let (hr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
                        ui.painter().rect_filled(hr, 0.0, t.chrome_dark);
                        for (k, title) in ["#", crate::tl!("Sheet"), crate::tl!("Drawing"), crate::tl!("Paper")].into_iter().enumerate() {
                            let cell = Rect::from_min_size(pos2(hr.left() + col_x(k), hr.top()), vec2(cols[k], hr.height()));
                            ui.painter().with_clip_rect(cell.shrink2(vec2(3.0, 0.0))).text(
                                pos2(cell.left() + 5.0, cell.center().y),
                                egui::Align2::LEFT_CENTER,
                                title,
                                crate::theme::small(),
                                t.text_dim,
                            );
                            if k > 0 {
                                ui.painter().vline(cell.left(), hr.y_range(), Stroke::new(1.0, t.border));
                            }
                        }
                        let body_h = list_h - 30.0;
                        egui::ScrollArea::vertical().id_salt("cc_batch_list").auto_shrink([false, false]).max_height(body_h).show(ui, |ui| {
                            ui.set_width(w);
                            let mut rects: Vec<Rect> = Vec::new();
                            for (i, (layout, drawing, paper)) in rows.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click_and_drag());
                                rects.push(r);
                                let on = st.selected.contains(&i);
                                if on {
                                    ui.painter().rect_filled(r, 2.0, t.accent.gamma_multiply(0.35));
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(r, 2.0, t.control_hover.gamma_multiply(0.6));
                                }
                                let y = r.center().y;
                                let cell = |k: usize| Rect::from_min_size(pos2(r.left() + col_x(k), r.top()), vec2(cols[k], r.height()));
                                // A grip that shows rows can be dragged, and the number.
                                let c0 = cell(0);
                                for dy in [-4.0, 0.0, 4.0] {
                                    ui.painter().hline((c0.left() + 3.0)..=(c0.left() + 9.0), y + dy, Stroke::new(1.0, t.text_faint));
                                }
                                ui.painter().with_clip_rect(c0).text(
                                    pos2(c0.left() + 12.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    format!("{}", i + 1),
                                    crate::theme::small(),
                                    t.text_dim,
                                );
                                let c1 = cell(1);
                                ui.painter().with_clip_rect(c1.shrink2(vec2(3.0, 0.0))).text(
                                    pos2(c1.left() + 5.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    crate::i18n::t(layout),
                                    crate::theme::body(),
                                    t.text,
                                );
                                // Hebrew names read right to left, as in the file manager.
                                let c2 = cell(2);
                                let job = crate::i18n::job(drawing, crate::theme::small(), t.text_dim);
                                let g = ui.ctx().fonts_mut(|f| f.layout_job(job));
                                ui.painter().with_clip_rect(c2.shrink2(vec2(3.0, 0.0))).galley(
                                    pos2(c2.left() + 5.0, y - g.size().y / 2.0),
                                    g,
                                    t.text_dim,
                                );
                                let c3 = cell(3);
                                ui.painter().with_clip_rect(c3.shrink2(vec2(3.0, 0.0))).text(
                                    pos2(c3.left() + 5.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    paper,
                                    crate::theme::small(),
                                    t.text_dim,
                                );
                                for k in 1..4 {
                                    ui.painter().vline(cell(k).left(), r.y_range(), Stroke::new(1.0, t.border.gamma_multiply(0.6)));
                                }
                                if resp.clicked() {
                                    act = Some(Act::Click(i, ui.input(|inp| inp.modifiers)));
                                }
                                if resp.drag_started() {
                                    // Dragging an unselected row drags it alone.
                                    if !on {
                                        act = Some(Act::Click(i, egui::Modifiers::NONE));
                                    }
                                    st.dragging = true;
                                }
                                resp.context_menu(|ui| {
                                    if !on {
                                        act = Some(Act::Click(i, egui::Modifiers::NONE));
                                    }
                                    if ui.button(crate::tl!("Duplicate")).clicked() {
                                        act = Some(Act::Duplicate);
                                        ui.close();
                                    }
                                    if ui.button(crate::tl!("Delete")).clicked() {
                                        act = Some(Act::Remove);
                                        ui.close();
                                    }
                                });
                            }
                            if rows.is_empty() {
                                ui.label(RichText::new(crate::tl!("No sheets. Add sheets below.")).color(t.text_dim));
                            }
                            // The space below the rows: a drag there draws a selection rectangle.
                            let rest = ui.available_size().max(vec2(w, 30.0));
                            let (blank, bresp) = ui.allocate_exact_size(vec2(w, rest.y), Sense::click_and_drag());
                            if bresp.clicked() && !ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                                act = Some(Act::Band(Vec::new(), false));
                            }
                            if bresp.drag_started() {
                                st.band_from = bresp.interact_pointer_pos();
                            }
                            if let (Some(from), Some(to)) = (st.band_from, ui.ctx().pointer_latest_pos()) {
                                let band = Rect::from_two_pos(from, to);
                                ui.painter().rect_filled(band, 0.0, t.accent.gamma_multiply(0.15));
                                ui.painter().rect_stroke(band, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                                let hit: Vec<usize> = rects.iter().enumerate().filter(|(_, r)| r.intersects(band)).map(|(i, _)| i).collect();
                                if ui.input(|i| i.pointer.any_released()) {
                                    act = Some(Act::Band(hit, ui.input(|i| i.modifiers.command)));
                                    st.band_from = None;
                                }
                            }
                            let _ = blank;
                            // Dragging rows: a line where they will land.
                            if st.dragging {
                                let pointer = ui.ctx().pointer_latest_pos();
                                let slot = pointer.map(|p| rects.iter().position(|r| p.y < r.center().y).unwrap_or(rects.len()));
                                if let Some(slot) = slot {
                                    let y = rects.get(slot).map_or_else(|| rects.last().map_or(0.0, |r| r.bottom()), |r| r.top());
                                    if let Some(first) = rects.first() {
                                        ui.painter().hline(first.left()..=first.right(), y, Stroke::new(2.0, t.accent));
                                    }
                                }
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                if ui.input(|i| i.pointer.any_released()) {
                                    if let Some(slot) = slot {
                                        act = Some(Act::MoveTo(slot));
                                    }
                                    st.dragging = false;
                                }
                            }
                        });
                    });
                });
                // The order and list buttons.
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    let any = !st.selected.is_empty();
                    let n = st.sheets.len();
                    let first = st.selected.iter().min().copied().unwrap_or(0);
                    let last = st.selected.iter().max().copied().unwrap_or(0);
                    let row = |ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool| -> bool {
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.horizontal(|ui| {
                                let r = ui.add_sized(vec2(176.0, 26.0), egui::Button::new(""));
                                let ir = Rect::from_center_size(pos2(r.rect.left() + 15.0, r.rect.center().y), vec2(18.0, 18.0));
                                icons::paint(ui.painter(), ir, icon, !enabled);
                                let col = if enabled { Tokens::get().text } else { Tokens::get().text_faint };
                                ui.painter().text(
                                    pos2(r.rect.left() + 30.0, r.rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    crate::theme::body(),
                                    col,
                                );
                                r.clicked()
                            })
                            .inner
                        })
                        .inner
                    };
                    if row(ui, Icon::MoveTop, crate::tl!("Move to Top"), any && first > 0) {
                        act = Some(Act::MoveTo(0));
                    }
                    if row(ui, Icon::MoveUp, crate::tl!("Move Higher"), any && first > 0) {
                        act = Some(Act::Up);
                    }
                    if row(ui, Icon::MoveDown, crate::tl!("Move Lower"), any && last + 1 < n) {
                        act = Some(Act::Down);
                    }
                    if row(ui, Icon::MoveBottom, crate::tl!("Move to Bottom"), any && last + 1 < n) {
                        act = Some(Act::MoveTo(n));
                    }
                    ui.add_space(6.0);
                    if row(ui, Icon::Copy, crate::tl!("Duplicate"), any) {
                        act = Some(Act::Duplicate);
                    }
                    if row(ui, Icon::Erase, crate::tl!("Delete"), any) {
                        act = Some(Act::Remove);
                    }
                    if row(ui, Icon::Close, crate::tl!("Clear List"), n > 0) {
                        act = Some(Act::Clear);
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(crate::tl!("Ctrl and Shift pick several sheets; drag in the empty space to pick with a rectangle."))
                            .small()
                            .color(t.text_dim),
                    );
                });
            });
            ui.add_space(6.0);
            ui.separator();
            // Adding sheets: from any open drawing.
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Add sheets from"));
                let from = st.add_from.clone().filter(|f| titles.contains(f)).unwrap_or_else(|| current.clone());
                let shown = crate::i18n::widget(ui, &from, egui::TextStyle::Body);
                egui::ComboBox::from_id_salt("cc_batch_from").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                    for title in &titles {
                        let name = crate::i18n::widget(ui, title, egui::TextStyle::Body);
                        if ui.selectable_label(*title == from, name).clicked() {
                            st.add_from = Some(title.clone());
                        }
                    }
                });
                if ui.button(crate::tl!("Add All")).clicked() {
                    act = Some(Act::AddAll(from.clone()));
                }
            });
            let from = st.add_from.clone().filter(|f| titles.contains(f)).unwrap_or_else(|| current.clone());
            let available = app.session.docs.iter().find(|d| d.title == from).map(|d| sheets_of(&d.doc, &from)).unwrap_or_default();
            egui::ScrollArea::horizontal().id_salt("cc_batch_add").max_height(34.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for s in available {
                        if ui
                            .button(format!("+ {}", crate::i18n::t(&s.layout)))
                            .on_hover_text(crate::tl!("Add this sheet to the end of the list"))
                            .clicked()
                        {
                            act = Some(Act::Add(s));
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.separate, false, crate::tl!("One PDF file with all sheets"));
                ui.radio_value(&mut st.separate, true, crate::tl!("A separate PDF file for each sheet"));
            });
            if let Some(n) = &st.note {
                ui.label(RichText::new(n).color(t.text_dim));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let enabled = can_plot && !st.sheets.is_empty();
                let r = ui.add_enabled(enabled, egui::Button::new(crate::tl!("Plot...")));
                let r = if can_plot { r } else { r.on_disabled_hover_text(crate::tl!("There is no file dialog here")) };
                if r.clicked() {
                    act = Some(Act::Plot);
                }
                if ui.button(crate::tl!("Close")).clicked() {
                    close = true;
                }
                ui.label(RichText::new(format!("{} {}", st.sheets.len(), crate::tl!("sheets"))).color(t.text_dim));
            });
            // Ctrl+A picks every sheet.
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                act = Some(Act::SelectAll);
            }
        });
    let n = st.sheets.len();
    match act {
        Some(Act::Click(i, m)) => {
            if m.shift {
                let a = st.anchor.unwrap_or(i).min(n.saturating_sub(1));
                let (lo, hi) = (a.min(i), a.max(i));
                if !m.command {
                    st.selected.clear();
                }
                for k in lo..=hi {
                    if !st.selected.contains(&k) {
                        st.selected.push(k);
                    }
                }
            } else if m.command {
                if let Some(p) = st.selected.iter().position(|k| *k == i) {
                    st.selected.remove(p);
                } else {
                    st.selected.push(i);
                }
                st.anchor = Some(i);
            } else {
                st.selected = vec![i];
                st.anchor = Some(i);
            }
        }
        Some(Act::Band(hit, add)) => {
            if !add {
                st.selected.clear();
            }
            for k in hit {
                if !st.selected.contains(&k) {
                    st.selected.push(k);
                }
            }
            st.anchor = st.selected.first().copied();
        }
        Some(Act::SelectAll) => st.selected = (0..n).collect(),
        Some(Act::MoveTo(slot)) => {
            let picked = st.selected.clone();
            st.selected = move_sheets(&mut st.sheets, &picked, slot);
        }
        Some(Act::Up) => {
            let first = st.selected.iter().min().copied().unwrap_or(0);
            let picked = st.selected.clone();
            st.selected = move_sheets(&mut st.sheets, &picked, first.saturating_sub(1));
        }
        Some(Act::Down) => {
            let last = st.selected.iter().max().copied().unwrap_or(0);
            let picked = st.selected.clone();
            st.selected = move_sheets(&mut st.sheets, &picked, (last + 2).min(n));
        }
        Some(Act::Duplicate) => {
            // Each picked sheet gets its copy right after the block of picked sheets.
            let mut idx = st.selected.clone();
            idx.sort_unstable();
            let copies: Vec<Sheet> = idx.iter().filter_map(|i| st.sheets.get(*i).cloned()).collect();
            let at = idx.last().map_or(n, |l| l + 1);
            let k = copies.len();
            for (j, c) in copies.into_iter().enumerate() {
                st.sheets.insert(at + j, c);
            }
            st.selected = (at..at + k).collect();
        }
        Some(Act::Remove) => {
            let mut idx = st.selected.clone();
            idx.sort_unstable();
            idx.dedup();
            for i in idx.iter().rev() {
                if *i < st.sheets.len() {
                    st.sheets.remove(*i);
                }
            }
            st.selected = idx.first().filter(|_| !st.sheets.is_empty()).map(|f| vec![(*f).min(st.sheets.len() - 1)]).unwrap_or_default();
        }
        Some(Act::Add(s)) => {
            st.sheets.push(s);
            st.selected = vec![st.sheets.len() - 1];
        }
        Some(Act::AddAll(title)) => {
            if let Some(d) = app.session.docs.iter().find(|d| d.title == title) {
                st.sheets.extend(sheets_of(&d.doc, &title));
            }
        }
        Some(Act::Clear) => {
            st.sheets.clear();
            st.selected.clear();
        }
        Some(Act::Plot) => {
            let stem = std::path::Path::new(&current)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Drawing".into());
            let picked = app.services.pick_save.as_ref().and_then(|pick| pick(&format!("{stem}.pdf")));
            if let Some(path) = picked {
                match publish_params(app, &st.sheets, !st.separate, Some(&path)).and_then(|p| app.run("publish", p)) {
                    Ok(r) => {
                        let msg = r["message"].as_str().unwrap_or("").to_string();
                        app.session.echo(msg);
                        close = true;
                    }
                    Err(e) => st.note = Some(e),
                }
            }
        }
        None => {}
    }
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !st.dragging {
        *open = false;
    }
    if *open {
        ctx.data_mut(|d| d.insert_temp(state_id(), st));
    } else {
        // Opened again later: filled anew from the drawing then current.
        ctx.data_mut(|d| d.remove::<State>(state_id()));
    }
}

#[cfg(test)]
mod tests {
    use super::{Sheet, move_sheet, move_sheets};

    fn s(n: &str) -> Sheet {
        Sheet { drawing: "A.dwg".into(), layout: n.into() }
    }

    #[test]
    fn sheets_move_to_a_slot() {
        let mut v = vec![s("1"), s("2"), s("3"), s("4")];
        assert_eq!(move_sheet(&mut v, 3, 0), Some(0));
        assert_eq!(v.iter().map(|x| x.layout.as_str()).collect::<Vec<_>>(), ["4", "1", "2", "3"]);
        assert_eq!(move_sheet(&mut v, 0, 4), Some(3));
        assert_eq!(v.iter().map(|x| x.layout.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
        assert_eq!(move_sheet(&mut v, 1, 3), Some(2));
        assert_eq!(v.iter().map(|x| x.layout.as_str()).collect::<Vec<_>>(), ["1", "3", "2", "4"]);
        assert_eq!(move_sheet(&mut v, 9, 0), None);
    }

    #[test]
    fn several_sheets_move_as_a_block() {
        let names = |v: &[Sheet]| v.iter().map(|x| x.layout.clone()).collect::<Vec<_>>().join("");
        let mut v = vec![s("1"), s("2"), s("3"), s("4"), s("5")];
        assert_eq!(move_sheets(&mut v, &[3, 1], 0), vec![0, 1]);
        assert_eq!(names(&v), "24135");
        assert_eq!(move_sheets(&mut v, &[0, 1], 5), vec![3, 4]);
        assert_eq!(names(&v), "13524");
        assert_eq!(move_sheets(&mut v, &[2], 1), vec![1]);
        assert_eq!(names(&v), "15324");
    }
}
