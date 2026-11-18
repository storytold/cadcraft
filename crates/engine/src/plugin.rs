//! Plug-ins: WebAssembly modules that act on the open drawing through the same commands as the
//! menus, the command line, scripts, the CLI, the control channel and MCP, inside the sandbox of
//! the shared `craft-plugin` crate (RFC-0046). This module is CADCraft's side of it: which commands
//! a plug-in may call ([`policy`], [`refusal`]), one undo step per run (rolled back when the run
//! fails), read-only access checked after every call, each plug-in's settings (kept with the
//! app's preferences) and the installed plug-ins (the user's `Plug-ins` folder, or this session
//! only). The `plugin.*` commands are in `cmd::plugins`. See `docs/plugins.md`.
//!
//! A plug-in always runs commands in their JSON form ([`Session::execute`]): never the
//! interactive prompts, never a dialog.

mod host;
mod runtime;

use std::sync::{Arc, OnceLock};

pub use craft_plugin;
use craft_plugin::fetch::{self, Fetch};
use craft_plugin::{API_VERSION, AppInfo, Instance, KIND_COMMAND, Limits, Manifest, Policy, RunOutput, Source, UiChange, bundle, catalog, github};
use serde_json::{Map, Value, json};

use crate::cmd::bad;
use crate::{DocState, EngineError, Result, Session, Settings, Snapshot};
use host::EngineHost;
pub use runtime::{InstalledPlugin, PluginPrefs, Runtime};

/// CADCraft as manifests name it: command plug-ins of every shared ABI version, and only
/// manifests that name `cadcraft` in `apps`.
pub const APP: AppInfo<'static> =
    AppInfo { id: "cadcraft", version: env!("CARGO_PKG_VERSION"), api: 1..=API_VERSION, kinds: &[KIND_COMMAND], accept_legacy: false, kind_api: &[] };

/// What one plug-in may keep in its settings (keys and JSON values, in bytes), and its longest key.
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024;
pub const MAX_SETTING_KEY: usize = 128;
/// Catalogs a person may add.
pub const MAX_CATALOGS: usize = 32;
/// Messages a run may show (`notify`), and their length in characters.
const MAX_NOTES: usize = 20;
const MAX_NOTE_CHARS: usize = 1000;
/// Least instruction budget `plugin.run {fuel}` may ask for.
const MIN_FUEL: u64 = 1000;

/// CADCraft's commands no plug-in may call, on top of [`craft_plugin::ALWAYS_DENIED`] (whose
/// `file.*`, `app.*`, `edit.undo`… are other apps' names: CADCraft's commands are AutoCAD's). A
/// plug-in works on the open drawing only: it gets no file or folder, no other drawing, no undo
/// history, no clipboard, no clock, no user profile and no code of another kind (LISP, scripts).
const DENIED: &[&str] = &[
    // The undo history (U, UNDO with its marks and groups, REDO, MREDO, OOPS).
    "u",
    "undo",
    "redo",
    "mredo",
    "oops",
    // Other drawings, and files: opening, saving, recovering, importing and exporting, plotting.
    "new",
    "open",
    "close",
    "closeall",
    "qsave",
    "saveas",
    "recover",
    "drawingrecovery",
    "document.*",
    "import",
    "export",
    "exportpdf",
    "plot",
    "preview",
    "plotwindow",
    "publish",
    "pdfbook",
    "plottermanager",
    "-plottermanager",
    "stylesmanager",
    "layout.saveas",
    "wblock",
    "-wblock",
    "datalink",
    "datalinkupdate",
    "dataextraction",
    "eattext",
    "tableexport",
    "compare*",
    // References to other files: attaching, opening and binding them, and editing them in place.
    "attach",
    "imageattach",
    "xattach",
    "xref",
    "-xref",
    "xopen",
    "xbind",
    "refedit",
    "refclose",
    "refset",
    // The Block Editor swaps the drawing for the block's while it is open.
    "bedit",
    "bclose",
    "bsave",
    "bsaveas",
    // Code of another kind, which would run commands past these checks.
    "lisp",
    "appload",
    "script",
    // The user profile and the session's own libraries, kept outside the drawing.
    "options",
    "aliasedit",
    "reinit",
    "hatch.patterns",
    // The clipboard belongs to the session and may hold another drawing's objects.
    "copyclip",
    "cutclip",
    "copybase",
    "cutbase",
    "pasteclip",
    "pasteorig",
    "pasteblock",
    // The clock.
    "time",
];

