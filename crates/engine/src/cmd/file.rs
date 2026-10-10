//! File menu: new, close, switching drawings. Open/save of DWG/DXF go through the io layer,
//! registered by the host (desktop/CLI) via [`crate::cmd::file::set_io`].

use std::sync::OnceLock;

use cadcraft_doc::Drawing;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Plot a space to PDF bytes with JSON options (`{paper?, landscape?, fit?, lineweights?, …}`).
pub type PlotHook = fn(&Drawing, &cadcraft_doc::Space, &Value) -> std::result::Result<Vec<u8>, String>;

/// File format hooks (installed by the app so the engine stays I/O-agnostic and wasm-safe).
pub struct IoHooks {
    pub read: fn(&[u8], &str) -> std::result::Result<Drawing, String>,
    pub write: fn(&Drawing, &str) -> std::result::Result<Vec<u8>, String>,
    /// PLOT / EXPORTPDF (optional: builds without a plotter report "not available").
    pub plot: Option<PlotHook>,
}

static IO: OnceLock<IoHooks> = OnceLock::new();

pub fn set_io(h: IoHooks) {
    let _ = IO.set(h);
}
pub fn io() -> Option<&'static IoHooks> {
    IO.get()
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("new", "New Drawing...", run_new)
            .menu(&["File", "New Drawing..."])
            .key("Cmd+N")
            .alias(&["qnew"])
            .params("{metric?: bool}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("open", "Open...", run_open)
            .menu(&["File", "Open..."])
            .key("Cmd+O")
            .params("{path} | {data: base64, name}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("close", "Close", run_close).menu(&["File", "Close"]).key("Cmd+W").params("{index?}").noundo(),
        CommandSpec::new("closeall", "Close All", run_closeall).menu(&["File", "Close All"]).enabled(always).noundo(),
        CommandSpec::new("qsave", "Save", run_qsave_reporting).menu(&["File", "Save"]).key("Cmd+S").alias(&["save"]).params("{path?}").noundo(),
        CommandSpec::new("saveas", "Save As...", run_saveas_reporting)
            .menu(&["File", "Save As..."])
            .key("Cmd+Shift+S")
            .params("{path, format?: dxf|dwg}")
            .noundo(),
        CommandSpec::new("document.switch", "Switch Drawing", run_switch).params("{index}").enabled(always).noundo(),
        CommandSpec::new("document.bytes", "Drawing as bytes", run_bytes).params("{format?: dxf} → {data: base64}").noundo(),
    ]
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let i = s.new_drawing(bool_or(p, "metric", false));
    Ok(json!({ "index": i, "title": s.docs.get(i).map(|d| d.title.clone()) }))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

pub(crate) fn open_bytes(s: &mut Session, bytes: &[u8], name: &str, path: Option<String>) -> Result<usize> {
    let hooks = io().ok_or_else(|| bad("open", "file formats are not available in this build"))?;
    let d = (hooks.read)(bytes, name).map_err(|e| bad("open", e))?;
    Ok(s.open_drawing(d, &file_name(name), path))
}

fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(_path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let path = _path;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = std::fs::read(path).map_err(|e| bad("open", format!("{path}: {e}")))?;
            let i = open_bytes(s, &bytes, path, Some(path.to_string()))?;
            return Ok(json!({ "index": i, "entities": s.docs.get(i).map(|d| d.doc.entity_count()) }));
        }
        #[cfg(target_arch = "wasm32")]
        return Err(bad("open", "paths are not available on the web; pass `data`"));
    }
    let data = str_param(p, "data").ok_or_else(|| bad("open", "`path` or `data` (base64) is required"))?;
    let bytes = base64_decode(data).ok_or_else(|| bad("open", "invalid base64"))?;
    let name = str_param(p, "name").unwrap_or("Drawing.dxf");
    let i = open_bytes(s, &bytes, name, None)?;
    Ok(json!({ "index": i }))
}

fn run_close(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(s.active);
    if i >= s.docs.len() {
        return Err(bad("close", "no such drawing"));
    }
    s.cancel();
    s.docs.remove(i);
    if i < s.active {
        // An earlier drawing went away: the active one shifted down by one.
        s.active -= 1;
    }
    if s.active >= s.docs.len() {
        s.active = s.docs.len().saturating_sub(1);
    }
    ok()
}

fn run_closeall(s: &mut Session, _p: &Value) -> Result<Value> {
    s.cancel();
    s.docs.clear();
    s.active = 0;
    ok()
}

pub(crate) fn save_to(s: &mut Session, path: &str) -> Result<usize> {
    let hooks = io().ok_or_else(|| bad("save", "file formats are not available in this build"))?;
    let bytes = (hooks.write)(s.doc()?, path).map_err(|e| bad("save", e))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tmp = format!("{path}.cadcraft-tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| bad("save", format!("{path}: {e}")))?;
        std::fs::rename(&tmp, path).map_err(|e| bad("save", format!("{path}: {e}")))?;
    }
    let st = s.state_mut()?;
    st.saved = st.doc.clone();
    st.path = Some(path.to_string());
    st.title = file_name(path);
    Ok(bytes.len())
}

