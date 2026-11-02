//! Plot styles: PLOTSTYLE (an object's or the current named plot style) and STYLESMANAGER (the
//! plot style tables: list, view, create, edit, delete, load and save in our own file format).
//!
//! Tables the drawing keeps live in `Drawing::plot_style_tables` and are saved with it; they
//! shadow built-in tables of the same name. Editing a built-in table stores an edited copy in
//! the drawing.

use cadcraft_doc::{Drawing, PlotStyle, PlotStyleKind, PlotStyleTable};
use serde_json::{Value, json};

use super::super::targets;
use super::{CommandSpec, Result, Session, bad, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("plotstyle", "Plot Style...", run_plotstyle)
            .menu(&["Format", "Plot Style..."])
            .params(
                "{name?: \"ByLayer\"|\"ByBlock\"|style name, handles?|handle? (default: the selection; nothing selected sets the current plot style for new objects, CPLOTSTYLE)} (no name: reports the current style and the named styles)",
            ),
        CommandSpec::new("stylesmanager", "Plot Style Manager...", run_stylesmanager)
            .menu(&["File", "Plot Styles..."])
            .params(
                "{action?: list (default)|get|new|set|delete|load|save, name?: table (\"x.ctb\"/\"x.stb\"), kind?: colorDependent|named (new), from?: table to copy (new), description?, style?: {name, color?: \"#rrggbb\"|null, grayscale?, screening?: 0-100, linetype?: name|null, lineweight?: mm|null, end?, join?, fill?, description?} (set), removeStyle?: name (set, named tables), path? (load/save; save without one returns `text`), text? (load)}",
            ),
    ]
}

// ---------- PLOTSTYLE ----------

/// The style names an object can be given: those of the named tables the layouts use, else of
/// every named table the drawing can use.
fn named_styles(d: &Drawing) -> Vec<String> {
    let mut used: Vec<PlotStyleTable> =
        d.layouts.iter().filter_map(|l| cadcraft_doc::plot_style_table(d, &l.page.plot_style_table)).filter(|t| !t.is_color_dependent()).collect();
    if used.is_empty() {
        used = cadcraft_doc::plot_style_table_names(d)
            .iter()
            .filter_map(|n| cadcraft_doc::plot_style_table(d, n))
            .filter(|t| !t.is_color_dependent())
            .collect();
    }
    let mut out: Vec<String> = Vec::new();
    for s in used.iter().flat_map(|t| t.styles.iter()) {
        if !out.iter().any(|n| n.eq_ignore_ascii_case(&s.name)) {
            out.push(s.name.clone());
        }
    }
    out
}

/// A plot style property value: `ByLayer`, `ByBlock` or a style name (at most 255 characters).
fn style_value(cmd: &str, v: &str) -> Result<String> {
    let v = v.trim();
    if v.is_empty() {
        return Err(bad(cmd, "the plot style name is empty"));
    }
    Ok(if v.eq_ignore_ascii_case("bylayer") {
        "ByLayer".into()
    } else if v.eq_ignore_ascii_case("byblock") {
        "ByBlock".into()
    } else {
        v.chars().take(255).collect()
    })
}

fn run_plotstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "plotstyle";
    let Some(name) = p.get("name") else {
        let d = s.doc()?;
        return Ok(json!({ "current": d.header.str("CPLOTSTYLE", "ByLayer"), "styles": named_styles(d) }));
    };
    let name = style_value(cmd, name.as_str().ok_or_else(|| bad(cmd, "`name` must be a plot style name"))?)?;
    let handles = targets(s, p)?;
    if handles.is_empty() {
        s.doc_mut()?.header.set_str("CPLOTSTYLE", &name);
        return Ok(json!({ "current": name }));
    }
    let mut changed = 0usize;
    for h in &handles {
        if super::super::curves::is_locked(s, *h) {
            continue;
        }
        let d = s.doc_mut()?;
        if d.entity(*h).is_none() {
            return Err(bad(cmd, format!("no object {}", h.hex())));
        }
        d.modify_entity(*h, |e| e.common.plot_style = name.clone())?;
        changed += 1;
    }
    Ok(json!({ "changed": changed, "plotStyle": name }))
}

// ---------- STYLESMANAGER ----------

fn table_summary(d: &Drawing, t: &PlotStyleTable) -> Value {
    let in_drawing = d.plot_style_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&t.name));
    json!({
        "name": t.name,
        "description": t.description,
        "kind": if t.is_color_dependent() { "colorDependent" } else { "named" },
        "styles": t.styles.len(),
        "inDrawing": in_drawing,
        "builtin": PlotStyleTable::builtin(&t.name).is_some() && !in_drawing,
    })
}