/// Parameter names that name a file or folder on disk, at any depth.
const FILE_KEYS: &[&str] = &["path", "paths", "file", "files", "dir", "folder", "template", "patternFile"];
/// Commands whose top-level `path` is an object handle (the path of a path array), not a file.
const PATH_IS_A_HANDLE: &[&str] = &["array", "arraypath"];

/// The rules every plug-in run follows: [`craft_plugin::ALWAYS_DENIED`] and [`DENIED`].
pub fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| Policy::new(DENIED, &[]))
}

/// Why plug-in `m` may not call `command` (an id in lower case) with `params`, before it runs: a
/// refused command, one outside its manifest's `permissions.commands`, or parameters that name a
/// file.
pub fn refusal(m: &Manifest, command: &str, params: &Value) -> Option<String> {
    if let Err(why) = craft_plugin::check_call(m, policy(), command) {
        return Some(why);
    }
    if names_a_file(command, params) {
        return Some(format!(
            "plug-ins can't name files (`{command}` was given a {}): use what is already in the drawing",
            FILE_KEYS.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    None
}

/// Whether `params` hold one of [`FILE_KEYS`] at any depth (but the handle `path` of a path array).
fn names_a_file(command: &str, params: &Value) -> bool {
    let Value::Object(top) = params else { return file_key_inside(params, 0) };
    top.iter().any(|(k, v)| {
        let handle = k == "path" && PATH_IS_A_HANDLE.contains(&command) && (v.is_string() || v.is_number());
        (!handle && FILE_KEYS.contains(&k.as_str())) || file_key_inside(v, 1)
    })
}

fn file_key_inside(v: &Value, depth: usize) -> bool {
    // serde_json refuses nesting past 128, so nothing deeper reaches here
    depth < 128
        && match v {
            Value::Object(m) => m.iter().any(|(k, v)| FILE_KEYS.contains(&k.as_str()) || file_key_inside(v, depth + 1)),
            Value::Array(a) => a.iter().any(|v| file_key_inside(v, depth + 1)),
            _ => false,
        }
}

/// Run `f` with the session's HTTPS client (or [`NoNetwork`]).
pub(crate) fn with_fetch<T>(s: &Session, f: impl FnOnce(&mut dyn Fetch) -> T) -> T {
    let mut client = s.plugins.fetch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match client.as_deref_mut() {
        Some(http) => f(http),
        None => f(&mut NoNetwork),
    }
}

/// Longest part of a URL an error repeats.
const SHOWN_URL_CHARS: usize = 120;

/// The fetcher of sessions without an HTTPS client: `file://` catalogs still read.
struct NoNetwork;

impl Fetch for NoNetwork {
    fn get(&mut self, url: &str, _max: usize) -> std::result::Result<Option<Vec<u8>>, String> {
        let shown: String = url.chars().take(SHOWN_URL_CHARS).collect();
        Err(format!("there is no network access here to read {shown}: install the plug-in from a file, or in the CADCraft app"))
    }
}

/// A plug-in error as the engine reports it.
fn failed(name: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{name}: {e}"))
}

/// Store (`Some`) or forget (`None`) plug-in `id`'s setting `key` in `all`, within
/// [`MAX_SETTINGS_BYTES`] and [`MAX_SETTING_KEY`].
fn set_setting(
    all: &mut std::collections::BTreeMap<String, Map<String, Value>>,
    id: &str,
    key: &str,
    value: Option<&str>,
) -> std::result::Result<(), String> {
    let Some(text) = value else {
        if let Some(mine) = all.get_mut(id) {
            mine.remove(key);
            if mine.is_empty() {
                all.remove(id);
            }
        }
        return Ok(());
    };
    if key.is_empty() || key.chars().count() > MAX_SETTING_KEY || key.chars().any(char::is_control) {
        return Err(format!("a setting's key must be 1–{MAX_SETTING_KEY} characters without control characters"));
    }
    let too_big = || format!("a plug-in's settings are limited to {} KB", MAX_SETTINGS_BYTES / 1024);
    if key.len().saturating_add(text.len()) > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("a setting must be JSON: {e}"))?;
    let others: usize = all.get(id).map_or(0, |mine| {
        mine.iter().filter(|(k, _)| k.as_str() != key).map(|(k, v)| k.len().saturating_add(v.to_string().len())).fold(0, usize::saturating_add)
    });
    if others.saturating_add(key.len()).saturating_add(v.to_string().len()) > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    all.entry(id.to_string()).or_default().insert(key.to_string(), v);
    Ok(())
}

// ---------- running ----------

/// The selected objects' handles, as a plug-in sees the selection.
fn selection_json(s: &Session) -> Value {
    json!(s.selection().iter().map(|h| h.hex()).collect::<Vec<_>>())
}

/// What a run puts back when it fails: the drawing as it was (with its views, selection and undo
/// history) and the drafting settings.
struct Before {
    uid: u64,
    doc: DocState,
    settings: Settings,
}

impl Before {
    fn take(s: &Session) -> Result<Before> {
        let doc = s.state()?.clone();
        Ok(Before { uid: doc.uid, doc, settings: s.settings.clone() })
    }

    /// Put the drawing and the settings back, making the drawing the active one again.
    fn restore(self, s: &mut Session) {
        s.settings = self.settings;
        let Some(i) = s.docs.iter().position(|d| d.uid == self.uid) else { return };
        if s.active != i {
            s.active = i;
            s.doc_epoch = s.doc_epoch.wrapping_add(1);
        }
        if let Some(st) = s.docs.get_mut(i) {
            // Redraw: the revision only ever grows, so caches keyed by it are invalidated
            let revision = st.revision.max(self.doc.revision).saturating_add(1);
            *st = self.doc;
            st.revision = revision;
        }
    }
}

/// Whether the drafting settings are still `before` (Settings has no `PartialEq`; its JSON is
/// what the profile keeps).
fn same_settings(a: &Settings, b: &Settings) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

/// Run plug-in `id`'s `command` (default: its first) with `params` on the open drawing, as one
/// undo step named after the plug-in. A run that fails (a trap, an exhausted budget, a refusal it
/// passes on, a non-zero status) leaves the drawing, its selection, views and undo history and
/// the drafting settings as they were.
pub fn run(s: &mut Session, id: &str, command: Option<&str>, params: &Value, fuel: Option<u64>) -> Result<Value> {
    let cmd = "plugin.run";
    // Inside a routine its commands are one undo step of the routine's, which this one can't keep
    if s.lisp_groups_undo() {
        return Err(bad(cmd, "plug-ins can't run inside a LISP routine"));
    }
    let p = s.plugins.get(id).cloned().ok_or_else(|| bad(cmd, format!("no plug-in `{id}` is installed (see plugin.list)")))?;
    let m = &p.manifest;
    let api = m.target(&APP).map_err(|e| bad(cmd, format!("{} can't run here: {e}", m.name)))?.api();
    let c = m.command(command).ok_or_else(|| bad(cmd, format!("{} has no command `{}`", m.name, command.unwrap_or_default())))?;
    let params = c.resolve_params(params).map_err(|e| bad(cmd, e))?;
    let mut limits = Limits::default();
    if let Some(f) = fuel {
        limits.fuel = f.clamp(MIN_FUEL, limits.fuel);
    }
    let mut instance = Instance::new(&p.wasm, &limits).map_err(|e| failed(&m.name, e))?;

    let before = Before::take(s)?;
    let input = json!({"api": api, "command": c.id, "params": params, "selection": selection_json(s)}).to_string();
    s.plugins.last = Some((m.id.clone(), c.id.clone(), params));
    let label = if m.commands.len() > 1 { format!("{}: {}", m.name, c.label.trim_end_matches(['…', '.'])) } else { m.name.clone() };

    // The run's commands record their steps in an empty history, with every step kept; the run
    // becomes one step of the drawing's own history below.
    if let Ok(st) = s.state_mut() {
        st.undo.clear();
        st.redo.clear();
        st.undo_ctl = Default::default();
    }
    let mut host = EngineHost::new(s, m, before.uid);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| instance.call(&input, &mut host)));
    let (notes, strayed) = host.finish();
    let ran: std::result::Result<RunOutput, String> = match ran {
        Ok(Ok(_)) if strayed => Err("the run left the drawing it started in".into()),
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(e.to_string()),
        Err(p) => Err(format!("internal error: {}", crate::guard::message(&*p))),
    };
    let out = match ran {
        Ok(out) => out,
        Err(e) => {
            before.restore(s);
            return Err(failed(&m.name, e));
        }
    };

    let Before { doc: was, .. } = before;
    if let Ok(st) = s.state_mut() {
        let changed = !Arc::ptr_eq(&was.doc, &st.doc);
        st.undo = was.undo;
        st.redo = was.redo;
        st.undo_ctl = was.undo_ctl;
        if changed {
            st.push_undo(Snapshot { label, doc: was.doc, selection: was.selection });
        }
    }

    let output = serde_json::from_str::<Value>(&out.output).unwrap_or(Value::String(out.output));
    // Panels (`ui_show`) aren't shown in CADCraft yet: say so rather than drop it silently
    let panel = matches!(out.ui, Some(UiChange::Show(_))).then_some("CADCraft doesn't show plug-in panels yet");
    Ok(
        json!({"plugin": m.id, "command": c.id, "output": output, "notes": notes, "logs": out.logs, "calls": out.calls, "fuel": out.fuel, "panel": panel}),
    )
}

