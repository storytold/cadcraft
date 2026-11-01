//! Block libraries for the Blocks palette: a folder of drawings or a single drawing file whose
//! blocks can be placed in the current drawing.
//!
//! BLOCKLIBRARY lists what a library holds, BLOCKLIBRARIES keeps the list of libraries the user
//! added (in the settings folder; in memory where there is none), and BLOCKLIBIMPORT defines a
//! library block in the current drawing so that INSERT can place it. Files are read through the
//! host's io hooks like OPEN, so the engine stays I/O-agnostic.

use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use cadcraft_doc::Drawing;
use serde_json::{Value, json};

use super::insert_file::{define_from, read_drawing, stem};
use super::valid_block_name;
use crate::cmd::{CommandSpec, bad, has_doc, str_param};
use crate::{Result, Session};

/// Most drawings one folder library lists.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILES: usize = 2000;
/// Most directory entries looked at in a folder library.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SCANNED: usize = 100_000;
/// Most blocks a palette listing holds: the rest of a hostile file's table is left out.
pub(super) const MAX_LISTED: usize = 10_000;
/// Most libraries the list keeps.
const MAX_LIBRARIES: usize = 64;
/// The longest library path kept.
const MAX_PATH_BYTES: usize = 4096;
/// The file in the settings folder that keeps the library list, and its largest size.
#[cfg(not(target_arch = "wasm32"))]
const LIST_FILE: &str = "block-libraries.json";
#[cfg(not(target_arch = "wasm32"))]
const MAX_LIST_BYTES: usize = 256 << 10;

/// How many parsed library drawings stay in memory.
const CACHE_MAX: usize = 4;

/// A library drawing already read, and the file's size and time then.
struct Cached {
    path: String,
    stamp: (u64, Option<SystemTime>),
    drawing: Arc<Drawing>,
}

static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

/// The file's size and modification time; `None` where it cannot be told (nothing is kept then).
fn stamp(path: &str) -> Option<(u64, Option<SystemTime>)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file).map(|m| (m.len(), m.modified().ok()))
}

/// A library drawing, read once and kept while the file is as it was: placing a block must not
/// parse the whole file again.
fn cached_drawing(path: &str) -> Result<Arc<Drawing>> {
    let now = stamp(path);
    if let Some(now) = now
        && let Ok(cache) = CACHE.lock()
        && let Some(hit) = cache.iter().find(|c| c.path == path && c.stamp == now)
    {
        return Ok(hit.drawing.clone());
    }
    // The palette reads previews outside a command, so a parser panic stops here too.
    let read = std::panic::catch_unwind(|| read_drawing(path));
    let drawing = Arc::new(read.unwrap_or_else(|p| Err(bad("blocklibrary", format!("{path}: {}", crate::guard::message(p.as_ref())))))?);
    if let Some(now) = now
        && let Ok(mut cache) = CACHE.lock()
    {
        cache.retain(|c| c.path != path);
        cache.push(Cached { path: path.to_string(), stamp: now, drawing: drawing.clone() });
        while cache.len() > CACHE_MAX {
            cache.remove(0);
        }
    }
    Ok(drawing)
}

/// The list where no settings folder keeps it, and the copy read from it.
static MEMORY: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The last write of the list failed: the copy in memory is newer than the file.
static UNSAVED: AtomicBool = AtomicBool::new(false);

/// The commands the Blocks palette uses: BLOCKLIBRARY, BLOCKLIBIMPORT and BLOCKLIBRARIES.
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("blocklibrary", "Block Library", run_library)
            .params("{path: a folder of DWG/DXF files, or one such file} → {kind: \"folder\", path, files: [{name, file}]} | {kind: \"file\", path, blocks: [{name, description}]}")
            .noundo(),
        CommandSpec::new("blocklibraries", "Block Libraries", run_libraries)
            .params("{op?: \"list\" (default)|\"add\"|\"remove\", path? (add, remove)} → {libraries: [path], path? (add: the entry as kept)}")
            .noundo(),
        CommandSpec::new("blocklibimport", "Import Library Block", run_import)
            .params("{path: DWG/DXF file, block?: the block to take from it (default: the whole drawing), name?: the block to define (default: `block`, else the file name)} → {name}")
            .enabled(has_doc),
    ]
}

/// A drawing file by its extension.
fn is_drawing(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
}

