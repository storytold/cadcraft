//! The Property Lookup Table dialog: BLOOKUPTABLE typed, or a double-click on a lookup action in
//! the Block Editor (as in AutoCAD), while the Block Editor is open on a block with one. As AutoCAD's dialog: the
//! action's input properties (left) and lookup properties (right) as columns, one row per set of
//! inputs, a blank row at the end to add one, then the `<Unmatched>` row (what a reference shows
//! when no row matches, editable for each lookup property) and whether each lookup property can be
//! set on a reference ("Allow reverse lookup", only while its values don't repeat) or is "Read
//! only". Add Properties... adds columns, Audit checks the table, OK writes it as one step
//! (`blookuptable {inputs, lookups, rows, unmatched, reverse}`); Cancel changes nothing. A block
//! with several lookup actions picks one at the top (the one picked in the editor first). JSON
//! calls skip the dialog.

use cadcraft_engine::doc::dynblock::{ActionKind, DynamicBlock, ParameterKind, column_property, dynamic_block, input_properties, set_lookup_table};
use egui::{RichText, Stroke, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// One column as the dialog edits it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Column {
    /// The property name shown ("Distance1", "Position1 X", "Size").
    pub name: String,
    pub parameter: i32,
    pub property: String,
    pub lookup: bool,
    pub unmatched: String,
    pub reverse: bool,
    /// A visibility state column's states (its cells are picked from them).
    pub states: Vec<String>,
}

/// The Add Parameter Properties dialog's fields: which list, and the properties ticked in it.
#[derive(Clone, Debug, Default)]
pub struct Adding {
    pub lookup: bool,
    pub ticked: Vec<String>,
}

/// The dialog's state while it is open: the table as it will be written on OK.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// The block's lookup actions (graph id, name) and the one shown.
    pub actions: Vec<(i32, String)>,
    pub action: i32,
    /// Input columns first, then lookup columns.
    pub columns: Vec<Column>,
    /// One cell per column; the blank row at the end isn't kept here.
    pub rows: Vec<Vec<String>>,
    /// The Add Parameter Properties dialog, while open.
    pub adding: Option<Adding>,
    /// Audit's verdict or why OK was refused (`true`: a problem).
    pub message: Option<(String, bool)>,
}

impl State {
    /// Load action `action` of block `m` into the dialog.
    fn load(m: &DynamicBlock, action: i32) -> State {
        let actions: Vec<(i32, String)> =
            m.actions.iter().filter(|a| matches!(a.kind, ActionKind::Lookup { .. })).map(|a| (a.node, a.name.clone())).collect();
        let Some(ActionKind::Lookup { columns, rows }) = m.actions.iter().find(|a| a.node == action).map(|a| &a.kind) else {
            return State { actions, action, ..State::default() };
        };
        let columns = columns
            .iter()
            .map(|c| Column {
                name: column_property(m, c.node, &c.property),
                parameter: c.node,
                property: c.property.clone(),
                lookup: c.lookup,
                unmatched: c.unmatched.clone(),
                reverse: c.lookup && c.reverse,
                states: states_of(m, c.node, &c.property),
            })
            .collect();
        State { actions, action, columns, rows: rows.clone(), ..State::default() }
    }

    fn inputs(&self) -> usize {
        self.columns.iter().filter(|c| !c.lookup).count()
    }

