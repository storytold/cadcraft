//! File menu: new, close, switching drawings. Open/save of DWG/DXF go through the io layer,
//! registered by the host (desktop/CLI) via [`crate::cmd::file::set_io`]. On the web, where there is
//! no file system, saved files reach the user through the host's [`crate::cmd::file::set_deliver`].

mod pgp;
mod template;

use std::sync::OnceLock;

use cadcraft_doc::{Drawing, Format};
pub use craft_dxf::Version;
use serde_json::{Value, json};

use super::{CommandSpec, EngineError, Interactive, always, bad, bool_or, find_command, ok, str_param};

use crate::{Accept, Input, Prompt, Result, Session, Step};

/// The rules the Command Aliases dialog checks before it sends an alias to ALIASEDIT.
pub use pgp::{MAX_ALIAS_PROFILE_BYTES, MAX_ALIASES, alias_name, alias_profile, command_id, restore_aliases};
pub(crate) use pgp::{specs as alias_specs, user_alias};
pub use template::{qnew_template, set_qnew_template};

/// Largest drawing accepted from a local path, matching the per-file xref limit. A path naming an
/// endless device must be rejected before it exhausts memory (CWE-400), so native opens read
/// through [`crate::capped_read`] rather than buffering to EOF.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_DRAWING_BYTES: usize = 512 << 20;
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

/// Edit one field of a Tangent (天正) custom entity, installed by the app with the DWG bridge
/// (a lower layer). `None` in builds without the native DWG bridge (e.g. web), where such
/// entities are not read either.
pub type TianzhengSetter = fn(&mut Drawing, cadcraft_doc::Handle, &str, &str) -> bool;

static TIANZHENG_SETTER: OnceLock<TianzhengSetter> = OnceLock::new();

pub fn set_tianzheng_setter(f: TianzhengSetter) {
    let _ = TIANZHENG_SETTER.set(f);
}
pub fn tianzheng_setter() -> Option<TianzhengSetter> {
    TIANZHENG_SETTER.get().copied()
}

/// An entity as DXF groups and back, for AutoLISP's `entget`, `entmake` and `entmod`
/// (installed by the app with the DXF reader and writer, a lower layer).
pub struct EntityCodec {
    /// The entity's type name and the groups of its record, without its `0` group.
    pub groups: fn(&Drawing, &cadcraft_doc::Entity) -> (String, Vec<craft_dxf::Tag>),
    /// An entity from a type name and groups.
    pub entity: fn(&str, &[craft_dxf::Tag]) -> Option<(cadcraft_doc::Common, cadcraft_doc::EntityKind)>,
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

/// Decode a raster image file (PNG, JPEG, BMP, TIFF) to pixels: installed by the app with the
/// image reader, a lower layer.
pub type ImageHook = fn(&[u8]) -> std::result::Result<cadcraft_doc::Raster, String>;

static IMAGE: OnceLock<ImageHook> = OnceLock::new();

pub fn set_image_decoder(h: ImageHook) {
    let _ = IMAGE.set(h);
}
pub fn image_decoder() -> Option<ImageHook> {
    IMAGE.get().copied()
}

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

/// How a drawing file is written: its version (`None`: the drawing's own, `$ACADVER`), binary
/// instead of ASCII DXF, and Windows fonts for Linux ones ([`cadcraft_doc::Drawing::portable_fonts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveAs {
    pub version: Option<Version>,
    pub binary: bool,
    pub portable_fonts: bool,
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
    let portable_fonts = match p.get("portableFonts") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`portableFonts` must be true or false")),
    };
    Ok(SaveAs { version, binary, portable_fonts })
}

