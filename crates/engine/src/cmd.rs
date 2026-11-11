//! The command registry. Ids are AutoCAD command names in lower case; menu paths follow
//! AutoCAD's menu bar so the catalog doubles as the parity metric.

pub(crate) mod annoscale;
mod annotate;
mod array;
mod audit;
pub(crate) mod blocks;
pub mod clipboard;
pub mod constraints;
mod draw;
mod draw2;
mod draworder;
mod edit;
mod exchange;
pub mod file;
mod gripcmds;
mod griphot;
pub(crate) mod groups;
pub(crate) mod hatch;
mod image;
mod inquiry;
mod layer;
mod layout;
mod lisp;
mod merge;
mod mleader_content;
mod mleader_edit;
mod modify;
pub(crate) use modify::Erased;
mod modify2;
mod mtext_opts;
mod overkill;
mod pline_opts;
pub mod plotters;
mod props;
pub use draw2::breakline::BreaklineOptions;
pub use props::MatchProps;
pub(crate) use props::UNIT_RANGES;
pub(crate) use props::{current_color_value, set_current_color_value};
mod qselect;
mod selecttools;
mod settings;
mod spline_opts;
mod table;
mod tolerance;
mod trimextend;
mod txt2mtxt;
pub use txt2mtxt::Txt2MtxtOptions;
mod utility;
mod view;

pub mod curves;
pub mod helpers;
pub mod machines;
pub mod options;

use cadcraft_doc::Handle;
use cadcraft_geom::Vec2;
use serde::Serialize;
use serde_json::Value;

use crate::{EngineError, Interactive, Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;
pub type Factory = fn(&Session) -> Result<Box<dyn Interactive>>;

#[derive(Clone)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Draw", "Circle", "Center, Radius"]`. Empty = not in menus.
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    pub undoable: bool,
    pub interactive: Option<Factory>,
    /// Can run inside another command with a leading apostrophe.
    pub transparent: bool,
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, run: Run) -> Self {
        CommandSpec {
            id,
            label,
            menu: &[],
            shortcut: None,
            aliases: &[],
            params: "",
            enabled: has_doc,
            run,
            undoable: true,
            interactive: None,
            transparent: false,
        }
    }
    pub const fn menu(mut self, m: &'static [&'static str]) -> Self {
        self.menu = m;
        self
    }
    pub const fn key(mut self, k: &'static str) -> Self {
        self.shortcut = Some(k);
        self
    }
    pub const fn alias(mut self, a: &'static [&'static str]) -> Self {
        self.aliases = a;
        self
    }
    pub const fn params(mut self, p: &'static str) -> Self {
        self.params = p;
        self
    }
    pub const fn enabled(mut self, e: Enabled) -> Self {
        self.enabled = e;
        self
    }
    pub const fn noundo(mut self) -> Self {
        self.undoable = false;
        self
    }
    pub const fn interactive(mut self, f: Factory) -> Self {
        self.interactive = Some(f);
        self
    }
    pub const fn transparent(mut self) -> Self {
        self.transparent = true;
        self
    }
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            aliases: self.aliases.to_vec(),
            params: self.params,
            interactive: self.interactive.is_some(),
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub aliases: Vec<&'static str>,
    pub params: &'static str,
    pub interactive: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
pub fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.state().map(|_| ()).map_err(|_| "no drawing open".into())
}
pub fn has_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.selection().is_empty() { Err("nothing selected".into()) } else { Ok(()) }
}

// ---------- extension registries ----------
//
// Another crate (an add-on built into an app) contributes commands, MCP tools and MCP
// instructions here at startup, before or after the catalog was first read. Its commands then go
// through the same paths as built-in ones: the command line, `engine.execute`, the control
// channel, scripts, the CLI and MCP.

/// Add commands to the catalog. An id the catalog already has (built in, or registered before)
/// is refused and returned, so an add-on can never replace a built-in command. The catalog is
/// rebuilt the next time it is read, so registering after commands have already run works too;
/// the previous snapshot stays valid for anyone still holding it.
pub fn register_specs(specs: Vec<CommandSpec>) -> Vec<&'static str> {
    let mut refused = Vec::new();
    let taken: Vec<&'static str> = command_specs().iter().map(|c| c.id).collect();
    let mut added = false;
    let mut e = extra_specs().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for spec in specs {
        if taken.contains(&spec.id) || e.iter().any(|c| c.id == spec.id) {
            refused.push(spec.id);
        } else {
            e.push(spec);
            added = true;
        }
    }
    drop(e);
    // Only a registration that added something rebuilds the catalog: each rebuild keeps a
    // snapshot for the life of the process.
    if added {
        CATALOG_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    refused
}

