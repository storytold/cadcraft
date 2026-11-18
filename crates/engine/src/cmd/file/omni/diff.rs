//! OMNI-DIFF: a version laid under the drawing, and the report of what changed.

use std::path::Path;

use cadcraft_color::Color;
use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::cmd::blocks::insert_with;
use crate::cmd::{bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

use super::super::{compare, io};
use super::disk::{history_of, materialize, overlay_path, read_snap};
use super::log::echo_list;
use super::name::{Snap, chosen, overlay_name};
use super::overlay::{overlays, recolour};

/// Whether the block `name` is placed anywhere in the drawing.
fn placed(s: &Session, name: &str) -> bool {
    let ins = |st: &cadcraft_doc::EntityStore| st.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)));
    let Ok(d) = s.doc() else { return false };
    ins(&d.model) || d.layouts.iter().any(|l| ins(&l.entities)) || d.blocks.values().filter(|b| !b.name.contains('|')).any(|b| ins(&b.entities))
}

/// The handles of the overlay references OMNI itself laid under a drawing: a report is about the
/// drawing, not about the underlay the tool put there. The overlay's name is what its references
/// are counted by, so a user's `OMNI_…` reference is not left out of the report either.
fn overlay_handles(d: &cadcraft_doc::Drawing, space: &cadcraft_doc::Space) -> Vec<cadcraft_doc::Handle> {
    let ours = overlays(d);
    let mine = |name: &str| ours.iter().any(|n| n.eq_ignore_ascii_case(name));
    d.space(space)
        .map(|st| st.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if mine(&i.block))).map(|e| e.handle).collect())
        .unwrap_or_default()
}
/// Attach `snap` as an overlay reference of the open drawing (a snapshot that is already attached
/// is reused), and recolour everything it brings to `color`, so the old revision reads as a
/// background under the new one. Returns the reference's name, how many layers it recoloured and
/// how many objects.
pub(super) fn attach_overlay(s: &mut Session, snap: &Snap, color: Color) -> Result<(String, usize, usize)> {
    let file = materialize(snap, "omni-diff")?;
    let host = s.state()?.path.clone();
    let path = match host.as_deref() {
        Some(h) => overlay_path(Path::new(h), &file),
        None => file.to_string_lossy().into_owned(),
    };
    let base = overlay_name(snap);
    // Reuse a block under this name only when it is this snapshot's own overlay (its `xref_path`
    // is the file this snapshot resolves to). A block of the same name left by another version, or
    // attached by the user, must not be laid under the drawing as if it were this revision.
    let name = match s.doc()?.block(&base) {
        Some(b) if b.xref_path.as_deref() == Some(path.as_str()) => base,
        Some(_) => free_overlay_name(s, &base)?,
        None => base,
    };
    if s.doc()?.block(&name).is_some() {
        // Already attached: only make sure it is placed (its reference may have been erased).
        if !placed(s, &name) {
            insert_with(s, &name, Vec2::ZERO, Vec2::new(1.0, 1.0), 0.0, None, &serde_json::Map::new())?;
        }
    } else {
        s.execute("xattach", &json!({ "path": path, "name": name, "overlay": true, "at": [0.0, 0.0] }))?;
    }
    let (layers, objects) = recolour(s, &name, color)?;
    Ok((name, layers, objects))
}

/// The most look-alike names an overlay will try before giving up: a bound, so a drawing full of
/// `OMNI_…` blocks cannot make this walk forever.
const MAX_OVERLAY_NAMES: u32 = 99;

/// The first free overlay name: `base`, else `base_2`, `base_3`… up to [`MAX_OVERLAY_NAMES`].
fn free_overlay_name(s: &Session, base: &str) -> Result<String> {
    let doc = s.doc()?;
    if doc.block(base).is_none() {
        return Ok(base.to_string());
    }
    for n in 2..=MAX_OVERLAY_NAMES {
        let candidate = format!("{base}_{n}");
        if doc.block(&candidate).is_none() {
            return Ok(candidate);
        }
    }
    Err(bad("omni-diff", format!("{base}: too many overlays share this name")))
}

