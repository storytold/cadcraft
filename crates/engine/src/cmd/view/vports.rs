//! Tiled model space viewports: -VPORTS (SIngle, 2/3/4 with an arrangement, Join, Save, Restore,
//! Delete, ?), VPORTS and View > Viewports in the Model tab, and the current tile.
//!
//! The tiles shown are the drawing's `*Active` viewport configuration (saved in the DXF VPORT
//! table), the first tile being the current one; named configurations sit next to it. Arranging
//! tiles is an undo step. The session keeps each tile's view ([`crate::ModelTiles`]); the current
//! tile shows the model space view itself, so ZOOM, PAN and every pick work in it unchanged. The
//! UI draws the tiles from [`Session::model_tiles`] and hands clicks in other tiles to
//! [`Session::click_tile`]. In a layout -VPORTS is MVIEW. View > Viewports > Named Viewports...
//! restores or deletes a saved configuration.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, Space, VportConfig, VportTile};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::super::qselect::wildcard;
use super::super::{CommandSpec, bad, find_command, str_param};

use crate::{Accept, DocState, EngineError, Input, Interactive, ModelTile, ModelTiles, Prompt, Result, Session, Step, View};

/// Most tiles on screen at once.
const MAX_TILES: usize = 64;
/// The smallest tile side, as a fraction of the drawing area.
const MIN_SIDE: f64 = 1.0 / 64.0;
const EPS: f64 = 1e-6;

/// The -VPORTS Join prompts; a click in a tile answers them ([`Session::click_tile`]).
const DOMINANT_PROMPT: &str = "Select dominant viewport";
const JOIN_PROMPT: &str = "Select viewport to join";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("-vports", "Viewports", run_vports)
            .params(
                "{option: single|2|3|4|join|save|restore|delete|list, arrangement?: horizontal|vertical (2) | horizontal|vertical|above|below|left|right (3; default right), applyTo?: current|display (default current: split the current tile), name? (save/restore), names? (delete/list; wildcards), dominant?, join? (tile numbers from 1; dominant defaults to the current tile)} (model space; in a layout: MVIEW)",
            )
            .interactive(vports_factory),
        CommandSpec::new("vports.activate", "Set Current Viewport", run_activate).params("{index: tile number from 1}").noundo(),
        CommandSpec::new("vports.named", "Named Viewports...", run_named)
            .menu(&["View", "Viewports", "Named Viewports..."])
            .params(
                "{name?: a saved viewport configuration, delete?: bool, p1?, p2?, layout?} (no name: → {current, configurations: [{name, viewports}]}; the Model tab shows the configuration's tiles, a layout gets one viewport per tile over p1-p2 or the printable area; typed or from the menu: the Named Viewports dialog)",
            )
            .interactive(named_factory),
    ]
}

/// -VPORTS: the tile prompts in the Model tab, MVIEW's in a layout.
fn vports_factory(s: &Session) -> Result<Box<dyn Interactive>> {
    if let Some(m) = model_vports(s, None) {
        return Ok(m);
    }
    let mview = find_command("mview").and_then(|c| c.interactive).ok_or_else(|| EngineError::UnknownCommand("mview".into()))?;
    mview(s)
}

/// The tile machine for the Model tab (`count`: View > Viewports > n), else `None` (the caller
/// runs its layout form).
pub(in crate::cmd) fn model_vports(s: &Session, count: Option<usize>) -> Option<Box<dyn Interactive>> {
    if s.layout_space() != Space::Model {
        return None;
    }
    Some(Box::new(VportsM { count, ..Default::default() }))
}

/// The JSON form of VPORTS / View > Viewports > n in the Model tab: `count` tiles (1: single).
pub(in crate::cmd) fn tile_model(s: &mut Session, p: &Value, count: usize) -> Result<Value> {
    let option = if count <= 1 { "single".to_string() } else { count.to_string() };
    run_vports(s, &with_option(p, &option))
}

