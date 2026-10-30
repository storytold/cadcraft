//! TABLE: create tables and edit cells, rows, columns and merges; cell styles, formulas
//! (`formula`), TABLEDIT and TABLEEXPORT (`cells`).

mod breaks;
mod cells;
pub(crate) mod formula;

pub(crate) use cells::{recalc, tablestyle_cell_style};

use cadcraft_doc::{EntityKind, Handle, Table, TableCell};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::helpers::v3;
use super::{CommandSpec, bad, point_req, size_param, str_param};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Caps for hostile parameters.
const MAX_ROWS: usize = 2000;
const MAX_COLS: usize = 200;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("table", "Table...", run_table)
            .menu(&["Draw", "Table..."])
            .alias(&["tb"])
            .params("{at: [x,y], rows (data rows), cols, rowHeight?, colWidth?, cells?: [[text]], title?: text, header?: [text]}")
            .interactive(|s| Ok(Box::new(TableM::new(s)))),
        CommandSpec::new("table.set", "Set Table Cell", run_set)
            .params("{handle, row, col, text? (text starting with = is a formula), formula?: \"=Sum(A2:A5)\"}"),
        CommandSpec::new("table.insertrow", "Insert Row", run_insert_row).params("{handle, row? (insert before; default: append), height?}"),
        CommandSpec::new("table.deleterow", "Delete Row", run_delete_row).params("{handle, row}"),
        CommandSpec::new("table.insertcol", "Insert Column", run_insert_col).params("{handle, col? (insert before; default: append), width?}"),
        CommandSpec::new("table.deletecol", "Delete Column", run_delete_col).params("{handle, col}"),
        CommandSpec::new("table.merge", "Merge Cells", run_merge).params("{handle, row, col, rows, cols}"),
        CommandSpec::new("table.unmerge", "Unmerge Cells", run_unmerge).params("{handle, row, col}"),
    ]
    .into_iter()
    .chain(cells::specs())
    .chain(breaks::specs())
    .collect()
}

fn usize_param(p: &Value, key: &str) -> Option<usize> {
    p.get(key).and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok())
}

fn style_of(s: &Session) -> cadcraft_doc::TableStyle {
    let Ok(d) = s.doc() else { return Default::default() };
    let cur = d.header.str("CTABLESTYLE", "Standard");
    d.table_styles.iter().find(|t| t.name.eq_ignore_ascii_case(&cur)).or(d.table_styles.first()).cloned().unwrap_or_default()
}

/// Build a table entity. `rows` data rows; a title row and a header row are added when given.
#[allow(clippy::too_many_arguments)]
pub fn make_table(
    st: &cadcraft_doc::TableStyle,
    at: Vec2,
    rows: usize,
    cols: usize,
    row_h: Option<f64>,
    col_w: Option<f64>,
    title: Option<&str>,
    header: Option<Vec<String>>,
    cells: &[Vec<String>],
) -> Table {
    let cols = cols.clamp(1, MAX_COLS);
    let rows = rows.clamp(if title.is_some() || header.is_some() { 0 } else { 1 }, MAX_ROWS);
    let th = st.text_height;
    let data_h = row_h.unwrap_or(th * 5.0 / 3.0 + 2.0 * st.margin);
    let mut t = Table {
        insert: v3(at),
        col_widths: vec![col_w.unwrap_or(2.5); cols],
        row_heights: Vec::new(),
        cells: Vec::new(),
        style: st.name.clone(),
        text_height: th,
        title: title.is_some(),
        header: header.is_some(),
        ..Table::default()
    };
    let empty_row = || vec![TableCell::default(); cols];
    if let Some(ti) = title {
        let mut r = empty_row();
        if let Some(c) = r.first_mut() {
            c.text = ti.to_string();
            c.merged = Some((1, cols as u32));
        }
        t.cells.push(r);
        t.row_heights.push(th * 1.4 * 5.0 / 3.0 + 2.0 * st.margin);
    }
    if let Some(h) = header {
        let mut r = empty_row();
        for (c, txt) in r.iter_mut().zip(h) {
            c.text = txt;
        }
        t.cells.push(r);
        t.row_heights.push(data_h);
    }
    for i in 0..rows {
        let mut r = empty_row();
        if let Some(src) = cells.get(i) {
            for (c, txt) in r.iter_mut().zip(src) {
                c.text = txt.clone();
            }
        }
        t.cells.push(r);
        t.row_heights.push(data_h);
    }
    t
}