    /// Whether lookup column `j`'s values repeat (or it has none): it can't be reversed then.
    fn repeats(&self, j: usize) -> bool {
        let values: Vec<String> = self.rows.iter().filter_map(|r| r.get(j)).map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()).collect();
        values.is_empty() || values.iter().enumerate().any(|(i, v)| values.iter().take(i).any(|w| w == v))
    }

    /// Add `column` with an empty cell in every row: an input after the other inputs, a lookup
    /// column last.
    fn add_column(&mut self, column: Column) {
        let at = if column.lookup { self.columns.len() } else { self.inputs() };
        self.columns.insert(at, column);
        for r in &mut self.rows {
            r.insert(at.min(r.len()), String::new());
        }
    }

    /// Remove column `j` (not the action's own lookup column, the first lookup column).
    fn remove_column(&mut self, j: usize) {
        if !self.removable(j) {
            return;
        }
        self.columns.remove(j);
        for r in &mut self.rows {
            if j < r.len() {
                r.remove(j);
            }
        }
    }

    fn removable(&self, j: usize) -> bool {
        j != self.inputs() && j < self.columns.len()
    }

    /// The `blookuptable` parameters writing this table.
    fn params(&self) -> Value {
        let names = |lookup: bool| self.columns.iter().filter(|c| c.lookup == lookup).map(|c| c.name.clone()).collect::<Vec<_>>();
        let mut unmatched = Map::new();
        for c in &self.columns {
            unmatched.insert(c.name.clone(), json!(c.unmatched));
        }
        let reverse: Vec<String> =
            self.columns.iter().enumerate().filter(|(j, c)| c.lookup && c.reverse && !self.repeats(*j)).map(|(_, c)| c.name.clone()).collect();
        let rows: Vec<&Vec<String>> = self.rows.iter().filter(|r| r.iter().any(|c| !c.trim().is_empty())).collect();
        json!({ "action": self.action, "inputs": names(false), "lookups": names(true), "rows": rows, "unmatched": unmatched, "reverse": reverse })
    }

    /// The table written into a copy of drawing `d`'s block `block`: `Err` with AutoCAD's reason
    /// when it would be refused (Audit, and OK before writing).
    fn audit(&self, d: &cadcraft_engine::doc::Drawing, block: &str) -> Result<(), String> {
        let mut d = d.clone();
        let columns: Vec<cadcraft_engine::doc::dynblock::TableColumn> = self
            .columns
            .iter()
            .enumerate()
            .map(|(j, c)| cadcraft_engine::doc::dynblock::TableColumn {
                node: c.parameter,
                property: c.property.clone(),
                lookup: c.lookup,
                unmatched: c.unmatched.clone(),
                reverse: c.reverse && !self.repeats(j),
            })
            .collect();
        set_lookup_table(&mut d, block, self.action, &columns, &self.rows)
    }
}

/// A visibility column's states (empty for other columns).
fn states_of(m: &DynamicBlock, node: i32, property: &str) -> Vec<String> {
    if !property.eq_ignore_ascii_case("VisibilityState") {
        return Vec::new();
    }
    m.parameters
        .iter()
        .find(|p| p.node == node)
        .and_then(|p| match &p.kind {
            ParameterKind::Visibility { states, .. } => Some(states.iter().map(|s| s.name.clone()).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

/// The edited block's name and dynamic data, when the Block Editor is open on a block.
fn edited(app: &CadApp) -> Option<(String, DynamicBlock, Vec<i32>)> {
    let st = app.session.state().ok()?;
    let be = st.block_edit.as_ref()?;
    if be.current {
        return None;
    }
    Some((be.name.clone(), dynamic_block(&st.doc, &be.name)?, be.picked.clone()))
}

/// BLOOKUPTABLE typed or from the Block Editor's bar without parameters opens the dialog when the
/// edited block has a lookup action (the one picked in the editor, else its first); otherwise
/// BLOOKUPTABLE runs and says why it can't.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != "blookuptable" || !params.is_null() {
        return None;
    }
    let (_, m, picked) = edited(app)?;
    let lookups: Vec<i32> = m.actions.iter().filter(|a| matches!(a.kind, ActionKind::Lookup { .. })).map(|a| a.node).collect();
    let action = lookups.iter().copied().find(|a| picked.contains(a)).or_else(|| lookups.first().copied())?;
    open(app, action).then_some(Ok(Value::Null))
}

/// Open the dialog on action `node` of the edited block when it is a lookup action (a
/// double-click on its mark); whether it opened.
pub fn open(app: &mut CadApp, node: i32) -> bool {
    let Some((_, m, _)) = edited(app) else { return false };
    if !m.actions.iter().any(|a| a.node == node && matches!(a.kind, ActionKind::Lookup { .. })) {
        return false;
    }
    app.blookup = State::load(&m, node);
    app.ui.dialog = Some("blookuptable".into());
    true
}

/// The Property Lookup Table window (and Add Parameter Properties over it).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let Some((block, m, _)) = edited(app) else {
        *open = false;
        return;
    };
    let (mut ok, mut cancel, mut audit) = (false, false, false);
    let mut st = std::mem::take(&mut app.blookup);
    let modal = st.adding.is_some();
    let mut switch: Option<i32> = None;
    egui::Window::new(crate::tl!("Property Lookup Table"))
        .id(egui::Id::unique("Property Lookup Table"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_width(760.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.add_enabled_ui(!modal, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Action name:"));
                    let current = st.actions.iter().find(|(id, _)| *id == st.action).map(|(_, n)| n.clone()).unwrap_or_default();
                    if st.actions.len() > 1 {
                        craft_ui::ComboBox::from_id_salt("blookup-action").selected_text(current).show_ui(ui, |ui| {
                            for (id, n) in &st.actions {
                                if ui.selectable_label(*id == st.action, n.as_str()).clicked() && *id != st.action {
                                    switch = Some(*id);
                                }
                            }
                        });
                    } else {
                        ui.label(RichText::new(current).color(t.text));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(crate::tl!("Audit")).clicked() {
                            audit = true;
                        }
                        if ui.button(crate::tl!("Add Properties...")).clicked() {
                            st.adding = Some(Adding::default());
                            st.message = None;
                        }
                    });
                });
                ui.add_space(6.0);
                egui::ScrollArea::both().id_salt("blookup-table").auto_shrink([false, true]).max_height(420.0).show(ui, |ui| {
                    table(ui, &mut st);
                });
                ui.add_space(6.0);
                if let Some((msg, problem)) = &st.message {
                    let color = if *problem { t.warn } else { t.accent };
                    egui::Frame::new().stroke(Stroke::new(1.0, color)).corner_radius(3.0).inner_margin(vec2(8.0, 4.0)).show(ui, |ui| {
                        ui.colored_label(color, msg);
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(crate::tl!("Cancel")).clicked() {
                            cancel = true;
                        }
                        if ui.button(crate::tl!("OK")).clicked() {
                            ok = true;
                        }
                    });
                });
            });
        });
    if st.adding.is_some() {
        add_dialog(ctx, &mut st, &m);
    }
    if let Some(id) = switch {
        st = State::load(&m, id);
    }
    if audit {
        st.message = Some(match st.audit(&app.session.state().map(|s| s.doc.clone()).unwrap_or_default(), &block) {
            Ok(()) => (crate::i18n::t("No errors have been found in the Property Lookup Table.").to_string(), false),
            Err(e) => (e, true),
        });
    }
    if ok {
        match app.run("blookuptable", st.params()) {
            Ok(_) => *open = false,
            Err(e) => {
                st.message = Some((e, true));
                app.blookup = st;
                return;
            }
        }
    }
    if cancel {
        *open = false;
    }
    app.blookup = st;
}

