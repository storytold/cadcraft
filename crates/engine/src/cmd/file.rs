//! File menu: new, close, switching drawings. Open/save of DWG/DXF go through the io layer,
//! registered by the host (desktop/CLI) via [`crate::cmd::file::set_io`]. On the web, where there is
//! no file system, saved files reach the user through the host's [`crate::cmd::file::set_deliver`].

mod pgp;
mod template;

use std::sync::OnceLock;

use cadcraft_doc::Drawing;
pub use cadcraft_dxf::Version;
use serde_json::{Value, json};

use super::{CommandSpec, EngineError, Interactive, always, bad, bool_or, find_command, ok, str_param};

use crate::{Accept, Input, Prompt, Result, Session, Step};

/// The rules the Command Aliases dialog checks before it sends an alias to ALIASEDIT.
pub use pgp::{MAX_ALIASES, alias_name, command_id};
pub(crate) use pgp::{specs as alias_specs, user_alias};
pub use template::{qnew_template, set_qnew_template};

/// Largest drawing accepted from a local path, matching the per-file xref limit. A path naming an
/// endless device must be rejected before it exhausts memory (CWE-400), so native opens read
/// through [`crate::capped_read`] rather than buffering to EOF.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DRAWING_BYTES: usize = 512 << 20;
/// Largest `.scr` script accepted from a local path.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SCRIPT_BYTES: usize = 64 << 20;

/// Plot a space to PDF bytes with JSON options (`{paper?, landscape?, fit?, lineweights?, …}`).
pub type PlotHook = fn(&Drawing, &cadcraft_doc::Space, &Value) -> std::result::Result<Vec<u8>, String>;

/// File format hooks (installed by the app so the engine stays I/O-agnostic and wasm-safe).
pub struct IoHooks {
    pub read: fn(&[u8], &str) -> std::result::Result<Drawing, String>,
    pub write: fn(&Drawing, &str) -> std::result::Result<Vec<u8>, String>,
    /// PLOT / EXPORTPDF (optional: builds without a plotter report "not available").
    pub plot: Option<PlotHook>,
}

static IO: OnceLock<IoHooks> = OnceLock::new();

pub fn set_io(h: IoHooks) {
    let _ = IO.set(h);
}
pub fn io() -> Option<&'static IoHooks> {
    IO.get()
}

/// An entity as DXF groups and back, for AutoLISP's `entget`, `entmake` and `entmod`
/// (installed by the app with the DXF reader and writer, a lower layer).
pub struct EntityCodec {
    /// The entity's type name and the groups of its record, without its `0` group.
    pub groups: fn(&Drawing, &cadcraft_doc::Entity) -> (String, Vec<cadcraft_dxf::Tag>),
    /// An entity from a type name and groups.
    pub entity: fn(&str, &[cadcraft_dxf::Tag]) -> Option<(cadcraft_doc::Common, cadcraft_doc::EntityKind)>,
}

static CODEC: OnceLock<EntityCodec> = OnceLock::new();

pub fn set_entity_codec(c: EntityCodec) {
    let _ = CODEC.set(c);
}
pub fn entity_codec() -> Option<&'static EntityCodec> {
    CODEC.get()
}

/// COMPARE (drawing comparison).
pub mod compare;

/// Hand finished file bytes to the user where there is no file system (the browser: a download).
/// Arguments: file name, MIME type, bytes. `Ok` only once the delivery was actually started.
pub type DeliverHook = fn(&str, &str, &[u8]) -> std::result::Result<(), String>;

static DELIVER: OnceLock<DeliverHook> = OnceLock::new();

/// Install the host's file delivery (the web app: a browser download). Saving uses it on wasm;
/// anything else that produces a file on the web (plots) can use [`deliver`] too.
pub fn set_deliver(h: DeliverHook) {
    let _ = DELIVER.set(h);
}
pub fn deliver() -> Option<DeliverHook> {
    DELIVER.get().copied()
}

/// How a drawing file is written: its version (`None`: the drawing's own, `$ACADVER`) and
/// binary instead of ASCII DXF.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveAs {
    pub version: Option<Version>,
    pub binary: bool,
}

/// Write a drawing with [`SaveAs`] options (installed by hosts whose writer supports them).
pub type WriteAsHook = fn(&Drawing, &str, &SaveAs) -> std::result::Result<Vec<u8>, String>;

