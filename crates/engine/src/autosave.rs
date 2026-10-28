//! Autosave (SAVETIME, SAVEFILEPATH) and the files Drawing Recovery offers after a crash.
//!
//! Every SAVETIME minutes after a change, each drawing with unsaved changes made since its last
//! autosave is written as DXF to `<SAVEFILEPATH>/<drawing>_<instance>_<uid>.sv$`. The file goes
//! away when the drawing is saved or closed and when the app exits normally, so a file left behind
//! means the app (or the machine) went down; the next start offers it for recovery. A running app
//! keeps a lock file (`<instance>.lock`) fresh, so another copy running at the same time doesn't
//! offer its files. SAVEFILEPATH may be a folder other programs use too, so only names built from
//! an instance id ([`is_instance_id`]) are ever read as ours or deleted.
//!
//! [`Autosaver::plan`] decides what to write and delete (no I/O, so it is testable); on the desktop
//! [`Autosaver::tick`] carries it out, writing on a background thread. SAVETIME and SAVEFILEPATH
//! are profile settings: process-wide, not saved in the drawing.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use cadcraft_doc::Drawing;
use serde::Serialize;

use crate::Session;

/// The autosave file extension.
pub const EXT: &str = "sv$";
/// SAVETIME's default, in minutes.
pub const DEFAULT_SAVETIME: u32 = 10;
/// SAVETIME's largest value, in minutes.
pub const MAX_SAVETIME: u32 = 600;
/// A lock file not refreshed for this long belongs to an app that is no longer running.
pub const STALE_LOCK_SECS: u64 = 5 * 60;
/// How often a running app refreshes its lock file.
#[cfg(not(target_arch = "wasm32"))]
const HEARTBEAT_SECS: f64 = 60.0;

static SAVETIME: AtomicU32 = AtomicU32::new(DEFAULT_SAVETIME);
static SAVEFILEPATH: Mutex<String> = Mutex::new(String::new());

/// SAVETIME: minutes between autosaves; 0 turns autosave off.
pub fn savetime() -> u32 {
    SAVETIME.load(Ordering::Relaxed)
}

/// Set SAVETIME (clamped to 0–600 minutes).
pub fn set_savetime(minutes: u32) {
    SAVETIME.store(minutes.min(MAX_SAVETIME), Ordering::Relaxed);
}

/// SAVEFILEPATH: the folder autosave files go to (the host sets a per-user default at start).
pub fn savefilepath() -> String {
    SAVEFILEPATH.lock().map(|p| p.clone()).unwrap_or_default()
}

pub fn set_savefilepath(path: &str) {
    if let Ok(mut p) = SAVEFILEPATH.lock() {
        *p = path.trim().to_string();
    }
}

/// The autosave file name for drawing `uid` titled `title`, written by app instance `instance`.
pub fn file_name(title: &str, instance: &str, uid: u64) -> String {
    let stem: String = title.chars().map(|c| if c.is_alphanumeric() || " .-_()".contains(c) { c } else { '-' }).take(80).collect();
    let stem = stem.trim().trim_matches('.');
    let stem = if stem.is_empty() { "Drawing" } else { stem };
    format!("{stem}_{instance}_{uid}.{EXT}")
}

/// Whether `s` has the form of an app instance id (`<process id>-<start time in hex>`, see
/// `instance_id`). Only file names built from one are treated as CADCraft's own, so files of
/// other programs in the autosave folder are never offered for recovery or deleted.
pub fn is_instance_id(s: &str) -> bool {
    let Some((pid, time)) = s.split_once('-') else { return false };
    (1..=10).contains(&pid.len())
        && pid.bytes().all(|b| b.is_ascii_digit())
        && (1..=32).contains(&time.len())
        && time.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// An autosave file name's parts: the drawing's title, the instance and the drawing uid.
pub fn parse_file_name(name: &str) -> Option<(&str, &str, u64)> {
    let base = name.strip_suffix(&format!(".{EXT}"))?;
    let (rest, uid) = base.rsplit_once('_')?;
    let (title, instance) = rest.rsplit_once('_')?;
    let uid_ok = !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit());
    (!title.is_empty() && is_instance_id(instance) && uid_ok).then_some(())?;
    Some((title, instance, uid.parse().ok()?))
}

/// The instance whose lock file is named `name` (`<instance>.lock`), if it is one of ours.
pub fn parse_lock_name(name: &str) -> Option<&str> {
    name.strip_suffix(".lock").filter(|i| is_instance_id(i))
}

/// What to do with the file system.
#[derive(Clone, Debug)]
pub enum Job {
    /// Write `doc` (drawing `uid`) to `path`.
    Write {
        uid: u64,
        path: String,
        doc: Arc<Drawing>,
    },
    Delete {
        path: String,
    },
}