fn run_table(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("table", p, "at")?;
    let cells: Vec<Vec<String>> = p
        .get("cells")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .take(MAX_ROWS)
                .map(|r| {
                    r.as_array()
                        .map(|a| a.iter().take(MAX_COLS).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let rows = usize_param(p, "rows").unwrap_or(cells.len().max(1));
    let cols = usize_param(p, "cols").unwrap_or_else(|| cells.iter().map(Vec::len).max().unwrap_or(1).max(1));
    if rows > MAX_ROWS || cols > MAX_COLS || cols == 0 {
        return Err(bad("table", format!("rows must be ≤ {MAX_ROWS} and columns 1..={MAX_COLS}")));
    }
    let row_height = size_param("table", p, "rowHeight", false)?;
    let col_width = size_param("table", p, "colWidth", false)?;
    let header: Option<Vec<String>> =
        p.get("header").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect());
    let st = style_of(s);
    let t = make_table(&st, at, rows, cols, row_height, col_width, str_param(p, "title"), header, &cells);
    let h = s.add_entity(EntityKind::Table(t))?;
    Ok(json!({ "handle": h.hex() }))
}

/// The table `handle`, or else the selected table. A supplied `handle` that can't be read is an
/// error, never a silent fall back to the selection (like `targets`).
fn table_handle(s: &Session, p: &Value, id: &str) -> Result<Handle> {
    let explicit = match p.get("handle") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(Handle::parse_hex)
                .or_else(|| v.as_u64().map(Handle))
                .ok_or_else(|| bad(id, format!("`handle`: {v} is not an object handle (a hex string such as \"1A2\")")))?,
        ),
    };
    explicit
        .or_else(|| {
            s.selection().into_iter().find(|h| s.doc().ok().and_then(|d| d.entity(*h)).is_some_and(|e| matches!(e.kind, EntityKind::Table(_))))
        })
        .ok_or_else(|| bad(id, "`handle` of a table is required"))
}

/// Modify the table `handle` with `f`.
fn edit(s: &mut Session, p: &Value, id: &str, f: impl FnOnce(&mut Table) -> std::result::Result<(), String>) -> Result<Value> {
    let h = table_handle(s, p, id)?;
    let mut t = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Table(t)) => t.clone(),
        _ => return Err(bad(id, "object is not a table")),
    };
    normalize(&mut t);
    f(&mut t).map_err(|m| bad(id, m))?;
    let st = cadcraft_render::table_style_of(s.doc()?, &t);
    formula::recalc(&mut t, &st);
    s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::Table(t))?;
    Ok(json!({ "handle": h.hex() }))
}

/// Make the cell grid rows × cols.
fn normalize(t: &mut Table) {
    let rows = t.row_heights.len().min(MAX_ROWS);
    let cols = t.col_widths.len().min(MAX_COLS);
    t.row_heights.truncate(rows);
    t.col_widths.truncate(cols);
    t.cells.resize_with(rows, Vec::new);
    for r in &mut t.cells {
        r.resize_with(cols, TableCell::default);
    }
}

/// Merge anchors: (row, col, rows, cols).
fn anchors(t: &Table) -> Vec<(usize, usize, usize, usize)> {
    let mut out = Vec::new();
    for (r, row) in t.cells.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if let Some((rs, cs)) = cell.merged {
                out.push((r, c, (rs as usize).max(1), (cs as usize).max(1)));
            }
        }
    }
    out
}

