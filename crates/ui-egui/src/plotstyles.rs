//! Plot Style Manager (STYLESMANAGER): the plot style tables the drawing can use, a table's
//! styles in an editable grid, and the current layout's table and "Display plot styles".
//!
//! The dialog reads the drawing and changes it only through STYLESMANAGER and PAGESETUP
//! (`app.run`), so every edit is undoable and scriptable.

use egui::RichText;
use std::collections::{BTreeSet, HashSet};
use cadcraft_doc::{Drawing, Entity, EntityKind};
use cadcraft_color::{Color, aci_rgb};
use serde_json::{Value, json};

use crate::{CadApp, Pick};
use crate::theme::Tokens;

/// Open the dockable panel for a typed or menu-invoked STYLESMANAGER (no parameters).
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() || id != "stylesmanager" {
        return None;
    }
    Some(crate::docking::command(app, false, &json!({"operation": "open", "panel": "plotStyles"})))
}

fn temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(egui::Id::unique(id)))
}

fn set_temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(id), v));
}

// Column widths are stored by egui and remain editable independently for CTB and STB.
const DEFAULT_WIDTHS: [f32; 7] = [150.0, 190.0, 125.0, 110.0, 190.0, 190.0, 90.0];
const MIN_WIDTH: f32 = 25.0;
const COLUMN_GAP: f32 = 8.0;
const CELL_PADDING: f32 = 7.0;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ColumnWidths {
    widths: [f32; 7],
    manual: [bool; 7],
}
impl Default for ColumnWidths {
    fn default() -> Self { Self { widths: DEFAULT_WIDTHS, manual: [false; 7] } }
}

fn column_state(ctx: &egui::Context, named: bool) -> (egui::Id, ColumnWidths) {
    let id = egui::Id::unique(if named { "pstyle_columns_stb" } else { "pstyle_columns_ctb" });
    let state = ctx.data_mut(|d| d.get_persisted::<ColumnWidths>(id)).unwrap_or_default();
    (id, state)
}

// Keep manual widths; automatic columns use measured content, without filling spare space.
fn displayed_widths(state: &ColumnWidths, count: usize, fitted: &[f32; 7]) -> [f32; 7] {
    let mut widths = state.widths;
    for i in 0..count {
        widths[i] = if state.manual[i] { state.widths[i] } else { fitted[i] }.max(MIN_WIDTH);
    }
    widths
}

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), crate::theme::body(), egui::Color32::WHITE).size().x
}

