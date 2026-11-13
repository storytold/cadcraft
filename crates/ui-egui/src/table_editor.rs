//! The table editor: a double-click on a table cell selects cells instead of editing their text
//! (a second double-click edits the text of the cell). Selected cells take rows and columns
//! inserted and deleted, merges, alignment, sizes typed or fitted to their text, and their
//! properties in the Properties palette; dragging a border between rows or columns resizes them.
//! Every change runs a `table.*` command.

use cadcraft_doc::{EntityKind, Handle, Table};
use cadcraft_geom::Vec2;
use egui::{Pos2, Rect, Stroke, pos2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// The cells selected in a table: from `anchor` to `focus` (row, column), both included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableEdit {
    pub handle: Handle,
    pub anchor: (usize, usize),
    pub focus: (usize, usize),
    drag: Option<Drag>,
}

/// What a drag in the edited table does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// Selects cells from the anchor to the cell under the pointer.
    Select,
    /// Moves the right border of this column.
    Column(usize),
    /// Moves the bottom border of this row.
    Row(usize),
}

impl TableEdit {
    pub fn new(handle: Handle, cell: (usize, usize)) -> Self {
        TableEdit { handle, anchor: cell, focus: cell, drag: None }
    }

    /// First row, first column, last row, last column.
    pub fn range(&self) -> (usize, usize, usize, usize) {
        (self.anchor.0.min(self.focus.0), self.anchor.1.min(self.focus.1), self.anchor.0.max(self.focus.0), self.anchor.1.max(self.focus.1))
    }

    /// The selection as a `table.*` range: row, rows, col, cols.
    pub fn params(&self) -> Value {
        let (r0, c0, r1, c1) = self.range();
        json!({ "handle": self.handle.hex(), "row": r0, "rows": r1 - r0 + 1, "col": c0, "cols": c1 - c0 + 1 })
    }

    /// The selection's name, as formulas name cells (`A1`, `A1:C3`).
    pub fn name(&self) -> String {
        let (r0, c0, r1, c1) = self.range();
        let cell = |r: usize, c: usize| cadcraft_engine::cmd::table_cell_name(r, c);
        if (r0, c0) == (r1, c1) { cell(r0, c0) } else { format!("{}:{}", cell(r0, c0), cell(r1, c1)) }
    }
}

/// The table being edited, with its editor state kept inside it (rows and columns that went
/// away are left out of the selection); `None` closes the editor.
pub fn current(app: &mut CadApp) -> Option<Table> {
    let edit = app.table_edit.as_mut()?;
    let table = app.session.doc().ok().and_then(|d| match &d.entity(edit.handle)?.kind {
        EntityKind::Table(t) => Some(t.clone()),
        _ => None,
    });
    let Some(t) = table.filter(|t| !t.row_heights.is_empty() && !t.col_widths.is_empty()) else {
        app.table_edit = None;
        return None;
    };
    let (rows, cols) = (t.row_heights.len(), t.col_widths.len());
    for cell in [&mut edit.anchor, &mut edit.focus] {
        *cell = (cell.0.min(rows - 1), cell.1.min(cols - 1));
    }
    Some(t)
}

pub fn active(app: &CadApp) -> bool {
    app.table_edit.is_some()
}

/// Opens the editor on `cell` of table `h`, or edits the cell's text when the editor is already
/// open on that table (the second double-click). Whether the double-click was used.
pub(crate) fn double_click(app: &mut CadApp, h: Handle, cell: (usize, usize), p: Vec2) -> bool {
    if app.table_edit.as_ref().is_some_and(|e| e.handle == h) {
        app.table_edit = None;
        edit_text(app, p);
        return true;
    }
    app.table_edit = Some(TableEdit::new(h, cell));
    app.session.set_selection(Vec::new());
    true
}