fn with_option(p: &Value, option: &str) -> Value {
    let mut p = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = p.as_object_mut() {
        o.insert("option".into(), json!(option));
    }
    p
}

/// The tiles of viewport configuration `name` (any case), for MVIEW Restore: `*Active` is the
/// model space as shown now (one full tile with the model view when it isn't tiled), any other
/// name a saved configuration. `None` when there is no such configuration.
pub(in crate::cmd) fn config_tiles(s: &Session, name: &str) -> Option<Vec<VportTile>> {
    let name = name.trim();
    if name.eq_ignore_ascii_case(VportConfig::ACTIVE) {
        return Some(live(s.state().ok()?, s.viewport_px).0);
    }
    s.doc().ok()?.vports.iter().find(|c| !c.is_active() && c.name.eq_ignore_ascii_case(name)).map(|c| c.tiles.clone())
}

/// The saved viewport configurations' names.
pub(in crate::cmd) fn config_names(s: &Session) -> Vec<String> {
    s.doc().map(|d| d.vports.iter().filter(|c| !c.is_active()).map(|c| c.name.clone()).collect()).unwrap_or_default()
}

// ---------------- tile state ----------------

type Rect = (Vec2, Vec2);

fn rect(t: &VportTile) -> Rect {
    (t.min, t.max)
}

fn view_of(t: &VportTile) -> View {
    View { center: t.center, height: t.height }
}

fn active_config(d: &Drawing) -> Option<&VportConfig> {
    d.vports.iter().find(|c| c.is_active() && c.tiles.len() > 1)
}

/// The tiles on show with their views as they are now (the current one's is the model view), and
/// the current tile. One full tile when there is no tiling.
fn live(st: &DocState, px: (f64, f64)) -> (Vec<VportTile>, usize) {
    let now = st.views.iter().find(|(sp, _)| *sp == Space::Model).map(|(_, v)| *v).unwrap_or_default();
    let Some(cfg) = active_config(&st.doc) else {
        let aspect = px.0 / px.1.max(1.0);
        return (vec![VportTile { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0), center: now.center, height: now.height, aspect }], 0);
    };
    let mut tiles = cfg.tiles.clone();
    let synced =
        st.tiles.rects.len() == tiles.len() && st.tiles.views.len() == tiles.len() && st.tiles.rects.iter().zip(&tiles).all(|(r, t)| *r == rect(t));
    let active = if synced { st.tiles.active.min(tiles.len() - 1) } else { 0 };
    if synced {
        for (t, v) in tiles.iter_mut().zip(&st.tiles.views) {
            (t.center, t.height) = (v.center, v.height);
        }
    }
    if let Some(t) = tiles.get_mut(active) {
        (t.center, t.height) = (now.center, now.height);
    }
    // Screen aspects from the current tile's size in pixels.
    if let Some(cur) = tiles.get(active).copied() {
        let (fw, fh) = ((cur.max.x - cur.min.x).max(EPS), (cur.max.y - cur.min.y).max(EPS));
        let (w, h) = (px.0 / fw, px.1.max(1.0) / fh);
        for t in tiles.iter_mut() {
            t.aspect = ((t.max.x - t.min.x) * w / ((t.max.y - t.min.y) * h).max(EPS)).max(EPS);
        }
    }
    (tiles, active)
}

/// Show `tiles` with tile `current` current: the drawing's `*Active` configuration (one undo
/// step with the command) and the session's views.
fn apply(s: &mut Session, mut tiles: Vec<VportTile>, current: usize) -> Result<()> {
    if tiles.is_empty() {
        return Err(EngineError::Other("A viewport configuration needs a viewport.".into()));
    }
    let cur = tiles.remove(current.min(tiles.len() - 1));
    tiles.insert(0, cur);
    let st = s.state_mut()?;
    let d = Arc::make_mut(&mut st.doc);
    d.vports.retain(|c| !c.is_active());
    if tiles.len() > 1 {
        d.vports.insert(0, VportConfig { name: VportConfig::ACTIVE.into(), tiles: tiles.clone() });
        st.tiles = ModelTiles { rects: tiles.iter().map(rect).collect(), views: tiles.iter().map(view_of).collect(), active: 0 };
    } else {
        st.tiles = ModelTiles::default();
    }
    if st.space == Space::Model && st.mspace.is_none() {
        st.set_view_quiet(view_of(&cur));
    }
    s.touch();
    Ok(())
}

