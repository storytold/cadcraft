//! Where a file named without a full path is looked for, in AutoCAD's order: the current folder,
//! the folder of the current drawing, then the folders of the support file search path.
//!
//! The search path is the one SHX fonts are found by (`SUPPORTPATH` / `ACADPREFIX`, or
//! `CADCRAFT_SHXPATH` at start): one setting for every support file, so a headless run sets it
//! once. An absolute path is used as given.

use std::path::PathBuf;

use crate::Session;

/// The paths the file `name` may be at, in the order to try them ([`candidates_of`]).
pub fn candidates(s: &Session, name: &str) -> Vec<PathBuf> {
    candidates_of(s, &[name])
}

/// The paths any of `names` (one file under several names: with and without an extension) may be
/// at: as given (an absolute path, or relative to the current folder), then, for a relative name,
/// under the active drawing's folder and under each support search path folder. Every name is
/// tried in one folder before the next folder.
pub fn candidates_of<S: AsRef<str>>(s: &Session, names: &[S]) -> Vec<PathBuf> {
    let drawing_dir = s.state().ok().and_then(|d| d.path.as_deref()).and_then(|p| std::path::absolute(p).ok());
    let mut folders: Vec<PathBuf> = drawing_dir.and_then(|p| p.parent().map(PathBuf::from)).into_iter().collect();
    folders.extend(cadcraft_fonts::shx::search_dirs());
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    for n in names.iter().map(AsRef::as_ref).filter(|n| !n.is_empty()) {
        push(PathBuf::from(n));
    }
    for dir in &folders {
        for n in names.iter().map(AsRef::as_ref).filter(|n| !n.is_empty()) {
            let given = PathBuf::from(n);
            if !given.is_absolute() && !given.has_root() {
                push(dir.join(given));
            }
        }
    }
    out
}

/// The first existing file `name` names ([`candidates`]); `None` when there is none (always on
/// the web, which has no file system).
pub fn find(s: &Session, name: &str) -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        candidates(s, name).into_iter().find(|p| p.is_file())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, name);
        None
    }
}
