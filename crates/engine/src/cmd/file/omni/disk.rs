//! The history folder: where versions live, how they are compressed, and how a version becomes a
//! file on disk again.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde_json::Value;

use crate::cmd::{bad, str_param};
use crate::{Result, Session};

use super::super::store;
use super::HISTORY_DIR;
use super::name::{COMPRESSED_EXT, DRAWING_EXTS, Snap, ext_of, is_snapshot, parse};

/// Where versions are unpacked when something needs them as a file (the overlay reference) —
/// inside the history folder, so it travels with it and never pollutes the drawing's own folder.
const CACHE_DIR: &str = ".cache";
/// The largest a single version may unpack to: a history file is input, and input is hostile
/// (a gzip bomb costs memory before anything else can refuse it).
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
/// Versions read from a folder in one go; a folder with more says so.
pub(super) const MAX_SNAPSHOTS: usize = 2000;
/// The folder a drawing's snapshots live in.
pub(super) fn history_dir(folder: &Path) -> PathBuf {
    folder.join(HISTORY_DIR)
}

/// The folder a file is in (`.` when it names no folder), so a bare file name works everywhere.
pub(super) fn folder_of(p: &Path) -> PathBuf {
    match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// The drawing a version file belongs to. A version lives in `<folder>/_OMNI_HISTORY` (or in its
/// `.cache`), so opening an old revision keeps browsing the timeline it came from instead of
/// offering the snapshot a history of its own inside the history folder.
fn history_host(file: &Path) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy().into_owned();
    if !is_snapshot(&name) {
        return None;
    }
    let parent = file.parent()?;
    let history = match parent.file_name()?.to_string_lossy() {
        d if d.eq_ignore_ascii_case(HISTORY_DIR) => parent.to_path_buf(),
        d if d.eq_ignore_ascii_case(CACHE_DIR) => parent.parent()?.to_path_buf(),
        _ => return None,
    };
    let drawing = parse(&name, file.to_path_buf(), 0).drawing;
    (!drawing.is_empty()).then(|| history.parent().unwrap_or(parent).join(drawing))
}

/// The drawing file a history belongs to: the open drawing's own file, or `path` when it has none
/// yet. A file that is neither DXF nor DWG is refused rather than guessed at.
pub(super) fn drawing_file(s: &Session, cmd: &str, p: &Value) -> Result<PathBuf> {
    let path = str_param(p, "path")
        .map(str::to_string)
        .or_else(|| s.state().ok().and_then(|st| st.path.clone()))
        .ok_or_else(|| bad(cmd, "the drawing has no file name yet: pass `path`, the DXF or DWG file this history belongs to"))?;
    let file = PathBuf::from(path.trim());
    let Some(name) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(bad(cmd, format!("`path` must name a file, not \"{}\"", path.trim())));
    };
    // The extension a version is *read* as: a stored one carries the compression suffix on top.
    let plain = name.strip_suffix(&format!(".{COMPRESSED_EXT}")).unwrap_or(&name);
    if !DRAWING_EXTS.contains(&ext_of(Path::new(plain)).as_str()) {
        return Err(bad(cmd, format!("\"{}\" is not a DXF or DWG drawing", file.display())));
    }
    if let Some(host) = history_host(&file) {
        return Ok(host);
    }
    Ok(file)
}

/// The snapshots in `dir`, newest first — the name starts with a zero-padded UTC stamp, so a plain
/// name sort is a chronological one. Returns whether the folder holds more than [`MAX_SNAPSHOTS`].
///
/// Every version in the folder is collected before the sort: `read_dir` hands its entries over in
/// no defined order, so capping the loop itself would keep an arbitrary [`MAX_SNAPSHOTS`] of them
/// and the "newest first" list, the `1` that OMNI-RESTORE and OMNI-DIFF mean by it, and the
/// duplicate check in the save's version would all point at the wrong end of the timeline. The
/// names are cheap to hold — nothing is read until a version is asked for.
pub(super) fn snapshots(dir: &Path) -> (Vec<Snap>, bool) {
    let mut out: Vec<Snap> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            if !is_snapshot(&file) {
                continue;
            }
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(parse(&file, dir.join(&file), bytes));
        }
    }
    // Newest first by the moment in the name, not by the name itself: a file in the folder whose
    // name carries no stamp (`notes.dxf`, a rename) has no time to sort by, and text order would
    // put it before every version — where `1`, OMNI-DIFF's default, and the duplicate check in the
    // save's version would all pick it as the newest. It goes last instead.
    out.sort_by(|a, b| match (a.epoch, b.epoch) {
        (Some(x), Some(y)) => y.cmp(&x).then_with(|| b.millis.cmp(&a.millis)).then_with(|| b.file.cmp(&a.file)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.file.cmp(&a.file),
    });
    let capped = out.len() > MAX_SNAPSHOTS;
    out.truncate(MAX_SNAPSHOTS);
    (out, capped)
}

/// A version's bytes, gzipped: a DXF is verbose text, so this is roughly a tenth of the size.
pub(super) fn gzip(cmd: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(bytes).and_then(|()| e.finish()).map_err(|e| bad(cmd, format!("compressing the version: {e}")))
}

/// A version's bytes as they went in. Bounded: a version file is input, and input is hostile.
fn gunzip(cmd: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut d = GzDecoder::new(bytes).take(MAX_UNPACKED);
    d.read_to_end(&mut out).map_err(|e| bad(cmd, format!("unpacking the version: {e}")))?;
    Ok(out)
}

