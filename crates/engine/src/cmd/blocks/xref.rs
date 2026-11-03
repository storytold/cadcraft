//! External references: XATTACH, -XREF (?/Bind/Detach/Path/Unload/Reload/Overlay/Attach),
//! XBIND and XOPEN.
//!
//! An xref is a block definition with a path ([`Block::xref_path`]): its objects are the model
//! space of the referenced drawing, loaded when the host drawing opens and on Reload, never saved
//! with the host. Symbols that come with it are named `xref|name` ([`load`]). Bind turns it into
//! a normal block of the host drawing ([`bind`]).

mod bind;
pub(crate) mod load;

use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, EntityKind, EntityStore, XrefStatus};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use self::load::{Outcome, Sym};
use super::{InsStage, InsertM, insert_with, valid_block_name};
use crate::cmd::{CommandSpec, bad, bool_or, f64_or, point_param, point_value, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xattach", "DWG Reference...", run_xattach)
            .menu(&["Insert", "DWG Reference..."])
            .alias(&["xa"])
            .params("{path: DWG/DXF file (saved as given; a relative path is relative to this drawing's folder), name? (default: the file name), overlay?: bool, at?: [x,y] (default 0,0), scale?: number | [x, y], rotation? (degrees)} → {name, handle, file}")
            .interactive(|_| Ok(Box::new(XrefM::new("XATTACH", XStage::File { overlay: false })))),
        CommandSpec::new("xref", "Reference Manager", run_xref)
            .menu(&["Window", "Reference Manager"])
            .alias(&["xr", "externalreferences"])
            .params("{} → {xrefs: [{name, path, foundAt, type: Attach|Overlay, status: Loaded|Unloaded|Not Found|Unresolved|Unreferenced|Not loaded, references, objects}]} (the app opens the External References palette instead)")
            .noundo(),
        CommandSpec::new("-xref", "External References", run_dash_xref)
            .alias(&["-xr"])
            .params("{option: \"?\"|bind|detach|path|unload|reload|overlay|attach|type, names?: \"A,B*\" (default *), path? (path, attach, overlay), insert?: bool (bind: Insert type), overlay?: bool (type), at?, scale?, rotation? (attach)}")
            .interactive(|_| Ok(Box::new(XrefM::new("-XREF", XStage::Option)))),
        CommandSpec::new("xbind", "Bind...", run_xbind)
            .menu(&["Modify", "Object", "External Reference", "Bind..."])
            .alias(&["-xbind", "xb", "-xb"])
            .params("{type: block|dimstyle|layer|linetype|style, names: \"XREF|NAME,…\" (wildcards) | [names]} → {bound: [names]}")
            .interactive(|_| Ok(Box::new(XbindM::default()))),
        CommandSpec::new("xopen", "Open Xref", run_xopen)
            .menu(&["Tools", "Open Xref"])
            .params("{name} | {handles: [xref reference]}")
            .noundo()
            .interactive(|_| Ok(Box::new(super::super::machines::SelectRun::single("xopen", "XOPEN", "Select xref")))),
    ]
}

/// Load the xrefs of drawing `i`, just opened (OPEN): the opened state counts as saved, and the
/// view frames the drawing with them.
pub(crate) fn after_open(s: &mut Session, i: usize) {
    let px = s.viewport_px;
    let Some(st) = s.docs.get_mut(i) else { return };
    if !st.doc.blocks.values().any(|b| b.xref_path.is_some()) {
        return;
    }
    let path = st.path.clone();
    let out = load::load_all(Arc::make_mut(&mut st.doc), path.as_deref());
    st.saved = st.doc.clone();
    let ext = st.doc.extents(&cadcraft_doc::Space::Model);
    if !ext.is_empty() {
        st.set_view_quiet(crate::View { center: ext.center(), height: (ext.height().max(ext.width() * px.1 / px.0.max(1.0)) * 1.1).max(1e-6) });
    }
    echo_all(s, &out);
}

