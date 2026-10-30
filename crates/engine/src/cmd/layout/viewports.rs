//! Clipped viewports and MVIEW's options: polygonal and object viewports (MVIEW Polygonal /
//! Object), VPCLIP, ON/OFF, Lock and Restore (a model space viewport configuration as layout
//! viewports), and the typed MVIEW prompts.
//!
//! A clipped viewport keeps its rectangle as the boundary's extents and points at a paper-space
//! entity (a closed polyline, circle, ellipse or spline) whose outline clips what it shows
//! ([`Viewport::clip`], DXF group 340).

use cadcraft_doc::{Drawing, EntityKind, Handle, Space, Viewport, VportConfig, VportTile};
use cadcraft_geom::{Bounds2, PolyVertex, Vec2};
use serde_json::{Value, json};

use super::super::curves::{self, handle_value};
use super::super::helpers::{lwpoly, rect_vertices};
use super::super::{bad, bool_or, point_param, points_param, str_param, targets};
use super::{fit_view, layout_viewports, mview, printable, split, target_layout};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Most vertices a typed or JSON polygonal boundary may have.
const MAX_VERTICES: usize = 10_000;
/// Most viewports MVIEW Restore creates (a configuration read from a file may hold more tiles).
const MAX_RESTORED: usize = 64;

/// The polygon outline's extents of a closed paper-space entity usable as a clip boundary.
fn boundary_bounds(k: &EntityKind) -> Option<Bounds2> {
    cadcraft_render::clip::boundary_polygon(k).map(Bounds2::from_points)
}

/// The next free viewport id (ids are unique across layouts; 1 is a sheet's own view).
pub(super) fn next_viewport_id(d: &Drawing) -> u32 {
    d.layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some(v.id) } else { None })
        .max()
        .unwrap_or(1)
        .max(1)
        .saturating_add(1)
}

/// Layers marked "New VP Freeze": frozen in new viewports.
pub(super) fn new_vp_frozen(d: &Drawing) -> Vec<String> {
    d.layers.iter().filter(|l| l.vp_freeze_new).map(|l| l.name.clone()).collect()
}

/// A new viewport over `r`, zoomed to the model extents.
pub(super) fn new_viewport(d: &Drawing, r: Bounds2, id: u32) -> Viewport {
    let (view_center, view_height) = fit_view(d, r.width(), r.height());
    Viewport {
        center: r.center().to3(0.0),
        width: r.width(),
        height: r.height(),
        view_center,
        view_height,
        id,
        frozen_layers: new_vp_frozen(d),
        ..Viewport::default()
    }
}

/// Polygon vertices from `points` (+ optional `bulges`, one per vertex: the arc to the next).
fn vertices_param(cmd: &str, p: &Value) -> Result<Vec<PolyVertex>> {
    let raw = p.get("points").ok_or_else(|| bad(cmd, "`points` ([[x, y], …], at least three) is required"))?;
    if raw.as_array().is_some_and(|a| a.len() > MAX_VERTICES) {
        return Err(bad(cmd, format!("at most {MAX_VERTICES} points")));
    }
    let pts = points_param(p, "points").ok_or_else(|| bad(cmd, "`points` must be [[x, y], …]"))?;
    let bulges: Vec<f64> = match p.get("bulges") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a
            .iter()
            .take(MAX_VERTICES)
            .map(|v| v.as_f64().filter(|b| b.is_finite() && b.abs() <= 1e6).ok_or_else(|| bad(cmd, "`bulges` must be finite numbers")))
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, "`bulges` must be an array of numbers")),
    };
    let mut vs: Vec<PolyVertex> = Vec::new();
    for (i, q) in pts.into_iter().enumerate() {
        if vs.last().is_some_and(|l: &PolyVertex| l.p.near(q, 1e-12)) {
            continue;
        }
        vs.push(PolyVertex { bulge: bulges.get(i).copied().unwrap_or(0.0), ..PolyVertex::new(q) });
    }
    if vs.len() > 1 && vs.first().zip(vs.last()).is_some_and(|(a, b)| a.p.near(b.p, 1e-12)) {
        vs.pop();
    }
    if vs.len() < 3 {
        return Err(bad(cmd, "a polygonal viewport needs at least three distinct points"));
    }
    Ok(vs)
}

/// Check that `h` is a usable clip boundary in `space`; returns its extents.
fn check_boundary(cmd: &str, d: &Drawing, space: &Space, h: Handle) -> Result<Bounds2> {
    let e = d.space(space).and_then(|st| st.get(h)).ok_or_else(|| bad(cmd, format!("object {} is not in this layout", h.hex())))?;
    if matches!(e.kind, EntityKind::Viewport(_)) {
        return Err(bad(cmd, "a viewport can't be a clip boundary"));
    }
    if viewport_of_boundary(d, space, h).is_some() {
        return Err(bad(cmd, "the object already clips a viewport"));
    }
    let b = boundary_bounds(&e.kind).ok_or_else(|| bad(cmd, "the object must be a closed polyline, circle, ellipse or spline"))?;
    // The exact extents (the tessellated outline runs just inside curves).
    let eb = cadcraft_doc::entity_bounds(d, e, 0);
    let b = if eb.is_empty() || !eb.min.is_finite() || !eb.max.is_finite() { b } else { b.union(&eb) };
    if b.width() > 1e7 || b.height() > 1e7 {
        return Err(bad(cmd, "the viewport is too large"));
    }
    Ok(b)
}

