//! Loading external references: finding the referenced drawing, reading it through the host's io
//! hooks, and bringing its model space into the xref's block definition with its symbols renamed
//! `xref|name`.
//!
//! A path is looked for as given (relative paths against the host drawing's folder), then by its
//! file name in the host drawing's folder. A path stored in a drawing is followed only to this
//! computer or the host drawing's network share ([`crate::doc_path`]); one that isn't followed and
//! has no file of its name beside the host leaves the xref unresolved. A referenced drawing's own
//! attached xrefs load with it (overlays don't), at most [`MAX_BLOCK_DEPTH`] levels deep; a drawing that is already being
//! loaded further up (a circular reference) is not loaded again. Every failure leaves the xref's
//! block empty with a status saying why; nothing here stops the host drawing from opening.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_doc::{Attrib, Drawing, Entity, EntityKind, EntityStore, MAX_BLOCK_DEPTH, XrefStatus};

use crate::doc_path::{self, Refused};

/// Most objects the xrefs of one drawing load in one go (nested xrefs included).
pub(crate) const MAX_XREF_OBJECTS: usize = 4_000_000;
/// Most referenced files read in one go (nested xrefs included).
const MAX_XREF_FILES: usize = 1000;
/// Largest referenced file read.
#[cfg(not(target_arch = "wasm32"))]
const MAX_XREF_BYTES: u64 = 512 << 20;
/// Most bytes read from referenced files in one go (nested xrefs included).
#[cfg(not(target_arch = "wasm32"))]
const MAX_XREF_TOTAL_BYTES: u64 = 1 << 30;

/// The kinds of named symbols an object can refer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Sym {
    Layer,
    Linetype,
    Style,
    DimStyle,
    Block,
    MLeaderStyle,
    TableStyle,
}

/// Names no xref brings: layer 0 and the built-in linetypes stay the host's.
fn shared(sym: Sym, name: &str) -> bool {
    name.is_empty()
        || match sym {
            Sym::Layer => name == "0",
            Sym::Linetype => ["ByLayer", "ByBlock", "Continuous"].iter().any(|n| n.eq_ignore_ascii_case(name)),
            _ => false,
        }
}

/// Whether `name` starts with `prefix`, ignoring ASCII case.
pub(crate) fn has_prefix(name: &str, prefix: &str) -> bool {
    name.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

/// Rename the symbols entity `e` refers to (`f` returns the new name, or `None` to keep it).
pub(crate) fn rename_refs(e: &mut Entity, f: &dyn Fn(Sym, &str) -> Option<String>) {
    let fix = |sym: Sym, n: &mut String| {
        if let Some(to) = f(sym, n) {
            *n = to;
        }
    };
    fix(Sym::Layer, &mut e.common.layer);
    fix(Sym::Linetype, &mut e.common.linetype);
    let attrib = |a: &mut Attrib| {
        fix(Sym::Style, &mut a.text.style);
        if let Some(l) = &mut a.props.layer {
            fix(Sym::Layer, l);
        }
        if let Some(l) = &mut a.props.linetype {
            fix(Sym::Linetype, l);
        }
    };
    match &mut e.kind {
        EntityKind::Text(t) => fix(Sym::Style, &mut t.style),
        EntityKind::MText(m) => fix(Sym::Style, &mut m.style),
        EntityKind::AttDef(a) => attrib(a),
        EntityKind::Insert(i) => {
            fix(Sym::Block, &mut i.block);
            i.attribs.iter_mut().for_each(attrib);
        }
        EntityKind::Dimension(dm) => {
            fix(Sym::DimStyle, &mut dm.style);
            if let Some(b) = &mut dm.block {
                fix(Sym::Block, b);
            }
        }
        EntityKind::Leader(l) => fix(Sym::DimStyle, &mut l.style),
        EntityKind::MLeader(m) => {
            fix(Sym::MLeaderStyle, &mut m.style);
            if let Some(t) = &mut m.text {
                fix(Sym::Style, &mut t.style);
            }
            if let Some(b) = &mut m.block {
                fix(Sym::Block, &mut b.block);
            }
        }
        EntityKind::Table(t) => fix(Sym::TableStyle, &mut t.style),
        _ => {}
    }
}

/// The outcome of loading one xref, for the command line.
#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub name: String,
    pub status: XrefStatus,
    /// Why it isn't loaded, or the file it was loaded from.
    pub detail: String,
}

