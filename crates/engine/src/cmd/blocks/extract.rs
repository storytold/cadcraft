//! DATAEXTRACTION / EATTEXT: block references and their attribute values as rows — counted by
//! block name and values, or one row per reference with its handle, layer, position and
//! rotation — returned, written to a CSV file and/or inserted as a table. Tags match in any case.

use std::collections::BTreeMap;

use cadcraft_doc::{EntityKind, Insert};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::cmd::curves::KW;
use crate::cmd::qselect::wildcard;
use crate::cmd::table::make_table;
use crate::cmd::{CommandSpec, bad, bool_or, point_param, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Table size limits (the TABLE command's).
const MAX_TABLE_ROWS: usize = 2000;
const MAX_TABLE_COLS: usize = 200;
/// Attribute columns kept at most.
const MAX_TAGS: usize = 190;

const PARAMS: &str = "{blocks?: \"A*,B\" (block names, wildcards, default *), tags?: attribute tags to include (wildcards, default *), handles? (default: the block references in model space and every layout), combine?: bool (default true: identical rows once, with a Count column; false: one row per reference with handle, layer, position and rotation), file?: path (.csv; .txt is tab-separated), at?: [x,y] (insert a table there), title?: table title} → {columns, rows, file?, table?}";

pub(super) fn dataextraction_spec() -> CommandSpec {
    CommandSpec::new("dataextraction", "Data Extraction...", run)
        .menu(&["Tools", "Data Extraction..."])
        .alias(&["dx"])
        .params(PARAMS)
        .interactive(|_| Ok(Box::new(ExtractM::default())))
}

/// EATTEXT: the older name of the same command.
pub(super) fn eattext_spec() -> CommandSpec {
    CommandSpec::new("eattext", "Data Extraction...", run).params(PARAMS).interactive(|_| Ok(Box::new(ExtractM::default())))
}

/// A cell: a count, a number or text.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Count(u64),
    Num(f64),
    Text(String),
}

impl Cell {
    fn json(&self) -> Value {
        match self {
            Cell::Count(n) => json!(n),
            Cell::Num(v) => json!(v),
            Cell::Text(t) => json!(t),
        }
    }
    fn text(&self) -> String {
        match self {
            Cell::Count(n) => n.to_string(),
            Cell::Num(v) => {
                let t = format!("{v:.4}");
                let t = if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t };
                if t == "-0" { "0".into() } else { t }
            }
            Cell::Text(t) => t.clone(),
        }
    }
}

/// What to extract.
#[derive(Clone, Debug)]
struct Options {
    blocks: String,
    tags: String,
    combine: bool,
}

/// One block reference found: block name, handle, layer, position, rotation (degrees), and its
/// attribute values by tag (in capitals), constant ones from the block definition included.
struct Found {
    block: String,
    handle: String,
    layer: String,
    at: Vec2,
    rotation: f64,
    values: Vec<(String, String)>,
}

/// A spec of comma-separated wildcards matching `text` (any case).
fn matches(spec: &str, text: &str) -> bool {
    spec.split(',').any(|p| wildcard(p.trim(), text))
}

/// The attribute values of `ins`: its own, then the constant ones of its block.
fn values(d: &cadcraft_doc::Drawing, ins: &Insert) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = ins.attribs.iter().map(|a| (a.tag.to_ascii_uppercase(), a.text.value.clone())).collect();
    if let Some(b) = d.block(&ins.block) {
        for e in b.entities.iter() {
            if let EntityKind::AttDef(a) = &e.kind
                && a.constant
                && !v.iter().any(|(t, _)| t.eq_ignore_ascii_case(&a.tag))
            {
                v.push((a.tag.to_ascii_uppercase(), a.text.value.clone()));
            }
        }
    }
    v
}

/// The block references to extract from: `handles` when given, else model space and the layouts.
fn found(s: &Session, p: &Value, o: &Options) -> Result<Vec<Found>> {
    let d = s.doc()?;
    let ents: Vec<std::sync::Arc<cadcraft_doc::Entity>> = if p.get("handles").is_some() || p.get("handle").is_some() {
        targets(s, p)?.iter().filter_map(|h| d.entity(*h).cloned()).collect()
    } else {
        d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).cloned().collect()
    };
    let mut out = Vec::new();
    for e in &ents {
        let EntityKind::Insert(ins) = &e.kind else { continue };
        let anonymous = ins.block.starts_with('*') || d.block(&ins.block).is_some_and(|b| b.anonymous);
        if anonymous || !matches(&o.blocks, &ins.block) {
            continue;
        }
        let block = d.block(&ins.block).map(|b| b.name.clone()).unwrap_or_else(|| ins.block.clone());
        out.push(Found {
            block,
            handle: e.handle.hex(),
            layer: e.common.layer.clone(),
            at: ins.insert.xy(),
            rotation: ins.rotation.to_degrees(),
            values: values(d, ins),
        });
    }
    Ok(out)
}