/// The viewport in `space` clipped by `h`.
fn viewport_of_boundary(d: &Drawing, space: &Space, h: Handle) -> Option<Handle> {
    d.space(space)?.iter().find(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.clip == Some(h))).map(|e| e.handle)
}

/// Add a closed polyline (a polygonal viewport's boundary) to `space`.
fn add_boundary(s: &mut Session, space: &Space, vs: Vec<PolyVertex>) -> Result<Handle> {
    let common = s.current_common();
    Ok(s.doc_mut()?.add(space, common, lwpoly(vs, true))?)
}

/// A new viewport clipped by `boundary` (an entity in `space`), zoomed to the model extents.
fn add_clipped(s: &mut Session, cmd: &str, space: &Space, boundary: Handle) -> Result<Handle> {
    let b = check_boundary(cmd, s.doc()?, space, boundary)?;
    let common = s.current_common();
    let d = s.doc_mut()?;
    let id = next_viewport_id(d);
    let vp = Viewport { clip: Some(boundary), ..new_viewport(d, b, id) };
    Ok(d.add(space, common, EntityKind::Viewport(vp))?)
}

/// MVIEW Polygonal: `{points, bulges?, layout?}`.
fn mview_polygonal(s: &mut Session, p: &Value) -> Result<Value> {
    let space = target_layout(s, "mview", p)?;
    let vs = vertices_param("mview", p)?;
    let poly = lwpoly(vs.clone(), true);
    if boundary_bounds(&poly).is_none() {
        return Err(bad("mview", "the polygon has no area"));
    }
    let boundary = add_boundary(s, &space, vs)?;
    let vp = add_clipped(s, "mview", &space, boundary)?;
    Ok(json!({ "viewport": vp.hex(), "boundary": boundary.hex() }))
}

/// MVIEW Object: `{object, layout?}`.
fn mview_object(s: &mut Session, p: &Value) -> Result<Value> {
    let space = target_layout(s, "mview", p)?;
    let h = p.get("object").and_then(handle_value).ok_or_else(|| bad("mview", "`object` (a closed object's handle) is required"))?;
    let vp = add_clipped(s, "mview", &space, h)?;
    Ok(json!({ "viewport": vp.hex(), "boundary": h.hex() }))
}

/// Viewports among `hs` in `space`; a clip boundary stands for its viewport.
fn viewports_in(d: &Drawing, space: &Space, hs: &[Handle]) -> Vec<Handle> {
    let mut out = Vec::new();
    for h in hs {
        let vp = match d.space(space).and_then(|st| st.get(*h)).map(|e| &e.kind) {
            Some(EntityKind::Viewport(v)) if v.id != 1 => Some(*h),
            Some(_) => viewport_of_boundary(d, space, *h),
            None => None,
        };
        if let Some(v) = vp.filter(|v| !out.contains(v)) {
            out.push(v);
        }
    }
    out
}

/// The viewports an option acts on: `handle`/`handles` or the selection (boundaries count).
fn chosen(s: &Session, cmd: &str, p: &Value) -> Result<(Space, Vec<Handle>)> {
    let space = target_layout(s, cmd, p)?;
    let hs = viewports_in(s.doc()?, &space, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad(cmd, "no viewport given (pass `handles` or select viewports)"));
    }
    Ok((space, hs))
}

fn set_viewports(s: &mut Session, hs: &[Handle], f: impl Fn(&mut Viewport)) -> Result<()> {
    let d = s.doc_mut()?;
    for h in hs {
        d.modify_entity(*h, |e| {
            if let EntityKind::Viewport(v) = &mut e.kind {
                f(v);
            }
        })?;
    }
    Ok(())
}

/// MVIEW ON / OFF: `{handles?}`.
fn mview_on(s: &mut Session, p: &Value, on: bool) -> Result<Value> {
    let (_, hs) = chosen(s, "mview", p)?;
    set_viewports(s, &hs, |v| v.on = on)?;
    // A viewport turned off can't stay the active model space viewport.
    if !on && s.state()?.mspace.is_some_and(|m| hs.contains(&m)) {
        s.state_mut()?.mspace = None;
    }
    Ok(json!({ "viewports": hs.iter().map(|h| h.hex()).collect::<Vec<_>>(), "on": on }))
}

/// MVIEW Lock: `{locked: bool, handles?}`.
fn mview_lock(s: &mut Session, p: &Value) -> Result<Value> {
    let locked = match p.get("locked") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(t)) if t.eq_ignore_ascii_case("on") => true,
        Some(Value::String(t)) if t.eq_ignore_ascii_case("off") => false,
        _ => return Err(bad("mview", "`locked` (true/false) is required")),
    };
    let (_, hs) = chosen(s, "mview", p)?;
    set_viewports(s, &hs, |v| v.locked = locked)?;
    Ok(json!({ "viewports": hs.iter().map(|h| h.hex()).collect::<Vec<_>>(), "locked": locked }))
}

