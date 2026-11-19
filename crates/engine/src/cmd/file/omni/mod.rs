//! Drawing history — the OMNI toolkit.
//!
//! A *version* is a full copy of the drawing written into `_OMNI_HISTORY` next to it, named
//! `<date>.<time>_<milliseconds>_<user>_<description>_<file>.<ext>`. Versions arrive two ways:
//!
//! - **A save that changes the drawing leaves one behind**, without being asked.
//!   `SAVE`/`QSAVE`/`SAVEAS` write one when the drawing differs from what its file held (the
//!   unsaved-changes flag is the test), so a history is a timeline of the saves the user already
//!   makes. Those are filed under the description `save`. A save of an unchanged drawing adds
//!   nothing.
//! - **`OMNI-SNAP` takes a named milestone** of the drawing as it is right now — unsaved changes
//!   included, the file on disk untouched — for a state the user wants to name and come back to.
//!
//! `OMNI-LOG` lists the versions and opens one, `OMNI-DIFF` lays one under the current drawing as
//! an overlay reference recoloured to a single colour (red by default) and can also *report* what
//! changed between the two as numbers instead of pixels, `OMNI-TOGGLE` hides and shows that
//! overlay, `OMNI-CLEAR` detaches it again, `OMNI-RESTORE` puts a version back into the drawing
//! (undoably — the file on disk is never touched, and the document becomes the version's whole,
//! layers and settings included), and `OMNI-NOTE` drops a marker and a comment on the drawing.
//! `OMNI-VERSIONS` is the listing the Drawing History palette draws.
//!
//! A version is history, not a drawing of its own: opening one reads it, and a save onto it is
//! refused (see [`is_version_path`]) — `SAVEAS` is how an opened revision gets a name and a history
//! of its own.
//!
//! Versions are gzipped on disk (`…_plan.dxf.gz`): a DXF is verbose text and shrinks several times
//! over (about seven on the sample plan), so a long history does not cost what a folder of plain
//! copies would. A plain `.dxf`/`.dwg` left by an older build is still listed, read and compared;
//! an older version is unpacked into `_OMNI_HISTORY/.cache` only when it has to be a file on disk
//! (the overlay reference reads it back through the io hooks).
//!
//! The AutoLISP original this comes from only ever ran inside AutoCAD on DWG files and shelled
//! out to its `-XREF`/`-LAYER` command line. Here everything is native and format-agnostic: a
//! snapshot is written through the same io hooks as a save, so a DXF history is DXF and a DWG
//! history is DWG, and the overlay is read back through the same hooks. Nothing is
//! platform-specific — paths are built with [`std::path`], a snapshot reference is stored
//! relative to the drawing's folder with `/` (every platform CADCraft builds for accepts that),
//! the user name comes from the environment (`CADCRAFT_USER`, `USER`, `USERNAME`, `LOGNAME`), and
//! the stamp is UTC, so one history folder stays sortable and comparable on Windows, Linux, macOS
//! and FreeBSD alike. The commands that need a file system report "not available on the web" in
//! the browser build; the ones that only touch the drawing (TOGGLE, CLEAR, NOTE) work there too.
//!
//! The toolkit is split by what each part knows about: [`time`] turns a second into a UTC stamp,
//! [`name`] reads a version's name back into its fields, [`disk`] owns the history folder and its
//! files, and each group of commands has its own module — [`snap`], [`log`], [`diff`],
//! [`overlay`], [`note`]. Everything that needs a file system is gated on its `mod` line, so the
//! browser build compiles only what it can run and the stubs below answer the rest.

use super::super::CommandSpec;

#[cfg(not(target_arch = "wasm32"))]
mod diff;
#[cfg(not(target_arch = "wasm32"))]
mod disk;
#[cfg(not(target_arch = "wasm32"))]
mod log;
#[cfg(not(target_arch = "wasm32"))]
mod name;
mod note;
mod overlay;
#[cfg(not(target_arch = "wasm32"))]
mod snap;
#[cfg(not(target_arch = "wasm32"))]
mod time;

