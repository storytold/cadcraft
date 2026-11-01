//! Table cell styles, formulas, TABLEDIT and TABLEEXPORT.

use cadcraft_color::Color;
use cadcraft_doc::{CellBorder, CellFormat, CellStyle, Drawing, EntityKind, Handle, Lineweight, Table, TableStyle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::formula::{self, cell_name, parse_cell_name};
use super::{MAX_COLS, MAX_ROWS, edit, table_handle, usize_param};
use crate::cmd::{CommandSpec, bad, bool_or, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const CELLSTYLE_PARAMS: &str = "{style? (table style; default current), name (cell style: Title, Header, Data or your own), textStyle?, textHeight?, \
     textColor?, fill?: color | \"none\", align?: TL|TC|TR|ML|MC|MR|BL|BC|BR or 1..9, margin?, marginH?, marginV?, \
     borders?: {all|outside|inside|top|insideH|bottom|left|insideV|right: {visible?, color?, lineweight? (mm or ByBlock)}}, \
     format?: general|decimal|percent|text, precision? (0..8), merge?: bool, delete?: bool} → cellStyles";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("tablestyle.cellstyle", "Table Cell Style", run_cellstyle_def)
            .params(CELLSTYLE_PARAMS)
            .interactive(|s| Ok(Box::new(CellStyleM::new(s)))),
        CommandSpec::new("table.cellstyle", "Set Table Cell Style", run_cellstyle_use)
            .params("{handle, style (cell style name; \"\" = by position), row?, col?, rows?, cols?} (rows without col: the row style)"),
        CommandSpec::new("table.formula", "Insert Table Formula", run_formula).params(
            "{handle, row, col, function: sum|average|count|min|max|cell|equation, range?: \"A2:B5\" (default: the cells above), cell?: \"A1\", expression?: \"A1*2\"}",
        ),
        CommandSpec::new("table.recalc", "Recalculate Table Formulas", run_recalc).params("{handle?} (default: every table) → {changed}"),
        CommandSpec::new("tabledit", "Edit Table Cell", run_tabledit)
            .params("{handle?, at: [x,y] | row+col, text (=… for a formula)}")
            .interactive(|_| Ok(Box::new(TableEditM::default()))),
        CommandSpec::new("tableexport", "Export Table", run_export)
            .params("{handle, path? (.csv), separator?: \",\" | \"\\t\"} → {csv, rows, cols, path?}")
            .interactive(|_| Ok(Box::new(ExportM::default()))),
    ]
}

// ---------------------------------------------------------------- cell styles

/// A colour parameter: a name, an index, `r,g,b`, ByLayer / ByBlock.
fn color_value(v: &Value) -> Option<Color> {
    match v {
        Value::Number(n) => n.as_u64().filter(|i| (1..=255).contains(i)).map(|i| Color::Index(i as u8)),
        Value::String(s) => Color::parse(s),
        Value::Object(_) => serde_json::from_value(v.clone()).ok(),
        _ => None,
    }
}

/// An alignment parameter: `TL` … `BR`, `MiddleLeft`, or the MTEXT attachment 1..9.
fn align_value(v: &Value) -> Option<u8> {
    if let Some(n) = v.as_u64() {
        return u8::try_from(n).ok().filter(|n| (1..=9).contains(n));
    }
    let s: String = v.as_str()?.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase();
    let (vert, horiz) = match s.as_str() {
        "tl" | "topleft" => (0, 0),
        "tc" | "topcenter" => (0, 1),
        "tr" | "topright" => (0, 2),
        "ml" | "middleleft" => (1, 0),
        "mc" | "middlecenter" | "center" | "middle" => (1, 1),
        "mr" | "middleright" => (1, 2),
        "bl" | "bottomleft" => (2, 0),
        "bc" | "bottomcenter" => (2, 1),
        "br" | "bottomright" => (2, 2),
        _ => return None,
    };
    Some(vert * 3 + horiz + 1)
}