fn summary(s: &Session) -> Result<Value> {
    let (tiles, active) = live(s.state()?, s.viewport_px);
    let rows: Vec<Value> = tiles
        .iter()
        .enumerate()
        .map(|(i, t)| json!({ "index": i + 1, "min": [t.min.x, t.min.y], "max": [t.max.x, t.max.y], "center": [t.center.x, t.center.y], "height": t.height, "current": i == active }))
        .collect();
    Ok(json!({ "tiles": rows }))
}

/// Split rectangle `r` into `n` tiles (2 to 4) by `arrangement`; the first is the main one.
fn split(r: Rect, n: usize, arrangement: &str) -> Option<Vec<Rect>> {
    let ((x0, y0), (x1, y1)) = ((r.0.x, r.0.y), (r.1.x, r.1.y));
    let (xm, ym) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let b = |a: f64, b: f64, c: f64, d: f64| (Vec2::new(a, b), Vec2::new(c, d));
    let third = |a: f64, z: f64, k: f64| a + (z - a) * k / 3.0;
    let a = arrangement.trim().to_ascii_lowercase();
    let a = a.as_str();
    let out = match (n, a) {
        (2, "h" | "horizontal") => vec![b(x0, ym, x1, y1), b(x0, y0, x1, ym)],
        (2, "" | "v" | "vertical") => vec![b(x0, y0, xm, y1), b(xm, y0, x1, y1)],
        (3, "h" | "horizontal") => {
            let (ya, yb) = (third(y0, y1, 2.0), third(y0, y1, 1.0));
            vec![b(x0, ya, x1, y1), b(x0, yb, x1, ya), b(x0, y0, x1, yb)]
        }
        (3, "v" | "vertical") => {
            let (xa, xb) = (third(x0, x1, 1.0), third(x0, x1, 2.0));
            vec![b(x0, y0, xa, y1), b(xa, y0, xb, y1), b(xb, y0, x1, y1)]
        }
        (3, "a" | "above") => vec![b(x0, ym, x1, y1), b(x0, y0, xm, ym), b(xm, y0, x1, ym)],
        (3, "b" | "below") => vec![b(x0, y0, x1, ym), b(x0, ym, xm, y1), b(xm, ym, x1, y1)],
        (3, "l" | "left") => vec![b(x0, y0, xm, y1), b(xm, ym, x1, y1), b(xm, y0, x1, ym)],
        (3, "" | "r" | "right") => vec![b(xm, y0, x1, y1), b(x0, ym, xm, y1), b(x0, y0, xm, ym)],
        (4, _) => vec![b(x0, ym, xm, y1), b(xm, ym, x1, y1), b(x0, y0, xm, ym), b(xm, y0, x1, ym)],
        _ => return None,
    };
    Some(out)
}

