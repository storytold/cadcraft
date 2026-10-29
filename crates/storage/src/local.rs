//! The local-filesystem backend: a folder on disk, a NAS mount, or any synced folder.
//!
//! Writes with [`WriteOptions::atomic`][crate::WriteOptions::atomic] follow the engine's own
//! `safe_save` shape: stage a sibling temporary file (private `0600` while it holds bytes the
//! destination may not have had), `fsync` it, journal the pending commit, rename over the
//! destination, then journal the commit. A crash between the lines is finished or dropped by
//! [`recover`][crate::StorageBackend::recover]. Non-atomic writes go straight to the final name
//! (parents created first).
//!
//! A file's ETag is a hash of its current bytes, so a change made outside this backend (a sync
//! client, another tool on the NAS) is seen by the next conditional write. Every write holds a
//! per-document lock file (`.cadcraft-lock-<name>`, created exclusively) across the `if_match`
//! check and the commit, so two writers going through this backend cannot both pass the check.
//! The lock is cooperative: a program that ignores it can still replace the file between the
//! check and the rename, and only a server-side compare-and-swap (WebDAV `If-Match`) closes that.
//! The lock is a lease: its holder renews it (touches it) every [`LOCK_RENEW`] for as long as the
//! write runs, so only a lock nobody has renewed for [`LOCK_STALE`] (a crashed writer's residue)
//! is taken over. Takeover renames the stale file aside (one taker wins) and backs off if it was
//! renewed meanwhile; the lock file names its owner, the writer re-checks that it still owns the
//! lock right before the commit, and release removes the file only while it is still the owner's.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::journal::{self, JournalEntry, JournalState};
use crate::{Capabilities, Entry, FileMeta, RecoveryReport, StorageBackend, StorageError, WriteOptions, clean_path};

/// Prefix of a staged write's temporary file (beside its document).
const STAGE_PREFIX: &str = ".cadcraft-save-";
/// Prefix of a document's writer lock file (beside its document).
const LOCK_PREFIX: &str = ".cadcraft-lock-";
/// How long a writer waits for another writer's lock before reporting a conflict.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// A lock file not renewed for this long belongs to a writer that crashed; it is taken over.
pub const LOCK_STALE: Duration = Duration::from_secs(30);
/// How often a writer renews the lock it holds (well inside [`LOCK_STALE`]).
const LOCK_RENEW: Duration = if cfg!(test) { Duration::from_millis(50) } else { Duration::from_secs(5) };

/// A folder on disk holding documents. `root` must be (or become) a directory.
#[derive(Clone, Debug)]
pub struct LocalBackend {
    root: PathBuf,
}

impl LocalBackend {
    /// Open (creating) the folder at `root`.
    pub fn open(root: &Path) -> Result<LocalBackend, StorageError> {
        if let Ok(meta) = fs::symlink_metadata(root)
            && !meta.is_dir()
        {
            return Err(StorageError::BadPath(format!("{}: not a directory", root.display())));
        }
        fs::create_dir_all(root).map_err(|e| StorageError::Io(format!("{}: {e}", root.display())))?;
        Ok(LocalBackend { root: root.to_path_buf() })
    }

