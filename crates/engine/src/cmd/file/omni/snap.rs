//! OMNI-SNAP, and the version a save leaves behind.

use std::path::Path;

use serde_json::{Value, json};

use crate::cmd::{bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

use super::super::{SaveAs, store, write_bytes};
use super::HISTORY_DIR;
use super::disk::{drawing_file, folder_of, gzip, history_dir, read_snap, size, snapshots};
use super::name::{COMPRESSED_EXT, DRAWING_EXTS, SAVE_DESCRIPTION, ext_of, slug, user, user_part_of};
use super::time::{iso, now};
/// OMNI-SNAP: write a snapshot of the drawing as it is now.
pub(super) fn run_snap(s: &mut Session, p: &Value) -> Result<Value> {
    let file = drawing_file(s, "omni-snap", p)?;
    let dir = history_dir(&folder_of(&file));
    std::fs::create_dir_all(&dir).map_err(|e| bad("omni-snap", format!("{}: {e}", dir.display())))?;
    let (stamp, secs, ms) = now();
    let word = |t: Option<&str>, default: &str| {
        let s = slug(t.unwrap_or(default), "-");
        if s.is_empty() { default.to_string() } else { s }
    };
    let description = word(str_param(p, "description"), "snapshot");
    let me = user_part_of(&user());
    let drawing = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The stamp's milliseconds keep two snapshots taken in the same second apart.
    let name = format!("{stamp}_{ms:03}_{me}_{description}_{drawing}");
    let name = format!("{name}.{COMPRESSED_EXT}");
    let target = dir.join(&name);
    let target = target.to_string_lossy().into_owned();
    // The snapshot is the drawing as it is now, unsaved changes included: it goes through the
    // same writer a save uses, never through a copy of the file on disk (which would be older).
    let plain = dir.join(name.strip_suffix(&format!(".{COMPRESSED_EXT}")).unwrap_or(&name));
    let bytes = write_bytes(s, "omni-snap", &plain.to_string_lossy(), &SaveAs::default())?;
    let packed = gzip("omni-snap", &bytes)?;
    store(&target, &packed)?;
    let (list, _) = snapshots(&dir);
    s.echo(format!("[OMNI] Snapshot saved: {name} ({} in {HISTORY_DIR}).", list.len()));
    Ok(json!({
        "file": name,
        "path": target,
        "folder": dir.to_string_lossy(),
        "bytes": packed.len(),
        "size": bytes.len(),
        "compressed": true,
        "time": iso(secs, ms),
        "user": me,
        "description": description,
        "snapshots": list.len(),
    }))
}

/// A save leaves a version behind. The save path calls this once the drawing's own file is
/// written; nothing here may fail the save — a history that cannot be written is a warning on the
/// command line, never a lost drawing.
///
/// Returns what was written (or `None` when the version would repeat the newest one).
pub fn after_save(s: &mut Session, path: &str, bytes: &[u8]) -> Option<Value> {
    let file = Path::new(path);
    if !DRAWING_EXTS.contains(&ext_of(file).as_str()) {
        return None;
    }
    match version_of_save(s, file, bytes) {
        Ok(v) => v,
        Err(msg) => {
            s.echo(format!("[OMNI] The save left no version: {msg}"));
            None
        }
    }
}

/// The version file a save writes, or `None` when it would be a copy of the newest.
pub(super) fn version_of_save(s: &mut Session, file: &Path, bytes: &[u8]) -> std::result::Result<Option<Value>, String> {
    let dir = history_dir(&folder_of(file));
    let (list, _) = snapshots(&dir);
    let packed = gzip("omni-snap", bytes).map_err(|e| e.to_string())?;
    // An edit and its undo, or a restore of the newest version, come back to what the history
    // already holds: that is not a change the user made since the last save, so it leaves no
    // version behind. A compressed version is compared as it is stored (gzip is deterministic),
    // a plain one left by an older build as it is.
    if let Some(newest) = list.first() {
        let same = if newest.compressed {
            newest.bytes as usize == packed.len() && std::fs::read(&newest.path).is_ok_and(|b| b == packed)
        } else {
            read_snap(newest, "omni-snap").is_ok_and(|b| b == bytes)
        };
        if same {
            return Ok(None);
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (stamp, secs, ms) = now();
    let drawing = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = format!("{stamp}_{ms:03}_{}_{}_{}.{COMPRESSED_EXT}", user_part_of(&user()), SAVE_DESCRIPTION, drawing);
    let target = dir.join(&name);
    store(&target.to_string_lossy(), &packed).map_err(|e| e.to_string())?;
    s.echo(format!("[OMNI] Version saved: {name} ({} → {}).", size(bytes.len() as u64), size(packed.len() as u64)));
    Ok(Some(json!({
        "file": name,
        "path": target.to_string_lossy(),
        "folder": dir.to_string_lossy(),
        "time": iso(secs, ms),
        "bytes": packed.len(),
        "size": bytes.len(),
        "description": SAVE_DESCRIPTION,
        "kind": "save",
        "versions": snapshots(&dir).0.len(),
    })))
}

/// OMNI-SNAP typed at the command line: the description, then the snapshot.
pub(super) struct SnapM;

impl Interactive for SnapM {
    fn name(&self) -> &'static str {
        "OMNI-SNAP"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter a description for this snapshot", Accept::TEXT).default("snapshot")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let description = match &i {
            Input::Text(t) => t.trim().to_string(),
            Input::Enter => "snapshot".to_string(),
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        match run_snap(s, &json!({ "description": description })) {
            Ok(_) => Ok(Step::Done),
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}