/// Where tile `t` of a viewport configuration lands when the whole configuration fills `r`; `None`
/// for a tile with no usable area (configurations come from files).
fn tile_rect(t: &VportTile, r: Bounds2) -> Option<Bounds2> {
    let f = |v: f64| v.is_finite().then(|| v.clamp(0.0, 1.0));
    let (x0, y0, x1, y1) = (f(t.min.x)?, f(t.min.y)?, f(t.max.x)?, f(t.max.y)?);
    if x1 - x0 < 1e-6 || y1 - y0 < 1e-6 {
        return None;
    }
    let at = |x: f64, y: f64| Vec2::new(r.min.x + x * r.width(), r.min.y + y * r.height());
    Some(Bounds2::new(at(x0, y0), at(x1, y1)))
}

/// The view a restored tile shows in a viewport of size `r`: its saved centre, and its height
/// grown when the viewport is narrower than the tile was, so the whole saved view still shows.
fn tile_view(t: &VportTile, r: Bounds2) -> Option<(Vec2, f64)> {
    if !(t.center.is_finite() && t.height.is_finite() && t.height > 0.0) {
        return None;
    }
    let aspect = r.width() / r.height().max(1e-12);
    let h = if t.aspect.is_finite() && t.aspect > 0.0 && aspect > 0.0 { t.height.max(t.height * t.aspect / aspect) } else { t.height };
    (h.is_finite() && h > 0.0).then_some((t.center, h))
}

/// MVIEW Restore: `{name? (default *Active), p1?, p2? (default: the printable area), layout?}`.
/// One viewport per tile of the model space viewport configuration, arranged as the tiles are
/// over the window, each showing its tile's view.
fn mview_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let space = target_layout(s, "mview", p)?;
    let Space::Paper(layout) = &space else { return Err(bad("mview", "not a layout")) };
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or(VportConfig::ACTIVE).to_string();
    let tiles = super::super::view::config_tiles(s, &name)
        .ok_or_else(|| EngineError::Other(format!("Could not find viewport configuration \"{}\".", name.chars().take(64).collect::<String>())))?;
    let rect = match (point_param(p, "p1"), point_param(p, "p2")) {
        (Some(a), Some(b)) => Bounds2::new(a, b),
        _ => printable(s, layout)?,
    };
    if rect.is_empty() || !rect.min.is_finite() || !rect.max.is_finite() || rect.width() < 1e-6 || rect.height() < 1e-6 {
        return Err(bad("mview", "the viewport has no area"));
    }
    if rect.width() > 1e7 || rect.height() > 1e7 {
        return Err(bad("mview", "the viewport is too large"));
    }
    let common = s.current_common();
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for t in tiles.iter().take(MAX_RESTORED) {
        let Some(r) = tile_rect(t, rect) else { continue };
        let id = next_viewport_id(d);
        let mut vp = new_viewport(d, r, id);
        if let Some((c, h)) = tile_view(t, r) {
            (vp.view_center, vp.view_height) = (c, h);
        }
        out.push(d.add(&space, common.clone(), EntityKind::Viewport(vp))?);
    }
    if out.is_empty() {
        return Err(EngineError::Other(format!("Viewport configuration \"{name}\" has no viewports to restore.")));
    }
    Ok(json!({ "configuration": name, "viewports": handles_json(&out) }))
}

/// MVIEW's options other than corners and Fit; `None` when `option` is not one of them.
pub(super) fn mview_option(s: &mut Session, p: &Value) -> Option<Result<Value>> {
    let opt = str_param(p, "option")?.trim().to_ascii_lowercase();
    Some(match opt.as_str() {
        "polygonal" | "p" => mview_polygonal(s, p),
        "object" | "o" => mview_object(s, p),
        "on" => mview_on(s, p, true),
        "off" => mview_on(s, p, false),
        "lock" | "l" => mview_lock(s, p),
        "restore" | "r" => mview_restore(s, p),
        "fit" | "f" | "corners" | "" => return None,
        _ => Err(bad("mview", "option must be fit, polygonal, object, on, off, lock or restore")),
    })
}