impl Outcome {
    pub fn message(&self) -> String {
        match self.status {
            XrefStatus::Loaded => format!("\"{}\" loaded from {}.", self.name, self.detail),
            _ => format!("Xref \"{}\" not loaded: {}.", self.name, self.detail),
        }
    }
}

/// What one load (with its nested xrefs) has used so far.
struct Ctx {
    /// The files being loaded, outermost first (a file already here is a circular reference).
    chain: Vec<PathBuf>,
    objects: usize,
    files: usize,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    bytes: u64,
    /// Messages about nested xrefs.
    notes: Vec<String>,
}

/// The folder of the host drawing at `host` (a file path).
fn folder(host: Option<&Path>) -> Option<PathBuf> {
    let p = host?.parent()?;
    Some(if p.as_os_str().is_empty() { PathBuf::from(".") } else { p.to_path_buf() })
}

/// Why a referenced drawing wasn't found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Missing {
    NotFound,
    /// The saved path isn't followed, and there is no file of its name beside the host.
    Refused(Refused),
}

impl Missing {
    fn outcome(self, path: &str) -> (XrefStatus, String) {
        match self {
            Self::NotFound => {
                let why = if cfg!(target_arch = "wasm32") { "files are not available on the web" } else { "file not found" };
                (XrefStatus::NotFound, format!("{path}: {why}"))
            }
            Self::Refused(r) => (XrefStatus::Unresolved, format!("{path}: not opened, {r}")),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}
#[cfg(target_arch = "wasm32")]
fn canonical(p: &Path) -> PathBuf {
    p.to_path_buf()
}

/// Where to look for a referenced drawing at `path` in the drawing at `host`: the path itself (a
/// relative one against the host's folder), then its file name in the host's folder. Both
/// separators work everywhere. A path stored in a drawing goes through [`doc_path::resolve`]
/// (the refusal is returned); a path the user just `chosen` is used as given.
fn candidates(path: &str, host: Option<&Path>, chosen: bool) -> (Vec<PathBuf>, Option<Refused>) {
    let path = path.trim();
    let mut out = Vec::new();
    let mut refused = None;
    if chosen {
        let norm = if cfg!(windows) { path.to_string() } else { path.replace('\\', "/") };
        let p = PathBuf::from(norm);
        match folder(host) {
            Some(d) if !p.is_absolute() => out.push(d.join(&p)),
            _ => out.push(p),
        }
    } else {
        match doc_path::resolve(path, host) {
            Ok(p) => out.push(p),
            Err(Refused::Invalid) => {}
            Err(r) => refused = Some(r),
        }
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if host.is_some()
        && !name.is_empty()
        && let Ok(p) = doc_path::resolve(name, host)
        && !out.contains(&p)
    {
        out.push(p);
    }
    (out, refused)
}

/// The file a referenced drawing at `path` (saved in the drawing at `host`, or just `chosen` by
/// the user) is loaded from.
pub(crate) fn find(path: &str, host: Option<&Path>, chosen: bool) -> Result<PathBuf, Missing> {
    if path.trim().is_empty() {
        return Err(Missing::NotFound);
    }
    let (files, refused) = candidates(path, host, chosen);
    #[cfg(not(target_arch = "wasm32"))]
    let found = files.into_iter().find(|p| p.is_file());
    #[cfg(target_arch = "wasm32")]
    let found: Option<PathBuf> = {
        let _ = files;
        None
    };
    found.ok_or(refused.map_or(Missing::NotFound, Missing::Refused))
}

/// Read a drawing file through the host's io hooks, counting its bytes against the load's total.
fn read(file: &Path, cx: &mut Ctx) -> Result<Drawing, String> {
    let hooks = super::super::super::file::io().ok_or("file formats are not available in this build")?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let left = MAX_XREF_TOTAL_BYTES.saturating_sub(cx.bytes);
        let limit = MAX_XREF_BYTES.min(left);
        let mut f = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        let too_big = || {
            if limit < MAX_XREF_BYTES {
                format!("the referenced files are larger than {} MB together", MAX_XREF_TOTAL_BYTES >> 20)
            } else {
                format!("the file is larger than {} MB", MAX_XREF_BYTES >> 20)
            }
        };
        if len > limit {
            return Err(too_big());
        }
        let mut bytes = Vec::new();
        f.by_ref().take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let n = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if n > limit {
            return Err(too_big());
        }
        cx.bytes = cx.bytes.saturating_add(n);
        (hooks.read)(&bytes, &file.to_string_lossy())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (hooks, file, cx);
        Err("files are not available on the web".into())
    }
}

/// Load every xref of `d` that isn't unloaded from the paths saved in it; `host` is the drawing's
/// own path.
pub(crate) fn load_all(d: &mut Drawing, host: Option<&str>) -> Vec<Outcome> {
    let names: Vec<String> = d
        .blocks
        .values()
        .filter(|b| b.xref_path.is_some() && b.xref_status != XrefStatus::Unloaded && !b.name.contains('|'))
        .map(|b| b.name.clone())
        .collect();
    load_each(d, &names, host, false)
}

/// Load (or reload) xref `name` of `d` from its file; `host` is the drawing's own path, and
/// `chosen` says the xref's path was just given by the user (not read from a drawing). Returns the
/// outcome, then notes about its nested xrefs.
pub(crate) fn load(d: &mut Drawing, name: &str, host: Option<&str>, chosen: bool) -> Vec<Outcome> {
    load_each(d, std::slice::from_ref(&name.to_string()), host, chosen)
}

/// [`load`] the xrefs `names` within one set of limits.
pub(crate) fn load_each(d: &mut Drawing, names: &[String], host: Option<&str>, chosen: bool) -> Vec<Outcome> {
    let mut cx = Ctx::new(host);
    names.iter().flat_map(|n| load_with(d, n, host, chosen, &mut cx)).collect()
}

impl Ctx {
    fn new(host: Option<&str>) -> Self {
        let host_file = host.map(Path::new).filter(|p| p.file_name().is_some());
        Ctx { chain: host_file.map(canonical).into_iter().collect(), objects: 0, files: 0, bytes: 0, notes: Vec::new() }
    }
}

fn load_with(d: &mut Drawing, name: &str, host: Option<&str>, chosen: bool, cx: &mut Ctx) -> Vec<Outcome> {
    let host = host.map(Path::new).filter(|p| p.file_name().is_some());
    let (status, detail) = load_in(d, name, host, chosen, cx, 0);
    let key = d.block(name).map(|b| b.name.clone()).unwrap_or_else(|| name.to_string());
    let mut out = vec![Outcome { name: key, status, detail }];
    out.extend(cx.notes.drain(..).map(|n| Outcome { name: String::new(), status: XrefStatus::Unresolved, detail: n }));
    out
}

fn load_in(d: &mut Drawing, name: &str, host: Option<&Path>, chosen: bool, cx: &mut Ctx, depth: usize) -> (XrefStatus, String) {
    let Some(b) = d.block(name) else { return (XrefStatus::NotFound, "no such xref".into()) };
    let key = b.name.clone();
    let Some(path) = b.xref_path.clone() else { return (XrefStatus::Unresolved, "not an xref".into()) };
    clear(d, &key);
    let (status, detail) = match fetch(&path, host, chosen, cx, depth) {
        Ok((x, file)) => match import(d, &key, &x, cx) {
            Ok(()) => (XrefStatus::Loaded, file.to_string_lossy().into_owned()),
            Err(e) => (XrefStatus::Unresolved, e),
        },
        Err(e) => e,
    };
    if status != XrefStatus::Loaded {
        // Whatever made it in before the failure goes again.
        clear(d, &key);
    }
    set_status(d, &key, status);
    (status, detail)
}

/// Read the drawing referenced as `path` and load its own attached xrefs into it.
fn fetch(path: &str, host: Option<&Path>, chosen: bool, cx: &mut Ctx, depth: usize) -> Result<(Drawing, PathBuf), (XrefStatus, String)> {
    let unresolved = |m: &str| (XrefStatus::Unresolved, m.to_string());
    if depth >= MAX_BLOCK_DEPTH {
        return Err(unresolved("xrefs are nested too deeply"));
    }
    let file = find(path, host, chosen).map_err(|m| m.outcome(path))?;
    let canon = canonical(&file);
    if cx.chain.contains(&canon) {
        return Err(unresolved("circular reference"));
    }
    if cx.files >= MAX_XREF_FILES {
        return Err(unresolved("too many referenced files"));
    }
    cx.files += 1;
    let mut x = read(&file, cx).map_err(|e| (XrefStatus::Unresolved, format!("{}: {e}", file.to_string_lossy())))?;
    cx.chain.push(canon);
    let nested: Vec<String> =
        x.blocks.values().filter(|b| b.xref_path.is_some() && !b.xref_overlay && !b.name.contains('|')).map(|b| b.name.clone()).collect();
    for n in nested {
        let (status, detail) = load_in(&mut x, &n, Some(&file), false, cx, depth + 1);
        if status != XrefStatus::Loaded {
            cx.notes.push(format!("nested xref \"{n}\" not loaded: {detail}"));
        }
    }
    cx.chain.pop();
    Ok((x, file))
}

/// Bring referenced drawing `x` into xref `key`: its model space becomes the block's objects, its
/// symbols and blocks are added as `key|name` (dependent symbols the host already has keep the
/// host's settings, so layer changes survive a reload).
fn import(d: &mut Drawing, key: &str, x: &Drawing, cx: &mut Ctx) -> Result<(), String> {
    let count = x.blocks.values().fold(x.model.len(), |n, b| n.saturating_add(b.entities.len()));
    cx.objects = cx.objects.saturating_add(count);
    if cx.objects > MAX_XREF_OBJECTS {
        return Err(format!("more than {MAX_XREF_OBJECTS} objects"));
    }
    let dep = |sym: Sym, n: &str| (!shared(sym, n)).then(|| format!("{key}|{n}"));
    for l in &x.layers {
        if let Some(n) = dep(Sym::Layer, &l.name)
            && d.layer(&n).is_none()
        {
            let mut l = l.clone();
            l.name = n;
            l.linetype = dep(Sym::Linetype, &l.linetype).unwrap_or(l.linetype);
            d.layers.push(l);
        }
    }
    for t in &x.linetypes {
        if let Some(n) = dep(Sym::Linetype, &t.name)
            && d.linetype(&n).is_none()
        {
            d.linetypes.push(cadcraft_doc::Linetype { name: n, ..t.clone() });
        }
    }
    for t in &x.text_styles {
        if let Some(n) = dep(Sym::Style, &t.name)
            && d.text_style(&n).is_none()
        {
            d.text_styles.push(cadcraft_doc::TextStyle { name: n, ..t.clone() });
        }
    }
    for t in &x.dim_styles {
        if let Some(n) = dep(Sym::DimStyle, &t.name)
            && d.dim_style(&n).is_none()
        {
            let text_style = dep(Sym::Style, &t.text_style).unwrap_or_else(|| t.text_style.clone());
            d.dim_styles.push(cadcraft_doc::DimStyle { name: n, text_style, ..t.clone() });
        }
    }
    for t in &x.mleader_styles {
        if let Some(n) = dep(Sym::MLeaderStyle, &t.name)
            && !d.mleader_styles.iter().any(|o| o.name.eq_ignore_ascii_case(&n))
        {
            let text_style = dep(Sym::Style, &t.text_style).unwrap_or_else(|| t.text_style.clone());
            d.mleader_styles.push(cadcraft_doc::MLeaderStyle { name: n, text_style, ..t.clone() });
        }
    }
    for t in &x.table_styles {
        if let Some(n) = dep(Sym::TableStyle, &t.name)
            && !d.table_styles.iter().any(|o| o.name.eq_ignore_ascii_case(&n))
        {
            d.table_styles.push(cadcraft_doc::TableStyle { name: n, ..t.clone() });
        }
    }
    let adopt = |d: &mut Drawing, e: &Entity| {
        let mut e = e.clone();
        e.handle = d.new_handle();
        rename_refs(&mut e, &dep);
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.assoc.clear();
        }
        e
    };
    for (name, b) in &x.blocks {
        let up = name.to_ascii_uppercase();
        if up.starts_with("*MODEL_SPACE") || up.starts_with("*PAPER_SPACE") {
            continue;
        }
        let mut nb = cadcraft_doc::Block::new(&format!("{key}|{name}"));
        nb.base = b.base;
        nb.description = b.description.clone();
        nb.anonymous = b.anonymous;
        nb.explodable = b.explodable;
        nb.units = b.units;
        // Array parameters keep their kind and spacing; block names follow the block renaming, and
        // a source edit in progress in the referenced file is not carried over.
        nb.array = b.array.as_ref().map(|a| {
            let mut a = a.clone();
            a.source = dep(Sym::Block, &a.source).unwrap_or(a.source);
            for r in &mut a.replaced {
                r.block = dep(Sym::Block, &r.block).unwrap_or_else(|| r.block.clone());
            }
            a.editing = None;
            a
        });
        for e in b.entities.iter() {
            let e = adopt(d, e);
            nb.entities.push(e);
        }
        d.blocks.insert(nb.name.clone(), Arc::new(nb));
    }
    let mut store = EntityStore::new();
    for e in x.model.iter() {
        let e = adopt(d, e);
        store.push(e);
    }
    let base = x.header.point("INSBASE").filter(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).unwrap_or_default();
    let units = u8::try_from(x.header.i64("INSUNITS", 0)).unwrap_or(0);
    if let Some(b) = d.blocks.get_mut(key) {
        let b = Arc::make_mut(b);
        b.entities = store;
        b.base = base;
        b.units = units;
    }
    Ok(())
}

fn set_status(d: &mut Drawing, key: &str, status: XrefStatus) {
    if let Some(b) = d.blocks.get_mut(key) {
        Arc::make_mut(b).xref_status = status;
    }
}

/// Empty xref `key`: its objects and the blocks it brought go (its other symbols stay, with any
/// changes made to them, until it is detached).
pub(crate) fn clear(d: &mut Drawing, key: &str) {
    let prefix = format!("{key}|");
    d.blocks.retain(|n, _| !has_prefix(n, &prefix));
    if let Some(b) = d.blocks.get_mut(key)
        && !b.entities.is_empty()
    {
        Arc::make_mut(b).entities = EntityStore::new();
    }
}

/// Unload xref `key`: it stays attached but shows nothing until it is reloaded.
pub(crate) fn unload(d: &mut Drawing, key: &str) {
    clear(d, key);
    set_status(d, key, XrefStatus::Unloaded);
}

/// Detach xref `key`: its definition, every reference to it and every symbol it brought go.
/// Returns how many references were erased.
pub(crate) fn detach(d: &mut Drawing, key: &str) -> usize {
    clear(d, key);
    d.blocks.remove(key);
    let prefix = format!("{key}|");
    let mine = |n: &str| has_prefix(n, &prefix);
    d.layers.retain(|t| !mine(&t.name));
    d.linetypes.retain(|t| !mine(&t.name));
    d.text_styles.retain(|t| !mine(&t.name));
    d.dim_styles.retain(|t| !mine(&t.name));
    d.mleader_styles.retain(|t| !mine(&t.name));
    d.table_styles.retain(|t| !mine(&t.name));
    let refs = |st: &EntityStore| -> Vec<cadcraft_doc::Handle> {
        st.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(key))).map(|e| e.handle).collect()
    };
    let mut hs = refs(&d.model);
    hs.extend(d.layouts.iter().flat_map(|l| refs(&l.entities)));
    let mut n = hs.len();
    for h in hs {
        d.remove_entity(h);
    }
    for b in d.blocks.values_mut() {
        let hs = refs(&b.entities);
        if !hs.is_empty() {
            n += hs.len();
            let b = Arc::make_mut(b);
            for h in hs {
                b.entities.remove(h);
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_resolve_against_the_host_folder_then_by_name() {
        let dir = std::env::temp_dir().join("proj").join("plans");
        let host = dir.join("host.dxf");
        let c = |path: &str, chosen| candidates(path, Some(&host), chosen);
        assert_eq!(c("parts/a.dxf", false), (vec![dir.join("parts").join("a.dxf"), dir.join("a.dxf")], None));
        let abs = std::env::temp_dir().join("abs").join("b.dwg");
        assert_eq!(c(&abs.to_string_lossy(), false), (vec![abs.clone(), dir.join("b.dwg")], None));
        if !cfg!(windows) {
            // A Windows path saved elsewhere still finds the file next to the host.
            assert_eq!(c("C:\\jobs\\site.dwg", false), (vec![dir.join("site.dwg")], None));
        }
        // Paths to other computers are only looked for beside the host.
        assert_eq!(c(r"\\192.0.2.1\share\x.dxf", false), (vec![dir.join("x.dxf")], Some(Refused::Network)));
        assert_eq!(c("file://192.0.2.1/s/x.dxf", false), (vec![dir.join("x.dxf")], Some(Refused::Network)));
        // Without a host drawing a relative path names no file.
        assert_eq!(candidates("a.dxf", None, false), (vec![], None));
        assert_eq!(candidates("a.dxf", None, true), (vec![PathBuf::from("a.dxf")], None));
        assert_eq!(find("", Some(&host), false), Err(Missing::NotFound));
    }

    #[test]
    fn prefixes_ignore_case() {
        assert!(has_prefix("SITE|Walls", "site|"));
        assert!(!has_prefix("SITE", "site|"));
        assert!(!has_prefix("é", "ab"));
    }

    #[test]
    fn imported_array_blocks_keep_their_parameters() {
        use cadcraft_doc::{ArrayKind, ArrayParams, Block, RectArray, ReplacedItem};
        let rect = RectArray { cols: 3, rows: 2, col_spacing: 5.0, row_spacing: 4.0, axis_angle: 0.0, row_elevation: 0.0 };
        let params = ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Rect(rect),
            levels: 1,
            level_spacing: 0.0,
            replaced: vec![ReplacedItem { i: 1, j: 0, level: 0, block: "ALT".into() }],
            editing: Some(Default::default()),
        };
        let mut arr = Block::new("*U2");
        arr.array = Some(params);
        let mut x = Drawing::default();
        x.blocks.insert("*U2".into(), Arc::new(arr));

        let mut d = Drawing::default();
        d.blocks.insert("SITE".into(), Arc::new(Block::new("SITE")));
        let mut cx = Ctx { chain: Vec::new(), objects: 0, files: 0, bytes: 0, notes: Vec::new() };
        assert!(import(&mut d, "SITE", &x, &mut cx).is_ok());

        let got = d.blocks.get("SITE|*U2").and_then(|b| b.array.clone());
        let Some(got) = got else { panic!("array parameters were dropped") };
        assert_eq!(got.source, "SITE|*U1");
        assert_eq!(got.replaced[0].block, "SITE|ALT");
        assert!(got.editing.is_none());
        let ArrayKind::Rect(r) = got.kind else { panic!("not a rectangular array") };
        assert_eq!((r.cols, r.rows), (3, 2));
    }
}