/// Starts editing the text of the cell at `p` (TABLEDIT).
fn edit_text(app: &mut CadApp, p: Vec2) {
    app.start("tabledit.cell");
    if let Err(e) = app.session.input(cadcraft_engine::Input::Point(p)) {
        app.session.echo(e.to_string());
    }
}

/// A click while the editor is open: a cell of the edited table selects it (Shift: up to it);
/// anywhere else closes the editor and the click does what it always does. Whether it was used.
pub(crate) fn click(app: &mut CadApp, p: Vec2, shift: bool) -> bool {
    let Some(t) = current(app) else { return false };
    let Some(edit) = app.table_edit.as_mut() else { return false };
    match t.cell_at_point(p) {
        Some(cell) => {
            edit.focus = cell;
            if !shift {
                edit.anchor = cell;
            }
            true
        }
        None => {
            app.table_edit = None;
            false
        }
    }
}

/// How close to a border, in screen points, a drag resizes.
const BORDER_GRAB: f32 = 4.0;

/// The border under screen point `at`: the column whose right edge or the row whose bottom edge
/// it is.
fn border_at(t: &Table, xf: &Xf, at: Pos2) -> Option<Drag> {
    let p = xf.to_world(at);
    let tol = f64::from(BORDER_GRAB) / xf.scale.max(1e-12);
    for part in t.parts() {
        let h: f64 = part.rows.iter().map(|r| t.row_heights.get(*r).copied().unwrap_or(0.0).max(0.0)).sum();
        let w: f64 = t.col_widths.iter().map(|w| w.max(0.0)).sum();
        let (x, y) = (p.x - part.origin.x, part.origin.y - p.y);
        if !(-tol..=w + tol).contains(&x) || !(-tol..=h + tol).contains(&y) {
            continue;
        }
        let mut acc = 0.0;
        for (c, cw) in t.col_widths.iter().enumerate() {
            acc += cw.max(0.0);
            if (x - acc).abs() <= tol {
                return Some(Drag::Column(c));
            }
        }
        let mut acc = 0.0;
        for r in &part.rows {
            acc += t.row_heights.get(*r).copied().unwrap_or(0.0).max(0.0);
            if (y - acc).abs() <= tol {
                return Some(Drag::Row(*r));
            }
        }
    }
    None
}

/// Whether a drag starting at screen point `origin` belongs to the editor (a border or a cell of
/// the edited table): then it opens no selection window.
pub(crate) fn owns(app: &mut CadApp, xf: &Xf, origin: Pos2) -> bool {
    let Some(t) = current(app) else { return false };
    border_at(&t, xf, origin).is_some() || t.cell_at_point(xf.to_world(origin)).is_some()
}

/// Drags in the edited table: across cells selects them, on a border moves it (applied when the
/// button is let go). The resize cursor shows over a border.
pub(crate) fn drag(app: &mut CadApp, ui: &egui::Ui, resp: &egui::Response, xf: &Xf) {
    let Some(t) = current(app) else { return };
    let hover = ui.input(|i| i.pointer.hover_pos());
    let dragging = app.table_edit.as_ref().and_then(|e| e.drag);
    let over = dragging.or_else(|| hover.and_then(|h| border_at(&t, xf, h)));
    match over {
        Some(Drag::Column(_)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
        Some(Drag::Row(_)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical),
        _ => {}
    }
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let start = border_at(&t, xf, origin).or_else(|| {
            let cell = t.cell_at_point(xf.to_world(origin))?;
            let edit = app.table_edit.as_mut()?;
            edit.anchor = cell;
            edit.focus = cell;
            Some(Drag::Select)
        });
        if let Some(edit) = app.table_edit.as_mut() {
            edit.drag = start;
        }
    }
    let Some(d) = app.table_edit.as_ref().and_then(|e| e.drag) else { return };
    let pointer = ui.input(|i| i.pointer.interact_pos()).map(|p| xf.to_world(p));
    if d == Drag::Select
        && let Some(cell) = pointer.and_then(|p| t.cell_at_point(p))
        && let Some(edit) = app.table_edit.as_mut()
    {
        edit.focus = cell;
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) || !ui.input(|i| i.pointer.primary_down()) {
        if let Some(edit) = app.table_edit.as_mut() {
            edit.drag = None;
        }
        let Some(p) = pointer else { return };
        let handle = t_handle(app);
        let run = match d {
            Drag::Column(c) => t.cell_rect(0, c).map(|(o, _, _)| ("width", c, p.x - o.x)),
            Drag::Row(r) => t.cell_rect(r, 0).map(|(o, _, _)| ("height", r, o.y - p.y)),
            Drag::Select => None,
        };
        if let (Some((key, i, size)), Some(h)) = (run, handle) {
            let size = size.max(t.text_height * 0.5).max(1e-6);
            let at = if key == "width" { "col" } else { "row" };
            let r = app.run("table.size", json!({ "handle": h.hex(), at: i, key: size }));
            report(app, r);
        }
    }
}