/// Whether `path` names a DXF file (or has no extension, which saves as DXF).
fn is_dxf(path: &str) -> bool {
    Format::from_path(path) == Some(Format::Dxf)
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
    let copy = (opts.portable_fonts && is_drawing_file(path)).then(|| Drawing { portable_fonts: true, ..d.clone() });
    let d = copy.as_ref().unwrap_or(d);
    let r = if opts.version.is_none() && !opts.binary {
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
            .params(
                "{path, version?: 2000|2004|2007|2010|2013|2018, binary?: bool, portableFonts?: bool (Windows fonts for Linux ones, in this file)}",
            )
            .noundo(),
        CommandSpec::new("document.switch", "Switch Drawing", run_switch).params("{index}").enabled(always).noundo(),
        CommandSpec::new("document.bytes", "Drawing as bytes", run_bytes)
            .params("{format?: dxf, version?: 2000…2018, binary?: bool, portableFonts?: bool} → {data: base64}")
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
pub fn read_file(path: &std::path::Path) -> std::result::Result<ReadFile, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let r = std::panic::catch_unwind(|| {
            let shown = path.display();
            // The session keeps paths as text; a name that isn't Unicode could never be saved back to.
            let Some(text_path) = path.to_str() else {
                return Err(bad("open", format!("{shown}: the file name is not valid Unicode, so CADCraft can't save back to it")));
            };
            let bytes = crate::capped_read::read_capped(path, MAX_DRAWING_BYTES).map_err(|e| bad("open", format!("{shown}: {e}")))?;
            read_bytes(&bytes, text_path, Some(text_path.to_string()))
        });
        match r {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(_) => Err(bad("open", format!("{}: the file could not be read", path.display())).to_string()),
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
    super::image::after_open(s, i);
    Ok(opened(s, i, true))
}

fn read_bytes(bytes: &[u8], name: &str, path: Option<String>) -> Result<ReadFile> {
    let hooks = io().ok_or_else(|| bad("open", "file formats are not available in this build"))?;
    let drawing = (hooks.read)(bytes, name).map_err(|e| bad("open", e))?;
    // A file that isn't a drawing (an imported SVG) gives the drawing no file name: QSAVE then
    // asks where to save instead of exporting over the original.
    let path = path.filter(|p| is_drawing_file(p));
    // The SHX fonts its text styles name are looked for beside the drawing too.
    if let Some(dir) = path.as_deref().and_then(|p| std::path::Path::new(p).parent()) {
        // A bare file name is in the current folder.
        let dir = if dir.as_os_str().is_empty() { std::path::Path::new(".") } else { dir };
        cadcraft_fonts::shx::add_drawing_dir(&std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()));
    }
    Ok(ReadFile { drawing, binary_dxf: craft_dxf::is_binary(bytes), name: name.to_string(), path })
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
    // Opened on the view it was saved at: a viewer keeps it instead of fitting the extents.
    let saved_view = d.is_some_and(|d| crate::view::saved_view(d).is_some());
    if entities {
        json!({ "index": i, "entities": d.map(|d| d.entity_count()), "warnings": warnings, "saved_view": saved_view })
    } else {
        json!({ "index": i, "warnings": warnings, "saved_view": saved_view })
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
            super::image::after_open(s, i);
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
    super::image::after_open(s, i);
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
    let opts = SaveAs { binary: s.state()?.binary_dxf && is_dxf(path), ..SaveAs::default() };
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

/// Put saved bytes where the user gets them: natively the file at `path`; on the web, which has
/// no file system, a browser download named after `path` via [`deliver`].
///
/// Natively the file is replaced atomically ([`crate::atomic_write`]: a fresh temporary file
/// beside it, renamed over it), so a crash leaves either the old file or the new one, never a
/// truncated one. Saving over a drawing first keeps the previous file as `<name>.bak` (ISAVEBAK,
/// #540).
fn store(path: &str, bytes: &[u8]) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // A file in a folder still to be made has nothing to back up.
        if is_drawing_file(path) && crate::autosave::isavebak() && crate::atomic_write::missing_folder(std::path::Path::new(path)).is_none() {
            backup(std::path::Path::new(path))
                .map_err(|e| bad("save", format!("{path}: the backup copy could not be written ({e}); nothing was saved")))?;
        }
        // Exclusively create a fresh temporary file and rename it over `path`, so a planted
        // `<path>.cadcraft-tmp` symlink cannot redirect the write onto another file (CWE-59).
        crate::atomic_write::write_atomic_making_folders(std::path::Path::new(path), bytes).map_err(|e| bad("save", format!("{path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let name = file_name(path);
        let deliver = deliver().ok_or_else(|| bad("save", "saving files is not available in this build; use document.bytes"))?;
        deliver(&name, "application/octet-stream", bytes).map_err(|e| bad("save", format!("{name}: {e}")))
    }
}

/// The backup of the drawing file `path`, beside it. A DWG's is `<name>.bak`, as AutoCAD names it,
/// in the extension's letter case (`plan.DWG` to `plan.BAK`); any other drawing appends `.bak`
/// (`plan.dxf.bak`), so `plan.dwg` and `plan.dxf` in one folder never overwrite each other's backup.
pub fn backup_path(path: &std::path::Path) -> std::path::PathBuf {
    match path.extension().map(|e| e.to_string_lossy()) {
        Some(e) if e.eq_ignore_ascii_case("dwg") => path.with_extension(
            e.chars().zip("bak".chars()).map(|(d, b)| if d.is_ascii_uppercase() { b.to_ascii_uppercase() } else { b }).collect::<String>(),
        ),
        _ => {
            let mut name = path.as_os_str().to_os_string();
            name.push(".bak");
            name.into()
        }
    }
}

/// Whether `bytes` read as a DWG or a DXF (ASCII or binary) by their start: what a save keeps as
/// the backup. A file that doesn't (truncated, overwritten by another program) never replaces a
/// good `.bak`.
pub fn looks_like_drawing(bytes: &[u8]) -> bool {
    // DWG: a version tag like `AC1032` (R12 is `AC1009`).
    let dwg = bytes.len() >= 6 && bytes.starts_with(b"AC") && bytes[2..6].iter().all(|b| b.is_ascii_digit());
    if dwg || craft_dxf::is_binary(bytes) {
        return true;
    }
    // ASCII DXF: optional comments (999), then group 0 SECTION.
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    loop {
        match (lines.next(), lines.next()) {
            (Some("999"), Some(_)) => continue,
            (Some("0"), Some(v)) => return v.eq_ignore_ascii_case("SECTION"),
            _ => return false,
        }
    }
}

/// How much of a drawing's end [`looks_complete`] reads.
#[cfg(not(target_arch = "wasm32"))]
const TAIL_BYTES: u64 = 256;

/// Whether a drawing that [`looks_like_drawing`] (`head`, its first bytes, at least 256 of them
/// for a DWG) is whole, from its end (`tail`, its last bytes) and size `len`, without reading it
/// all: a DXF ends with `0`/`EOF` (a binary one with `EOF` and its NUL); a DWG reaches the end its
/// own header gives ([`dwg_complete`]). `None` when that can't be told (a 2007 DWG), so a file cut
/// off by a failed save or another program doesn't replace a good backup.
pub fn completeness(head: &[u8], tail: &[u8], len: u64) -> Option<bool> {
    let dwg = head.len() >= 6 && head.starts_with(b"AC") && head[2..6].iter().all(|b| b.is_ascii_digit());
    if dwg {
        // Too short for any DWG's header: cut off, not unknown.
        return if head.len() < 256 { Some(false) } else { dwg_complete(head, len) };
    }
    if craft_dxf::is_binary(head) {
        return Some(tail.ends_with(b"EOF\0"));
    }
    let text = String::from_utf8_lossy(tail);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty()).rev();
    Some(matches!((lines.next(), lines.next()), (Some(last), Some("0")) if last.eq_ignore_ascii_case("EOF")))
}

/// [`completeness`] known to be whole.
pub fn looks_complete(head: &[u8], tail: &[u8], len: u64) -> bool {
    completeness(head, tail, len) != Some(false)
}

/// Whether a DWG of `len` bytes reaches the end its header gives (Open Design Specification for
/// .dwg files): from 2004 on, the copy of the file header that ends the file (its address is in
/// the encrypted header at 0x80, and the copy is 0x6C bytes); up to 2000, the end of every section
/// the locator records give. `None` for another layout (2007, or before R13).
fn dwg_complete(head: &[u8], len: u64) -> Option<bool> {
    let version = head.get(..6)?;
    match version {
        b"AC1018" | b"AC1024" | b"AC1027" | b"AC1032" => {
            // The header at 0x80 is XORed with the generator the specification gives.
            let enc = head.get(0x80..0x80 + 0x6C)?;
            let mut seed: u32 = 1;
            let h: Vec<u8> = enc
                .iter()
                .map(|b| {
                    seed = seed.wrapping_mul(0x343FD).wrapping_add(0x0026_9EC3);
                    b ^ (seed >> 16) as u8
                })
                .collect();
            if h.get(..12) != Some(&b"AcFssFcAJMB\0"[..]) {
                return Some(false);
            }
            let second = u64::from_le_bytes(h.get(0x34..0x3C)?.try_into().ok()?);
            Some(second >= 0x100 && second.checked_add(0x6C).is_some_and(|end| len >= end))
        }
        b"AC1012" | b"AC1014" | b"AC1015" => {
            let n = i32::from_le_bytes(head.get(0x15..0x19)?.try_into().ok()?);
            if !(1..=16).contains(&n) {
                return Some(false);
            }
            let mut end = 0u64;
            for i in 0..n as usize {
                let r = head.get(0x19 + i * 9..0x19 + i * 9 + 9)?;
                let seek = u64::from(u32::from_le_bytes(r.get(1..5)?.try_into().ok()?));
                let size = u64::from(u32::from_le_bytes(r.get(5..9)?.try_into().ok()?));
                end = end.max(seek + size);
            }
            Some(len >= end)
        }
        _ => None,
    }
}

/// A file's first bytes, last bytes and length, left positioned just after the first bytes.
#[cfg(not(target_arch = "wasm32"))]
fn ends(src: &mut std::fs::File) -> std::io::Result<(Vec<u8>, Vec<u8>, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut head = Vec::with_capacity(4096);
    (&mut *src).take(4096).read_to_end(&mut head)?;
    // Its end too (a DXF's EOF), then on from where the start left off for the copy.
    let len = src.metadata()?.len();
    let mut tail = Vec::new();
    src.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))?;
    (&mut *src).take(TAIL_BYTES).read_to_end(&mut tail)?;
    src.seek(SeekFrom::Start(head.len() as u64))?;
    Ok((head, tail, len))
}

/// Whether saving replaces the `.bak` with this file. A file known to be cut off never replaces a
/// backup already there. One that can't be told (a 2007 DWG) does: it is most likely fine, and it
/// is about to be replaced. With no backup yet any file is kept, because the checks can be wrong
/// about a real file and this save replaces the only copy.
#[cfg(not(target_arch = "wasm32"))]
fn backup_replaced(head: &[u8], tail: &[u8], len: u64, backup_exists: bool) -> bool {
    (looks_like_drawing(head) && completeness(head, tail, len) != Some(false)) || (len > 0 && !backup_exists)
}

/// Whether a save over `path` now would leave the current file in its `.bak`, for a warning that
/// must not promise a backup the save won't write.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_keeps_current(path: &std::path::Path) -> bool {
    let Ok(mut src) = std::fs::File::open(path) else { return false };
    crate::autosave::isavebak() && ends(&mut src).is_ok_and(|(head, tail, len)| backup_replaced(&head, &tail, len, backup_path(path).exists()))
}

