//! Named views: VIEW / -VIEW (Save, Window, Restore, Delete, ? list, sEttings > Layer snapshot).
//!
//! A named view keeps the centre and size of what was shown, in model space or a layout, and
//! optionally a layer snapshot (a layer state restored with it). Views are stored in the drawing
//! and round-trip through the DXF VIEW table. Restoring one is a view change like ZOOM: it goes
//! into the view history, not the undo stack (a layer snapshot it restores is an undo step).

use cadcraft_doc::{LayerState, NamedView, Space};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::{Value, json};

use super::super::qselect::wildcard;
use super::super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step, View};

/// Most names one Delete or list takes.
const MAX_NAMES: usize = 1000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("view", "Named Views...", run_view)
            .menu(&["View", "Named Views..."])
            .alias(&["v"])
            .params(
                "{option: save|window|restore|delete|list|settings, name? (save/window/restore), names?: \"a,b*\" | [names] (delete/list; wildcards), p1?, p2? (window), layerSnapshot?: bool (save: default the Layer snapshot setting; settings: change it)} (typed VIEW / -VIEW: option prompts)",
            )
            .interactive(|_| Ok(Box::new(ViewM::default()))),
    ]
}

/// A valid view name (the characters symbol table names can't hold are refused).
fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.chars().count() <= 255 && !n.starts_with('*') && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// The layer state holding view `name`'s layer snapshot.
fn snapshot_name(name: &str) -> String {
    format!("*VIEW {name}")
}

fn find<'a>(views: &'a [NamedView], name: &str) -> Option<&'a NamedView> {
    views.iter().find(|v| v.name.eq_ignore_ascii_case(name.trim()))
}

/// Names from `names` (a comma-separated string or an array; `*` when absent), as patterns.
fn name_patterns(p: &Value) -> Vec<String> {
    let raw: Vec<String> = match p.get("names").or_else(|| p.get("name")) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(Value::String(t)) => t.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let pats: Vec<String> = raw.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).take(MAX_NAMES).collect();
    if pats.is_empty() { vec!["*".into()] } else { pats }
}

fn space_label(v: &NamedView) -> String {
    match &v.layout {
        None => "Model".into(),
        Some(l) if l.is_empty() => "Paper space".into(),
        Some(l) => l.clone(),
    }
}

fn run_view(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").unwrap_or("list").trim().to_ascii_lowercase();
    match option.as_str() {
        "save" | "s" | "window" | "w" => {
            let name = str_param(p, "name").map(str::trim).ok_or_else(|| bad("view", "`name` is required"))?;
            let window = match (point_param(p, "p1"), point_param(p, "p2")) {
                (Some(a), Some(b)) => Some(Bounds2::new(a, b)),
                _ if option.starts_with('w') => return Err(bad("view", "`p1` and `p2` (the window) are required")),
                _ => None,
            };
            let snapshot = p.get("layerSnapshot").and_then(Value::as_bool).unwrap_or(s.settings.view_layer_snapshot);
            save(s, name, window, snapshot)
        }
        "restore" | "r" => {
            let name = str_param(p, "name").ok_or_else(|| bad("view", "`name` is required"))?;
            restore(s, name)
        }
        "delete" | "d" => {
            if p.get("names").or_else(|| p.get("name")).is_none() {
                return Err(bad("view", "`names` is required"));
            }
            delete(s, &name_patterns(p))
        }
        "list" | "?" => list(s, &name_patterns(p)),
        "settings" | "e" => {
            if let Some(b) = p.get("layerSnapshot").and_then(Value::as_bool) {
                s.settings.view_layer_snapshot = b;
            }
            Ok(json!({ "layerSnapshot": s.settings.view_layer_snapshot }))
        }
        "orthographic" | "o" => Err(EngineError::Other("Orthographic views are not available yet: CADCraft views are 2D.".into())),
        _ => Err(bad("view", "option must be save, window, restore, delete, list or settings")),
    }
}