    /// The folder this backend stores in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn journal_path(&self) -> PathBuf {
        self.root.join(journal::JOURNAL_FILE)
    }

    fn append_journal(&self, entry: &JournalEntry) {
        // The journal is best-effort crash evidence, never a reason to fail a save: a lost line
        // only means recovery has one fewer hint. (Errors are ignored deliberately.)
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(self.journal_path()) {
            let _ = f.write_all(journal::line(entry).as_bytes());
        }
    }

    fn meta_of(&self, rel: &str, dest: &Path) -> Result<FileMeta, StorageError> {
        let meta = fs::metadata(dest).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(rel.into())
            } else {
                StorageError::Io(format!("{}: {e}", dest.display()))
            }
        })?;
        if !meta.is_file() {
            return Err(StorageError::NotFound(rel.into()));
        }
        let modified = meta.modified().ok().map(|t| format!("{t:?}"));
        Ok(FileMeta { size: meta.len(), etag: current_etag(dest), modified, is_dir: false })
    }

    /// Stage `bytes` beside `dest`, journal the pending write, rename into place, journal the
    /// commit. The caller holds the document's [`PathLock`], re-checked right before the rename.
    fn atomic_commit(&self, rel: &str, dest: &Path, bytes: &[u8], lock: &PathLock) -> Result<FileMeta, StorageError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| StorageError::Io(format!("{}: {e}", parent.display())))?;
        }
        let keep_mode = match fs::metadata(dest) {
            Ok(m) if m.is_file() => Some(m.permissions()),
            Ok(_) => return Err(StorageError::Conflict(format!("{rel}: a folder is stored there"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(StorageError::Io(format!("{}: {e}", dest.display()))),
        };
        let fname = dest.file_name().and_then(|n| n.to_str()).unwrap_or("doc");
        for _ in 0..128 {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let tmp_name = format!("{STAGE_PREFIX}{id}-{fname}.tmp");
            let tmp = dest.with_file_name(&tmp_name);
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                // Stay private until the destination's own mode is applied below.
                opts.mode(0o600);
            }
            let mut f = match opts.open(&tmp) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(StorageError::Io(format!("{}: {e}", tmp.display()))),
            };
            // Journal the staged file relative to the root (beside `rel`), so recovery finds
            // staged writes in subfolders too.
            let staged = match rel.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/{tmp_name}"),
                None => tmp_name.clone(),
            };
            let entry = JournalEntry { path: rel.into(), staged, state: JournalState::Pending, len: bytes.len() as u64 };
            if f.write_all(bytes).is_err() || f.sync_all().is_err() {
                let _ = fs::remove_file(&tmp);
                return Err(StorageError::Io(format!("{}: could not stage the write", dest.display())));
            }
            if let Some(mode) = keep_mode {
                // A replaced file keeps its mode; a new one keeps the umask default.
                if f.set_permissions(mode).is_err() {
                    let _ = fs::remove_file(&tmp);
                    return Err(StorageError::Io(format!("{}: could not stage the write", dest.display())));
                }
            }
            drop(f);
            if let Err(e) = lock.still_held(rel) {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
            self.append_journal(&entry);
            if let Err(e) = fs::rename(&tmp, dest) {
                let _ = fs::remove_file(&tmp);
                return Err(StorageError::Io(format!("{}: {e}", dest.display())));
            }
            // The rename is durable only once the directory entry is: until then the journal
            // keeps the write pending, and recovery confirms it from the final file.
            if sync_parent(dest) {
                self.append_journal(&JournalEntry { state: JournalState::Committed, ..entry });
            }
            return Ok(FileMeta { size: bytes.len() as u64, etag: Some(etag_of(bytes)), modified: None, is_dir: false });
        }
        Err(StorageError::Io(format!("{}: no free staging name after 128 tries", dest.display())))
    }
}

/// Exclusive writer lease beside one document: renewed while held, released on drop.
struct PathLock {
    path: PathBuf,
    /// This writer's token, the lock file's contents.
    owner: String,
    /// Stops the renewal thread (flag + wake-up).
    stop: Arc<(Mutex<bool>, Condvar)>,
    /// Set by the renewal thread when the file is no longer this writer's.
    lost: Arc<AtomicBool>,
    renew: Option<JoinHandle<()>>,
}