/// The grid's row height, header height and row-number column width.
const ROW_H: f32 = 26.0;
const HEAD_H: f32 = 28.0;
const NUM_W: f32 = 34.0;

/// A column's width: wider for a list (a visibility state, a lookup's reverse-lookup choice).
fn col_width(c: &Column) -> f32 {
    if c.states.is_empty() && !c.lookup { 120.0 } else { 180.0 }
}

/// What the grid asks for once drawn: a column to remove, a row to delete.
#[derive(Default)]
struct Edits {
    remove: Option<usize>,
    delete_row: Option<usize>,
}

/// The table, as AutoCAD lays it out: Input Properties and Lookup Properties side by side, each
/// a grid with its header, a row per set of inputs, a blank row that adds one when filled in,
/// then the `<Unmatched>` row and (lookups) the reverse-lookup row. Right-click a header to
/// remove the property, a row number to delete the row.
fn table(ui: &mut egui::Ui, st: &mut State) {
    let inputs = st.inputs();
    let len = st.columns.len();
    let mut edits = Edits::default();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        pane(ui, st, crate::tl!("Input Properties"), 0..inputs, true, &mut edits);
        pane(ui, st, crate::tl!("Lookup Properties"), inputs..len, false, &mut edits);
    });
    if let Some(j) = edits.remove {
        st.remove_column(j);
    }
    if let Some(r) = edits.delete_row
        && r < st.rows.len()
    {
        st.rows.remove(r);
    }
}