/// Columns and rows for what was found.
fn table(found: &[Found], o: &Options) -> (Vec<String>, Vec<Vec<Cell>>) {
    let mut tags: Vec<String> = Vec::new();
    for f in found {
        for (t, _) in &f.values {
            if tags.len() < MAX_TAGS && matches(&o.tags, t) && !tags.contains(t) {
                tags.push(t.clone());
            }
        }
    }
    let attrs = |f: &Found| -> Vec<Cell> {
        tags.iter().map(|t| Cell::Text(f.values.iter().find(|(k, _)| k == t).map(|(_, v)| v.clone()).unwrap_or_default())).collect()
    };
    let mut columns: Vec<String>;
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    if o.combine {
        columns = vec!["Count".into(), "Name".into()];
        // Rows keyed by block name (any case) and values, sorted by them.
        let mut counted: BTreeMap<(String, Vec<String>), (String, Vec<Cell>, u64)> = BTreeMap::new();
        for f in found {
            let a = attrs(f);
            let key = (f.block.to_ascii_uppercase(), a.iter().map(Cell::text).collect());
            let entry = counted.entry(key).or_insert_with(|| (f.block.clone(), a, 0));
            entry.2 = entry.2.saturating_add(1);
        }
        for (_, (name, a, n)) in counted {
            let mut r = vec![Cell::Count(n), Cell::Text(name)];
            r.extend(a);
            rows.push(r);
        }
    } else {
        columns = ["Name", "Handle", "Layer", "Position X", "Position Y", "Rotation"].map(String::from).to_vec();
        let mut sorted: Vec<&Found> = found.iter().collect();
        sorted.sort_by_key(|f| f.block.to_ascii_uppercase());
        for f in sorted {
            let mut r = vec![
                Cell::Text(f.block.clone()),
                Cell::Text(f.handle.clone()),
                Cell::Text(f.layer.clone()),
                Cell::Num(f.at.x),
                Cell::Num(f.at.y),
                Cell::Num(f.rotation),
            ];
            r.extend(attrs(f));
            rows.push(r);
        }
    }
    columns.extend(tags);
    (columns, rows)
}

/// `text` as a CSV field: quoted when it holds a separator, quote or line break.
fn csv_field(text: &str, sep: char) -> String {
    if text.contains([sep, '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.to_string() }
}

/// The rows as CSV (or tab-separated with `sep` '\t'), header first.
fn csv(columns: &[String], rows: &[Vec<Cell>], sep: char) -> String {
    let line = |cells: Vec<String>| cells.iter().map(|c| csv_field(c, sep)).collect::<Vec<_>>().join(&sep.to_string());
    let mut out = line(columns.to_vec());
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&line(r.iter().map(Cell::text).collect()));
        out.push_str("\r\n");
    }
    out
}

fn write_file(path: &str, text: &str) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::consent::check_write(path).map_err(|e| EngineError::Other(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| EngineError::Other(format!("{path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = text;
        Err(EngineError::Other(format!("{path}: files can't be written in the browser")))
    }
}

/// Insert the rows as a table at `at` (a header row of the column names, an optional title); the
/// caller checks the size against the table limits.
fn insert_table(s: &mut Session, at: Vec2, title: Option<&str>, columns: &[String], rows: &[Vec<Cell>]) -> Result<String> {
    let st = {
        let d = s.doc()?;
        let cur = d.header.str("CTABLESTYLE", "Standard");
        d.table_styles.iter().find(|t| t.name.eq_ignore_ascii_case(&cur)).or(d.table_styles.first()).cloned().unwrap_or_default()
    };
    let cells: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(Cell::text).collect()).collect();
    let mut t = make_table(&st, at, rows.len(), columns.len(), None, None, title, Some(columns.to_vec()), &cells);
    // Columns as wide as their longest text.
    for (c, w) in t.col_widths.iter_mut().enumerate() {
        let chars = std::iter::once(columns.get(c).map_or(0, |h| h.chars().count()))
            .chain(cells.iter().map(|r| r.get(c).map_or(0, |v| v.chars().count())))
            .max()
            .unwrap_or(0)
            .clamp(1, 80);
        *w = chars as f64 * st.text_height * 0.9 + 2.0 * st.margin + st.text_height;
    }
    Ok(s.add_entity(EntityKind::Table(t))?.hex())
}

/// Extract; write `file` and insert a table at `at` when given.
fn extract(s: &mut Session, p: &Value, o: &Options, file: Option<&str>, at: Option<Vec2>, title: Option<&str>) -> Result<Value> {
    let found = found(s, p, o)?;
    let (columns, rows) = table(&found, o);
    let mut r = json!({
        "columns": columns,
        "rows": rows.iter().map(|r| r.iter().map(Cell::json).collect::<Vec<_>>()).collect::<Vec<_>>(),
    });
    if at.is_some() && (rows.len().saturating_add(2) > MAX_TABLE_ROWS || columns.len() > MAX_TABLE_COLS) {
        return Err(EngineError::Other(format!("Too much data for a table ({} rows, {} columns).", rows.len(), columns.len())));
    }
    if let Some(path) = file {
        let tab = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"));
        write_file(path, &csv(&columns, &rows, if tab { '\t' } else { ',' }))?;
        r["file"] = json!(path);
    }
    if let Some(at) = at {
        r["table"] = json!(insert_table(s, at, title, &columns, &rows)?);
    }
    Ok(r)
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let o = Options {
        blocks: str_param(p, "blocks").filter(|b| !b.trim().is_empty()).unwrap_or("*").to_string(),
        tags: str_param(p, "tags").filter(|b| !b.trim().is_empty()).unwrap_or("*").to_string(),
        combine: bool_or(p, "combine", true),
    };
    let at = match p.get("at") {
        None | Some(Value::Null) => None,
        Some(_) => Some(point_param(p, "at").ok_or_else(|| bad("dataextraction", "`at` must be a point [x, y]"))?),
    };
    let file = str_param(p, "file").filter(|f| !f.trim().is_empty());
    extract(s, p, &o, file, at, str_param(p, "title"))
}

// ---------------- command line ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Blocks,
    Combine,
    Output,
    Point,
    File,
}