fn t_handle(app: &CadApp) -> Option<Handle> {
    app.table_edit.as_ref().map(|e| e.handle)
}

fn report(app: &mut CadApp, r: Result<Value, String>) {
    if let Err(e) = r {
        app.session.echo(e);
    }
}

/// Keys while the editor is open (no command running, no text field typing): arrows move the
/// selection (Shift: extend it), Tab and Shift+Tab move across, Ctrl+A selects every cell, Enter
/// or F2 edits the text of the cell, Delete empties the cells, Escape closes the editor.
pub(crate) fn keyboard(app: &mut CadApp, ctx: &egui::Context) {
    if app.table_edit.is_none() || ctx.egui_wants_keyboard_input() || app.session.running.is_some() {
        return;
    }
    let Some(t) = current(app) else { return };
    let (rows, cols) = (t.row_heights.len(), t.col_widths.len());
    let mut used = Vec::new();
    let events = ctx.input(|i| i.events.clone());
    let mut edit_cell = false;
    let mut clear = false;
    for (n, ev) in events.iter().enumerate() {
        let egui::Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
        let Some(edit) = app.table_edit.as_mut() else { return };
        let step = |v: usize, d: isize, n: usize| v.saturating_add_signed(d).min(n.saturating_sub(1));
        let mv = |edit: &mut TableEdit, dr: isize, dc: isize, extend: bool| {
            edit.focus = (step(edit.focus.0, dr, rows), step(edit.focus.1, dc, cols));
            if !extend {
                edit.anchor = edit.focus;
            }
        };
        match key {
            egui::Key::ArrowUp => mv(edit, -1, 0, modifiers.shift),
            egui::Key::ArrowDown => mv(edit, 1, 0, modifiers.shift),
            egui::Key::ArrowLeft => mv(edit, 0, -1, modifiers.shift),
            egui::Key::ArrowRight => mv(edit, 0, 1, modifiers.shift),
            egui::Key::Tab => mv(edit, 0, if modifiers.shift { -1 } else { 1 }, false),
            egui::Key::A if modifiers.command => {
                edit.anchor = (0, 0);
                edit.focus = (rows - 1, cols - 1);
            }
            egui::Key::Enter | egui::Key::F2 => edit_cell = true,
            egui::Key::Delete | egui::Key::Backspace => clear = true,
            egui::Key::Escape => app.table_edit = None,
            _ => continue,
        }
        used.push(n);
    }
    if used.is_empty() {
        return;
    }
    // The keys used here don't also reach the command line (Escape, Enter, Delete…).
    ctx.input_mut(|i| {
        let mut n = 0;
        i.events.retain(|ev| {
            let keep = !used.contains(&n) && !matches!(ev, egui::Event::Text(s) if s == "\t");
            n += 1;
            keep
        });
    });
    if clear && let Some(p) = app.table_edit.as_ref().map(TableEdit::params) {
        let mut p = p;
        p["clear"] = json!(true);
        let r = app.run("table.cells", p);
        report(app, r);
    }
    if edit_cell
        && let Some(edit) = app.table_edit.take()
        && let Some((o, w, h)) = t.cell_rect(edit.focus.0, edit.focus.1)
    {
        edit_text(app, o + Vec2::new(w / 2.0, -h / 2.0));
    }
}