/// VPCLIP: `{handle? (viewport; default: the selected or active one), object? | points? (+ bulges?), delete?: bool}`.
pub(super) fn run_vpclip(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "vpclip";
    let space = target_layout(s, cmd, p)?;
    let d = s.doc()?;
    let mut hs = viewports_in(d, &space, &targets(s, p)?);
    if hs.is_empty()
        && p.get("handle").is_none()
        && let Some((h, _)) = s.state()?.active_viewport()
    {
        hs.push(h);
    }
    let vh = *hs.first().ok_or_else(|| bad(cmd, "no viewport given (pass `handle` or select a viewport)"))?;
    let Some(EntityKind::Viewport(cur)) = d.entity(vh).map(|e| e.kind.clone()) else { return Err(bad(cmd, "not a viewport")) };
    let old = cur.clip.filter(|b| *b != vh);
    if bool_or(p, "delete", false) {
        let Some(b) = old else { return Err(bad(cmd, "the viewport is not clipped")) };
        set_viewports(s, &[vh], |v| v.clip = None)?;
        s.doc_mut()?.remove_entity(b);
        return Ok(json!({ "viewport": vh.hex(), "clip": Value::Null }));
    }
    let boundary = match (p.get("object"), p.get("points")) {
        (Some(o), _) => {
            let h = handle_value(o).ok_or_else(|| bad(cmd, "`object` must be an object handle"))?;
            check_boundary(cmd, s.doc()?, &space, h)?;
            h
        }
        (None, Some(_)) => {
            let vs = vertices_param(cmd, p)?;
            if boundary_bounds(&lwpoly(vs.clone(), true)).is_none() {
                return Err(bad(cmd, "the polygon has no area"));
            }
            add_boundary(s, &space, vs)?
        }
        (None, None) => return Err(bad(cmd, "pass `object`, `points` or `delete`")),
    };
    let b = check_boundary(cmd, s.doc()?, &space, boundary)?;
    // The viewport takes the boundary's extents; model space stays where it was on the sheet.
    let k = cur.view_height / cur.height.max(1e-12);
    let mut v = cur;
    let c = b.center();
    let view_center = v.view_center + (c - v.center.xy()) * k;
    let view_height = b.height() * k;
    if view_center.is_finite() && view_height.is_finite() && view_height > 0.0 {
        v.view_center = view_center;
        v.view_height = view_height;
    }
    v.center = c.to3(v.center.z);
    v.width = b.width();
    v.height = b.height();
    v.clip = Some(boundary);
    s.doc_mut()?.modify_entity(vh, |e| e.kind = EntityKind::Viewport(v))?;
    if let Some(o) = old.filter(|o| *o != boundary) {
        s.doc_mut()?.remove_entity(o);
    }
    Ok(json!({ "viewport": vh.hex(), "clip": boundary.hex() }))
}

/// The viewport (turned on) under a paper-space point, topmost first, honouring clip boundaries.
pub(super) fn viewport_at(s: &Session, at: Vec2) -> Option<Handle> {
    let d = s.doc().ok()?;
    layout_viewports(s).ok()?.into_iter().rev().find(|(h, v)| contains(d, *h, v, at)).map(|(h, _)| h)
}

/// Like [`viewport_at`], also finding viewports that are turned off (MVIEW ON picks them).
fn any_viewport_at(s: &Session, at: Vec2) -> Option<Handle> {
    let d = s.doc().ok()?;
    let st = d.space(&s.layout_space())?;
    st.iter().filter(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.id != 1 && contains(d, e.handle, v, at))).last().map(|e| e.handle)
}

/// Is the paper point `at` inside the viewport (its clip outline when clipped)?
pub(super) fn contains(d: &Drawing, h: Handle, v: &Viewport, at: Vec2) -> bool {
    match cadcraft_render::viewport_clip(d, h, v) {
        Some(c) => c.contains(at),
        None => (at.x - v.center.x).abs() <= v.width / 2.0 && (at.y - v.center.y).abs() <= v.height / 2.0,
    }
}

// ---------- typed MVIEW / VPCLIP ----------

/// Bulge of an arc leaving `from` along `tangent` and ending at `to` (a polyline arc segment).
fn tangent_bulge(from: Vec2, tangent: Vec2, to: Vec2) -> Option<f64> {
    let c = to - from;
    let theta = tangent.cross(c).atan2(tangent.dot(c));
    let b = (theta / 2.0).tan();
    (theta.abs() < std::f64::consts::PI - 1e-6 && b.is_finite()).then_some(b)
}

/// Direction at the end of the polygon drawn so far (for a tangent arc).
fn end_tangent(vs: &[PolyVertex]) -> Vec2 {
    match vs {
        [.., a, b] => {
            let chord = b.p - a.p;
            let ang = chord.y.atan2(chord.x) + 2.0 * a.bulge.atan();
            Vec2::new(ang.cos(), ang.sin())
        }
        _ => Vec2::new(1.0, 0.0),
    }
}

/// Prompts for a polygon boundary: start point, then points with Arc/Line/Close/Undo.
#[derive(Clone, Debug, Default)]
pub(super) struct PolygonInput {
    pub vs: Vec<PolyVertex>,
    arc: bool,
}

pub(super) enum PolyStep {
    More,
    Closed(Vec<PolyVertex>),
}

