//! Drawing locks: who has a drawing open (WHOHAS), for drawings shared on a network folder (#540).
//!
//! While a drawing opened from a file is open, the app keeps a small lock file beside it,
//! `.<file name>.cadcraft-lock`, holding who opened it (user, computer, app instance, when).
//! The file's modification time is its heartbeat: the app touches it every [`HEARTBEAT_SECS`],
//! and a lock not touched for [`STALE_SECS`] belongs to an app that is no longer running (a crash
//! leaves it behind, and it is then ignored and taken over).
//!
//! A lock only informs: opening a drawing someone else has open says so on the command line and
//! in the status bar, and the drawing opens anyway (their lock stays theirs). Only lock files of
//! this app instance are ever deleted. The format is CADCraft's own (JSON), not AutoCAD's `.dwl`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How often a held lock is touched, in seconds.
pub const HEARTBEAT_SECS: u64 = 60;
/// A lock not touched for this long is stale, in seconds.
pub const STALE_SECS: u64 = 5 * 60;
/// Largest lock file read (bytes): anything bigger isn't ours.
const MAX_LOCK_BYTES: u64 = 4096;

/// Who holds a lock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub user: String,
    pub host: String,
    /// The app instance (see [`crate::autosave::Autosaver::instance`]).
    pub instance: String,
    /// When the drawing was opened, in seconds since 1970.
    pub opened: u64,
}

/// The lock file of the drawing at `drawing`.
pub fn lock_path(drawing: &Path) -> PathBuf {
    let name = drawing.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    drawing.with_file_name(format!(".{name}.cadcraft-lock"))
}

/// Whether drawings at `path` get a lock: drawing files only (not exports).
fn lockable(path: &str) -> bool {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "dwg" | "dxf" | "dwt")
}

/// `secs` since 1970 as `YYYY-MM-DD HH:MM` (UTC).
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (h, m) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02} UTC")
}