fn set_anchors(t: &mut Table, a: &[(usize, usize, usize, usize)]) {
    for row in &mut t.cells {
        for c in row.iter_mut() {
            c.merged = None;
        }
    }
    let (rows, cols) = (t.cells.len(), t.col_widths.len());
    for &(r, c, rs, cs) in a {
        let (rs, cs) = (rs.min(rows.saturating_sub(r)), cs.min(cols.saturating_sub(c)));
        if (rs > 1 || cs > 1)
            && let Some(cell) = t.cells.get_mut(r).and_then(|row| row.get_mut(c))
        {
            cell.merged = Some((rs as u32, cs as u32));
        }
    }
}

fn cell_at(p: &Value, t: &Table) -> std::result::Result<(usize, usize), String> {
    let r = usize_param(p, "row").ok_or("`row` is required")?;
    let c = usize_param(p, "col").ok_or("`col` is required")?;
    if r >= t.cells.len() || c >= t.col_widths.len() {
        return Err(format!("cell ({r}, {c}) is outside the {}×{} table", t.cells.len(), t.col_widths.len()));
    }
    Ok((r, c))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("").to_string();
    // A formula: the `formula` parameter, or text starting with "=".
    let formula = match str_param(p, "formula") {
        Some(f) => Some(f.trim().to_string()),
        None => text.trim().starts_with('=').then(|| text.trim().to_string()),
    };
    if let Some(f) = &formula {
        let f = if f.starts_with('=') { f.clone() } else { format!("={f}") };
        formula::check(&f).map_err(|e| bad("table.set", format!("{e}: not a formula CADCraft can evaluate: {f}")))?;
    }
    edit(s, p, "table.set", |t| {
        let (r, c) = cell_at(p, t)?;
        // Writing into a covered cell writes its merge anchor.
        let (r, c) = anchors(t)
            .into_iter()
            .find(|&(ar, ac, rs, cs)| r >= ar && r < ar + rs && c >= ac && c < ac + cs)
            .map(|(ar, ac, ..)| (ar, ac))
            .unwrap_or((r, c));
        if let Some(cell) = t.cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            match formula {
                Some(f) => cell.formula = Some(if f.starts_with('=') { f } else { format!("={f}") }),
                None => {
                    cell.formula = None;
                    cell.text = text;
                }
            }
        }
        Ok(())
    })
}

/// Move the references of every formula in `t` for a row/column insertion or deletion.
fn shift_formulas(t: &mut Table, shift: formula::Shift) {
    for cell in t.cells.iter_mut().flatten() {
        if let Some(f) = &mut cell.formula {
            *f = formula::shift_refs(f, shift);
        }
    }
}

fn run_insert_row(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.insertrow", |t| {
        if t.cells.len() >= MAX_ROWS {
            return Err("too many rows".into());
        }
        let r = usize_param(p, "row").unwrap_or(t.cells.len()).min(t.cells.len());
        let h = p.get("height").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0);
        let h = h.or_else(|| t.row_heights.last().copied()).unwrap_or(t.text_height * 5.0 / 3.0);
        let a: Vec<_> = anchors(t)
            .into_iter()
            .map(|(ar, ac, rs, cs)| {
                if ar >= r {
                    (ar + 1, ac, rs, cs)
                } else if r < ar + rs {
                    (ar, ac, rs + 1, cs)
                } else {
                    (ar, ac, rs, cs)
                }
            })
            .collect();
        t.row_heights.insert(r, h);
        t.cells.insert(r, vec![TableCell::default(); t.col_widths.len()]);
        if r < t.row_styles.len() {
            t.row_styles.insert(r, String::new());
        }
        set_anchors(t, &a);
        shift_formulas(t, formula::Shift::InsertRow(r));
        Ok(())
    })
}