// ---------- installing ----------

/// A bundle obtained for installing: its bytes, where it came from and, for a repository, whether
/// a catalog in use lists it.
pub(crate) struct Candidate {
    pub bytes: Vec<u8>,
    pub source: Source,
    /// `Some(true)` from a catalog; for a repository, whether a catalog in use lists it.
    pub in_catalog: Option<bool>,
}

/// A bundle checked and shown for review (`plugin.install {review: true}`), kept so the install
/// that follows doesn't download it again.
pub(crate) struct Pending {
    pub sha256: String,
    /// The `plugin.install` parameters that named it.
    pub request: Value,
    pub candidate: Candidate,
}

/// Read the bundle `p` names: `path`, `data` (base64), `repo` (`owner/repo[@tag]`) or `catalog` +
/// `id` (and optionally `version`).
pub(crate) fn obtain(s: &Session, cmd: &str, p: &Value) -> Result<Candidate> {
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
    if let Some(path) = text("path") {
        let bytes = read_bundle_file(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
        return Ok(Candidate { bytes, source: Source::File { path: path.into() }, in_catalog: None });
    }

    if let Some(data) = text("data") {
        if data.len() > bundle::MAX_BUNDLE_BYTES / 3 * 4 + 4 {
            return Err(bad(cmd, format!("the bundle is larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20)));
        }
        let bytes = crate::cmd::file::base64_decode(data).ok_or_else(|| bad(cmd, "`data` isn't base64"))?;
        let name = text("name").unwrap_or("data");
        return Ok(Candidate { bytes, source: Source::File { path: name.into() }, in_catalog: None });
    }

    if let Some(spec) = text("repo") {
        let repo = github::parse_repo_spec(spec).map_err(|e| bad(cmd, e))?;
        let (plan, bytes) = with_fetch(s, |f| {
            let plan = fetch::resolve_repo(f, &repo)?;
            let bytes = fetch::download_repo(f, &plan)?;
            Ok::<_, String>((plan, bytes))
        })
        .map_err(|e| bad(cmd, e))?;
        let source = Source::Github { repo: plan.repo.slug(), tag: plan.repo.tag.clone().unwrap_or_default(), asset: plan.asset.clone() };
        let catalogs = s.plugins.prefs.catalogs.clone();
        let in_catalog = with_fetch(s, |f| {
            catalogs.iter().any(|url| {
                fetch::fetch_catalog(f, url)
                    .is_ok_and(|(cat, _)| cat.packages.iter().any(|pkg| pkg.repo_ref().is_some_and(|r| r.same_repo(&repo.owner, &repo.name))))
            })
        });
        return Ok(Candidate { bytes, source, in_catalog: Some(in_catalog) });
    }

    if let Some(url) = text("catalog") {
        let id = text("id").ok_or_else(|| bad(cmd, "`id` required with `catalog` (a package of the catalog)"))?;
        let version = text("version");
        let bytes = with_fetch(s, |f| {
            let (cat, _) = fetch::fetch_catalog(f, url)?;
            let package = cat.package(id).ok_or_else(|| format!("{url} has no package `{id}`"))?;
            let release = match version {
                Some(v) => package.release(v).ok_or_else(|| format!("{id} has no version {v} in {url}"))?,
                None => package.latest_for(&APP).ok_or_else(|| format!("{id} has no release for CADCraft {}", APP.version))?,
            };
            release.target(&package.kind, &APP)?;
            let bytes = fetch::download_release(f, url, package, release)?;
            let (_, manifest) = bundle::open(&bytes)?;
            package.check_manifest(release, &manifest)?;
            Ok::<_, String>(bytes)
        })
        .map_err(|e| bad(cmd, e))?;
        return Ok(Candidate { bytes, source: Source::Catalog { catalog: url.into(), package: id.into() }, in_catalog: Some(true) });
    }

    Err(bad(cmd, "pass `path`, `data` (base64), `repo` (owner/repo or owner/repo@tag) or `catalog` with `id`"))
}

/// The bundle file at `path`, refused past [`bundle::MAX_BUNDLE_BYTES`] before it is read whole.
#[cfg(not(target_arch = "wasm32"))]
fn read_bundle_file(path: &str) -> std::result::Result<Vec<u8>, String> {
    crate::capped_read::read_capped(std::path::Path::new(path), bundle::MAX_BUNDLE_BYTES).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn read_bundle_file(_path: &str) -> std::result::Result<Vec<u8>, String> {
    Err("the browser has no file paths: pass the bundle as `data` (base64)".into())
}

/// What an install confirmation shows about `c`: the manifest's permissions, kind and commands,
/// the source, the SHA-256, whether a catalog lists it and what it replaces. An error when the
/// bundle isn't a plug-in CADCraft can run.
pub(crate) fn review(s: &Session, cmd: &str, c: &Candidate) -> Result<(Manifest, Value)> {
    let (_, manifest) = bundle::open(&c.bytes).map_err(|e| bad(cmd, e))?;
    manifest.target(&APP).map_err(|e| bad(cmd, format!("{} can't be installed: {e}", manifest.name)))?;
    c.source.check().map_err(|e| bad(cmd, e))?;
    let old = s.plugins.get(&manifest.id);
    let mut v = serde_json::to_value(&manifest).unwrap_or_else(|_| json!({"id": manifest.id}));
    v["kind"] = json!(manifest.kind);
    v["sha256"] = json!(bundle::sha256_hex(&c.bytes));
    v["source"] = serde_json::to_value(&c.source).unwrap_or(Value::Null);
    v["inCatalog"] = json!(c.in_catalog);
    v["replaces"] = json!(old.map(|o| &o.manifest.version));
    v["permissionsChanged"] = json!(old.is_some_and(|o| craft_plugin::permissions::changed(&o.manifest, &manifest)));
    v["stored"] = json!(s.plugins.has_store());
    Ok((manifest, v))
}

/// Install `c` (already reviewed): into the user's folder when the session has one, else for the
/// session. Replaces an installed plug-in with the same id; its settings stay.
pub(crate) fn install(s: &mut Session, cmd: &str, c: Candidate) -> Result<InstalledPlugin> {
    let (manifest, _) = review(s, cmd, &c)?;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(store) = &s.plugins.store {
        let (i, _) = store.install_for(&APP, &c.bytes, c.source).map_err(|e| bad(cmd, e))?;
        let plugin =
            InstalledPlugin { manifest: i.manifest, sha256: i.record.sha256, source: i.record.source, stored: true, wasm: Arc::new(i.bundle.wasm) };
        return Ok(s.plugins.keep(plugin));
    }

    let sha256 = bundle::sha256_hex(&c.bytes);
    let (b, _) = bundle::open(&c.bytes).map_err(|e| bad(cmd, e))?;
    Ok(s.plugins.keep(InstalledPlugin { manifest, sha256, source: c.source, stored: false, wasm: Arc::new(b.wasm) }))
}

/// Keep `c`, named by the install parameters `request`, for the install that follows its review
/// (one at a time).
pub(crate) fn hold(s: &mut Session, sha256: &str, request: Value, c: Candidate) {
    s.plugins.pending = Some(Pending { sha256: sha256.into(), request, candidate: c });
}

/// The bundle reviewed last, when it is `sha256` and `request` named it.
pub(crate) fn take_held(s: &mut Session, sha256: &str, request: &Value) -> Option<Candidate> {
    let held = s.plugins.pending.take_if(|h| h.sha256.eq_ignore_ascii_case(sha256) && h.request == *request)?;
    Some(held.candidate)
}

/// Remove plug-in `id` and its settings; whether it was installed.
pub(crate) fn remove(s: &mut Session, cmd: &str, id: &str) -> Result<bool> {
    let Some(p) = s.plugins.get(id).cloned() else { return Ok(false) };
    #[cfg(not(target_arch = "wasm32"))]
    if p.stored
        && let Some(store) = &s.plugins.store
    {
        store.remove(&p.manifest.id).map_err(|e| bad(cmd, e))?;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = cmd;
    s.plugins.forget(&p.manifest.id);
    Ok(true)
}

/// The catalog `url`'s packages CADCraft can run, with their newest release for it.
pub(crate) fn catalog_packages(s: &Session, url: &str) -> std::result::Result<(Vec<Value>, Vec<String>), String> {
    let (cat, warnings) = with_fetch(s, |f| fetch::fetch_catalog(f, url))?;
    let packages = cat
        .packages
        .iter()
        .filter_map(|pkg| {
            let release = pkg.latest_for(&APP)?;
            let installed = s.plugins.get(&pkg.id).map(|p| p.manifest.version.clone());
            Some(json!({
                "catalog": url, "id": pkg.id, "name": pkg.name, "version": release.version, "author": pkg.author,
                "description": pkg.description, "license": pkg.license, "repo": pkg.repo, "kind": pkg.kind,
                "sha256": release.sha256, "installed": installed,
            }))
        })
        .collect();
    Ok((packages, warnings))
}

/// Whether `url` is a catalog address CADCraft reads (`https://` on GitHub's hosts, or
/// `file://`), or why not.
pub(crate) fn check_catalog_url(url: &str) -> std::result::Result<(), String> {
    catalog::check_url(url, None).map(|_| ())
}

/// Where plug-in `p` has a newer release: the install parameters that get it and its version (a
/// tag, for a repository). `None` when it is up to date or came from a file.
pub(crate) fn update_for(s: &Session, p: &InstalledPlugin) -> std::result::Result<Option<(Value, String)>, String> {
    match &p.source {
        Source::Catalog { catalog: url, package: id } => {
            let (cat, _) = with_fetch(s, |f| fetch::fetch_catalog(f, url))?;
            let release = cat.package(id).and_then(|pkg| pkg.latest_for(&APP));
            Ok(release
                .filter(|r| bundle::newer(&r.version, &p.manifest.version))
                .map(|r| (json!({"catalog": url, "id": id, "version": r.version}), r.version.clone())))
        }
        Source::Github { repo, tag: installed, .. } => {
            let mut repo = github::parse_repo_spec(repo)?;
            repo.tag = None;
            let plan = with_fetch(s, |f| fetch::resolve_repo(f, &repo))?;
            let tag = plan.repo.tag.clone().unwrap_or_default();
            Ok((tag != *installed).then(|| (json!({"repo": format!("{}@{tag}", plan.repo.slug())}), tag)))
        }
        Source::File { .. } | Source::Other { .. } => Ok(None),
    }
}

#[cfg(test)]
mod tests;
