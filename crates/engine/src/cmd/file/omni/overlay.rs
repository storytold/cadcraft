//! The underlay's visibility (OMNI-TOGGLE) and its removal (OMNI-CLEAR).

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, XrefStatus};
use serde_json::{Value, json};

use crate::cmd::blocks::xref::load::has_prefix;
use crate::cmd::{bad, str_param};
use crate::{Result, Session};

use super::{OVERLAY_PREFIX, is_version_path};
/// Whether `b` is a snapshot overlay: an external reference that reads a file inside a history
/// folder. That is what OMNI-DIFF attaches, and the reference's own record says so — the `OMNI_`
/// name prefix does not: `XATTACH` accepts a name like `OMNI_site`, and a reference of the user's
/// own must survive OMNI-CLEAR (detaching it would take its definition, its references and the
/// symbols they brought with it). A reference into the history, on the other hand, is a version of
/// the drawing laid under it, which is exactly what the commands are for.
pub(super) fn is_overlay_block(b: &cadcraft_doc::Block) -> bool {
    b.xref_path.as_deref().is_some_and(is_version_path) && !b.name.contains('|') && has_prefix(&b.name, OVERLAY_PREFIX)
}

/// The overlay references attached to `d` (and nothing else).
pub(super) fn overlays(d: &Drawing) -> Vec<String> {
    d.blocks.values().filter(|b| is_overlay_block(b)).map(|b| b.name.clone()).collect()
}

/// Recolour the layers and objects an overlay brought. Both, not only the layers as the AutoLISP
/// original did: an object that carries a colour of its own would otherwise stay visible in it.
pub(super) fn recolour(s: &mut Session, name: &str, color: Color) -> Result<(usize, usize)> {
    let prefix = format!("{name}|");
    let mine = |n: &str| n.eq_ignore_ascii_case(name) || has_prefix(n, &prefix);
    let d = s.doc_mut()?;
    let mut layers = 0;
    for l in d.layers.iter_mut().filter(|l| has_prefix(&l.name, &prefix)) {
        l.color = color;
        l.on = true;
        layers += 1;
    }
    let keys: Vec<String> = d.blocks.keys().filter(|k| mine(k)).cloned().collect();
    let mut objects = 0;
    for k in keys {
        let Some(b) = d.blocks.get_mut(&k) else { continue };
        let b = Arc::make_mut(b);
        for h in b.entities.handles() {
            if b.entities.modify(h, |e| e.common.color = color) {
                objects += 1;
            }
        }
    }
    Ok((layers, objects))
}

/// The overlays a call names (`snapshot` is an overlay name, or the file name of the snapshot it
/// shows), or all of them.
pub(super) fn overlays_named(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let d = s.doc()?;
    let pattern = str_param(p, "snapshot");
    let picked: Vec<String> = overlays(d)
        .into_iter()
        .filter(|n| match pattern {
            None => true,
            Some(f) => {
                // A listing shows a compressed version as `…_plan.dxf.gz`, while the reference
                // reads the file `.cache` unpacks — `…_plan.dxf`. A name copied from the listing
                // is the one the user means, so the compression suffix does not count.
                let f = f.trim();
                let plain = f.strip_suffix(".gz").or_else(|| f.strip_suffix(".GZ")).unwrap_or(f);
                let file = d.block(n).and_then(|b| b.xref_path.clone()).unwrap_or_default();
                let name = file.rsplit(['/', '\\']).next().unwrap_or_default();
                n == f || has_prefix(n, f) || file == f || name == f || name.eq_ignore_ascii_case(plain)
            }
        })
        .collect();
    if picked.is_empty() {
        return Err(bad(cmd, "no snapshot overlay matches: run OMNI-DIFF first"));
    }
    Ok(picked)
}

/// OMNI-TOGGLE: show or hide the snapshot overlays.
///
/// The overlay is unloaded and reloaded rather than hidden layer by layer. An external reference's
/// objects on layer 0 keep the *host's* layer 0 — in AutoCAD as here — so a layer toggle would
/// leave them in the drawing, and the AutoLISP original had exactly that hole. Unloading cannot
/// leave a stray object behind. The colours set on the overlay's layers stay with them, so
/// reloading brings the underlay back red.
pub(super) fn run_toggle(s: &mut Session, p: &Value) -> Result<Value> {
    let names = overlays_named(s, p, "omni-toggle")?;
    let show = match p.get("show") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad("omni-toggle", "`show` must be true or false")),
    };
    let showing = names.first().and_then(|n| s.doc().ok()?.block(n)).is_some_and(|b| b.xref_status != XrefStatus::Unloaded);
    let want = show.unwrap_or(!showing);
    s.execute("-xref", &json!({ "option": if want { "reload" } else { "unload" }, "names": names.join(",") }))?;
    if want {
        // Reloading reads the snapshot again, so what it brings is one colour again: the layers
        // keep the colour the overlay was given, and their objects are forced back to it.
        for n in &names {
            let prefix = format!("{n}|");
            let color = s.doc()?.layers.iter().find(|l| has_prefix(&l.name, &prefix)).map(|l| l.color).unwrap_or(Color::Index(1));
            recolour(s, n, color)?;
        }
    }
    let objects: usize = names.iter().filter_map(|n| s.doc().ok()?.block(n).map(|b| b.entities.len())).sum();
    s.echo(format!("[OMNI] Overlay {} ({} reference(s), {objects} object(s)).", if want { "shown" } else { "hidden" }, names.len()));
    Ok(json!({ "overlay": names, "shown": want, "objects": objects }))
}

/// OMNI-CLEAR: detach the overlays OMNI-DIFF attached, and only those.
pub(super) fn run_clear(s: &mut Session, p: &Value) -> Result<Value> {
    if overlays(s.doc()?).is_empty() {
        s.echo("[OMNI] No snapshot overlay to clear.");
        return Ok(json!({ "cleared": [], "erased": 0 }));
    }
    let names = overlays_named(s, p, "omni-clear")?;
    let r = s.execute("-xref", &json!({ "option": "detach", "names": names.join(",") }))?;
    let erased = r.get("erased").and_then(Value::as_u64).unwrap_or(0);
    s.echo(format!("[OMNI] Overlay cleared: {} reference(s), {erased} object(s) removed.", names.len()));
    Ok(json!({ "cleared": names, "erased": erased }))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlay_is_a_reference_into_the_history_not_a_name() {
        let overlay = |name: &str, path: Option<&str>| {
            let mut b = cadcraft_doc::Block::new(name);
            b.xref_path = path.map(str::to_string);
            is_overlay_block(&b)
        };
        assert!(overlay("OMNI_plan_20261014_083512", Some("_OMNI_HISTORY/plan.dxf")));
        assert!(overlay("OMNI_plan", Some("C:\\jobs\\_OMNI_HISTORY\\.cache\\plan.dxf")));
        // A user's `XATTACH OMNI_site` reads a drawing of their own, wherever it is called from.
        assert!(!overlay("OMNI_site", Some("site.dxf")));
        assert!(!overlay("OMNI_site", Some("/jobs/OMNI_site.dxf")));
        // Not a reference at all, or a symbol that came with one.
        assert!(!overlay("OMNI_plan", None));
        assert!(!overlay("OMNI_plan|x", Some("_OMNI_HISTORY/plan.dxf")));
    }
}