// An ACI index is used only when an actual entity resolves to that exact index.
// RGB colours remain RGB: no nearest-index approximation.
fn used_aci_colors(d: &Drawing) -> BTreeSet<u8> {
    fn resolved(d: &Drawing, e: &Entity, inherited_layer: Option<&str>, inherited_block: Color) -> Color {
        let layer_name = if e.common.layer == "0" { inherited_layer.unwrap_or(&e.common.layer) } else { &e.common.layer };
        let layer_color = d.layer(layer_name).map(|l| l.color).unwrap_or(Color::Index(7));
        match e.common.color {
            Color::ByLayer => layer_color,
            Color::ByBlock => inherited_block,
            color => color,
        }
    }
    struct VisitState {
        used: BTreeSet<u8>,
        stack: Vec<String>,
        visited: HashSet<(String, String, String)>,
    }
    fn visit(d: &Drawing, e: &Entity, inherited_layer: Option<&str>, inherited_block: Color,
             state: &mut VisitState, depth: usize) {
        if depth > 64 { return; }
        let layer_name = if e.common.layer == "0" { inherited_layer.unwrap_or(&e.common.layer) } else { &e.common.layer };
        let color = resolved(d, e, inherited_layer, inherited_block);
        match &e.kind {
            EntityKind::Insert(insert) => {
                // An INSERT is a container; only its instantiated geometry and attributes count.
                if let Some(block) = d.block(&insert.block)
                    && !state.stack.iter().any(|name| name.eq_ignore_ascii_case(&insert.block))
                {
                        // Repeated instances with the same effective layer and colour have
                        // identical block-definition colours. Attributes are still checked below.
                        let context = (insert.block.to_ascii_lowercase(), layer_name.to_ascii_lowercase(), format!("{color:?}"));
                        if state.visited.insert(context) {
                            state.stack.push(insert.block.clone());
                            for child in block.entities.iter() {
                                // Layer 0 and ByBlock inherit the insertion context.
                                visit(d, child, Some(layer_name), color, state, depth + 1);
                            }
                            state.stack.pop();
                        }
                }
                // Attribute instances can override their definition's colour and layer.
                for attr in &insert.attribs {
                    if attr.invisible { continue; }
                    let attr_layer = attr.props.layer.as_deref().unwrap_or(layer_name);
                    let attr_color = attr.props.color.unwrap_or(Color::ByBlock);
                    let effective = match attr_color {
                        Color::ByLayer => d.layer(attr_layer).map(|l| l.color).unwrap_or(Color::Index(7)),
                        Color::ByBlock => color,
                        c => c,
                    };
                    if let Color::Index(i) = effective && i > 0 { state.used.insert(i); }
                }
            }
            EntityKind::AttDef(_) => {} // Definitions do not draw as independent objects.
            _ => { if let Color::Index(i) = color && i > 0 { state.used.insert(i); } }
        }
    }
    let mut state = VisitState {
        used: BTreeSet::new(),
        stack: Vec::new(),
        visited: HashSet::new(),
    };
    for e in d.model.iter() { visit(d, e, None, Color::Index(7), &mut state, 0); }
    for layout in &d.layouts {
        for e in layout.entities.iter() { visit(d, e, None, Color::Index(7), &mut state, 0); }
    }
    state.used
}

fn style_aci(name: &str) -> Option<u8> {
    name.strip_prefix("Color_").and_then(|s| s.parse::<u8>().ok()).filter(|i| *i > 0)
}

// Clip content to the cell: even a 25-pixel column must not displace its neighbour.
fn cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) -> egui::Rect {
    // Reserve the exact footprint in the parent before laying out any controls.
    // A child widget may request more space, but cannot move subsequent columns.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    child.add_space(CELL_PADDING.min((width - 1.0).max(0.0)));
    add(&mut child);
    rect
}

// Draw the handle on the actual right border, not after the heading text.
fn resize_handle(ui: &mut egui::Ui, rect: egui::Rect, index: usize, width: f32, columns: &mut ColumnWidths, fit: f32) {
    let x = rect.right();
    let hit = egui::Rect::from_min_max(egui::pos2(x - 4.0, rect.top()), egui::pos2(x + 4.0, rect.bottom()));
    let response = ui.interact(hit, ui.scope_id().with(("pstyle_resize", index)), egui::Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(crate::tl!("Drag to resize; double-click to fit"));
    ui.painter().vline(x, rect.y_range(), egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    if response.dragged() {
        columns.widths[index] = (width + response.drag_delta().x).max(MIN_WIDTH);
        columns.manual[index] = true;
    }
    if response.double_clicked() {
        columns.widths[index] = fit.max(MIN_WIDTH);
        columns.manual[index] = true;
    }
}

#[derive(Clone, Default)]
struct TableUiState {
    table: String,
    selected: BTreeSet<String>,
    sort: Option<(usize, bool)>,
    secondary: Option<(usize, bool)>,
}

fn aci_swatch(index: usize) -> egui::Color32 {
    let c = aci_rgb(index.min(255) as u8);
    egui::Color32::from_rgb(c.0, c.1, c.2)
}

fn swatch_at(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    ui.painter().rect_filled(rect, 1.0, color);
    ui.painter().rect_stroke(rect, 1.0, egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color), egui::StrokeKind::Inside);
}

fn swatch(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, color);
    ui.painter().rect_stroke(rect, 1.0, egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color), egui::StrokeKind::Inside);
}