#[cfg(target_arch = "wasm32")]
pub fn save_keeps_current(_path: &std::path::Path) -> bool {
    false
}

/// The mode of a new backup: no more open than the drawing (`src_mode`, group `src_gid`) nor than the
/// `.bak` it replaces (`old`: mode and group). The backup keeps the old `.bak`'s group, so when that
/// differs from the drawing's group, its group bits are dropped and its other bits are capped by
/// the drawing's group bits: a member of the drawing's group must not gain access through `other`.
#[cfg(unix)]
fn backup_mode(src_mode: u32, src_gid: u32, old: Option<(u32, u32)>) -> u32 {
    let mut mode = src_mode & 0o777;
    if let Some((old_mode, old_gid)) = old {
        mode &= old_mode;
        if old_gid != src_gid {
            mode = without_group(src_mode, mode);
        }
    }
    mode
}

/// `mode` for a backup that is not in the drawing's group: no group bits, and `other` capped by the
/// drawing's group bits (`src_mode`).
#[cfg(unix)]
fn without_group(src_mode: u32, mode: u32) -> u32 {
    (mode & !0o070) & (!0o007 | ((src_mode & 0o070) >> 3))
}

/// A first backup is created in the process's (or folder's) group, not the drawing's: move it to the
/// drawing's group before any data is written, else drop its group bits ([`without_group`]).
#[cfg(unix)]
fn adopt_source_group(out: &std::fs::File, src: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, fchown};
    if out.metadata()?.gid() == src.gid() || fchown(out, None, Some(src.gid())).is_ok() {
        return Ok(());
    }
    out.set_permissions(std::fs::Permissions::from_mode(without_group(src.mode(), src.mode() & 0o777)))
}