#[derive(Debug, Default)]
struct Tracked {
    /// The drawing as last autosaved (a weak pointer: compared, never kept alive).
    written: Option<Weak<Drawing>>,
    /// The drawing outside the Block Editor as last autosaved (the same as `written` when the
    /// editor is closed): saving the editor's changes into the block changes only this.
    host: Option<Weak<Drawing>>,
    /// Its autosave file, once written (or adopted).
    file: Option<String>,
    /// When a change not yet autosaved was first seen (seconds).
    since: Option<f64>,
}

/// Autosave state for one running app.
pub struct Autosaver {
    /// This app instance's id (`<process id>-<start time>`), part of every file name.
    pub instance: String,
    docs: HashMap<u64, Tracked>,
    /// Drawings whose autosave is being written right now.
    busy: HashSet<u64>,
    /// Writes in progress; each tells whether its file was written.
    #[cfg(not(target_arch = "wasm32"))]
    running: Vec<(u64, std::thread::JoinHandle<bool>)>,
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_beat: Option<f64>,
}

impl Autosaver {
    pub fn new(instance: impl Into<String>) -> Self {
        Autosaver {
            instance: instance.into(),
            docs: HashMap::new(),
            busy: HashSet::new(),
            #[cfg(not(target_arch = "wasm32"))]
            running: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
            #[cfg(not(target_arch = "wasm32"))]
            last_beat: None,
        }
    }

    /// Take over an existing autosave file for drawing `uid` (a drawing opened from Drawing
    /// Recovery, whose current state it holds): later autosaves overwrite it, and it is deleted
    /// once the drawing is saved or closed.
    pub fn adopt(&mut self, s: &Session, uid: u64, path: &str) {
        let st = s.docs.iter().find(|d| d.uid == uid);
        let (written, host) = (st.map(|d| Arc::downgrade(&d.doc)), st.map(|d| Arc::downgrade(d.drawing())));
        self.docs.insert(uid, Tracked { written, host, file: Some(path.to_string()), since: None });
    }

    /// The autosave files this app has written or adopted.
    pub fn files(&self) -> Vec<String> {
        self.docs.values().filter_map(|t| t.file.clone()).collect()
    }

    /// What to do at `now` (seconds, any fixed origin) with SAVETIME `minutes` and autosave folder
    /// `dir`: write drawings changed `minutes` ago or earlier and not autosaved since, delete the
    /// files of drawings that were saved (no unsaved changes left) or closed.
    pub fn plan(&mut self, s: &Session, now: f64, minutes: u32, dir: &str) -> Vec<Job> {
        self.plan_with(s, now, minutes, dir, false)
    }

    /// [`Autosaver::plan`], with `urgent` writing every drawing with changes not yet autosaved
    /// now instead of `minutes` after the change (autosave still off when `minutes` is 0).
    fn plan_with(&mut self, s: &Session, now: f64, minutes: u32, dir: &str, urgent: bool) -> Vec<Job> {
        let mut jobs = Vec::new();
        let live: HashSet<u64> = s.docs.iter().map(|d| d.uid).collect();
        let busy = &self.busy;
        self.docs.retain(|uid, t| {
            // A file being written is deleted once the write is done.
            if live.contains(uid) || busy.contains(uid) {
                return true;
            }
            if let Some(path) = t.file.take() {
                jobs.push(Job::Delete { path });
            }
            false
        });
        for st in &s.docs {
            if self.busy.contains(&st.uid) {
                continue;
            }
            let t = self.docs.entry(st.uid).or_default();
            if !st.is_dirty() {
                if let Some(path) = t.file.take() {
                    jobs.push(Job::Delete { path });
                }
                *t = Tracked::default();
                continue;
            }
            let same = |w: &Option<Weak<Drawing>>, d: &Arc<Drawing>| w.as_ref().is_some_and(|w| std::ptr::eq(w.as_ptr(), Arc::as_ptr(d)));
            let changed = !same(&t.written, &st.doc) || !same(&t.host, st.drawing());
            if !changed {
                t.since = None;
                continue;
            }
            let since = *t.since.get_or_insert(now);
            if minutes == 0 || dir.trim().is_empty() || (!urgent && now - since < f64::from(minutes) * 60.0) {
                continue;
            }
            // In the Block Editor: the drawing with the editor's unsaved changes in the block. When
            // they can't be put there, nothing is written and nothing is marked as covered, so the
            // next pass tries again instead of keeping the host without them.
            let Ok(doc) = st.recovery_drawing() else {
                continue;
            };
            let path = t
                .file
                .clone()
                .unwrap_or_else(|| std::path::Path::new(dir).join(file_name(&st.title, &self.instance, st.uid)).to_string_lossy().to_string());
            t.file = Some(path.clone());
            t.written = Some(Arc::downgrade(&st.doc));
            t.host = Some(Arc::downgrade(st.drawing()));
            t.since = None;
            jobs.push(Job::Write { uid: st.uid, path, doc });
        }
        jobs
    }