/// Save the current view (or `window`) as `name`, replacing a view of that name.
fn save(s: &mut Session, name: &str, window: Option<Bounds2>, snapshot: bool) -> Result<Value> {
    if !valid_name(name) {
        return Err(EngineError::Other(format!("\"{name}\" is not a valid view name.")));
    }
    let (w, h) = s.viewport_px;
    let st = s.state()?;
    // Inside an MSPACE viewport the view is a model space view of that viewport.
    let in_viewport = st.active_viewport().is_some();
    let layout = match (&st.space, in_viewport) {
        (Space::Paper(l), false) => Some(l.clone()),
        _ => None,
    };
    let (center, height, width) = match window {
        Some(b) => {
            if !(b.width() > 1e-12 && b.height() > 1e-12 && b.min.is_finite() && b.max.is_finite()) {
                return Err(EngineError::Other("The window has no area.".into()));
            }
            (b.center(), b.height(), b.width())
        }
        None => {
            let f = if in_viewport { st.zoom_frame() } else { st.view() };
            let aspect = if in_viewport { st.zoom_aspect(w / h.max(1.0)) } else { w / h.max(1.0) };
            (f.center, f.height, f.height * aspect)
        }
    };
    let replaced = find(&st.doc.views, name).map(|v| v.name.clone());
    let d = s.doc_mut()?;
    let layer_state = snapshot.then(|| {
        let state = snapshot_name(name);
        d.layer_states.retain(|l| !l.name.eq_ignore_ascii_case(&state));
        d.layer_states.push(LayerState { name: state.clone(), layers: d.layers.clone() });
        state
    });
    if let Some(old) = &replaced {
        // A replaced view's own snapshot goes with it unless the new one took its place.
        let old_state = snapshot_name(old);
        if layer_state.as_ref().is_none_or(|n| !n.eq_ignore_ascii_case(&old_state)) {
            d.layer_states.retain(|l| !l.name.eq_ignore_ascii_case(&old_state));
        }
        d.views.retain(|v| !v.name.eq_ignore_ascii_case(old));
    }
    d.views.push(NamedView { name: name.to_string(), center, height, width, layer_state, twist: 0.0, layout });
    let msg = if replaced.is_some() { format!("View \"{name}\" replaced.") } else { format!("View \"{name}\" saved.") };
    Ok(json!({ "name": name, "center": [center.x, center.y], "height": height, "width": width, "message": msg }))
}

/// Restore view `name`: switch to its space, show its area, and restore its layer snapshot.
fn restore(s: &mut Session, name: &str) -> Result<Value> {
    let v = find(&s.doc()?.views, name).cloned().ok_or_else(|| EngineError::Other(format!("Could not find view \"{}\".", name.trim())))?;
    if !(v.center.is_finite() && v.height.is_finite() && v.width.is_finite() && v.height > 0.0) {
        return Err(EngineError::Other(format!("View \"{}\" has no area.", v.name)));
    }
    match &v.layout {
        Some(l) => {
            // A paper space view: its layout (an unknown one: the current or first layout).
            let target = match (l.is_empty(), s.layout_space().layout()) {
                (false, _) => l.clone(),
                (true, Some(cur)) => cur.to_string(),
                (true, None) => s.doc()?.layouts.iter().min_by_key(|x| x.tab_order).map(|x| x.name.clone()).unwrap_or_default(),
            };
            if s.doc()?.layout(&target).is_none() {
                return Err(EngineError::Other(format!("View \"{}\" belongs to layout \"{target}\", which does not exist.", v.name)));
            }
            if s.layout_space() != Space::Paper(target.clone()) {
                super::run_layout_set(s, &json!({ "name": target }))?;
            }
            let (w, h) = s.viewport_px;
            let st = s.state_mut()?;
            st.mspace = None;
            let height = v.height.max(v.width / (w / h.max(1.0)).max(1e-9));
            st.set_view(View { center: v.center, height });
        }
        None => {
            // A model space view: into the active MSPACE viewport, else on the Model tab.
            if s.layout_space() != Space::Model && s.state()?.active_viewport().is_none() {
                super::run_layout_set(s, &json!({ "name": "Model" }))?;
            }
            let (w, h) = s.viewport_px;
            let st = s.state_mut()?;
            let aspect = st.zoom_aspect(w / h.max(1.0)).max(1e-9);
            st.set_zoom_frame(View { center: v.center, height: v.height.max(v.width / aspect) });
        }
    }
    let mut msg = format!("View \"{}\" restored.", v.name);
    if let Some(state) = &v.layer_state {
        let d = s.doc_mut()?;
        match d.layer_states.iter().find(|l| l.name.eq_ignore_ascii_case(state)).cloned() {
            Some(saved) => {
                for layer in saved.layers {
                    if let Some(l) = d.layer_mut(&layer.name) {
                        *l = layer;
                    }
                }
            }
            None => msg.push_str(&format!(" Its layer snapshot \"{state}\" is missing.")),
        }
    }
    s.touch();
    Ok(json!({ "name": v.name, "message": msg }))
}