#[cfg(not(target_arch = "wasm32"))]
use diff::{DiffM, run_diff};
#[cfg(not(target_arch = "wasm32"))]
use log::{LogM, run_log, run_restore, run_versions};
use note::{NoteM, run_note};
use overlay::{run_clear, run_toggle};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use snap::after_save;
#[cfg(not(target_arch = "wasm32"))]
use snap::{SnapM, run_snap};

/// The history folder created next to a drawing (the AutoLISP toolkit's folder name, so an
/// existing history keeps working).
pub(super) const HISTORY_DIR: &str = "_OMNI_HISTORY";
/// Overlay references are named with this prefix, so a report tells the underlay from the drawing's
/// own objects at a glance. The name alone does not make a reference an overlay — `XATTACH` takes
/// any name — so what OMNI-CLEAR and OMNI-TOGGLE claim is decided by [`overlay::is_overlay_block`].
pub(super) const OVERLAY_PREFIX: &str = "OMNI_";
/// The layer OMNI-NOTE's markers go on.
pub(super) const NOTE_LAYER: &str = "OMNI_NOTES";
/// OMNI-NOTE's defaults, in drawing units (`radius` and `height` override them).
pub(super) const NOTE_RADIUS: f64 = 50.0;
pub(super) const NOTE_HEIGHT: f64 = 25.0;
/// A note's text is at most this long, as AutoCAD's TEXT.
pub(super) const MAX_TEXT: usize = 255;
/// What the file-system commands reply in a build without one (the browser). Only the browser
/// build has the commands that need it: everywhere else they are the real thing.
#[cfg(target_arch = "wasm32")]
const NO_FILES: &str = "the drawing history needs a file system: it is not available on the web";

/// Whether a path is a version inside a history folder (`_OMNI_HISTORY`, or its `.cache`). History
/// is the record of what a drawing's saves held, so nothing may be saved onto one: an opened
/// revision is a file to read, and `SAVEAS` is how it becomes a drawing with a name of its own.
pub(crate) fn is_version_path(path: &str) -> bool {
    // Both separators, whatever the host: `Path` splits on the host's separator only, so a path
    // written on Windows (`C:\jobs\_OMNI_HISTORY\a.dxf`) has a single component on Unix and a
    // history file read from a project that travels between machines would not be recognised.
    path.split(['/', '\\']).any(|part| part.eq_ignore_ascii_case(HISTORY_DIR))
}