static WRITE_AS: OnceLock<WriteAsHook> = OnceLock::new();

/// Install the host's writer for [`SaveAs`] options; saves that ask for none use [`IoHooks::write`].
pub fn set_write_as(h: WriteAsHook) {
    let _ = WRITE_AS.set(h);
}
pub fn write_as() -> Option<WriteAsHook> {
    WRITE_AS.get().copied()
}

/// The versions a drawing can be saved in, for messages ("2000, 2004, …, 2018").
fn version_list() -> String {
    Version::ALL.iter().map(|v| v.year()).collect::<Vec<_>>().join(", ")
}

/// The `version` and `binary` parameters of a save: a version is a year (2018 or "2018"), "R2018"
/// or an `$ACADVER` code ("AC1032").
fn save_options(cmd: &str, p: &Value) -> Result<SaveAs> {
    let version = match p.get("version") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            };
            let name: String = text.chars().take(40).collect();
            Some(Version::parse(&text).ok_or_else(|| bad(cmd, format!("unknown version `{name}` (use {})", version_list())))?)
        }
    };
    let binary = match p.get("binary") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`binary` must be true or false")),
    };
    Ok(SaveAs { version, binary })
}

/// Whether `path` names a DXF file (or has no extension, which saves as DXF).
fn is_dxf(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "dxf" | "")
}