/// The bytes of a version, unpacked when it is stored gzipped.
pub(super) fn read_snap(snap: &Snap, cmd: &str) -> Result<Vec<u8>> {
    let raw = std::fs::read(&snap.path).map_err(|e| bad(cmd, format!("{}: {e}", snap.path.display())))?;
    if snap.compressed { gunzip(cmd, &raw) } else { Ok(raw) }
}

/// The version as a file on disk, unpacking it into the history's cache when it is stored
/// compressed — the overlay reference reads a snapshot back through the io hooks, which take a
/// path. Unpacked once and reused: the cache file is named after the version, so it never goes
/// stale, and it lives inside the history folder so it travels with it.
pub(super) fn materialize(snap: &Snap, cmd: &str) -> Result<PathBuf> {
    if !snap.compressed {
        return Ok(snap.path.clone());
    }
    let dir = snap.path.parent().ok_or_else(|| bad(cmd, format!("{}: no folder", snap.path.display())))?.join(CACHE_DIR);
    let out = dir.join(&snap.inner);
    if out.is_file() {
        return Ok(out);
    }
    let bytes = read_snap(snap, cmd)?;
    std::fs::create_dir_all(&dir).map_err(|e| bad(cmd, format!("{}: {e}", dir.display())))?;
    store(&out.to_string_lossy(), &bytes)?;
    Ok(out)
}

/// A size for the command line's listing.
pub(super) fn size(n: u64) -> String {
    match n {
        n if n < 10_000 => format!("{n} B"),
        n if n < 10_000_000 => format!("{:.1} kB", n as f64 / 1000.0),
        n => format!("{:.1} MB", n as f64 / 1_000_000.0),
    }
}

/// The history folder, its snapshots and whether the folder holds more than [`MAX_SNAPSHOTS`].
pub(super) fn history_of(s: &Session, cmd: &str, p: &Value) -> Result<(PathBuf, Vec<Snap>, bool)> {
    let file = drawing_file(s, cmd, p)?;
    let dir = history_dir(&folder_of(&file));
    let (list, capped) = snapshots(&dir);
    Ok((dir, list, capped))
}

/// The path an overlay stores for `snap`: relative to the drawing's folder when the snapshot is
/// inside its history folder or the folder's cache, written with `/` so the drawing resolves it
/// on every platform.
pub(super) fn overlay_path(host: &Path, snap: &Path) -> String {
    let dir = history_dir(&folder_of(host));
    if let Ok(rel) = snap.strip_prefix(&dir) {
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.is_empty() {
            return format!("{HISTORY_DIR}/{rel}");
        }
    }
    snap.to_string_lossy().into_owned()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_paths_are_relative_to_the_drawing_and_use_slashes() {
        // The paths OMNI-SNAP builds: the history folder next to the drawing, one file in it.
        let host = Path::new("jobs/site/plan.dxf");
        let snap = history_dir(&folder_of(host)).join("20261014.083512_482_u_d_plan.dxf");
        assert_eq!(overlay_path(host, &snap), format!("{HISTORY_DIR}/20261014.083512_482_u_d_plan.dxf"));
        // A drawing opened by a bare file name has a folder too (`.`, as everywhere else).
        let bare = Path::new("plan.dxf");
        let here = history_dir(&folder_of(bare)).join("a.dxf");
        assert_eq!(overlay_path(bare, &here), format!("{HISTORY_DIR}/a.dxf"));
        // Anywhere else the snapshot is named by its full path: only this drawing's own history
        // folder is stored relative (another drawing's is not this drawing's to find).
        let other = PathBuf::from("elsewhere/plan.dxf");
        assert_eq!(overlay_path(host, &other), other.to_string_lossy());
        let theirs = PathBuf::from("other").join(HISTORY_DIR).join("plan.dxf");
        assert_eq!(overlay_path(host, &theirs), theirs.to_string_lossy());
    }

    #[test]
    fn a_file_without_a_stamp_is_never_the_newest_version() {
        let dir = std::env::temp_dir().join(format!("cadcraft-omni-stray-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // One version, and two drawings someone put in the folder (or renamed). Text order alone
        // would list them first — and `1`, OMNI-DIFF's default, would then compare against one.
        std::fs::write(dir.join("20261014.083512_482_u_save_plan.dxf.gz"), b"").unwrap();
        std::fs::write(dir.join("notes.dxf"), b"").unwrap();
        std::fs::write(dir.join("zzz.dxf"), b"").unwrap();
        let (list, _) = snapshots(&dir);
        assert_eq!(
            list.first().map(|s| s.file.as_str()),
            Some("20261014.083512_482_u_save_plan.dxf.gz"),
            "{:?}",
            list.iter().map(|s| &s.file).collect::<Vec<_>>()
        );
        assert_eq!(list.last().map(|s| s.file.as_str()), Some("notes.dxf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_past_the_limit_still_starts_with_the_newest_version() {
        let dir = std::env::temp_dir().join(format!("cadcraft-omni-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // One version more than a listing holds. The names sort the way versions do (a stamp
        // first), and the loop writes them oldest first, so a cap applied while reading would keep
        // the oldest [`MAX_SNAPSHOTS`] and lose the newest.
        for i in 0..=MAX_SNAPSHOTS {
            let name = format!("{i:08}_000_u_save_plan.dxf.gz");
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let (list, capped) = snapshots(&dir);
        assert!(capped, "the folder holds more than the listing does");
        assert_eq!(list.len(), MAX_SNAPSHOTS);
        let newest = format!("{MAX_SNAPSHOTS:08}_000_u_save_plan.dxf.gz");
        assert_eq!(list.first().map(|s| s.file.as_str()), Some(newest.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