pub fn specs() -> Vec<CommandSpec> {
    let snap = CommandSpec::new("omni-snap", "Save Snapshot", run_snap)
        .menu(&["Tools", "Drawing History", "Save Snapshot"])
        .alias(&["omnisnap"])
        .params("{description? (default \"snapshot\"; folded into the file name, where anything but letters, digits and `-` becomes `-`), path? (the DXF/DWG file this history belongs to)} → {file, path, folder, bytes, size, time, user, description, snapshots}")
        .noundo();
    #[cfg(not(target_arch = "wasm32"))]
    let snap = snap.interactive(|_| Ok(Box::new(SnapM)));

    let log = CommandSpec::new("omni-log", "Snapshot History...", run_log)
        .menu(&["Tools", "Drawing History", "Snapshot History..."])
        .alias(&["omnilog", "omni", "versions", "history"])
        .params("{path?, open? (a listed number, a file name or a path), limit?} → {folder, count, capped, snapshots: [{n, file, path, time, user, description, kind, drawing, bytes, compressed}], opened?}")
        .noundo();
    #[cfg(not(target_arch = "wasm32"))]
    let log = log.interactive(|_| Ok(Box::new(LogM)));

    let versions = CommandSpec::new("omni-versions", "Version History", run_versions)
        .params("{path?, limit? (default 200), kind?: save|milestone|all (default all)} → {folder, count, capped, versions: [{n, file, path, time, user, description, kind, drawing, bytes, compressed}]}: what the Drawing History palette lists, without echoing it to the command line")
        .noundo();

    let restore = CommandSpec::new("omni-restore", "Restore Version", run_restore)
        .params("{snapshot: a listed number or a file name, path?} → {restored, description, kind, time, versions}: put that version back into the drawing as an unsaved change (undoable; the file on disk is untouched, so nothing is lost by looking)")
        .noundo();

    let diff = CommandSpec::new("omni-diff", "Compare with Snapshot", run_diff)
        .menu(&["Tools", "Drawing History", "Compare with Snapshot"])
        .alias(&["omnidiff"])
        .params(
            "{snapshot? (1 = the newest, a file name or a path; default 1), mode?: overlay|report|both (default overlay), \
             color?: ACI 1-255 (default 1 = red), path?} → {overlay?, file, layers, objects, counts?, regions?, truncated?, message}",
        )
        .noundo();
    #[cfg(not(target_arch = "wasm32"))]
    let diff = diff.interactive(|_| Ok(Box::new(DiffM)));

    let toggle = CommandSpec::new("omni-toggle", "Show/Hide Snapshot", run_toggle)
        .menu(&["Tools", "Drawing History", "Show/Hide Snapshot"])
        .alias(&["omnitoggle"])
        .params("{show?: bool (default: flip), snapshot? (the overlay's name or the snapshot's file name)} → {overlay: [names], shown, objects}")
        .noundo();

    let clear = CommandSpec::new("omni-clear", "Clear Snapshot Overlay", run_clear)
        .menu(&["Tools", "Drawing History", "Clear Snapshot Overlay"])
        .alias(&["omniclear"])
        .params("{} → {cleared: [names], erased}: detaches the overlays OMNI-DIFF attached, and nothing else")
        .noundo();

    let note = CommandSpec::new("omni-note", "Add Note", run_note)
        .menu(&["Tools", "Drawing History", "Add Note"])
        .alias(&["omninote"])
        .params("{at: [x,y], text, height? (default 25), radius? (default 50), layer? (default OMNI_NOTES)} → {handle, textHandle, layer}")
        .interactive(|_| Ok(Box::new(NoteM::default())));

    vec![snap, log, versions, restore, diff, toggle, clear, note]
}

// ---------------------------------------------------------------- the browser build
//
// Without a file system there is no history to read or write, so the commands that need one answer
// so and the rest of the toolkit (TOGGLE, CLEAR, NOTE) stays exactly what it is natively.

#[cfg(target_arch = "wasm32")]
use serde_json::{Value, json};

#[cfg(target_arch = "wasm32")]
use super::super::bad;
#[cfg(target_arch = "wasm32")]
use crate::{Result, Session};

/// OMNI-SNAP needs a folder to write the milestone into.
#[cfg(target_arch = "wasm32")]
fn run_snap(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-snap", NO_FILES))
}

/// OMNI-LOG lists a folder.
#[cfg(target_arch = "wasm32")]
fn run_log(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-log", NO_FILES))
}

/// OMNI-VERSIONS is what the palette asks for: an empty listing, so the palette still opens.
#[cfg(target_arch = "wasm32")]
fn run_versions(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({ "folder": Value::Null, "count": 0, "capped": false, "versions": [], "note": NO_FILES }))
}

/// OMNI-RESTORE reads a version file.
#[cfg(target_arch = "wasm32")]
fn run_restore(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-restore", NO_FILES))
}

/// OMNI-DIFF reads a version file and attaches it as a reference.
#[cfg(target_arch = "wasm32")]
fn run_diff(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-diff", NO_FILES))
}

/// A save in the browser writes no files, so it leaves no version.
#[cfg(target_arch = "wasm32")]
pub(crate) fn after_save(_s: &mut Session, _path: &str, _bytes: &[u8]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_paths_are_recognised_in_any_case_and_folder() {
        for p in [
            "jobs/site/_OMNI_HISTORY/20261014.083512_482_u_save_plan.dxf.gz",
            "C:\\jobs\\_omni_history\\a.dxf",
            "jobs/site/_OMNI_HISTORY/.cache/a.dxf",
        ] {
            assert!(is_version_path(p), "{p}");
        }
        for p in ["jobs/site/plan.dxf", "jobs/_OMNI_HISTORY_FULL/plan.dxf", "", "plan.dxf"] {
            assert!(!is_version_path(p), "{p}");
        }
    }
}