/// The rectangle two tiles make together, if they share a whole side.
fn joined(a: Rect, b: Rect) -> Option<Rect> {
    let near = |p: f64, q: f64| (p - q).abs() < EPS;
    let rows = near(a.0.y, b.0.y) && near(a.1.y, b.1.y) && (near(a.1.x, b.0.x) || near(b.1.x, a.0.x));
    let cols = near(a.0.x, b.0.x) && near(a.1.x, b.1.x) && (near(a.1.y, b.0.y) || near(b.1.y, a.0.y));
    (rows || cols).then(|| (Vec2::new(a.0.x.min(b.0.x), a.0.y.min(b.0.y)), Vec2::new(a.1.x.max(b.1.x), a.1.y.max(b.1.y))))
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.chars().count() <= 255 && !n.starts_with('*') && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

fn model_only(s: &Session) -> Result<()> {
    if s.layout_space() == Space::Model {
        Ok(())
    } else {
        Err(EngineError::Other("Tiled viewports are for the Model tab; use MVIEW in a layout.".into()))
    }
}

/// A tile number (from 1) in `p[key]`.
fn tile_param(p: &Value, key: &str, n: usize) -> Result<Option<usize>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let i = v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())).and_then(|i| usize::try_from(i).ok());
            match i {
                Some(i) if (1..=n).contains(&i) => Ok(Some(i - 1)),
                _ => Err(bad("-vports", format!("`{key}` must be a viewport number from 1 to {n}"))),
            }
        }
    }
}

fn names(p: &Value) -> Vec<String> {
    let raw: Vec<String> = match p.get("names").or_else(|| p.get("name")) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(Value::String(t)) => t.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    };
    raw.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).take(1000).collect()
}

