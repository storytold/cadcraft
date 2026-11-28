//! Table breaking (`table.break`) and data links (DATALINK, DATALINKUPDATE: cells filled from a
//! CSV file).

use cadcraft_doc::{BreakDirection, DataLink, EntityKind, Handle, Table, TableBreak, TableCell};
use serde_json::{Value, json};

use super::cells::cell_of;
use super::{MAX_COLS, MAX_ROWS, anchors, edit, set_anchors, usize_param};
use crate::cmd::machines::SelectRun;
use crate::cmd::{CommandSpec, bad, bool_or, str_param};
use crate::{Accept, DocState, Input, Interactive, Prompt, Result, Session, Step};

/// Largest linked file read (bytes).
#[cfg(not(target_arch = "wasm32"))]
const MAX_LINK_BYTES: usize = 8 << 20;
/// Most files remembered per drawing as linked by the user ([`DocState::chosen_links`]).
const MAX_CHOSEN_LINKS: usize = 1000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("table.break", "Table Breaks", run_break)
            .params(
                "{handle, enabled?, direction?: right|left|down, repeatTop?, repeatBottom?, height? (largest part height), heights?: [first parts], \
                 spacing?, resetPositions?: bool} → {parts}",
            )
            .interactive(|_| Ok(Box::new(BreakM::default()))),
        CommandSpec::new("datalink", "Data Link...", run_datalink)
            .menu(&["Insert", "Data Link..."])
            .params("{handle, path (.csv, .txt/.tsv tab-separated), startRow? (default: below the title), remove?: bool} → {rows, cols}"),
        CommandSpec::new("datalinkupdate", "Update Data Links", run_update)
            .params(
                "{handles?} (default: every linked table) → {updated, errors}; a link saved in the drawing is read only from the drawing's \
                 folder or below it, unless DATALINK linked that file since the drawing was opened",
            )
            .interactive(|_| Ok(Box::new(SelectRun::new("datalinkupdate", "DATALINKUPDATE")))),
    ]
}

// ---------------------------------------------------------------- table breaking

fn run_break(s: &mut Session, p: &Value) -> Result<Value> {
    let id = "table.break";
    let num = |k: &str| -> Result<Option<f64>> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x <= 1e12).map(Some).ok_or_else(|| bad(id, format!("invalid {k}: {v}"))),
        }
    };
    let height = num("height")?;
    let spacing = num("spacing")?;
    let heights: Option<Vec<f64>> = match p.get("heights") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(
            a.iter()
                .take(cadcraft_doc::MAX_TABLE_PARTS)
                .map(|v| v.as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x <= 1e12))
                .collect::<Option<Vec<f64>>>()
                .ok_or_else(|| bad(id, "`heights` must be non-negative numbers"))?,
        ),
        Some(v) => return Err(bad(id, format!("invalid heights: {v}"))),
    };
    let direction = match str_param(p, "direction").map(|d| d.trim().to_ascii_lowercase()) {
        None => None,
        Some(d) => Some(match d.as_str() {
            "right" | "r" => BreakDirection::Right,
            "left" | "l" => BreakDirection::Left,
            "down" | "d" => BreakDirection::Down,
            _ => return Err(bad(id, format!("invalid direction {d} (right, left, down)"))),
        }),
    };
    let r = edit(s, p, id, |t| {
        let mut b = t.breaks.clone().unwrap_or_default();
        // Giving a height switches breaking on unless `enabled` says otherwise.
        b.enabled = p.get("enabled").and_then(Value::as_bool).unwrap_or(b.enabled || height.is_some_and(|h| h > 0.0));
        if let Some(d) = direction {
            b.direction = d;
        }
        if let Some(v) = p.get("repeatTop").and_then(Value::as_bool) {
            b.repeat_top = v;
        }
        if let Some(v) = p.get("repeatBottom").and_then(Value::as_bool) {
            b.repeat_bottom = v;
        }
        if let Some(h) = height {
            b.height = h;
        }
        if let Some(h) = heights {
            b.heights = h;
        }
        if let Some(sp) = spacing {
            b.spacing = sp;
        }
        if bool_or(p, "resetPositions", false) {
            b.offsets.clear();
        }
        t.breaks = if b == TableBreak::default() { None } else { Some(b) };
        Ok(())
    })?;
    let h = r.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex);
    let parts = match h.and_then(|h| s.doc().ok()?.entity(h)).map(|e| &e.kind) {
        Some(EntityKind::Table(t)) => t.parts().len(),
        _ => 1,
    };
    Ok(json!({ "handle": r["handle"], "parts": parts }))
}

/// `table.break` at the command line: select a table, then the part height (or Off).
#[derive(Default)]
struct BreakM {
    table: Option<Handle>,
}