/// Before `path` is replaced: copy it to its `.bak`, streamed and written atomically too
/// ([`crate::atomic_write::write_atomic_with`]). Nothing
/// to keep when there is no file yet. A file that doesn't look like a drawing
/// ([`looks_like_drawing`]) is not backed up, and the `.bak` already there is left alone.
#[cfg(not(target_arch = "wasm32"))]
fn backup(path: &std::path::Path) -> std::io::Result<()> {
    let mut src = match std::fs::File::open(path) {
        Ok(f) if f.metadata()?.is_file() => f,
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let (head, tail, len) = ends(&mut src)?;
    let bak = backup_path(path);
    if !backup_replaced(&head, &tail, len, bak.exists()) {
        log::warn!("save: {} doesn't look like a whole drawing; its backup is left as it was", path.display());
        return Ok(());
    }
    // The backup is no more readable than the drawing, nor than the .bak it replaces (a private
    // 0600 drawing or .bak stays private). The mode is set as the new file is created, so a
    // failed write leaves the old .bak as it was; a symlink at the .bak is replaced, never
    // written through.
    #[cfg(unix)]
    let source_metadata = src.metadata()?;
    #[cfg(unix)]
    let first_backup = std::cell::Cell::new(false);
    #[cfg(unix)]
    let permissions = || {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let old = std::fs::symlink_metadata(&bak).ok().filter(|m| m.is_file());
        first_backup.set(old.is_none());
        Ok(Some(std::fs::Permissions::from_mode(backup_mode(source_metadata.mode(), source_metadata.gid(), old.map(|m| (m.mode(), m.gid()))))))
    };
    #[cfg(not(unix))]
    let permissions = || Ok(None);
    crate::atomic_write::write_atomic_with_permissions(&bak, permissions, |out| {
        #[cfg(unix)]
        if first_backup.get() {
            adopt_source_group(out, &source_metadata)?;
        }
        std::io::Write::write_all(out, &head)?;
        std::io::copy(&mut src, out).map(|_| ())
    })
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
        r["portableFonts"] = json!(opts.portable_fonts);
    }
    Ok(r)
}