impl Drop for PathLock {
    fn drop(&mut self) {
        let (flag, cv) = &*self.stop;
        if let Ok(mut stopped) = flag.lock() {
            *stopped = true;
            cv.notify_all();
        }
        if let Some(h) = self.renew.take() {
            let _ = h.join();
        }
        // Remove the file only while it is still ours: a writer that took over a lock we failed
        // to renew must keep it.
        if lock_owner(&self.path).as_deref() == Some(self.owner.as_str()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Who holds the lock at `path` (its token), if anyone.
fn lock_owner(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Whether the lock file at `path` has gone [`LOCK_STALE`] without renewal.
fn lock_is_stale(path: &Path) -> bool {
    fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > LOCK_STALE)
}

impl PathLock {
    /// Take `dest`'s writer lock, waiting up to [`LOCK_WAIT`] for another writer and taking over
    /// a lock nobody has renewed for [`LOCK_STALE`].
    fn acquire(rel: &str, dest: &Path) -> Result<PathLock, StorageError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| StorageError::Io(format!("{}: {e}", parent.display())))?;
        }
        let fname = dest.file_name().and_then(|n| n.to_str()).unwrap_or("doc");
        let path = dest.with_file_name(format!("{LOCK_PREFIX}{fname}"));
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let owner = format!("{}-{nanos}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let start = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    if f.write_all(owner.as_bytes()).and_then(|()| f.sync_all()).is_err() {
                        let _ = fs::remove_file(&path);
                        return Err(StorageError::Io(format!("{}: could not write the lock", path.display())));
                    }
                    return Ok(PathLock::hold(path, owner, f));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        Self::take_over_stale(&path, &owner);
                        continue;
                    }
                    if start.elapsed() > LOCK_WAIT {
                        return Err(StorageError::Conflict(format!("{rel}: another writer holds {}", path.display())));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(StorageError::Io(format!("{}: {e}", path.display()))),
            }
        }
    }

    /// Move a stale lock out of the way. The rename is atomic, so of several writers seeing the
    /// same stale lock only one moves it; if its holder renewed it in between, it goes back.
    fn take_over_stale(path: &Path, owner: &str) {
        let aside = path.with_file_name(format!("{}.stale-{owner}", path.file_name().and_then(|n| n.to_str()).unwrap_or("lock")));
        if fs::rename(path, &aside).is_err() {
            return; // Someone else moved it first: try again from the top.
        }
        if !lock_is_stale(&aside) {
            // Renewed between our check and the rename: it is live, so restore it (unless a new
            // lock has appeared meanwhile, which then wins and the old holder's re-check fails).
            let _ = fs::hard_link(&aside, path);
        }
        let _ = fs::remove_file(&aside);
    }

    /// Start renewing a freshly created lock.
    fn hold(path: PathBuf, owner: String, file: File) -> PathLock {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let lost = Arc::new(AtomicBool::new(false));
        let renew = {
            let (stop, lost, path, owner) = (Arc::clone(&stop), Arc::clone(&lost), path.clone(), owner.clone());
            std::thread::Builder::new()
                .name("cadcraft-storage-lock".into())
                .spawn(move || {
                    let (flag, cv) = &*stop;
                    let Ok(mut stopped) = flag.lock() else { return };
                    while !*stopped {
                        let Ok((guard, _)) = cv.wait_timeout(stopped, LOCK_RENEW) else { return };
                        stopped = guard;
                        if *stopped {
                            break;
                        }
                        if lock_owner(&path).as_deref() != Some(owner.as_str()) || file.set_modified(std::time::SystemTime::now()).is_err() {
                            lost.store(true, Ordering::SeqCst);
                        }
                    }
                })
                .ok()
        };
        PathLock { path, owner, stop, lost, renew }
    }

    /// Fail with [`StorageError::Conflict`] unless this writer still owns the lock (checked right
    /// before a commit, so a writer whose lease was taken over cannot rename over the new commit).
    fn still_held(&self, rel: &str) -> Result<(), StorageError> {
        if self.lost.load(Ordering::SeqCst) || lock_owner(&self.path).as_deref() != Some(self.owner.as_str()) {
            return Err(StorageError::Conflict(format!("{rel}: lost the writer lock {} to another writer", self.path.display())));
        }
        Ok(())
    }
}

