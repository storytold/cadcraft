//! Long file work (writing a drawing file, plotting a PDF) run away from the window: a command
//! the UI runs through [`step`] stops where it would write the bytes and hands that work back as
//! a [`Deferred`]. The UI runs it on another thread, keeps drawing (a progress window), gives the
//! bytes back with [`provide`] and runs the command again, which then takes the bytes instead of
//! working them out and does the rest (writing the file, marking the drawing saved) as always.
//!
//! The work is the output hooks of [`crate::cmd::file`] on a copy of the drawing: nothing else of
//! the session leaves the window's thread. Calls outside [`step`] (scripts, the control channel,
//! MCP, tests) run as they always did.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use cadcraft_doc::{Drawing, Space};
use serde_json::Value;

use crate::Session;
use crate::cmd::file::SaveAs;

/// The work an output hook does, with copies of what it reads.
enum Work {
    Write { d: Box<Drawing>, path: String },
    WriteAs { d: Box<Drawing>, path: String, opts: SaveAs },
    Plot { d: Box<Drawing>, space: Space, opts: Value },
    Book { items: Vec<(Drawing, Space)>, opts: Value },
}

/// What a deferred command waits for: run it with [`Deferred::run`], anywhere.
pub struct Deferred {
    key: String,
    work: Work,
}

/// The result of [`Deferred::run`], for [`provide`].
pub struct Done {
    key: String,
    bytes: std::result::Result<Vec<u8>, String>,
}

impl Deferred {
    /// What the work is, for the progress window: "save" or "plot".
    pub fn kind(&self) -> &'static str {
        match self.work {
            Work::Write { .. } | Work::WriteAs { .. } => "save",
            Work::Plot { .. } | Work::Book { .. } => "plot",
        }
    }

    /// Work out the bytes (slow: the DWG or PDF writer).
    pub fn run(self) -> Done {
        let bytes = match self.work {
            Work::Write { d, path } => match crate::cmd::file::io() {
                Some(h) => (h.write)(&d, &path),
                None => Err("file formats are not available in this build".into()),
            },
            Work::WriteAs { d, path, opts } => match crate::cmd::file::write_as() {
                Some(w) => w(&d, &path, &opts),
                None => Err("saving at a chosen version is not available in this build".into()),
            },
            Work::Plot { d, space, opts } => match crate::cmd::file::io().and_then(|h| h.plot) {
                Some(plot) => plot(&d, &space, &opts),
                None => Err("plotting is not available in this build".into()),
            },
            Work::Book { items, opts } => match crate::cmd::file::plot_book() {
                Some(book) => {
                    let refs: Vec<(&Drawing, Space)> = items.iter().map(|(d, s)| (d, s.clone())).collect();
                    book(&refs, &opts)
                }
                None => Err("combining sheets is not available in this build".into()),
            },
        };
        Done { key: self.key, bytes }
    }
}

thread_local! {
    /// [`step`] is running a command: an output hook defers its work instead of doing it.
    static CAPTURE: Cell<bool> = const { Cell::new(false) };
    /// The work an output hook deferred.
    static CAPTURED: RefCell<Option<Deferred>> = const { RefCell::new(None) };
    /// Bytes worked out by [`Deferred::run`], by key, for the next run of the command.
    static DONE: RefCell<HashMap<String, std::result::Result<Vec<u8>, String>>> = RefCell::new(HashMap::new());
}

/// The error a hook returns when it deferred its work (the command stops there).
const DEFERRED: &str = "deferred: the file is being written";

/// What [`step`] did: ran the command, or stopped it at its long work.
pub enum Step {
    Done(crate::Result<Value>),
    Slow(Deferred),
}

/// Run command `id` with `p` up to its long work: [`Step::Slow`] hands the work back (the command
/// stopped before changing anything that a second run would do again, and its messages were
/// taken back), [`Step::Done`] when it finished (with any bytes [`provide`]d).
pub fn step(s: &mut Session, id: &str, p: &Value) -> Step {
    let log = s.log.len();
    CAPTURE.with(|c| c.set(true));
    let r = s.execute(id, p);
    CAPTURE.with(|c| c.set(false));
    match CAPTURED.with(|c| c.borrow_mut().take()) {
        Some(work) => {
            s.log.truncate(log);
            Step::Slow(work)
        }
        None => {
            DONE.with(|d| d.borrow_mut().clear());
            Step::Done(r)
        }
    }
}

/// Bytes [`Deferred::run`] worked out, for the command's next [`step`].
pub fn provide(done: Done) {
    DONE.with(|d| d.borrow_mut().insert(done.key, done.bytes));
}

/// Forget provided bytes no command took (a save given up on).
pub fn forget() {
    DONE.with(|d| d.borrow_mut().clear());
}

