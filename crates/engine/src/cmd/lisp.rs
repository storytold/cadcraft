//! AutoLISP & .NET (`NETLOAD` / `VBAIDE`) commands: APPLOAD loads a `.lsp` file; `NETLOAD` loads
//! `.dll` or `.lsp` applications; `dotnet.bridge` provides live drawing and interactive pick
//! operations to out-of-process `.NET` (`VB.NET` / `C#`) plugins.

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::{
    Block, Entity, EntityKind, HAlign, Handle, Insert, Line, Lineweight, LwPolyline, Polyline3d,
    RawTag, Space, Text, VAlign,
};
use cadcraft_geom::{Mat3, PolyVertex, Polyline, Vec2, Vec3};
use serde_json::{Value, json};

use super::{bad, str_param};
use crate::lisp::value::{Value as Lv, to_text};
use crate::{
    Accept, CommandSpec, DotNetPickState, EngineError, Input, Interactive, Prompt, Result, Session,
    Step,
};

/// Maximum size in bytes of an external block `.dwg` file loaded by `dotnet.bridge` (`insert_block`).
#[cfg(not(target_arch = "wasm32"))]
const MAX_BLOCK_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Command specifications for AutoLISP loading (`appload`, `netload`), the IDE (`vbaide`),
/// evaluation (`lisp`), and the `.NET` plugin bridge (`dotnet.bridge`).
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("appload", "Load Application...", run_appload)
            .menu(&["Tools", "Load Application..."])
            .alias(&["ap"])
            .params("{path: .lsp file} | {name, data: base64 | text} → {value?, commands: [commands the file defined], error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("netload", "Load .NET DLL / LISP Application (NETLOAD)...", run_appload)
            .menu(&["Tools", "Load Application (NETLOAD)..."])
            .params("{path: .dll or .lsp file} | {name, data: base64 | text} → {value?, error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("vbaide", "AutoLISP / VB.NET / C# IDE...", run_vbaide)
            .menu(&["Tools", "AutoLISP / VBA IDE..."])
            .alias(&["vlide", "vlisp"])
            .params("{path?: .lsp/.dll file, code?: LISP source} → {value?, output?, error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("lisp", "Evaluate LISP", run_lisp)
            .params(
                "{code: LISP source, inputs?: [answers to get… calls: numbers, strings, [x,y] points]} → {value (printed as prin1), output: lines printed, error?}",
            )
            .noundo(),
        CommandSpec::new("dotnet.bridge", ".NET Plugin Bridge", run_dotnet_bridge)
            .params("{op: state|entity|select_all|set_selection|highlight|set_xdata|add_lwpolyline|add_3dpoly|add_line|add_text|insert_block|update_entity|offset_lwpolyline|delete_entity|ensure_layer|begin_pick|poll_pick}"),
    ]
}

/// Whether `path` names an AutoLISP routine: APPLOAD loads `.lsp` files only, in any case.
fn is_lisp_path(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("lsp"))
}

/// The text of a LISP file from `{path}` or `{name, data | text}`, and its name.
fn source(p: &Value) -> Result<(String, String)> {
    if let Some(text) = str_param(p, "text") {
        return Ok((str_param(p, "name").unwrap_or("untitled.lsp").to_string(), text.to_string()));
    }
    if let Some(data) = str_param(p, "data") {
        let bytes = super::file::base64_decode(data).ok_or_else(|| bad("appload", "invalid base64"))?;
        return Ok((str_param(p, "name").unwrap_or("untitled.lsp").to_string(), String::from_utf8_lossy(&bytes).into_owned()));
    }
    // The path is read as it is given (a JSON caller may name a file or folder with spaces);
    // only a path of blanks alone is refused.
    let path = str_param(p, "path")
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| bad("appload", "`path`, or `name` with `data` (base64) or `text`, is required"))?;
    if !is_lisp_path(path) {
        return Err(bad("appload", format!("{path}: not an AutoLISP file (APPLOAD loads .lsp files)")));
    }
    let text = crate::lisp::builtins::read_text_file(path).ok_or_else(|| bad("appload", format!("{path}: can't read the file")))?;
    Ok((path.to_string(), text))
}

/// The result of a blocking LISP run as JSON, with the lines it printed.
fn report(s: &Session, from_log: usize, r: Option<std::result::Result<Lv, String>>) -> Value {
    let output: Vec<String> = s.log.get(from_log..).map(|l| l.to_vec()).unwrap_or_default();
    match r {
        Some(Ok(v)) => json!({ "value": to_text(&v, true), "output": output }),
        Some(Err(e)) => json!({ "error": e, "output": output }),
        None => json!({ "output": output }),
    }
}

/// Load and evaluate an AutoLISP routine from `{path}`, `{text}`, or base64 `{data}`.
fn run_appload(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, text) = source(p)?;
    let before = s.lisp_commands();
    let from = s.log.len();
    let r = s.lisp_load(&name, &text, true)?;
    let mut out = report(s, from, r);
    // The commands the file defined, so a caller can run them next (a failed file may still
    // have defined some before the error).
    out["commands"] = json!(s.lisp_commands().into_iter().filter(|c| !before.contains(c)).collect::<Vec<_>>());
    Ok(out)
}