/// An MCP tool an add-on offers next to the built-in ones: calling it runs `command` with the
/// tool's arguments as JSON params, `fixed` merged on top (e.g. `{"mode": "begin"}`). Lets an
/// add-on give agents task-shaped tools without the MCP crate knowing about it.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema of the arguments (the subset the MCP server checks: type, required, properties).
    pub input_schema: Value,
    pub command: &'static str,
    /// Params set on every call, over the caller's arguments: a JSON object, or `Value::Null`
    /// for none (any other value is ignored).
    pub fixed: Value,
}

/// Add MCP tools (see [`ToolSpec`]); a name already registered is replaced. Built-in tool names
/// always win over registered ones.
pub fn register_tools(tools: Vec<ToolSpec>) {
    let mut t = extra_tools_store().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for tool in tools {
        t.retain(|x| x.name != tool.name);
        t.push(tool);
    }
}

/// The MCP tools add-ons registered, in registration order.
pub fn extra_tools() -> Vec<ToolSpec> {
    extra_tools_store().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// Add a paragraph to the instructions the MCP server gives an agent when it connects (how to
/// use an add-on's commands). Registering the same text twice keeps one copy.
pub fn register_mcp_instructions(text: &'static str) {
    let mut t = mcp_instructions_store().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !t.contains(&text) {
        t.push(text);
    }
}

/// The paragraphs add-ons registered with [`register_mcp_instructions`], in order.
pub fn mcp_instructions() -> Vec<&'static str> {
    mcp_instructions_store().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

fn mcp_instructions_store() -> &'static std::sync::Mutex<Vec<&'static str>> {
    static TEXT: std::sync::OnceLock<std::sync::Mutex<Vec<&'static str>>> = std::sync::OnceLock::new();
    TEXT.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn extra_tools_store() -> &'static std::sync::Mutex<Vec<ToolSpec>> {
    static TOOLS: std::sync::OnceLock<std::sync::Mutex<Vec<ToolSpec>>> = std::sync::OnceLock::new();
    TOOLS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn extra_specs() -> &'static std::sync::Mutex<Vec<CommandSpec>> {
    static EXTRA: std::sync::OnceLock<std::sync::Mutex<Vec<CommandSpec>>> = std::sync::OnceLock::new();
    EXTRA.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

static CATALOG_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn catalog_cache() -> &'static std::sync::Mutex<Option<(u64, &'static [CommandSpec])>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(u64, &'static [CommandSpec])>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn command_specs() -> &'static [CommandSpec] {
    let generation = CATALOG_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    // A poisoned lock is recovered: the cache is only assigned once a catalog is fully built.
    let mut cache = catalog_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((g, specs)) = *cache
        && g == generation
    {
        return specs;
    }
    // Rebuilt once per registration (registrations happen at startup): the snapshot is kept for
    // the life of the process, as the catalog always was, and an older one stays valid for
    // callers that still hold it.
    let specs: &'static [CommandSpec] = Box::leak(build_catalog().into_boxed_slice());
    *cache = Some((generation, specs));
    specs
}

fn build_catalog() -> Vec<CommandSpec> {
    {
        let mut v = Vec::new();
        v.extend(file::specs());
        v.extend(file::alias_specs());
        v.extend(audit::specs());
        v.extend(edit::specs());
        v.extend(qselect::specs());
        v.extend(selecttools::specs());
        v.extend(view::specs());
        v.extend(draw::specs());
        v.extend(draw2::specs());
        v.extend(annotate::specs());
        v.extend(mleader_edit::specs());
        v.extend(annoscale::specs());
        v.extend(hatch::specs());
        v.extend(overkill::specs());
        v.extend(blocks::specs());
        v.extend(modify::specs());
        v.extend(draworder::specs());
        v.extend(array::specs());
        v.extend(modify2::specs());
        v.extend(groups::specs());
        v.extend(gripcmds::specs());
        v.extend(file::compare::specs());
        v.extend(layer::specs());
        v.extend(layout::specs());
        v.extend(exchange::specs());
        v.extend(image::specs());
        v.extend(props::specs());
        v.extend(inquiry::specs());
        v.extend(settings::specs());
        v.extend(options::specs());
        v.extend(utility::specs());
        v.extend(table::specs());
        v.extend(tolerance::specs());
        v.extend(txt2mtxt::specs());
        v.extend(constraints::specs());
        v.extend(lisp::specs());
        let extra: Vec<CommandSpec> = extra_specs().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        v.extend(extra.into_iter().filter(|spec| !v.iter().any(|c| c.id == spec.id)).collect::<Vec<_>>());
        v
    }
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    let l = id.to_ascii_lowercase();
    command_specs().iter().find(|c| c.id == l)
}

/// Resolve a typed name or alias (AutoCAD-style short names such as `L`, `C`, `PL`) to an id.
/// Command names win; the user's aliases (ALIASEDIT, REINIT) come before the built-in ones.
pub fn resolve_alias(name: &str) -> String {
    let l = name.to_ascii_lowercase();
    if find_command(&l).is_some() {
        return l;
    }
    if let Some(id) = file::user_alias(&l) {
        return id;
    }
    command_specs().iter().find(|c| c.aliases.iter().any(|a| a.eq_ignore_ascii_case(&l))).map(|c| c.id.to_string()).unwrap_or(l)
}

// ---------- param helpers ----------

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}
pub(crate) fn f64_or(p: &Value, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default)
}
pub(crate) fn f64_req(cmd: &str, p: &Value, key: &str) -> Result<f64> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, format!("`{key}` (number) is required")))
}
/// An optional size: `None` when absent or null, else a finite number above zero (or zero too
/// with `zero_ok`). Anything else is refused, not clamped or made absolute, so a bad value never
/// makes a degenerate object.
pub(crate) fn size_param(cmd: &str, p: &Value, key: &str, zero_ok: bool) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64().filter(|x| x.is_finite()) {
            Some(x) if x > 0.0 || (zero_ok && x == 0.0) => Ok(Some(x)),
            _ => Err(bad(cmd, format!("`{key}` must be {}", if zero_ok { "zero or positive" } else { "positive" }))),
        },
    }
}
/// `pts` without consecutive repeats (closer than `1e-12`), as the interactive LINE ignores a
/// point picked twice.
pub(crate) fn distinct_points(pts: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        if !out.last().is_some_and(|l| l.near(*p, 1e-12)) {
            out.push(*p);
        }
    }
    out
}
pub(crate) fn bool_or(p: &Value, key: &str, default: bool) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(default)
}
pub(crate) fn str_param<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}
pub fn point_value(v: &Value) -> Option<Vec2> {
    if let Some(a) = v.as_array() {
        let x = a.first()?.as_f64()?;
        let y = a.get(1)?.as_f64()?;
        let p = Vec2::new(x, y);
        return p.is_finite().then_some(p);
    }
    if let Some(s) = v.as_str() {
        return crate::prompt::parse_point(s, Vec2::ZERO);
    }
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    let p = Vec2::new(x, y);
    p.is_finite().then_some(p)
}
pub(crate) fn point_param(p: &Value, key: &str) -> Option<Vec2> {
    p.get(key).and_then(point_value)
}
pub(crate) fn point_req(cmd: &str, p: &Value, key: &str) -> Result<Vec2> {
    point_param(p, key).ok_or_else(|| bad(cmd, format!("`{key}` ([x, y]) is required")))
}
pub(crate) fn points_param(p: &Value, key: &str) -> Option<Vec<Vec2>> {
    p.get(key)?.as_array()?.iter().map(point_value).collect()
}
/// Handles from `handles` (hex strings or numbers), `handle`, or else the current selection. A
/// `handle`/`handles` that can't be read is an error, never a silent fall back to the selection.
pub(crate) fn targets(s: &Session, p: &Value) -> Result<Vec<Handle>> {
    let handle = |v: &Value| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle));
    let not_handle = |key: &str, v: &Value| EngineError::Other(format!("`{key}`: {v} is not an object handle (a hex string such as \"1A2\")"));
    match p.get("handles") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => return a.iter().map(|v| handle(v).ok_or_else(|| not_handle("handles", v))).collect(),
        Some(v) => return Err(EngineError::Other(format!("`handles` must be an array of object handles, got {v}"))),
    }
    match p.get("handle") {
        None | Some(Value::Null) => Ok(s.selection()),
        Some(v) => Ok(vec![handle(v).ok_or_else(|| not_handle("handle", v))?]),
    }
}
pub(crate) fn ok() -> Result<Value> {
    Ok(Value::Null)
}