/// The open drawing's path (xref paths are relative to its folder).
fn host(s: &Session) -> Option<String> {
    s.state().ok().and_then(|st| st.path.clone())
}

/// The file name without folders and extension (either separator, so Windows paths work anywhere).
fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

/// The xrefs attached to `d` (not the nested ones that came with them).
fn xrefs(d: &Drawing) -> impl Iterator<Item = &Arc<Block>> {
    d.blocks.values().filter(|b| b.xref_path.is_some() && !b.name.contains('|'))
}

/// The xrefs whose names match `pattern` (comma-separated wildcards).
fn matching(d: &Drawing, pattern: &str) -> Vec<String> {
    let pats: Vec<&str> = pattern.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    xrefs(d).filter(|b| pats.iter().any(|p| crate::cmd::qselect::wildcard(p, &b.name))).map(|b| b.name.clone()).collect()
}

/// References to block `key` in model space, the layouts and the host's own blocks.
fn reference_count(d: &Drawing, key: &str) -> usize {
    let refs = |st: &EntityStore| st.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(key))).count();
    refs(&d.model)
        + d.layouts.iter().map(|l| refs(&l.entities)).sum::<usize>()
        + d.blocks.values().filter(|b| !b.name.contains('|')).map(|b| refs(&b.entities)).sum::<usize>()
}

/// One xref as `?` and the palette list it.
fn describe(d: &Drawing, b: &Block, host: Option<&str>) -> Value {
    let refs = reference_count(d, &b.name);
    let status = if refs == 0 && b.xref_status == XrefStatus::Loaded { "Unreferenced" } else { b.xref_status.label() };
    let found =
        b.xref_path.as_deref().and_then(|p| load::find(p, host.map(std::path::Path::new), false).ok()).map(|f| f.to_string_lossy().into_owned());
    json!({
        "name": b.name,
        "path": b.xref_path,
        "foundAt": found,
        "type": if b.xref_overlay { "Overlay" } else { "Attach" },
        "status": status,
        "references": refs,
        "objects": b.entities.len(),
    })
}

fn echo_all(s: &mut Session, out: &[Outcome]) {
    for o in out {
        s.echo(o.message());
    }
}

/// Attach the drawing at `path` as xref `name` (defined and loaded; nothing changes when it can't
/// be loaded). An xref of that name attached from the same file is used again. Returns its name.
fn attach(s: &mut Session, name: &str, path: &str, overlay: bool) -> Result<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(bad("xattach", "`path` is required"));
    }
    if !valid_block_name(name) {
        return Err(bad("xattach", format!("\"{name}\" is not a valid xref name")));
    }
    if let Some(b) = s.doc()?.block(name) {
        return match &b.xref_path {
            Some(p) if p.eq_ignore_ascii_case(path) => Ok(b.name.clone()),
            Some(_) => Err(bad("xattach", format!("an xref named \"{}\" is already attached from another file", b.name))),
            None => Err(bad("xattach", format!("a block named \"{}\" already exists", b.name))),
        };
    }
    let host = host(s);
    let mut d = s.doc()?.clone();
    let mut b = Block::new(name);
    b.xref_path = Some(path.to_string());
    b.xref_overlay = overlay;
    d.blocks.insert(name.to_string(), Arc::new(b));
    let out = load::load(&mut d, name, host.as_deref(), true);
    match out.first() {
        Some(o) if o.status == XrefStatus::Loaded => {}
        Some(o) => return Err(bad("xattach", format!("{path} can't be attached: {}", o.detail))),
        None => return Err(bad("xattach", format!("{path} can't be attached"))),
    }
    *s.doc_mut()? = d;
    echo_all(s, &out);
    Ok(name.to_string())
}

/// The X/Y scale factors of a JSON `scale` (a number or [x, y]).
fn scale_param(cmd: &str, p: &Value) -> Result<Vec2> {
    let scale = match p.get("scale") {
        Some(v) if v.is_array() => point_value(v).ok_or_else(|| bad(cmd, "`scale` must be a number or [x, y]"))?,
        _ => {
            let f = f64_or(p, "scale", 1.0);
            Vec2::new(f, f)
        }
    };
    if scale.x == 0.0 || scale.y == 0.0 || !scale.x.is_finite() || !scale.y.is_finite() {
        return Err(bad(cmd, "scale must be a non-zero number"));
    }
    Ok(scale)
}