impl Interactive for BreakM {
    fn name(&self) -> &'static str {
        "TABLEBREAK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.table {
            None => Prompt::new("Select a table", Accept::SELECT),
            Some(_) => Prompt::new("Enter maximum part height", Accept::POINT_OR_NUMBER).kw(&["Off"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let table_at = |s: &Session, p| {
            let d = s.doc().ok()?;
            d.space(&s.space())?.iter().find(|e| matches!(&e.kind, EntityKind::Table(t) if cell_of(t, p).is_some())).map(|e| e.handle)
        };
        match (self.table, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (None, Input::Pick(hs)) => {
                let d = s.doc()?;
                self.table = hs.into_iter().find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Table(_))));
                if self.table.is_none() {
                    s.echo("The object is not a table.");
                }
                Ok(Step::Continue)
            }
            (None, Input::Point(p)) => {
                self.table = table_at(s, p);
                if self.table.is_none() {
                    s.echo("No table there.");
                }
                Ok(Step::Continue)
            }
            (None, Input::Enter) => Ok(Step::Cancel),
            (None, _) => Ok(Step::Continue),
            (Some(_), Input::Enter) => Ok(Step::Done),
            (Some(h), Input::Text(t) | Input::Keyword(t)) => {
                let params = if t.trim().eq_ignore_ascii_case("off") || t.trim().eq_ignore_ascii_case("o") {
                    json!({ "handle": h.hex(), "enabled": false })
                } else {
                    match super::super::machines::number(&t).filter(|v| v.is_finite() && *v > 0.0) {
                        Some(v) => json!({ "handle": h.hex(), "height": v, "enabled": true }),
                        None => {
                            s.echo("Requires a positive number or Off.");
                            return Ok(Step::Continue);
                        }
                    }
                };
                let r = run_break(s, &params)?;
                s.echo(format!("Table in {} part(s).", r["parts"]));
                Ok(Step::Done)
            }
            (Some(_), _) => Ok(Step::Continue),
        }
    }
}

// ---------------------------------------------------------------- data links

/// Parse CSV text (quotes with doubled quotes inside, CRLF or LF), bounded to the table limits.
pub(crate) fn parse_csv(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let push_field = |row: &mut Vec<String>, field: &mut String| {
        if row.len() < MAX_COLS {
            row.push(std::mem::take(field));
        } else {
            field.clear();
        }
    };
    while let Some(c) = chars.next() {
        if rows.len() >= MAX_ROWS {
            break;
        }
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == sep {
            push_field(&mut row, &mut field);
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            push_field(&mut row, &mut field);
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(c);
        }
    }
    if (!field.is_empty() || !row.is_empty()) && rows.len() < MAX_ROWS {
        push_field(&mut row, &mut field);
        rows.push(row);
    }
    rows
}

/// The separator of a linked file: tab for .txt/.tsv, else `;` when the first line has semicolons
/// but no commas, else `,`.
fn separator(path: &str, text: &str) -> char {
    let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "txt" || ext == "tsv" {
        return '\t';
    }
    let first = text.lines().next().unwrap_or("");
    if first.contains(';') && !first.contains(',') { ';' } else { ',' }
}

/// The file to read for a data link of the drawing `st`: a path the user just `chosen` as given;
/// a path stored in the drawing through [`crate::doc_path::resolve`], and then only as the file
/// system finds it (`..` and links followed) in the drawing's `folder`
/// ([`crate::doc_path::drawing_folder`]) or below it, or when the user linked that same file since
/// the drawing was opened ([`DocState::chosen_links`]).
fn link_file(id: &str, path: &str, st: &DocState, folder: Option<&std::path::Path>, chosen: bool) -> Result<std::path::PathBuf> {
    if chosen {
        return Ok(std::path::PathBuf::from(path));
    }
    let host = st.path.as_deref().map(std::path::Path::new);
    let file = crate::doc_path::resolve(path, host).map_err(|r| bad(id, format!("{path}: not opened, {r}")))?;
    if cfg!(target_arch = "wasm32") {
        // Nothing is read there; [`read_link`] says so.
        return Ok(file);
    }
    // The automation roots first: outside them nothing about the file is asked.
    crate::file_access::check_read(&file).map_err(|e| bad(id, e))?;
    let real = std::fs::canonicalize(&file).map_err(|e| bad(id, format!("{path}: {e}")))?;
    // Whole components: `plans-old` is not in `plans`.
    if folder.is_some_and(|f| real.starts_with(f)) || st.chosen_links.contains_key(&real) {
        // Read where it was checked, not through the links again.
        return Ok(real);
    }
    Err(bad(id, format!("{path}: not read, the file is outside the drawing's folder; link it with DATALINK to read it")))
}

