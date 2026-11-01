use super::args::Framing;
use super::error::Result;
use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::{Session, View};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The process's output streams. A write to stdout that fails never stops the program: once
/// stdout has failed, later output is dropped and the work goes on, so the rest of `run` (later
/// `--cmd` results, `--save`, `--export`) still happens and `cadcraft-cli run … --save out.dxf |
/// head` keeps the drawing. A reader that stopped early (a closed pipe, `cadcraft-cli commands |
/// head`) is not an error, as in ripgrep; any other write error is reported once on stderr and
/// makes the exit status 1.
pub(crate) struct Streams<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    /// Set once a write to `out` has failed; later output is dropped.
    gone: bool,
    /// Set when `out` failed for a reason other than a closed reader.
    failed: bool,
}

impl<'a> Streams<'a> {
    pub(crate) fn new(out: &'a mut dyn Write, err: &'a mut dyn Write) -> Self {
        Streams { out, err, gone: false, failed: false }
    }

    /// Write a line to stdout.
    pub(crate) fn outln(&mut self, args: std::fmt::Arguments) {
        if !self.gone
            && let Err(e) = writeln!(self.out, "{args}")
        {
            self.out_failed(e);
        }
    }

    /// Write `v` to stdout: pretty-printed, or on one line with `compact` (`--compact`).
    pub(crate) fn json(&mut self, v: &serde_json::Value, compact: bool) {
        if compact {
            self.outln(format_args!("{v}"));
        } else {
            self.outln(format_args!("{v:#}"));
        }
    }

    /// Write a line to stderr. A failing stderr has nowhere to report to, so it is ignored.
    pub(crate) fn errln(&mut self, args: std::fmt::Arguments) {
        let _ = writeln!(self.err, "{args}");
    }

    /// Flush stdout (a pipe that closed during buffered output surfaces here).
    pub(crate) fn finish(&mut self) {
        if !self.gone
            && let Err(e) = self.out.flush()
        {
            self.out_failed(e);
        }
    }

    /// Whether stdout failed for a reason other than a closed reader: the exit status is 1.
    pub(crate) fn stdout_failed(&self) -> bool {
        self.failed
    }

    fn out_failed(&mut self, e: std::io::Error) {
        if std::mem::replace(&mut self.gone, true) || e.kind() == std::io::ErrorKind::BrokenPipe {
            return;
        }
        self.errln(format_args!("cadcraft-cli: can't write to stdout: {e}"));
        self.failed = true;
    }
}

/// Why writing or framing an output failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum OutputError {
    #[error("--view current: the drawing has no model-space view")]
    NoModelView,
    #[error("{}: {source}", path.display())]
    Write { path: PathBuf, source: std::io::Error },
}

/// The model-space view of every open drawing, by drawing id (to tell whether commands changed it).
pub(crate) fn model_views(s: &Session) -> Vec<(u64, Option<View>)> {
    s.docs.iter().map(|st| (st.uid, st.views.iter().find(|(sp, _)| *sp == Space::Model).map(|(_, v)| *v))).collect()
}

/// The active drawing's current model-space view as a rectangle with the viewport's aspect ratio
/// (the CLI viewport is 3:2 like the PNG export, so the image shows exactly this view).
fn current_window(s: &Session) -> Option<Bounds2> {
    let st = s.state().ok()?;
    let v = st.views.iter().find(|(sp, _)| *sp == Space::Model).map(|(_, v)| *v)?;
    let (w, h) = s.viewport_px;
    let half = Vec2::new(v.height * w / h.max(1.0), v.height) * 0.5;
    Some(Bounds2::new(v.center - half, v.center + half))
}