fn run_xattach(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("xattach", "`path` is required"))?.to_string();
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| stem(&path));
    let overlay = bool_or(p, "overlay", false) || str_param(p, "type").is_some_and(|t| t.eq_ignore_ascii_case("overlay"));
    let at = point_param(p, "at").unwrap_or(Vec2::ZERO);
    let scale = scale_param("xattach", p)?;
    let rotation = f64_or(p, "rotation", 0.0).to_radians();
    if !rotation.is_finite() {
        return Err(bad("xattach", "`rotation` must be a number"));
    }
    let key = attach(s, &name, &path, overlay)?;
    let h = insert_with(s, &key, at, scale, rotation, None, &serde_json::Map::new())?;
    let host = host(s);
    let d = s.doc()?;
    let info = d.block(&key).map(|b| describe(d, b, host.as_deref()));
    Ok(json!({ "name": key, "handle": h.hex(), "xref": info }))
}

/// The xrefs a JSON call names (`names`, default all); an error when none match.
fn named(s: &Session, cmd: &str, p: &Value) -> Result<Vec<String>> {
    let pattern = match p.get("names") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","),
        Some(Value::String(t)) => t.clone(),
        _ => str_param(p, "name").unwrap_or("*").to_string(),
    };
    let names = matching(s.doc()?, &pattern);
    if names.is_empty() {
        return Err(bad(cmd, format!("no xref matches \"{pattern}\"")));
    }
    Ok(names)
}

/// XREF's JSON form: the xrefs as the External References palette lists them (the palette is
/// the app's; typed without a UI, the list is echoed).
fn run_xref(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = run_dash_xref(s, &json!({ "option": "?" }))?;
    if r["xrefs"].as_array().is_some_and(|a| a.is_empty()) {
        s.echo("No xrefs are attached.");
    }
    Ok(r)
}

fn run_dash_xref(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").unwrap_or("?").trim().to_ascii_lowercase();
    let host = host(s);
    match option.as_str() {
        "?" | "list" => {
            let pattern = str_param(p, "names").unwrap_or("*");
            let d = s.doc()?;
            let names = matching(d, pattern);
            let list: Vec<Value> = names.iter().filter_map(|n| d.block(n)).map(|b| describe(d, b, host.as_deref())).collect();
            Ok(json!({ "xrefs": list }))
        }
        "reload" | "r" => {
            let names = named(s, "-xref", p)?;
            let out = load::load_each(s.doc_mut()?, &names, host.as_deref(), false);
            echo_all(s, &out);
            Ok(
                json!({ "xrefs": out.iter().filter(|o| !o.name.is_empty()).map(|o| json!({"name": o.name, "status": o.status.label(), "detail": o.detail})).collect::<Vec<_>>() }),
            )
        }
        "unload" | "u" => {
            let names = named(s, "-xref", p)?;
            for n in &names {
                load::unload(s.doc_mut()?, n);
                s.echo(format!("Xref \"{n}\" unloaded."));
            }
            Ok(json!({ "unloaded": names }))
        }
        "detach" | "d" => {
            let names = named(s, "-xref", p)?;
            let mut erased = 0;
            for n in &names {
                erased += load::detach(s.doc_mut()?, n);
                s.echo(format!("Xref \"{n}\" detached."));
            }
            s.set_selection(Vec::new());
            Ok(json!({ "detached": names, "erased": erased }))
        }
        "bind" | "b" => {
            let names = named(s, "-xref", p)?;
            let insert = bool_or(p, "insert", false);
            let mut d = s.doc()?.clone();
            for n in &names {
                bind::bind(&mut d, n, insert).map_err(|e| bad("-xref", e))?;
            }
            *s.doc_mut()? = d;
            for n in &names {
                s.echo(format!("Xref \"{n}\" bound."));
            }
            Ok(json!({ "bound": names }))
        }
        "path" | "p" => {
            let path = str_param(p, "path").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("-xref", "`path` is required"))?.to_string();
            let names = named(s, "-xref", p)?;
            let d = s.doc_mut()?;
            for n in &names {
                if let Some(b) = d.blocks.get_mut(n) {
                    Arc::make_mut(b).xref_path = Some(path.clone());
                }
            }
            let out = load::load_each(d, &names, host.as_deref(), true);
            echo_all(s, &out);
            Ok(json!({ "xrefs": names, "path": path }))
        }
        "type" => {
            let overlay = bool_or(p, "overlay", false);
            let names = named(s, "-xref", p)?;
            for n in &names {
                if let Some(b) = s.doc_mut()?.blocks.get_mut(n) {
                    Arc::make_mut(b).xref_overlay = overlay;
                }
            }
            Ok(json!({ "xrefs": names, "type": if overlay { "Overlay" } else { "Attach" } }))
        }
        "attach" | "a" | "overlay" | "o" => {
            let mut q = p.clone();
            if let Some(o) = q.as_object_mut() {
                o.insert("overlay".into(), json!(option.starts_with('o')));
            }
            run_xattach(s, &q)
        }
        _ => Err(bad("-xref", format!("unknown option \"{option}\""))),
    }
}