impl Holder {
    /// "user on host since …".
    pub fn describe(&self) -> String {
        let who = if self.user.is_empty() { "someone" } else { self.user.as_str() };
        let host = if self.host.is_empty() { String::new() } else { format!(" on {}", self.host) };
        format!("{who}{host}, since {}", utc(self.opened))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::{HashMap, HashSet};
    use std::time::{Duration, Instant, SystemTime};

    use super::*;
    use crate::Session;

    /// This user's name, from the environment.
    fn user() -> String {
        ["USER", "USERNAME", "LOGNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())).unwrap_or_default()
    }

    /// This computer's name (looked up once).
    fn host() -> String {
        static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        HOST.get_or_init(|| {
            if let Some(h) = ["COMPUTERNAME", "HOSTNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())) {
                return h;
            }
            std::process::Command::new("hostname")
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().chars().take(255).collect())
                .unwrap_or_default()
        })
        .clone()
    }

    fn now_secs() -> u64 {
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    /// The holder of the lock on `drawing` while its lock is fresh (touched within
    /// [`STALE_SECS`]); `None` when there is no lock, it is stale or it isn't one of ours.
    pub fn holder(drawing: &Path) -> Option<Holder> {
        let lock = lock_path(drawing);
        let meta = std::fs::metadata(&lock).ok()?;
        if !meta.is_file() || meta.len() > MAX_LOCK_BYTES {
            return None;
        }
        // A time in the future (clock changes, another computer's clock) counts as fresh.
        let age = meta.modified().ok().map(|m| SystemTime::now().duration_since(m).unwrap_or_default())?;
        if age >= Duration::from_secs(STALE_SECS) {
            return None;
        }
        serde_json::from_slice(&std::fs::read(&lock).ok()?).ok()
    }

    /// This instance's lock contents for `drawing`, staged next to its lock file.
    fn stage(drawing: &Path, h: &Holder) -> std::io::Result<PathBuf> {
        let lock = lock_path(drawing);
        let tmp = lock.with_file_name(format!(
            "{}.{}.tmp",
            lock.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            std::process::id()
        ));
        std::fs::write(&tmp, serde_json::to_vec(h).unwrap_or_default())?;
        Ok(tmp)
    }

    /// Write this instance's lock on `drawing` (staged, then renamed over any stale one).
    fn write(drawing: &Path, h: &Holder) -> std::io::Result<()> {
        let tmp = stage(drawing, h)?;
        std::fs::rename(&tmp, lock_path(drawing)).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    /// Take the lock on `drawing` when there is none or it is stale: whether it is now this
    /// instance's. With no lock file it is created atomically (two apps opening the drawing at
    /// once can't both get it); a stale one is replaced and read back, so of two apps taking it
    /// over at once only the last one keeps it.
    fn claim(drawing: &Path, h: &Holder) -> std::io::Result<bool> {
        let lock = lock_path(drawing);
        if lock.exists() {
            // Taken meanwhile by another app (its lock is fresh again): leave it.
            if holder(drawing).is_some_and(|o| o.instance != h.instance) {
                return Ok(false);
            }
            write(drawing, h)?;
        } else {
            let tmp = stage(drawing, h)?;
            // A hard link appears with the whole lock written, or fails when the lock exists.
            let linked = std::fs::hard_link(&tmp, &lock);
            let _ = std::fs::remove_file(&tmp);
            match linked {
                Ok(()) => return Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
                // No hard links on this file system: create it only if it isn't there.
                Err(_) => {
                    use std::io::Write;
                    match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
                        Ok(mut f) => f.write_all(&serde_json::to_vec(h).unwrap_or_default())?,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        let back = std::fs::read(&lock).ok().and_then(|b| serde_json::from_slice::<Holder>(&b).ok());
        Ok(back.is_some_and(|b| b.instance == h.instance))
    }

    /// Touch a held lock (its modification time is the heartbeat).
    fn touch(drawing: &Path) -> std::io::Result<()> {
        std::fs::OpenOptions::new().write(true).open(lock_path(drawing))?.set_modified(SystemTime::now())
    }

    /// Delete the lock on `drawing` if it is still `instance`'s.
    fn release(drawing: &Path, instance: &str) {
        let lock = lock_path(drawing);
        let ours = std::fs::read(&lock).ok().and_then(|b| serde_json::from_slice::<Holder>(&b).ok()).is_some_and(|h| h.instance == instance);
        if ours {
            let _ = std::fs::remove_file(lock);
        }
    }

    /// The note that `h` also has the drawing at `path` (named `p`) open.
    fn also_open(path: &Path, p: &str, h: &Holder) -> String {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string());
        format!("{name} is also open by {}. Saving it here can overwrite their changes (or theirs yours).", h.describe())
    }

    /// This instance's lock contents.
    fn me(instance: &str) -> Holder {
        Holder { user: user(), host: host(), instance: instance.to_string(), opened: now_secs() }
    }

    /// The drawing locks of one running app.
    pub struct Locks {
        /// How often held locks are touched and others' locks looked at again.
        beat: Duration,
        /// Drawings this app holds the lock of, with when it was last touched.
        held: HashMap<String, Instant>,
        /// Drawings open here that someone else holds, with when that was last checked.
        others: HashMap<String, Instant>,
        /// What to tell the user (someone else has a drawing open), taken by [`Locks::take_notes`].
        notes: Vec<String>,
    }

    impl Default for Locks {
        fn default() -> Self {
            Locks::with_heartbeat(Duration::from_secs(HEARTBEAT_SECS))
        }
    }

    impl Locks {
        /// Locks touched (and others' looked at again) every `beat` instead of [`HEARTBEAT_SECS`].
        pub fn with_heartbeat(beat: Duration) -> Self {
            Locks { beat, held: HashMap::new(), others: HashMap::new(), notes: Vec::new() }
        }

        /// Bring the locks in line with the drawings open in `s`: take the lock of a drawing just
        /// opened from a file (or note who else has it open), touch held locks every
        /// [`HEARTBEAT_SECS`], take over a lock its holder let go stale, and release the locks of
        /// drawings no longer open here (closed, or saved under another name).
        pub fn sync(&mut self, s: &Session, instance: &str) {
            let open: HashSet<String> = s.docs.iter().filter_map(|d| d.path.clone()).filter(|p| lockable(p)).collect();
            let gone: Vec<String> = self.held.keys().filter(|p| !open.contains(*p)).cloned().collect();
            for p in gone {
                release(Path::new(&p), instance);
                self.held.remove(&p);
            }
            self.others.retain(|p, _| open.contains(p));
            let beat = self.beat;
            for p in open {
                let path = Path::new(&p);
                if let Some(at) = self.held.get(&p) {
                    if at.elapsed() < beat {
                        continue;
                    }
                    // Still ours? Another app may have taken it over (this one was asleep past
                    // STALE_SECS): then it is theirs, and noted below.
                    if holder(path).is_none_or(|h| h.instance == instance) {
                        // A lock removed meanwhile (by hand, a sync tool) is written again.
                        if touch(path).is_err() {
                            let _ = write(path, &me(instance));
                        }
                        self.held.insert(p, Instant::now());
                        continue;
                    }
                    self.held.remove(&p);
                }
                if let Some(at) = self.others.get(&p)
                    && at.elapsed() < beat
                {
                    continue;
                }
                match holder(path) {
                    Some(h) if h.instance != instance => {
                        if !self.others.contains_key(&p) {
                            self.notes.push(also_open(path, &p, &h));
                        }
                        self.others.insert(p, Instant::now());
                    }
                    _ => match claim(path, &me(instance)) {
                        // Another app took it first: tell the user as for a lock found.
                        Ok(false) => {
                            if let Some(h) = holder(path).filter(|h| h.instance != instance) {
                                self.notes.push(also_open(path, &p, &h));
                            }
                            self.others.insert(p, Instant::now());
                        }
                        // No lock, a stale one or our own: now ours (a folder we can't write
                        // to just gets no lock).
                        _ => {
                            self.others.remove(&p);
                            self.held.insert(p, Instant::now());
                        }
                    },
                }
            }
        }

        /// The notes [`Locks::sync`] made since the last call.
        pub fn take_notes(&mut self) -> Vec<String> {
            std::mem::take(&mut self.notes)
        }

        /// The app is closing: release every lock it holds.
        pub fn release_all(&mut self, instance: &str) {
            for (p, _) in self.held.drain() {
                release(Path::new(&p), instance);
            }
            self.others.clear();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Of two apps claiming a drawing that has no lock, only the first gets it: the second
        /// finds it taken and leaves it. A stale lock is taken over.
        #[test]
        fn only_one_app_claims_a_free_lock() {
            let dir = std::env::temp_dir().join(format!("cadcraft-claim-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("Plan.dxf");
            std::fs::write(&file, b"").unwrap();
            assert!(claim(&file, &me("1-a")).unwrap());
            assert!(!claim(&file, &me("2-b")).unwrap(), "a fresh lock stays the first app's");
            assert_eq!(holder(&file).map(|h| h.instance), Some("1-a".to_string()));
            assert!(claim(&file, &me("1-a")).unwrap(), "its own lock is still its own");
            let stale = SystemTime::now() - Duration::from_secs(STALE_SECS + 60);
            std::fs::OpenOptions::new().write(true).open(lock_path(&file)).unwrap().set_modified(stale).unwrap();
            assert!(claim(&file, &me("2-b")).unwrap(), "a stale lock is taken over");
            assert_eq!(holder(&file).map(|h| h.instance), Some("2-b".to_string()));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