/// Dispatch `VBAIDE` (`VLIDE`, `VLISP`): evaluates `code` when given, loads a file/text when given,
/// or reports `"open"` so the UI opens the AutoLISP IDE window.
fn run_vbaide(s: &mut Session, p: &Value) -> Result<Value> {
    if str_param(p, "code").is_some() {
        return run_lisp(s, p);
    }
    if str_param(p, "path").is_some() || str_param(p, "text").is_some() || str_param(p, "data").is_some() {
        return run_appload(s, p);
    }
    Ok(json!({ "status": "open" }))
}

/// A JSON input as a LISP value: numbers, strings, `[x, y(, z)]` points, null for Enter.
fn input_value(v: &Value) -> Lv {
    match v {
        Value::Array(a) if a.len() >= 2 && a.iter().all(Value::is_number) => Lv::point(&a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()),
        other => crate::lisp::drawing_from_json(other),
    }
}

/// Evaluate a LISP expression from `{code, inputs?}` and return its printed result and output lines.
fn run_lisp(s: &mut Session, p: &Value) -> Result<Value> {
    let code = str_param(p, "code").ok_or_else(|| bad("lisp", "`code` is required"))?;
    let inputs = p.get("inputs").and_then(Value::as_array).map(|a| a.iter().map(input_value).collect()).unwrap_or_default();
    let from = s.log.len();
    let r = s.lisp_eval(code, false, true, inputs).map_err(|e| match e {
        EngineError::Other(m) => bad("lisp", m),
        e => e,
    })?;
    Ok(report(s, from, r))
}

/// APPLOAD typed or chosen from the menu: asks for the file by name.
struct ApploadM;

impl Interactive for ApploadM {
    fn name(&self) -> &'static str {
        "APPLOAD"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Enter name of LISP file to load", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel | Input::Enter) {
            return Ok(Step::Cancel);
        }
        let Input::Text(t) = i else { return Ok(Step::Continue) };
        let path = t.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return Ok(Step::Cancel);
        }
        match source(&json!({ "path": path })) {
            Ok((name, text)) => {
                // The routine may wait for the user: it carries on after this command ends.
                if let Err(e) = s.lisp_load(&name, &text, false) {
                    s.echo(e.to_string());
                }
                Ok(Step::Done)
            }
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}

// ---------------- .NET Bridge (`dotnet.bridge`) ----------------

/// Return the AutoCAD ActiveX / ObjectDBX class name (`ObjectName`) for an entity kind.
fn acad_object_name(k: &EntityKind) -> &'static str {
    match k {
        EntityKind::LwPolyline(_) => "AcDbPolyline",
        EntityKind::Polyline3d(_) => "AcDb3dPolyline",
        EntityKind::Line(_) => "AcDbLine",
        EntityKind::Insert(_) => "AcDbBlockReference",
        EntityKind::Text(_) => "AcDbText",
        EntityKind::MText(_) => "AcDbMText",
        EntityKind::Arc(_) => "AcDbArc",
        EntityKind::Circle(_) => "AcDbCircle",
        EntityKind::Ellipse(_) => "AcDbEllipse",
        EntityKind::Point(_) => "AcDbPoint",
        EntityKind::Hatch(_) => "AcDbHatch",
        EntityKind::Spline(_) => "AcDbSpline",
        EntityKind::Viewport(_) => "AcDbViewport",
        _ => "AcDbEntity",
    }
}