/// One side of the table: columns `cols` (with row numbers when `numbers`).
fn pane(ui: &mut egui::Ui, st: &mut State, title: &str, cols: std::ops::Range<usize>, numbers: bool, edits: &mut Edits) {
    let t = Tokens::get();
    let n = st.rows.len();
    // The data rows, the blank row, <Unmatched> and reverse lookup: both sides line up.
    let rows = n + 3;
    let num_w = if numbers { NUM_W } else { 0.0 };
    let widths: Vec<f32> = cols.clone().filter_map(|j| st.columns.get(j)).map(col_width).collect();
    let w = num_w + if widths.is_empty() { 240.0 } else { widths.iter().sum() };
    let font = egui::FontId::proportional(13.0);
    ui.vertical(|ui| {
        ui.label(RichText::new(title).strong().color(t.text));
        ui.add_space(2.0);
        let (rect, _) = ui.allocate_exact_size(vec2(w, HEAD_H + ROW_H * rows as f32), egui::Sense::hover());
        let p = ui.painter_at(rect.expand(1.0));
        let top = rect.top() + HEAD_H;
        let row_y = |r: usize| top + ROW_H * r as f32;
        // Backgrounds: the cells, the header, the row numbers, and the rows after the data.
        p.rect_filled(rect, 3.0, t.field);
        p.rect_filled(egui::Rect::from_min_size(rect.min, vec2(w, HEAD_H)), egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, t.panel);
        if numbers {
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), top), egui::pos2(rect.left() + num_w, rect.bottom())), 0.0, t.panel);
        }
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left() + num_w, row_y(n + 1)), rect.max), 0.0, t.panel.gamma_multiply(0.55));
        let line = Stroke::new(1.0, t.border);
        for r in 0..rows {
            p.hline(rect.x_range(), row_y(r), line);
        }
        let mut x = rect.left() + num_w;
        if numbers {
            p.vline(x, rect.y_range(), line);
        }
        for cw in &widths {
            x += cw;
            if x < rect.right() - 1.0 {
                p.vline(x, rect.y_range(), line);
            }
        }
        p.rect_stroke(rect, 3.0, line, egui::StrokeKind::Inside);
        if widths.is_empty() {
            let hint = crate::tl!("Add Properties... adds input properties");
            p.text(rect.center(), egui::Align2::CENTER_CENTER, hint, font.clone(), t.text_dim);
            return;
        }

        // Headers: right-click one to remove its property (not the action's own lookup).
        let mut x = rect.left() + num_w;
        for (k, j) in cols.clone().enumerate() {
            let (Some(c), Some(cw)) = (st.columns.get(j), widths.get(k)) else { continue };
            let hr = egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(*cw, HEAD_H));
            let color = if c.lookup { t.accent } else { t.text };
            p.text(hr.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, &c.name, egui::FontId::proportional(13.0), color);
            let removable = st.removable(j);
            ui.interact(hr, ui.scope_id().with(("blookup-head", j)), egui::Sense::click()).context_menu(|ui| {
                if ui.add_enabled(removable, egui::Button::new(crate::tl!("Remove Property"))).clicked() {
                    edits.remove = Some(j);
                    ui.close();
                }
            });
            x += cw;
        }

        // Row numbers: right-click one to delete its row; `*` marks the row that adds one.
        if numbers {
            for r in 0..=n {
                let nr = egui::Rect::from_min_size(egui::pos2(rect.left(), row_y(r)), vec2(num_w, ROW_H));
                let label = if r < n { format!("{}", r + 1) } else { "*".to_string() };
                p.text(nr.center(), egui::Align2::CENTER_CENTER, label, font.clone(), t.text_dim);
                if r < n {
                    ui.interact(nr, ui.scope_id().with(("blookup-row", r)), egui::Sense::click()).context_menu(|ui| {
                        if ui.button(crate::tl!("Delete Row")).clicked() {
                            edits.delete_row = Some(r);
                            ui.close();
                        }
                    });
                }
            }
        }

        // The cells: text, or a visibility state picked from its list.
        let cell_rect = |k: usize, r: usize| {
            let x = rect.left() + num_w + widths.iter().take(k).sum::<f32>();
            egui::Rect::from_min_size(egui::pos2(x, row_y(r)), vec2(widths.get(k).copied().unwrap_or(0.0), ROW_H)).shrink(1.0)
        };
        let blank = vec![String::new(); st.columns.len()];
        for r in 0..=n {
            let mut row = st.rows.get(r).cloned().unwrap_or_else(|| blank.clone());
            for (k, j) in cols.clone().enumerate() {
                let (Some(c), Some(cell)) = (st.columns.get(j), row.get_mut(j)) else { continue };
                let cr = cell_rect(k, r);
                if c.states.is_empty() {
                    let edit = egui::TextEdit::singleline(cell)
                        .frame(egui::Frame::NONE)
                        .margin(vec2(8.0, 0.0))
                        .align(egui::Align2::LEFT_CENTER)
                        .id_salt(("blookup-cell", r, j));
                    ui.put(cr.with_min_x(cr.min.x + 6.0), edit);
                } else {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(cr.shrink2(vec2(4.0, 1.0))), |ui| {
                        flat(ui);
                        craft_ui::ComboBox::from_id_salt(("blookup-state", r, j)).width(cr.width() - 8.0).selected_text(cell.as_str()).show_ui(
                            ui,
                            |ui| {
                                for s in &c.states {
                                    if ui.selectable_label(cell == s, s.as_str()).clicked() {
                                        *cell = s.clone();
                                    }
                                }
                            },
                        );
                    });
                }
            }
            if r < n {
                if let Some(old) = st.rows.get_mut(r) {
                    *old = row;
                }
            } else if row.iter().any(|c| !c.trim().is_empty()) {
                st.rows.push(row);
            }
        }

        // What references show when no row matches: <Unmatched> under the inputs, each lookup's
        // value (editable) under the lookups; then whether each lookup can be set backwards.
        if numbers {
            let ur = egui::Rect::from_min_max(egui::pos2(rect.left() + num_w, row_y(n + 1)), egui::pos2(rect.right(), row_y(n + 2)));
            let italic = egui::FontId::new(13.0, egui::FontFamily::Proportional);
            p.text(ur.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, "<Unmatched>", italic, t.text_dim);
            return;
        }
        let repeats: Vec<bool> = cols.clone().map(|j| st.repeats(j)).collect();
        for (k, j) in cols.clone().enumerate() {
            let Some(c) = st.columns.get_mut(j) else { continue };
            let edit = egui::TextEdit::singleline(&mut c.unmatched)
                .frame(egui::Frame::NONE)
                .margin(vec2(8.0, 0.0))
                .align(egui::Align2::LEFT_CENTER)
                .id_salt(("blookup-unmatched", j));
            let ur = cell_rect(k, n + 1);
            ui.put(ur.with_min_x(ur.min.x + 6.0), edit);
            let can = !repeats.get(k).copied().unwrap_or(true);
            let shown = if c.reverse && can { crate::tl!("Allow reverse lookup") } else { crate::tl!("Read only") };
            let rr = cell_rect(k, n + 2);
            ui.scope_builder(egui::UiBuilder::new().max_rect(rr.shrink2(vec2(4.0, 1.0))), |ui| {
                flat(ui);
                ui.add_enabled_ui(can, |ui| {
                    craft_ui::ComboBox::from_id_salt(("blookup-reverse", j)).width(rr.width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
                        if ui.selectable_label(c.reverse, crate::tl!("Allow reverse lookup")).clicked() {
                            c.reverse = true;
                        }
                        if ui.selectable_label(!c.reverse, crate::tl!("Read only")).clicked() {
                            c.reverse = false;
                        }
                    });
                })
                .response
                .on_disabled_hover_text(crate::tl!("Read only while lookup values repeat"));
            });
        }
    });
}