fn delete(s: &mut Session, pats: &[String]) -> Result<Value> {
    let d = s.doc_mut()?;
    let gone: Vec<String> = d.views.iter().filter(|v| pats.iter().any(|p| wildcard(p, &v.name))).map(|v| v.name.clone()).collect();
    if gone.is_empty() {
        return Err(EngineError::Other("No matching views found.".into()));
    }
    d.views.retain(|v| !gone.contains(&v.name));
    let states: Vec<String> = gone.iter().map(|n| snapshot_name(n)).collect();
    d.layer_states.retain(|l| !states.iter().any(|n| n.eq_ignore_ascii_case(&l.name)));
    Ok(json!({ "deleted": gone, "message": format!("{} view(s) deleted.", gone.len()) }))
}

fn list(s: &mut Session, pats: &[String]) -> Result<Value> {
    let d = s.doc()?;
    let views: Vec<&NamedView> = d.views.iter().filter(|v| pats.iter().any(|p| wildcard(p, &v.name))).collect();
    let mut lines = vec!["Saved views:".to_string()];
    if views.is_empty() {
        lines.push("  (none)".into());
    }
    for v in &views {
        lines.push(format!("  {:<31} {}", v.name, space_label(v)));
    }
    let rows: Vec<Value> = views
        .iter()
        .map(|v| {
            json!({
                "name": v.name, "space": space_label(v), "center": [v.center.x, v.center.y], "height": v.height,
                "width": v.width, "layerSnapshot": v.layer_state.is_some(),
            })
        })
        .collect();
    Ok(json!({ "views": rows, "message": lines.join("\n") }))
}

// ---------------- prompts ----------------

#[derive(Clone, Copy, Default)]
enum Stage {
    #[default]
    Main,
    List,
    Delete,
    Restore,
    Save,
    WindowName,
    WindowFirst,
    WindowSecond(Vec2),
    Settings,
    Snapshot,
}

/// -VIEW: "Enter an option [?/Delete/Orthographic/Restore/Save/sEttings/Window]:".
#[derive(Default)]
struct ViewM {
    stage: Stage,
    name: String,
}