/// The drawings in a folder, by name: `(block name, full path)`, and whether some were left out.
fn folder_files(dir: &Path) -> Result<(Vec<(String, String)>, bool)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rd = std::fs::read_dir(dir).map_err(|e| bad("blocklibrary", format!("{}: {e}", dir.display())))?;
        let entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).take(MAX_SCANNED.saturating_add(1)).collect();
        let mut truncated = entries.len() > MAX_SCANNED;
        let mut out: Vec<(String, String)> = entries
            .iter()
            .take(MAX_SCANNED)
            .filter(|p| is_drawing(p) && p.is_file())
            .filter_map(|p| p.to_str().map(|s| (stem(s), s.to_string())))
            .collect();
        out.sort_by_key(|(n, _)| n.to_ascii_lowercase());
        truncated |= out.len() > MAX_FILES;
        out.truncate(MAX_FILES);
        Ok((out, truncated))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dir;
        Err(bad("blocklibrary", "paths are not available on the web"))
    }
}

/// Whether two library paths name the same place: letter case counts except where the file
/// system ignores it (Windows, and macOS by default).
fn same_path(a: &str, b: &str) -> bool {
    if cfg!(any(windows, target_os = "macos")) { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// Whether `path` is a folder.
fn is_folder(path: &str) -> bool {
    !cfg!(target_arch = "wasm32") && Path::new(path).is_dir()
}

/// The named blocks of a library drawing: no generated or layout blocks, no xrefs.
pub(crate) fn library_blocks(d: &Drawing) -> Vec<&std::sync::Arc<cadcraft_doc::Block>> {
    let mut v: Vec<_> =
        d.blocks.values().filter(|b| !b.anonymous && b.xref_path.is_none() && !b.name.starts_with('*') && !b.name.contains('|')).collect();
    v.sort_by_key(|b| b.name.to_ascii_lowercase());
    v
}

/// Read a library file (for the palette's previews too), kept until the file changes.
pub fn read_library_file(path: &str) -> Result<Arc<Drawing>> {
    cached_drawing(path)
}

/// BLOCKLIBRARY: list a folder's drawings or a drawing's blocks.
fn run_library(_s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad("blocklibrary", "`path` is required"))?;
    if is_folder(path) {
        let (files, truncated) = folder_files(Path::new(path))?;
        let list: Vec<Value> = files.iter().map(|(name, file)| json!({ "name": name, "file": file })).collect();
        return Ok(json!({ "kind": "folder", "path": path, "files": list, "truncated": truncated }));
    }
    let d = cached_drawing(path)?;
    let blocks = library_blocks(&d);
    let list: Vec<Value> = blocks.iter().take(MAX_LISTED).map(|b| json!({ "name": b.name, "description": b.description })).collect();
    Ok(json!({ "kind": "file", "path": path, "blocks": list, "truncated": blocks.len() > MAX_LISTED }))
}

/// BLOCKLIBIMPORT: define a library block (or a whole drawing) in the current drawing.
fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad("blocklibimport", "`path` is required"))?;
    let block = str_param(p, "block").map(str::trim).filter(|s| !s.is_empty());
    // A whole drawing is named after its file; a block of it after itself.
    let name = str_param(p, "name")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| block.map_or_else(|| stem(path), str::to_string));
    if !valid_block_name(&name) {
        return Err(bad("blocklibimport", format!("\"{name}\" is not a valid block name")));
    }
    let mut src = (*cached_drawing(path)?).clone();
    if let Some(block) = block {
        let b = src.block(block).cloned().ok_or_else(|| bad("blocklibimport", format!("{path} has no block \"{block}\"")))?;
        // The block is a drawing of its own: its objects as model space and its base point as
        // $INSBASE, with the file's other blocks around it.
        src.model = b.entities.clone();
        src.header.set("INSBASE", cadcraft_doc::HVal::Point(b.base));
    }
    // Only the layers, linetypes and styles the block needs come along, not the whole file's.
    let name = define_from(s, &src, &name, true)?;
    Ok(json!({ "name": name }))
}

/// Where the library list is kept.
#[cfg(not(target_arch = "wasm32"))]
fn list_path() -> Option<PathBuf> {
    crate::settings_dir::current().map(|d| d.join(LIST_FILE))
}

/// A path the list can hold: short enough, with no control characters.
fn acceptable(path: &str) -> bool {
    !path.is_empty() && path.len() <= MAX_PATH_BYTES && !path.chars().any(char::is_control)
}

