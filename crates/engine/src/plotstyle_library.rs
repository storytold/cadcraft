//! The user-level plot style library: table files shared by every drawing.
//!
//! Tables are JSON files in our own `STYLESMANAGER` format (`docs/plot-styles.md`), kept in
//! the `plotstyles` folder of the CADCraft settings directory (`CADCRAFT_CONFIG_DIR`, else the
//! platform's settings directory). They are read once at start-up and installed in
//! [`cadcraft_doc`], which resolves a page setup's table as the drawing's own, then the
//! library's, then the built-in one. Reading is bounded: bad, oversized or duplicate files are
//! skipped with a warning and never stop the others loading. On the web the library lasts for
//! the session unless a host installs a folder.

use std::sync::{Arc, Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use cadcraft_doc::MAX_TABLE_FILE;
use cadcraft_doc::{PlotStyleLibrary, PlotStyleTable};

/// Most library files read from a folder.
pub const MAX_LIBRARY_FILES: usize = 1000;

/// Where library files live. A folder on a native build, memory on the web, or nothing when
/// there is no settings directory.
#[derive(Clone, Debug)]
pub enum PlotStyleStore {
    /// A folder of `*.json` table files.
    #[cfg(not(target_arch = "wasm32"))]
    Folder(std::path::PathBuf),
    /// Files kept for the life of the process.
    Memory,
    /// No usable storage: listing is empty and writing reports why.
    Unavailable,
}

static MEMORY: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn memory() -> Result<std::sync::MutexGuard<'static, Vec<(String, String)>>, String> {
    MEMORY.lock().map_err(|_| "the plot style library is unavailable".to_string())
}

/// Whether `name` is a plain library file name of ours (a `.json` file, no path parts).
fn is_library_file(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(cadcraft_doc::PLOT_STYLE_FILE_EXT)
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.contains("..")
}

#[cfg(not(target_arch = "wasm32"))]
fn library_file(dir: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    if !is_library_file(name) {
        return Err(format!("`{name}` is not a plot style library file name"));
    }
    Ok(dir.join(name))
}

impl PlotStyleStore {
    /// Files kept in memory (the web, and tests).
    pub fn memory() -> PlotStyleStore {
        PlotStyleStore::Memory
    }

    /// A specific folder, for a host that keeps the library somewhere of its own.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn folder(dir: impl Into<std::path::PathBuf>) -> PlotStyleStore {
        PlotStyleStore::Folder(dir.into())
    }

    /// The `plotstyles` folder of the CADCraft settings directory (`CADCRAFT_CONFIG_DIR`, else
    /// `$XDG_CONFIG_HOME/cadcraft`, `~/Library/Application Support/CADCraft` or `%APPDATA%`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn settings_folder() -> PlotStyleStore {
        fn root() -> Option<std::path::PathBuf> {
            use std::path::PathBuf;
            let get = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
            if let Some(dir) = get("CADCRAFT_CONFIG_DIR") {
                return Some(dir);
            }
            match std::env::consts::OS {
                "macos" => get("HOME").map(|h| h.join("Library/Application Support/CADCraft")),
                "windows" => get("APPDATA").map(|r| r.join("CADCraft")),
                _ => {
                    get("XDG_CONFIG_HOME").filter(|p| p.is_absolute()).or_else(|| get("HOME").map(|h| h.join(".config"))).map(|c| c.join("cadcraft"))
                }
            }
        }
        match root() {
            Some(dir) => PlotStyleStore::Folder(dir.join("plotstyles")),
            None => PlotStyleStore::Unavailable,
        }
    }

    fn list(&self) -> Result<Vec<String>, String> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            PlotStyleStore::Folder(dir) => {
                let entries = match std::fs::read_dir(dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                    Err(e) => return Err(format!("{}: {e}", dir.display())),
                };
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().ok().is_some_and(|t| t.is_file()))
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| is_library_file(n))
                    .collect();
                names.sort();
                names.truncate(MAX_LIBRARY_FILES);
                Ok(names)
            }
            PlotStyleStore::Memory => {
                let mut names: Vec<String> = memory()?.iter().map(|(n, _)| n.clone()).filter(|n| is_library_file(n)).collect();
                names.sort();
                names.truncate(MAX_LIBRARY_FILES);
                Ok(names)
            }
            PlotStyleStore::Unavailable => Ok(Vec::new()),
        }
    }

    fn read(&self, name: &str) -> Result<String, String> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            PlotStyleStore::Folder(dir) => {
                let path = library_file(dir, name)?;
                let len = std::fs::metadata(&path).map_err(|e| format!("{name}: {e}"))?.len();
                if len > MAX_TABLE_FILE as u64 {
                    return Err(format!("{name}: the plot style table file is too large"));
                }
                std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))
            }
            PlotStyleStore::Memory => {
                memory()?.iter().find(|(f, _)| f == name).map(|(_, t)| t.clone()).ok_or_else(|| format!("{name}: no such file"))
            }
            PlotStyleStore::Unavailable => Err("there is no plot style library folder".into()),
        }
    }

    fn write(&self, name: &str, text: &str) -> Result<(), String> {
        if !is_library_file(name) {
            return Err(format!("`{name}` is not a plot style library file name"));
        }
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            PlotStyleStore::Folder(dir) => {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let path = library_file(dir, name)?;
                let tmp = dir.join(format!("{name}.cadcraft-tmp"));
                std::fs::write(&tmp, text).map_err(|e| format!("{name}: {e}"))?;
                std::fs::rename(&tmp, &path).map_err(|e| format!("{name}: {e}"))
            }
            PlotStyleStore::Memory => {
                let mut files = memory()?;
                files.retain(|(f, _)| f != name);
                files.push((name.to_string(), text.to_string()));
                Ok(())
            }
            PlotStyleStore::Unavailable => Err("there is no plot style library folder".into()),
        }
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            PlotStyleStore::Folder(dir) => std::fs::remove_file(library_file(dir, name)?).map_err(|e| format!("{name}: {e}")),
            PlotStyleStore::Memory => {
                let mut files = memory()?;
                let before = files.len();
                files.retain(|(f, _)| f != name);
                if files.len() == before { Err(format!("{name}: no such file")) } else { Ok(()) }
            }
            PlotStyleStore::Unavailable => Err("there is no plot style library folder".into()),
        }
    }
}