/// A lineweight parameter: millimetres, or ByBlock / ByLayer / Default.
fn lineweight_value(v: &Value) -> Option<Lineweight> {
    if let Some(mm) = v.as_f64() {
        if !(mm.is_finite() && (0.0..=2.11).contains(&mm)) {
            return None;
        }
        let want = (mm * 100.0).round() as u16;
        // The nearest standard lineweight.
        return Lineweight::STANDARD.iter().min_by_key(|w| w.abs_diff(want)).map(|w| Lineweight::Mm100(*w));
    }
    match v.as_str()?.trim().to_ascii_lowercase().as_str() {
        "byblock" => Some(Lineweight::ByBlock),
        "bylayer" => Some(Lineweight::ByLayer),
        "default" => Some(Lineweight::Default),
        s => s.trim_end_matches("mm").trim().parse::<f64>().ok().and_then(|mm| lineweight_value(&json!(mm))),
    }
}

fn apply_border(b: &mut CellBorder, v: &Value) -> std::result::Result<(), String> {
    if let Some(on) = v.as_bool() {
        b.visible = on;
        return Ok(());
    }
    if let Some(on) = v.get("visible").and_then(Value::as_bool) {
        b.visible = on;
    }
    if let Some(c) = v.get("color") {
        b.color = color_value(c).ok_or_else(|| format!("invalid border color {c}"))?;
    }
    if let Some(w) = v.get("lineweight") {
        b.lineweight = lineweight_value(w).ok_or_else(|| format!("invalid lineweight {w}"))?;
    }
    Ok(())
}

/// Apply the cell style fields of `p` to `cs`.
pub(super) fn apply_fields(cs: &mut CellStyle, p: &Value) -> std::result::Result<(), String> {
    let num = |k: &str, min: f64| -> std::result::Result<Option<f64>, String> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= min && *x <= 1e9).map(Some).ok_or_else(|| format!("invalid {k}: {v}")),
        }
    };
    if let Some(ts) = str_param(p, "textStyle").map(str::trim).filter(|t| !t.is_empty()) {
        cs.text_style = ts.to_string();
    }
    if let Some(h) = num("textHeight", f64::MIN_POSITIVE)? {
        cs.text_height = h;
    }
    if let Some(c) = p.get("textColor") {
        cs.text_color = color_value(c).ok_or_else(|| format!("invalid textColor {c}"))?;
    }
    match p.get("fill") {
        None => {}
        Some(Value::Null) | Some(Value::Bool(false)) => cs.fill = None,
        Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("none") => cs.fill = None,
        Some(v) => cs.fill = Some(color_value(v).ok_or_else(|| format!("invalid fill {v}"))?),
    }
    if let Some(a) = p.get("align") {
        cs.align = align_value(a).ok_or_else(|| format!("invalid align {a} (TL, TC, TR, ML, MC, MR, BL, BC, BR)"))?;
    }
    if let Some(m) = num("margin", 0.0)? {
        cs.margin_h = m;
        cs.margin_v = m;
    }
    if let Some(m) = num("marginH", 0.0)? {
        cs.margin_h = m;
    }
    if let Some(m) = num("marginV", 0.0)? {
        cs.margin_v = m;
    }
    if let Some(b) = p.get("borders").and_then(Value::as_object) {
        for (k, v) in b {
            let targets: &[usize] = match k.to_ascii_lowercase().as_str() {
                "all" => &[0, 1, 2, 3, 4, 5],
                "outside" => &[0, 2, 3, 5],
                "inside" => &[1, 4],
                "top" => &[0],
                "insideh" => &[1],
                "bottom" => &[2],
                "left" => &[3],
                "insidev" => &[4],
                "right" => &[5],
                _ => return Err(format!("unknown border {k}")),
            };
            for (i, border) in cs.borders.as_array_mut().into_iter().enumerate() {
                if targets.contains(&i) {
                    apply_border(border, v)?;
                }
            }
        }
    }
    if let Some(f) = str_param(p, "format") {
        cs.format = match f.trim().to_ascii_lowercase().as_str() {
            "general" => CellFormat::General,
            "decimal" | "number" => CellFormat::Decimal,
            "percent" | "percentage" => CellFormat::Percent,
            "text" => CellFormat::Text,
            _ => return Err(format!("invalid format {f} (general, decimal, percent, text)")),
        };
    }
    if let Some(n) = p.get("precision") {
        cs.precision = n.as_u64().filter(|n| *n <= 8).map(|n| n as u8).ok_or_else(|| format!("invalid precision {n} (0..8)"))?;
    }
    if let Some(b) = p.get("merge").and_then(Value::as_bool) {
        cs.merge = b;
    }
    Ok(())
}