/// The native comparison of the open drawing with `snap`: the same object matching COMPARE does
/// (geometry and properties, handle first), reported as numbers and change areas. The underlay a
/// previous OMNI-DIFF laid under the drawing (or that is attached in the snapshot) is not counted:
/// attaching it is the tool's own doing, not a change the user made.
pub(super) fn report(s: &Session, snap: &Snap) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("omni-diff", "file formats are not available in this build"))?;
    let bytes = read_snap(snap, "omni-diff")?;
    let other = (hooks.read)(&bytes, &snap.inner).map_err(|e| bad("omni-diff", format!("{}: {e}", snap.file)))?;
    let st = s.state()?;
    let (cur, space) = (st.doc.clone(), st.space.clone());
    let mut d = compare::diff(&cur, &other, &space);
    let (mine, theirs) = (overlay_handles(&cur, &space), overlay_handles(&other, &space));
    d.added.retain(|h| !mine.contains(h));
    d.removed.retain(|h| !theirs.contains(h));
    d.changed.retain(|(a, b, _, _)| !mine.contains(a) && !theirs.contains(b));
    let regions = compare::diff_regions(&cur, &other, &d);
    Ok(json!({
        "counts": { "added": d.added.len(), "removed": d.removed.len(), "changed": d.changed.len(), "common": d.common },
        "regions": regions.iter().map(|b| json!([[b.min.x, b.min.y], [b.max.x, b.max.y]])).collect::<Vec<_>>(),
        "truncated": d.truncated,
        "message": format!(
            "Compared with {}: {} only in this drawing, {} only in the snapshot, {} changed, {} the same{}.",
            snap.file,
            d.added.len(),
            d.removed.len(),
            d.changed.len(),
            d.common,
            if d.truncated { " (the comparison stopped early: too many objects)" } else { "" }
        ),
    }))
}

/// OMNI-DIFF: lay a snapshot under the drawing (an overlay reference, recoloured), report what
/// changed between the two, or both.
pub(super) fn run_diff(s: &mut Session, p: &Value) -> Result<Value> {
    let (dir, list, _) = history_of(s, "omni-diff", p)?;
    if list.is_empty() {
        return Err(bad("omni-diff", format!("no snapshots in {}: take one with OMNI-SNAP first", dir.display())));
    }
    let sel = p.get("snapshot").filter(|v| !v.is_null()).cloned().unwrap_or(json!(1));
    let snap = chosen(&list, &sel).ok_or_else(|| bad("omni-diff", format!("no snapshot {}", json!(sel))))?;
    let mode = str_param(p, "mode").unwrap_or("overlay").trim().to_ascii_lowercase();
    if !["overlay", "report", "both"].contains(&mode.as_str()) {
        return Err(bad("omni-diff", format!("`mode` must be overlay, report or both, not \"{mode}\"")));
    }
    let color = match p.get("color") {
        None | Some(Value::Null) => 1u64,
        Some(v) => v.as_u64().ok_or_else(|| bad("omni-diff", "`color` must be an ACI colour 1-255"))?,
    };
    let aci = u8::try_from(color)
        .ok()
        .filter(|v| (1..=255).contains(v))
        .ok_or_else(|| bad("omni-diff", "`color` must be an ACI colour 1-255 (1 is red)"))?;
    let mut out = json!({ "file": snap.file, "path": snap.path.to_string_lossy(), "time": snap.time(), "mode": mode });
    // The report comes first when both are asked for: the underlay is laid under the *drawing*,
    // and its own reference would otherwise be counted as something only this drawing has.
    if mode != "overlay" {
        let r = report(s, &snap)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        for (k, v) in r.as_object().into_iter().flatten() {
            out[k] = v.clone();
        }
    }
    if mode != "report" {
        let (name, layers, objects) = attach_overlay(s, &snap, Color::Index(aci))?;
        s.echo(format!(
            "[OMNI] {name}: {objects} object(s) on {layers} layer(s) laid under the drawing in ACI {aci}. Hide it with OMNI-TOGGLE, detach it with OMNI-CLEAR."
        ));
        out["overlay"] = json!(name);
        out["layers"] = json!(layers);
        out["objects"] = json!(objects);
    }
    s.touch();
    Ok(out)
}

/// OMNI-DIFF typed at the command line: the history, then the number to lay under the drawing.
pub(super) struct DiffM;

impl Interactive for DiffM {
    fn name(&self) -> &'static str {
        "OMNI-DIFF"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match history_of(s, "omni-diff", &json!({})) {
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
        Prompt::new("Enter the number of the snapshot to compare with (Enter: the newest, 0 to cancel)", Accept::NUMBER).default("1")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match &i {
            Input::Enter => "1".to_string(),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        if t == "0" {
            return Ok(Step::Done);
        }
        if t.parse::<usize>().is_err() {
            s.echo("Enter the number of a snapshot (0 to cancel).");
            return Ok(Step::Continue);
        }
        match run_diff(s, &json!({ "snapshot": t })) {
            Ok(_) => Ok(Step::Done),
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}
