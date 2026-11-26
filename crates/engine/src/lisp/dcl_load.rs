//! Read DCL files for `load_dialog`.
//!
//! [`super::dcl`] parses one string and records `@include` paths. It does not open files. This
//! module reads regular files, resolves includes, and rejects a cycle or a tree that would retain
//! more text than one parser input.

use std::path::{Path, PathBuf};

use super::LispError;
#[cfg(not(target_arch = "wasm32"))]
use super::dcl;
use super::dcl::Dcl;
#[cfg(not(target_arch = "wasm32"))]
use super::value::charge_bytes;

/// `load_dialog` returns this negative integer when it cannot load the file.
pub const LOAD_FAILED: i32 = -1;

/// Most DCL handles one drawing keeps. One load cannot retain more source text than one parser input.
pub const MAX_DIALOGS: usize = 32;

/// Most DCL text one load retains, including every included file.
///
/// The cap is [`dcl::MAX_INPUT_BYTES`], so a wide include list cannot multiply that per-file limit.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_LOAD_BYTES: usize = dcl::MAX_INPUT_BYTES;

/// Most root and include requests per load, including repeated paths.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_FILES: usize = 64;

/// Most files in one include chain. Stops a long chain from recursing when it is not a cycle.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_INCLUDE_DEPTH: usize = 32;

/// One file that was parsed while loading a dialog.
pub struct LoadedFile {
    /// Canonical path of the file, used to avoid reading it twice.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub path: PathBuf,
    /// Parsed tiles and include records, in source order.
    #[allow(dead_code)] // retained for the dialog commands that follow this load
    pub parsed: Dcl,
}

/// The files retained for one positive `load_dialog` handle.
pub struct LoadedDialog {
    /// Root file first, then each included file. Dialog commands read these tiles.
    #[allow(dead_code)] // retained for the dialog commands that follow this load
    pub files: Vec<LoadedFile>,
}

/// Load `name` from the working folder, then `support`. On the web there is no native lookup.
#[cfg(target_arch = "wasm32")]
pub fn load_named(_name: &str, _support: Option<&Path>) -> Result<Option<LoadedDialog>, LispError> {
    Ok(None)
}

/// Load `name` from the working folder, then `support`.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_named(name: &str, support: Option<&Path>) -> Result<Option<LoadedDialog>, LispError> {
    if name.is_empty() || name.contains('\0') {
        return Ok(None);
    }
    let Some(root) = find_root(name, support) else { return Ok(None) };
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut load = Load { cwd: &cwd, budget: MAX_LOAD_BYTES, include_requests: MAX_FILES - 1, files: Vec::new() };
    match load.read(&root, MAX_INCLUDE_DEPTH) {
        Ok(_) => Ok(Some(LoadedDialog { files: load.files.into_iter().map(|entry| entry.file).collect() })),
        Err(LoadError::Rejected) => Ok(None),
        Err(LoadError::RunBudget(error)) => Err(error),
    }
}

#[cfg(not(target_arch = "wasm32"))]
enum LoadError {
    Rejected,
    RunBudget(LispError),
}

#[cfg(not(target_arch = "wasm32"))]
struct LoadingFile {
    file: LoadedFile,
    /// Longest path from this file to a leaf, counting this file. None while on the include stack.
    height: Option<usize>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Load<'a> {
    cwd: &'a Path,
    budget: usize,
    /// How many include requests can still be resolved.
    include_requests: usize,
    files: Vec<LoadingFile>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Load<'_> {
    /// Read this file and its includes. Return the longest path length if it fits `height_limit`.
    fn read(&mut self, path: &Path, height_limit: usize) -> Result<usize, LoadError> {
        let child_limit = height_limit.checked_sub(1).ok_or(LoadError::Rejected)?;
        let canonical = std::fs::canonicalize(path).map_err(|_| LoadError::Rejected)?;
        if let Some(entry) = self.files.iter().find(|entry| entry.file.path == canonical) {
            // An unfinished entry is a cycle. A finished subtree must fit this path as well.
            return entry.height.filter(|&height| height <= height_limit).ok_or(LoadError::Rejected);
        }
        let parsed = dcl::parse(&read_text(&canonical, &mut self.budget)?).map_err(|_| LoadError::Rejected)?;
        let includes = include_paths(&parsed, self.include_requests);
        self.include_requests = self.include_requests.checked_sub(includes.len()).ok_or(LoadError::Rejected)?;
        let index = self.files.len();
        self.files.push(LoadingFile { file: LoadedFile { path: canonical.clone(), parsed }, height: None });
        let mut height = 1;
        for spec in includes {
            let child = resolve_include(&spec, &canonical, self.cwd).ok_or(LoadError::Rejected)?;
            height = height.max(1 + self.read(&child, child_limit)?);
        }
        self.files.get_mut(index).ok_or(LoadError::Rejected)?.height = Some(height);
        Ok(height)
    }
}

/// Use the working folder, then the Lisp support folder.
#[cfg(not(target_arch = "wasm32"))]
fn find_root(name: &str, support: Option<&Path>) -> Option<PathBuf> {
    let name = Path::new(name);
    let requested = if name.extension().is_none() { name.with_extension("dcl") } else { name.to_path_buf() };
    if is_readable_file(&requested) {
        return Some(requested);
    }
    if requested.is_absolute() {
        return None;
    }
    let candidate = support?.join(requested);
    is_readable_file(&candidate).then_some(candidate)
}

/// Look up the exact include name in the working folder, then beside the including file.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_include(spec: &str, including: &Path, cwd: &Path) -> Option<PathBuf> {
    if spec.is_empty() || spec.contains('\0') {
        return None;
    }
    let requested = Path::new(spec);
    if requested.is_absolute() {
        return is_readable_file(requested).then(|| requested.to_path_buf());
    }
    let from_cwd = cwd.join(requested);
    if is_readable_file(&from_cwd) {
        return Some(from_cwd);
    }
    let from_parent = including.parent()?.join(requested);
    is_readable_file(&from_parent).then_some(from_parent)
}

#[cfg(not(target_arch = "wasm32"))]
fn is_readable_file(path: &Path) -> bool {
    crate::file_access::check_read(path).is_ok() && path.is_file()
}

#[cfg(not(target_arch = "wasm32"))]
fn read_text(path: &Path, budget: &mut usize) -> Result<String, LoadError> {
    crate::file_access::check_read(path).map_err(|_| LoadError::Rejected)?;
    let mut bytes = Vec::new();
    let read = crate::capped_read::read_capped_into(path, *budget, &mut bytes);
    charge_bytes(bytes.len()).map_err(LoadError::RunBudget)?;
    read.map_err(|_| LoadError::Rejected)?;
    *budget -= bytes.len();
    String::from_utf8(bytes).map_err(|_| LoadError::Rejected)
}

#[cfg(not(target_arch = "wasm32"))]
fn include_paths(parsed: &Dcl, limit: usize) -> Vec<String> {
    parsed
        .items
        .iter()
        .filter_map(|item| match item {
            dcl::Item::Include { path, .. } => Some(path.clone()),
            dcl::Item::Definition(_) => None,
        })
        .take(limit + 1)
        .collect()
}
