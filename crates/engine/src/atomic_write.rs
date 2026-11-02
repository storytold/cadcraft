//! Safe, atomic file replacement.
//!
//! Save, export, plot, autosave and the audit log all write a file by first putting the bytes in
//! a sibling temporary file and then renaming it over the destination, so a failed or partial
//! write never truncates the real file. The temporary file must be created *exclusively*: opening
//! a predictable, pre-existing temporary path for a truncating write lets another local user plant
//! a symlink there and redirect the write onto a file the victim can write but the attacker chose
//! (CWE-59). [`write_atomic`] creates a fresh temporary file with `create_new` (`O_CREAT|O_EXCL`),
//! so an existing entry at that path — symlink or not — makes the create fail rather than follow
//! it. The handle is retained for the whole write, and only its own fresh file is renamed into
//! place.

#![cfg(not(target_arch = "wasm32"))]

use std::fs::{File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter making temporary names unique within this process without a RNG dependency.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// How many fresh temporary names to try before giving up (each collision is another local race).
const MAX_TRIES: u32 = 32;

/// Write `bytes` to `path`, replacing it atomically and without ever following a pre-existing
/// temporary symlink.
///
/// A fresh temporary file is created with exclusive semantics in `path`'s own directory, the bytes
/// are written through the retained handle and flushed, and the temporary is then renamed over
/// `path`. On any failure the temporary is removed and the original `path` is left untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A symlinked destination is resolved so the link stays a link and its target is replaced.
    let resolved = resolve_symlink(path)?;
    let path = resolved.as_deref().unwrap_or(path);
    let permissions = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Some(meta.permissions()),
        Ok(_) => None,
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    write_atomic_with(path, permissions, |file| file.write_all(bytes))
}

/// [`write_atomic`] filling the temporary file through `fill` (streamed rather than one buffer),
/// with `permissions` given to it from its creation (`None`: the usual creation mode). A symlink
/// at `path` is replaced by the new file, never written through.
pub fn write_atomic_with(path: &Path, permissions: Option<Permissions>, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let (mut file, tmp) = create_temp(&dir, permissions.as_ref())?;
    let write = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        fill(&mut file)?;
        file.flush()?;
        file.sync_all()
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(&dir);
    Ok(())
}

/// Flush `dir` so a rename just made in it survives a crash or power loss (POSIX records a
/// rename in the directory, not in the file). Best effort: some file systems (network shares)
/// refuse to sync a directory, and the file itself is already on disk.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Resolve `path` when it is a symlink, or return `None` if it is not one.
///
/// A dangling link (for example into an unavailable mount) cannot be canonicalized, so its target
/// is followed by hand with `read_link`, joining relative targets onto the link's directory.
fn resolve_symlink(path: &Path) -> io::Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {}
        _ => return Ok(None),
    }
    if let Ok(target) = std::fs::canonicalize(path) {
        return Ok(Some(target));
    }
    let mut current = path.to_path_buf();
    for _ in 0..40 {
        let target = std::fs::read_link(&current)?;
        current = if target.is_absolute() { target } else { current.parent().map_or(target.clone(), |p| p.join(&target)) };
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {}
            Ok(_) => return finish_target(&current),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return finish_target(&current),
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "too many levels of symbolic links"))
}

/// Build the write path for a resolved, non-link (possibly missing) target.
fn finish_target(current: &Path) -> io::Result<Option<PathBuf>> {
    // The target's directory must exist; otherwise report the error rather than replacing the link.
    let parent = current.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let parent = std::fs::canonicalize(parent)?;
    let name = current.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "symlink target has no file name"))?;
    Ok(Some(parent.join(name)))
}

/// Create a fresh, exclusively-owned temporary file inside `dir`, returning its handle and path.
///
/// `create_new(true)` opens with `O_CREAT | O_EXCL`, which refuses to follow or reuse an existing
/// entry (including a symlink), closing the plant-a-symlink race. Names are unpredictable enough to
/// avoid collisions in practice, and genuine collisions are retried with a new name.
fn create_temp(dir: &Path, permissions: Option<&Permissions>) -> io::Result<(File, PathBuf)> {
    let pid = std::process::id();
    let mut last_err = None;
    for _ in 0..MAX_TRIES {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let tmp = dir.join(format!(".cadcraft-tmp-{pid}-{seq}-{nanos}"));
        match create_exclusive(&tmp, permissions) {
            Ok(file) => return Ok((file, tmp)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "could not create a unique temporary file")))
}

fn create_exclusive(path: &Path, permissions: Option<&Permissions>) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(permissions) = permissions {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // Restrict access in the create syscall: chmod afterwards leaves a window in which a
        // different user can open the inode and retain read access to bytes written later.
        options.mode(permissions.mode() & 0o777);
    }
    #[cfg(not(unix))]
    let _ = permissions;
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_contents_atomically() {
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out.bin");
        std::fs::write(&dest, b"old").unwrap();
        write_atomic(&dest, b"new bytes").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new bytes");
        // No temporary files left behind.
        let leftovers: Vec<_> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).filter(|n| n.to_string_lossy().starts_with(".cadcraft-tmp")).collect();
        assert!(leftovers.is_empty(), "left temp files: {leftovers:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A write into a folder whose entries can be listed syncs that folder without failing, and
    /// a folder that can't be opened (gone) is no error for the sync itself.
    #[test]
    fn the_folder_is_synced_after_the_rename() {
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-sync-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        write_atomic(&dir.join("a.bin"), b"x").unwrap();
        assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), b"x");
        sync_dir(&dir.join("missing"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_planted_temp_symlink() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-sym-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim");
        std::fs::write(&victim, b"precious").unwrap();
        let planted = dir.join(".cadcraft-tmp-plant");
        symlink(&victim, &planted).unwrap();
        assert_eq!(create_exclusive(&planted, None).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        let dest = dir.join("drawing.dxf");
        write_atomic(&dest, b"drawing bytes").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"precious", "victim file was overwritten");
        assert_eq!(std::fs::read(&dest).unwrap(), b"drawing bytes");
        std::fs::remove_dir_all(&dir).ok();
    }
    #[cfg(unix)]
    #[test]
    fn replacement_preserves_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-mode-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("private.adt");
        std::fs::write(&dest, b"old").unwrap();
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&dest, b"private report").unwrap();
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read(&dest).unwrap(), b"private report");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_destination_updates_the_target() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-link-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.dxf");
        let link = dir.join("link.dxf");
        std::fs::write(&real, b"old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&real, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "symlink was replaced by a regular file");
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_destination_creates_the_target() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-dangle-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(dir.join("mnt")).unwrap();
        let real = dir.join("mnt").join("real.dxf");
        let link = dir.join("link.dxf");
        symlink("mnt/real.dxf", &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "symlink was replaced by a regular file");
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_with_missing_parent_errors_and_keeps_the_link() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-nomnt-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("link.dxf");
        symlink("missing/real.dxf", &link).unwrap();
        assert!(write_atomic(&link, b"new").is_err());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_temp_is_restricted_from_creation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cadcraft-aw-create-mode-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("private-temp");
        let permissions = Permissions::from_mode(0o600);
        let file = create_exclusive(&path, Some(&permissions)).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
        drop(file);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