fn sym_type(t: &str) -> Option<Sym> {
    Some(match t.trim().to_ascii_lowercase().as_str() {
        "block" | "b" => Sym::Block,
        "dimstyle" | "d" => Sym::DimStyle,
        "layer" | "la" => Sym::Layer,
        "linetype" | "ltype" | "lt" => Sym::Linetype,
        "style" | "s" => Sym::Style,
        _ => return None,
    })
}

/// The dependent symbols of table `sym` whose names match `pattern`.
fn dependents(d: &Drawing, sym: Sym, pattern: &str) -> Vec<String> {
    let all: Vec<&String> = match sym {
        Sym::Block => d.blocks.keys().collect(),
        Sym::DimStyle => d.dim_styles.iter().map(|t| &t.name).collect(),
        Sym::Layer => d.layers.iter().map(|t| &t.name).collect(),
        Sym::Linetype => d.linetypes.iter().map(|t| &t.name).collect(),
        Sym::Style => d.text_styles.iter().map(|t| &t.name).collect(),
        _ => Vec::new(),
    };
    let pats: Vec<&str> = pattern.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    all.into_iter().filter(|n| n.contains('|') && pats.iter().any(|p| crate::cmd::qselect::wildcard(p, n))).cloned().collect()
}

fn run_xbind(s: &mut Session, p: &Value) -> Result<Value> {
    let sym = str_param(p, "type").and_then(sym_type).ok_or_else(|| bad("xbind", "`type` must be block, dimstyle, layer, linetype or style"))?;
    let pattern = match p.get("names") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","),
        Some(Value::String(t)) => t.clone(),
        _ => return Err(bad("xbind", "`names` is required")),
    };
    let names = dependents(s.doc()?, sym, &pattern);
    if names.is_empty() {
        return Err(bad("xbind", format!("no xref-dependent symbol matches \"{pattern}\"")));
    }
    let mut d = s.doc()?.clone();
    let mut bound = Vec::new();
    for n in &names {
        bound.push(bind::xbind(&mut d, sym, n).map_err(|e| bad("xbind", e))?);
    }
    *s.doc_mut()? = d;
    s.echo(format!("{} symbol(s) bound.", bound.len()));
    Ok(json!({ "bound": bound }))
}