/// `fsync` the directory holding `dest`, so a rename into it survives power loss. Returns whether
/// that succeeded (always `true` where directories cannot be synced).
fn sync_parent(dest: &Path) -> bool {
    #[cfg(unix)]
    {
        match dest.parent() {
            Some(parent) => File::open(parent).and_then(|d| d.sync_all()).is_ok(),
            None => true,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dest;
        true
    }
}

/// The ETag of whatever is stored at `dest` right now (`None` when it cannot be read).
fn current_etag(dest: &Path) -> Option<String> {
    fs::read(dest).ok().map(|b| etag_of(&b))
}

/// Content tag: a short hex of a wrapping hash (FNV-1a, 64-bit) of the stored bytes, so any change
/// to the file (through this backend or not) changes it. It is not a cryptographic digest.
fn etag_of(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

impl StorageBackend for LocalBackend {
    fn read(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        let rel = clean_path(path)?;
        let dest = self.root.join(&rel);
        fs::read(&dest).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { StorageError::NotFound(rel) } else { StorageError::Io(format!("{}: {e}", dest.display())) }
        })
    }

    fn write(&self, path: &str, bytes: &[u8], opts: &WriteOptions) -> Result<FileMeta, StorageError> {
        let rel = clean_path(path)?;
        if bytes.len() as u64 > (2 << 30) {
            return Err(StorageError::Unsupported(format!("{rel}: larger than 2 GB")));
        }
        let dest = self.root.join(&rel);
        // Held across the `if_match` check and the commit (dropped on every return).
        let lock = PathLock::acquire(&rel, &dest)?;
        if let Some(want) = &opts.if_match {
            match current_etag(&dest) {
                Some(have) if &have == want => {}
                _ if !dest.exists() => return Err(StorageError::Conflict(format!("{rel}: no stored version matches `{want}`"))),
                _ => return Err(StorageError::Conflict(format!("{rel}: stored version changed (stale If-Match)"))),
            }
        }
        if opts.atomic {
            return self.atomic_commit(&rel, &dest, bytes, &lock);
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| StorageError::Io(format!("{}: {e}", parent.display())))?;
        }
        lock.still_held(&rel)?;
        let mut f = File::create(&dest).map_err(|e| StorageError::Io(format!("{}: {e}", dest.display())))?;
        f.write_all(bytes).map_err(|e| StorageError::Io(format!("{}: {e}", dest.display())))?;
        Ok(FileMeta { size: bytes.len() as u64, etag: Some(etag_of(bytes)), modified: None, is_dir: false })
    }

    fn stat(&self, path: &str) -> Result<FileMeta, StorageError> {
        let rel = clean_path(path)?;
        let dest = self.root.join(&rel);
        self.meta_of(&rel, &dest)
    }

    fn list(&self, prefix: &str) -> Result<Vec<Entry>, StorageError> {
        let dir = if prefix.is_empty() { self.root.clone() } else { self.root.join(clean_path(prefix)?) };
        let rd = fs::read_dir(&dir).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(prefix.into())
            } else {
                StorageError::Io(format!("{}: {e}", dir.display()))
            }
        })?;
        let mut out = Vec::new();
        for ent in rd {
            let ent = ent.map_err(|e| StorageError::Io(e.to_string()))?;
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.starts_with(STAGE_PREFIX) || name.starts_with(LOCK_PREFIX) || name.ends_with(".tmp") || name == journal::JOURNAL_FILE {
                continue;
            }
            let ft = ent.file_type().map_err(|e| StorageError::Io(e.to_string()))?;
            let rel = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix.trim_end_matches('/'), name) };
            if ft.is_dir() {
                out.push(Entry { path: rel, meta: FileMeta { size: 0, etag: None, modified: None, is_dir: true } });
            } else if ft.is_file() {
                let size = ent.metadata().map(|m| m.len()).unwrap_or(0);
                // No ETag here: hashing every file would read the whole folder. `stat` has it.
                out.push(Entry { path: rel, meta: FileMeta { size, etag: None, modified: None, is_dir: false } });
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let rel = clean_path(path)?;
        let dest = self.root.join(&rel);
        match fs::remove_file(&dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(StorageError::Io(format!("{}: {e}", dest.display()))),
        }
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { atomic_write: true, conditional_write: true, list: true, etag: true, persistent: true }
    }

    fn recover(&self) -> Result<RecoveryReport, StorageError> {
        let mut report = RecoveryReport::default();
        let bytes = fs::read(self.journal_path()).unwrap_or_default();
        let plan = journal::RecoveryPlan::plan(&journal::read(&bytes));
        for pending in &plan.complete {
            let Ok(rel) = clean_path(&pending.path) else {
                report.dropped += 1;
                continue;
            };
            let dest = self.root.join(&rel);
            // The journal sits inside the (possibly shared) root, so its `staged` field is
            // untrusted: it must name one of our staging files beside `rel`, or nothing is
            // touched for this entry.
            let Some(staged) = staged_beside(&rel, &pending.staged) else {
                report.dropped += 1;
                continue;
            };
            let staged = self.root.join(staged);
            match fs::read(&staged).ok() {
                // The rename moves the staged file, so one that survives with its full length
                // means the rename never happened: finish it (the staged bytes are the newer,
                // fsynced save).
                Some(b) if b.len() as u64 == pending.len => {
                    if fs::rename(&staged, &dest).is_ok() {
                        sync_parent(&dest);
                        report.completed += 1;
                    } else {
                        report.dropped += 1;
                    }
                }
                // Gone: the rename happened (the commit line or the directory sync was lost).
                None if fs::metadata(&dest).is_ok_and(|m| m.is_file() && m.len() == pending.len) => report.completed += 1,
                _ => {
                    let _ = fs::remove_file(&staged);
                    report.dropped += 1;
                }
            }
            self.append_journal(&JournalEntry { state: JournalState::Committed, ..pending.clone() });
        }
        report.completed += plan.done.len();
        report.removed_temp = remove_orphan_temps(&self.root);
        // The log has served its purpose; a fresh one starts now.
        let _ = fs::remove_file(self.journal_path());
        Ok(report)
    }
}