/// The index of table style `name` (the current one when `None`).
fn style_index(d: &Drawing, name: Option<&str>) -> Option<usize> {
    let cur = d.header.str("CTABLESTYLE", "Standard");
    let name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&cur);
    d.table_styles.iter().position(|t| t.name.eq_ignore_ascii_case(name))
}

/// Recalculate the formulas of every table using table style `style`.
fn recalc_tables_of(d: &mut Drawing, style: &str) {
    let hs: Vec<Handle> = std::iter::once(&d.model)
        .chain(d.layouts.iter().map(|l| &l.entities))
        .flat_map(|st| st.iter())
        .filter(|e| matches!(&e.kind, EntityKind::Table(t) if t.style.eq_ignore_ascii_case(style)))
        .map(|e| e.handle)
        .collect();
    let st = d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(style)).cloned().unwrap_or_default();
    for h in hs {
        let _ = d.modify_entity(h, |e| {
            if let EntityKind::Table(t) = &mut e.kind {
                formula::recalc(t, &st);
            }
        });
    }
}

/// Define, change or delete cell style `p.name` of a table style.
pub(crate) fn define_cell_style(d: &mut Drawing, p: &Value) -> Result<Value> {
    let id = "tablestyle.cellstyle";
    let i = style_index(d, str_param(p, "style")).ok_or_else(|| bad(id, "no such table style"))?;
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(id, "`name` of the cell style is required"))?;
    let Some(st) = d.table_styles.get_mut(i) else { return Err(bad(id, "no such table style")) };
    if bool_or(p, "delete", false) {
        if CellStyle::is_builtin(name) {
            st.cell_styles.retain(|c| !c.name.eq_ignore_ascii_case(name));
        } else {
            let before = st.cell_styles.len();
            st.cell_styles.retain(|c| !c.name.eq_ignore_ascii_case(name));
            if before == st.cell_styles.len() {
                return Err(bad(id, format!("no cell style {name}")));
            }
        }
    } else {
        if !st.has_cell_style(name) && st.cell_styles.len() >= cadcraft_doc::MAX_CELL_STYLES {
            return Err(bad(id, "too many cell styles"));
        }
        let mut cs = st.cell_style(name);
        if !st.has_cell_style(name) {
            // A new style starts as a copy of Data.
            cs.name = name.chars().take(255).collect();
        }
        apply_fields(&mut cs, p).map_err(|m| bad(id, m))?;
        st.set_cell_style(cs);
    }
    let st = st.clone();
    recalc_tables_of(d, &st.name);
    let styles: Vec<Value> = st.cell_style_names().iter().map(|n| serde_json::to_value(st.cell_style(n)).unwrap_or(Value::Null)).collect();
    Ok(json!({ "style": st.name, "cellStyles": styles }))
}

fn run_cellstyle_def(s: &mut Session, p: &Value) -> Result<Value> {
    define_cell_style(s.doc_mut()?, p)
}

/// TABLESTYLE's `cellStyle` parameter: `{name, …}` cell style fields for the style just edited.
pub(crate) fn tablestyle_cell_style(d: &mut Drawing, style: &str, p: &Value) -> Result<()> {
    if let Some(cs) = p.get("cellStyle").filter(|v| v.is_object()) {
        let mut q = cs.clone();
        q["style"] = json!(style);
        define_cell_style(d, &q)?;
    }
    Ok(())
}