/// The drawing written to `path` with `opts`: through [`IoHooks::write`] when no option is set,
/// else through the [`write_as`] hook.
fn write_bytes(s: &Session, cmd: &str, path: &str, opts: &SaveAs) -> Result<Vec<u8>> {
    let hooks = io().ok_or_else(|| bad(cmd, "file formats are not available in this build"))?;
    if opts.binary && !is_dxf(path) {
        return Err(bad(cmd, "binary applies to DXF files only"));
    }
    // A drawing file is the drawing outside the Block Editor; an image is what is shown.
    let d: &cadcraft_doc::Drawing = if is_drawing_file(path) { s.state()?.drawing() } else { s.doc()? };
    let r = if *opts == SaveAs::default() {
        (hooks.write)(d, path)
    } else {
        let w = write_as().ok_or_else(|| bad(cmd, "saving at a chosen version or as binary DXF is not available in this build"))?;
        w(d, path, opts)
    };
    r.map_err(|e| bad(cmd, e))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("new", "New Drawing...", run_new)
            .menu(&["File", "New Drawing..."])
            .key("Cmd+N")
            .alias(&["qnew"])
            .params("{metric?: bool (default MEASUREINIT), template?: path to a .dwt/.dxf template (\".\" for none; default QNEWTEMPLATE)}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("open", "Open...", run_open)
            .menu(&["File", "Open..."])
            .key("Cmd+O")
            .params("{path} | {data: base64, name} → {index, entities?, warnings: what reading left out}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("close", "Close", run_close).menu(&["File", "Close"]).key("Cmd+W").params("{index?}").noundo(),
        CommandSpec::new("closeall", "Close All", run_closeall).menu(&["File", "Close All"]).enabled(always).noundo(),
        CommandSpec::new("qsave", "Save", run_qsave_reporting).menu(&["File", "Save"]).key("Cmd+S").alias(&["save"]).params("{path?}").noundo(),
        CommandSpec::new("saveas", "Save As...", run_saveas_reporting)
            .menu(&["File", "Save As..."])
            .key("Cmd+Shift+S")
            .params("{path, version?: 2000|2004|2007|2010|2013|2018, binary?: bool}")
            .noundo(),
        CommandSpec::new("document.switch", "Switch Drawing", run_switch).params("{index}").enabled(always).noundo(),
        CommandSpec::new("document.bytes", "Drawing as bytes", run_bytes)
            .params("{format?: dxf, version?: 2000…2018, binary?: bool} → {data: base64}")
            .noundo(),
        CommandSpec::new("script", "Run Script...", run_script)
            .menu(&["Tools", "Run Script..."])
            .alias(&["scr"])
            .params("{path: script (.scr) file} | {data: base64} → {lines}")
            .noundo()
            .interactive(|_| Ok(Box::new(ScriptM))),
    ]
}

/// SCRIPT: run the lines of a `.scr` file through [`Session::script`] — the same execution the
/// CLI, MCP and the control channel already use, now reachable from the command line and menus.
fn run_script(s: &mut Session, p: &Value) -> Result<Value> {
    let text = if let Some(_path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let path = _path;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes =
                crate::capped_read::read_capped(std::path::Path::new(path), MAX_SCRIPT_BYTES).map_err(|e| bad("script", format!("{path}: {e}")))?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        #[cfg(target_arch = "wasm32")]
        return Err(bad("script", "paths are not available on the web; pass `data`"));
    } else {
        let data = str_param(p, "data").ok_or_else(|| bad("script", "`path` or `data` (base64) is required"))?;
        let bytes = base64_decode(data).ok_or_else(|| bad("script", "invalid base64"))?;
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let lines = text.lines().count();
    s.script(&text)?;
    Ok(json!({ "lines": lines }))
}

/// SCRIPT typed at the command line or chosen from the menu: asks for the script file by name
/// (the prompt form; the menu item runs this same machine).
struct ScriptM;

impl Interactive for ScriptM {
    fn name(&self) -> &'static str {
        "SCRIPT"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Enter name of script file", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel | Input::Enter) {
            return Ok(Step::Cancel);
        }
        let Input::Text(t) = i else { return Ok(Step::Continue) };
        let t = t.trim();
        if t.is_empty() {
            return Ok(Step::Cancel);
        }
        match run_script(s, &json!({ "path": t })) {
            Ok(_) => Ok(Step::Done),
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(r) = template::run_new(s, p)? {
        return Ok(r);
    }
    let metric = s.settings.measureinit;
    let i = s.new_drawing(bool_or(p, "metric", metric));
    Ok(json!({ "index": i, "title": s.docs.get(i).map(|d| d.title.clone()) }))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// A file read and decoded but not yet added to a [`Session`]. Reading is the slow part of OPEN and
/// needs no session, so a host can do it on a worker thread and hand the result to
/// [`finish_open`] on the thread that owns the session.
pub struct ReadFile {
    drawing: Drawing,
    binary_dxf: bool,
    name: String,
    path: Option<String>,
}

/// Read and decode `path` (the slow half of `OPEN {path}`). Needs no session; runs anywhere.
pub fn read_file(path: &str) -> std::result::Result<ReadFile, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let r = std::panic::catch_unwind(|| {
            let bytes =
                crate::capped_read::read_capped(std::path::Path::new(path), MAX_DRAWING_BYTES).map_err(|e| bad("open", format!("{path}: {e}")))?;
            read_bytes(&bytes, path, Some(path.to_string()))
        });
        match r {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(_) => Err(bad("open", format!("{path}: the file could not be read")).to_string()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        Err(bad("open", "paths are not available on the web; pass `data`").to_string())
    }
}

/// Add a [`read_file`] result to the session as the active drawing: the fast half of OPEN.
pub fn finish_open(s: &mut Session, f: ReadFile) -> Result<Value> {
    let ReadFile { drawing, binary_dxf, name, path } = f;
    let i = add_read(s, drawing, binary_dxf, &name, path);
    super::blocks::xref::after_open(s, i);
    Ok(opened(s, i, true))
}

fn read_bytes(bytes: &[u8], name: &str, path: Option<String>) -> Result<ReadFile> {
    let hooks = io().ok_or_else(|| bad("open", "file formats are not available in this build"))?;
    let drawing = (hooks.read)(bytes, name).map_err(|e| bad("open", e))?;
    // A file that isn't a drawing (an imported SVG) gives the drawing no file name: QSAVE then
    // asks where to save instead of exporting over the original.
    let path = path.filter(|p| is_drawing_file(p));
    Ok(ReadFile { drawing, binary_dxf: cadcraft_dxf::is_binary(bytes), name: name.to_string(), path })
}

fn add_read(s: &mut Session, drawing: Drawing, binary_dxf: bool, name: &str, path: Option<String>) -> usize {
    let notes = drawing.read_notes.clone();
    let i = s.open_drawing(drawing, &file_name(name), path);
    if let Some(st) = s.docs.get_mut(i) {
        // QSAVE keeps a binary DXF binary.
        st.binary_dxf = binary_dxf;
    }
    for note in notes {
        s.echo(note);
    }
    i
}

pub(crate) fn open_bytes(s: &mut Session, bytes: &[u8], name: &str, path: Option<String>) -> Result<usize> {
    let ReadFile { drawing, binary_dxf, name, path } = read_bytes(bytes, name, path)?;
    Ok(add_read(s, drawing, binary_dxf, &name, path))
}

/// OPEN's reply: the drawing's index and what reading it left out.
fn opened(s: &Session, i: usize, entities: bool) -> Value {
    let d = s.docs.get(i).map(|st| &st.doc);
    let warnings = d.map(|d| d.read_notes.clone()).unwrap_or_default();
    if entities {
        json!({ "index": i, "entities": d.map(|d| d.entity_count()), "warnings": warnings })
    } else {
        json!({ "index": i, "warnings": warnings })
    }
}

/// Open a drawing from bounded regular-file bytes or base64 data and resolve its external references.
fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(_path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let path = _path;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes =
                crate::capped_read::read_capped(std::path::Path::new(path), MAX_DRAWING_BYTES).map_err(|e| bad("open", format!("{path}: {e}")))?;
            let i = open_bytes(s, &bytes, path, Some(path.to_string()))?;
            super::blocks::xref::after_open(s, i);
            return Ok(opened(s, i, true));
        }
        #[cfg(target_arch = "wasm32")]
        return Err(bad("open", "paths are not available on the web; pass `data`"));
    }
    let data = str_param(p, "data").ok_or_else(|| bad("open", "`path` or `data` (base64) is required"))?;
    let bytes = base64_decode(data).ok_or_else(|| bad("open", "invalid base64"))?;
    let name = str_param(p, "name").unwrap_or("Drawing.dxf");
    let i = open_bytes(s, &bytes, name, None)?;
    super::blocks::xref::after_open(s, i);
    Ok(opened(s, i, false))
}

/// `close [index]`: cancel the running command, remove the drawing (default: active), keep the active index valid, and bump the document epoch; an unknown index is an error.
fn run_close(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(s.active);
    if i >= s.docs.len() {
        return Err(bad("close", "no such drawing"));
    }
    s.cancel();
    s.docs.remove(i);
    if i < s.active {
        // An earlier drawing went away: the active one shifted down by one.
        s.active -= 1;
    }
    if s.active >= s.docs.len() {
        s.active = s.docs.len().saturating_sub(1);
    }
    s.bump_doc_epoch();
    ok()
}

/// `closeall`: cancel the running command, remove every drawing, reset the active index to 0 and bump the document epoch.
fn run_closeall(s: &mut Session, _p: &Value) -> Result<Value> {
    s.cancel();
    s.docs.clear();
    s.active = 0;
    s.bump_doc_epoch();
    ok()
}

pub(crate) fn save_to(s: &mut Session, path: &str) -> Result<usize> {
    let opts = SaveAs { version: None, binary: s.state()?.binary_dxf && is_dxf(path) };
    save_as(s, "save", path, &opts)
}

/// Save to `path` with `opts`. A drawing file then keeps the version and binary choice: the
/// version goes into the drawing's `$ACADVER`, so later saves (QSAVE) write it again.
fn save_as(s: &mut Session, cmd: &str, path: &str, opts: &SaveAs) -> Result<usize> {
    if s.doc()?.refedit.is_some() {
        return Err(bad(cmd, "A block reference is being edited in place: finish with REFCLOSE (Save or Discard) before saving."));
    }
    let bytes = write_bytes(s, cmd, path, opts)?;
    let n = commit_save(s, path, &bytes, store)?;
    if is_drawing_file(path) {
        let st = s.state_mut()?;
        st.binary_dxf = opts.binary;
        if let Some(v) = opts.version
            && st.drawing().header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str) != Some(v.code())
        {
            std::sync::Arc::make_mut(st.drawing_arc_mut()).header.set_str("ACADVER", v.code());
            st.saved = st.drawing().clone();
        }
    }
    Ok(n)
}

