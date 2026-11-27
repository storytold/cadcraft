//! Paste tabular clipboard data (TSV from Excel or other spreadsheets) as a native `ACAD_TABLE`.

use cadcraft_doc::{CellStyle, EntityKind, Table, TableCell, TableStyle};
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
    // Why: `cadcraft_render::table::layout` scales the resolved `Data` cell style's text height
    // and horizontal/vertical margins by `th / st.text_height`, so sizing widths, wrap width, and
    // row heights with the same scaled `Data` metrics keeps wrapped text inside custom-styled cells.
    let scale = if st.text_height.is_finite() && st.text_height > 0.0 { th / st.text_height } else { 1.0 };
    let data_cs = st.cell_style(CellStyle::DATA);
    let cell_th = Some(data_cs.text_height * scale).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(th);
    let margin_h = Some(data_cs.margin_h * scale).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(th * 0.5);
    let margin_v = Some(data_cs.margin_v * scale).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(th * 0.5);

    let mut col_chars = vec![1usize; cols];
    let mut cells = Vec::with_capacity(data.len());

    for row in data {
        let mut table_row = Vec::with_capacity(cols);
        for (c, raw) in row.iter().enumerate() {
            let max_chars = raw.split('\n').map(|line| line.chars().count()).max().unwrap_or(0).clamp(1, 60);
            if let Some(slot) = col_chars.get_mut(c) {
                *slot = (*slot).max(max_chars);
            }
            table_row.push(TableCell { text: cadcraft_doc::mtext_literal(raw), ..TableCell::default() });
        }
        cells.push(table_row);
    }

    let min_col_w = (cell_th * 4.0 + 2.0 * margin_h).max(cell_th * 5.0);
    let col_widths: Vec<f64> =
        col_chars.into_iter().map(|chars| (0.95 * cell_th * chars as f64 + 2.0 * margin_h + 0.5 * cell_th).max(min_col_w)).collect();

    let mut row_heights = Vec::with_capacity(cells.len());
    for row in &cells {
        let mut max_lines = 1usize;
        for (c, cell) in row.iter().enumerate() {
            if cell.text.is_empty() {
                continue;
            }
            // Why: size each row to the exact wrapped line count from `layout_mtext_with` (without an
            // arbitrary line cap, as input size is already bounded by `MAX_TSV_BYTES`/`MAX_PASTE_CELLS`)
            // so cells wrapping past 50 lines match the renderer and do not spill into the next row.
            let inner_w = col_widths.get(c).copied().unwrap_or(0.0) - 2.0 * margin_h;
            let wrap_w = if inner_w.is_finite() && inner_w > 0.0 { inner_w } else { 0.0 };
            let mp = cadcraft_fonts::MTextParams { width: wrap_w, ..cadcraft_fonts::MTextParams::new(Vec2::ZERO, cell_th) };
            let line_count = cadcraft_fonts::layout_mtext_with(&cell.text, &mp).lines.len().max(1);
            max_lines = max_lines.max(line_count);
        }
        row_heights.push(cell_th * (max_lines as f64 * 5.0 / 3.0) + 2.0 * margin_v);
    }

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

    #[test]
    fn long_wrapped_cells_cap_col_width_and_grow_row_height() {
        let mut s = Session::new();
        let long_cell = "Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
        assert!(long_cell.chars().count() > 60);
        let tsv = format!("Short\t{long_cell}\n\"First line\n{long_cell}\"\t\nA\tB",);
        let res = s.execute("pastetable", &json!({ "tsv": tsv, "at": [0, 0], "text_height": 2.0 })).unwrap();
        let h = cadcraft_doc::Handle::parse_hex(res["handle"].as_str().unwrap()).unwrap();
        let e = s.doc().unwrap().entity(h).unwrap();
        let EntityKind::Table(t) = &e.kind else { panic!("expected table") };

        // Column 1 width is capped at 60 characters rather than growing unboundedly.
        let st = style_of(&s);
        let expected_cap_w = 0.95 * 2.0 * 60.0 + 2.0 * (st.margin * (2.0 / st.text_height)) + 0.5 * 2.0;
        assert!((t.col_widths[1] - expected_cap_w).abs() < 1e-9);
        // Row 0 wraps into multiple lines and is taller than single-line Row 2.
        assert!(t.row_heights[0] > t.row_heights[2]);
        // Row 1 combines a hard newline and a long wrapped line, making it taller than Row 0.
        assert!(t.row_heights[1] > t.row_heights[0]);
    }

    #[test]
    fn custom_data_cell_style_sizes_wrapped_rows_and_columns() {
        let long_cell = "Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
        let tsv = format!("Short\t{long_cell}\nA\tB");

        let mut base = Session::new();
        let r_base = base.execute("pastetable", &json!({ "tsv": tsv, "at": [0, 0] })).unwrap();
        let h_base = cadcraft_doc::Handle::parse_hex(r_base["handle"].as_str().unwrap()).unwrap();
        let EntityKind::Table(t_base) = &base.doc().unwrap().entity(h_base).unwrap().kind else { panic!("expected table") };

        let mut custom = Session::new();
        custom.doc_mut().unwrap().table_styles.iter_mut().find(|ts| ts.name.eq_ignore_ascii_case("Standard")).unwrap().set_cell_style(CellStyle {
            name: CellStyle::DATA.into(),
            text_height: 5.0,
            margin_h: 4.0,
            margin_v: 3.0,
            ..CellStyle::default()
        });
        let r_custom = custom.execute("pastetable", &json!({ "tsv": tsv, "at": [0, 0] })).unwrap();
        let h_custom = cadcraft_doc::Handle::parse_hex(r_custom["handle"].as_str().unwrap()).unwrap();
        let EntityKind::Table(t_custom) = &custom.doc().unwrap().entity(h_custom).unwrap().kind else { panic!("expected table") };

        assert!(t_custom.col_widths[1] > t_base.col_widths[1]);
        assert!(t_custom.row_heights[0] > t_base.row_heights[0]);
        assert!(t_custom.row_heights[0] > t_custom.row_heights[1]);

        let g = cadcraft_render::table_geometry(custom.doc().unwrap(), t_custom);
        let tx = g.texts.iter().find(|tx| tx.row == 0 && tx.col == 1).unwrap();
        let laid = cadcraft_fonts::layout_mtext_with(
            &tx.text,
            &cadcraft_fonts::MTextParams { width: tx.wrap_width, ..cadcraft_fonts::MTextParams::new(Vec2::ZERO, tx.height) },
        );
        assert!(laid.lines.len() > 1);
        assert!(t_custom.row_heights[0] >= laid.lines.len() as f64 * tx.height * 5.0 / 3.0 + 2.0 * 3.0 - 1e-6);
    }

    #[test]
    fn cell_wrapping_past_fifty_lines_is_not_clamped() {
        let mut s = Session::new();
        let long_cell = vec!["word"; 800].join(" ");
        let res = s.execute("pastetable", &json!({ "tsv": format!("A\t{long_cell}\n1\t2"), "at": [0, 0], "text_height": 2.0 })).unwrap();
        let h = cadcraft_doc::Handle::parse_hex(res["handle"].as_str().unwrap()).unwrap();
        let EntityKind::Table(t) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("expected table") };

        let g = cadcraft_render::table_geometry(s.doc().unwrap(), t);
        let tx = g.texts.iter().find(|tx| tx.row == 0 && tx.col == 1).unwrap();
        let laid = cadcraft_fonts::layout_mtext_with(
            &tx.text,
            &cadcraft_fonts::MTextParams { width: tx.wrap_width, ..cadcraft_fonts::MTextParams::new(Vec2::ZERO, tx.height) },
        );
        assert!(laid.lines.len() > 50, "expected >50 wrapped lines, got {}", laid.lines.len());
        let st = style_of(&s);
        let margin_v = st.margin * (2.0 / st.text_height);
        let fifty_line_h = 2.0 * (50.0 * 5.0 / 3.0) + 2.0 * margin_v;
        assert!(t.row_heights[0] > fifty_line_h);
        assert!((t.row_heights[0] - (2.0 * (laid.lines.len() as f64 * 5.0 / 3.0) + 2.0 * margin_v)).abs() < 1e-9);
    }
}
