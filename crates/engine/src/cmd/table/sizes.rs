//! The table editor's commands on a range of cells: row heights and column widths (typed, or
//! fitted to the cells' text), and the cells' own alignment and text height.

use cadcraft_doc::{Drawing, Table};
use serde_json::Value;

use super::cells::align_value;
use super::{edit, usize_param};
use crate::cmd::CommandSpec;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("table.size", "Set Row Height / Column Width", run_size)
            .params("{handle, row?, rows? (default 1; no row: every row), col?, cols? (default 1; no col: every column), height?: row height, width?: column width}"),
        CommandSpec::new("table.fit", "Fit Rows / Columns to Contents", run_fit)
            .params("{handle, row?, rows?, col?, cols? (as table.size), width?: bool (columns to their widest text), height?: bool (rows to their text)} (neither: both)"),
        CommandSpec::new("table.cells", "Set Cell Properties", run_cells).params(
            "{handle, row?, rows?, col?, cols? (as table.size), align?: TL|TC|TR|ML|MC|MR|BL|BC|BR or 1..9 (null: the cell style's), textHeight?: number (null: the cell style's), clear?: bool (empty the texts)}",
        ),
    ]
}

/// The rows (or columns) `[start, end)` a range names in a table with `n` of them: `key`
/// (default: all of them) and `key + "s"` (default 1).
fn span(p: &Value, key: &str, many: &str, n: usize) -> std::result::Result<(usize, usize), String> {
    let Some(start) = usize_param(p, key) else { return Ok((0, n)) };
    if start >= n {
        return Err(format!("`{key}` {start} is outside the table ({n})"));
    }
    let count = usize_param(p, many).unwrap_or(1).max(1);
    Ok((start, start.saturating_add(count).min(n)))
}

fn positive(p: &Value, key: &str) -> std::result::Result<Option<f64>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 1e12).map(Some).ok_or_else(|| format!("`{key}` must be a positive number"))
        }
    }
}

fn run_size(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.size", |t| {
        let (height, width) = (positive(p, "height")?, positive(p, "width")?);
        if height.is_none() && width.is_none() {
            return Err("give `height` or `width`".into());
        }
        if let Some(h) = height {
            let (a, b) = span(p, "row", "rows", t.row_heights.len())?;
            t.row_heights.iter_mut().take(b).skip(a).for_each(|x| *x = h);
        }
        if let Some(w) = width {
            let (a, b) = span(p, "col", "cols", t.col_widths.len())?;
            t.col_widths.iter_mut().take(b).skip(a).for_each(|x| *x = w);
        }
        Ok(())
    })
}

/// Line spacing of cell text, in text heights (MTEXT's "at least" 1.0).
const LINE_SPACING: f64 = 5.0 / 3.0;

/// The text lines of a cell value: paragraphs (`\P`) and line breaks.
fn lines(text: &str) -> Vec<String> {
    text.replace("\\P", "\n").lines().map(|l| l.to_string()).collect()
}

/// What each cell needs to show its text: (row, col, rows, cols, width, height) for the cells
/// that have text, margins included.
fn needs(d: &Drawing, t: &Table) -> Vec<(usize, usize, usize, usize, f64, f64)> {
    let st = cadcraft_render::table_style_of(d, t);
    let k = Some(t.text_height / st.text_height).filter(|k| k.is_finite() && *k > 0.0).unwrap_or(1.0);
    let g = cadcraft_render::table_geometry(d, t);
    g.texts
        .iter()
        .map(|tx| {
            let cs = st.cell_style(t.cell_style_name(tx.row, tx.col));
            let ts = d.text_style(&tx.style).cloned().unwrap_or_default();
            let font = cadcraft_render::style_font(&ts);
            let wf = if ts.width_factor.is_finite() && ts.width_factor > 0.0 { ts.width_factor } else { 1.0 };
            let ls = lines(&tx.text);
            let w = ls.iter().map(|l| cadcraft_fonts::text_width(&font, l, tx.height, wf)).fold(0.0, f64::max);
            let n = ls.len().max(1) as f64;
            let h = tx.height * (1.0 + (n - 1.0) * LINE_SPACING);
            let (rs, cs_) = t
                .cells
                .get(tx.row)
                .and_then(|r| r.get(tx.col))
                .and_then(|c| c.merged)
                .map_or((1, 1), |(a, b)| (a.max(1) as usize, b.max(1) as usize));
            (tx.row, tx.col, rs, cs_, w + 2.0 * cs.margin_h * k, h + 2.0 * cs.margin_v * k)
        })
        .collect()
}

fn run_fit(s: &mut Session, p: &Value) -> Result<Value> {
    let flag = |k: &str| p.get(k).and_then(Value::as_bool);
    let (fw, fh) = match (flag("width"), flag("height")) {
        (None, None) => (true, true),
        (w, h) => (w.unwrap_or(false), h.unwrap_or(false)),
    };
    let doc = s.doc()?.clone();
    edit(s, p, "table.fit", |t| {
        let need = needs(&doc, t);
        let st = cadcraft_render::table_style_of(&doc, t);
        let k = Some(t.text_height / st.text_height).filter(|k| k.is_finite() && *k > 0.0).unwrap_or(1.0);
        if fw {
            let (a, b) = span(p, "col", "cols", t.col_widths.len())?;
            for c in a..b {
                let w = need.iter().filter(|n| n.1 == c && n.3 == 1).map(|n| n.4).fold(0.0, f64::max);
                // An empty column keeps room for a few characters.
                let min = 2.0 * t.text_height + 2.0 * st.margin * k;
                if let Some(x) = t.col_widths.get_mut(c) {
                    *x = w.max(min);
                }
            }
        }
        if fh {
            let (a, b) = span(p, "row", "rows", t.row_heights.len())?;
            for r in a..b {
                let h = need.iter().filter(|n| n.0 == r && n.2 == 1).map(|n| n.5).fold(0.0, f64::max);
                let min = t.text_height + 2.0 * st.margin * k;
                if let Some(x) = t.row_heights.get_mut(r) {
                    *x = h.max(min);
                }
            }
        }
        Ok(())
    })
}

