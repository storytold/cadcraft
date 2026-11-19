//! OMNI-LOG, OMNI-VERSIONS and OMNI-RESTORE: reading the timeline.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::cmd::{bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

use super::super::io;
use super::disk::{MAX_SNAPSHOTS, history_of, materialize, read_snap, size};
use super::name::{Snap, chosen};
use super::time::iso;
/// OMNI-VERSIONS: what the Drawing History palette lists, without the command-line echo and with
/// the kind filter the palette offers. A drawing that was never saved has no history — that is an
/// empty list rather than an error, so the palette still opens on an untitled drawing.
pub(super) fn run_versions(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind").unwrap_or("all").trim().to_ascii_lowercase();
    if !["all", "save", "milestone"].contains(&kind.as_str()) {
        return Err(bad("omni-versions", format!("`kind` must be all, save or milestone, not \"{kind}\"")));
    }
    let limit = p.get("limit").and_then(Value::as_u64).map(|l| l as usize).unwrap_or(200).min(MAX_SNAPSHOTS);
    let (folder, list, capped) = match history_of(s, "omni-versions", p) {
        Ok(v) => v,
        Err(EngineError::BadParams { msg, .. }) if str_param(p, "path").is_none() => {
            return Ok(json!({ "folder": Value::Null, "count": 0, "capped": false, "versions": [], "note": msg }));
        }
        Err(e) => return Err(e),
    };
    let versions: Vec<Snap> = list.into_iter().filter(|x| kind == "all" || x.kind() == kind).collect();
    Ok(json!({
        "folder": folder.to_string_lossy(),
        "count": versions.len(),
        "capped": capped,
        "versions": versions.iter().take(limit).enumerate().map(|(i, x)| x.json(i + 1)).collect::<Vec<_>>(),
    }))
}

/// OMNI-RESTORE: put a version back into the drawing. The drawing's own file is not touched, the
/// step undoes like any other change, and the next save writes it and leaves a version of its own,
/// so looking at an old revision costs nothing: the timeline carries on from it.
pub(super) fn run_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let want = p
        .get("snapshot")
        .filter(|v| !v.is_null())
        .cloned()
        .ok_or_else(|| bad("omni-restore", "say which version to restore: `snapshot`, a listed number or a file name"))?;
    let (_dir, list, _) = history_of(s, "omni-restore", p)?;
    let snap = chosen(&list, &want).ok_or_else(|| bad("omni-restore", format!("no version {want}")))?;
    if s.state()?.block_edit.is_some() {
        return Err(bad("omni-restore", "close the Block Editor first: it is editing a block of this drawing"));
    }
    let hooks = io().ok_or_else(|| bad("omni-restore", "file formats are not available in this build"))?;
    let bytes = read_snap(&snap, "omni-restore")?;
    let restored = (hooks.read)(&bytes, &snap.inner).map_err(|e| bad("omni-restore", format!("{}: {e}", snap.file)))?;
    let st = s.state_mut()?;
    let before = crate::Snapshot { label: "restore version".to_string(), doc: st.drawing().clone(), selection: st.selection.clone() };
    st.doc = Arc::new(restored);
    st.selection.clear();
    // A layout the restored version doesn't have sends the view back to the model tab, and the
    // tiled viewports belong to the version that had them.
    if let cadcraft_doc::Space::Paper(name) = &st.space
        && !st.doc.layouts.iter().any(|l| &l.name == name)
    {
        st.space = cadcraft_doc::Space::Model;
    }
    st.mspace = None;
    st.tiles = Default::default();
    st.revision += 1;
    st.push_undo(before);
    s.echo(format!("[OMNI] Restored {}. The file on disk is untouched; UNDO goes back.", snap.file));
    Ok(json!({
        "restored": snap.file,
        "time": snap.time(),
        "description": snap.description,
        "kind": snap.kind(),
        "dirty": true,
        "versions": list.len(),
    }))
}

/// The listing OMNI-LOG and OMNI-DIFF show.
pub(super) fn echo_list(s: &mut Session, dir: &Path, list: &[Snap], limit: usize) {
    if list.is_empty() {
        s.echo(format!("[OMNI] No snapshots in {}. Take one with OMNI-SNAP.", dir.display()));
        return;
    }
    s.echo(format!("[OMNI] Snapshot history ({} in {}):", list.len(), dir.display()));
    for (i, x) in list.iter().take(limit).enumerate() {
        let when = match x.epoch {
            Some(e) => iso(e, x.millis),
            None => "-".to_string(),
        };
        s.echo(format!("[{:>3}] {}  {:>8}  {:<12} {}", i + 1, when, size(x.bytes), x.description.chars().take(12).collect::<String>(), x.file));
    }
}

/// Open a snapshot as a drawing of its own (OMNI-LOG). A version stored gzipped is opened from
/// the history's cache, where it is unpacked once; the history it belongs to stays the drawing's
/// own (`history_host`), so browsing the timeline carries on in the opened revision.
fn open_snapshot(s: &mut Session, snap: &Snap) -> Result<Value> {
    let file = materialize(snap, "omni-log")?;
    s.execute("open", &json!({ "path": file.to_string_lossy() }))?;
    s.echo(format!("[OMNI] Opened {}.", snap.file));
    Ok(json!(snap.file))
}

/// OMNI-LOG: list the drawing's snapshots, and open one when asked to.
pub(super) fn run_log(s: &mut Session, p: &Value) -> Result<Value> {
    let (dir, list, capped) = history_of(s, "omni-log", p)?;
    let limit = p.get("limit").and_then(Value::as_u64).map(|l| l as usize).unwrap_or(list.len()).min(list.len());
    echo_list(s, &dir, &list, limit);
    let mut out = json!({
        "folder": dir.to_string_lossy(),
        "count": list.len(),
        "capped": capped,
        "snapshots": list.iter().take(limit).enumerate().map(|(i, x)| x.json(i + 1)).collect::<Vec<_>>(),
    });
    if let Some(sel) = p.get("open").filter(|v| !v.is_null()) {
        let snap = chosen(&list, sel).ok_or_else(|| bad("omni-log", format!("no snapshot {}", json!(sel))))?;
        out["opened"] = open_snapshot(s, &snap)?;
    }
    Ok(out)
}

/// OMNI-LOG typed at the command line: the history, then the number of the snapshot to open.
pub(super) struct LogM;

impl Interactive for LogM {
    fn name(&self) -> &'static str {
        "OMNI-LOG"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match history_of(s, "omni-log", &json!({})) {
            Ok((dir, list, _)) => {
                echo_list(s, &dir, &list, list.len());
                Ok(if list.is_empty() { Step::Cancel } else { Step::Continue })
            }
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Cancel)
            }
            Err(e) => Err(e),
        }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter the number of the snapshot to open (0 to cancel)", Accept::NUMBER).default("0")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match &i {
            Input::Enter => "0".to_string(),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        if t == "0" {
            return Ok(Step::Done);
        }
        let Ok((_, list, _)) = history_of(s, "omni-log", &json!({})) else { return Ok(Step::Done) };
        match chosen(&list, &json!(t)) {
            Some(snap) => {
                open_snapshot(s, &snap)?;
                Ok(Step::Done)
            }
            None => {
                s.echo(format!("\"{t}\" is not a snapshot in this drawing's history."));
                Ok(Step::Done)
            }
        }
    }
}