/// The selected cells' outline and wash, and the editor's bar above the table.
pub(crate) fn paint(app: &mut CadApp, ui: &mut egui::Ui, painter: &egui::Painter, xf: &Xf) {
    let Some(t) = current(app) else { return };
    let Some(edit) = app.table_edit.clone() else { return };
    let tk = Tokens::get();
    let (r0, c0, r1, c1) = edit.range();
    let mut sel = Rect::NOTHING;
    for r in r0..=r1 {
        for c in c0..=c1 {
            if let Some((o, w, h)) = t.cell_rect(r, c) {
                let cell = Rect::from_two_pos(xf.to_screen(o), xf.to_screen(o + Vec2::new(w, -h)));
                painter.rect_filled(cell, 0.0, tk.accent.gamma_multiply(0.18));
                sel = sel.union(cell);
            }
        }
    }
    if sel.is_positive() {
        painter.rect_stroke(sel, 0.0, Stroke::new(2.0, tk.accent), egui::StrokeKind::Middle);
    }
    // While a border is dragged: where it goes.
    if let Some(d @ (Drag::Column(_) | Drag::Row(_))) = edit.drag
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        let b = t.bounds();
        let (a, z) = (xf.to_screen(Vec2::new(b.min.x, b.max.y)), xf.to_screen(Vec2::new(b.max.x, b.min.y)));
        let s = Stroke::new(1.0, tk.accent);
        if matches!(d, Drag::Column(_)) {
            painter.vline(p.x, a.y.min(z.y)..=a.y.max(z.y), s);
        } else {
            painter.hline(a.x.min(z.x)..=a.x.max(z.x), p.y, s);
        }
    }
    let b = t.bounds();
    let top_left = xf.to_screen(Vec2::new(b.min.x, b.max.y));
    bar(app, ui.ctx(), pos2(top_left.x, top_left.y - 6.0), &t);
}

/// Alignments in the bar's menu: (code, name), as the cell styles name them.
const ALIGNMENTS: [(&str, &str); 9] = [
    ("TL", "Top left"),
    ("TC", "Top center"),
    ("TR", "Top right"),
    ("ML", "Middle left"),
    ("MC", "Middle center"),
    ("MR", "Middle right"),
    ("BL", "Bottom left"),
    ("BC", "Bottom center"),
    ("BR", "Bottom right"),
];