/// A list inside a grid cell: no button background until hovered, so it reads as the cell.
fn flat(ui: &mut egui::Ui) {
    let w = &mut ui.style_mut().visuals.widgets;
    w.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::NONE;
}

/// The properties the Add Parameter Properties dialog lists: (name, type shown, column).
fn addable(st: &State, m: &DynamicBlock, lookup: bool) -> Vec<(String, String, Column)> {
    let has = |node: i32, property: &str| st.columns.iter().any(|c| c.parameter == node && c.property.eq_ignore_ascii_case(property));
    if lookup {
        // Lookup parameters no table gives yet.
        let taken = |node: i32| {
            m.actions.iter().any(|a| matches!(&a.kind, ActionKind::Lookup { columns, .. } if columns.iter().any(|c| c.lookup && c.node == node)))
        };
        return m
            .parameters
            .iter()
            .filter_map(|p| match &p.kind {
                ParameterKind::Lookup { label, .. } if !has(p.node, "lookupString") && !taken(p.node) => Some((
                    label.clone(),
                    p.element.clone(),
                    Column { name: label.clone(), parameter: p.node, property: "lookupString".into(), lookup: true, ..Column::default() },
                )),
                _ => None,
            })
            .collect();
    }
    input_properties(m)
        .into_iter()
        .filter(|(_, node, prop)| !has(*node, prop))
        .map(|(name, node, property)| {
            let kind = match m.parameters.iter().find(|p| p.node == node).map(|p| &p.kind) {
                Some(ParameterKind::Linear { .. }) => crate::i18n::t("Linear"),
                Some(ParameterKind::Point { .. }) => crate::i18n::t("Point"),
                _ => crate::i18n::t("Visibility State"),
            };
            let states = states_of(m, node, &property);
            (name.clone(), kind.to_string(), Column { name, parameter: node, property, states, ..Column::default() })
        })
        .collect()
}