/// Serialize a drawing entity into the JSON wire format expected by `cadcraft-nethost`.
fn dotnet_entity_json(d: &cadcraft_doc::Drawing, e: &Entity) -> Value {
    let b = cadcraft_doc::entity_bounds(d, e, 0);
    let bounds = if b.is_empty() {
        json!([[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]])
    } else {
        json!([[b.min.x, b.min.y, 0.0], [b.max.x, b.max.y, 0.0]])
    };
    let mut coords = Vec::new();
    let mut start_pt = [0.0, 0.0, 0.0];
    let mut end_pt = [0.0, 0.0, 0.0];
    let mut ins_pt = [0.0, 0.0, 0.0];
    let mut rotation = 0.0;
    let mut closed = false;
    let mut const_width = 0.0;
    let mut block_name = String::new();
    let mut text_val = String::new();
    match &e.kind {
        EntityKind::LwPolyline(pl) => {
            closed = pl.closed;
            const_width = pl.const_width;
            for v in &pl.vertices {
                coords.push(v.p.x);
                coords.push(v.p.y);
            }
            if let Some(first) = pl.vertices.first() {
                start_pt = [first.p.x, first.p.y, pl.elevation];
            }
            if let Some(last) = pl.vertices.last() {
                end_pt = [last.p.x, last.p.y, pl.elevation];
            }
        }
        EntityKind::Polyline3d(pl) => {
            closed = pl.closed;
            for p in &pl.points {
                coords.push(p.x);
                coords.push(p.y);
                coords.push(p.z);
            }
            if let Some(first) = pl.points.first() {
                start_pt = [first.x, first.y, first.z];
            }
            if let Some(last) = pl.points.last() {
                end_pt = [last.x, last.y, last.z];
            }
        }
        EntityKind::Line(l) => {
            start_pt = [l.a.x, l.a.y, l.a.z];
            end_pt = [l.b.x, l.b.y, l.b.z];
            coords.extend_from_slice(&[l.a.x, l.a.y, l.b.x, l.b.y]);
        }
        EntityKind::Arc(a) => {
            let s = a.center.xy() + Vec2::from_angle(a.start) * a.radius;
            let end = a.center.xy() + Vec2::from_angle(a.end) * a.radius;
            start_pt = [s.x, s.y, a.center.z];
            end_pt = [end.x, end.y, a.center.z];
            ins_pt = [a.center.x, a.center.y, a.center.z];
        }
        EntityKind::Circle(c) => {
            ins_pt = [c.center.x, c.center.y, c.center.z];
            start_pt = ins_pt;
        }
        EntityKind::Insert(ins) => {
            ins_pt = [ins.insert.x, ins.insert.y, ins.insert.z];
            rotation = ins.rotation;
            block_name = ins.block.clone();
        }
        EntityKind::Text(t) => {
            let jp = t.justify_point();
            ins_pt = [jp.x, jp.y, t.insert.z];
            rotation = t.rotation;
            text_val = t.value.clone();
        }
        EntityKind::MText(t) => {
            ins_pt = [t.insert.x, t.insert.y, t.insert.z];
            rotation = t.rotation;
            text_val = t.contents.clone();
        }
        EntityKind::Point(pt) => {
            ins_pt = [pt.p.x, pt.p.y, pt.p.z];
            start_pt = ins_pt;
        }
        _ => {}
    }
    let aci = i32::from(e.common.color.to_aci());
    let xdata: Vec<Value> = e
        .common
        .xdata
        .iter()
        .map(|t| json!({ "code": t.code, "value": t.value }))
        .collect();
    json!({
        "handle": e.handle.hex(),
        "objectName": acad_object_name(&e.kind),
        "layer": e.common.layer,
        "color": aci,
        "lineweight": e.common.lineweight.to_dxf(),
        "closed": closed,
        "constantWidth": const_width,
        "coordinates": coords,
        "startPoint": start_pt,
        "endPoint": end_pt,
        "insertionPoint": ins_pt,
        "rotation": rotation,
        "name": block_name,
        "text": text_val,
        "bounds": bounds,
        "xdata": xdata,
    })
}

/// Extract a finite [`Vec3`] from a JSON array `[x, y, z?]`, defaulting non-finite or missing components to `0.0`.
fn vec3_from_json(v: Option<&Value>) -> Vec3 {
    let Some(a) = v.and_then(Value::as_array) else { return Vec3::ZERO };
    let x = a.first().and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    let y = a.get(1).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    let z = a.get(2).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    Vec3::new(x, y, z)
}

/// Sanitize a block name so it cannot traverse directories when probing sibling `.dwg` files.
fn sanitize_block_name(raw: &str) -> String {
    let stem = std::path::Path::new(raw)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| raw.to_string());
    let cleaned: String = stem
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "SYMBOL".into()
    } else {
        trimmed.to_string()
    }
}