fn run_save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match str_param(p, "path") {
        Some(p) => p.to_string(),
        None => s.state()?.path.clone().ok_or_else(|| bad("qsave", "drawing has no file name yet; use saveas {path}"))?,
    };
    let n = save_to(s, &path)?;
    Ok(json!({ "path": path, "bytes": n }))
}

fn run_saveas(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("saveas", "`path` is required"))?.to_string();
    let n = save_to(s, &path)?;
    Ok(json!({ "path": path, "bytes": n }))
}

fn run_switch(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("document.switch", "`index` is required"))? as usize;
    if i >= s.docs.len() {
        return Err(bad("document.switch", "no such drawing"));
    }
    s.cancel();
    s.active = i;
    ok()
}

fn run_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("document.bytes", "file formats are not available in this build"))?;
    let fmt = str_param(p, "format").unwrap_or("dxf");
    let lost = not_saved(s.doc()?);
    let bytes = (hooks.write)(s.doc()?, &format!("drawing.{fmt}")).map_err(|e| bad("document.bytes", e))?;
    let mut r = json!({ "data": base64_encode(&bytes), "bytes": bytes.len() });
    if is_drawing_file(&format!("drawing.{fmt}")) {
        report_not_saved(s, &mut r, &lost);
    }
    Ok(r)
}

/// Entities a DXF or DWG file doesn't keep: those of object types CADCraft doesn't model
/// (MLINE, 3DSOLID, MULTILEADER… read from other programs' files and kept in the drawing, but
/// not written), counted by DXF type across model space, layouts and block definitions.
pub fn not_saved(d: &Drawing) -> std::collections::BTreeMap<String, usize> {
    let mut out: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for e in stores.flat_map(|s| s.iter()) {
        if let cadcraft_doc::EntityKind::Unknown(u) = &e.kind {
            let n = out.entry(u.dxf_type.clone()).or_insert(0);
            *n = n.saturating_add(1);
        }
    }
    out
}

/// The command-line warning for [`not_saved`] entities, or `None` when the file keeps everything.
pub fn not_saved_message(lost: &std::collections::BTreeMap<String, usize>) -> Option<String> {
    if lost.is_empty() {
        return None;
    }
    let list: Vec<String> = lost.iter().map(|(t, n)| format!("{n} {t}")).collect();
    Some(format!("Not saved (CADCraft can't write these object types yet): {}", list.join(", ")))
}

/// Whether `path` is saved as a drawing (DXF, DWG) rather than exported as an image.
fn is_drawing_file(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "dxf" | "dwg" | "")
}

/// Say on the command line what the file couldn't keep, and list it in the result's `notSaved`.
fn report_not_saved(s: &mut Session, r: &mut Value, lost: &std::collections::BTreeMap<String, usize>) {
    let Some(msg) = not_saved_message(lost) else { return };
    s.echo(msg);
    if let Some(o) = r.as_object_mut() {
        o.insert("notSaved".into(), json!(lost));
    }
}

/// Save, then report the entities the file couldn't keep (nothing is dropped silently).
fn save_reporting(s: &mut Session, p: &Value, run: fn(&mut Session, &Value) -> Result<Value>) -> Result<Value> {
    let lost = s.doc().map(not_saved).unwrap_or_default();
    let mut r = run(s, p)?;
    if r.get("path").and_then(Value::as_str).is_some_and(is_drawing_file) {
        report_not_saved(s, &mut r, &lost);
    }
    Ok(r)
}

fn run_qsave_reporting(s: &mut Session, p: &Value) -> Result<Value> {
    save_reporting(s, p, run_save)
}

fn run_saveas_reporting(s: &mut Session, p: &Value) -> Result<Value> {
    save_reporting(s, p, run_saveas)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn active_title(s: &Session) -> String {
        s.state().unwrap().title.clone()
    }

    #[test]
    fn closing_earlier_inactive_drawing_keeps_active_drawing() {
        let mut s = Session::new();
        s.execute("new", &json!({})).unwrap();
        s.execute("new", &json!({})).unwrap();
        s.execute("document.switch", &json!({"index": 1})).unwrap();
        assert_eq!(active_title(&s), "Drawing2");
        s.execute("close", &json!({"index": 0})).unwrap();
        assert_eq!(active_title(&s), "Drawing2");
        assert_eq!(s.docs.len(), 2);
    }

    #[test]
    fn closing_later_inactive_drawing_keeps_active_drawing() {
        let mut s = Session::new();
        s.execute("new", &json!({})).unwrap();
        s.execute("new", &json!({})).unwrap();
        s.execute("document.switch", &json!({"index": 1})).unwrap();
        s.execute("close", &json!({"index": 2})).unwrap();
        assert_eq!(active_title(&s), "Drawing2");
    }
}