/// Put saved bytes where the user gets them: natively the file at `path` (written atomically); on
/// the web, which has no file system, a browser download named after `path` via [`deliver`].
fn store(path: &str, bytes: &[u8]) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Exclusively create a fresh temporary file and rename it over `path`, so a planted
        // `<path>.cadcraft-tmp` symlink cannot redirect the write onto another file (CWE-59).
        crate::atomic_write::write_atomic(std::path::Path::new(path), bytes).map_err(|e| bad("save", format!("{path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let name = file_name(path);
        let deliver = deliver().ok_or_else(|| bad("save", "saving files is not available in this build; use document.bytes"))?;
        deliver(&name, "application/octet-stream", bytes).map_err(|e| bad("save", format!("{name}: {e}")))
    }
}

/// Store the bytes, then mark the drawing saved under `path`. If storing fails the drawing keeps
/// its unsaved changes, file name and title. On the web a started download counts as saved: the
/// browser owns the file from there (we can't see whether the user keeps it), and the name stays so
/// a later QSAVE downloads the drawing again under it. An image or PDF export (SVG, PNG, PDF) only
/// stores the file: the drawing keeps its name, title and unsaved changes, so closing still asks.
fn commit_save(s: &mut Session, path: &str, bytes: &[u8], store: impl FnOnce(&str, &[u8]) -> Result<()>) -> Result<usize> {
    s.state()?;
    store(path, bytes)?;
    if !is_drawing_file(path) {
        return Ok(bytes.len());
    }
    let st = s.state_mut()?;
    st.saved = st.drawing().clone();
    st.path = Some(path.to_string());
    st.title = file_name(path);
    // A BEgin group still open when the drawing is saved ends there.
    st.end_undo_group();
    Ok(bytes.len())
}

/// The save result; on the web it also names the file the browser downloaded.
fn saved(path: &str, bytes: usize) -> Value {
    let mut r = json!({ "path": path, "bytes": bytes });
    if cfg!(target_arch = "wasm32") {
        r["download"] = json!(file_name(path));
    }
    r
}

fn run_save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match str_param(p, "path") {
        Some(p) => p.to_string(),
        None => s.state()?.path.clone().ok_or_else(|| bad("qsave", "drawing has no file name yet; use saveas {path}"))?,
    };
    let n = save_to(s, &path)?;
    Ok(saved(&path, n))
}