/// The list in the settings folder. The file is hand-editable, so what it holds is checked here.
/// `Ok(None)`: no settings folder or no file yet. `Err`: a file that is there but unreadable.
#[cfg(not(target_arch = "wasm32"))]
fn read_list() -> std::result::Result<Option<Vec<String>>, String> {
    let Some(path) = list_path() else { return Ok(None) };
    if !path.exists() {
        return Ok(None);
    }
    let bytes = crate::capped_read::read_capped(&path, MAX_LIST_BYTES).map_err(|e| format!("{}: {e}", path.display()))?;
    let list: Vec<String> = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(list.into_iter().filter(|p| acceptable(p)).take(MAX_LIBRARIES).collect()))
}

/// On the web there is no settings folder: the list lives in memory only.
#[cfg(target_arch = "wasm32")]
fn read_list() -> std::result::Result<Option<Vec<String>>, String> {
    Ok(None)
}

/// The list, and why the file could not be read when it exists but is unreadable. Such a file is
/// never written over: `run_libraries` refuses to change the list while it stays unreadable.
fn load_list() -> (Vec<String>, Option<String>) {
    let memory = || MEMORY.lock().map(|m| m.clone()).unwrap_or_default();
    if UNSAVED.load(Ordering::Relaxed) {
        return (memory(), None);
    }
    match read_list() {
        Ok(Some(list)) => (list, None),
        Ok(None) => (memory(), None),
        Err(why) => (memory(), Some(why)),
    }
}

/// Write the list to `path`, its folder made if needed. `false` when it could not be written.
#[cfg(not(target_arch = "wasm32"))]
fn write_list(path: &Path, list: &[String]) -> bool {
    let Ok(bytes) = serde_json::to_vec_pretty(list) else { return false };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    crate::atomic_write::write_atomic(path, &bytes).is_ok()
}

/// Keep the list in memory and write it to the settings folder.
fn save_list(list: &[String]) {
    if let Ok(mut m) = MEMORY.lock() {
        *m = list.to_vec();
    }
    // A list that cannot be written is still kept for this session.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(p) = list_path() {
        UNSAVED.store(!write_list(&p, list), Ordering::Relaxed);
    }
}

/// BLOCKLIBRARIES: list, add to or remove from the libraries.
fn run_libraries(_s: &mut Session, p: &Value) -> Result<Value> {
    let (mut list, unreadable) = load_list();
    let op = str_param(p, "op").unwrap_or("list");
    if let (Some(why), "add" | "remove") = (&unreadable, op) {
        return Err(bad("blocklibraries", format!("the library list cannot be read, so it is left as it is ({why})")));
    }
    let path = str_param(p, "path").map(str::trim).filter(|s| !s.is_empty());
    match op {
        "list" => {}
        "add" => {
            let path = path.ok_or_else(|| bad("blocklibraries", "`path` is required"))?;
            let drawing = is_drawing(Path::new(path)) && Path::new(path).is_file();
            if !acceptable(path) || !(is_folder(path) || drawing) {
                return Err(bad("blocklibraries", "a library is a folder or a DWG/DXF file"));
            }
            let stored = match list.iter().find(|l| same_path(l, path)) {
                Some(l) => l.clone(),
                None => {
                    if list.len() >= MAX_LIBRARIES {
                        return Err(bad("blocklibraries", format!("at most {MAX_LIBRARIES} libraries")));
                    }
                    list.push(path.to_string());
                    path.to_string()
                }
            };
            save_list(&list);
            return Ok(json!({ "libraries": list, "path": stored }));
        }
        "remove" => {
            let path = path.ok_or_else(|| bad("blocklibraries", "`path` is required"))?;
            list.retain(|l| !same_path(l, path));
            save_list(&list);
        }
        other => return Err(bad("blocklibraries", format!("unknown op \"{other}\""))),
    }
    Ok(json!({ "libraries": list }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list that could not be written stays the list of this session, whatever the file says.
    #[test]
    fn a_failed_write_keeps_the_list_in_memory() {
        let dir = std::env::temp_dir().join(format!("cadcraft-library-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("a-file");
        std::fs::write(&blocker, b"x").unwrap();
        let list = vec!["C:/kept".to_string()];
        // A folder that is a file: neither the folder nor the list can be made.
        assert!(!write_list(&blocker.join("block-libraries.json"), &list));
        assert!(write_list(&dir.join("block-libraries.json"), &list));

        *MEMORY.lock().unwrap() = list.clone();
        UNSAVED.store(true, Ordering::Relaxed);
        assert_eq!(load_list().0, list);
        UNSAVED.store(false, Ordering::Relaxed);
        MEMORY.lock().unwrap().clear();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
