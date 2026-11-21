//! Finding SHX files by the name a text style or linetype gives (`romans.shx`, `GBCBIG`,
//! `C:\fonts\simplex.shx`): only the file name counts, so a path stored in a drawing never
//! reaches another folder or computer.
//!
//! Order: the font map ([`set_font_map`]) renames the file; then fonts registered by the app
//! ([`register`]: the web build's files opened alongside a drawing, tests); then the app's
//! [`FontProvider`]; then, natively, the folders of opened drawings ([`add_drawing_dir`], most
//! recent first), the user's font folders ([`set_font_dirs`], from Options) and the support
//! search path ([`set_search_path`], from `CADCRAFT_SHXPATH` at start). Results, found or not, are cached until one of these changes ([`generation`]
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
    /// The bytes of the font file `file_name`, or `None` when this provider has none by that name.
    fn load(&self, file_name: &str) -> Option<Vec<u8>>;
}

/// Decides whether the file at a path may be opened by the font search; asked of every candidate in
/// a folder, before anything opens it.
pub type OpenGuard = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// Most fonts kept loaded, and their total bytes.
const MAX_FONTS: usize = 128;
const MAX_LOADED_BYTES: usize = 256 << 20;
/// Most drawing folders and search path entries remembered.
const MAX_DIRS: usize = 32;
/// Most folders whose guards are kept.
const MAX_GUARDED_DIRS: usize = 1024;
/// Most guards kept past [`MAX_GUARDED_DIRS`], each applying to every folder searched.
const MAX_OVERFLOW_GUARDS: usize = 4096;
/// Most font map entries.
const MAX_MAP: usize = 1024;

#[derive(Default)]
struct Table {
    registered: HashMap<String, Arc<ShxFont>>,
    resolved: HashMap<String, Option<Arc<ShxFont>>>,
    loaded_bytes: usize,
    provider: Option<Arc<dyn FontProvider>>,
    drawing_dirs: Vec<PathBuf>,
    /// The guards of the drawing folders that have any, by the owner that gave each one.
    drawing_guards: HashMap<PathBuf, Vec<(String, OpenGuard)>>,
    /// Guards given once [`MAX_GUARDED_DIRS`] folders had one: they apply to every folder searched.
    overflow_guards: Vec<(String, OpenGuard)>,
    /// The overflow guards ran out: every file is refused from then on.
    overflow_closed: bool,
    font_dirs: Vec<PathBuf>,
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
    add_dir(dir, None);
}

/// Like [`add_drawing_dir`], with every file the search would open in `dir` put to `guard` first,
/// whatever its name and however it is found in the folder: a font the guard refuses is skipped.
///
/// The table is shared by the whole process, so a folder keeps the guard of every `owner` that gave
/// one (an owner giving it again replaces its own) and a font is opened only when all of them allow
/// it. Adding the folder without a guard never removes one. Fonts found are kept for every caller,
/// so owners with different rules shouldn't share a process.
pub fn add_drawing_dir_guarded(dir: &Path, owner: &str, guard: OpenGuard) {
    add_dir(dir, Some((owner, guard)));
}

/// Remember `dir`, most recent first, with the guard it is searched under, if any. The folder's
/// guards are kept after it drops off the list of drawing folders.
fn add_dir(dir: &Path, guard: Option<(&str, OpenGuard)>) {
    if dir.as_os_str().is_empty() {
        return;
    }
    let mut t = table();
    if guard.is_none() && t.drawing_dirs.first().is_some_and(|d| d == dir) {
        return;
    }
    // Guards outlive the drawing list: a folder dropped from it can still be reached as a font
    // folder or on the search path. When there is no room for another folder's guards, they apply to
    // every folder searched from then on, so nothing is left unguarded.
    let no_room = !t.drawing_guards.contains_key(dir) && t.drawing_guards.len() >= MAX_GUARDED_DIRS;
    t.drawing_dirs.retain(|d| d != dir);
    t.drawing_dirs.insert(0, dir.to_path_buf());
    t.drawing_dirs.truncate(MAX_DIRS);
    if let Some((owner, guard)) = guard {
        if no_room {
            // Every overflow guard stays: none may be replaced by another folder's. When even these
            // run out, everything is refused from then on.
            let kept = &mut t.overflow_guards;
            if !kept.iter().any(|(_, g)| Arc::ptr_eq(g, &guard)) {
                if kept.len() < MAX_OVERFLOW_GUARDS {
                    kept.push((owner.to_string(), guard));
                } else {
                    t.overflow_closed = true;
                }
            }
        } else {
            let guards = t.drawing_guards.entry(dir.to_path_buf()).or_default();
            guards.retain(|(o, _)| o != owner);
            guards.push((owner.to_string(), guard));
        }
    }
    t.changed();
}