fn run_cellstyle_use(s: &mut Session, p: &Value) -> Result<Value> {
    let id = "table.cellstyle";
    let name = match p.get("style") {
        Some(Value::Null) => String::new(),
        Some(v) => v.as_str().map(|x| x.trim().to_string()).ok_or_else(|| bad(id, "`style` must be a cell style name"))?,
        None => return Err(bad(id, "`style` (cell style name) is required")),
    };
    let styles: Vec<TableStyle> = s.doc()?.table_styles.clone();
    edit(s, p, id, |t| {
        let st = styles.iter().find(|x| x.name.eq_ignore_ascii_case(&t.style)).cloned().unwrap_or_default();
        if !name.is_empty() && !st.has_cell_style(&name) {
            return Err(format!("table style {} has no cell style {name} (it has {})", st.name, st.cell_style_names().join(", ")));
        }
        let rows = t.cells.len();
        let cols = t.col_widths.len();
        let r = usize_param(p, "row").unwrap_or(0);
        if r >= rows {
            return Err(format!("row {r} is outside the table"));
        }
        let nr = usize_param(p, "rows").unwrap_or(1).clamp(1, rows - r);
        match usize_param(p, "col") {
            Some(c) => {
                if c >= cols {
                    return Err(format!("column {c} is outside the table"));
                }
                let nc = usize_param(p, "cols").unwrap_or(1).clamp(1, cols - c);
                for row in t.cells.iter_mut().skip(r).take(nr) {
                    for cell in row.iter_mut().skip(c).take(nc) {
                        cell.style = name.clone();
                    }
                }
            }
            None => {
                if t.row_styles.len() < r + nr {
                    t.row_styles.resize(r + nr, String::new());
                }
                for rs in t.row_styles.iter_mut().skip(r).take(nr) {
                    rs.clone_from(&name);
                }
                // Cells take the new row style.
                for row in t.cells.iter_mut().skip(r).take(nr) {
                    for cell in row.iter_mut() {
                        cell.style.clear();
                    }
                }
                while t.row_styles.last().is_some_and(String::is_empty) {
                    t.row_styles.pop();
                }
            }
        }
        Ok(())
    })
}

// ---------------------------------------------------------------- formulas

fn run_formula(s: &mut Session, p: &Value) -> Result<Value> {
    let id = "table.formula";
    let func = str_param(p, "function").unwrap_or("sum").trim().to_ascii_lowercase();
    let r = usize_param(p, "row").ok_or_else(|| bad(id, "`row` is required"))?;
    let c = usize_param(p, "col").ok_or_else(|| bad(id, "`col` is required"))?;
    let (title, header) = {
        let d = s.doc()?;
        let h = p.get("handle").and_then(|v| v.as_str().and_then(Handle::parse_hex));
        match h.and_then(|h| d.entity(h)).map(|e| &e.kind) {
            Some(EntityKind::Table(t)) => (t.title, t.header),
            _ => (false, false),
        }
    };
    let f = match func.as_str() {
        "sum" | "average" | "count" | "min" | "max" => {
            let range = match str_param(p, "range") {
                Some(rg) => rg.trim().to_string(),
                None => {
                    // The cells above, from the first data row.
                    let first = usize::from(title) + usize::from(header);
                    if r <= first {
                        return Err(bad(id, "`range` is required (no data cells above this one)"));
                    }
                    format!("{}:{}", cell_name(first, c), cell_name(r - 1, c))
                }
            };
            let name = match func.as_str() {
                "sum" => "Sum",
                "average" => "Average",
                "count" => "Count",
                "min" => "Min",
                _ => "Max",
            };
            format!("={name}({range})")
        }
        "cell" => {
            let cell = str_param(p, "cell").ok_or_else(|| bad(id, "`cell` (e.g. \"A1\") is required"))?;
            parse_cell_name(cell).ok_or_else(|| bad(id, format!("invalid cell {cell}")))?;
            format!("={}", cell.trim())
        }
        "equation" => {
            let e = str_param(p, "expression").ok_or_else(|| bad(id, "`expression` is required"))?.trim();
            if e.starts_with('=') { e.to_string() } else { format!("={e}") }
        }
        other => return Err(bad(id, format!("unknown function {other} (sum, average, count, min, max, cell, equation)"))),
    };
    let mut q = p.clone();
    q["formula"] = json!(f);
    let mut r = super::run_set(s, &q)?;
    r["formula"] = json!(f);
    Ok(r)
}