/// Handle a `dotnet.bridge` RPC operation from an out-of-process `.NET` plugin.
fn run_dotnet_bridge(s: &mut Session, p: &Value) -> Result<Value> {
    let op = str_param(p, "op").unwrap_or("state");
    match op {
        "state" => {
            let st = s.state()?;
            let (dwgprefix, dwgname) = match &st.path {
                Some(path) => {
                    let pb = std::path::Path::new(path);
                    let dir = pb.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
                    let file = pb.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| st.title.clone());
                    (dir, file)
                }
                None => {
                    let dir = std::env::temp_dir().to_string_lossy().into_owned();
                    let name = if st.title.ends_with(".dwg") || st.title.ends_with(".dxf") {
                        st.title.clone()
                    } else {
                        format!("{}.dwg", st.title)
                    };
                    (dir, name)
                }
            };
            Ok(json!({
                "selection": s.selection().iter().map(|h| h.hex()).collect::<Vec<_>>(),
                "prompting": s.prompting(),
                "dwgname": dwgname,
                "dwgprefix": dwgprefix,
                "clayer": st.doc.header.str("CLAYER", "0"),
                "osmode": s.settings.osmode,
                "mspace": !st.space.is_paper(),
                "pick": s.dotnet_pick,
            }))
        }
        "entity" => {
            let h_str = str_param(p, "handle").ok_or_else(|| bad("dotnet.bridge", "`handle` required"))?;
            let h = Handle::parse_hex(h_str).ok_or_else(|| bad("dotnet.bridge", "invalid hex handle"))?;
            let d = s.doc()?;
            let Some(e) = d.entity(h) else {
                return Ok(Value::Null);
            };
            Ok(dotnet_entity_json(d, e))
        }
        "select_all" => {
            let app_filter = str_param(p, "app").unwrap_or("").trim();
            let d = s.doc()?;
            let mut handles = Vec::new();
            let mut entities = Vec::new();
            for e in d.model.iter() {
                let matches = if app_filter.is_empty() || app_filter == "*" {
                    true
                } else {
                    e.common.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(app_filter))
                };
                if matches {
                    handles.push(e.handle.hex());
                    entities.push(dotnet_entity_json(d, e));
                }
            }
            Ok(json!({ "handles": handles, "entities": entities }))
        }
        "set_selection" => {
            let hs: Vec<Handle> = p
                .get("handles")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect())
                .unwrap_or_default();
            s.set_selection(hs);
            Ok(json!({ "ok": true }))
        }
        "highlight" => {
            if let Some(h) = str_param(p, "handle").and_then(Handle::parse_hex) {
                let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
                let mut sel = s.selection();
                if on {
                    if !sel.contains(&h) {
                        sel.push(h);
                    }
                } else {
                    sel.retain(|x| *x != h);
                }
                s.set_selection(sel);
            }
            Ok(json!({ "ok": true }))
        }
        "set_xdata" => {
            let h_str = str_param(p, "handle").ok_or_else(|| bad("dotnet.bridge", "`handle` required"))?;
            let h = Handle::parse_hex(h_str).ok_or_else(|| bad("dotnet.bridge", "invalid handle"))?;
            let arr = p.get("xdata").and_then(Value::as_array).ok_or_else(|| bad("dotnet.bridge", "`xdata` array required"))?;
            let mut incoming: Vec<RawTag> = Vec::new();
            let mut apps_updated: Vec<String> = Vec::new();
            for item in arr {
                let code = item
                    .get("code")
                    .and_then(Value::as_i64)
                    .and_then(|c| i32::try_from(c).ok())
                    .unwrap_or(1000);
                let val = match item.get("value") {
                    Some(Value::String(st)) => st.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                if code == 1001 && !apps_updated.iter().any(|a| a.eq_ignore_ascii_case(&val)) {
                    apps_updated.push(val.clone());
                }
                incoming.push(RawTag { code, value: val });
            }
            let d = s.doc_mut()?;
            let _ = d.modify_entity(h, |e| {
                let mut kept: Vec<RawTag> = Vec::new();
                let mut skip = false;
                for tag in &e.common.xdata {
                    if tag.code == 1001 {
                        skip = apps_updated.iter().any(|a| a.eq_ignore_ascii_case(&tag.value));
                    }
                    if !skip {
                        kept.push(tag.clone());
                    }
                }
                kept.extend(incoming);
                e.common.xdata = kept;
            });
            s.touch();
            Ok(json!({ "ok": true }))
        }
        "add_lwpolyline" => {
            let coords = p.get("coordinates").and_then(Value::as_array).ok_or_else(|| bad("dotnet.bridge", "`coordinates` required"))?;
            let mut vertices = Vec::new();
            let mut i = 0;
            while i + 1 < coords.len() {
                let x = coords[i].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                let y = coords[i + 1].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                vertices.push(PolyVertex::new(Vec2::new(x, y)));
                i += 2;
            }
            let closed = p.get("closed").and_then(Value::as_bool).unwrap_or(false);
            let const_width = p.get("constantWidth").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
            let space = s.space();
            let d = s.doc_mut()?;
            let layer = str_param(p, "layer").map(str::to_string).unwrap_or_else(|| d.header.str("CLAYER", "0").to_string());
            d.ensure_layer(&layer);
            let h = d.new_handle();
            let mut ent = Entity::new(
                h,
                EntityKind::LwPolyline(LwPolyline {
                    vertices,
                    closed,
                    const_width,
                    elevation: 0.0,
                    plinegen: false,
                }),
            );
            ent.common.layer = layer;
            if let Some(st) = d.space_mut(&space) {
                st.push(ent);
            }
            s.set_selection(vec![h]);
            s.touch();
            let d = s.doc()?;
            let e = d.entity(h).ok_or_else(|| bad("dotnet.bridge", "entity not found"))?;
            Ok(dotnet_entity_json(d, e))
        }
        "add_3dpoly" => {
            let coords = p.get("coordinates").and_then(Value::as_array).ok_or_else(|| bad("dotnet.bridge", "`coordinates` required"))?;
            let mut points = Vec::new();
            let mut i = 0;
            while i + 2 < coords.len() {
                let x = coords[i].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                let y = coords[i + 1].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                let z = coords[i + 2].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                points.push(Vec3::new(x, y, z));
                i += 3;
            }
            if points.is_empty() && coords.len() >= 2 {
                let x = coords[0].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                let y = coords[1].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                points.push(Vec3::new(x, y, 0.0));
            }
            let closed = p.get("closed").and_then(Value::as_bool).unwrap_or(false);
            let space = s.space();
            let d = s.doc_mut()?;
            let layer = str_param(p, "layer").map(str::to_string).unwrap_or_else(|| d.header.str("CLAYER", "0").to_string());
            d.ensure_layer(&layer);
            let h = d.new_handle();
            let mut ent = Entity::new(h, EntityKind::Polyline3d(Polyline3d { points, closed }));
            ent.common.layer = layer;
            if let Some(st) = d.space_mut(&space) {
                st.push(ent);
            }
            s.set_selection(vec![h]);
            s.touch();
            let d = s.doc()?;
            let e = d.entity(h).ok_or_else(|| bad("dotnet.bridge", "entity not found"))?;
            Ok(dotnet_entity_json(d, e))
        }
        "add_line" => {
            let a = vec3_from_json(p.get("start"));
            let b = vec3_from_json(p.get("end"));
            let space = s.space();
            let d = s.doc_mut()?;
            let layer = str_param(p, "layer").map(str::to_string).unwrap_or_else(|| d.header.str("CLAYER", "0").to_string());
            d.ensure_layer(&layer);
            let h = d.new_handle();
            let mut ent = Entity::new(h, EntityKind::Line(Line { a, b }));
            ent.common.layer = layer;
            if let Some(st) = d.space_mut(&space) {
                st.push(ent);
            }
            s.set_selection(vec![h]);
            s.touch();
            let d = s.doc()?;
            let e = d.entity(h).ok_or_else(|| bad("dotnet.bridge", "entity not found"))?;
            Ok(dotnet_entity_json(d, e))
        }
        "add_text" => {
            let value = str_param(p, "text").unwrap_or("").to_string();
            let insert = vec3_from_json(p.get("point"));
            let height = p.get("height").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(2.5).max(0.01);
            let space = s.space();
            let d = s.doc_mut()?;
            let layer = str_param(p, "layer").map(str::to_string).unwrap_or_else(|| d.header.str("CLAYER", "0").to_string());
            d.ensure_layer(&layer);
            let h = d.new_handle();
            let mut ent = Entity::new(
                h,
                EntityKind::Text(Text {
                    insert,
                    align_pt: Some(insert),
                    height,
                    value,
                    rotation: 0.0,
                    width_factor: 1.0,
                    oblique: 0.0,
                    style: "Standard".into(),
                    halign: HAlign::Left,
                    valign: VAlign::Baseline,
                }),
            );
            ent.common.layer = layer;
            if let Some(st) = d.space_mut(&space) {
                st.push(ent);
            }
            s.touch();
            let d = s.doc()?;
            let e = d.entity(h).ok_or_else(|| bad("dotnet.bridge", "entity not found"))?;
            Ok(dotnet_entity_json(d, e))
        }
        "insert_block" => {
            let raw_name = str_param(p, "name").unwrap_or("SYMBOL");
            let bname = sanitize_block_name(raw_name);
            let pt = vec3_from_json(p.get("point"));
            let xs = p.get("xscale").and_then(Value::as_f64).filter(|v| v.is_finite() && *v != 0.0).unwrap_or(1.0);
            let ys = p.get("yscale").and_then(Value::as_f64).filter(|v| v.is_finite() && *v != 0.0).unwrap_or(1.0);
            let zs = p.get("zscale").and_then(Value::as_f64).filter(|v| v.is_finite() && *v != 0.0).unwrap_or(1.0);
            let rot = p.get("rotation").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
            let dll_dir = str_param(p, "dllDir").unwrap_or("");
            let space = if p.get("paperSpace").and_then(Value::as_bool).unwrap_or(false) && s.space().is_paper() {
                s.space()
            } else {
                Space::Model
            };
            let d = s.doc_mut()?;
            if d.block(&bname).is_none() {
                let mut imported = false;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let mut candidates = Vec::new();
                    if !dll_dir.is_empty() {
                        let base = std::path::Path::new(dll_dir);
                        let dwg_file = format!("{bname}.dwg");
                        candidates.push(base.join(&dwg_file));
                        candidates.push(base.join("Blocken").join(&dwg_file));
                        candidates.push(base.join("Blocks").join(&dwg_file));
                        if let Some(parent) = base.parent() {
                            candidates.push(parent.join(&dwg_file));
                            candidates.push(parent.join("Blocken").join(&dwg_file));
                            candidates.push(parent.join("Blocks").join(&dwg_file));
                        }
                    }
                    if let Some(hooks) = super::file::io() {
                        for cand in candidates {
                            if let Ok(bytes) = crate::capped_read::read_capped(&cand, MAX_BLOCK_FILE_BYTES)
                                && let Ok(src) = (hooks.read)(&bytes, &cand.to_string_lossy())
                            {
                                let mut blk = Block::new(&bname);
                                for e in src.model.iter() {
                                    blk.entities.push((**e).clone());
                                }
                                if !blk.entities.is_empty() {
                                    d.blocks.insert(bname.clone(), Arc::new(blk));
                                    imported = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                let _ = &dll_dir;
                if !imported {
                    let mut blk = Block::new(&bname);
                    let h1 = d.new_handle();
                    let h2 = d.new_handle();
                    let h3 = d.new_handle();
                    blk.entities.push(Entity::new(
                        h1,
                        EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::ZERO, radius: 0.75 }),
                    ));
                    blk.entities.push(Entity::new(
                        h2,
                        EntityKind::Line(Line { a: Vec3::new(-1.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) }),
                    ));
                    blk.entities.push(Entity::new(
                        h3,
                        EntityKind::Line(Line { a: Vec3::new(0.0, -1.0, 0.0), b: Vec3::new(0.0, 1.0, 0.0) }),
                    ));
                    d.blocks.insert(bname.clone(), Arc::new(blk));
                }
            }
            let layer = d.header.str("CLAYER", "0").to_string();
            let h = d.new_handle();
            let mut ent = Entity::new(
                h,
                EntityKind::Insert(Insert {
                    block: bname,
                    insert: pt,
                    scale: Vec3::new(xs, ys, zs),
                    rotation: rot,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                }),
            );
            ent.common.layer = layer;
            if let Some(st) = d.space_mut(&space) {
                st.push(ent);
            }
            s.set_selection(vec![h]);
            s.touch();
            let d = s.doc()?;
            let e = d.entity(h).ok_or_else(|| bad("dotnet.bridge", "entity not found"))?;
            Ok(dotnet_entity_json(d, e))
        }
        "update_entity" => {
            let h_str = str_param(p, "handle").ok_or_else(|| bad("dotnet.bridge", "`handle` required"))?;
            let h = Handle::parse_hex(h_str).ok_or_else(|| bad("dotnet.bridge", "invalid handle"))?;
            let new_layer = str_param(p, "layer").map(str::to_string);
            let d = s.doc_mut()?;
            if let Some(ref l) = new_layer {
                d.ensure_layer(l);
            }
            let _ = d.modify_entity(h, |e| {
                if let Some(l) = new_layer.clone() {
                    e.common.layer = l;
                }
                if let Some(aci) = p.get("color").and_then(Value::as_i64).and_then(|c| i16::try_from(c).ok()) {
                    e.common.color = Color::from_aci(aci);
                }
                if let Some(lw) = p.get("lineweight").and_then(Value::as_i64).and_then(|w| i16::try_from(w).ok()) {
                    e.common.lineweight = Lineweight::from_dxf(lw);
                }
                if let (Some(base), Some(ang)) = (p.get("rotateBase"), p.get("rotateAngle").and_then(Value::as_f64).filter(|v| v.is_finite())) {
                    let bp = vec3_from_json(Some(base)).xy();
                    let m = Mat3::translate(bp).then_before(Mat3::rotate(ang)).then_before(Mat3::translate(-bp));
                    e.kind.transform(&m);
                }
                match &mut e.kind {
                    EntityKind::LwPolyline(pl) => {
                        if let Some(c) = p.get("closed").and_then(Value::as_bool) {
                            pl.closed = c;
                        }
                        if let Some(w) = p.get("constantWidth").and_then(Value::as_f64).filter(|v| v.is_finite()) {
                            pl.const_width = w;
                        }
                        if let Some(arr) = p.get("coordinates").and_then(Value::as_array) {
                            let mut verts = Vec::new();
                            let mut i = 0;
                            while i + 1 < arr.len() {
                                let x = arr[i].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                                let y = arr[i + 1].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                                verts.push(PolyVertex::new(Vec2::new(x, y)));
                                i += 2;
                            }
                            if !verts.is_empty() {
                                pl.vertices = verts;
                            }
                        }
                    }
                    EntityKind::Polyline3d(pl) => {
                        if let Some(c) = p.get("closed").and_then(Value::as_bool) {
                            pl.closed = c;
                        }
                        if let Some(arr) = p.get("coordinates").and_then(Value::as_array) {
                            let mut pts = Vec::new();
                            let mut i = 0;
                            while i + 2 < arr.len() {
                                let x = arr[i].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                                let y = arr[i + 1].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                                let z = arr[i + 2].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                                pts.push(Vec3::new(x, y, z));
                                i += 3;
                            }
                            if !pts.is_empty() {
                                pl.points = pts;
                            }
                        }
                    }
                    EntityKind::Text(t) => {
                        if let Some(al) = p.get("alignment").and_then(Value::as_i64) {
                            let (ha, va) = match al {
                                0 => (HAlign::Left, VAlign::Baseline),
                                1 => (HAlign::Center, VAlign::Baseline),
                                2 => (HAlign::Right, VAlign::Baseline),
                                4 => (HAlign::Middle, VAlign::Middle),
                                6 => (HAlign::Left, VAlign::Top),
                                7 => (HAlign::Center, VAlign::Top),
                                8 => (HAlign::Right, VAlign::Top),
                                9 => (HAlign::Left, VAlign::Middle),
                                10 => (HAlign::Center, VAlign::Middle),
                                11 => (HAlign::Right, VAlign::Middle),
                                12 => (HAlign::Left, VAlign::Bottom),
                                13 => (HAlign::Center, VAlign::Bottom),
                                14 => (HAlign::Right, VAlign::Bottom),
                                _ => (HAlign::Center, VAlign::Middle),
                            };
                            t.halign = ha;
                            t.valign = va;
                        }
                        if p.get("textAlignmentPoint").is_some() {
                            let pt = vec3_from_json(p.get("textAlignmentPoint"));
                            t.align_pt = Some(pt);
                            t.insert = pt;
                        }
                    }
                    EntityKind::Insert(ins) => {
                        if let Some(n) = str_param(p, "name") {
                            ins.block = sanitize_block_name(n);
                        }
                        if let Some(r) = p.get("rotation").and_then(Value::as_f64).filter(|v| v.is_finite()) {
                            ins.rotation = r;
                        }
                    }
                    _ => {}
                }
            });
            s.touch();
            let d = s.doc()?;
            let Some(e) = d.entity(h) else { return Ok(Value::Null) };
            Ok(dotnet_entity_json(d, e))
        }
        "offset_lwpolyline" => {
            let h_str = str_param(p, "handle").ok_or_else(|| bad("dotnet.bridge", "`handle` required"))?;
            let h = Handle::parse_hex(h_str).ok_or_else(|| bad("dotnet.bridge", "invalid handle"))?;
            let dist = p.get("distance").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
            let space = s.space();
            let (verts, closed, layer) = {
                let d = s.doc()?;
                let Some(e) = d.entity(h) else { return Ok(json!({ "entities": [] })) };
                let EntityKind::LwPolyline(pl) = &e.kind else { return Ok(json!({ "entities": [] })) };
                let gpl = Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
                let Some(off) = super::modify::offset_polyline(&gpl, dist) else {
                    return Ok(json!({ "entities": [] }));
                };
                (off, pl.closed, e.common.layer.clone())
            };
            let d = s.doc_mut()?;
            let new_h = d.new_handle();
            let mut ent = Entity::new(
                new_h,
                EntityKind::LwPolyline(LwPolyline {
                    vertices: verts,
                    closed,
                    const_width: 0.0,
                    elevation: 0.0,
                    plinegen: false,
                }),
            );
            ent.common.layer = layer;
            if let Some(st) = d.space_mut(&space) {
                st.push(ent);
            }
            s.touch();
            let d = s.doc()?;
            let Some(new_e) = d.entity(new_h) else { return Ok(json!({ "entities": [] })) };
            Ok(json!({ "entities": [dotnet_entity_json(d, new_e)] }))
        }
        "delete_entity" => {
            let Some(h) = str_param(p, "handle").and_then(Handle::parse_hex) else { return Ok(json!({ "ok": false })) };
            let Some(space) = s.doc()?.space_of(h) else { return Ok(json!({ "ok": false })) };
            let d = s.doc_mut()?;
            if let Some(st) = d.space_mut(&space) {
                st.remove(h);
            }
            s.touch();
            Ok(json!({ "ok": true }))
        }
        "ensure_layer" => {
            let name = str_param(p, "name").unwrap_or("0").trim().to_string();
            let color_aci = p.get("color").and_then(Value::as_i64);
            let d = s.doc_mut()?;
            d.ensure_layer(&name);
            if let Some(aci) = color_aci
                && let Some(l) = d.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&name))
                && (1..=255).contains(&aci)
                && let Ok(aci_i16) = i16::try_from(aci)
            {
                l.color = Color::from_aci(aci_i16);
            }
            s.touch();
            Ok(json!({ "name": name, "color": color_aci.unwrap_or(7) }))
        }
        "begin_pick" => {
            let mode = str_param(p, "mode").unwrap_or("point").to_string();
            let prompt = str_param(p, "prompt")
                .unwrap_or("Specify point")
                .trim_start_matches(['\r', '\n'])
                .trim_end_matches([':', ' '])
                .to_string();
            let base = p.get("base").and_then(super::point_value);
            if s.running.is_some() {
                s.cancel();
            }
            s.dotnet_pick = DotNetPickState {
                status: "waiting".into(),
                point: None,
                handle: None,
                handles: Vec::new(),
                value: None,
            };
            let st = s.state()?;
            let before = st.doc.clone();
            let selection_before = st.selection.clone();
            s.running = Some(crate::Running {
                id: "dotnet.pick".into(),
                machine: Box::new(DotNetPickM {
                    mode,
                    prompt,
                    base,
                    picked: Vec::new(),
                }),
                before,
                selection_before,
            });
            Ok(json!({ "status": "waiting" }))
        }
        "poll_pick" => Ok(json!(s.dotnet_pick)),
        _ => Err(bad("dotnet.bridge", format!("unknown op `{op}`"))),
    }
}