fn table_name(cmd: &str, p: &Value) -> Result<String> {
    let n = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` (the table) is required"))?;
    Ok(n.chars().take(255).collect())
}

/// The table `name` as the drawing sees it.
fn find_table(cmd: &str, d: &Drawing, name: &str) -> Result<PlotStyleTable> {
    cadcraft_doc::plot_style_table(d, name).ok_or_else(|| bad(cmd, format!("no plot style table `{name}`")))
}

/// Store `t` in the drawing, replacing a table of the same name.
fn store(d: &mut Drawing, t: PlotStyleTable) {
    match d.plot_style_tables.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&t.name)) {
        Some(x) => *x = t,
        None => d.plot_style_tables.push(t),
    }
}

/// Apply the fields present in `v` to style `st`.
fn edit_style(cmd: &str, st: &mut PlotStyle, v: &Value) -> Result<()> {
    let opt_str = |k: &str| -> Result<Option<Option<String>>> {
        match v.get(k) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(s)) if s.trim().is_empty() || s.eq_ignore_ascii_case("object") => Ok(Some(None)),
            Some(Value::String(s)) => Ok(Some(Some(s.trim().chars().take(255).collect()))),
            Some(_) => Err(bad(cmd, format!("`{k}` must be a string or null"))),
        }
    };
    if let Some(c) = opt_str("color")? {
        st.color = match c {
            None => None,
            Some(h) => {
                Some(cadcraft_color::Rgb::parse_hex(h.trim_start_matches('#')).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or null"))?)
            }
        };
    }
    if let Some(lt) = opt_str("linetype")? {
        st.linetype = lt;
    }
    if let Some(d) = opt_str("description")? {
        st.description = d.unwrap_or_default();
    }
    if let Some(g) = v.get("grayscale") {
        st.grayscale = g.as_bool().ok_or_else(|| bad(cmd, "`grayscale` must be true or false"))?;
    }
    if let Some(sc) = v.get("screening") {
        let n = sc.as_f64().filter(|n| n.is_finite() && (0.0..=100.0).contains(n)).ok_or_else(|| bad(cmd, "`screening` must be 0 to 100"))?;
        st.screening = n.round() as u8;
    }
    match v.get("lineweight") {
        None => {}
        Some(Value::Null) => st.lineweight = None,
        Some(Value::String(s)) if s.eq_ignore_ascii_case("object") => st.lineweight = None,
        Some(w) => {
            let mm = w
                .as_f64()
                .filter(|n| n.is_finite() && (0.0..=2.11).contains(n))
                .ok_or_else(|| bad(cmd, "`lineweight` must be 0 to 2.11 mm or null"))?;
            st.lineweight = Some((mm * 100.0).round() as u16);
        }
    }
    fn field<T: serde::de::DeserializeOwned>(cmd: &str, v: &Value, k: &str, msg: &str) -> Result<Option<T>> {
        v.get(k).map(|x| serde_json::from_value(x.clone()).map_err(|_| bad(cmd, msg))).transpose()
    }
    if let Some(e) = field(cmd, v, "end", "`end` must be object, butt, square, round or diamond")? {
        st.end = e;
    }
    if let Some(j) = field(cmd, v, "join", "`join` must be object, miter, bevel, round or diamond")? {
        st.join = j;
    }
    if let Some(f) = field(
        cmd,
        v,
        "fill",
        "`fill` must be object, solid, checkerboard, crosshatch, diamonds, horizontalBars, slantLeft, slantRight, squareDots or verticalBars",
    )? {
        st.fill = f;
    }
    Ok(())
}

fn run_stylesmanager(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "stylesmanager";
    let action = str_param(p, "action").unwrap_or("list").to_ascii_lowercase();
    match action.as_str() {
        "list" => {
            let d = s.doc()?;
            let tables: Vec<Value> = cadcraft_doc::plot_style_table_names(d)
                .iter()
                .filter_map(|n| cadcraft_doc::plot_style_table(d, n))
                .map(|t| table_summary(d, &t))
                .collect();
            Ok(json!({ "tables": tables }))
        }
        "get" => {
            let d = s.doc()?;
            let t = find_table(cmd, d, &table_name(cmd, p)?)?;
            let mut v = serde_json::to_value(&t).unwrap_or(Value::Null);
            if let (Some(o), Value::Object(sum)) = (v.as_object_mut(), table_summary(d, &t)) {
                o.insert("inDrawing".into(), sum.get("inDrawing").cloned().unwrap_or(Value::Null));
                o.insert("builtin".into(), sum.get("builtin").cloned().unwrap_or(Value::Null));
            }
            Ok(v)
        }
        "new" => {
            let mut name = table_name(cmd, p)?;
            let d = s.doc()?;
            let mut t = match str_param(p, "from") {
                Some(from) => find_table(cmd, d, from)?,
                None => {
                    let named = match str_param(p, "kind").map(str::to_ascii_lowercase).as_deref() {
                        None if name.to_ascii_lowercase().ends_with(".stb") => true,
                        None | Some("colordependent" | "ctb") => false,
                        Some("named" | "stb") => true,
                        Some(_) => return Err(bad(cmd, "`kind` must be colorDependent or named")),
                    };
                    if named { PlotStyleTable::named("", "", Vec::new()) } else { PlotStyleTable::color_dependent("", "", |_| None) }
                }
            };
            let ext = if t.is_color_dependent() { ".ctb" } else { ".stb" };
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(ext) {
                if lower.ends_with(".ctb") || lower.ends_with(".stb") {
                    return Err(bad(cmd, format!("a {} table's name ends in {ext}", if ext == ".ctb" { "colour-dependent" } else { "named" })));
                }
                name.push_str(ext);
            }
            if d.plot_style_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
                return Err(bad(cmd, format!("plot style table `{name}` already exists")));
            }
            t.name = name;
            if let Some(desc) = str_param(p, "description") {
                t.description = desc.chars().take(255).collect();
            }
            let t = t.sanitized();
            let out = table_summary(d, &t);
            store(s.doc_mut()?, t);
            Ok(out)
        }
        "set" => {
            let name = table_name(cmd, p)?;
            let mut t = find_table(cmd, s.doc()?, &name)?;
            if let Some(desc) = str_param(p, "description") {
                t.description = desc.chars().take(255).collect();
            }
            if let Some(rm) = str_param(p, "removeStyle") {
                if t.is_color_dependent() {
                    return Err(bad(cmd, "a colour-dependent table always has 255 styles"));
                }
                if rm.trim().eq_ignore_ascii_case(cadcraft_doc::NORMAL_STYLE) {
                    return Err(bad(cmd, "the Normal style can't be removed"));
                }
                let before = t.styles.len();
                t.styles.retain(|x| !x.name.eq_ignore_ascii_case(rm.trim()));
                if t.styles.len() == before {
                    return Err(bad(cmd, format!("`{name}` has no style `{rm}`")));
                }
            }
            if let Some(v) = p.get("style") {
                let sname = v.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
                let sname = sname.ok_or_else(|| bad(cmd, "`style.name` is required"))?;
                let idx = match t.styles.iter().position(|x| x.name.eq_ignore_ascii_case(sname)) {
                    Some(i) => i,
                    None if t.kind == PlotStyleKind::Named => {
                        if t.styles.len() >= cadcraft_doc::MAX_NAMED_STYLES {
                            return Err(bad(cmd, "the table has too many styles"));
                        }
                        t.styles.push(PlotStyle::object(&sname.chars().take(255).collect::<String>()));
                        t.styles.len() - 1
                    }
                    None => return Err(bad(cmd, format!("`{name}` has no style `{sname}` (colour-dependent styles are Color_1 to Color_255)"))),
                };
                if let Some(st) = t.styles.get_mut(idx) {
                    edit_style(cmd, st, v)?;
                }
            }
            let t = t.sanitized();
            let out = serde_json::to_value(&t).unwrap_or(Value::Null);
            store(s.doc_mut()?, t);
            Ok(out)
        }
        "delete" => {
            let name = table_name(cmd, p)?;
            let d = s.doc_mut()?;
            let before = d.plot_style_tables.len();
            d.plot_style_tables.retain(|t| !t.name.eq_ignore_ascii_case(&name));
            if d.plot_style_tables.len() == before {
                return Err(bad(cmd, format!("the drawing keeps no plot style table `{name}` (built-in tables can't be deleted)")));
            }
            Ok(json!({ "deleted": name }))
        }
        "load" => {
            let text = match (str_param(p, "text"), str_param(p, "path")) {
                (Some(t), _) => t.to_string(),
                (None, Some(_path)) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let meta = std::fs::metadata(_path).map_err(|e| bad(cmd, format!("{_path}: {e}")))?;
                        if meta.len() > cadcraft_doc::MAX_TABLE_FILE as u64 {
                            return Err(bad(cmd, format!("{_path}: the plot style table file is too large")));
                        }
                        std::fs::read_to_string(_path).map_err(|e| bad(cmd, format!("{_path}: {e}")))?
                    }
                    #[cfg(target_arch = "wasm32")]
                    return Err(bad(cmd, "files can't be read here: pass `text`"));
                }
                (None, None) => return Err(bad(cmd, "`path` or `text` is required")),
            };
            let mut t = PlotStyleTable::from_file_text(&text).map_err(|e| bad(cmd, e))?;
            if let Some(n) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
                t.name = n.chars().take(255).collect();
            }
            if t.name.is_empty() {
                return Err(bad(cmd, "the table has no name: pass `name`"));
            }
            let out = table_summary(s.doc()?, &t);
            store(s.doc_mut()?, t);
            Ok(out)
        }
        "save" => {
            let name = table_name(cmd, p)?;
            let text = find_table(cmd, s.doc()?, &name)?.to_file_text();
            match str_param(p, "path") {
                None => Ok(json!({ "name": name, "text": text })),
                Some(_path) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let tmp = format!("{_path}.cadcraft-tmp");
                        std::fs::write(&tmp, text.as_bytes()).map_err(|e| bad(cmd, format!("{_path}: {e}")))?;
                        std::fs::rename(&tmp, _path).map_err(|e| bad(cmd, format!("{_path}: {e}")))?;
                        Ok(json!({ "name": name, "path": _path, "bytes": text.len() }))
                    }
                    #[cfg(target_arch = "wasm32")]
                    Err(bad(cmd, "files can't be written here: leave out `path` to get `text`"))
                }
            }
        }
        other => Err(bad(cmd, format!("unknown action `{other}` (list, get, new, set, delete, load, save)"))),
    }
}