/// Recalculate the formulas of the tables `hs` (all when `None`); the number of tables changed.
pub(crate) fn recalc(s: &mut Session, hs: Option<Vec<Handle>>) -> Result<usize> {
    let d = s.doc()?;
    let hs: Vec<Handle> = match hs {
        Some(h) => h,
        None => std::iter::once(&d.model)
            .chain(d.layouts.iter().map(|l| &l.entities))
            .flat_map(|st| st.iter())
            .filter(|e| matches!(&e.kind, EntityKind::Table(t) if t.cells.iter().flatten().any(|c| c.formula.is_some())))
            .map(|e| e.handle)
            .collect(),
    };
    let mut changed = Vec::new();
    for h in hs {
        let Some(EntityKind::Table(t)) = d.entity(h).map(|e| &e.kind) else { continue };
        let mut t = t.clone();
        let st = cadcraft_render::table_style_of(d, &t);
        if formula::recalc(&mut t, &st) {
            changed.push((h, t));
        }
    }
    let n = changed.len();
    let d = s.doc_mut()?;
    for (h, t) in changed {
        d.modify_entity(h, |e| e.kind = EntityKind::Table(t))?;
    }
    Ok(n)
}

fn run_recalc(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = p.get("handle").and_then(|v| v.as_str().and_then(Handle::parse_hex)).map(|h| vec![h]);
    let n = recalc(s, hs)?;
    Ok(json!({ "changed": n }))
}

// ---------------------------------------------------------------- TABLEDIT

/// The table and cell (row, col) under `p` in the current space.
fn cell_under(s: &Session, p: Vec2) -> Option<(Handle, usize, usize)> {
    let d = s.doc().ok()?;
    let space = d.space(&s.space())?;
    for e in space.iter() {
        let EntityKind::Table(t) = &e.kind else { continue };
        if let Some((r, c)) = cell_of(t, p) {
            return Some((e.handle, r, c));
        }
    }
    None
}

/// The cell of `t` containing `p`.
pub(crate) fn cell_of(t: &Table, p: Vec2) -> Option<(usize, usize)> {
    t.cell_at_point(p)
}

fn run_tabledit(s: &mut Session, p: &Value) -> Result<Value> {
    let mut q = p.clone();
    if let Some(at) = p.get("at").and_then(|v| serde_json::from_value::<[f64; 2]>(v.clone()).ok()) {
        let (h, r, c) = cell_under(s, Vec2::new(at[0], at[1])).ok_or_else(|| bad("tabledit", "no table cell at that point"))?;
        q["handle"] = json!(h.hex());
        q["row"] = json!(r);
        q["col"] = json!(c);
    }
    super::run_set(s, &q)
}

/// TABLEDIT at the command line: pick a cell, type its text (or `=formula`).
#[derive(Default)]
struct TableEditM {
    cell: Option<(Handle, usize, usize)>,
}