fn run_vports(s: &mut Session, p: &Value) -> Result<Value> {
    let option = match p.get("option") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(t)) => t.trim().to_ascii_lowercase(),
        _ => "list".into(),
    };
    let count = match option.as_str() {
        "single" | "si" | "1" => Some(1),
        "2" | "3" | "4" => option.parse::<u64>().ok(),
        _ => None,
    };
    if let Some(count) = count
        && (s.layout_space() != Space::Model || p.get("layout").is_some())
    {
        // In a layout, -VPORTS makes layout viewports: MVIEW.
        let mview = find_command("mview").ok_or_else(|| EngineError::UnknownCommand("mview".into()))?;
        let mut p = p.clone();
        if let Some(o) = p.as_object_mut() {
            o.remove("option");
            o.insert("count".into(), json!(count));
        }
        return (mview.run)(s, &p);
    }
    let (tiles, current) = live(s.state()?, s.viewport_px);
    let n = tiles.len();
    match option.as_str() {
        "single" | "si" | "1" => {
            model_only(s)?;
            let cur = tiles.get(current).copied().ok_or(EngineError::NoDocument)?;
            apply(s, vec![VportTile { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0), ..cur }], 0)?;
        }
        "2" | "3" | "4" => {
            model_only(s)?;
            let count = option.parse::<usize>().unwrap_or(2);
            let display = str_param(p, "applyTo").is_some_and(|a| a.eq_ignore_ascii_case("display"));
            let cur = tiles.get(current).copied().ok_or(EngineError::NoDocument)?;
            let area = if display { (Vec2::ZERO, Vec2::new(1.0, 1.0)) } else { rect(&cur) };
            let arrangement = str_param(p, "arrangement").unwrap_or("");
            let parts = split(area, count, arrangement)
                .ok_or_else(|| bad("-vports", format!("unknown arrangement `{arrangement}` for {count} viewports")))?;
            if parts.iter().any(|r| r.1.x - r.0.x < MIN_SIDE - EPS || r.1.y - r.0.y < MIN_SIDE - EPS) {
                return Err(EngineError::Other("The current viewport is too small to divide.".into()));
            }
            let mut out: Vec<VportTile> =
                if display { Vec::new() } else { tiles.iter().enumerate().filter(|(i, _)| *i != current).map(|(_, t)| *t).collect() };
            if out.len() + parts.len() > MAX_TILES {
                return Err(EngineError::Other(format!("At most {MAX_TILES} viewports can be shown.")));
            }
            let first = out.len();
            out.extend(parts.into_iter().map(|(min, max)| VportTile { min, max, ..cur }));
            apply(s, out, first)?;
        }
        "join" | "j" => {
            model_only(s)?;
            let dominant = tile_param(p, "dominant", n)?.unwrap_or(current);
            let other = tile_param(p, "join", n)?.ok_or_else(|| bad("-vports", "`join` (the viewport to join) is required"))?;
            let (Some(a), Some(b)) = (tiles.get(dominant).copied(), tiles.get(other).copied()) else {
                return Err(EngineError::Other("There is only one viewport.".into()));
            };
            if dominant == other {
                return Err(EngineError::Other("Select a different viewport to join.".into()));
            }
            let (min, max) =
                joined(rect(&a), rect(&b)).ok_or_else(|| EngineError::Other("The selected viewports do not form a rectangle.".into()))?;
            let mut out: Vec<VportTile> = tiles.iter().enumerate().filter(|(i, _)| *i != dominant && *i != other).map(|(_, t)| *t).collect();
            out.push(VportTile { min, max, ..a });
            let current = out.len() - 1;
            apply(s, out, current)?;
        }
        "save" | "s" => {
            model_only(s)?;
            let name = str_param(p, "name").map(str::trim).unwrap_or_default().to_string();
            if !valid_name(&name) {
                return Err(EngineError::Other(format!("\"{name}\" is not a valid viewport configuration name.")));
            }
            // The current tile first, as in the shown configuration.
            let mut saved = tiles.clone();
            let cur = saved.remove(current);
            saved.insert(0, cur);
            let d = s.doc_mut()?;
            d.vports.retain(|c| !c.name.eq_ignore_ascii_case(&name));
            d.vports.push(VportConfig { name: name.clone(), tiles: saved });
            return Ok(json!({ "name": name, "message": format!("Viewport configuration \"{name}\" saved.") }));
        }
        "restore" | "r" => {
            model_only(s)?;
            let name = str_param(p, "name").map(str::trim).unwrap_or_default();
            let cfg = s
                .doc()?
                .vports
                .iter()
                .find(|c| !c.is_active() && c.name.eq_ignore_ascii_case(name))
                .cloned()
                .ok_or_else(|| EngineError::Other(format!("Could not find viewport configuration \"{name}\".")))?;
            apply(s, cfg.tiles, 0)?;
        }
        "delete" | "d" => {
            let pats = names(p);
            if pats.is_empty() {
                return Err(bad("-vports", "`names` is required"));
            }
            let d = s.doc_mut()?;
            let gone: Vec<String> =
                d.vports.iter().filter(|c| !c.is_active() && pats.iter().any(|q| wildcard(q, &c.name))).map(|c| c.name.clone()).collect();
            if gone.is_empty() {
                return Err(EngineError::Other("No matching viewport configurations found.".into()));
            }
            d.vports.retain(|c| !gone.contains(&c.name));
            return Ok(json!({ "deleted": gone, "message": format!("{} viewport configuration(s) deleted.", gone.len()) }));
        }
        "list" | "?" => {
            let pats = names(p);
            let pats = if pats.is_empty() { vec!["*".to_string()] } else { pats };
            let mut lines = vec!["Current configuration:".to_string()];
            let corners = |t: &VportTile| format!("{:.4},{:.4}  {:.4},{:.4}", t.min.x, t.min.y, t.max.x, t.max.y);
            for (i, t) in tiles.iter().enumerate() {
                lines.push(format!("  {}{}  {}", i + 1, if i == current { "*" } else { " " }, corners(t)));
            }
            let saved: Vec<&VportConfig> = s.doc()?.vports.iter().filter(|c| !c.is_active() && pats.iter().any(|q| wildcard(q, &c.name))).collect();
            lines.push("Saved configurations:".into());
            if saved.is_empty() {
                lines.push("  (none)".into());
            }
            for c in &saved {
                lines.push(format!("  {}: {} viewport(s)", c.name, c.tiles.len()));
            }
            let configs: Vec<Value> = saved.iter().map(|c| json!({ "name": c.name, "viewports": c.tiles.len() })).collect();
            let mut r = summary(s)?;
            if let Some(o) = r.as_object_mut() {
                o.insert("configurations".into(), json!(configs));
                o.insert("message".into(), json!(lines.join("\n")));
            }
            return Ok(r);
        }
        _ => return Err(bad("-vports", "option must be single, 2, 3, 4, join, save, restore, delete or list")),
    }
    summary(s)
}