/// Interactive state machine that collects a point, entity, selection set, distance, or angle on
/// behalf of an external `.NET` plugin (`dotnet.bridge` `begin_pick` / `poll_pick`).
struct DotNetPickM {
    mode: String,
    prompt: String,
    base: Option<Vec2>,
    picked: Vec<Handle>,
}

impl Interactive for DotNetPickM {
    fn name(&self) -> &'static str {
        "NETLOAD"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.mode.as_str() {
            "entity" | "selection" => Prompt::new(&self.prompt, Accept::SELECT),
            "distance" | "angle" => Prompt::new(&self.prompt, Accept::POINT_OR_NUMBER).base_opt(self.base),
            _ => Prompt::new(&self.prompt, Accept::POINT).base_opt(self.base),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.mode.as_str() {
            "entity" => match i {
                Input::Pick(hs) if !hs.is_empty() => {
                    let h = hs[0];
                    s.set_selection(vec![h]);
                    s.dotnet_pick.status = "ok".into();
                    s.dotnet_pick.handle = Some(h.hex());
                    s.dotnet_pick.handles = vec![h.hex()];
                    s.dotnet_pick.point = Some([s.cursor.x, s.cursor.y, 0.0]);
                    Ok(Step::Done)
                }
                Input::Point(p) => {
                    let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 2.0;
                    if let Ok(d) = s.doc()
                        && let Some(h) = crate::select::pick(d, &s.space(), p, ap)
                    {
                        s.set_selection(vec![h]);
                        s.dotnet_pick.status = "ok".into();
                        s.dotnet_pick.handle = Some(h.hex());
                        s.dotnet_pick.handles = vec![h.hex()];
                        s.dotnet_pick.point = Some([p.x, p.y, 0.0]);
                        return Ok(Step::Done);
                    }
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Cancel => {
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            "selection" => match i {
                Input::Pick(hs) => {
                    for h in hs {
                        if !self.picked.contains(&h) {
                            self.picked.push(h);
                        }
                    }
                    s.set_selection(self.picked.clone());
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    if self.picked.is_empty() {
                        s.dotnet_pick.status = "cancel".into();
                    } else {
                        s.dotnet_pick.status = "ok".into();
                        s.dotnet_pick.handles = self.picked.iter().map(|h| h.hex()).collect();
                        s.dotnet_pick.handle = self.picked.first().map(|h| h.hex());
                    }
                    Ok(Step::Done)
                }
                Input::Cancel => {
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            "distance" => match i {
                Input::Point(p) => {
                    let b = self.base.unwrap_or(s.last_point);
                    s.dotnet_pick.status = "ok".into();
                    s.dotnet_pick.value = Some((p - b).len());
                    s.dotnet_pick.point = Some([p.x, p.y, 0.0]);
                    Ok(Step::Done)
                }
                Input::Text(t) => {
                    if let Ok(v) = t.trim().parse::<f64>()
                        && v.is_finite()
                    {
                        s.dotnet_pick.status = "ok".into();
                        s.dotnet_pick.value = Some(v);
                        return Ok(Step::Done);
                    }
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                Input::Enter | Input::Cancel => {
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            "angle" => match i {
                Input::Point(p) => {
                    let b = self.base.unwrap_or(s.last_point);
                    s.dotnet_pick.status = "ok".into();
                    s.dotnet_pick.value = Some((p.y - b.y).atan2(p.x - b.x));
                    s.dotnet_pick.point = Some([p.x, p.y, 0.0]);
                    Ok(Step::Done)
                }
                Input::Text(t) => {
                    if let Ok(v) = t.trim().parse::<f64>()
                        && v.is_finite()
                    {
                        s.dotnet_pick.status = "ok".into();
                        s.dotnet_pick.value = Some(v.to_radians());
                        return Ok(Step::Done);
                    }
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                Input::Enter | Input::Cancel => {
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            _ => match i {
                Input::Point(p) => {
                    s.last_point = p;
                    s.dotnet_pick.status = "ok".into();
                    s.dotnet_pick.point = Some([p.x, p.y, 0.0]);
                    Ok(Step::Done)
                }
                Input::Enter | Input::Cancel => {
                    s.dotnet_pick.status = "cancel".into();
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(b) = self.base
            && matches!(self.mode.as_str(), "point" | "distance" | "angle")
        {
            return vec![EntityKind::Line(Line { a: b.to3(0.0), b: c.to3(0.0) })];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::resolve_alias;

    #[test]
    fn netload_and_vbaide_dispatch_and_aliases_resolve() {
        let mut s = Session::new();
        assert_eq!(resolve_alias("vlide"), "vbaide");
        assert_eq!(resolve_alias("vlisp"), "vbaide");

        let open = s.execute("vbaide", &json!({})).unwrap();
        assert_eq!(open["status"], "open");

        let eval = s.execute("vbaide", &json!({ "code": "(+ 1 2)" })).unwrap();
        assert_eq!(eval["value"], "3");

        let loaded = s.execute("vbaide", &json!({ "name": "test.lsp", "text": "(defun sq (x) (* x x)) (sq 4)" })).unwrap();
        assert_eq!(loaded["value"], "16");

        let nl = s.execute("netload", &json!({ "name": "nl.lsp", "text": "(+ 10 20)" })).unwrap();
        assert_eq!(nl["value"], "30");
    }

    #[test]
    fn dotnet_bridge_creates_updates_and_queries_entities() {
        let mut s = Session::new();
        let line = s
            .execute(
                "dotnet.bridge",
                &json!({
                    "op": "add_line",
                    "start": [0.0, 0.0, 0.0],
                    "end": [10.0, 20.0, 0.0],
                    "layer": "TEST_LAYER"
                }),
            )
            .unwrap();
        let h = line["handle"].as_str().unwrap().to_string();
        assert_eq!(line["objectName"], "AcDbLine");
        assert_eq!(line["layer"], "TEST_LAYER");

        s.execute(
            "dotnet.bridge",
            &json!({
                "op": "set_xdata",
                "handle": h,
                "xdata": [
                    { "code": 1001, "value": "MY_APP" },
                    { "code": 1000, "value": "TAG_VAL" }
                ]
            }),
        )
        .unwrap();

        let sel = s
            .execute("dotnet.bridge", &json!({ "op": "select_all", "app": "MY_APP" }))
            .unwrap();
        assert_eq!(sel["handles"].as_array().unwrap().len(), 1);

        let poly3d = s
            .execute(
                "dotnet.bridge",
                &json!({
                    "op": "add_3dpoly",
                    "coordinates": [0.0, 0.0, 1.5, 10.0, 0.0, 2.5, 10.0, 10.0, 3.5],
                    "closed": true
                }),
            )
            .unwrap();
        assert_eq!(poly3d["objectName"], "AcDb3dPolyline");
        assert_eq!(poly3d["closed"], true);
    }
}