/// Run `f` and echo its message (an error re-prompts), ending the command.
fn report(s: &mut Session, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Step> {
    let v = f(s)?;
    if let Some(m) = v.get("message").and_then(Value::as_str) {
        for l in m.lines() {
            s.echo(l.to_string());
        }
    }
    Ok(Step::Done)
}

impl Interactive for ViewM {
    fn name(&self) -> &'static str {
        "VIEW"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            Stage::Main => Prompt::new("Enter an option", Accept::TEXT).kw(&["?", "Delete", "Orthographic", "Restore", "Save", "sEttings", "Window"]),
            Stage::List => Prompt::new("Enter view name(s) to list", Accept::TEXT).default("*"),
            Stage::Delete => Prompt::new("Enter view name(s) to delete", Accept::TEXT),
            Stage::Restore => Prompt::new("Enter view name to restore", Accept::TEXT),
            Stage::Save | Stage::WindowName => Prompt::new("Enter view name to save", Accept::TEXT),
            Stage::WindowFirst => Prompt::new("Specify first corner", Accept::POINT),
            Stage::WindowSecond(a) => Prompt::new("Specify opposite corner", Accept::POINT).corner(a),
            Stage::Settings => Prompt::new("Enter an option", Accept::TEXT).kw(&["Layer snapshot"]),
            Stage::Snapshot => {
                let on = if s.settings.view_layer_snapshot { "Yes" } else { "No" };
                Prompt::new("Save layer settings with views?", Accept::TEXT).kw(&["Yes", "No"]).default(on)
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()),
            _ => None,
        };
        match self.stage {
            Stage::Main => {
                let Some(t) = text else {
                    return if i == Input::Enter { Ok(Step::Done) } else { Err(EngineError::Other("Invalid option keyword.".into())) };
                };
                self.stage = match t.to_ascii_lowercase().as_str() {
                    "?" => Stage::List,
                    "delete" | "d" => Stage::Delete,
                    "restore" | "r" => Stage::Restore,
                    "save" | "s" => Stage::Save,
                    "window" | "w" => Stage::WindowName,
                    "settings" | "e" => Stage::Settings,
                    "orthographic" | "o" => {
                        s.echo("Orthographic views are not available yet: CADCraft views are 2D.");
                        return Ok(Step::Done);
                    }
                    _ => return Err(EngineError::Other("Invalid option keyword.".into())),
                };
                Ok(Step::Continue)
            }
            Stage::List => {
                let names = text.filter(|t| !t.is_empty()).unwrap_or_else(|| "*".into());
                report(s, |s| list(s, &name_patterns(&json!({ "names": names }))))
            }
            Stage::Delete => match text.filter(|t| !t.is_empty()) {
                Some(names) => report(s, |s| delete(s, &name_patterns(&json!({ "names": names })))),
                None => Ok(Step::Done),
            },
            Stage::Restore => match text.filter(|t| !t.is_empty()) {
                Some(name) => report(s, |s| restore(s, &name)),
                None => Ok(Step::Done),
            },
            Stage::Save | Stage::WindowName => {
                let Some(name) = text.filter(|t| !t.is_empty()) else { return Ok(Step::Done) };
                if !valid_name(&name) {
                    return Err(EngineError::Other(format!("\"{name}\" is not a valid view name.")));
                }
                if matches!(self.stage, Stage::WindowName) {
                    self.name = name;
                    self.stage = Stage::WindowFirst;
                    return Ok(Step::Continue);
                }
                let snapshot = s.settings.view_layer_snapshot;
                report(s, |s| save(s, &name, None, snapshot))
            }
            Stage::WindowFirst => match i {
                Input::Point(a) => {
                    self.stage = Stage::WindowSecond(a);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err(EngineError::Other("Requires a point.".into())),
            },
            Stage::WindowSecond(a) => match i {
                Input::Point(b) => {
                    let snapshot = s.settings.view_layer_snapshot;
                    let name = std::mem::take(&mut self.name);
                    report(s, |s| save(s, &name, Some(Bounds2::new(a, b)), snapshot))
                }
                Input::Enter => Ok(Step::Done),
                _ => Err(EngineError::Other("Requires a point.".into())),
            },
            Stage::Settings => match text.map(|t| t.to_ascii_lowercase()) {
                Some(t) if t == "l" || "layer snapshot".starts_with(t.as_str()) && !t.is_empty() => {
                    self.stage = Stage::Snapshot;
                    Ok(Step::Continue)
                }
                None if i == Input::Enter => Ok(Step::Done),
                _ => Err(EngineError::Other("Invalid option keyword.".into())),
            },
            Stage::Snapshot => {
                let on = match text.map(|t| t.to_ascii_lowercase()) {
                    None if i == Input::Enter => s.settings.view_layer_snapshot,
                    Some(t) if t == "y" || t == "yes" => true,
                    Some(t) if t == "n" || t == "no" => false,
                    _ => return Err(EngineError::Other("Requires Yes or No.".into())),
                };
                s.settings.view_layer_snapshot = on;
                Ok(Step::Done)
            }
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<cadcraft_doc::EntityKind> {
        match self.stage {
            Stage::WindowSecond(a) => vec![super::super::helpers::lwpoly(super::super::helpers::rect_vertices(a, c), true)],
            _ => Vec::new(),
        }
    }
}