/// The bytes provided for `key`, or: while [`step`] runs, defer `work` and stop the command;
/// otherwise `None` (work it out now).
fn take_or_defer(key: String, work: impl FnOnce() -> Work) -> Option<std::result::Result<Vec<u8>, String>> {
    if let Some(r) = DONE.with(|d| d.borrow_mut().remove(&key)) {
        return Some(r);
    }
    if !CAPTURE.with(Cell::get) {
        return None;
    }
    let work = work();
    CAPTURED.with(|c| *c.borrow_mut() = Some(Deferred { key, work }));
    Some(Err(DEFERRED.to_string()))
}

/// Which drawing: its place in memory, unchanged while the command waits (the window takes no
/// input then).
fn id_of(d: &Drawing) -> usize {
    std::ptr::from_ref(d) as usize
}

/// [`crate::cmd::file::IoHooks::write`], deferred under [`step`].
pub(crate) fn write(
    hook: fn(&Drawing, &str) -> std::result::Result<Vec<u8>, String>,
    d: &Drawing,
    path: &str,
) -> std::result::Result<Vec<u8>, String> {
    let key = format!("write|{}|{path}", id_of(d));
    take_or_defer(key, || Work::Write { d: Box::new(d.clone()), path: path.to_string() }).unwrap_or_else(|| hook(d, path))
}

/// [`crate::cmd::file::WriteAsHook`], deferred under [`step`].
pub(crate) fn write_as(hook: crate::cmd::file::WriteAsHook, d: &Drawing, path: &str, opts: &SaveAs) -> std::result::Result<Vec<u8>, String> {
    let key = format!("writeas|{}|{path}|{opts:?}", id_of(d));
    take_or_defer(key, || Work::WriteAs { d: Box::new(d.clone()), path: path.to_string(), opts: *opts }).unwrap_or_else(|| hook(d, path, opts))
}

/// [`crate::cmd::file::PlotHook`], deferred under [`step`].
pub(crate) fn plot(hook: crate::cmd::file::PlotHook, d: &Drawing, space: &Space, opts: &Value) -> std::result::Result<Vec<u8>, String> {
    let key = format!("plot|{}|{space:?}|{opts}", id_of(d));
    take_or_defer(key, || Work::Plot { d: Box::new(d.clone()), space: space.clone(), opts: opts.clone() }).unwrap_or_else(|| hook(d, space, opts))
}

/// [`crate::cmd::file::PlotBookHook`], deferred under [`step`].
pub(crate) fn book(hook: crate::cmd::file::PlotBookHook, items: &[(&Drawing, Space)], opts: &Value) -> std::result::Result<Vec<u8>, String> {
    let ids: Vec<String> = items.iter().map(|(d, s)| format!("{}:{s:?}", id_of(d))).collect();
    let key = format!("book|{}|{opts}", ids.join(","));
    take_or_defer(key, || Work::Book { items: items.iter().map(|(d, s)| ((*d).clone(), s.clone())).collect(), opts: opts.clone() })
        .unwrap_or_else(|| hook(items, opts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Install stand-in io hooks; whether a working writer is installed (the hooks are
    /// process-wide, and some tests install writers that always fail).
    fn hooks() -> bool {
        crate::cmd::file::set_io(crate::cmd::file::IoHooks {
            read: |_, _| Err("no reader in tests".into()),
            write: |d, name| Ok(format!("{name}:{}", d.model.len()).into_bytes()),
            plot: None,
        });
        crate::cmd::file::io().is_some_and(|h| (h.write)(&Drawing::new_imperial(), "probe.dxf").is_ok())
    }

    /// A save run through [`step`] stops at writing the bytes with nothing changed; with the bytes
    /// provided it saves as a plain save does.
    #[test]
    fn a_save_waits_for_its_bytes_and_then_finishes() {
        if !hooks() {
            return;
        }
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        let dir = std::env::temp_dir().join(format!("cadcraft-slow-{}", std::process::id()));
        let path = dir.join("a.dxf").to_string_lossy().to_string();
        let log = s.log.len();
        let Step::Slow(work) = step(&mut s, "saveas", &json!({ "path": path })) else { panic!("the save did not wait for its bytes") };
        assert_eq!(work.kind(), "save");
        assert!(!std::path::Path::new(&path).exists(), "nothing written yet");
        assert!(s.state().unwrap().path.is_none(), "the drawing is not saved yet");
        assert_eq!(s.log.len(), log, "no messages from the stopped run");
        provide(work.run());
        let Step::Done(r) = step(&mut s, "saveas", &json!({ "path": path })) else { panic!("the bytes were provided") };
        r.unwrap();
        assert!(std::path::Path::new(&path).exists());
        assert_eq!(s.state().unwrap().path.as_deref(), Some(path.as_str()));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Outside [`step`] the hooks work as always.
    #[test]
    fn calls_outside_a_step_are_not_deferred() {
        if !hooks() {
            return;
        }
        let mut s = Session::new();
        let dir = std::env::temp_dir().join(format!("cadcraft-slow-plain-{}", std::process::id()));
        let path = dir.join("b.dxf").to_string_lossy().to_string();
        s.execute("saveas", &json!({ "path": path })).unwrap();
        assert!(std::path::Path::new(&path).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