fn run_saveas(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("saveas", "`path` is required"))?.to_string();
    let opts = save_options("saveas", p)?;
    let n = save_as(s, "saveas", &path, &opts)?;
    let mut r = saved(&path, n);
    if is_drawing_file(&path) {
        r["version"] = json!(opts.version.unwrap_or_else(|| drawing_version(s, &path)).year());
        r["binary"] = json!(opts.binary);
    }
    Ok(r)
}

/// The version a drawing is saved in at `path` when none is asked for: its `$ACADVER` (2018 by
/// default), and for DWG at least 2004 (the io layer's rule: R2000 DWG has no true colours).
fn drawing_version(s: &Session, path: &str) -> Version {
    let v =
        s.doc().ok().and_then(|d| d.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(Version::for_acadver)).unwrap_or_default();
    if is_dxf(path) { v } else { v.max(Version::R2004) }
}

/// `document.switch index`: require the index, cancel the running command, activate that drawing and bump the epoch even if it was already active (screenshot identity).
fn run_switch(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("document.switch", "`index` is required"))? as usize;
    if i >= s.docs.len() {
        return Err(bad("document.switch", "no such drawing"));
    }
    s.cancel();
    s.active = i;
    // Bump even when `i` is already active. This is intentionally conservative: the command
    // cancels the active command, and a screenshot client can only see observed state, not
    // whether the switch was a no-op. Rejecting an outstanding capture is safer than returning
    // an image whose queueing identity the client cannot distinguish from a real switch.
    s.bump_doc_epoch();
    ok()
}

fn run_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let fmt = str_param(p, "format").unwrap_or("dxf");
    let opts = save_options("document.bytes", p)?;
    let version = opts.version.unwrap_or_else(|| drawing_version(s, &format!("drawing.{fmt}")));
    let lost = not_saved_as(s.doc()?, version);
    let bytes = write_bytes(s, "document.bytes", &format!("drawing.{fmt}"), &opts)?;
    let mut r = json!({ "data": base64_encode(&bytes), "bytes": bytes.len() });
    if drops_unknown(&format!("drawing.{fmt}")) {
        report_not_saved(s, &mut r, &lost);
    }
    Ok(r)
}

/// Entities a DXF or DWG file doesn't keep: those of object types CADCraft doesn't model
/// (3DSOLID, MULTILEADER… read from other programs' files and kept in the drawing, but
/// not written), counted by DXF type across model space, layouts and block definitions. Saved
/// at the drawing's own version; see [`not_saved_as`].
pub fn not_saved(d: &Drawing) -> std::collections::BTreeMap<String, usize> {
    let v = d.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(Version::for_acadver).unwrap_or_default();
    not_saved_as(d, v)
}