impl Interactive for TableEditM {
    fn name(&self) -> &'static str {
        "TABLEDIT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.cell {
            None => Prompt::new("Pick a table cell", Accept::POINT),
            Some((h, r, c)) => {
                let cur = s
                    .doc()
                    .ok()
                    .and_then(|d| d.entity(h))
                    .and_then(|e| if let EntityKind::Table(t) = &e.kind { t.cells.get(r).and_then(|row| row.get(c)).cloned() } else { None })
                    .map(|c| c.formula.unwrap_or(c.text))
                    .unwrap_or_default();
                let p = Prompt::new(format!("Enter text for cell {} (=formula)", cell_name(r, c)), Accept::TEXT);
                if cur.is_empty() { p } else { p.default(cur) }
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.cell, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (None, Input::Point(p)) => {
                match cell_under(s, p) {
                    Some(c) => self.cell = Some(c),
                    None => s.echo("No table cell there."),
                }
                Ok(Step::Continue)
            }
            (None, Input::Enter) => Ok(Step::Cancel),
            (Some(_), Input::Enter) => Ok(Step::Done),
            (Some((h, r, c)), Input::Text(t) | Input::Keyword(t)) => {
                let q = json!({ "handle": h.hex(), "row": r, "col": c, "text": t });
                match super::run_set(s, &q) {
                    Ok(_) => Ok(Step::Done),
                    Err(e) => {
                        s.echo(e.to_string());
                        Ok(Step::Continue)
                    }
                }
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------------------------------------------------------- TABLEEXPORT

/// `text` as a CSV field: quoted when it holds the separator, a quote or a line break.
fn csv_field(text: &str, sep: char) -> String {
    if text.contains(sep) || text.contains('"') || text.contains('\n') || text.contains('\r') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// The table's cell values as CSV (cells covered by a merge are empty).
pub(crate) fn table_csv(t: &Table, sep: char) -> String {
    let cov = cadcraft_render::table_covered(t);
    let rows = t.row_heights.len().min(t.cells.len()).min(MAX_ROWS);
    let cols = t.col_widths.len().min(MAX_COLS);
    let mut out = String::new();
    for r in 0..rows {
        let line: Vec<String> = (0..cols)
            .map(|c| {
                let covered = cov.get(r).and_then(|v| v.get(c)).copied().unwrap_or(false);
                let text = if covered { "" } else { t.cells.get(r).and_then(|row| row.get(c)).map(|x| x.text.as_str()).unwrap_or("") };
                csv_field(text, sep)
            })
            .collect();
        out.push_str(&line.join(&sep.to_string()));
        out.push_str("\r\n");
    }
    out
}

/// Write `text` to `path`: natively the file, on the web a download through the host.
fn write_text(path: &str, text: &str) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::consent::check_write(path).map_err(|e| bad("tableexport", e.to_string()))?;
        std::fs::write(path, text).map_err(|e| bad("tableexport", format!("{path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let deliver = crate::cmd::file::deliver().ok_or_else(|| bad("tableexport", "files can't be written in this build; use the returned csv"))?;
        deliver(&name, "text/csv", text.as_bytes()).map_err(|e| bad("tableexport", format!("{name}: {e}")))
    }
}

fn run_export(s: &mut Session, p: &Value) -> Result<Value> {
    let id = "tableexport";
    let h = table_handle(s, p, id)?;
    let t = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Table(t)) => t.clone(),
        _ => return Err(bad(id, "object is not a table")),
    };
    let sep = match str_param(p, "separator") {
        Some("\t") | Some("tab") => '\t',
        Some(";") => ';',
        Some(",") | None => ',',
        Some(other) => return Err(bad(id, format!("unsupported separator {other:?} (\",\", \";\" or tab)"))),
    };
    let csv = table_csv(&t, sep);
    let mut r = json!({ "handle": h.hex(), "rows": t.cells.len(), "cols": t.col_widths.len(), "csv": csv });
    if let Some(path) = str_param(p, "path").map(str::trim).filter(|x| !x.is_empty()) {
        write_text(path, &csv)?;
        r["path"] = json!(path);
        r["message"] = json!(format!("Table exported to {path}."));
    }
    Ok(r)
}

/// The default export file: the drawing's name with a .csv extension.
fn default_path(s: &Session) -> String {
    let base = s.state().ok().and_then(|st| st.path.clone()).unwrap_or_else(|| "table.csv".into());
    std::path::Path::new(&base).with_extension("csv").to_string_lossy().into_owned()
}

/// TABLEEXPORT at the command line: select a table, then the file name.
#[derive(Default)]
struct ExportM {
    table: Option<Handle>,
}

impl ExportM {
    fn pick(&mut self, s: &mut Session, hs: &[Handle]) {
        let d = s.doc().ok();
        self.table = hs.iter().copied().find(|h| d.and_then(|d| d.entity(*h)).is_some_and(|e| matches!(e.kind, EntityKind::Table(_))));
        if self.table.is_none() {
            s.echo("The object is not a table.");
        }
    }
}

impl Interactive for ExportM {
    fn name(&self) -> &'static str {
        "TABLEEXPORT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.table {
            None => Prompt::new("Select a table", Accept::SELECT),
            Some(_) => Prompt::new("Enter file name", Accept::TEXT).default(default_path(s)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.table, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (None, Input::Pick(hs)) => {
                self.pick(s, &hs);
                Ok(Step::Continue)
            }
            (None, Input::Point(p)) => {
                match cell_under(s, p) {
                    Some((h, ..)) => self.table = Some(h),
                    None => s.echo("No table there."),
                }
                Ok(Step::Continue)
            }
            (None, Input::Enter) => Ok(Step::Cancel),
            (None, _) => Ok(Step::Continue),
            (Some(h), i) => {
                let path = match i {
                    Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
                    Input::Enter => default_path(s),
                    _ => return Ok(Step::Continue),
                };
                let r = run_export(s, &json!({ "handle": h.hex(), "path": path }))?;
                if let Some(m) = r.get("message").and_then(Value::as_str) {
                    s.echo(m.to_string());
                }
                Ok(Step::Done)
            }
        }
    }
}

// ---------------------------------------------------------------- typed cell styles

/// `tablestyle.cellstyle` at the command line: table style, cell style, then options.
struct CellStyleM {
    style: String,
    cell: Option<String>,
    /// The option waiting for its value.
    option: Option<&'static str>,
}

const OPTIONS: &[&str] = &["Height", "Color", "Fill", "Alignment", "Margins", "Borders", "fOrmat", "Precision", "Textstyle", "eXit"];

impl CellStyleM {
    fn new(s: &Session) -> Self {
        let style = s.doc().map(|d| d.header.str("CTABLESTYLE", "Standard")).unwrap_or_else(|_| "Standard".into());
        CellStyleM { style, cell: None, option: None }
    }
    fn set(&self, s: &mut Session, field: Value) -> Result<()> {
        let mut q = field;
        q["style"] = json!(self.style);
        q["name"] = json!(self.cell.clone().unwrap_or_else(|| CellStyle::DATA.into()));
        define_cell_style(s.doc_mut()?, &q).map(|_| ())
    }
}

impl Interactive for CellStyleM {
    fn name(&self) -> &'static str {
        "TABLESTYLE.CELLSTYLE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (&self.cell, self.option) {
            (None, _) => Prompt::new(format!("Enter cell style name of table style {}", self.style), Accept::TEXT).default(CellStyle::DATA),
            (Some(_), None) => Prompt::new("Enter an option", Accept::TEXT).kw(OPTIONS).default("eXit"),
            (Some(_), Some(o)) => {
                let msg = match o {
                    "Height" => "Enter text height",
                    "Color" => "Enter text color (name, index or ByBlock)",
                    "Fill" => "Enter fill color or None",
                    "Alignment" => "Enter alignment [TL/TC/TR/ML/MC/MR/BL/BC/BR]",
                    "Margins" => "Enter cell margin",
                    "Borders" => "Show borders? [On/Off]",
                    "fOrmat" => "Enter data format [General/Decimal/Percent/Text]",
                    "Precision" => "Enter precision (0-8)",
                    _ => "Enter text style name",
                };
                Prompt::new(msg, Accept::TEXT)
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match i {
            Input::Cancel => return Ok(Step::Cancel),
            Input::Enter => None,
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()),
            _ => return Ok(Step::Continue),
        };
        if self.cell.is_none() {
            self.cell = Some(text.filter(|t| !t.is_empty()).unwrap_or_else(|| CellStyle::DATA.into()));
            // Make sure it exists (a new style starts as a copy of Data).
            if let Err(e) = self.set(s, json!({})) {
                s.echo(e.to_string());
                return Ok(Step::Cancel);
            }
            return Ok(Step::Continue);
        }
        let Some(opt) = self.option else {
            let Some(t) = text else { return Ok(Step::Done) };
            let p = self.prompt(s);
            match p.match_keyword(&t).and_then(|k| OPTIONS.iter().find(|o| o.eq_ignore_ascii_case(&k)).copied()) {
                Some("eXit") => return Ok(Step::Done),
                Some(o) => self.option = Some(o),
                None => s.echo("Invalid option keyword."),
            }
            return Ok(Step::Continue);
        };
        self.option = None;
        let Some(t) = text else { return Ok(Step::Continue) };
        let number = super::super::machines::number(&t).filter(|v| v.is_finite());
        if matches!(opt, "Height" | "Margins") && number.is_none() {
            s.echo("Requires a number.");
            return Ok(Step::Continue);
        }
        let number = number.unwrap_or(0.0);
        let field = match opt {
            "Height" => json!({ "textHeight": number }),
            "Color" => json!({ "textColor": t }),
            "Fill" => json!({ "fill": t }),
            "Alignment" => json!({ "align": t }),
            "Margins" => json!({ "margin": number }),
            "Borders" => json!({ "borders": { "all": { "visible": !t.eq_ignore_ascii_case("off") } } }),
            "fOrmat" => json!({ "format": t }),
            "Precision" => json!({ "precision": t.parse::<u64>().unwrap_or(99) }),
            _ => json!({ "textStyle": t }),
        };
        if let Err(e) = self.set(s, field) {
            s.echo(match e {
                EngineError::BadParams { msg, .. } => msg,
                e => e.to_string(),
            });
        }
        Ok(Step::Continue)
    }
}