/// The version a drawing is saved in at `path` when none is asked for: its `$ACADVER` (2018 by
/// default), and for DWG at least 2004 (the io layer's rule: R2000 DWG has no true colours).
fn drawing_version(s: &Session, path: &str) -> Version {
    let v = s
        .state()
        .ok()
        .and_then(|st| st.drawing().header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(Version::for_acadver))
        .unwrap_or_default();
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
    if drops_unknown(format!("drawing.{fmt}")) {
        report_not_saved(s, &mut r, &lost);
    } else if version >= Version::R2013 && is_dxf_file(format!("drawing.{fmt}")) {
        report_sat_text_dropped(s, &mut r);
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

/// Entity types CADCraft doesn't model that the DWG bridge writes all the same: acadrust reads
/// them from our DXF into entities of its own and writes them to DWG of every version we save,
/// an OLE object with its frame and its embedded object's bytes unchanged. Matched exactly: the
/// bridge gets the type as stored, and acadrust knows only this spelling of it.
const DWG_WRITES: [&str; 1] = ["OLE2FRAME"];

/// [`not_saved`] for a file saved as `version`. From 2013 on, a 3D solid, region or body the data
/// store has a record for keeps its geometry and is not counted; neither are [`DWG_WRITES`], nor
/// another program's proxy entity whose class the file names: the DWG writes it with that class.
/// Before 2013, or when its group 91 names no class of the file, the DWG leaves the proxy out and
/// it is counted.
pub fn not_saved_as(d: &Drawing, version: Version) -> std::collections::BTreeMap<String, usize> {
    let mut out: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let stored = if version >= Version::R2013 { d.foreign.data_store_entities() } else { Default::default() };
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for e in stores.flat_map(|s| s.iter()) {
        if let cadcraft_doc::EntityKind::Unknown(u) = &e.kind {
            // A DWG before 2013 writes a solid's SAT text as it is; from 2013 the geometry has to be
            // in the data store, and SAT text alone is lost (#2334).
            if MODELER_TYPES.iter().any(|t| u.dxf_type.eq_ignore_ascii_case(t))
                && (stored.contains(&e.handle) || (version < Version::R2013 && cadcraft_doc::foreign::has_sat_text(&u.tags)))
            {
                continue;
            }
            if DWG_WRITES.contains(&u.dxf_type.as_str()) {
                continue;
            }
            if version >= Version::R2013 && u.dxf_type.eq_ignore_ascii_case("ACAD_PROXY_ENTITY") && d.foreign.proxy_class(&u.tags).is_some() {
                continue;
            }
            let n = out.entry(u.dxf_type.clone()).or_insert(0);
            *n = n.saturating_add(1);
        }
    }
    // Another program's proxy objects (in the OBJECTS section) follow the same rule as its proxy
    // entities: the DWG writer leaves them out before 2013, or when their class can't be named.
    for o in d.foreign.objects.iter().filter(|o| o.kind.eq_ignore_ascii_case("ACAD_PROXY_OBJECT")) {
        if version < Version::R2013 || d.foreign.proxy_class(&o.tags).is_none() {
            let n = out.entry("ACAD_PROXY_OBJECT".to_string()).or_insert(0);
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
    Some(format!(
        "Not saved in the DWG file (its writer can't write these object types yet; DXF keeps them, solids only when saved as 2010 or earlier): {}",
        list.join(", ")
    ))
}

/// What saving the active drawing to `path` would leave out, before anything is written: empty
/// when the format keeps everything. The interactive save asks with this; [`save_reporting`]
/// reports the same list afterwards to callers that never get a dialog.
pub fn loss_on_save(s: &Session, path: &str) -> std::collections::BTreeMap<String, usize> {
    let version = drawing_version(s, path);
    if is_dxf_file(path) && version >= Version::R2013 {
        // The drawing a file holds: the host while the Block Editor is open.
        return s.state().map(|st| cadcraft_doc::foreign::sat_text_dropped(st.drawing())).unwrap_or_default();
    }
    if !drops_unknown(path) {
        return Default::default();
    }
    s.doc().map(|d| not_saved_as(d, version)).unwrap_or_default()
}

/// Report what the file `path`, just written at the drawing's own version, left out.
pub(super) fn report_loss(s: &mut Session, r: &mut Value, path: &str) {
    if is_dxf_file(path) {
        if drawing_version(s, path) >= Version::R2013 {
            report_sat_text_dropped(s, r);
        }
    } else {
        let lost = loss_on_save(s, path);
        report_not_saved(s, r, &lost);
    }
}

/// Whether saving to `path` leaves out the entities [`not_saved`] counts: DWG does (the DWG
/// bridge drops types it doesn't know); DXF writes them back as read.
pub fn drops_unknown(path: impl AsRef<std::path::Path>) -> bool {
    Format::from_path(path).is_some_and(Format::drops_unknown)
}

/// Whether `path` is saved as a drawing (DXF, DWG) rather than exported as an image.
fn is_drawing_file(path: impl AsRef<std::path::Path>) -> bool {
    Format::from_path(path).is_some_and(Format::is_drawing)
}

/// Whether `path` is a DXF file.
fn is_dxf_file(path: impl AsRef<std::path::Path>) -> bool {
    Format::from_path(path) == Some(Format::Dxf)
}

/// Say on the command line which solid modeler entities a DXF of 2013 or later left out because
/// they hold SAT-text ACIS data, which such files keep in a data store instead (#2334).
fn report_sat_text_dropped(s: &mut Session, r: &mut Value) {
    // The drawing a file holds: the host while the Block Editor is open.
    let lost = s.state().map(|st| cadcraft_doc::foreign::sat_text_dropped(st.drawing())).unwrap_or_default();
    if lost.is_empty() {
        return;
    }
    let list: Vec<String> = lost.iter().map(|(t, n)| format!("{n} {t}")).collect();
    s.echo(format!(
        "Not saved in the DXF file (2013 and later keep ACIS geometry in a data store these entities have no record in; save as 2010 or earlier to keep them): {}",
        list.join(", ")
    ));
    if let Some(o) = r.as_object_mut() {
        o.insert("notSaved".into(), json!(lost));
    }
}

/// The folder a write to `path` has to create, asked before writing so it can be reported after.
/// The web has no folders.
pub(super) fn new_folder(path: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::atomic_write::missing_folder(std::path::Path::new(path)).map(|f| f.display().to_string());
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

/// Say on the command line that the file went into a folder that didn't exist, so a mistyped path
/// is noticed, and name the folder in the result's `createdFolder`.
pub(super) fn report_new_folder(s: &mut Session, r: &mut Value, folder: Option<String>) {
    let Some(folder) = folder else { return };
    s.echo(format!("Created the folder {folder}, which did not exist. Check the path if you did not mean to."));
    if let Some(o) = r.as_object_mut() {
        o.insert("createdFolder".into(), json!(folder));
    }
}

/// Say on the command line what the file couldn't keep, and list it in the result's `notSaved`.
pub(super) fn report_not_saved(s: &mut Session, r: &mut Value, lost: &std::collections::BTreeMap<String, usize>) {
    let Some(msg) = not_saved_message(lost) else { return };
    s.echo(msg);
    if let Some(o) = r.as_object_mut() {
        o.insert("notSaved".into(), json!(lost));
    }
}

/// Save, then report the entities the file couldn't keep (nothing is dropped silently).
fn save_reporting(s: &mut Session, p: &Value, run: fn(&mut Session, &Value) -> Result<Value>, asked: Option<Version>) -> Result<Value> {
    // What the file will hold depends on the version it is saved as (the data store from 2013).
    // QSAVE takes no `version` and keeps the drawing's own; SAVEAS writes the one asked for.
    let path = str_param(p, "path").map(str::to_string).or_else(|| s.state().ok().and_then(|st| st.path.clone())).unwrap_or_default();
    let version = asked.unwrap_or_else(|| drawing_version(s, &path));
    let lost = s.doc().map(|d| not_saved_as(d, version)).unwrap_or_default();
    let folder = new_folder(&path);
    let mut r = run(s, p)?;
    report_new_folder(s, &mut r, folder);
    if r.get("path").and_then(Value::as_str).is_some_and(drops_unknown) {
        report_not_saved(s, &mut r, &lost);
    } else if version >= Version::R2013 && r.get("path").and_then(Value::as_str).is_some_and(is_dxf_file) {
        report_sat_text_dropped(s, &mut r);
    }
    if r.get("path").and_then(Value::as_str).is_some_and(is_drawing_file) {
        report_fonts(s, &mut r);
    }
    Ok(r)
}

/// Name the text styles whose font in the saved file AutoCAD on Windows doesn't have: in the
/// result, and at the command line when the list changed since the last save. Styles from an xref
/// (`XREF|name`) aren't saved: AutoCAD takes them from the xref.
fn report_fonts(s: &mut Session, r: &mut Value) {
    use cadcraft_fonts::windows::{missing_on_windows, windows_font, written_font};
    let portable = r.get("portableFonts").and_then(Value::as_bool).unwrap_or(false);
    let Ok(d) = s.state().map(|st| st.drawing()) else { return };
    let fonts: Vec<(String, String)> = d
        .text_styles
        .iter()
        .filter(|t| !t.name.contains('|') && missing_on_windows(written_font(&t.font, portable)))
        .map(|t| (t.name.clone(), t.font.clone()))
        .collect();
    let list = fonts.iter().map(|(style, font)| format!("{style} ({font})")).collect::<Vec<_>>().join(", ");
    let changed = s.state_mut().map(|st| std::mem::replace(&mut st.fonts_noted, list.clone()) != list).unwrap_or(true);
    if fonts.is_empty() {
        return;
    }
    if changed {
        let hint = if fonts.iter().any(|(_, font)| windows_font(font).is_some()) {
            " SAVEAS with portableFonts writes Windows fonts for those that have one."
        } else {
            ""
        };
        s.echo(format!("Fonts AutoCAD on Windows doesn't have: {list}.{hint}"));
    }
    if let Some(o) = r.as_object_mut() {
        o.insert("nonPortableFonts".into(), json!(fonts.iter().map(|(style, font)| json!({ "style": style, "font": font })).collect::<Vec<_>>()));
    }
}

fn run_qsave_reporting(s: &mut Session, p: &Value) -> Result<Value> {
    save_reporting(s, p, run_save, None)
}

fn run_saveas_reporting(s: &mut Session, p: &Value) -> Result<Value> {
    let asked = save_options("saveas", p)?.version;
    save_reporting(s, p, run_saveas, asked)
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
    use crate::temp_dir::TempDir;

    /// A backup is never more open than the drawing or the old `.bak`, and loses the group bits when
    /// it would keep the old `.bak`'s group while the drawing is in another.
    #[cfg(unix)]
    #[test]
    fn backup_mode_drops_group_bits_for_a_different_group() {
        use super::backup_mode;
        assert_eq!(backup_mode(0o640, 10, None), 0o640);
        assert_eq!(backup_mode(0o640, 10, Some((0o664, 10))), 0o640);
        assert_eq!(backup_mode(0o640, 10, Some((0o664, 20))), 0o600);
        assert_eq!(backup_mode(0o644, 10, Some((0o644, 20))), 0o604);
        assert_eq!(backup_mode(0o600, 10, Some((0o664, 10))), 0o600);
    }

    /// With no `.bak` yet, the first backup takes the drawing's group (when the user is in another
    /// group, as `id -G` says), never the group the new file would be created in.
    #[cfg(unix)]
    #[test]
    fn first_backup_takes_the_drawings_group() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, chown};
        let dir = TempDir::new("first-bak-grp");
        let src = dir.join("a.dxf");
        std::fs::write(&src, "0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF\n").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o640)).unwrap();
        let own = std::fs::metadata(&src).unwrap().gid();
        let other = std::process::Command::new("id")
            .arg("-G")
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().filter_map(|g| g.parse::<u32>().ok()).find(|g| *g != own));
        if let Some(gid) = other {
            chown(&src, None, Some(gid)).unwrap();
        }
        super::backup(&src).unwrap();
        let (s, b) = (std::fs::metadata(&src).unwrap(), std::fs::metadata(super::backup_path(&src)).unwrap());
        assert_eq!((b.gid(), b.mode() & 0o777), (s.gid(), 0o640));
    }

    /// The fail-safe when the group cannot be adopted: no group bits, `other` capped by the group's.
    #[cfg(unix)]
    #[test]
    fn without_group_drops_group_bits_and_caps_other() {
        assert_eq!(super::without_group(0o640, 0o640), 0o600);
        assert_eq!(super::without_group(0o604, 0o604), 0o600);
        assert_eq!(super::without_group(0o644, 0o644), 0o604);
    }

    fn active_title(s: &Session) -> String {
        s.state().unwrap().title.clone()
    }

    #[test]
    fn script_command_runs_a_scr_file() {
        let dir = TempDir::new("script");
        let path = dir.file("run.scr", "; a comment\ncircle 50,25 20\nrectang 0,0 100,60\n");
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
        let dir = TempDir::new("script");
        let inner = dir.file("inner.scr", "circle 0,0 1\n");
        let outer = dir.file("outer.scr", format!("script {}\ncircle 0,0 2\n", json!({ "path": inner })));
        let mut s = Session::new();
        let e = s.execute("script", &json!({ "path": outer })).unwrap_err().to_string();
        assert!(e.contains("already running"), "{e}");
    }

    #[test]
    fn typed_script_prompts_for_the_file_then_runs_or_reports_it() {
        let dir = TempDir::new("script");
        let good = dir.file("typed.scr", "circle 0,0 1\n");
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
