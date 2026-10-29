//! LAYOUT SAveas: a layout written to a drawing template file (`.dwt`, or a `.dwg` or `.dxf`).
//!
//! The template holds the layout's page setup and paper space objects, viewports included, and
//! no model space objects and no other layout. Named objects the layout does not use (blocks,
//! layers, linetypes, text, dimension, multileader, multiline and table styles, empty groups)
//! are left out, as PURGE All removes them; layers the layout's viewports freeze or recolor stay.
//! The open drawing is not changed.

use std::collections::BTreeSet;

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, EntityStore, Point};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::super::file::{drops_unknown, io, not_saved, not_saved_message};
use super::super::{bad, str_param};
use super::layout_name;
use super::prompts::current_layout;
use crate::{Result, Session};

/// File types a layout template is written as; a path without an extension gets `.dwt`.
const TEMPLATE_TYPES: [&str; 3] = ["dwt", "dwg", "dxf"];

/// The most PURGE All passes run on a template; they stop at the first pass that purges nothing.
const MAX_PURGE_PASSES: usize = 64;

/// The file a SAveas typed at the command line suggests: `<layout>.dwt` beside the drawing.
pub(super) fn default_path(s: &Session, layout: &str) -> String {
    let file = format!("{layout}.dwt");
    match s.state().ok().and_then(|st| st.path.clone()) {
        Some(p) => std::path::Path::new(&p).with_file_name(&file).to_string_lossy().into_owned(),
        None => file,
    }
}

/// `path` with `.dwt` added when it has no extension; an error for any other file type.
fn template_path(path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(bad("layout.saveas", "`path` is empty"));
    }
    match std::path::Path::new(path).extension() {
        None => Ok(format!("{path}.dwt")),
        Some(e) if TEMPLATE_TYPES.iter().any(|t| e.eq_ignore_ascii_case(t)) => Ok(path.to_string()),
        Some(_) => Err(bad("layout.saveas", "a layout template is saved as a .dwt, .dwg or .dxf file")),
    }
}

/// The drawing a template of `layout` holds: `d` with that layout alone, an empty model space,
/// and without the named objects nothing left in it uses.
fn template_of(d: &Drawing, layout: &str) -> Result<Drawing> {
    let mut t = d.clone();
    t.layouts.retain(|l| l.name == layout);
    t.active_layout = Some(layout.to_string());
    t.model = EntityStore::new();
    t.hidden_objects.clear();
    let gone: Vec<u32> = t.constraints.iter().filter(|c| c.refs.iter().any(|r| t.entity(r.handle).is_none())).map(|c| c.id).collect();
    t.constraints.retain(|c| !gone.contains(&c.id));
    // Layers the layout's viewports freeze or recolor stay, with those settings. PURGE counts
    // only objects, so each such layer gets a point in model space while PURGE runs.
    let named: BTreeSet<String> = t
        .layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) => Some(v.frozen_layers.iter().cloned().chain(v.layer_colors.iter().map(|(n, _)| n.clone())).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    for layer in named {
        let handle = t.new_handle();
        t.model.push(Entity { handle, common: Common { layer, ..Common::default() }, kind: EntityKind::Point(Point { p: Vec3::ZERO, angle: 0.0 }) });
    }
    // PURGE runs on a session of its own, so the open drawing keeps everything.
    let mut scratch = Session::empty();
    scratch.open_drawing(t, layout, None);
    for _ in 0..MAX_PURGE_PASSES {
        let r = super::super::blocks::purge::run(&mut scratch, &json!({ "type": "all" }))?;
        let purged = r.get("purged").and_then(Value::as_object).is_some_and(|o| o.values().any(|v| v.as_array().is_some_and(|a| !a.is_empty())));
        if !purged {
            break;
        }
    }
    let st = scratch.docs.pop().ok_or_else(|| bad("layout.saveas", "the template could not be made"))?;
    let mut t = std::sync::Arc::unwrap_or_clone(st.doc);
    t.model = EntityStore::new();
    Ok(t)
}

/// `layout.saveas {name?, path}`: write a layout as a drawing template.
pub(super) fn run_saveas(s: &mut Session, p: &Value) -> Result<Value> {
    let path = template_path(str_param(p, "path").ok_or_else(|| bad("layout.saveas", "`path` is required"))?)?;
    if s.doc()?.refedit.is_some() {
        return Err(bad("layout.saveas", "A block reference is being edited in place: finish with REFCLOSE (Save or Discard) before saving."));
    }
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => current_layout(s).ok_or_else(|| bad("layout.saveas", "the drawing has no layout"))?,
    };
    // A file is the drawing outside the Block Editor.
    let d = s.state()?.drawing();
    let name = layout_name("layout.saveas", d, &name)?;
    let t = template_of(d, &name)?;
    let hooks = io().ok_or_else(|| bad("layout.saveas", "file formats are not available in this build"))?;
    let bytes = (hooks.write)(&t, &path).map_err(|e| bad("layout.saveas", e))?;
    store(&path, &bytes)?;
    let mut r = json!({ "name": name, "path": path, "bytes": bytes.len() });
    let lost = not_saved(&t);
    if drops_unknown(&path)
        && let Some(msg) = not_saved_message(&lost)
    {
        s.echo(msg);
        if let Some(o) = r.as_object_mut() {
            o.insert("notSaved".into(), json!(lost));
        }
    }
    Ok(r)
}

/// Write the template: natively to `path` through a temporary file; on the web as a download.
fn store(path: &str, bytes: &[u8]) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Exclusively create a fresh temporary file and rename it into place so a planted
        // `<path>.cadcraft-tmp` symlink cannot redirect the write (CWE-59).
        crate::atomic_write::write_atomic(std::path::Path::new(path), bytes).map_err(|e| bad("layout.saveas", format!("{path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        super::super::exchange::deliver_file("layout.saveas", path, bytes, super::super::file::deliver())
    }
}