/// `staged` as a root-relative path, when it is a staging file in the same folder as `rel`.
fn staged_beside(rel: &str, staged: &str) -> Option<String> {
    let staged = clean_path(staged).ok()?;
    let (dir, name) = staged.rsplit_once('/').map_or(("", staged.as_str()), |(d, n)| (d, n));
    let rel_dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
    (dir == rel_dir && name.starts_with(STAGE_PREFIX)).then(|| staged.clone())
}

/// Delete staging files no journal entry names. Returns how many were removed.
fn remove_orphan_temps(root: &Path) -> usize {
    let mut removed = 0;
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        seen += 1;
        if seen > 4096 {
            break;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let Ok(ft) = ent.file_type() else { continue };
            if ft.is_dir() {
                stack.push(ent.path());
            } else if ft.is_file() && name.starts_with(STAGE_PREFIX) && fs::remove_file(ent.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> (TempDir, LocalBackend) {
        let dir = TempDir::create();
        let b = LocalBackend::open(dir.path()).unwrap();
        (dir, b)
    }

    /// A unique scratch folder under the system temp dir.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    impl TempDir {
        fn create() -> TempDir {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("cadcraft-storage-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
    }

    #[test]
    fn atomic_roundtrip_and_etag_match() {
        let (_tmp, b) = backend();
        let meta = b.write("memo.dxf", b"PK-data", &WriteOptions { atomic: true, ..Default::default() }).unwrap();
        assert_eq!(b.read("memo.dxf").unwrap(), b"PK-data");
        // `stat` agrees on content identity (`modified` is time-dependent, so compared separately).
        let st = b.stat("memo.dxf").unwrap();
        assert_eq!((st.size, st.etag, st.is_dir), (meta.size, meta.etag.clone(), false));
        assert!(st.modified.is_some());
        // Stale tag loses.
        let err = b.write("memo.dxf", b"new", &WriteOptions { if_match: Some("deadbeef".into()), ..Default::default() }).unwrap_err();
        assert!(matches!(err, StorageError::Conflict(_)));
        // Fresh tag wins.
        let tag = meta.etag.unwrap();
        b.write("memo.dxf", b"new", &WriteOptions { if_match: Some(tag), atomic: true }).unwrap();
        assert_eq!(b.read("memo.dxf").unwrap(), b"new");
    }

    #[test]
    fn no_half_file_visible() {
        let (_tmp, b) = backend();
        b.write("a/b/c.txt", b"hello", &WriteOptions { atomic: true, ..Default::default() }).unwrap();
        let leftovers: Vec<_> =
            fs::read_dir(b.root()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".cadcraft-save-")).collect();
        assert!(leftovers.is_empty());
        assert_eq!(b.list("a/b").unwrap().len(), 1);
    }

    #[test]
    fn recover_finishes_interrupted_commit() {
        let (_tmp, b) = backend();
        // Fake a crash: staged bytes + pending line, no commit line, no final file.
        let staged = ".cadcraft-save-0-crash.txt.tmp";
        fs::write(b.root().join(staged), b"saved?").unwrap();
        b.append_journal(&JournalEntry { path: "crash.txt".into(), staged: staged.into(), state: JournalState::Pending, len: 6 });
        let report = b.recover().unwrap();
        assert_eq!(report.completed, 1);
        assert_eq!(b.read("crash.txt").unwrap(), b"saved?");
    }

    #[test]
    fn recover_finishes_interrupted_commit_in_a_subfolder() {
        let (_tmp, b) = backend();
        fs::create_dir_all(b.root().join("a/b")).unwrap();
        fs::write(b.root().join("a/b/c.txt"), b"old").unwrap();
        // Crash after staging, before the rename: the old file is still in place.
        let staged = "a/b/.cadcraft-save-7-c.txt.tmp";
        fs::write(b.root().join(staged), b"newer").unwrap();
        b.append_journal(&JournalEntry { path: "a/b/c.txt".into(), staged: staged.into(), state: JournalState::Pending, len: 5 });
        let report = b.recover().unwrap();
        assert_eq!((report.completed, report.dropped), (1, 0));
        assert_eq!(b.read("a/b/c.txt").unwrap(), b"newer", "the staged (newer) save wins, it is not discarded");
        assert!(!b.root().join(staged).exists());
    }

    #[test]
    fn recover_journals_staged_path_relative_to_root() {
        let (_tmp, b) = backend();
        b.write("x/y.txt", b"one", &WriteOptions { atomic: true, ..Default::default() }).unwrap();
        let log = fs::read(b.root().join(journal::JOURNAL_FILE)).unwrap();
        let entries = journal::read(&log);
        assert!(entries.iter().all(|e| e.staged.starts_with("x/.cadcraft-save-")), "{entries:?}");
    }

    #[test]
    fn recover_ignores_a_staged_path_outside_the_document_folder() {
        let (_tmp, b) = backend();
        let outside = TempDir::create();
        let victim = outside.path().join("victim.txt");
        fs::write(&victim, b"secret").unwrap();
        fs::write(b.root().join("bait.txt"), b"x").unwrap();
        for staged in [victim.to_string_lossy().into_owned(), "../victim.txt".into(), "other/.cadcraft-save-1-bait.txt.tmp".into(), "bait.txt".into()]
        {
            b.append_journal(&JournalEntry { path: "bait.txt".into(), staged, state: JournalState::Pending, len: 6 });
        }
        let report = b.recover().unwrap();
        assert_eq!(report.dropped, 4);
        assert_eq!(fs::read(&victim).unwrap(), b"secret", "an untrusted journal line must not move or delete files");
        assert_eq!(b.read("bait.txt").unwrap(), b"x");
    }

    #[test]
    fn conditional_write_sees_changes_made_outside_the_backend() {
        let (_tmp, b) = backend();
        let tag = b.write("doc.dxf", b"v1", &WriteOptions::default()).unwrap().etag.unwrap();
        // A sync client replaces the file without going through the backend.
        fs::write(b.root().join("doc.dxf"), b"v1-from-elsewhere").unwrap();
        let err = b.write("doc.dxf", b"v2", &WriteOptions { if_match: Some(tag), atomic: true }).unwrap_err();
        assert!(matches!(err, StorageError::Conflict(_)));
        assert_eq!(b.read("doc.dxf").unwrap(), b"v1-from-elsewhere");
    }

    #[test]
    fn a_file_written_by_another_tool_has_a_usable_etag() {
        let (_tmp, b) = backend();
        fs::write(b.root().join("foreign.dxf"), b"made elsewhere").unwrap();
        let tag = b.stat("foreign.dxf").unwrap().etag.expect("etag from content");
        b.write("foreign.dxf", b"edited", &WriteOptions { if_match: Some(tag), atomic: true }).unwrap();
        assert_eq!(b.read("foreign.dxf").unwrap(), b"edited");
    }

    #[test]
    fn a_held_lock_blocks_a_second_writer_and_a_stale_one_is_taken_over() {
        let (_tmp, b) = backend();
        let dest = b.root().join("held.dxf");
        let lock = PathLock::acquire("held.dxf", &dest).unwrap();
        assert!(matches!(PathLock::acquire("held.dxf", &dest), Err(StorageError::Conflict(_))));
        assert!(b.list("").unwrap().is_empty(), "lock files are hidden from listings");
        drop(lock);
        // A crashed writer's lock, backdated past the stale limit.
        let path = b.root().join(".cadcraft-lock-held.dxf");
        let f = File::create(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() - LOCK_STALE - Duration::from_secs(1)).unwrap();
        drop(f);
        b.write("held.dxf", b"ok", &WriteOptions::default()).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_held_lock_is_renewed_so_a_slow_writer_is_not_taken_over() {
        let (_tmp, b) = backend();
        let dest = b.root().join("slow.dxf");
        let lock = PathLock::acquire("slow.dxf", &dest).unwrap();
        // Make the held lock look stale, as a write outlasting LOCK_STALE would without renewal.
        let path = b.root().join(".cadcraft-lock-slow.dxf");
        File::options().write(true).open(&path).unwrap().set_modified(std::time::SystemTime::now() - LOCK_STALE - Duration::from_secs(1)).unwrap();
        std::thread::sleep(LOCK_RENEW * 4);
        assert!(!lock_is_stale(&path), "the holder renews its lock");
        assert!(matches!(b.write("slow.dxf", b"second", &WriteOptions::default()), Err(StorageError::Conflict(_))));
        assert!(lock.still_held("slow.dxf").is_ok());
    }

    #[test]
    fn a_writer_that_lost_its_lock_cannot_commit_or_remove_the_new_owners_lock() {
        let (_tmp, b) = backend();
        let dest = b.root().join("lost.dxf");
        let lock = PathLock::acquire("lost.dxf", &dest).unwrap();
        // Another writer took the lock over (as after a missed renewal).
        let path = b.root().join(".cadcraft-lock-lost.dxf");
        fs::write(&path, "someone-else").unwrap();
        assert!(matches!(lock.still_held("lost.dxf"), Err(StorageError::Conflict(_))));
        assert!(matches!(b.atomic_commit("lost.dxf", &dest, b"late", &lock), Err(StorageError::Conflict(_))));
        assert!(!dest.exists(), "nothing was renamed over the new owner's commit");
        assert_eq!(b.list("").unwrap().len(), 0, "the staged file was cleaned up");
        drop(lock);
        assert_eq!(fs::read_to_string(&path).unwrap(), "someone-else", "release leaves another owner's lock alone");
    }

    #[test]
    fn list_skips_hashing_but_stat_has_the_etag() {
        let (_tmp, b) = backend();
        let tag = b.write("l.dxf", b"bytes", &WriteOptions::default()).unwrap().etag;
        let listed = b.list("").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].meta.etag, None);
        assert_eq!(b.stat("l.dxf").unwrap().etag, tag);
    }

    #[test]
    fn concurrent_conditional_writers_only_one_wins() {
        let (_tmp, b) = backend();
        let tag = b.write("race.dxf", b"base", &WriteOptions::default()).unwrap().etag.unwrap();
        let wins: usize = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let (b, tag) = (&b, tag.clone());
                    s.spawn(move || {
                        b.write("race.dxf", format!("writer {i}").as_bytes(), &WriteOptions { if_match: Some(tag), atomic: true }).is_ok()
                    })
                })
                .collect();
            handles.into_iter().map(|h| usize::from(h.join().unwrap())).sum()
        });
        assert_eq!(wins, 1);
    }

    #[test]
    fn remove_is_idempotent() {
        let (_tmp, b) = backend();
        b.remove("missing.dxf").unwrap();
        b.write("x.dxf", b"d", &WriteOptions::default()).unwrap();
        b.remove("x.dxf").unwrap();
        assert!(matches!(b.stat("x.dxf").unwrap_err(), StorageError::NotFound(_)));
    }
}