fn default_store() -> PlotStyleStore {
    #[cfg(not(target_arch = "wasm32"))]
    {
        PlotStyleStore::settings_folder()
    }
    #[cfg(target_arch = "wasm32")]
    {
        PlotStyleStore::memory()
    }
}

fn store_slot() -> &'static Mutex<PlotStyleStore> {
    static SLOT: OnceLock<Mutex<PlotStyleStore>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(default_store()))
}

fn store() -> PlotStyleStore {
    match store_slot().lock() {
        Ok(store) => store.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn loaded() -> &'static OnceLock<()> {
    static LOADED: OnceLock<()> = OnceLock::new();
    &LOADED
}

/// Read every library file (bad ones are skipped with a warning), then install the result in
/// [`cadcraft_doc`]. Returns the library as installed.
pub fn reload_plot_style_library() -> Arc<PlotStyleLibrary> {
    let store = store();
    let files = match store.list() {
        Ok(files) => files,
        Err(e) => {
            let lib = PlotStyleLibrary::new(Vec::new(), vec![format!("the plot style library: {e}")]);
            cadcraft_doc::set_plot_style_library(lib);
            return cadcraft_doc::plot_style_library();
        }
    };
    let mut tables = Vec::new();
    let mut warnings = Vec::new();
    for file in files.iter().take(MAX_LIBRARY_FILES) {
        let text = match store.read(file) {
            Ok(text) => text,
            Err(e) => {
                warnings.push(format!("{file}: {e}"));
                continue;
            }
        };
        match PlotStyleTable::from_file_text(&text) {
            Ok(t) if t.name.is_empty() => warnings.push(format!("{file}: the plot style table has no name")),
            Ok(t) => {
                if tables.iter().any(|x: &PlotStyleTable| x.name.eq_ignore_ascii_case(&t.name)) {
                    warnings.push(format!("{file}: a library table `{}` is already loaded", t.name));
                } else {
                    tables.push(t);
                }
            }
            Err(e) => warnings.push(format!("{file}: {e}")),
        }
    }
    let lib = PlotStyleLibrary::new(tables, warnings);
    for warning in lib.warnings() {
        log::warn!("plot style library: {warning}");
    }
    cadcraft_doc::set_plot_style_library(lib);
    cadcraft_doc::plot_style_library()
}

/// Load the user library once, at start-up (a host with its own storage calls
/// [`set_plot_style_store`] instead, which loads immediately).
pub fn init_plot_style_library() {
    if loaded().set(()).is_ok() {
        reload_plot_style_library();
    }
}

/// Install the host's library storage and reload it (tests use a temporary folder).
pub fn set_plot_style_store(s: PlotStyleStore) {
    let slot = store_slot();
    match slot.lock() {
        Ok(mut current) => *current = s,
        Err(poisoned) => *poisoned.into_inner() = s,
    }
    let _ = loaded().set(());
    reload_plot_style_library();
}

/// Save `table` into the library under its own name. Returns the file name. The drawing is not
/// changed: only loading a table into the drawing embeds it.
pub fn save_table(table: &PlotStyleTable) -> Result<String, String> {
    let file = cadcraft_doc::plot_style_file_name(&table.name).ok_or_else(|| "the plot style table has no name".to_string())?;
    store().write(&file, &table.to_file_text())?;
    reload_plot_style_library();
    Ok(file)
}

/// Delete the library table `name` (the drawing's own tables are untouched).
pub fn delete_table(name: &str) -> Result<(), String> {
    let file = cadcraft_doc::plot_style_file_name(name).ok_or_else(|| "the plot style table has no name".to_string())?;
    store().delete(&file)?;
    reload_plot_style_library();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_file_names_are_plain_json() {
        assert_eq!(cadcraft_doc::plot_style_file_name("office.stb").as_deref(), Some("office.stb.json"));
        assert_eq!(cadcraft_doc::plot_style_file_name(" a/b ").as_deref(), Some("a_b.json"));
        assert_eq!(cadcraft_doc::plot_style_file_name("   ").as_deref(), None);
        assert!(is_library_file("office.stb.json") && is_library_file("OFFICE.JSON"));
        for bad in ["../x.json", ".hidden.json", "a/b.json", "x.ctb"] {
            assert!(!is_library_file(bad), "{bad}");
        }
    }

    #[test]
    fn memory_store_round_trips() {
        let store = PlotStyleStore::memory();
        let table = PlotStyleTable::builtin("default.stb").unwrap_or_default();
        let text = table.to_file_text();
        store.write("default.stb.json", &text).unwrap();
        assert_eq!(store.read("default.stb.json").unwrap(), text);
        assert!(store.list().unwrap().contains(&"default.stb.json".to_string()));
        store.delete("default.stb.json").unwrap();
        assert!(store.list().unwrap().is_empty());
    }
}