/// The drawing folders searched, most recent first.
pub fn drawing_dirs() -> Vec<PathBuf> {
    table().drawing_dirs.clone()
}

/// Search the user's font folders (and the folders under them that hold SHX files, as the
/// font list finds them) after the drawings' folders. Looks again even when the folders are the
/// same: files may have been added.
pub fn set_font_dirs(dirs: Vec<PathBuf>) {
    let mut t = table();
    t.font_dirs = dirs.into_iter().filter(|d| !d.as_os_str().is_empty()).take(MAX_DIRS).collect();
    t.changed();
}

/// Set the support search path for SHX files: folders separated by `;` (or the platform's
/// path separator). Empty clears it.
pub fn set_search_path(list: &str) {
    let dirs: Vec<PathBuf> = list
        .split(';')
        .flat_map(|p| if cfg!(any(windows, target_arch = "wasm32")) { vec![PathBuf::from(p)] } else { std::env::split_paths(p).collect() })
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

/// Most times [`find`] looks again because the folders or guards changed while it read.
const MAX_LOOKUPS: usize = 4;

/// The SHX font file a style or linetype names, if CADCraft can find and read it.
///
/// A font read while what the search may open changed is never given: the read could have used a
/// folder or guard since dropped (a folder whose new guard refuses the file). The search runs again
/// under the new ones, and a table that keeps changing gives no font.
pub fn find(name: &str) -> Option<Arc<ShxFont>> {
    (0..MAX_LOOKUPS).find_map(|_| lookup(name)).flatten()
}

/// One search for [`find`]: `None` when the table changed while the file was read.
fn lookup(name: &str) -> Option<Option<Arc<ShxFont>>> {
    let Some(mut k) = key(name) else { return Some(None) };
    let (provider, folders, (guarded, overflow), generation) = {
        let t = table();
        // The font map renames once (a map can't loop).
        if let Some(to) = t.map.iter().find(|(from, _)| *from == k).and_then(|(_, to)| key(to)) {
            k = to;
        }
        if let Some(f) = t.registered.get(&k) {
            return Some(Some(f.clone()));
        }
        if let Some(r) = t.resolved.get(&k) {
            return Some(r.clone());
        }
        let folders: Vec<PathBuf> = t.drawing_dirs.iter().chain(t.font_dirs.iter()).chain(t.search_path.iter()).cloned().collect();
        let guarded: Vec<(PathBuf, Vec<OpenGuard>)> =
            t.drawing_guards.iter().map(|(d, g)| (d.clone(), g.iter().map(|(_, g)| g.clone()).collect())).collect();
        let mut overflow: Vec<OpenGuard> = t.overflow_guards.iter().map(|(_, g)| g.clone()).collect();
        if t.overflow_closed {
            overflow.push(Arc::new(|_: &Path| false));
        }
        (t.provider.clone(), folders, (guarded, overflow), t.generation)
    };
    // A guard belongs to the folder, however the search reaches it: a font folder or the search path
    // naming the same folder as a drawing's is held to the same guards.
    // Each folder is resolved once per lookup (spelled, and as the file system has it).
    let resolved = |d: &Path| std::fs::canonicalize(d).ok();
    let guarded: Vec<(PathBuf, Option<PathBuf>, Vec<OpenGuard>)> = guarded.into_iter().map(|(d, g)| (d.clone(), resolved(&d), g)).collect();
    let dirs: Vec<(PathBuf, Vec<OpenGuard>)> = folders
        .into_iter()
        .map(|d| {
            let real = resolved(&d);
            let own = guarded.iter().filter(|(g, r, _)| *g == d || (real.is_some() && *r == real)).flat_map(|(_, _, g)| g.iter().cloned());
            let guards = own.chain(overflow.iter().cloned()).collect();
            (d, guards)
        })
        .collect();
    let font = provider
        .and_then(|p| p.load(&k))
        .or_else(|| read_from(&dirs, &k))
        .filter(|b| b.len() <= MAX_FILE_BYTES)
        .and_then(|b| ShxFont::parse(&b).ok())
        .map(Arc::new);
    let mut t = table();
    // Something changed while the file was read: the result may come from a folder or under a
    // guard that no longer applies, so it is neither kept nor given.
    if t.generation != generation {
        return None;
    }
    if t.resolved.len() >= MAX_FONTS * 4 {
        t.resolved.clear();
        t.loaded_bytes = t.registered.values().map(|f| f.data.len()).sum();
    }
    let size = font.as_ref().map_or(0, |f| f.data.len());
    if t.loaded_bytes.saturating_add(size) > MAX_LOADED_BYTES {
        // Over the memory bound: used, not kept.
        return Some(font);
    }
    t.loaded_bytes += size;
    t.resolved.insert(k, font.clone());
    Some(font)
}

/// Most directory entries looked at for a font file whose name differs only in case.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DIR_ENTRIES: usize = 10_000;

/// The SHX fonts in the user's font folders ([`set_font_dirs`]) and the registered ones, by file
/// stem, sorted without repeats (for font lists). Not read: only their names are listed.
pub fn installed() -> Vec<String> {
    let (dirs, mut out): (Vec<PathBuf>, Vec<String>) = {
        let t = table();
        (t.font_dirs.clone(), t.registered.keys().map(|k| k.trim_end_matches(".shx").to_string()).collect())
    };
    #[cfg(not(target_arch = "wasm32"))]
    for d in &dirs {
        let Ok(entries) = std::fs::read_dir(d) else { continue };
        for e in entries.take(MAX_DIR_ENTRIES).filter_map(Result::ok) {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("shx"))
                && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            {
                out.push(stem.to_string());
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = dirs;
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// The file `name` (a lower-case key) in the first of `dirs` that has it, whatever the case of
/// its name: lower-case, upper-case, else any other case found in the directory (`Simplex.shx`
/// on a case-sensitive file system). Each of a folder's guards is asked about each candidate before
/// anything opens it, so a link is judged by where it leads.
#[cfg(not(target_arch = "wasm32"))]
fn read_from(dirs: &[(PathBuf, Vec<OpenGuard>)], name: &str) -> Option<Vec<u8>> {
    for (d, guards) in dirs {
        let read = |file: &Path| -> Option<Vec<u8>> {
            if !guards.iter().all(|g| g(file)) {
                return None;
            }
            read_file(file)
        };
        if let Some(b) = [d.join(name), d.join(name.to_ascii_uppercase())].iter().find_map(|f| read(f)) {
            return Some(b);
        }
        // The folder is listed only when its guards allow the folder itself.
        if !guards.iter().all(|g| g(d)) {
            continue;
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

/// The bytes of the SHX file at `file`, when it is a file of a sensible size.
#[cfg(not(target_arch = "wasm32"))]
fn read_file(file: &Path) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let meta = std::fs::metadata(file).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    std::fs::File::open(file).and_then(|f| f.take(limit).read_to_end(&mut bytes)).ok()?;
    Some(bytes)
}

/// On the web there are no folders to search.
#[cfg(target_arch = "wasm32")]
fn read_from(_dirs: &[(PathBuf, Vec<OpenGuard>)], _name: &str) -> Option<Vec<u8>> {
    None
}