/// Solid modeler entities whose ACIS geometry a file of AutoCAD 2013 and later keeps in its data
/// store section, which is written back with the drawing (`Foreign::data_store`).
const MODELER_TYPES: [&str; 3] = ["3DSOLID", "REGION", "BODY"];

/// [`not_saved`] for a file saved as `version`. From 2013 on, a 3D solid, region or body the data
/// store has a record for keeps its geometry and is not counted.
pub fn not_saved_as(d: &Drawing, version: Version) -> std::collections::BTreeMap<String, usize> {
    let mut out: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let stored = if version >= Version::R2013 { d.foreign.data_store_entities() } else { Default::default() };
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for e in stores.flat_map(|s| s.iter()) {
        if let cadcraft_doc::EntityKind::Unknown(u) = &e.kind {
            if MODELER_TYPES.iter().any(|t| u.dxf_type.eq_ignore_ascii_case(t)) && stored.contains(&e.handle) {
                continue;
            }
            let n = out.entry(u.dxf_type.clone()).or_insert(0);
            *n = n.saturating_add(1);
        }
    }
    out
}

/// The command-line warning for [`not_saved`] entities, or `None` when the file keeps everything.
pub fn not_saved_message(lost: &std::collections::BTreeMap<String, usize>) -> Option<String> {
    if lost.is_empty() {
        return None;
    }
    let list: Vec<String> = lost.iter().map(|(t, n)| format!("{n} {t}")).collect();
    Some(format!("Not saved in the DWG file (its writer can't write these object types yet; DXF keeps them): {}", list.join(", ")))
}

/// Whether saving to `path` leaves out the entities [`not_saved`] counts: DWG does (the DWG
/// bridge drops types it doesn't know); DXF writes them back as read.
pub fn drops_unknown(path: &str) -> bool {
    // Templates (.dwt) are written as DWG too.
    std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dwt"))
}

/// Whether `path` is saved as a drawing (DXF, DWG) rather than exported as an image.
fn is_drawing_file(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "dxf" | "dwg" | "dwt" | "")
}

/// Say on the command line what the file couldn't keep, and list it in the result's `notSaved`.
fn report_not_saved(s: &mut Session, r: &mut Value, lost: &std::collections::BTreeMap<String, usize>) {
    let Some(msg) = not_saved_message(lost) else { return };
    s.echo(msg);
    if let Some(o) = r.as_object_mut() {
        o.insert("notSaved".into(), json!(lost));
    }
}

/// Save, then report the entities the file couldn't keep (nothing is dropped silently).
fn save_reporting(s: &mut Session, p: &Value, run: fn(&mut Session, &Value) -> Result<Value>) -> Result<Value> {
    // What the file will hold depends on the version it is saved as (the data store from 2013).
    let path = str_param(p, "path").map(str::to_string).or_else(|| s.state().ok().and_then(|st| st.path.clone())).unwrap_or_default();
    let version = save_options("save", p).ok().and_then(|o| o.version).unwrap_or_else(|| drawing_version(s, &path));
    let lost = s.doc().map(|d| not_saved_as(d, version)).unwrap_or_default();
    let mut r = run(s, p)?;
    if r.get("path").and_then(Value::as_str).is_some_and(drops_unknown) {
        report_not_saved(s, &mut r, &lost);
    }
    Ok(r)
}

fn run_qsave_reporting(s: &mut Session, p: &Value) -> Result<Value> {
    save_reporting(s, p, run_save)
}