    /// Seconds from `now` until the next autosave is due, if one is waiting.
    pub fn next_due(&self, now: f64, minutes: u32) -> Option<f64> {
        if minutes == 0 {
            return None;
        }
        self.docs.values().filter_map(|t| t.since).map(|since| (since + f64::from(minutes) * 60.0 - now).max(0.0)).reduce(f64::min)
    }
}

/// A file Drawing Recovery offers: an autosave left behind by an app that is no longer running.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recoverable {
    pub path: String,
    /// The drawing's title when it was autosaved.
    pub drawing: String,
    /// When it was written, in seconds since 1970.
    pub modified: u64,
    pub bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::*;

    /// A new instance id: the process id and the start time.
    pub fn instance_id() -> String {
        let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        format!("{}-{t:x}", std::process::id())
    }

    fn lock_path(dir: &Path, instance: &str) -> std::path::PathBuf {
        dir.join(format!("{instance}.lock"))
    }

    /// Whether the app instance that owns a lock file modified `age` ago (`None`: no lock file)
    /// may still be running.
    pub fn instance_alive(age: Option<Duration>) -> bool {
        age.is_some_and(|a| a < Duration::from_secs(STALE_LOCK_SECS))
    }

    fn lock_age(dir: &Path, instance: &str) -> Option<Duration> {
        let m = std::fs::metadata(lock_path(dir, instance)).and_then(|m| m.modified()).ok()?;
        // A time in the future (clock changes) counts as fresh.
        Some(SystemTime::now().duration_since(m).unwrap_or_default())
    }

    /// The autosave files in `dir` left behind by apps that are no longer running (never those of
    /// `own`, this app's instance), oldest first.
    pub fn recoverable(dir: &Path, own: &str) -> Vec<Recoverable> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut alive: HashMap<String, bool> = HashMap::new();
        let mut out = Vec::new();
        for e in entries.flatten().take(10_000) {
            let name = e.file_name().to_string_lossy().to_string();
            let Some((title, instance, _)) = parse_file_name(&name) else { continue };
            if instance == own || *alive.entry(instance.to_string()).or_insert_with(|| instance_alive(lock_age(dir, instance))) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().ok().and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push(Recoverable { path: e.path().to_string_lossy().to_string(), drawing: title.to_string(), modified, bytes: meta.len() });
        }
        out.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.path.cmp(&b.path)));
        out
    }

    /// Delete a recovery file the user discarded (or that was recovered and then saved), and lock
    /// files of stopped apps that have no files left. Only an autosave file is deleted: a path
    /// whose name doesn't parse as one ([`parse_file_name`]) is left alone.
    pub fn discard(path: &str) {
        let ours = Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).is_some_and(|n| parse_file_name(n).is_some());
        if !ours {
            log::warn!("autosave: not deleting {path}: not an autosave file");
            return;
        }
        if let Err(e) = std::fs::remove_file(path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("autosave: can't delete {path}: {e}");
        }
        if let Some(dir) = Path::new(path).parent() {
            prune_locks(dir);
        }
    }

    /// Delete the lock files of apps that are no longer running and left no autosave files. Only
    /// names of the form `<instance id>.lock` are ours ([`parse_lock_name`]): SAVEFILEPATH may
    /// point at a folder shared with other programs, whose `Cargo.lock` or `foo.lock` stay.
    pub fn prune_locks(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let names: Vec<String> = entries.flatten().take(10_000).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        for n in &names {
            let Some(instance) = parse_lock_name(n) else { continue };
            let has_files = names.iter().any(|f| parse_file_name(f).is_some_and(|(_, i, _)| i == instance));
            if !has_files && !instance_alive(lock_age(dir, instance)) {
                let _ = std::fs::remove_file(dir.join(n));
            }
        }
    }

    type WriteHook = fn(&Drawing, &str) -> std::result::Result<Vec<u8>, String>;

    /// Create the autosave folder `dir` and any missing parents. On Unix the folders made here are
    /// owner-only (`0o700`): autosave files are unsaved drawings, and with the default mode other
    /// accounts on the machine could read them. A folder that already exists keeps its mode, since
    /// SAVEFILEPATH can be any folder the user picked.
    fn create_folder(dir: &Path) -> std::io::Result<()> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)
    }

    fn write_file(write: WriteHook, doc: &Drawing, path: &str) -> std::result::Result<(), String> {
        let bytes = write(doc, "autosave.dxf")?;
        if let Some(dir) = Path::new(path).parent() {
            create_folder(dir).map_err(|e| e.to_string())?;
        }
        // Create a fresh temporary file exclusively and rename it into place, so a planted
        // `<path>.tmp` symlink in a shared autosave directory cannot redirect the write (CWE-59).
        crate::atomic_write::write_atomic(Path::new(path), &bytes).map_err(|e| e.to_string())
    }

    impl Autosaver {
        /// Autosave what is due (writing in the background) and delete files no longer needed.
        /// Returns how long until the next autosave is due, so the host can wake up for it.
        pub fn tick(&mut self, s: &Session) -> Option<Duration> {
            let now = self.start.elapsed().as_secs_f64();
            self.reap();
            let dir = savefilepath();
            let minutes = savetime();
            let jobs = self.plan(s, now, minutes, &dir);
            self.perform(jobs);
            self.heartbeat(&dir, now);
            self.next_due(now, minutes).map(Duration::from_secs_f64)
        }

        /// Autosave every drawing with changes not yet autosaved right away, without waiting for
        /// SAVETIME (the app may not survive what just happened, e.g. a GPU error). Returns how
        /// many drawings are being written.
        pub fn save_now(&mut self, s: &Session) -> usize {
            let now = self.start.elapsed().as_secs_f64();
            let jobs = self.plan_with(s, now, savetime(), &savefilepath(), true);
            let writes = jobs.iter().filter(|j| matches!(j, Job::Write { .. })).count();
            self.perform(jobs);
            // The lock file marks the files as this running app's (Drawing Recovery skips them).
            self.last_beat = None;
            self.heartbeat(&savefilepath(), now);
            writes
        }

        /// Take in the writes that have finished ([`Autosaver::tick`] does this first). A drawing
        /// whose write failed is no longer counted as autosaved: the next plan restarts its clock,
        /// so it is tried again SAVETIME later even if it doesn't change.
        pub fn reap(&mut self) {
            let (done, running) = std::mem::take(&mut self.running).into_iter().partition::<Vec<_>, _>(|(_, h)| h.is_finished());
            self.running = running;
            for (uid, h) in done {
                self.busy.remove(&uid);
                if !h.join().unwrap_or(false) {
                    self.unwritten(uid);
                }
            }
        }

        fn unwritten(&mut self, uid: u64) {
            if let Some(t) = self.docs.get_mut(&uid) {
                t.written = None;
            }
        }

        /// Carry out jobs from [`Autosaver::plan`]: writes on a background thread, deletes now.
        pub fn perform(&mut self, jobs: Vec<Job>) {
            for job in jobs {
                match job {
                    Job::Delete { path } => discard(&path),
                    Job::Write { uid, path, doc } => {
                        let Some(write) = crate::cmd::file::io().map(|h| h.write) else {
                            log::warn!("autosave: file formats are not available in this build");
                            continue;
                        };
                        let spawned = std::thread::Builder::new().name("autosave".into()).spawn(move || match write_file(write, &doc, &path) {
                            Ok(()) => {
                                log::info!("autosaved {path}");
                                true
                            }
                            Err(e) => {
                                log::warn!("autosave {path}: {e}");
                                false
                            }
                        });
                        match spawned {
                            Ok(h) => {
                                self.busy.insert(uid);
                                self.running.push((uid, h));
                            }
                            Err(e) => {
                                log::warn!("autosave: {e}");
                                self.unwritten(uid);
                            }
                        }
                    }
                }
            }
        }

        /// Keep this app's lock file fresh while it has autosave files.
        fn heartbeat(&mut self, dir: &str, now: f64) {
            if dir.is_empty() || self.docs.values().all(|t| t.file.is_none()) || self.last_beat.is_some_and(|b| now - b < HEARTBEAT_SECS) {
                return;
            }
            self.last_beat = Some(now);
            let dir = Path::new(dir);
            let r = create_folder(dir).and_then(|()| std::fs::write(lock_path(dir, &self.instance), std::process::id().to_string()));
            if let Err(e) = r {
                log::warn!("autosave: lock file in {}: {e}", dir.display());
            }
        }

        /// The app is closing normally: finish writes in progress, then delete this app's autosave
        /// files and its lock file.
        pub fn shutdown(&mut self) {
            for (_, h) in self.running.drain(..) {
                let _ = h.join();
            }
            self.busy.clear();
            for path in self.files() {
                discard(&path);
            }
            self.docs.clear();
            let dir = savefilepath();
            if !dir.is_empty() && is_instance_id(&self.instance) {
                let _ = std::fs::remove_file(lock_path(Path::new(&dir), &self.instance));
            }
        }
    }
}