/// Lineweights offered for a style, in millimetres.
const LINEWEIGHTS: [f64; 12] = [0.0, 0.09, 0.13, 0.18, 0.25, 0.3, 0.35, 0.5, 0.7, 1.0, 1.4, 2.11];

/// A command to run after the dialog is drawn.
type Action = Vec<(&'static str, Value)>;

pub fn panel(app: &mut CadApp, ui: &mut egui::Ui) {
    let action = content(app, ui);
    for (cmd, params) in action {
        match cmd {
            "plotstyle.import" => {
                let result = app.pick_then(Pick::PlotStyleOpen, |app, path| {
                    app.run("stylesmanager", json!({ "action": "load", "path": path }))?;
                    app.set_status(crate::tl!("Plot style table imported."));
                    Ok(Value::Null)
                });
                match result {
                    Some(Err(e)) => app.set_status(e),
                    None => app.set_status(crate::tl!("No file picker available for import.")),
                    _ => {}
                }
            }
            "plotstyle.export" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else { continue };
                let filename = if name.to_ascii_lowercase().ends_with(".json") {
                    name.to_owned()
                } else {
                    format!("{name}.json")
                };
                let table_name = name.to_owned();
                let result = app.pick_then(Pick::PlotStyleSave(filename), move |app, path| {
                    app.run("stylesmanager", json!({ "action": "save", "name": table_name, "path": path }))?;
                    app.set_status(crate::tl!("Plot style table exported."));
                    Ok(Value::Null)
                });
                match result {
                    Some(Err(e)) => app.set_status(e),
                    None => app.set_status(crate::tl!("No file picker available for export.")),
                    _ => {}
                }
            }
            _ => {
                if let Err(e) = app.run(cmd, params) {
                    app.set_status(e);
                }
            }
        }
    }
}