fn run_saveas_reporting(s: &mut Session, p: &Value) -> Result<Value> {
    save_reporting(s, p, run_saveas)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn active_title(s: &Session) -> String {
        s.state().unwrap().title.clone()
    }

    fn temp_script(name: &str, text: &str) -> String {
        let path = std::env::temp_dir().join(format!("cadcraft-script-{}-{name}", std::process::id()));
        std::fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn script_command_runs_a_scr_file() {
        let path = temp_script("run.scr", "; a comment\ncircle 50,25 20\nrectang 0,0 100,60\n");
        let mut s = Session::new();
        let r = s.execute("script", &json!({ "path": path })).unwrap();
        assert_eq!(r["lines"], 3);
        assert_eq!(s.state().unwrap().doc.model.len(), 2, "the comment is skipped, circle and rectang draw");
    }

    #[test]
    fn script_command_reports_a_missing_file() {
        let mut s = Session::new();
        let e = s.execute("script", &json!({ "path": "no/such/script.scr" })).unwrap_err().to_string();
        assert!(e.contains("no/such/script.scr"), "{e}");
        assert_eq!(s.state().unwrap().doc.model.len(), 0);
    }

    /// A script that runs itself must be refused, not recurse until the stack dies.
    #[test]
    fn a_script_cannot_start_a_script() {
        let inner = temp_script("inner.scr", "circle 0,0 1\n");
        let outer = temp_script("outer.scr", &format!("script {}\ncircle 0,0 2\n", json!({ "path": inner })));
        let mut s = Session::new();
        let e = s.execute("script", &json!({ "path": outer })).unwrap_err().to_string();
        assert!(e.contains("already running"), "{e}");
    }

    #[test]
    fn typed_script_prompts_for_the_file_then_runs_or_reports_it() {
        let good = temp_script("typed.scr", "circle 0,0 1\n");
        let mut s = Session::new();
        s.cmdline("script").unwrap();
        assert!(s.running.is_some(), "SCRIPT asks for the file name");
        s.cmdline(&good).unwrap();
        assert!(s.running.is_none());
        assert_eq!(s.state().unwrap().doc.model.len(), 1);

        // A file that cannot be read: the error is echoed, the command still ends.
        s.cmdline("script").unwrap();
        s.cmdline("no/such/script.scr").unwrap();
        assert!(s.running.is_none());
        assert!(s.log.iter().any(|l| l.contains("no/such/script.scr")), "{:?}", s.log);
    }

    #[test]
    fn closing_earlier_inactive_drawing_keeps_active_drawing() {
        let mut s = Session::new();
        s.execute("new", &json!({})).unwrap();
        s.execute("new", &json!({})).unwrap();
        s.execute("document.switch", &json!({"index": 1})).unwrap();
        assert_eq!(active_title(&s), "Drawing2");
        s.execute("close", &json!({"index": 0})).unwrap();
        assert_eq!(active_title(&s), "Drawing2");
        assert_eq!(s.docs.len(), 2);
    }

    #[test]
    fn closing_later_inactive_drawing_keeps_active_drawing() {
        let mut s = Session::new();
        s.execute("new", &json!({})).unwrap();
        s.execute("new", &json!({})).unwrap();
        s.execute("document.switch", &json!({"index": 1})).unwrap();
        s.execute("close", &json!({"index": 2})).unwrap();
        assert_eq!(active_title(&s), "Drawing2");
    }
}

#[cfg(test)]
mod pr193_tests {
    use super::*;
    use crate::Session;

    #[test]
    fn failed_store_keeps_unsaved_changes_and_delivered_save_clears_them() {
        let mut s = Session::new();
        s.cmdline("circle 4,4 1").unwrap();
        let st = s.state().unwrap();
        let title = st.title.clone();
        assert!(st.is_dirty() && st.path.is_none());

        // The file route is unavailable or fails (the web without a download): no success, still dirty.
        let failed = commit_save(&mut s, "a/Owned.dxf", b"0\nEOF\n", |_, _| Err(bad("save", "no download")));
        assert!(failed.is_err());
        let st = s.state().unwrap();
        assert!(st.is_dirty(), "a save that delivered nothing must keep the unsaved state");
        assert_eq!((st.path.as_deref(), st.title.as_str()), (None, title.as_str()));

        // The file route took the bytes (written, or a download started): saved under that name.
        let mut got = None;
        let n = commit_save(&mut s, "a/Owned.dxf", b"0\nEOF\n", |p, b| {
            got = Some((p.to_string(), b.len()));
            Ok(())
        })
        .unwrap();
        assert_eq!((n, got), (6, Some(("a/Owned.dxf".to_string(), 6))));
        let st = s.state().unwrap();
        assert!(!st.is_dirty());
        assert_eq!((st.path.as_deref(), st.title.as_str()), (Some("a/Owned.dxf"), "Owned.dxf"));
        assert_eq!(saved("a/Owned.dxf", n)["bytes"], 6);
    }
}