/// The window an image export shows, or `None` to fit the extents. `before` is [`model_views`]
/// taken before the commands ran (`None` for `convert`, where nothing ran).
pub(crate) fn export_window(s: &Session, framing: Framing, before: Option<&[(u64, Option<View>)]>) -> Result<Option<Bounds2>, OutputError> {
    match framing {
        Framing::Window(w) => Ok(Some(w)),
        Framing::Extents => Ok(None),
        Framing::Current => current_window(s).map(Some).ok_or(OutputError::NoModelView),
        Framing::Auto => {
            let Some(before) = before else { return Ok(None) };
            let Ok(st) = s.state() else { return Ok(None) };
            let now = model_views(s);
            let was = before.iter().find(|(uid, _)| *uid == st.uid);
            let is = now.iter().find(|(uid, _)| *uid == st.uid);
            // Only a view the commands changed counts; a drawing opened by the commands keeps
            // the extents framing.
            match (was, is) {
                (Some(a), Some(b)) if a.1 != b.1 => Ok(current_window(s)),
                _ => Ok(None),
            }
        }
    }
}

/// Write `bytes` to `out` through the engine's atomic writer (an exclusively created sibling file
/// renamed over `out`), so a failed or interrupted write leaves the previous file untouched. An
/// existing `out` is resolved first: the rename would otherwise replace a symlink with a file and
/// leave the file it points to (`--in-place` through a link) unchanged.
pub(crate) fn write_atomic(out: &Path, bytes: &[u8]) -> Result<(), OutputError> {
    let target = std::fs::canonicalize(out).unwrap_or_else(|_| out.to_path_buf());
    cadcraft_engine::atomic_write::write_atomic(&target, bytes).map_err(|source| OutputError::Write { path: out.to_path_buf(), source })
}

pub(crate) fn export(s: &Session, out: &Path, io: &mut Streams) -> Result {
    export_framed(s, out, None, io)
}

pub(crate) fn export_framed(s: &Session, out: &Path, window: Option<Bounds2>, io: &mut Streams) -> Result {
    let d = s.doc()?;
    let bytes = cadcraft_io::write_framed(d, out, window)?;
    write_atomic(out, &bytes)?;
    io.errln(format_args!("wrote {} ({} bytes)", out.display(), bytes.len()));
    if cadcraft_engine::cmd::file::drops_unknown(out) {
        let lost = cadcraft_engine::cmd::file::not_saved(d);
        if let Some(m) = cadcraft_engine::cmd::file::not_saved_message(&lost) {
            io.errln(format_args!("{m}"));
        }
    }
    Ok(())
}

/// The warning for the model-space entities of `path` an image can't show, by type (those kept
/// as unknown objects, `Proxy` in `info`), or `None` when there are none.
pub(crate) fn undrawn_message(d: &cadcraft_engine::doc::Drawing, path: &str) -> Option<String> {
    let mut types: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for e in d.model.iter() {
        if let cadcraft_engine::doc::EntityKind::Unknown(u) = &e.kind {
            let n = types.entry(u.dxf_type.as_str()).or_insert(0);
            *n = n.saturating_add(1);
        }
    }
    let n = types.values().fold(0usize, |a, n| a.saturating_add(*n));
    if n == 0 {
        return None;
    }
    let list: Vec<String> = types.iter().map(|(t, n)| format!("{t}: {n}")).collect();
    let entities = if n == 1 { "entity" } else { "entities" };
    Some(format!("{path}: {n} {entities} not drawn ({})", list.join(", ")))
}

/// The warning for the records of `path` the reader could not place, or `None` when there are none.
pub(crate) fn skipped_message(d: &cadcraft_engine::doc::Drawing, path: &str) -> Option<String> {
    let n = d.skipped_records.values().fold(0usize, |a, n| a.saturating_add(*n));
    if n == 0 {
        return None;
    }
    let list: Vec<String> = d.skipped_records.iter().map(|(section, n)| format!("{section}: {n}")).collect();
    let records = if n == 1 { "record" } else { "records" };
    Some(format!("{path}: {n} {records} could not be read and are left out ({})", list.join(", ")))
}