/// Remember `real` (resolved), the file of a link the user just gave, as theirs to read for the
/// drawing `st`, as its latest choice. At [`MAX_CHOSEN_LINKS`] files a new one makes room by
/// dropping the file chosen longest ago.
fn remember_link(st: &mut DocState, real: std::path::PathBuf) {
    let links = &mut st.chosen_links;
    let latest = links.values().max().map_or(0, |n| n.saturating_add(1));
    if links.len() >= MAX_CHOSEN_LINKS
        && !links.contains_key(&real)
        && let Some(oldest) = links.iter().min_by_key(|(_, n)| **n).map(|(p, _)| p.clone())
    {
        links.remove(&oldest);
    }
    links.insert(real, latest);
}

/// Read a linked file through the host (natively the file system; not on the web).
fn read_link(id: &str, file: &std::path::Path) -> Result<String> {
    let path = file.display();
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::file_access::check_read(file).map_err(|e| bad(id, e))?;
        // The limit is enforced on the bytes read, not on reported metadata, so a device or FIFO
        // that misreports its length can't be buffered to EOF.
        let bytes = crate::capped_read::read_capped(file, MAX_LINK_BYTES).map_err(|e| bad(id, format!("{path}: {e}")))?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        Err(bad(id, "data links can't read files in the browser"))
    }
}

/// Put `data` into `t` from row `start`: the table gets exactly the rows and columns needed.
fn fill(t: &mut Table, start: usize, data: &[Vec<String>]) {
    let cols = data.iter().map(Vec::len).max().unwrap_or(1).clamp(1, MAX_COLS);
    let start = start.min(t.cells.len());
    let rows = (start + data.len().max(1)).min(MAX_ROWS);
    let row_h = t.row_heights.last().copied().unwrap_or(t.text_height * 5.0 / 3.0);
    let col_w = t.col_widths.last().copied().unwrap_or(2.5);
    // Keep merges above the data, cut at the new size.
    let a: Vec<_> = anchors(t).into_iter().filter(|(r, ..)| *r < start).collect();
    t.row_heights.resize(rows, row_h);
    t.col_widths.resize(cols, col_w);
    t.cells.resize_with(rows, Vec::new);
    t.row_styles.truncate(rows);
    for row in &mut t.cells {
        row.resize_with(cols, TableCell::default);
    }
    for (i, row) in t.cells.iter_mut().enumerate().skip(start) {
        let src = data.get(i - start);
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = TableCell {
                text: src.and_then(|r| r.get(c)).cloned().unwrap_or_default(),
                style: std::mem::take(&mut cell.style),
                ..TableCell::default()
            };
        }
    }
    set_anchors(t, &a);
    // A title merged across the table stays across.
    if t.title
        && start > 0
        && let Some(cell) = t.cells.first_mut().and_then(|r| r.first_mut())
    {
        cell.merged = (cols > 1).then_some((1, cols as u32));
    }
}

/// Re-read the linked file of table `h`; `chosen` says its path was just given by the user, else
/// it is read from the drawing's `folder` ([`crate::doc_path::drawing_folder`]) or a file the user
/// linked.
fn update(s: &mut Session, h: Handle, folder: Option<&std::path::Path>, chosen: bool) -> Result<(usize, usize)> {
    let id = "datalinkupdate";
    let link = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Table(t)) => t.data_link.clone().ok_or_else(|| bad(id, "the table has no data link"))?,
        _ => return Err(bad(id, "object is not a table")),
    };
    let file = link_file(id, &link.path, s.state()?, folder, chosen)?;
    let text = read_link(id, &file)?;
    if chosen && let Ok(real) = std::fs::canonicalize(&file) {
        remember_link(s.state_mut()?, real);
    }
    let data = parse_csv(&text, separator(&link.path, &text));
    let mut size = (0, 0);
    edit(s, &json!({ "handle": h.hex() }), id, |t| {
        fill(t, link.start_row, &data);
        size = (t.cells.len(), t.col_widths.len());
        Ok(())
    })?;
    Ok(size)
}

fn run_datalink(s: &mut Session, p: &Value) -> Result<Value> {
    let id = "datalink";
    if bool_or(p, "remove", false) {
        edit(s, p, id, |t| {
            t.data_link = None;
            Ok(())
        })?;
        return Ok(json!({ "removed": true }));
    }
    let path = str_param(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(id, "`path` of a CSV file is required"))?.to_string();
    let start = usize_param(p, "startRow");
    let r = edit(s, p, id, |t| {
        let start_row = start.unwrap_or(usize::from(t.title)).min(t.cells.len());
        t.data_link = Some(DataLink { path: path.clone(), start_row });
        Ok(())
    })?;
    let h = r.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| bad(id, "no table"))?;
    let (rows, cols) = update(s, h, None, true)?;
    Ok(json!({ "handle": h.hex(), "rows": rows, "cols": cols }))
}