fn run_delete_row(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.deleterow", |t| {
        let r = usize_param(p, "row").ok_or("`row` is required")?;
        if r >= t.cells.len() || t.cells.len() <= 1 {
            return Err("no such row (a table keeps at least one row)".into());
        }
        let mut a = Vec::new();
        for (ar, ac, rs, cs) in anchors(t) {
            if ar == r {
                // The anchor row goes: the merge (and its text) moves to the next row.
                if rs > 1 {
                    let text = t.cells.get(ar).and_then(|row| row.get(ac)).map(|c| c.text.clone()).unwrap_or_default();
                    if let Some(cell) = t.cells.get_mut(ar + 1).and_then(|row| row.get_mut(ac)) {
                        cell.text = text;
                    }
                    a.push((ar, ac, rs - 1, cs));
                }
            } else if ar > r {
                a.push((ar - 1, ac, rs, cs));
            } else if r < ar + rs {
                a.push((ar, ac, rs - 1, cs));
            } else {
                a.push((ar, ac, rs, cs));
            }
        }
        t.row_heights.remove(r);
        t.cells.remove(r);
        if r < t.row_styles.len() {
            t.row_styles.remove(r);
        }
        if r == 0 {
            t.title = false;
        }
        set_anchors(t, &a);
        shift_formulas(t, formula::Shift::DeleteRow(r));
        Ok(())
    })
}

fn run_insert_col(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.insertcol", |t| {
        if t.col_widths.len() >= MAX_COLS {
            return Err("too many columns".into());
        }
        let c = usize_param(p, "col").unwrap_or(t.col_widths.len()).min(t.col_widths.len());
        let w = p.get("width").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).or_else(|| t.col_widths.last().copied()).unwrap_or(2.5);
        let a: Vec<_> = anchors(t)
            .into_iter()
            .map(|(ar, ac, rs, cs)| {
                if ac >= c {
                    (ar, ac + 1, rs, cs)
                } else if c < ac + cs {
                    (ar, ac, rs, cs + 1)
                } else {
                    (ar, ac, rs, cs)
                }
            })
            .collect();
        t.col_widths.insert(c, w);
        for row in &mut t.cells {
            row.insert(c.min(row.len()), TableCell::default());
        }
        set_anchors(t, &a);
        shift_formulas(t, formula::Shift::InsertCol(c));
        Ok(())
    })
}

fn run_delete_col(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.deletecol", |t| {
        let c = usize_param(p, "col").ok_or("`col` is required")?;
        if c >= t.col_widths.len() || t.col_widths.len() <= 1 {
            return Err("no such column (a table keeps at least one column)".into());
        }
        let mut a = Vec::new();
        for (ar, ac, rs, cs) in anchors(t) {
            if ac == c {
                if cs > 1 {
                    let text = t.cells.get(ar).and_then(|row| row.get(ac)).map(|x| x.text.clone()).unwrap_or_default();
                    if let Some(cell) = t.cells.get_mut(ar).and_then(|row| row.get_mut(ac + 1)) {
                        cell.text = text;
                    }
                    a.push((ar, ac, rs, cs - 1));
                }
            } else if ac > c {
                a.push((ar, ac - 1, rs, cs));
            } else if c < ac + cs {
                a.push((ar, ac, rs, cs - 1));
            } else {
                a.push((ar, ac, rs, cs));
            }
        }
        t.col_widths.remove(c);
        for row in &mut t.cells {
            if c < row.len() {
                row.remove(c);
            }
        }
        set_anchors(t, &a);
        shift_formulas(t, formula::Shift::DeleteCol(c));
        Ok(())
    })
}