/// The Add Parameter Properties dialog: input or lookup properties to tick and add.
fn add_dialog(ctx: &egui::Context, st: &mut State, m: &DynamicBlock) {
    let Some(mut adding) = st.adding.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    let list = addable(st, m, adding.lookup);
    egui::Window::new(crate::tl!("Add Parameter Properties"))
        .id(egui::Id::unique("Add Parameter Properties"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(crate::tl!("Parameter properties:"));
            egui::Frame::new().stroke(Stroke::new(1.0, Tokens::get().border)).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_height(160.0);
                egui::Grid::new("blookup-add").striped(true).show(ui, |ui| {
                    ui.label(RichText::new(crate::tl!("Name")).strong());
                    ui.label(RichText::new(crate::tl!("Type")).strong());
                    ui.end_row();
                    for (name, kind, _) in &list {
                        let mut on = adding.ticked.contains(name);
                        if ui.checkbox(&mut on, name.as_str()).changed() {
                            if on {
                                adding.ticked.push(name.clone());
                            } else {
                                adding.ticked.retain(|n| n != name);
                            }
                        }
                        ui.label(kind.as_str());
                        ui.end_row();
                    }
                });
            });
            ui.group(|ui| {
                ui.label(crate::tl!("Property type"));
                let before = adding.lookup;
                ui.radio_value(&mut adding.lookup, false, crate::tl!("Add input properties"));
                ui.radio_value(&mut adding.lookup, true, crate::tl!("Add lookup properties"));
                if adding.lookup != before {
                    adding.ticked.clear();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(crate::tl!("OK")).clicked() {
                        ok = true;
                    }
                });
            });
        });
    if ok {
        for (name, _, column) in list {
            if adding.ticked.contains(&name) {
                st.add_column(column);
            }
        }
        return;
    }
    if cancel || !open {
        return;
    }
    st.adding = Some(adding);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app in the Block Editor on block "B" (a rectangle and a circle) with Distance1,
    /// Position1, Visibility1 (VisibilityState0, Small), lookup parameter Size and its action.
    fn editing() -> CadApp {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        let s = &mut app.session;
        s.execute("new", &Value::Null).unwrap();
        let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
        let r = made(s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 5] })).unwrap());
        let c = made(s.execute("circle", &json!({ "center": [10, 2.5], "radius": 1 })).unwrap());
        s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [r, c], "keep": "delete" })).unwrap();
        s.execute("bedit", &json!({ "name": "B" })).unwrap();
        s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0] })).unwrap();
        s.execute("bparameter", &json!({ "type": "point", "at": [10, 2.5] })).unwrap();
        s.execute("bparameter", &json!({ "type": "visibility", "at": [-2, 5] })).unwrap();
        s.execute("bvstate", &json!({ "new": "Small", "visibility": "show" })).unwrap();
        s.execute("bparameter", &json!({ "type": "lookup", "at": [-2, 7], "label": "Size" })).unwrap();
        app
    }

    #[test]
    fn blookuptable_opens_the_dialog_and_ok_writes_the_table_in_one_step() {
        let mut app = editing();
        assert!(route(&mut app, "blookuptable", &Value::Null).is_none(), "no lookup action: BLOOKUPTABLE says why");
        let act = app.session.execute("baction", &json!({ "parameter": "Size" })).unwrap()["node"].as_i64().unwrap() as i32;
        assert!(!open(&mut app, 1), "a parameter isn't a lookup action");
        assert!(open(&mut app, act), "a double-click on the lookup action opens its table");
        app.ui.dialog = None;
        assert!(route(&mut app, "blookuptable", &json!({})).is_none(), "JSON calls skip the dialog");
        assert!(route(&mut app, "blookuptable", &Value::Null).is_some());
        assert_eq!(app.ui.dialog.as_deref(), Some("blookuptable"));
        let mut st = app.blookup.clone();
        assert_eq!(st.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Size"]);
        let st_m = dynamic_block(&app.session.state().unwrap().doc, "B").unwrap();
        let inputs = addable(&st, &st_m, false);
        assert_eq!(
            inputs.iter().map(|(n, k, _)| (n.as_str(), k.as_str())).collect::<Vec<_>>(),
            [("Distance1", "Linear"), ("Position1 X", "Point"), ("Position1 Y", "Point"), ("Visibility1", "Visibility State")]
        );
        for (_, _, c) in inputs.into_iter().filter(|(n, ..)| n != "Position1 Y") {
            st.add_column(c);
        }
        assert_eq!(st.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Distance1", "Position1 X", "Visibility1", "Size"]);
        assert_eq!(st.columns[2].states, ["VisibilityState0", "Small"]);
        st.rows = vec![
            ["10", "10", "VisibilityState0", "Big"].map(String::from).to_vec(),
            ["6", "6", "Small", "Small"].map(String::from).to_vec(),
            ["", "", "", ""].map(String::from).to_vec(),
        ];
        st.columns[3].reverse = true;
        st.columns[3].unmatched = "Other".into();
        let block = "B";
        st.audit(&app.session.state().unwrap().doc, block).unwrap();
        let before = app.session.state().unwrap().undo.len();
        app.run("blookuptable", st.params()).unwrap();
        assert_eq!(app.session.state().unwrap().undo.len(), before + 1, "one undo step");
        let t = app.session.execute("blookuptable", &json!({})).unwrap();
        assert_eq!(t["rows"], json!([["10", "10", "VisibilityState0", "Big"], ["6", "6", "Small", "Small"]]));
        assert_eq!((t["columns"][3]["unmatched"].clone(), t["columns"][3]["reverse"].clone()), (json!("Other"), json!(true)));
        // Opened again, the dialog shows what was written.
        route(&mut app, "blookuptable", &Value::Null).unwrap().unwrap();
        assert_eq!(app.blookup.rows.len(), 2);
        assert!(app.blookup.columns[3].reverse);
    }

    #[test]
    fn audit_and_reverse_lookup_follow_autocads_rules() {
        let mut app = editing();
        app.session.execute("baction", &json!({ "parameter": "Size" })).unwrap();
        route(&mut app, "blookuptable", &Value::Null).unwrap().unwrap();
        let mut st = app.blookup.clone();
        let m = dynamic_block(&app.session.state().unwrap().doc, "B").unwrap();
        let d1 = addable(&st, &m, false).into_iter().next().unwrap().2;
        st.add_column(d1);
        st.rows = vec![vec!["10".into(), "Big".into()], vec!["10.0".into(), "Small".into()]];
        let doc = app.session.state().unwrap().doc.clone();
        assert_eq!(st.audit(&doc, "B").unwrap_err(), "Duplicate row of input values found.");
        st.rows = vec![vec!["10".into(), "Big".into()], vec!["6".into(), "big".into()]];
        assert!(st.repeats(1), "repeated lookup values: read only");
        st.columns[1].reverse = true;
        assert_eq!(st.params()["reverse"], json!([]));
        // The action's own lookup column stays; an input can go, its cells with it.
        assert!(!st.removable(1));
        st.remove_column(1);
        assert_eq!(st.columns.len(), 2);
        st.remove_column(0);
        assert_eq!(st.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Size"]);
        assert_eq!(st.rows, [vec!["Big".to_string()], vec!["big".to_string()]]);
        // Lookup parameters another table gives aren't offered as lookup columns.
        app.session.execute("bparameter", &json!({ "type": "lookup", "at": [-2, 9] })).unwrap();
        let m = dynamic_block(&app.session.state().unwrap().doc, "B").unwrap();
        assert_eq!(
            addable(&st, &m, true).iter().map(|(n, k, _)| (n.as_str(), k.as_str())).collect::<Vec<_>>(),
            [("Lookup1", "Lookup2")],
            "label, then its name in the editor"
        );
    }
}