fn run_activate(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state()?;
    let n = active_config(&st.doc).map_or(1, |c| c.tiles.len());
    let i = tile_param(p, "index", n)?.ok_or_else(|| bad("vports.activate", "`index` is required"))?;
    s.activate_tile(i)?;
    summary(s)
}

/// View > Viewports > Named Viewports...: without a name, the saved configurations and the one
/// the Model tab shows; with one, restore it (its tiles in the Model tab, MVIEW Restore in a
/// layout) or, with `delete`, delete it.
fn run_named(s: &mut Session, p: &Value) -> Result<Value> {
    let delete = match p.get("delete") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("vports.named", "`delete` must be true or false")),
    };
    let Some(name) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) else {
        let configs: Vec<Value> =
            s.doc()?.vports.iter().filter(|c| !c.is_active()).map(|c| json!({ "name": c.name, "viewports": c.tiles.len() })).collect();
        return Ok(json!({ "current": s.viewport_configuration(), "configurations": configs }));
    };
    let saved = s
        .doc()?
        .vports
        .iter()
        .find(|c| !c.is_active() && c.name.eq_ignore_ascii_case(name))
        .map(|c| c.name.clone())
        .ok_or_else(|| EngineError::Other(format!("Could not find viewport configuration \"{}\".", name.chars().take(64).collect::<String>())))?;
    if delete {
        s.doc_mut()?.vports.retain(|c| c.is_active() || c.name != saved);
        return Ok(json!({ "deleted": saved, "message": format!("Viewport configuration \"{saved}\" deleted.") }));
    }
    if s.layout_space() != Space::Model || p.get("layout").is_some() {
        let mview = find_command("mview").ok_or_else(|| EngineError::UnknownCommand("mview".into()))?;
        let mut p = p.clone();
        if let Some(o) = p.as_object_mut() {
            o.remove("delete");
            o.insert("option".into(), json!("restore"));
            o.insert("name".into(), json!(saved));
        }
        return (mview.run)(s, &p);
    }
    run_vports(s, &json!({ "option": "restore", "name": saved }))
}

/// Whether two configurations have their tiles in the same places (in any order).
fn same_places(a: &[VportTile], b: &[VportTile]) -> bool {
    let near = |p: Vec2, q: Vec2| (p.x - q.x).abs() < EPS && (p.y - q.y).abs() < EPS;
    a.len() == b.len() && a.iter().all(|t| b.iter().any(|u| near(t.min, u.min) && near(t.max, u.max)))
}

impl Session {
    /// The saved viewport configuration whose tiles are where the Model tab's tiles are now (the
    /// first such one in the drawing), if any.
    pub fn viewport_configuration(&self) -> Option<String> {
        let st = self.state().ok()?;
        let (tiles, _) = live(st, self.viewport_px);
        st.doc.vports.iter().filter(|c| !c.is_active()).find(|c| same_places(&c.tiles, &tiles)).map(|c| c.name.clone())
    }

    /// The model space tiles to draw (empty: one view, or not in the Model tab).
    pub fn model_tiles(&self) -> Vec<ModelTile> {
        let Ok(st) = self.state() else { return Vec::new() };
        if st.space != Space::Model || active_config(&st.doc).is_none() {
            return Vec::new();
        }
        let (tiles, active) = live(st, self.viewport_px);
        tiles.iter().enumerate().map(|(i, t)| ModelTile { min: t.min, max: t.max, view: view_of(t), active: i == active }).collect()
    }

    /// Make tile `index` (from 0, in [`Session::model_tiles`] order) the current one: the views
    /// trade places; nothing in the drawing changes.
    pub fn activate_tile(&mut self, index: usize) -> Result<()> {
        let (tiles, active) = live(self.state()?, self.viewport_px);
        if tiles.len() < 2 || index >= tiles.len() {
            return Err(EngineError::Other("No such viewport.".into()));
        }
        if index == active {
            return Ok(());
        }
        let st = self.state_mut()?;
        st.tiles = ModelTiles { rects: tiles.iter().map(rect).collect(), views: tiles.iter().map(view_of).collect(), active: index };
        if let Some(t) = tiles.get(index) {
            st.set_view_quiet(view_of(t));
        }
        self.touch();
        Ok(())
    }