fn run_merge(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.merge", |t| {
        let (r, c) = cell_at(p, t)?;
        let rs = usize_param(p, "rows").unwrap_or(1).max(1).min(t.cells.len() - r);
        let cs = usize_param(p, "cols").unwrap_or(1).max(1).min(t.col_widths.len() - c);
        if rs == 1 && cs == 1 {
            return Err("merge at least two cells (`rows` / `cols`)".into());
        }
        // Existing merges overlapping the range are absorbed; texts are joined into the anchor.
        let overlaps = |&(ar, ac, ars, acs): &(usize, usize, usize, usize)| ar < r + rs && r < ar + ars && ac < c + cs && c < ac + acs;
        let mut a: Vec<_> = anchors(t).into_iter().filter(|x| !overlaps(x)).collect();
        let mut texts = Vec::new();
        for rr in r..r + rs {
            for cc in c..c + cs {
                if let Some(cell) = t.cells.get_mut(rr).and_then(|row| row.get_mut(cc))
                    && !cell.text.is_empty()
                {
                    texts.push(std::mem::take(&mut cell.text));
                }
            }
        }
        if let Some(cell) = t.cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            cell.text = texts.join(" ");
        }
        a.push((r, c, rs, cs));
        set_anchors(t, &a);
        Ok(())
    })
}

fn run_unmerge(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.unmerge", |t| {
        let (r, c) = cell_at(p, t)?;
        let a: Vec<_> = anchors(t).into_iter().filter(|&(ar, ac, rs, cs)| !(r >= ar && r < ar + rs && c >= ac && c < ac + cs)).collect();
        set_anchors(t, &a);
        Ok(())
    })
}

/// TABLE at the command line: columns, data rows, insertion point.
struct TableM {
    cols: Option<usize>,
    rows: Option<usize>,
    style: cadcraft_doc::TableStyle,
}

impl TableM {
    fn new(s: &Session) -> Self {
        TableM { cols: None, rows: None, style: style_of(s) }
    }
    fn table(&self, at: Vec2) -> Table {
        let cols = self.cols.unwrap_or(5);
        let rows = self.rows.unwrap_or(1);
        let title = self.style.title.then_some("");
        let header = self.style.header.then(|| vec![String::new(); cols]);
        make_table(&self.style, at, rows, cols, None, None, title, header, &[])
    }
}

impl Interactive for TableM {
    fn name(&self) -> &'static str {
        "TABLE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.cols, self.rows) {
            (None, _) => Prompt::new("Enter number of columns", Accept::NUMBER).default("5"),
            (Some(_), None) => Prompt::new("Enter number of data rows", Accept::NUMBER).default("1"),
            _ => Prompt::new("Specify insertion point", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let count = |i: &Input, max: usize| -> Option<usize> {
            let v = match i {
                Input::Text(t) => super::machines::number(t),
                _ => None,
            }?;
            (v.is_finite() && v >= 1.0 && v <= max as f64).then_some(v.round() as usize)
        };
        match (self.cols, self.rows, &i) {
            (_, _, Input::Cancel) => Ok(Step::Cancel),
            (None, _, Input::Enter) => {
                self.cols = Some(5);
                Ok(Step::Continue)
            }
            (None, _, _) => {
                match count(&i, MAX_COLS) {
                    Some(n) => self.cols = Some(n),
                    None => s.echo(format!("Requires an integer between 1 and {MAX_COLS}.")),
                }
                Ok(Step::Continue)
            }
            (Some(_), None, Input::Enter) => {
                self.rows = Some(1);
                Ok(Step::Continue)
            }
            (Some(_), None, _) => {
                match count(&i, MAX_ROWS) {
                    Some(n) => self.rows = Some(n),
                    None => s.echo(format!("Requires an integer between 1 and {MAX_ROWS}.")),
                }
                Ok(Step::Continue)
            }
            (Some(_), Some(_), Input::Point(p)) => {
                let t = self.table(*p);
                s.add_entity(EntityKind::Table(t))?;
                Ok(Step::Done)
            }
            (Some(_), Some(_), Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.cols.is_some() && self.rows.is_some() { vec![EntityKind::Table(self.table(c))] } else { Vec::new() }
    }
}