fn run_update(s: &mut Session, p: &Value) -> Result<Value> {
    let given: Option<Vec<Handle>> = p
        .get("handles")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect())
        .or_else(|| p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex).map(|h| vec![h]));
    let hs: Vec<Handle> = {
        let d = s.doc()?;
        match given {
            Some(hs) => {
                hs.into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Table(t)) if t.data_link.is_some())).collect()
            }
            None => std::iter::once(&d.model)
                .chain(d.layouts.iter().map(|l| &l.entities))
                .flat_map(|st| st.iter())
                .filter(|e| matches!(&e.kind, EntityKind::Table(t) if t.data_link.is_some()))
                .map(|e| e.handle)
                .collect(),
        }
    };
    let mut updated = 0;
    let mut errors = Vec::new();
    // Resolved once for every table, and only when there is one.
    let folder = if hs.is_empty() { None } else { crate::doc_path::drawing_folder(s.state()?.path.as_deref().map(std::path::Path::new)) };
    for h in hs {
        match update(s, h, folder.as_deref(), false) {
            Ok(_) => updated += 1,
            Err(e) => errors.push(e.to_string()),
        }
    }
    let mut msg = format!("{updated} data link(s) updated.");
    for e in &errors {
        msg.push('\n');
        msg.push_str(e);
    }
    Ok(json!({ "updated": updated, "errors": errors, "message": msg }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linked path naming an endless device is refused instead of buffered to EOF.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn link_to_endless_device_is_rejected() {
        let dev = std::path::Path::new("/dev/zero");
        if dev.exists() {
            assert!(read_link("datalink", dev).is_err());
        }
    }

    /// Whether the drawing `st` remembers `file` as linked by the user.
    fn remembered(st: &DocState, file: &std::path::Path) -> bool {
        st.chosen_links.contains_key(file)
    }

    /// However many files the user links, the drawing remembers at most [`MAX_CHOSEN_LINKS`] of
    /// them: the ones linked last, a file linked again counting as just linked.
    #[test]
    fn remembered_links_are_the_most_recent_ones() {
        let mut st = DocState::new(cadcraft_doc::Drawing::new_metric(), "t", None);
        let file = |i: usize| std::path::PathBuf::from(format!("/data/{i}.csv"));
        for i in 0..MAX_CHOSEN_LINKS {
            remember_link(&mut st, file(i));
        }
        // Linked again at the bound, the second file drops nothing and becomes the newest, so two
        // new files make room by dropping the first and the third.
        remember_link(&mut st, file(1));
        assert_eq!(st.chosen_links.len(), MAX_CHOSEN_LINKS);
        assert!(remembered(&st, &file(0)));
        remember_link(&mut st, file(MAX_CHOSEN_LINKS));
        remember_link(&mut st, file(MAX_CHOSEN_LINKS + 1));
        assert!([1, MAX_CHOSEN_LINKS, MAX_CHOSEN_LINKS + 1].iter().all(|&i| remembered(&st, &file(i))));
        assert!(!remembered(&st, &file(0)) && !remembered(&st, &file(2)));
        // After many more, the ones linked last are the ones remembered.
        let end = 2 * MAX_CHOSEN_LINKS + 10;
        for i in MAX_CHOSEN_LINKS + 2..end {
            remember_link(&mut st, file(i));
        }
        assert_eq!(st.chosen_links.len(), MAX_CHOSEN_LINKS, "the bound is reached, not a smaller one");
        assert!((end - MAX_CHOSEN_LINKS..end).all(|i| remembered(&st, &file(i))));
    }

    #[test]
    fn csv_parsing() {
        assert_eq!(parse_csv("a,b\r\n\"c,d\",\"e\"\"f\"\n", ','), vec![vec!["a", "b"], vec!["c,d", "e\"f"]]);
        assert_eq!(parse_csv("x;y", ';'), vec![vec!["x", "y"]]);
        assert_eq!(parse_csv("", ','), Vec::<Vec<String>>::new());
        let wide = ",".repeat(10_000);
        assert_eq!(parse_csv(&wide, ',').first().map(Vec::len), Some(MAX_COLS));
        let tall = "1\n".repeat(10_000);
        assert_eq!(parse_csv(&tall, ',').len(), MAX_ROWS);
        assert_eq!(separator("a.tsv", "x,y"), '\t');
        assert_eq!(separator("a.csv", "x;y"), ';');
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn link_strips_utf8_bom() {
        let path = std::env::temp_dir().join(format!("cadcraft_bom_link_{}.csv", std::process::id()));
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"\"Item, name\",Qty\r\nBolt,4\r\n");
        std::fs::write(&path, bytes).expect("write temp csv");
        let text = read_link("datalink", &path);
        let _ = std::fs::remove_file(&path);
        let text = text.expect("read link");
        let rows = parse_csv(&text, separator("bom.csv", &text));
        assert_eq!(rows, vec![vec!["Item, name", "Qty"], vec!["Bolt", "4"]]);
    }
}
