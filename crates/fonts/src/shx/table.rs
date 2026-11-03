//! Finding SHX files by the name a text style or linetype gives (`romans.shx`, `GBCBIG`,
//! `C:\fonts\simplex.shx`): only the file name counts, so a path stored in a drawing never
//! reaches another folder or computer.
//!
//! Order: the font map ([`set_font_map`]) renames the file; then fonts registered by the app
//! ([`register`]: the web build's files opened alongside a drawing, tests); then the app's
//! [`FontProvider`]; then, natively, the folders of opened drawings ([`add_drawing_dir`], most
//! recent first) and the support search path ([`set_search_path`], from `CADCRAFT_SHXPATH` at
//! start). Results, found or not, are cached until one of these changes ([`generation`]
//! counts the changes, so a viewer knows to regenerate).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use super::parse::MAX_FILE_BYTES;
use super::{ShxError, ShxFont};

/// Where an app finds font files CADCraft can't reach itself (a web page's file store, an
/// app's own font folder). Called with the normalised file name (`romans.shx`); returns the
/// file's bytes. Called without the font table's lock held.
pub trait FontProvider: Send + Sync {
    fn load(&self, file_name: &str) -> Option<Vec<u8>>;
}

/// Most fonts kept loaded, and their total bytes.
const MAX_FONTS: usize = 128;
const MAX_LOADED_BYTES: usize = 256 << 20;
/// Most drawing folders and search path entries remembered.
const MAX_DIRS: usize = 32;
/// Most font map entries.
const MAX_MAP: usize = 1024;

#[derive(Default)]
struct Table {
    registered: HashMap<String, Arc<ShxFont>>,
    resolved: HashMap<String, Option<Arc<ShxFont>>>,
    loaded_bytes: usize,
    provider: Option<Arc<dyn FontProvider>>,
    drawing_dirs: Vec<PathBuf>,
    search_path: Vec<PathBuf>,
    map: Vec<(String, String)>,
    generation: u64,
}

impl Table {
    /// Forget cached results: something that changes them changed.
    fn changed(&mut self) {
        self.resolved.clear();
        self.loaded_bytes = self.registered.values().map(|f| f.data.len()).sum();
        self.generation = self.generation.wrapping_add(1);
    }
}

fn table() -> MutexGuard<'static, Table> {
    static T: OnceLock<Mutex<Table>> = OnceLock::new();
    T.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        let search_path = std::env::var_os("CADCRAFT_SHXPATH")
            .map(|p| std::env::split_paths(&p).filter(|d| !d.as_os_str().is_empty()).take(MAX_DIRS).collect())
            .unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let search_path = Vec::new();
        Mutex::new(Table { search_path, ..Table::default() })
    })
    .lock()
    .unwrap_or_else(PoisonError::into_inner)
}

/// The lookup key of a font name: its lower-case file name with `.shx` (`romans.shx`), or
/// `None` for a name that isn't an SHX file (a TrueType font, empty, the built-in font).
pub(crate) fn key(name: &str) -> Option<String> {
    let base = name.trim().rsplit(['/', '\\']).next()?.trim();
    if base.is_empty() || base.len() > 255 || base.contains(':') || base.chars().any(char::is_control) {
        return None;
    }
    let lower = base.to_ascii_lowercase();
    if lower.ends_with(".shx") {
        return (lower.len() > 4).then_some(lower);
    }
    let ext = Path::new(&lower).extension().map(|e| e.to_string_lossy().to_string());
    match ext.as_deref() {
        // `txt` (no extension) is a classic style font name; other extensions are other kinds.
        None => (lower != crate::BUILTIN_FONT.to_ascii_lowercase() && !lower.contains(' ')).then(|| format!("{lower}.shx")),
        Some(_) => None,
    }
}

/// Register font bytes under a file name (the web build's files opened alongside a drawing;
/// tests). Replaces a font registered under the same name.
pub fn register(name: &str, bytes: &[u8]) -> Result<(), ShxError> {
    let k = key(name).ok_or(ShxError::NotShx)?;
    let font = Arc::new(ShxFont::parse(bytes)?);
    let mut t = table();
    if !t.registered.contains_key(&k) && t.registered.len() >= MAX_FONTS {
        return Err(ShxError::TooLarge);
    }
    t.registered.insert(k, font);
    t.changed();
    Ok(())
}

/// Forget every registered font (a web session closing its drawings).
pub fn clear_registered() {
    let mut t = table();
    t.registered.clear();
    t.changed();
}

/// Set (or clear) the app's font provider.
pub fn set_provider(provider: Option<Arc<dyn FontProvider>>) {
    let mut t = table();
    t.provider = provider;
    t.changed();
}