impl PolygonInput {
    pub fn prompt(&self) -> Prompt {
        let base = self.vs.last().map(|v| v.p);
        match (self.vs.len(), self.arc) {
            (0, _) => Prompt::new("Specify start point", Accept::POINT),
            (_, false) => Prompt::new("Specify next point", Accept::POINT).kw(&["Arc", "Close", "Undo"]).base_opt(base),
            (_, true) => Prompt::new("Specify endpoint of arc", Accept::POINT).kw(&["Line", "Close", "Undo"]).base_opt(base),
        }
    }
    pub fn input(&mut self, i: Input) -> Result<PolyStep> {
        match i {
            Input::Point(q) => {
                if self.vs.last().is_some_and(|l| l.p.near(q, 1e-12)) {
                    return Ok(PolyStep::More);
                }
                if self.vs.len() >= MAX_VERTICES {
                    return Err(EngineError::Other(format!("At most {MAX_VERTICES} points.")));
                }
                if self.arc
                    && let Some(last) = self.vs.last().map(|v| v.p)
                {
                    let b = tangent_bulge(last, end_tangent(&self.vs), q).ok_or_else(|| EngineError::Other("Invalid arc endpoint.".into()))?;
                    if let Some(v) = self.vs.last_mut() {
                        v.bulge = b;
                    }
                }
                self.vs.push(PolyVertex::new(q));
                Ok(PolyStep::More)
            }
            Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                "arc" | "a" if !self.vs.is_empty() => {
                    self.arc = true;
                    Ok(PolyStep::More)
                }
                "line" | "l" => {
                    self.arc = false;
                    Ok(PolyStep::More)
                }
                "undo" | "u" => {
                    self.vs.pop();
                    if let Some(v) = self.vs.last_mut() {
                        v.bulge = 0.0;
                    }
                    Ok(PolyStep::More)
                }
                "close" | "c" => self.close(),
                _ => Err(EngineError::Other("Requires a point or option keyword.".into())),
            },
            Input::Enter => self.close(),
            _ => Ok(PolyStep::More),
        }
    }
    fn close(&mut self) -> Result<PolyStep> {
        if self.vs.len() < 3 {
            return Err(EngineError::Other("A polygonal boundary needs at least three points.".into()));
        }
        if self.arc
            && let (Some(first), Some(last)) = (self.vs.first().map(|v| v.p), self.vs.last().map(|v| v.p))
        {
            let b = tangent_bulge(last, end_tangent(&self.vs), first).unwrap_or(0.0);
            if let Some(v) = self.vs.last_mut() {
                v.bulge = b;
            }
        }
        Ok(PolyStep::Closed(std::mem::take(&mut self.vs)))
    }
    pub fn preview(&self, c: Vec2) -> Vec<EntityKind> {
        if self.vs.is_empty() {
            return Vec::new();
        }
        let mut vs = self.vs.clone();
        if self.arc
            && let (Some(last), Some(b)) = (vs.last().map(|v| v.p), vs.last().map(|v| v.p).and_then(|l| tangent_bulge(l, end_tangent(&self.vs), c)))
            && !last.near(c, 1e-12)
            && let Some(v) = vs.last_mut()
        {
            v.bulge = b;
        }
        vs.push(PolyVertex::new(c));
        vec![lwpoly(vs, true)]
    }
}

fn points_json(vs: &[PolyVertex]) -> Value {
    json!({
        "points": vs.iter().map(|v| [v.p.x, v.p.y]).collect::<Vec<_>>(),
        "bulges": vs.iter().map(|v| v.bulge).collect::<Vec<_>>(),
    })
}

pub(super) fn with(p: Value, key: &str, v: Value) -> Value {
    let mut p = p;
    if let Some(o) = p.as_object_mut() {
        o.insert(key.into(), v);
    }
    p
}

fn handles_json(hs: &[Handle]) -> Value {
    Value::Array(hs.iter().map(|h| json!(h.hex())).collect())
}

/// Viewports picked at a "Select viewports" prompt.
fn pick_viewports(s: &mut Session, i: &Input, picked: &mut Vec<Handle>) {
    let found: Vec<Handle> = match i {
        Input::Pick(hs) => s.doc().map(|d| viewports_in(d, &s.space(), hs)).unwrap_or_default(),
        Input::Point(p) => any_viewport_at(s, *p).into_iter().collect(),
        _ => Vec::new(),
    };
    let before = picked.len();
    for h in found {
        if !picked.contains(&h) {
            picked.push(h);
        }
    }
    if picked.len() > before {
        s.echo(format!("{} viewport(s) selected.", picked.len()));
    } else {
        s.echo("No viewport found there.");
    }
}

#[derive(Clone, Debug, Default)]
enum Stage {
    /// "Specify corner of viewport or [ON/OFF/Fit/Lock/Object/Polygonal/Restore/2/3/4]".
    #[default]
    Corner,
    /// 2 or 3 viewports: their arrangement.
    Arrange,
    Opposite(Vec2),
    Polygon(PolygonInput),
    Object,
    Lock,
    /// Restore: the configuration's name, then where its viewports go.
    RestoreName,
    RestoreCorner(String),
    RestoreOpposite(String, Vec2),
    /// ON/OFF (Some(on)) or Lock (None, with the chosen state) on picked viewports.
    Select {
        on: Option<bool>,
        locked: bool,
        picked: Vec<Handle>,
    },
}

/// MVIEW / VPORTS prompts.
pub(super) struct MviewM {
    count: usize,
    /// Offer MVIEW's options (MVIEW) rather than a fixed count (View > Viewports > n).
    choose: bool,
    arrangement: String,
    stage: Stage,
}

