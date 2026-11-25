//! Paste tabular clipboard data (TSV from Excel or other spreadsheets) as a native `ACAD_TABLE`.

use cadcraft_doc::{EntityKind, Table, TableCell, TableStyle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::{MAX_COLS, MAX_ROWS, style_of};
use crate::cmd::helpers::{line, v3};
use crate::cmd::{bad, point_param, str_param};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Maximum UTF-8 byte length accepted for a pasted TSV payload (16 MiB).
pub const MAX_TSV_BYTES: usize = 16 << 20;
/// Maximum total grid cells created from pasted TSV (`rows * cols`).
pub const MAX_PASTE_CELLS: usize = 20_000;
/// Above this many cells, interactive cursor preview draws the outer table box instead of laying
/// out every cell each frame.
const PREVIEW_CELL_LIMIT: usize = 200;

/// Parse RFC-4180 tab-delimited text into a uniform rectangular grid of cells via [`craft_csv`].
///
/// Supports quoted fields (`"..."`), escaped doubled quotes (`""`), and multiline cells inside
/// quotes (`\n` / `\r\n` normalized to `\n`). Enforces [`MAX_TSV_BYTES`], [`MAX_ROWS`],
/// [`MAX_COLS`], and the total cell budget [`MAX_PASTE_CELLS`]. Requires at least 2 cells
/// (`rows * cols >= 2`) and at least one non-whitespace cell.
pub fn parse_tsv(text: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    if text.len() > MAX_TSV_BYTES {
        return Err(format!("pasted table exceeds {} MB limit", MAX_TSV_BYTES >> 20));
    }
    let clean = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut max_cols = 0usize;
    let mut err: Option<String> = None;

    craft_csv::parse_records(clean, '\t', '"', MAX_COLS + 1, |mut rec| {
        if rec.len() > MAX_COLS {
            err = Some(format!("pasted table exceeds {MAX_COLS} columns"));
            return false;
        }
        if rows.len() >= MAX_ROWS {
            err = Some(format!("pasted table exceeds {MAX_ROWS} rows"));
            return false;
        }
        max_cols = max_cols.max(rec.len());
        if (rows.len() + 1).saturating_mul(max_cols) > MAX_PASTE_CELLS {
            err = Some(format!("pasted table exceeds {MAX_PASTE_CELLS} cell limit"));
            return false;
        }
        for cell in &mut rec {
            if cell.contains('\r') {
                *cell = cell.replace("\r\n", "\n").replace('\r', "\n");
            }
        }
        rows.push(rec);
        true
    });

    if let Some(msg) = err {
        return Err(msg);
    }

    let total = rows.len().saturating_mul(max_cols);
    if rows.is_empty() || max_cols == 0 || total < 2 {
        return Err("pasted text must contain at least 2 table cells".into());
    }
    if rows.iter().all(|r| r.iter().all(|c| c.trim().is_empty())) {
        return Err("pasted table contains no text".into());
    }
    for r in &mut rows {
        r.resize(max_cols, String::new());
    }
    Ok(rows)
}

/// Whether `text` looks like non-DXF tab-delimited spreadsheet data containing at least 2 cells.
pub fn is_tsv_table(text: &str) -> bool {
    if text.is_empty() || text.len() > MAX_TSV_BYTES || !text.contains('\t') {
        return false;
    }
    let mut lines = text.lines().map(str::trim);
    if lines.next() == Some("0") && lines.next() == Some("SECTION") {
        return false;
    }
    parse_tsv(text).is_ok()
}

/// Build an `ACAD_TABLE` entity from parsed cell strings, sizing columns and rows cleanly.
fn build_table(st: &TableStyle, at: Vec2, data: &[Vec<String>], text_height: Option<f64>) -> Table {
    let cols = data.first().map_or(1, Vec::len).max(1);
    let th = text_height.unwrap_or_else(|| if st.text_height.is_finite() && st.text_height > 0.0 { st.text_height } else { 2.5 });
    let margin = if st.text_height.is_finite() && st.text_height > 0.0 { st.margin * (th / st.text_height) } else { th * 0.5 };

    let mut col_chars = vec![1usize; cols];
    let mut row_heights = Vec::with_capacity(data.len());
    let mut cells = Vec::with_capacity(data.len());

    for row in data {
        let mut max_lines = 1usize;
        let mut table_row = Vec::with_capacity(cols);
        for (c, raw) in row.iter().enumerate() {
            let line_count = raw.split('\n').count().clamp(1, 50);
            max_lines = max_lines.max(line_count);
            let max_chars = raw.split('\n').map(|line| line.chars().count()).max().unwrap_or(0).clamp(1, 60);
            if let Some(slot) = col_chars.get_mut(c) {
                *slot = (*slot).max(max_chars);
            }
            table_row.push(TableCell { text: cadcraft_doc::mtext_literal(raw), ..TableCell::default() });
        }
        row_heights.push(th * (max_lines as f64 * 5.0 / 3.0) + 2.0 * margin);
        cells.push(table_row);
    }

    let min_col_w = (th * 4.0 + 2.0 * margin).max(th * 5.0);
    let col_widths: Vec<f64> = col_chars.into_iter().map(|chars| (0.75 * th * chars as f64 + 2.0 * margin + 0.5 * th).max(min_col_w)).collect();

    Table { insert: v3(at), col_widths, row_heights, cells, style: st.name.clone(), text_height: th, title: false, header: false, ..Table::default() }
}

fn parse_text_height(p: &Value) -> Result<Option<f64>> {
    let val = match p.get("text_height").or_else(|| p.get("textHeight")) {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v,
    };
    val.as_f64()
        .filter(|h| h.is_finite() && *h > 0.0 && *h <= 1e9)
        .map(Some)
        .ok_or_else(|| bad("pastetable", "`text_height` must be a finite positive number"))
}

fn parse_at(p: &Value) -> Result<Vec2> {
    match p.get("at") {
        None | Some(Value::Null) => Ok(Vec2::ZERO),
        Some(_) => point_param(p, "at").ok_or_else(|| bad("pastetable", "`at` must be a point [x, y]")),
    }
}

pub(super) fn run_pastetable(s: &mut Session, p: &Value) -> Result<Value> {
    let tsv = str_param(p, "tsv").ok_or_else(|| bad("pastetable", "`tsv` string is required"))?;
    let at = parse_at(p)?;
    let th = parse_text_height(p)?;
    let data = parse_tsv(tsv).map_err(|m| bad("pastetable", m))?;
    let rows = data.len();
    let cols = data.first().map_or(0, Vec::len);
    let st = style_of(s);
    let table = build_table(&st, at, &data, th);
    let h = s.add_entity(EntityKind::Table(table))?;
    s.set_selection(vec![h]);
    Ok(json!({ "handle": h.hex(), "rows": rows, "cols": cols }))
}

struct PasteTableM {
    table: Table,
    rows: usize,
    cols: usize,
}

impl Interactive for PasteTableM {
    fn name(&self) -> &'static str {
        "PASTETABLE"
    }
    fn begin(&mut self, _s: &mut Session) -> Result<Step> {
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify insertion point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Cancel | Input::Enter => Ok(Step::Cancel),
            Input::Point(p) => {
                let mut t = self.table.clone();
                t.insert = v3(p);
                let h = s.add_entity(EntityKind::Table(t))?;
                s.set_selection(vec![h]);
                s.echo(format!("Pasted {}x{} table.", self.rows, self.cols));
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.rows.saturating_mul(self.cols) <= PREVIEW_CELL_LIMIT {
            let mut t = self.table.clone();
            t.insert = v3(c);
            return vec![EntityKind::Table(t)];
        }
        let w: f64 = self.table.col_widths.iter().sum();
        let h: f64 = self.table.row_heights.iter().sum();
        let a = c;
        let b = c + Vec2::new(w, 0.0);
        let d = c + Vec2::new(w, -h);
        let e = c + Vec2::new(0.0, -h);
        vec![line(a, b), line(b, d), line(d, e), line(e, a)]
    }
}

impl Session {
    /// Start interactive placement of a native `ACAD_TABLE` parsed from tab-delimited clipboard
    /// text (`tsv`), asking for an insertion point through the standard interactive command path.
    pub fn start_paste_table(&mut self, tsv: &str, text_height: Option<f64>) -> Result<()> {
        if let Some(h) = text_height
            && !(h.is_finite() && h > 0.0 && h <= 1e9)
        {
            return Err(bad("pastetable", "`text_height` must be a finite positive number"));
        }
        let data = parse_tsv(tsv).map_err(|m| bad("pastetable", m))?;
        let rows = data.len();
        let cols = data.first().map_or(0, Vec::len);
        let st = style_of(self);
        let table = build_table(&st, Vec2::ZERO, &data, text_height);
        let enabled = crate::find_command("pastetable").map_or((|_| Ok(())) as fn(&Session) -> std::result::Result<(), String>, |c| c.enabled);
        self.launch_interactive("pastetable", enabled, Box::new(PasteTableM { table, rows, cols }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc4180_tsv_with_multiline_and_quoted_cells() {
        let input = "\u{feff}Item\tDescription\tQty\r\n\"Bolt M8\"\t\"Line 1\r\nLine 2\"\t10\r\n\"Say \"\"Hi\"\"\"\tPlain\t20\r\n";
        let got = parse_tsv(input).unwrap();
        assert_eq!(got, vec![vec!["Item", "Description", "Qty"], vec!["Bolt M8", "Line 1\nLine 2", "10"], vec!["Say \"Hi\"", "Plain", "20"],]);
    }

    #[test]
    fn rejects_invalid_single_cell_empty_dxf_and_over_budget_inputs() {
        assert!(parse_tsv("SingleCell").is_err());
        assert!(parse_tsv("\"quoted\ttab\"").is_err());
        assert!(parse_tsv("\t\n\t\n").is_err());
        assert!(!is_tsv_table("SingleCell"));
        assert!(!is_tsv_table("\"quoted\ttab\""));
        assert!(!is_tsv_table("just plain text\non two lines"));
        assert!(!is_tsv_table("0\nSECTION\n2\tTAB"));
        assert!(is_tsv_table("A\tB"));
        assert!(is_tsv_table("A\t\n1\t"));

        // Column budget > MAX_COLS rejected during parse.
        let too_wide = vec!["x"; MAX_COLS + 1].join("\t");
        let err = parse_tsv(&too_wide).unwrap_err();
        assert!(err.contains("columns"), "{err}");

        // Cell budget > MAX_PASTE_CELLS rejected during parse.
        let wide_row = vec!["x"; MAX_COLS].join("\t");
        let over_rows = (MAX_PASTE_CELLS / MAX_COLS) + 2;
        let oversized = vec![wide_row; over_rows].join("\n");
        let err = parse_tsv(&oversized).unwrap_err();
        assert!(err.contains("cell limit") || err.contains("rows"), "{err}");
    }

    #[test]
    fn pads_ragged_rows_to_uniform_columns() {
        let got = parse_tsv("A\tB\tC\n1\n2\t3\n").unwrap();
        assert_eq!(got, vec![vec!["A", "B", "C"], vec!["1", "", ""], vec!["2", "3", ""]]);
    }

    #[test]
    fn pastetable_command_creates_scaled_multiline_table() {
        let mut s = Session::new();
        let res =
            s.execute("pastetable", &json!({ "tsv": "ColA\tLong Header Value\n\"Row 1\nRow 2\"\t42", "at": [10, 20], "text_height": 2.0 })).unwrap();
        assert_eq!(res["rows"], 2);
        assert_eq!(res["cols"], 2);
        let h = cadcraft_doc::Handle::parse_hex(res["handle"].as_str().unwrap()).unwrap();
        let e = s.doc().unwrap().entity(h).unwrap();
        let EntityKind::Table(t) = &e.kind else { panic!("expected table") };
        assert_eq!(t.insert.xy(), Vec2::new(10.0, 20.0));
        assert_eq!(t.text_height, 2.0);
        assert!(!t.title && !t.header);
        assert_eq!(t.cells[1][0].text, "Row 1\\PRow 2");
        assert!(t.row_heights[1] > t.row_heights[0], "multiline row gets extra height");
        assert!(t.col_widths[1] > t.col_widths[0], "wider column gets extra width");
    }

    #[test]
    fn interactive_start_paste_table_places_on_point_and_supports_undo() {
        let mut s = Session::new();
        s.start_paste_table("A\tB\n1\t2\n", None).unwrap();
        assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("pastetable"));
        assert!(!s.preview(Vec2::new(5.0, 5.0)).is_empty());
        s.input(Input::Point(Vec2::new(100.0, 50.0))).unwrap();
        assert!(s.running.is_none());
        assert_eq!(s.doc().unwrap().model.len(), 1);
        let e = s.doc().unwrap().model.iter().next().unwrap();
        let EntityKind::Table(t) = &e.kind else { panic!("expected table") };
        assert_eq!(t.insert.xy(), Vec2::new(100.0, 50.0));
        assert_eq!(t.cells.len(), 2);
        s.undo().unwrap();
        assert!(s.doc().unwrap().model.is_empty());
    }

    #[test]
    fn start_paste_table_uses_standard_command_lifecycle() {
        let mut empty = Session::empty();
        assert!(matches!(empty.start_paste_table("A\tB\n1\t2\n", None), Err(crate::EngineError::Disabled(_, _))));

        let mut s = Session::new();
        let before_started = s.commands_started;
        s.start_paste_table("A\tB\n1\t2\n", None).unwrap();
        assert_eq!(s.commands_started, before_started + 1);
        assert_eq!(s.last_command.as_deref(), Some("pastetable"));
    }
}