    /// A click in tile `index` outside the current one: it answers a -VPORTS Join prompt, else
    /// makes the tile current (also in the middle of a command, as in AutoCAD).
    pub fn click_tile(&mut self, index: usize) -> Result<()> {
        if self.current_prompt().is_some_and(|p| p.message == DOMINANT_PROMPT || p.message == JOIN_PROMPT) {
            return self.input(Input::Text((index + 1).to_string()));
        }
        self.activate_tile(index)
    }
}

// ---------------- prompts ----------------

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Main,
    Arrange(usize),
    Save,
    Restore,
    Delete,
    List,
    Dominant,
    Join(Option<usize>),
}

/// -VPORTS in the Model tab: "Enter an option [Save/Restore/Delete/Join/SIngle/?/2/3/4] <3>:".
#[derive(Default)]
struct VportsM {
    stage: Stage,
    /// View > Viewports > n: start at that count.
    count: Option<usize>,
}

impl VportsM {
    fn run(s: &mut Session, p: Value) -> Result<Step> {
        let r = run_vports(s, &p)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            for l in m.lines() {
                s.echo(l.to_string());
            }
        }
        Ok(Step::Done)
    }
    fn count(&mut self, s: &mut Session, n: usize) -> Result<Step> {
        if n == 4 {
            return Self::run(s, json!({ "option": "4" }));
        }
        if n <= 1 {
            return Self::run(s, json!({ "option": "single" }));
        }
        self.stage = Stage::Arrange(n);
        Ok(Step::Continue)
    }
}