impl MviewM {
    pub fn new(count: usize, choose: bool) -> Self {
        MviewM { count, choose, arrangement: String::new(), stage: Stage::default() }
    }
    /// View > Viewports > Polygonal Viewport: straight to the polygon's points.
    pub fn polygonal() -> Self {
        MviewM { stage: Stage::Polygon(PolygonInput::default()), ..MviewM::new(1, true) }
    }
    fn create(&self, s: &mut Session, rect: Option<Bounds2>) -> Result<Step> {
        let mut p = json!({ "count": self.count, "arrangement": self.arrangement });
        if let Some(r) = rect {
            p = with(p, "p1", json!([r.min.x, r.min.y]));
            p = with(p, "p2", json!([r.max.x, r.max.y]));
        }
        let r = mview(s, &p, self.count, "mview")?;
        let n = r.get("viewports").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        s.echo(format!("{n} viewport(s) created, zoomed to the model extents."));
        Ok(Step::Done)
    }
    /// MVIEW Restore with the typed name, over `rect` (None: Fit).
    fn restore(s: &mut Session, name: &str, rect: Option<Bounds2>) -> Result<Step> {
        let mut p = json!({ "option": "restore", "name": name });
        if let Some(r) = rect {
            p = with(p, "p1", json!([r.min.x, r.min.y]));
            p = with(p, "p2", json!([r.max.x, r.max.y]));
        }
        let r = mview_restore(s, &p)?;
        let n = r.get("viewports").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        s.echo(format!("{n} viewport(s) restored from \"{name}\"."));
        Ok(Step::Done)
    }
    fn corner_keyword(&mut self, s: &mut Session, k: &str) -> Result<Step> {
        if k == "fit" || k == "f" {
            return self.create(s, None);
        }
        if !self.choose {
            return Err(EngineError::Other("Requires a point or option keyword.".into()));
        }
        self.stage = match k {
            n @ ("2" | "3" | "4") => {
                self.count = n.parse().unwrap_or(1);
                if self.count == 4 { Stage::Corner } else { Stage::Arrange }
            }
            "on" => Stage::Select { on: Some(true), locked: false, picked: Vec::new() },
            "off" => Stage::Select { on: Some(false), locked: false, picked: Vec::new() },
            "lock" | "l" => Stage::Lock,
            "object" | "o" => Stage::Object,
            "polygonal" | "p" => Stage::Polygon(PolygonInput::default()),
            "restore" | "r" => Stage::RestoreName,
            _ => return Err(EngineError::Other("Requires a point or option keyword.".into())),
        };
        Ok(Step::Continue)
    }
}