fn run_xopen(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => {
            let hs = crate::cmd::targets(s, p)?;
            hs.iter()
                .filter_map(|h| d.entity(*h))
                .find_map(|e| match &e.kind {
                    EntityKind::Insert(i) if d.block(&i.block).is_some_and(|b| b.xref_path.is_some()) => Some(i.block.clone()),
                    _ => None,
                })
                .ok_or_else(|| bad("xopen", "the selected object is not an xref"))?
        }
    };
    let b = d.block(&name).ok_or_else(|| bad("xopen", format!("no xref \"{name}\"")))?;
    let path = b.xref_path.clone().ok_or_else(|| bad("xopen", format!("\"{name}\" is not an xref")))?;
    let host = host(s);
    let file = load::find(&path, host.as_deref().map(std::path::Path::new), false).map_err(|m| match m {
        load::Missing::NotFound => bad("xopen", format!("{path}: file not found")),
        load::Missing::Refused(r) => bad("xopen", format!("{path}: not opened, {r}")),
    })?;
    s.execute("open", &json!({ "path": file.to_string_lossy() }))
}

// ---------------- interactive ----------------

/// Where -XREF / XATTACH are in their prompt sequence.
#[derive(Default)]
enum XStage {
    #[default]
    Option,
    /// Names for Bind/Detach/Unload/Reload/? (the option's JSON name).
    Names(&'static str),
    PathNames,
    /// New paths for these xrefs, one after the other.
    NewPath(Vec<String>),
    File {
        overlay: bool,
    },
    /// Placing the attached xref (INSERT's prompts from the insertion point on).
    Insert(Box<InsertM>),
}

struct XrefM {
    name: &'static str,
    stage: XStage,
}

impl XrefM {
    fn new(name: &'static str, stage: XStage) -> Self {
        XrefM { name, stage }
    }
}

/// Typed text, or `None` for Enter.
fn text(i: &Input) -> Option<String> {
    match i {
        Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

/// Run -XREF's JSON form for an option chosen at the command line; failures are reported.
fn run_option(s: &mut Session, p: Value) -> Result<Option<Value>> {
    match run_dash_xref(s, &p) {
        Ok(v) => Ok(Some(v)),
        Err(EngineError::BadParams { msg, .. }) => {
            s.echo(msg);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

impl Interactive for XrefM {
    fn name(&self) -> &'static str {
        self.name
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            XStage::Option => Prompt::new("Enter an option", crate::cmd::curves::KW)
                .kw(&["?", "Bind", "Detach", "Path", "Unload", "Reload", "Overlay", "Attach"])
                .default("Attach"),
            XStage::Names("?") => Prompt::new("Enter xref(s) to list", Accept::TEXT).default("*"),
            XStage::Names(op) => Prompt::new(format!("Enter xref name(s) to {op}"), Accept::TEXT),
            XStage::PathNames => Prompt::new("Edit path for which xref(s)", Accept::TEXT),
            XStage::NewPath(names) => {
                let old = names.first().and_then(|n| s.doc().ok()?.block(n)?.xref_path.clone()).unwrap_or_default();
                Prompt::new(format!("Enter new path for \"{}\"", names.first().cloned().unwrap_or_default()), Accept::TEXT).default(old)
            }
            XStage::File { overlay } => Prompt::new(format!("Enter name of file to {}", if *overlay { "overlay" } else { "attach" }), Accept::TEXT),
            XStage::Insert(m) => m.prompt(s),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match &mut self.stage {
            XStage::Option => {
                let k = match &i {
                    Input::Enter => "Attach".to_string(),
                    Input::Keyword(k) => k.clone(),
                    Input::Text(t) => match self.prompt(s).match_keyword(t) {
                        Some(k) => k,
                        None => {
                            s.echo("Invalid option keyword.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                };
                self.stage = match k.as_str() {
                    "?" => XStage::Names("?"),
                    "Bind" => XStage::Names("bind"),
                    "Detach" => XStage::Names("detach"),
                    "Unload" => XStage::Names("unload"),
                    "Reload" => XStage::Names("reload"),
                    "Path" => XStage::PathNames,
                    "Overlay" => XStage::File { overlay: true },
                    _ => XStage::File { overlay: false },
                };
                Ok(Step::Continue)
            }
            XStage::Names(op) => {
                let op = *op;
                let names = match text(&i) {
                    Some(t) => t,
                    None if op == "?" => "*".into(),
                    None => return Ok(Step::Done),
                };
                let r = run_option(s, json!({ "option": op, "names": names }))?;
                if op == "?" {
                    list(s, r.as_ref());
                }
                Ok(Step::Done)
            }
            XStage::PathNames => {
                let Some(t) = text(&i) else { return Ok(Step::Done) };
                let names = matching(s.doc()?, &t);
                if names.is_empty() {
                    s.echo(format!("No xref matches \"{t}\"."));
                    return Ok(Step::Done);
                }
                self.stage = XStage::NewPath(names);
                Ok(Step::Continue)
            }
            XStage::NewPath(names) => {
                let n = if names.is_empty() { String::new() } else { names.remove(0) };
                if let Some(t) = text(&i) {
                    run_option(s, json!({ "option": "path", "names": n, "path": t }))?;
                }
                Ok(if names.is_empty() { Step::Done } else { Step::Continue })
            }
            XStage::File { overlay } => {
                let overlay = *overlay;
                let Some(path) = text(&i) else { return Ok(Step::Continue) };
                let key = match attach(s, &stem(&path), &path, overlay) {
                    Ok(k) => k,
                    Err(e) => {
                        s.echo(match e {
                            EngineError::BadParams { msg, .. } => msg,
                            other => other.to_string(),
                        });
                        return Ok(Step::Continue);
                    }
                };
                self.stage = XStage::Insert(Box::new(InsertM { name: Some(key), stage: InsStage::Point, ..InsertM::default() }));
                Ok(Step::Continue)
            }
            XStage::Insert(m) => m.input(s, i),
        }
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        match &self.stage {
            XStage::Insert(m) => m.preview(s, cursor),
            _ => Vec::new(),
        }
    }
}

/// -XREF `?`: the xrefs, one per line.
fn list(s: &mut Session, r: Option<&Value>) {
    let rows: Vec<Value> = r.and_then(|v| v.get("xrefs")).and_then(Value::as_array).cloned().unwrap_or_default();
    s.echo(format!("{:<24} {:<8} {:<12} Path", "Xref name", "Type", "Status"));
    for x in &rows {
        let f = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        s.echo(format!("{:<24} {:<8} {:<12} {}", f("name"), f("type"), f("status"), f("path")));
    }
    s.echo(format!("Total xref(s): {}", rows.len()));
}

/// XBIND at the command line: the symbol type, then the dependent names.
#[derive(Default)]
struct XbindM {
    sym: Option<&'static str>,
}

impl Interactive for XbindM {
    fn name(&self) -> &'static str {
        "XBIND"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.sym {
            None => Prompt::new("Enter symbol type to bind", crate::cmd::curves::KW).kw(&["Block", "Dimstyle", "LAyer", "LType", "Style"]),
            Some(t) => Prompt::new(format!("Enter dependent {t} name(s)"), Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sym {
            None => {
                let k = match &i {
                    Input::Keyword(k) => Some(k.clone()),
                    Input::Text(t) => self.prompt(s).match_keyword(t),
                    Input::Enter | Input::Cancel => return Ok(Step::Cancel),
                    _ => None,
                };
                self.sym = match k.as_deref() {
                    Some("Block") => Some("block"),
                    Some("Dimstyle") => Some("dimstyle"),
                    Some("LAyer") => Some("layer"),
                    Some("LType") => Some("linetype"),
                    Some("Style") => Some("style"),
                    _ => {
                        s.echo("Invalid option keyword.");
                        None
                    }
                };
                Ok(Step::Continue)
            }
            Some(t) => {
                let Some(names) = text(&i) else { return Ok(Step::Done) };
                match run_xbind(s, &json!({ "type": t, "names": names })) {
                    Ok(_) => {}
                    Err(EngineError::BadParams { msg, .. }) => s.echo(msg),
                    Err(e) => return Err(e),
                }
                Ok(Step::Done)
            }
        }
    }
}