impl Interactive for VportsM {
    fn name(&self) -> &'static str {
        "VPORTS"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match self.count {
            Some(n) => self.count(s, n),
            None => Ok(Step::Continue),
        }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Main => {
                Prompt::new("Enter an option", Accept::TEXT).kw(&["Save", "Restore", "Delete", "Join", "SIngle", "?", "2", "3", "4"]).default("3")
            }
            Stage::Arrange(2) => Prompt::new("Enter a configuration option", Accept::TEXT).kw(&["Horizontal", "Vertical"]).default("Vertical"),
            Stage::Arrange(_) => Prompt::new("Enter a configuration option", Accept::TEXT)
                .kw(&["Horizontal", "Vertical", "Above", "Below", "Left", "Right"])
                .default("Right"),
            Stage::Save => Prompt::new("Enter name for new viewport configuration", Accept::TEXT).kw(&["?"]),
            Stage::Restore => Prompt::new("Enter name of viewport configuration to restore", Accept::TEXT).kw(&["?"]),
            Stage::Delete => Prompt::new("Enter name(s) of viewport configurations to delete", Accept::TEXT).default("none"),
            Stage::List => Prompt::new("Enter name(s) of viewport configurations to list", Accept::TEXT).default("*"),
            Stage::Dominant => Prompt::new(DOMINANT_PROMPT, Accept::POINT_OR_NUMBER).default("current viewport"),
            Stage::Join(_) => Prompt::new(JOIN_PROMPT, Accept::POINT_OR_NUMBER),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_ascii_lowercase()),
            _ => None,
        };
        let bad_kw = || EngineError::Other("Invalid option keyword.".into());
        match self.stage {
            Stage::Main => {
                let t = match (&i, text) {
                    (Input::Enter, _) => "3".to_string(),
                    (_, Some(t)) => t,
                    _ => return Err(bad_kw()),
                };
                match t.as_str() {
                    "2" | "3" | "4" => self.count(s, t.parse().unwrap_or(3)),
                    "si" | "single" => Self::run(s, json!({ "option": "single" })),
                    "s" | "save" => Ok(self.go(Stage::Save)),
                    "r" | "restore" => Ok(self.go(Stage::Restore)),
                    "d" | "delete" => Ok(self.go(Stage::Delete)),
                    "?" => Ok(self.go(Stage::List)),
                    "j" | "join" => {
                        model_only(s)?;
                        Ok(self.go(Stage::Dominant))
                    }
                    _ => Err(bad_kw()),
                }
            }
            Stage::Arrange(n) => {
                let arrangement = match (&i, text) {
                    (Input::Enter, _) => if n == 2 { "vertical" } else { "right" }.to_string(),
                    (_, Some(t)) => t,
                    _ => return Err(bad_kw()),
                };
                Self::run(s, json!({ "option": n.to_string(), "arrangement": arrangement }))
            }
            Stage::Save | Stage::Restore => {
                let Some(name) = text.filter(|t| !t.is_empty()) else { return Ok(Step::Done) };
                if name == "?" {
                    let r = run_vports(s, &json!({ "option": "list" }))?;
                    if let Some(m) = r.get("message").and_then(Value::as_str) {
                        m.lines().for_each(|l| s.echo(l.to_string()));
                    }
                    return Ok(Step::Continue);
                }
                // The name as typed (keywords and text arrive in lower case above).
                let typed = match &i {
                    Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
                    _ => name,
                };
                let option = if self.stage == Stage::Save { "save" } else { "restore" };
                Self::run(s, json!({ "option": option, "name": typed }))
            }
            Stage::Delete => match &i {
                Input::Text(t) if !t.trim().is_empty() => Self::run(s, json!({ "option": "delete", "names": t.trim() })),
                _ => Ok(Step::Done),
            },
            Stage::List => {
                let names = match &i {
                    Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
                    _ => "*".into(),
                };
                Self::run(s, json!({ "option": "list", "names": names }))
            }
            Stage::Dominant => {
                let d = self.tile(s, &i)?;
                self.stage = Stage::Join(d);
                Ok(Step::Continue)
            }
            Stage::Join(d) => {
                let other = self.tile(s, &i)?.ok_or_else(|| EngineError::Other("Select a viewport.".into()))?;
                let mut p = json!({ "option": "join", "join": other + 1 });
                if let (Some(d), Some(o)) = (d, p.as_object_mut()) {
                    o.insert("dominant".into(), json!(d + 1));
                }
                Self::run(s, p)
            }
        }
    }
}

impl VportsM {
    fn go(&mut self, stage: Stage) -> Step {
        self.stage = stage;
        Step::Continue
    }
    /// A tile answer: a number from 1, a point (in the current tile) or Enter (the current tile).
    fn tile(&self, s: &Session, i: &Input) -> Result<Option<usize>> {
        let (_, current) = live(s.state()?, s.viewport_px);
        match i {
            Input::Enter | Input::Point(_) => Ok(Some(current)),
            Input::Text(t) => {
                let n = active_config(s.doc()?).map_or(1, |c| c.tiles.len());
                tile_param(&json!({ "tile": t.trim() }), "tile", n)
            }
            _ => Err(EngineError::Other("Requires a viewport.".into())),
        }
    }
}

/// Named Viewports... without the dialog: -VPORTS Restore in the Model tab, MVIEW Restore in a
/// layout.
fn named_factory(s: &Session) -> Result<Box<dyn Interactive>> {
    Ok(Box::new(Restoring(vports_factory(s)?)))
}

/// -VPORTS or MVIEW started at its Restore option.
struct Restoring(Box<dyn Interactive>);

impl Interactive for Restoring {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match self.0.begin(s)? {
            Step::Continue => self.0.input(s, Input::Keyword("Restore".into())),
            other => Ok(other),
        }
    }
    fn prompt(&self, s: &Session) -> Prompt {
        self.0.prompt(s)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        self.0.input(s, i)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        self.0.preview(s, cursor)
    }
}