fn run_cells(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "table.cells", |t| {
        let align = match p.get("align") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => Some(Some(align_value(v).ok_or_else(|| format!("invalid align {v} (TL, TC, TR, ML, MC, MR, BL, BC, BR)"))?)),
        };
        let height = match p.get("textHeight") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(_) => Some(positive(p, "textHeight")?),
        };
        let clear = p.get("clear").and_then(Value::as_bool) == Some(true);
        if align.is_none() && height.is_none() && !clear {
            return Err("give `align`, `textHeight` or `clear`".into());
        }
        let (r0, r1) = span(p, "row", "rows", t.cells.len())?;
        let (c0, c1) = span(p, "col", "cols", t.col_widths.len())?;
        for row in t.cells.iter_mut().take(r1).skip(r0) {
            for cell in row.iter_mut().take(c1).skip(c0) {
                if let Some(a) = align {
                    cell.align = a;
                }
                if let Some(h) = height {
                    cell.text_height = h;
                }
                if clear {
                    cell.text.clear();
                    cell.formula = None;
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;
    use cadcraft_doc::EntityKind;

    fn table(s: &Session, h: &str) -> cadcraft_doc::Table {
        let h = cadcraft_doc::Handle::parse_hex(h).unwrap();
        match &s.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::Table(t) => t.clone(),
            _ => panic!("not a table"),
        }
    }

    fn new_table(s: &mut Session) -> String {
        let v = s.execute("table", &json!({ "at": [0, 0], "rows": 3, "cols": 3, "rowHeight": 1, "colWidth": 5 })).unwrap();
        v["handle"].as_str().map(str::to_string).unwrap()
    }

    #[test]
    fn rows_and_columns_take_a_typed_size() {
        let mut s = Session::new();
        let h = new_table(&mut s);
        s.execute("table.size", &json!({ "handle": h, "col": 1, "cols": 2, "width": 8 })).unwrap();
        s.execute("table.size", &json!({ "handle": h, "height": 2.5 })).unwrap();
        let t = table(&s, &h);
        assert_eq!(t.col_widths, vec![5.0, 8.0, 8.0]);
        assert!(t.row_heights.iter().all(|r| *r == 2.5), "{:?}", t.row_heights);
        assert!(s.execute("table.size", &json!({ "handle": h, "col": 9, "width": 8 })).is_err());
        assert!(s.execute("table.size", &json!({ "handle": h, "width": -1 })).is_err());
        assert!(s.execute("table.size", &json!({ "handle": h })).is_err());
    }

    #[test]
    fn columns_fit_their_widest_text_and_rows_their_lines() {
        let mut s = Session::new();
        let h = new_table(&mut s);
        let rows = table(&s, &h).cells.len();
        let last = rows - 1;
        s.execute("table.set", &json!({ "handle": h, "row": last, "col": 0, "text": "A rather long piece of text that runs well past the five units the column starts with" })).unwrap();
        s.execute("table.set", &json!({ "handle": h, "row": last, "col": 1, "text": "one\\Ptwo\\Pthree" })).unwrap();
        s.execute("table.fit", &json!({ "handle": h, "col": 0, "width": true })).unwrap();
        let t = table(&s, &h);
        assert!(t.col_widths[0] > 5.0, "the long text widens its column: {:?}", t.col_widths);
        assert_eq!(t.col_widths[1], 5.0, "only the column asked for");
        s.execute("table.fit", &json!({ "handle": h, "row": last, "height": true })).unwrap();
        let t2 = table(&s, &h);
        let th = t2.text_height;
        assert!(t2.row_heights[last] > th * 3.0, "three lines: {:?}", t2.row_heights);
        // Fitting everything shrinks the wide empty column.
        s.execute("table.fit", &json!({ "handle": h })).unwrap();
        let t3 = table(&s, &h);
        assert!(t3.col_widths[2] < 5.0, "{:?}", t3.col_widths);
    }

    #[test]
    fn cells_take_their_own_alignment_and_text_height() {
        let mut s = Session::new();
        let h = new_table(&mut s);
        s.execute("table.cells", &json!({ "handle": h, "row": 1, "col": 0, "rows": 2, "cols": 2, "align": "MC", "textHeight": 0.5 })).unwrap();
        let t = table(&s, &h);
        assert_eq!(t.cells[1][0].align, Some(5));
        assert_eq!(t.cells[2][1].text_height, Some(0.5));
        assert_eq!(t.cells[1][2].align, None);
        s.execute("table.cells", &json!({ "handle": h, "align": null })).unwrap();
        assert_eq!(table(&s, &h).cells[1][0].align, None);
        assert!(s.execute("table.cells", &json!({ "handle": h, "align": "XX" })).is_err());
    }
}