/// Plot style editor content, hosted by the CADCraft docking system.
fn content(app: &mut CadApp, ui: &mut egui::Ui) -> Action {
    let ctx = ui.ctx().clone();
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        ui.label(crate::tl!("Open a drawing first."));
        return Vec::new();
    };
    let names = cadcraft_doc::plot_style_table_names(d);
    let space = app.session.layout_space();
    let layout = space.layout().and_then(|n| d.layout(n).map(|l| (n.to_string(), l.page.plot_style_table.clone(), l.page.show_plot_styles)));
    let mut sel = temp::<String>(&ctx, "pstyle_sel")
        .filter(|s| names.iter().any(|n| n.eq_ignore_ascii_case(s)))
        .or_else(|| layout.as_ref().map(|l| l.1.clone()).filter(|n| names.iter().any(|x| x.eq_ignore_ascii_case(n))))
        .or_else(|| names.first().cloned())
        .unwrap_or_default();
    let table = cadcraft_doc::plot_style_table(d, &sel);
    let in_drawing = d.plot_style_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&sel));
    let linetypes: Vec<String> =
        d.linetypes.iter().map(|l| l.name.clone()).filter(|n| !n.eq_ignore_ascii_case("ByLayer") && !n.eq_ignore_ascii_case("ByBlock")).collect();
    let mut new_style = temp::<String>(&ctx, "pstyle_new").unwrap_or_default();
    let mut action: Action = Vec::new();
    let mut state = temp::<TableUiState>(&ctx, "pstyle_ui_state").unwrap_or_default();
    if !state.table.eq_ignore_ascii_case(&sel) { state = TableUiState { table: sel.clone(), ..Default::default() }; }
    let set = |table: &str, style: Value| ("stylesmanager", json!({ "action": "set", "name": table, "style": style }));
   egui::Frame::new()
    .inner_margin(egui::Margin {
        left: 8,
        right: 8,
        top: 0,
        bottom: 0,
    })
    .show(ui, |ui| {
        ui.set_min_height(ui.available_height());
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Plot style table"));
            craft_ui::ComboBox::from_id_salt("pstyle_table").selected_text(sel.as_str()).width(200.0).show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(n.eq_ignore_ascii_case(&sel), n).clicked() {
                        sel = n.clone();
                    }
                }
            });
            if ui.button(crate::tl!("Copy")).on_hover_text(crate::tl!("Keep an editable copy of this table in the drawing")).clicked() {
                let stem = sel.rsplit_once('.').map(|(s, _)| s).unwrap_or(&sel);
                let copy = format!("{stem}-copy");
                action.push(("stylesmanager", json!({ "action": "new", "name": copy, "from": sel })));
            }
            if ui.button("+CTB").clicked() {
                action.push(("stylesmanager", json!({ "action": "new", "name": "new.ctb" })));
            }
            if ui.button("+STB").clicked() {
                action.push(("stylesmanager", json!({ "action": "new", "name": "new.stb" })));
            }
            if ui.button(crate::tl!("Import")).on_hover_text(crate::tl!("Load a CADCraft native JSON plot style table")).clicked() {
                action.push(("plotstyle.import", Value::Null));
            }
            if ui.add_enabled(table.is_some(), egui::Button::new(crate::tl!("Export")))
                .on_hover_text(crate::tl!("Save the selected table in CADCraft native JSON format"))
                .clicked()
            {
                action.push(("plotstyle.export", json!({ "name": sel })));
            }
            if ui.add_enabled(in_drawing, egui::Button::new(crate::tl!("Delete"))).clicked() {
                action.push(("stylesmanager", json!({ "action": "delete", "name": sel })));
            }
        });
        if let Some((name, current, show)) = &layout {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(crate::tf!(
                        "Layout {name}: {table}",
                        name = name,
                        table = if current.is_empty() { crate::i18n::t("None") } else { current.as_str() }
                    ))
                    .color(t.text_dim),
                );
                if ui.add_enabled(!current.eq_ignore_ascii_case(&sel), egui::Button::new(crate::tl!("Use for this layout"))).clicked() {
                    action.push(("pagesetup", json!({ "layout": name, "plotStyleTable": sel })));
                }
                let mut s = *show;
                if ui.checkbox(&mut s, crate::tl!("Display plot styles")).changed() {
                    action.push(("pagesetup", json!({ "layout": name, "displayPlotStyles": s })));
                }
            });
        }
        ui.separator();
        let Some(table) = &table else {
            ui.label(crate::tl!("No plot style tables."));
            return;
        };
        if !state.table.eq_ignore_ascii_case(&sel) { state = TableUiState { table: sel.clone(), ..Default::default() }; }
        let named = !table.is_color_dependent();
        ui.label(RichText::new(&table.description).color(t.text_dim));
        if !in_drawing {
            ui.label(RichText::new(crate::tl!("A built-in table: editing it keeps an edited copy in the drawing.")).color(t.text_faint));
        }
        let used_colors = if table.is_color_dependent() { used_aci_colors(d) } else { BTreeSet::new() };
        // Selection is keyed by style name, never by its displayed row position.
        let mut rows: Vec<_> = table.styles.iter().collect();
        macro_rules! sort_key {
            ($style:expr, $col:expr) => {
                match $col {
                    0 => $style.name.to_lowercase(),
                    1 => $style.color.map(|c| c.hex()).unwrap_or_default(),
                    2 => format!("{}", u8::from($style.grayscale)),
                    3 => format!("{:03}", $style.screening),
                    4 => $style.linetype.clone().unwrap_or_default().to_lowercase(),
                    5 => format!("{:05}", $style.lineweight.unwrap_or(0)),
                    6 => format!("{}", u8::from(style_aci(&$style.name).is_some_and(|i| used_colors.contains(&i)))),
                    _ => $style.name.to_lowercase(),
                }
            };
        }
        if let Some((col, descending)) = state.sort {
            rows.sort_by(|a, b| {
                let mut cmp = sort_key!(a, col).cmp(&sort_key!(b, col));
                if descending { cmp = cmp.reverse(); }
                if cmp == std::cmp::Ordering::Equal
                    && let Some((second, reverse)) = state.secondary
                {
                    cmp = sort_key!(a, second).cmp(&sort_key!(b, second));
                    if reverse { cmp = cmp.reverse(); }
                }
                cmp
            });
        }
        // Remove names deleted from the table so a later batch edit cannot recreate them.
        let current_names: BTreeSet<String> = table.styles.iter().map(|st| st.name.clone()).collect();
        state.selected.retain(|name| current_names.contains(name));
        let count = 7;
        let (state_id, mut columns) = column_state(&ctx, named);
        let fit_requested = ui.button(crate::tl!("Fit columns")).clicked();
        let height = (ui.available_height() - if named { 46.0 } else { 0.0 }).max(120.0);
        let headings = [crate::tl!("Plot style"), crate::tl!("Color"), crate::tl!("Grayscale"), crate::tl!("Screening"), crate::tl!("Linetype"), crate::tl!("Lineweight"), if named { crate::tl!("Remove") } else { crate::tl!("Used") }];
        let mut fitted = [MIN_WIDTH; 7];
        for i in 0..count { fitted[i] = text_width(ui, headings[i]) + 26.0; }
        for st in &rows {
            fitted[0] = fitted[0].max(text_width(ui, &st.name) + 40.0);
            fitted[1] = fitted[1].max(text_width(ui, &st.color.map(|c| c.hex()).unwrap_or_else(|| crate::tl!("Object").to_owned())) + 42.0);
            fitted[3] = fitted[3].max(text_width(ui, &format!("{} %", st.screening)) + 28.0);
            fitted[4] = fitted[4].max(text_width(ui, st.linetype.as_deref().unwrap_or(crate::tl!("Default"))) + 42.0);
            let lw = st.lineweight.map(|w| format!("{:.2} mm", f64::from(w) / 100.0)).unwrap_or_else(|| crate::tl!("Default").to_owned());
            fitted[5] = fitted[5].max(text_width(ui, &lw) + 42.0);
        }
        for name in &linetypes { fitted[4] = fitted[4].max(text_width(ui, name) + 42.0); }
        for mm in LINEWEIGHTS { fitted[5] = fitted[5].max(text_width(ui, &format!("{mm:.2} mm")) + 42.0); }
        if fit_requested { columns.manual = [false; 7]; }
        let widths = displayed_widths(&columns, count, &fitted);
        let total_width = widths[..count].iter().sum::<f32>() + COLUMN_GAP * count as f32;
        ui.label(format!("{} {}", state.selected.len(), crate::tl!("styles selected")));
        // One scrollable grid: a single horizontal coordinate system for headers and rows.
        // The selection checkboxes share the same vertical scroll as their data rows.
        egui::ScrollArea::horizontal()
            .id_salt("pstyle_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(total_width + 36.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = COLUMN_GAP;
                    let all = !rows.is_empty() && rows.iter().all(|s| state.selected.contains(&s.name));
                    let mut check = all;
                    if ui.add_sized([24.0, 24.0], egui::Checkbox::without_text(&mut check))
                        .on_hover_text(crate::tl!("Select all displayed styles")).changed() {
                        for st in &rows {
                            if check { state.selected.insert(st.name.clone()); }
                            else { state.selected.remove(&st.name); }
                        }
                    }
                for i in 0..count {
                    let label = if let Some((col, desc)) = state.sort {
                        if col == i { format!("{} {}", headings[i], if desc { "▼" } else { "▲" }) }
                        else { headings[i].to_owned() }
                    } else { headings[i].to_owned() };
                    let rect = cell(ui, widths[i], |ui| {
                        if ui.add(egui::Label::new(RichText::new(label).strong()).sense(egui::Sense::click()).truncate()).clicked() {
                            let next = match state.sort {
                                Some((c, false)) if c == i => Some((i, true)),
                                Some((c, true)) if c == i => None,
                                _ => Some((i, false)),
                            };
                            if ui.input(|input| input.modifiers.shift) {
                                state.secondary = next;
                            } else {
                                state.sort = next;
                                state.secondary = None;
                            }
                        }
                        if i == 4 { ui.label("?").on_hover_text(crate::tl!("Use object linetype")); }
                        if i == 5 { ui.label("?").on_hover_text(crate::tl!("Use object lineweight")); }
                    });
                    resize_handle(ui, rect, i, widths[i], &mut columns, fitted[i]);
                }
            });
                egui::ScrollArea::vertical()
                    .id_salt("pstyle_rows_vertical")
                    .max_height(height - 30.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    for st in &rows {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
                            let mut checked = state.selected.contains(&st.name);
                            if ui.add_sized([24.0, 24.0], egui::Checkbox::without_text(&mut checked)).changed() {
                                if checked { state.selected.insert(st.name.clone()); }
                                else { state.selected.remove(&st.name); }
                            }
                    let targets: Vec<_> = if state.selected.contains(&st.name) {
                        state.selected.iter().cloned().collect()
                    } else { vec![st.name.clone()] };
                    let mut edit = |key: &str, value: Value| {
                        for name in &targets {
                            action.push(set(&table.name, json!({ "name": name, (key): value })));
                        }
                    };
                    cell(ui, widths[0], |ui| {
                        let index = style_aci(&st.name).map(usize::from).unwrap_or(7);
                        swatch(ui, aci_swatch(index));
                        ui.add(egui::Label::new(&st.name).truncate());
                    });
                    cell(ui, widths[1], |ui| {
                        let current = st.color.map(Color::True).unwrap_or(Color::Index(7));
                        let id = ui.scope_id().with(("pstyle_color", &st.name));
                        let button_width = (widths[1] - CELL_PADDING * 2.0).max(12.0);
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(button_width, 20.0), egui::Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 2.0, ui.visuals().widgets.hovered.bg_fill);
                        }
                        if let Some(rgb) = st.color {
                            let square = egui::Rect::from_min_size(
                                egui::pos2(rect.left() + 3.0, rect.center().y - 6.0),
                                egui::vec2(12.0, 12.0),
                            );
                            swatch_at(ui, square, egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2));
                            let text_rect = egui::Rect::from_min_max(
                                egui::pos2(rect.left() + 20.0, rect.top()), rect.right_bottom(),
                            );
                            ui.painter().with_clip_rect(text_rect).text(
                                text_rect.left_center(), egui::Align2::LEFT_CENTER,
                                rgb.hex(), crate::theme::body(), ui.visuals().text_color(),
                            );
                        } else {
                            ui.painter().with_clip_rect(rect).text(
                                rect.left_center(), egui::Align2::LEFT_CENTER,
                                crate::tl!("Object"), crate::theme::body(), ui.visuals().text_color(),
                            );
                        }
                        let response = response.on_hover_text(
                            st.color.map(|c| format!("RGB : {}", c.hex())).unwrap_or_else(|| crate::tl!("Object color").to_owned())
                        );
                        egui::Popup::from_toggle_button_response(&response)
                            .id(id)
                            .width(310.0)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                // The popup must not inherit the clipped grid cell: the RGB
                                // picker paints its gradient outside the cell's bounds.
                                ui.set_clip_rect(ui.ctx().content_rect());
                                if ui.button(crate::tl!("Object color")).clicked() {
                                    edit("color", Value::Null);
                                    ui.close();
                                }
                                if let Some(picked) = crate::layers::color_palette(ui, id, current, false) {
                                    let rgb = picked.resolve(Color::WHITE, Color::WHITE);
                                    edit("color", json!(rgb.hex()));
                                    ui.close();
                                }
                            });
                    });
                    cell(ui, widths[2], |ui| {
                        let mut gray = st.grayscale;
                        if ui.checkbox(&mut gray, "").on_hover_text(crate::tl!("Print in grayscale")).changed() {
                            edit("grayscale", json!(gray));
                        }
                    });
                    cell(ui, widths[3], |ui| {
                        let mut screen = st.screening.min(100);
                        ui.push_id(("pstyle_screening", &table.name, &st.name), |ui| {
                            if ui.add(egui::DragValue::new(&mut screen).range(0..=100).suffix(" %")).changed() {
                                edit("screening", json!(screen));
                            }
                        });
                    });
                    cell(ui, widths[4], |ui| {
                        let lt = st.linetype.clone().unwrap_or_else(|| crate::tl!("Default").to_string());
                        craft_ui::ComboBox::from_id_salt(("pstyle_lt", &st.name)).selected_text(lt).width((widths[4] - 18.0).max(12.0)).show_ui(ui, |ui| {
                            if ui.selectable_label(st.linetype.is_none(), crate::tl!("Default")).clicked() { edit("linetype", Value::Null); }
                            for n in &linetypes {
                                if ui.selectable_label(st.linetype.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n)), n).clicked() { edit("linetype", json!(n)); }
                            }
                        });
                    });
                    cell(ui, widths[5], |ui| {
                        let lw = st.lineweight.map(|w| format!("{:.2} mm", f64::from(w) / 100.0)).unwrap_or_else(|| crate::tl!("Default").to_string());
                        craft_ui::ComboBox::from_id_salt(("pstyle_lw", &st.name)).selected_text(lw).width((widths[5] - 18.0).max(12.0)).show_ui(ui, |ui| {
                            if ui.selectable_label(st.lineweight.is_none(), crate::tl!("Default")).clicked() { edit("lineweight", Value::Null); }
                            for mm in LINEWEIGHTS {
                                let hundredths = (mm * 100.0).round() as u16;
                                if ui.selectable_label(st.lineweight == Some(hundredths), format!("{mm:.2} mm")).clicked() { edit("lineweight", json!(mm)); }
                            }
                        });
                    });
                    drop(edit);
                    if !named {
                        cell(ui, widths[6], |ui| {
                            if style_aci(&st.name).is_some_and(|i| used_colors.contains(&i)) {
                                ui.label("✓").on_hover_text(crate::tl!("Color used by at least one object"));
                            }
                        });
                    }
                    if named {
                        cell(ui, widths[6], |ui| {
                            let normal = st.name.eq_ignore_ascii_case(cadcraft_doc::NORMAL_STYLE);
                            if ui.add_enabled(!normal, egui::Button::new(crate::tl!("Remove"))).clicked() {
                                state.selected.remove(&st.name);
                                action.push(("stylesmanager", json!({ "action": "set", "name": table.name, "removeStyle": st.name })));
                            }
                        });
                    }
                    });
                }
            });
        });
        ctx.data_mut(|d| d.insert_persisted(state_id, columns));
        if named {
            ui.separator();
            ui.horizontal(|ui| {
                let label = ui.label(crate::tl!("New style"));
                ui.add(egui::TextEdit::singleline(&mut new_style).desired_width(160.0)).labelled_by(label.id);
                if ui.add_enabled(!new_style.trim().is_empty(), egui::Button::new(crate::tl!("Add"))).clicked() {
                    action.push(set(&table.name, json!({ "name": new_style.trim() })));
                    new_style.clear();
                }
            });
        }
    });
    set_temp(&ctx, "pstyle_ui_state", state);
    set_temp(&ctx, "pstyle_sel", sel);
    set_temp(&ctx, "pstyle_new", new_style);
    action
}