/// DATAEXTRACTION at the command line: blocks, combine, then a table and/or a file.
#[derive(Default)]
struct ExtractM {
    stage: Stage,
    blocks: Option<String>,
    combine: bool,
    table: bool,
    file: bool,
    at: Option<Vec2>,
}

impl ExtractM {
    fn options(&self) -> Options {
        Options { blocks: self.blocks.clone().unwrap_or_else(|| "*".into()), tags: "*".into(), combine: self.combine }
    }

    /// The default file name: the drawing's, with a .csv extension.
    fn default_file(s: &Session) -> String {
        let Ok(st) = s.state() else { return "extraction.csv".into() };
        let base = st.path.clone().unwrap_or_else(|| st.title.clone());
        std::path::Path::new(&base).with_extension("csv").to_string_lossy().into_owned()
    }

    fn finish(&mut self, s: &mut Session, file: Option<&str>) -> Result<Step> {
        let r = extract(s, &json!({}), &self.options(), file, self.at, None)?;
        let n = r["rows"].as_array().map_or(0, Vec::len);
        s.echo(format!("{n} row{} extracted.", if n == 1 { "" } else { "s" }));
        if let Some(f) = r["file"].as_str() {
            s.echo(format!("Written to {f}."));
        }
        Ok(Step::Done)
    }

    fn after_point(&mut self, s: &mut Session) -> Result<Step> {
        if self.file {
            self.stage = Stage::File;
            Ok(Step::Continue)
        } else {
            self.finish(s, None)
        }
    }
}

impl Interactive for ExtractM {
    fn name(&self) -> &'static str {
        "DATAEXTRACTION"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            Stage::Blocks => Prompt::new("Enter block names to extract", Accept::TEXT).default("*"),
            Stage::Combine => Prompt::new("Combine identical rows?", KW).kw(&["Yes", "No"]).default("Yes"),
            Stage::Output => Prompt::new("Where should the data go?", KW).kw(&["Table", "File", "Both"]).default("Table"),
            Stage::Point => Prompt::new("Specify insertion point of the table", Accept::POINT),
            Stage::File => Prompt::new("Enter file name", Accept::TEXT).default(Self::default_file(s)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Blocks => {
                let spec = match &i {
                    Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
                    Input::Text(_) | Input::Enter => "*".into(),
                    _ => return Ok(Step::Continue),
                };
                self.blocks = Some(spec);
                self.stage = Stage::Combine;
            }
            Stage::Combine => {
                self.combine = match &i {
                    Input::Keyword(k) if k == "No" => false,
                    Input::Keyword(_) | Input::Enter => true,
                    _ => return Err(EngineError::Other("Invalid option keyword.".into())),
                };
                self.stage = Stage::Output;
            }
            Stage::Output => {
                (self.table, self.file) = match &i {
                    Input::Keyword(k) if k == "File" => (false, true),
                    Input::Keyword(k) if k == "Both" => (true, true),
                    Input::Keyword(_) | Input::Enter => (true, false),
                    _ => return Err(EngineError::Other("Invalid option keyword.".into())),
                };
                self.stage = if self.table { Stage::Point } else { Stage::File };
            }
            Stage::Point => match i {
                Input::Point(p) => {
                    self.at = Some(p);
                    return self.after_point(s);
                }
                Input::Enter => return Ok(Step::Cancel),
                _ => {}
            },
            Stage::File => {
                let f = match &i {
                    Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
                    Input::Text(_) | Input::Enter => Self::default_file(s),
                    _ => return Ok(Step::Continue),
                };
                return self.finish(s, Some(&f));
            }
        }
        Ok(Step::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_separators_quotes_and_line_breaks() {
        assert_eq!(csv_field("plain", ','), "plain");
        assert_eq!(csv_field("a,b", ','), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\"", ','), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("a,b", '\t'), "a,b");
        assert_eq!(Cell::Num(-0.00001).text(), "0");
        assert_eq!(Cell::Num(2.5).text(), "2.5");
    }
}