/// The table editor's bar, its bottom left corner at `at` (above the table).
fn bar(app: &mut CadApp, ctx: &egui::Context, at: Pos2, t: &Table) {
    let Some(edit) = app.table_edit.clone() else { return };
    let (r0, c0, r1, c1) = edit.range();
    let (rows, cols) = (r1 - r0 + 1, c1 - c0 + 1);
    let h = edit.handle.hex();
    let mut action: Option<(&str, Value)> = None;
    let mut close = false;
    let mut edit_cell = false;
    egui::Area::new(egui::Id::unique("cc_table_editor"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(at)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(edit.name()).strong());
                    ui.separator();
                    let b = |ui: &mut egui::Ui, label: &str, tip: &str| ui.button(crate::i18n::t(label)).on_hover_text(crate::i18n::t(tip)).clicked();
                    if b(ui, "Row ↑", "Insert a row above") {
                        action = Some(("table.insertrow", json!({ "handle": h, "row": r0 })));
                    }
                    if b(ui, "Row ↓", "Insert a row below") {
                        action = Some(("table.insertrow", json!({ "handle": h, "row": r1 + 1 })));
                    }
                    if b(ui, "Column ←", "Insert a column to the left") {
                        action = Some(("table.insertcol", json!({ "handle": h, "col": c0 })));
                    }
                    if b(ui, "Column →", "Insert a column to the right") {
                        action = Some(("table.insertcol", json!({ "handle": h, "col": c1 + 1 })));
                    }
                    if b(ui, "Delete rows", "Delete the selected rows") {
                        action = Some(("table.deleterow", json!({ "handle": h, "row": r0, "rows": rows })));
                    }
                    if b(ui, "Delete columns", "Delete the selected columns") {
                        action = Some(("table.deletecol", json!({ "handle": h, "col": c0, "cols": cols })));
                    }
                    ui.separator();
                    if b(ui, "Merge", "Merge the selected cells") {
                        action = Some(("table.merge", json!({ "handle": h, "row": r0, "col": c0, "rows": rows, "cols": cols })));
                    }
                    if b(ui, "Unmerge", "Unmerge the cell") {
                        action = Some(("table.unmerge", json!({ "handle": h, "row": edit.focus.0, "col": edit.focus.1 })));
                    }
                    ui.menu_button(crate::i18n::t("Align"), |ui| {
                        for (code, name) in ALIGNMENTS {
                            if ui.button(crate::i18n::t(name)).clicked() {
                                let mut p = edit.params();
                                p["align"] = json!(code);
                                action = Some(("table.cells", p));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button(crate::i18n::t("By cell style")).clicked() {
                            let mut p = edit.params();
                            p["align"] = Value::Null;
                            action = Some(("table.cells", p));
                            ui.close();
                        }
                    });
                    ui.separator();
                    if b(ui, "Fit width", "Columns as wide as their text") {
                        let mut p = edit.params();
                        p["width"] = json!(true);
                        action = Some(("table.fit", p));
                    }
                    if b(ui, "Fit height", "Rows as high as their text") {
                        let mut p = edit.params();
                        p["height"] = json!(true);
                        action = Some(("table.fit", p));
                    }
                    if b(ui, "Equal widths", "The selected columns share their width equally") {
                        let w: f64 = t.col_widths.iter().skip(c0).take(cols).sum::<f64>() / cols as f64;
                        action = Some(("table.size", json!({ "handle": h, "col": c0, "cols": cols, "width": w })));
                    }
                    if b(ui, "Equal heights", "The selected rows share their height equally") {
                        let v: f64 = t.row_heights.iter().skip(r0).take(rows).sum::<f64>() / rows as f64;
                        action = Some(("table.size", json!({ "handle": h, "row": r0, "rows": rows, "height": v })));
                    }
                    ui.separator();
                    if b(ui, "Select all", "Select every cell (Ctrl+A)")
                        && let Some(e) = app.table_edit.as_mut()
                    {
                        e.anchor = (0, 0);
                        e.focus = (t.row_heights.len().saturating_sub(1), t.col_widths.len().saturating_sub(1));
                    }
                    if b(ui, "Edit text", "Edit the text of the cell (Enter, or double-click it)") {
                        edit_cell = true;
                    }
                    if b(ui, "Close", "Close the table editor (Esc)") {
                        close = true;
                    }
                });
            });
        });
    if let Some((id, p)) = action {
        let r = app.run(id, p);
        report(app, r);
    }
    if edit_cell
        && let Some(e) = app.table_edit.take()
        && let Some((o, w, hh)) = t.cell_rect(e.focus.0, e.focus.1)
    {
        edit_text(app, o + Vec2::new(w / 2.0, -hh / 2.0));
    }
    if close {
        app.table_edit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use cadcraft_engine::Session;

    fn app_with_table() -> (CadApp, Handle) {
        let mut app = CadApp::new(Session::new(), Services::default());
        let v = app.run("table", json!({ "at": [0, 0], "rows": 3, "cols": 3, "rowHeight": 1, "colWidth": 5 })).unwrap();
        let h = Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap();
        (app, h)
    }

    fn table(app: &CadApp, h: Handle) -> Table {
        match &app.session.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::Table(t) => t.clone(),
            _ => panic!("not a table"),
        }
    }

    fn centre(t: &Table, r: usize, c: usize) -> Vec2 {
        let (o, w, h) = t.cell_rect(r, c).unwrap();
        o + Vec2::new(w / 2.0, -h / 2.0)
    }

    /// The first double-click on a cell opens the table editor on it; the second edits the
    /// text of the cell.
    #[test]
    fn a_double_click_selects_the_cell_and_a_second_one_edits_its_text() {
        let (mut app, h) = app_with_table();
        let t = table(&app, h);
        let p = centre(&t, 2, 1);
        assert!(double_click(&mut app, h, (2, 1), p));
        assert_eq!(app.table_edit.as_ref().map(TableEdit::range), Some((2, 1, 2, 1)));
        assert!(app.session.running.is_none(), "no text editor yet");
        assert!(double_click(&mut app, h, (2, 1), p));
        assert!(app.table_edit.is_none());
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("tabledit.cell"));
    }

    /// Clicks select cells (Shift: a range); a click away from the table closes the editor.
    #[test]
    fn clicks_select_cells_and_a_click_elsewhere_closes() {
        let (mut app, h) = app_with_table();
        let t = table(&app, h);
        let last = t.row_heights.len() - 1;
        app.table_edit = Some(TableEdit::new(h, (1, 0)));
        assert!(click(&mut app, centre(&t, last, 2), true), "{:?} rows", t.row_heights);
        let e = app.table_edit.clone().unwrap();
        assert_eq!(e.range(), (1, 0, last, 2));
        assert_eq!(e.name(), format!("A2:C{}", last + 1));
        assert!(!click(&mut app, Vec2::new(-100.0, 100.0), false));
        assert!(app.table_edit.is_none());
    }

    /// The selection keys move and grow the selection; Escape closes the editor without
    /// reaching the command line.
    #[test]
    fn keys_move_and_extend_the_selection() {
        let (mut app, h) = app_with_table();
        app.table_edit = Some(TableEdit::new(h, (1, 1)));
        let ctx = egui::Context::default();
        let key = |k: egui::Key, shift: bool| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { shift, ..Default::default() },
        };
        let run = |app: &mut CadApp, events: Vec<egui::Event>| {
            let raw = egui::RawInput { events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| keyboard(app, ui.ctx()));
            out.textures_delta.clear();
        };
        run(&mut app, vec![key(egui::Key::ArrowRight, true), key(egui::Key::ArrowDown, true)]);
        assert_eq!(app.table_edit.as_ref().map(TableEdit::range), Some((1, 1, 2, 2)));
        run(&mut app, vec![key(egui::Key::ArrowLeft, false)]);
        assert_eq!(app.table_edit.as_ref().map(TableEdit::range), Some((2, 1, 2, 1)));
        run(&mut app, vec![key(egui::Key::Escape, false)]);
        assert!(app.table_edit.is_none());
    }

    /// Rows and columns that go away leave the selection inside the table.
    #[test]
    fn the_selection_stays_inside_a_table_that_shrinks() {
        let (mut app, h) = app_with_table();
        let rows = table(&app, h).row_heights.len();
        let mut e = TableEdit::new(h, (0, 0));
        e.focus = (rows - 1, 2);
        app.table_edit = Some(e);
        app.run("table.deleterow", json!({ "handle": h.hex(), "row": 2, "rows": rows - 2 })).unwrap();
        app.run("table.deletecol", json!({ "handle": h.hex(), "col": 1, "cols": 2 })).unwrap();
        assert!(current(&mut app).is_some());
        assert_eq!(app.table_edit.as_ref().map(TableEdit::range), Some((0, 0, 1, 0)));
        app.run("erase", json!({ "handles": [h.hex()] })).unwrap();
        assert!(current(&mut app).is_none());
        assert!(app.table_edit.is_none());
    }
}