impl Interactive for MviewM {
    fn name(&self) -> &'static str {
        "MVIEW"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if s.space() == Space::Model && s.state()?.mspace.is_none() {
            s.echo("** Command not allowed in Model Tab ** (switch to a layout first)");
            return Ok(Step::Done);
        }
        // MVIEW works on the sheet.
        if s.state()?.mspace.is_some() {
            s.state_mut()?.mspace = None;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.stage {
            Stage::Corner if self.choose && self.count == 1 => Prompt::new("Specify corner of viewport", Accept::POINT)
                .kw(&["ON", "OFF", "Fit", "Lock", "Object", "Polygonal", "Restore", "2", "3", "4"])
                .default("Fit"),
            Stage::Corner => Prompt::new("Specify first corner", Accept::POINT).kw(&["Fit"]).default("Fit"),
            Stage::Arrange if self.count == 2 => {
                Prompt::new("Enter viewport arrangement", curves::KW).kw(&["Horizontal", "Vertical"]).default("Vertical")
            }
            Stage::Arrange => Prompt::new("Enter viewport arrangement", curves::KW)
                .kw(&["Horizontal", "Vertical", "Above", "Below", "Left", "Right"])
                .default("Right"),
            Stage::Opposite(a) => Prompt::new("Specify opposite corner", Accept::POINT).corner(*a),
            Stage::Polygon(pi) => pi.prompt(),
            Stage::Object => Prompt::new("Select object to clip viewport", Accept::POINT),
            Stage::Lock => Prompt::new("Lock the view of viewports?", curves::KW).kw(&["ON", "OFF"]),
            Stage::RestoreName => Prompt::new("Enter viewport configuration name", Accept::TEXT).kw(&["?"]).default(VportConfig::ACTIVE),
            Stage::RestoreCorner(_) => Prompt::new("Specify first corner", Accept::POINT).kw(&["Fit"]).default("Fit"),
            Stage::RestoreOpposite(_, a) => Prompt::new("Specify opposite corner", Accept::POINT).corner(*a),
            Stage::Select { .. } => Prompt::new("Select viewports", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.stage) {
            Stage::Corner => match i {
                Input::Enter => self.create(s, None),
                Input::Keyword(k) | Input::Text(k) => self.corner_keyword(s, &k.trim().to_ascii_lowercase()),
                Input::Point(p) => {
                    self.stage = Stage::Opposite(p);
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::Arrange => {
                let a = match &i {
                    Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
                    _ => String::new(),
                };
                let ok = match self.count {
                    2 => ["", "h", "horizontal", "v", "vertical"].contains(&a.as_str()),
                    _ => ["", "h", "horizontal", "v", "vertical", "a", "above", "b", "below", "l", "left", "r", "right"].contains(&a.as_str()),
                };
                if !ok || !matches!(i, Input::Keyword(_) | Input::Text(_) | Input::Enter) {
                    self.stage = Stage::Arrange;
                    return Err(EngineError::Other("Invalid option keyword.".into()));
                }
                self.arrangement = a;
                Ok(Step::Continue)
            }
            Stage::Opposite(a) => match i {
                Input::Point(b) => self.create(s, Some(Bounds2::new(a, b))),
                _ => {
                    self.stage = Stage::Opposite(a);
                    Ok(Step::Continue)
                }
            },
            Stage::Polygon(mut pi) => match pi.input(i) {
                Ok(PolyStep::More) => {
                    self.stage = Stage::Polygon(pi);
                    Ok(Step::Continue)
                }
                Ok(PolyStep::Closed(vs)) => {
                    mview_polygonal(s, &points_json(&vs))?;
                    s.echo("Viewport created, zoomed to the model extents.");
                    Ok(Step::Done)
                }
                Err(e) => {
                    self.stage = Stage::Polygon(pi);
                    Err(e)
                }
            },
            Stage::Object => match i {
                Input::Point(p) => {
                    let Some(h) = curves::pick_at(s, p) else {
                        self.stage = Stage::Object;
                        s.echo("No object found.");
                        return Ok(Step::Continue);
                    };
                    if let Err(e) = mview_object(s, &json!({ "object": h.hex() })) {
                        self.stage = Stage::Object;
                        return Err(e);
                    }
                    s.echo("Viewport created, zoomed to the model extents.");
                    Ok(Step::Done)
                }
                Input::Pick(hs) if hs.len() == 1 => {
                    let h = hs.first().copied().unwrap_or_default();
                    mview_object(s, &json!({ "object": h.hex() }))?;
                    Ok(Step::Done)
                }
                _ => {
                    self.stage = Stage::Object;
                    Ok(Step::Continue)
                }
            },
            Stage::Lock => {
                let k = match &i {
                    Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
                    _ => String::new(),
                };
                self.stage = match k.as_str() {
                    "on" => Stage::Select { on: None, locked: true, picked: Vec::new() },
                    "off" => Stage::Select { on: None, locked: false, picked: Vec::new() },
                    _ => {
                        self.stage = Stage::Lock;
                        return Err(EngineError::Other("Invalid option keyword.".into()));
                    }
                };
                Ok(Step::Continue)
            }
            Stage::RestoreName => {
                let name = match &i {
                    Input::Enter => VportConfig::ACTIVE.to_string(),
                    Input::Text(t) | Input::Keyword(t) if !t.trim().is_empty() => t.trim().to_string(),
                    _ => {
                        self.stage = Stage::RestoreName;
                        return Ok(Step::Continue);
                    }
                };
                self.stage = Stage::RestoreName;
                if name == "?" {
                    let names = super::super::view::config_names(s);
                    s.echo(if names.is_empty() { "No saved viewport configurations.".to_string() } else { names.join(", ") });
                    return Ok(Step::Continue);
                }
                if super::super::view::config_tiles(s, &name).is_none() {
                    return Err(EngineError::Other(format!(
                        "Could not find viewport configuration \"{}\".",
                        name.chars().take(64).collect::<String>()
                    )));
                }
                self.stage = Stage::RestoreCorner(name);
                Ok(Step::Continue)
            }
            Stage::RestoreCorner(name) => match i {
                Input::Enter => Self::restore(s, &name, None),
                Input::Keyword(k) | Input::Text(k) if matches!(k.trim().to_ascii_lowercase().as_str(), "fit" | "f") => Self::restore(s, &name, None),
                Input::Point(a) => {
                    self.stage = Stage::RestoreOpposite(name, a);
                    Ok(Step::Continue)
                }
                Input::Keyword(_) | Input::Text(_) => {
                    self.stage = Stage::RestoreCorner(name);
                    Err(EngineError::Other("Requires a point or option keyword.".into()))
                }
                _ => {
                    self.stage = Stage::RestoreCorner(name);
                    Ok(Step::Continue)
                }
            },
            Stage::RestoreOpposite(name, a) => match i {
                Input::Point(b) => Self::restore(s, &name, Some(Bounds2::new(a, b))),
                _ => {
                    self.stage = Stage::RestoreOpposite(name, a);
                    Ok(Step::Continue)
                }
            },
            Stage::Select { on, locked, mut picked } => match i {
                Input::Enter => {
                    if picked.is_empty() {
                        return Ok(Step::Done);
                    }
                    let p = json!({ "handles": handles_json(&picked) });
                    match on {
                        Some(on) => mview_on(s, &p, on)?,
                        None => mview_lock(s, &with(p, "locked", json!(locked)))?,
                    };
                    Ok(Step::Done)
                }
                other => {
                    pick_viewports(s, &other, &mut picked);
                    self.stage = Stage::Select { on, locked, picked };
                    Ok(Step::Continue)
                }
            },
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        match &self.stage {
            Stage::Opposite(a) => {
                split(Bounds2::new(*a, c), self.count, &self.arrangement).into_iter().map(|r| lwpoly(rect_vertices(r.min, r.max), true)).collect()
            }
            Stage::Polygon(pi) => pi.preview(c),
            Stage::RestoreOpposite(name, a) => {
                let r = Bounds2::new(*a, c);
                super::super::view::config_tiles(s, name)
                    .unwrap_or_default()
                    .iter()
                    .take(MAX_RESTORED)
                    .filter_map(|t| tile_rect(t, r))
                    .map(|r| lwpoly(rect_vertices(r.min, r.max), true))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
enum ClipStage {
    #[default]
    Viewport,
    /// "Select clipping object or [Polygonal/Delete] <Polygonal>".
    Clip(Handle),
    Polygon(Handle, PolygonInput),
}

/// VPCLIP typed at the command line.
#[derive(Default)]
pub(super) struct VpclipM {
    stage: ClipStage,
}

impl Interactive for VpclipM {
    fn name(&self) -> &'static str {
        "VPCLIP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if s.layout_space() == Space::Model {
            s.echo("** Command not allowed in Model Tab ** (switch to a layout first)");
            return Ok(Step::Done);
        }
        // A selected viewport (or the active one) is the one to clip.
        let pre = s.doc().map(|d| viewports_in(d, &s.layout_space(), &s.selection())).unwrap_or_default();
        if let Some(h) = pre.first().copied().or_else(|| s.state().ok().and_then(|st| st.active_viewport()).map(|(h, _)| h)) {
            self.stage = ClipStage::Clip(h);
            s.state_mut()?.mspace = None;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            ClipStage::Viewport => Prompt::new("Select viewport to clip", Accept::POINT),
            ClipStage::Clip(h) => {
                let clipped = s.doc().ok().and_then(|d| d.entity(*h)).is_some_and(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.clip.is_some()));
                let kws: &[&str] = if clipped { &["Polygonal", "Delete"] } else { &["Polygonal"] };
                Prompt::new("Select clipping object", Accept::POINT).kw(kws).default("Polygonal")
            }
            ClipStage::Polygon(_, pi) => pi.prompt(),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.stage) {
            ClipStage::Viewport => {
                let h = match &i {
                    Input::Point(p) => viewport_at(s, *p),
                    Input::Pick(hs) => s.doc().ok().and_then(|d| viewports_in(d, &s.layout_space(), hs).first().copied()),
                    _ => None,
                };
                match h {
                    Some(h) => self.stage = ClipStage::Clip(h),
                    None => {
                        if matches!(i, Input::Point(_) | Input::Pick(_)) {
                            s.echo("No viewport found there.");
                        }
                        if matches!(i, Input::Enter) {
                            return Ok(Step::Done);
                        }
                    }
                }
                Ok(Step::Continue)
            }
            ClipStage::Clip(vh) => {
                let object = match &i {
                    Input::Point(p) => {
                        let Some(o) = curves::pick_at(s, *p) else {
                            self.stage = ClipStage::Clip(vh);
                            s.echo("No object found.");
                            return Ok(Step::Continue);
                        };
                        o
                    }
                    Input::Pick(hs) if hs.len() == 1 => hs.first().copied().unwrap_or_default(),
                    Input::Enter => {
                        self.stage = ClipStage::Polygon(vh, PolygonInput::default());
                        return Ok(Step::Continue);
                    }
                    Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                        "polygonal" | "p" => {
                            self.stage = ClipStage::Polygon(vh, PolygonInput::default());
                            return Ok(Step::Continue);
                        }
                        "delete" | "d" => {
                            run_vpclip(s, &json!({ "handle": vh.hex(), "delete": true }))?;
                            s.echo("Clipping boundary deleted.");
                            return Ok(Step::Done);
                        }
                        _ => {
                            self.stage = ClipStage::Clip(vh);
                            return Err(EngineError::Other("Requires a point or option keyword.".into()));
                        }
                    },
                    _ => {
                        self.stage = ClipStage::Clip(vh);
                        return Ok(Step::Continue);
                    }
                };
                if let Err(e) = run_vpclip(s, &json!({ "handle": vh.hex(), "object": object.hex() })) {
                    self.stage = ClipStage::Clip(vh);
                    return Err(e);
                }
                Ok(Step::Done)
            }
            ClipStage::Polygon(vh, mut pi) => match pi.input(i) {
                Ok(PolyStep::More) => {
                    self.stage = ClipStage::Polygon(vh, pi);
                    Ok(Step::Continue)
                }
                Ok(PolyStep::Closed(vs)) => {
                    run_vpclip(s, &with(points_json(&vs), "handle", json!(vh.hex())))?;
                    Ok(Step::Done)
                }
                Err(e) => {
                    self.stage = ClipStage::Polygon(vh, pi);
                    Err(e)
                }
            },
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match &self.stage {
            ClipStage::Polygon(_, pi) => pi.preview(c),
            _ => Vec::new(),
        }
    }
}