/// Search the folder of a drawing being opened for the fonts its styles name (most recent
/// first).
pub fn add_drawing_dir(dir: &Path) {
    if dir.as_os_str().is_empty() {
        return;
    }
    let mut t = table();
    if t.drawing_dirs.first().is_some_and(|d| d == dir) {
        return;
    }
    t.drawing_dirs.retain(|d| d != dir);
    t.drawing_dirs.insert(0, dir.to_path_buf());
    t.drawing_dirs.truncate(MAX_DIRS);
    t.changed();
}

/// The drawing folders searched, most recent first.
pub fn drawing_dirs() -> Vec<PathBuf> {
    table().drawing_dirs.clone()
}

/// Set the support search path for SHX files: folders separated by `;` (or the platform's
/// path separator). Empty clears it.
pub fn set_search_path(list: &str) {
    let dirs: Vec<PathBuf> = list
        .split(';')
        .flat_map(|p| if cfg!(windows) { vec![PathBuf::from(p)] } else { std::env::split_paths(p).collect() })
        .filter(|d| !d.as_os_str().is_empty() && !d.to_string_lossy().trim().is_empty())
        .take(MAX_DIRS)
        .collect();
    let mut t = table();
    t.search_path = dirs;
    t.changed();
}

/// The support search path, `;`-separated.
pub fn search_path() -> String {
    table().search_path.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join(";")
}

/// Set the font map: one `font;replacement` per line (or separated by `|`), as a font mapping
/// file lists them. Lines that aren't a pair are ignored; empty clears it.
pub fn set_font_map(text: &str) {
    let map: Vec<(String, String)> = text
        .split(['\n', '|'])
        .filter_map(|l| {
            let (a, b) = l.split_once(';')?;
            Some((key(a)?, b.trim().to_string()))
        })
        .filter(|(_, b)| !b.is_empty())
        .take(MAX_MAP)
        .collect();
    let mut t = table();
    t.map = map;
    t.changed();
}

/// The font map, one `font;replacement` per line.
pub fn font_map() -> String {
    table().map.iter().map(|(a, b)| format!("{a};{b}")).collect::<Vec<_>>().join("\n")
}

/// Counts changes to what fonts are found (registration, provider, folders, map).
pub fn generation() -> u64 {
    table().generation
}

/// The SHX font file a style or linetype names, if CADCraft can find and read it.
pub fn find(name: &str) -> Option<Arc<ShxFont>> {
    let mut k = key(name)?;
    let (provider, dirs) = {
        let t = table();
        // The font map renames once (a map can't loop).
        if let Some(to) = t.map.iter().find(|(from, _)| *from == k).and_then(|(_, to)| key(to)) {
            k = to;
        }
        if let Some(f) = t.registered.get(&k) {
            return Some(f.clone());
        }
        if let Some(r) = t.resolved.get(&k) {
            return r.clone();
        }
        let dirs: Vec<PathBuf> = t.drawing_dirs.iter().chain(t.search_path.iter()).cloned().collect();
        (t.provider.clone(), dirs)
    };
    let generation = generation();
    let font = provider
        .and_then(|p| p.load(&k))
        .or_else(|| read_from(&dirs, &k))
        .filter(|b| b.len() <= MAX_FILE_BYTES)
        .and_then(|b| ShxFont::parse(&b).ok())
        .map(Arc::new);
    let mut t = table();
    // Something changed while the file was read: keep the result out of the new cache.
    if t.generation != generation {
        return font;
    }
    if t.resolved.len() >= MAX_FONTS * 4 {
        t.resolved.clear();
        t.loaded_bytes = t.registered.values().map(|f| f.data.len()).sum();
    }
    let size = font.as_ref().map_or(0, |f| f.data.len());
    if t.loaded_bytes.saturating_add(size) > MAX_LOADED_BYTES {
        // Over the memory bound: used, not kept.
        return font;
    }
    t.loaded_bytes += size;
    t.resolved.insert(k, font.clone());
    font
}

/// Most directory entries looked at for a font file whose name differs only in case.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DIR_ENTRIES: usize = 10_000;

/// The file `name` (a lower-case key) in the first of `dirs` that has it, whatever the case of
/// its name: lower-case, upper-case, else any other case found in the directory (`Simplex.shx`
/// on a case-sensitive file system).
#[cfg(not(target_arch = "wasm32"))]
fn read_from(dirs: &[PathBuf], name: &str) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let read = |file: &Path| -> Option<Vec<u8>> {
        let meta = std::fs::metadata(file).ok()?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        let limit = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
        std::fs::File::open(file).and_then(|f| f.take(limit).read_to_end(&mut bytes)).ok()?;
        Some(bytes)
    };
    for d in dirs {
        if let Some(b) = [d.join(name), d.join(name.to_ascii_uppercase())].iter().find_map(|f| read(f)) {
            return Some(b);
        }
        let Ok(entries) = std::fs::read_dir(d) else { continue };
        let found = entries
            .take(MAX_DIR_ENTRIES)
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
            .and_then(|e| read(&e.path()));
        if found.is_some() {
            return found;
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn read_from(_dirs: &[PathBuf], _name: &str) -> Option<Vec<u8>> {
    None
}
