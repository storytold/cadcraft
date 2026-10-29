//! Drawing templates: NEW / QNEW from a template file (`.dwt`, which is DWG content, or a `.dxf`),
//! and the default template QNEW uses (the QNEWTEMPLATE setting).
//!
//! A new drawing from a template is the template's content (layers, styles, layouts, objects…)
//! as an untitled drawing: saving asks for a name, so the template file is never overwritten by
//! accident. CADCraft ships no template; templates are the user's files.

use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use cadcraft_doc::Drawing;
use serde_json::{Value, json};

use super::{Result, Session, bad, io};

/// The default template (profile setting, process-wide like FONTALT): empty = none.
fn default_template() -> MutexGuard<'static, String> {
    static QNEW: OnceLock<Mutex<String>> = OnceLock::new();
    QNEW.get_or_init(|| Mutex::new(String::new())).lock().unwrap_or_else(PoisonError::into_inner)
}

/// QNEWTEMPLATE: the template NEW/QNEW start from when no template is named ("" = none).
pub fn qnew_template() -> String {
    default_template().clone()
}

/// Set QNEWTEMPLATE; "" or "." clears it. The file is read when a drawing is made from it.
pub fn set_qnew_template(path: &str) {
    let p = path.trim();
    *default_template() = if p == "." { String::new() } else { p.to_string() };
}

/// The template a NEW call starts from: `template` ("" or "." for none), else the default
/// template unless `metric` asks for a blank drawing in given units.
fn template_for(p: &Value) -> Result<Option<String>> {
    match p.get("template") {
        Some(Value::String(t)) => Ok(Some(t.trim().to_string()).filter(|t| !t.is_empty() && t != ".")),
        Some(Value::Null) | None if p.get("metric").is_some_and(|m| !m.is_null()) => Ok(None),
        Some(Value::Null) | None => Ok(Some(qnew_template()).filter(|t| !t.is_empty())),
        Some(_) => Err(bad("new", "`template` must be a file path")),
    }
}

/// NEW from a template, or `None` when the call names none (a blank drawing then).
pub(super) fn run_new(s: &mut Session, p: &Value) -> Result<Option<Value>> {
    let Some(path) = template_for(p)? else { return Ok(None) };
    let d = read_template(&path)?;
    s.untitled_counter = s.untitled_counter.saturating_add(1);
    let title = format!("Drawing{}", s.untitled_counter);
    let i = s.open_drawing(d, &title, None);
    // An empty template shows its drawing limits.
    if let Some(st) = s.docs.get_mut(i)
        && st.doc.extents(&cadcraft_doc::Space::Model).is_empty()
        && let (Some(lo), Some(hi)) = (st.doc.header.point("LIMMIN"), st.doc.header.point("LIMMAX"))
        && lo.x.is_finite()
        && lo.y.is_finite()
        && hi.x.is_finite()
        && hi.y.is_finite()
        && hi.x > lo.x
        && hi.y > lo.y
    {
        let center = cadcraft_geom::Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        st.set_view_quiet(crate::View { center, height: (hi.y - lo.y).max(1e-6) });
    }
    Ok(Some(json!({ "index": i, "title": title, "template": path })))
}

/// Read a template drawing through the io hooks (DWG content for `.dwt`, or DXF).
fn read_template(path: &str) -> Result<Drawing> {
    let hooks = io().ok_or_else(|| bad("new", "file formats are not available in this build"))?;
    let bytes = read_bytes(path)?;
    (hooks.read)(&bytes, path).map_err(|e| bad("new", format!("template {path}: {e}")))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_bytes(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| bad("new", format!("template {path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn read_bytes(path: &str) -> Result<Vec<u8>> {
    Err(bad("new", format!("template {path}: paths are not available on the web")))
}
