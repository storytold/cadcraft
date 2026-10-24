//! Durable file replacement for saving drawings (native builds only; the web build has no files).
//!
//! [`write_durable`] replaces the file atomically ([`crate::atomic_write::write_atomic`]: a fresh,
//! exclusively created temporary file, synced and renamed over the target), so the file holds
//! either the old content or the new, never a torn mix. On Unix it then flushes the folder as well,
//! best effort, so the rename also survives a power loss where the file system allows it.
//! [`backup_previous`] keeps the version being replaced as a backup ([`backup_path`]), but only
//! when that version is itself a readable drawing: a damaged file never replaces a good backup.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// Write `bytes` to `path` so that `path` is always either the old or the new content. On Unix the
/// folder is flushed too (best effort: a file system that refuses it leaves the rename unflushed).
pub fn write_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::atomic_write::write_atomic(path, bytes)?;
    sync_parent(path);
    Ok(())
}

/// Flush the folder entry of `path` (Unix); best effort, since some file systems refuse it.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().map(|d| if d.as_os_str().is_empty() { Path::new(".") } else { d })
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The backup file of the drawing at `path`. A DWG's is `<name>.bak`, as AutoCAD names it (rename
/// it back to `.dwg` to open it), in the extension's letter case (`plan.DWG` → `plan.BAK`); any
/// other drawing appends `.bak` (`plan.dxf.bak`). So drawings that differ only in their extension,
/// or in its case where file names are case-sensitive, never share, and overwrite, one backup.
/// None for a path that is itself a `.bak`.
pub fn backup_path(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    match p.extension() {
        Some(e) if e.eq_ignore_ascii_case("bak") => None,
        Some(e) if e.eq_ignore_ascii_case("dwg") => {
            let ext: String = e
                .to_string_lossy()
                .chars()
                .zip("bak".chars())
                .map(|(d, b)| if d.is_ascii_uppercase() { b.to_ascii_uppercase() } else { b })
                .collect();
            Some(p.with_extension(ext))
        }
        _ => Some(PathBuf::from(format!("{path}.bak"))),
    }
}

/// Before `path` is overwritten, keep its current content as `<name>.bak`, provided `is_valid`
/// accepts that content. A missing, unreadable or invalid current file leaves an existing backup
/// untouched. Returns whether a backup was written.
pub fn backup_previous(path: &str, is_valid: impl FnOnce(&[u8]) -> bool) -> io::Result<bool> {
    let Some(bak) = backup_path(path) else { return Ok(false) };
    let old = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if old.is_empty() || !is_valid(&old) {
        return Ok(false);
    }
    write_durable(&bak, &old)?;
    Ok(true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-durable-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_durable_replaces_and_leaves_no_staging_file() {
        let d = dir("durable");
        let p = d.join("a.dxf");
        write_durable(&p, b"one").unwrap();
        write_durable(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        // A failed write (the target is a folder, so the rename fails) leaves nothing behind.
        let blocked = d.join("blocked");
        fs::create_dir(&blocked).unwrap();
        assert!(write_durable(&blocked, b"new").is_err());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn backup_keeps_the_previous_version_only_while_it_is_valid() {
        let d = dir("bak");
        let p = d.join("a.dxf").to_string_lossy().to_string();
        let bak = d.join("a.dxf.bak");
        assert!(!backup_previous(&p, |_| true).unwrap(), "nothing to back up yet");
        fs::write(&p, b"good").unwrap();
        assert!(backup_previous(&p, |b| b == b"good").unwrap());
        assert_eq!(fs::read(&bak).unwrap(), b"good");
        // A damaged current file must not replace the good backup.
        fs::write(&p, b"torn").unwrap();
        assert!(!backup_previous(&p, |b| b == b"good").unwrap());
        fs::write(&p, b"").unwrap();
        assert!(!backup_previous(&p, |_| true).unwrap(), "an empty file is never a backup");
        assert_eq!(fs::read(&bak).unwrap(), b"good");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bak_file_has_no_backup() {
        assert_eq!(backup_path("/x/a.dwg"), Some(PathBuf::from("/x/a.bak")));
        assert_eq!(backup_path("/x/a.DWG"), Some(PathBuf::from("/x/a.BAK")));
        assert_eq!(backup_path("/x/a.Dwg"), Some(PathBuf::from("/x/a.Bak")));
        assert_eq!(backup_path("/x/a.BAK"), None);
    }

    /// Where file names are case-sensitive, `a.dwg` and `a.DWG` are two drawings with two backups.
    #[test]
    fn dwgs_differing_in_case_keep_their_own_backups() {
        let d = dir("dwg-case");
        let (lower, upper) = (d.join("a.dwg"), d.join("a.DWG"));
        fs::write(&lower, b"lower").unwrap();
        if upper.exists() {
            // A case-insensitive file system: there is only one file.
            let _ = fs::remove_dir_all(&d);
            return;
        }
        fs::write(&upper, b"upper").unwrap();
        assert!(backup_previous(&lower.to_string_lossy(), |_| true).unwrap());
        assert!(backup_previous(&upper.to_string_lossy(), |_| true).unwrap());
        assert_eq!(fs::read(d.join("a.bak")).unwrap(), b"lower");
        assert_eq!(fs::read(d.join("a.BAK")).unwrap(), b"upper");
        let _ = fs::remove_dir_all(&d);
    }

    /// Drawings that differ only in their extension keep separate backups.
    #[test]
    fn same_named_drawings_keep_their_own_backups() {
        assert_eq!(backup_path("/x/a.dxf"), Some(PathBuf::from("/x/a.dxf.bak")));
        assert_eq!(backup_path("/x/a.dwt"), Some(PathBuf::from("/x/a.dwt.bak")));
        assert_eq!(backup_path("/x/a"), Some(PathBuf::from("/x/a.bak")));
        let d = dir("same-name");
        let (dwg, dxf) = (d.join("a.dwg").to_string_lossy().to_string(), d.join("a.dxf").to_string_lossy().to_string());
        fs::write(&dwg, b"dwg v1").unwrap();
        fs::write(&dxf, b"dxf v1").unwrap();
        assert!(backup_previous(&dwg, |_| true).unwrap());
        assert!(backup_previous(&dxf, |_| true).unwrap());
        assert_eq!(fs::read(d.join("a.bak")).unwrap(), b"dwg v1");
        assert_eq!(fs::read(d.join("a.dxf.bak")).unwrap(), b"dxf v1");
        let _ = fs::remove_dir_all(&d);
    }
}
