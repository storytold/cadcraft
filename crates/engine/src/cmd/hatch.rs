//! HATCH, GRADIENT, BOUNDARY, REGION: boundaries from pick points or selected closed objects.

mod patfile;
pub use patfile::{PatternChoice, pattern_choices};
pub(super) use patfile::{lines_for, run_patterns};

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, EntityKind, Gradient, Handle, Hatch, HatchLoop, Space, entity_bounds};
use cadcraft_geom::{Bounds2, PolyVertex, Polyline, Vec2, point_in_polygon};
use serde_json::{Value, json};

use super::helpers::lwpoly;
use super::{CommandSpec, bad, bool_or, f64_or, point_param, points_param, size_param, str_param, targets};

use crate::{Accept, EngineError, Input, Interactive, Picker, Prompt, Result, Session, Step};

mod assoc;
mod edit;
mod pick;
use super::has_doc;
pub(crate) use assoc::update as refill_associative;
pub use pick::{HatchPicks, PickMode};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("hatch", "Hatch...", run_hatch).menu(&["Draw", "Hatch..."]).alias(&["h", "bh", "-hatch"]).params("{points?: [[x,y]] (internal points) and/or handles?: [hex] (closed objects), pattern?: \"ANSI31\" (built in, or defined by patternFile | patternText), patternFile?: path to a .pat file | patternText?: its contents (loads the patterns it defines), scale?, angle? (degrees), associative? (HPASSOC), separate? (HPSEPARATE: a hatch per point/object), origin?: [x,y] (HPORIGIN) | originMode?: \"current\"|\"bottomLeft\"|\"bottomRight\"|\"topLeft\"|\"topRight\"|\"center\" (HPORIGINMODE), style?: \"normal\"|\"outer\"|\"ignore\" (HPISLANDDETECTION)} — gaps up to HPGAPTOL close").interactive(|s| Ok(Box::new(HatchM::new(s, false)))),
        CommandSpec::new("gradient", "Gradient...", run_gradient).menu(&["Draw", "Gradient..."]).alias(&["gd"]).params("{points? and/or handles?, name?: \"LINEAR\"|\"CYLINDER\"|\"CURVED\"|\"SPHERICAL\"|\"HEMISPHERICAL\" or INV… (colours swapped), color1?, color2?, angle? (degrees), centered? (default true)}").interactive(|s| Ok(Box::new(HatchM::new(s, true)))),
        CommandSpec::new("boundary", "Boundary...", run_boundary).menu(&["Draw", "Boundary..."]).alias(&["bo", "bpoly"]).params("{points: [[x,y]]}").interactive(|_| Ok(Box::new(BoundaryM { region: false }))),
        CommandSpec::new("region", "Region", run_region).menu(&["Draw", "Region"]).alias(&["reg"]).params("{handles?}"),
        CommandSpec::new("hatchedit", "Hatch Edit", edit::run_hatchedit).menu(&["Modify", "Object", "Hatch..."]).alias(&["he", "-hatchedit"]).params("{handles?, pattern?, scale?, angle?, gradient?: {name?, color1?, color2?, angle? (degrees), centered?} (makes or changes a gradient fill; `pattern` turns it back), associative?: bool (false unlinks; true links each loop to a closed object of the same shape, or to `boundary` objects), boundary?: [hex], addPoints?: [[x,y]], addBoundary?: [hex], removeLoops?: [zero-based index] (geometry edits require one hatch), style?, origin? | originMode?, separate?: true, recreate?: \"polyline\"|\"region\", associate?: bool (link to the recreated boundary)}").interactive(|s| Ok(Box::new(edit::HatchEditM::new(s)))),
        CommandSpec::new("hatchpick", "Hatch Pick", pick::run_hatchpick)
            .params("{points?: [[x,y]] (internal points), handles?: [hex] (closed objects)} (typed: Points, Objects, oRigin or Hatch, then the picks): the Hatch and Gradient dialog's picks, kept in the session; changes nothing")
            .enabled(has_doc)
            .noundo()
            .interactive(|_| Ok(Box::new(pick::HatchPickM::default()))),
    ]
}

/// The tolerance curves are flattened with for boundary tracing, in a space with extents `ext`.
pub(super) fn outline_tol(ext: Bounds2) -> f64 {
    (ext.width() + ext.height()).max(1e-9) / 20000.0
}

/// Tessellated outlines of every visible curve in the space (hatch boundary candidates), each
/// with the handle of the object it came from.
fn outlines(d: &Drawing, space: &Space, near: Option<Bounds2>) -> (Vec<Vec<Vec2>>, Vec<Handle>) {
    let Some(store) = d.space(space) else { return (Vec::new(), Vec::new()) };
    let ext = d.extents(space);
    let tol = outline_tol(ext);
    let mut out = Vec::new();
    let mut owners = Vec::new();
    for e in store.iter() {
        if !d.is_visible(e) || matches!(e.kind, EntityKind::Hatch(_) | EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_)) {
            continue;
        }
        if let Some(n) = near
            && !matches!(e.kind, EntityKind::XLine(_) | EntityKind::Ray(_))
            && !entity_bounds(d, e, 0).intersects(&n)
        {
            continue;
        }
        let polys = object_polys(d, e, tol, ext);
        owners.extend(std::iter::repeat_n(e.handle, polys.len()));
        out.extend(polys);
    }
    (out, owners)
}

/// An object's outline as polylines (tolerance `tol`), infinite lines clipped to a generous
/// box around the extents `ext`.
fn object_polys(d: &Drawing, e: &Entity, tol: f64, ext: Bounds2) -> Vec<Vec<Vec2>> {
    let mut polys = crate::select::hit_polylines(d, e, tol);
    if matches!(e.kind, EntityKind::XLine(_) | EntityKind::Ray(_)) {
        let r = (ext.width() + ext.height()).max(1.0) * 4.0;
        for pl in &mut polys {
            if let [a, b] = pl.as_slice() {
                let m = a.mid(*b);
                let dir = (*b - *a).normalized();
                *pl = vec![m - dir * r, m + dir * r];
            }
        }
    }
    polys
}

/// The objects behind input polylines (indices into `owners`), without repeats.
fn handles_of(owners: &[Handle], idx: &[usize]) -> Vec<Handle> {
    let mut hs: Vec<Handle> = idx.iter().filter_map(|&i| owners.get(i).copied()).collect();
    hs.sort_unstable();
    hs.dedup();
    hs
}

/// Closed loops of entities that are closed curves (for islands and "select objects").
fn closed_loop(d: &Drawing, e: &Entity) -> Option<Vec<PolyVertex>> {
    match &e.kind {
        EntityKind::Circle(c) => {
            let ctr = c.center.xy();
            Some(vec![PolyVertex::with_bulge(ctr - Vec2::X * c.radius, 1.0), PolyVertex::with_bulge(ctr + Vec2::X * c.radius, 1.0)])
        }
        EntityKind::LwPolyline(p) if p.closed || p.vertices.first().zip(p.vertices.last()).is_some_and(|(a, b)| a.p.near(b.p, 1e-9)) => {
            Some(p.vertices.clone())
        }
        EntityKind::Ellipse(el) => {
            let ge = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            if !ge.is_full() {
                return None;
            }
            let mut pts = Vec::new();
            ge.tessellate(el.major.xy().len() * 1e-3, &mut pts);
            pts.pop();
            Some(pts.into_iter().map(PolyVertex::new).collect())
        }
        EntityKind::Spline(sp) if sp.closed => Some(sp.tessellate(1e-3).into_iter().map(PolyVertex::new).collect()),
        _ => {
            let _ = d;
            None
        }
    }
}

/// Open curves that can make up an island together (a square drawn as four LINEs, two ARCs
/// forming a circle, the inner lines of multiline walls around a room). Closed objects are
/// islands on their own (see `closed_loop`).
fn island_curve(d: &Drawing, e: &Entity) -> bool {
    matches!(
        e.kind,
        EntityKind::Line(_)
            | EntityKind::Arc(_)
            | EntityKind::Ellipse(_)
            | EntityKind::LwPolyline(_)
            | EntityKind::Polyline3d(_)
            | EntityKind::Spline(_)
            | EntityKind::MLine(_)
    ) && closed_loop(d, e).is_none()
}

/// HPGAPTOL: the widest gap between curve ends that still closes a boundary (0–5000).
fn gap_tolerance(d: &Drawing) -> f64 {
    let g = d.header.f64("HPGAPTOL", 0.0);
    if g.is_finite() { g.clamp(0.0, 5000.0) } else { 0.0 }
}

/// Where a pattern starts (HPORIGINMODE): the stored origin, or a corner or the centre of the
/// hatch's extents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OriginMode {
    #[default]
    Current,
    BottomLeft,
    BottomRight,
    TopRight,
    TopLeft,
    Center,
}

impl OriginMode {
    pub(crate) fn from_index(i: i64) -> Self {
        match i {
            1 => OriginMode::BottomLeft,
            2 => OriginMode::BottomRight,
            3 => OriginMode::TopRight,
            4 => OriginMode::TopLeft,
            5 => OriginMode::Center,
            _ => OriginMode::Current,
        }
    }
    pub(crate) fn index(self) -> i64 {
        self as i64
    }
    pub(crate) fn parse(t: &str) -> Option<Self> {
        Some(match t.trim().to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "current" | "usecurrent" => OriginMode::Current,
            "bottomleft" => OriginMode::BottomLeft,
            "bottomright" => OriginMode::BottomRight,
            "topright" => OriginMode::TopRight,
            "topleft" => OriginMode::TopLeft,
            "center" | "centre" => OriginMode::Center,
            _ => return None,
        })
    }
    /// The origin for loops: `current` when the mode keeps it.
    pub(crate) fn origin(self, loops: &[HatchLoop], current: Vec2) -> Vec2 {
        let b = Bounds2::from_points(loops.iter().flat_map(|l| l.vertices.iter().map(|v| v.p)).filter(|p| p.is_finite()));
        if b.is_empty() {
            return current;
        }
        match self {
            OriginMode::Current => current,
            OriginMode::BottomLeft => b.min,
            OriginMode::BottomRight => Vec2::new(b.max.x, b.min.y),
            OriginMode::TopRight => b.max,
            OriginMode::TopLeft => Vec2::new(b.min.x, b.max.y),
            OriginMode::Center => b.center(),
        }
    }
}

/// Island detection style by name (HPISLANDDETECTION 0 normal, 1 outer, 2 ignore).
pub(crate) fn island_style(t: &str) -> Option<u8> {
    match t.trim().to_ascii_lowercase().as_str() {
        "normal" | "n" | "0" => Some(0),
        "outer" | "o" | "1" => Some(1),
        "ignore" | "i" | "2" => Some(2),
        _ => None,
    }
}

/// Boundary loops (outer + islands) around an internal point.
pub(crate) fn loops_at(s: &Session, p: Vec2) -> Result<Vec<HatchLoop>> {
    let d = s.doc()?;
    let space = s.space();
    let ext = d.extents(&space);
    let tol = ((ext.width() + ext.height()) * 1e-9).max(1e-9);
    let (polys, owners) = outlines(d, &space, None);
    // A line that ends on a flattened curve (an ellipse) can stop short of its chords.
    let polys = cadcraft_geom::region::snap_ends(&polys, outline_tol(ext), tol);
    let found = cadcraft_geom::region::boundary_at_gap(&polys, p, tol, gap_tolerance(d))
        .ok_or_else(|| EngineError::Other("Valid hatch boundary not found.".into()))?;
    let outer = found.outer;
    let ob = Bounds2::from_points(outer.iter().copied());
    let sources = handles_of(&owners, &found.outer_sources);
    let mut loops = vec![HatchLoop { vertices: outer.iter().map(|q| PolyVertex::new(*q)).collect(), outer: true, sources }];
    // Islands: closed objects entirely inside the outer loop that don't contain the pick point.
    if let Some(store) = d.space(&space) {
        for e in store.iter() {
            if !d.is_visible(e) {
                continue;
            }
            let eb = entity_bounds(d, e, 0);
            if !ob.contains_box(&eb) {
                continue;
            }
            if let Some(vs) = closed_loop(d, e) {
                let pts = Polyline { vertices: vs.clone(), closed: true }.tessellate(tol.max(eb.width() * 1e-3));
                let inside = pts.iter().all(|q| point_in_polygon(&outer, *q) || outer.iter().any(|o| o.near(*q, tol * 10.0)));
                let same_as_outer = (cadcraft_geom::shoelace(&pts).abs() - cadcraft_geom::shoelace(&outer).abs()).abs() < tol.max(1e-6) * 1000.0;
                if inside && !same_as_outer && !point_in_polygon(&pts, p) {
                    loops.push(HatchLoop { vertices: vs, outer: false, sources: vec![e.handle] });
                }
            }
        }
    }
    // Islands made of separate open curves (four LINEs around a square). Groups that include a
    // closed object were handled above, object by object.
    for island in found.islands {
        let open_curves = !island.sources.is_empty()
            && island.sources.iter().all(|&i| owners.get(i).and_then(|h| d.entity(*h)).is_some_and(|e| island_curve(d, e)));
        if open_curves {
            let sources = handles_of(&owners, &island.sources);
            loops.push(HatchLoop { vertices: island.outline.into_iter().map(PolyVertex::new).collect(), outer: false, sources });
        }
    }
    Ok(loops)
}

pub(crate) fn loops_from_handles(s: &Session, hs: &[Handle]) -> Result<Vec<HatchLoop>> {
    let d = s.doc()?;
    let mut loops = Vec::new();
    for h in hs {
        if let Some(vs) = d.entity(*h).and_then(|e| closed_loop(d, e)) {
            loops.push(HatchLoop { vertices: vs, outer: loops.is_empty(), sources: vec![*h] });
        }
    }
    if loops.is_empty() {
        return Err(EngineError::Other("No closed boundary objects selected.".into()));
    }
    Ok(loops)
}

/// Gradient fills by name: the shapes the renderer draws (`render/src/hatch.rs`); the INV forms
/// swap the two colours.
pub(crate) const GRADIENT_NAMES: [&str; 10] =
    ["LINEAR", "CYLINDER", "CURVED", "SPHERICAL", "HEMISPHERICAL", "INVLINEAR", "INVCYLINDER", "INVCURVED", "INVSPHERICAL", "INVHEMISPHERICAL"];

/// A gradient name as stored (upper case), or None when it isn't one of [`GRADIENT_NAMES`].
pub(crate) fn gradient_name(t: &str) -> Option<&'static str> {
    let up = t.trim().to_ascii_uppercase();
    GRADIENT_NAMES.iter().copied().find(|n| *n == up)
}

#[derive(Clone)]
struct HatchSettings {
    pattern: String,
    scale: f64,
    angle: f64,
    gradient: Option<Gradient>,
    associative: bool,
    /// HPSEPARATE: one hatch per picked point or selected object.
    separate: bool,
    origin: Vec2,
    origin_mode: OriginMode,
    style: u8,
}

impl HatchSettings {
    fn from(s: &Session, p: &Value, gradient: bool) -> Self {
        let hdr = s.doc().map(|d| (d.header.str("HPNAME", "ANSI31"), d.header.f64("HPSCALE", 1.0), d.header.f64("HPANG", 0.0))).unwrap_or((
            "ANSI31".into(),
            1.0,
            0.0,
        ));
        let pattern = str_param(p, "pattern").map(str::to_ascii_uppercase).unwrap_or(hdr.0);
        let g = gradient.then(|| Gradient {
            name: str_param(p, "name").and_then(gradient_name).unwrap_or("LINEAR").to_string(),
            color1: str_param(p, "color1").and_then(Color::parse).unwrap_or(Color::Index(5)),
            color2: str_param(p, "color2").and_then(Color::parse).unwrap_or(Color::Index(7)),
            angle: f64_or(p, "angle", 0.0).to_radians(),
            centered: bool_or(p, "centered", true),
        });
        HatchSettings {
            pattern,
            scale: f64_or(p, "scale", hdr.1).max(1e-9),
            angle: p.get("angle").and_then(Value::as_f64).map(f64::to_radians).unwrap_or(hdr.2),
            gradient: g,
            associative: bool_or(p, "associative", s.doc().map_or(true, |d| d.header.i64("HPASSOC", 1) != 0)),
            separate: bool_or(p, "separate", s.doc().is_ok_and(|d| d.header.i64("HPSEPARATE", 0) != 0)),
            origin: point_param(p, "origin").filter(|o| o.is_finite()).unwrap_or_else(|| {
                s.doc().ok().and_then(|d| d.header.point("HPORIGIN")).map(|o| o.xy()).filter(|o| o.is_finite()).unwrap_or(Vec2::ZERO)
            }),
            origin_mode: if point_param(p, "origin").is_some() {
                OriginMode::Current
            } else {
                str_param(p, "originMode")
                    .and_then(OriginMode::parse)
                    .unwrap_or_else(|| OriginMode::from_index(s.doc().map_or(0, |d| d.header.i64("HPORIGINMODE", 0))))
            },
            style: str_param(p, "style")
                .and_then(island_style)
                .unwrap_or_else(|| s.doc().map_or(0, |d| d.header.i64("HPISLANDDETECTION", 0).clamp(0, 2) as u8)),
        }
    }
    fn hatch(&self, mut loops: Vec<HatchLoop>) -> EntityKind {
        if !self.associative {
            loops.iter_mut().for_each(|l| l.sources.clear());
        }
        let solid = self.gradient.is_some() || self.pattern == "SOLID";
        EntityKind::Hatch(Hatch {
            pattern: if self.gradient.is_some() { "SOLID".into() } else { self.pattern.clone() },
            solid,
            origin: self.origin_mode.origin(&loops, self.origin),
            loops,
            scale: self.scale,
            angle: self.angle,
            associative: self.associative,
            style: self.style,
            elevation: 0.0,
            gradient: self.gradient.clone(),
            background: None,
            pattern_lines: patfile::lines_for(&self.pattern),
        })
    }
}

fn make(s: &mut Session, p: &Value, gradient: bool) -> Result<Value> {
    // A zero or negative pattern scale would be clamped to a near-zero spacing.
    size_param(if gradient { "gradient" } else { "hatch" }, p, "scale", false)?;
    let cmd = if gradient { "gradient" } else { "hatch" };
    if str_param(p, "style").is_some_and(|t| island_style(t).is_none()) {
        return Err(bad(cmd, "`style` is \"normal\", \"outer\" or \"ignore\""));
    }
    if str_param(p, "originMode").is_some_and(|t| OriginMode::parse(t).is_none()) {
        return Err(bad(cmd, "unknown `originMode`"));
    }
    if gradient && str_param(p, "name").is_some_and(|t| gradient_name(t).is_none()) {
        return Err(bad(cmd, format!("unknown gradient `name`; use one of {}", GRADIENT_NAMES.join(", "))));
    }
    let st = HatchSettings::from(s, p, gradient);
    // Loops by picked point or selected object: one hatch each when separate.
    let mut groups: Vec<Vec<HatchLoop>> = Vec::new();
    // Internal points and boundary objects can come together (the dialog's Pick points and
    // Select objects); points alone don't fall back to the selection.
    let pts = points_param(p, "points");
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    let objects = pts.is_none() || given("handles") || given("handle");
    for q in pts.into_iter().flatten() {
        groups.push(loops_at(s, q)?);
    }
    if objects {
        let hs = targets(s, p)?;
        if st.separate {
            groups.extend(loops_from_handles(s, &hs)?.into_iter().map(|l| vec![HatchLoop { outer: true, ..l }]));
        } else {
            groups.push(loops_from_handles(s, &hs)?);
        }
    }
    if groups.iter().all(Vec::is_empty) {
        return Err(bad("hatch", "give internal `points` or boundary `handles`"));
    }
    let hs = add_hatches(s, &st, groups)?;
    let hex: Vec<String> = hs.iter().map(|h| h.hex()).collect();
    Ok(json!({ "handle": hex.first(), "handles": hex }))
}

/// True when two loops have the same vertex sequence (ignoring cyclic shift and winding order).
fn same_loop(a: &HatchLoop, b: &HatchLoop) -> bool {
    if a.vertices.len() != b.vertices.len() {
        return false;
    }
    let n = a.vertices.len();
    if n == 0 {
        return true;
    }
    let near = |v1: &PolyVertex, v2: &PolyVertex| v1.p.near(v2.p, 1e-6) && (v1.bulge - v2.bulge).abs() < 1e-6;
    (0..n).any(|k| (0..n).all(|i| near(&a.vertices[i], &b.vertices[(i + k) % n])))
        || (0..n).any(|k| {
            (0..n).all(|i| {
                let j = (k + n - (i % n)) % n;
                let prev_j = (j + n - 1) % n;
                a.vertices[i].p.near(b.vertices[j].p, 1e-6) && (a.vertices[i].bulge + b.vertices[prev_j].bulge).abs() < 1e-6
            })
        })
}

/// Deduplicate boundary loops that describe the same boundary.
fn dedup_loops(loops: &mut Vec<HatchLoop>) {
    let mut unique: Vec<HatchLoop> = Vec::with_capacity(loops.len());
    for l in loops.drain(..) {
        if let Some(existing) = unique.iter_mut().find(|u| same_loop(u, &l)) {
            for s in l.sources {
                if !existing.sources.contains(&s) {
                    existing.sources.push(s);
                }
            }
            existing.outer = existing.outer || l.outer;
        } else {
            unique.push(l);
        }
    }
    *loops = unique;
}

/// Add the hatches for groups of loops: one per group when separate, otherwise one for all.
fn add_hatches(s: &mut Session, st: &HatchSettings, groups: Vec<Vec<HatchLoop>>) -> Result<Vec<Handle>> {
    let groups: Vec<Vec<HatchLoop>> = if st.separate {
        let mut unique_groups: Vec<Vec<HatchLoop>> = Vec::new();
        for mut g in groups {
            dedup_loops(&mut g);
            if g.is_empty() {
                continue;
            }
            if !unique_groups.iter().any(|u| u.len() == g.len() && u.iter().zip(&g).all(|(a, b)| same_loop(a, b))) {
                unique_groups.push(g);
            }
        }
        unique_groups
    } else {
        let mut all: Vec<HatchLoop> = groups.into_iter().flatten().collect();
        dedup_loops(&mut all);
        vec![all]
    };
    let mut out = Vec::new();
    for loops in groups.into_iter().filter(|g| !g.is_empty()) {
        let h = s.add_entity(st.hatch(loops))?;
        // Hatches go under other objects (HPDRAWORDER = behind boundary).
        let space = s.space();
        if let Some(store) = s.doc_mut()?.space_mut(&space) {
            store.send_to_back(h);
        }
        out.push(h);
    }
    Ok(out)
}

fn run_hatch(s: &mut Session, p: &Value) -> Result<Value> {
    patfile::load_param(p)?;
    // Like the prompt: a name without a definition would be drawn as ANSI31.
    if let Some(up) = str_param(p, "pattern").map(str::to_ascii_uppercase)
        && cadcraft_doc::library::pattern(&up).is_none()
        && patfile::user_pattern(&up).is_none()
    {
        return Err(bad("hatch", format!("unknown pattern {up}: not built in; load it with patternFile or patternText (a .pat file)")));
    }
    make(s, p, false)
}
fn run_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, true)
}

fn boundary_at(s: &mut Session, q: Vec2) -> Result<Vec<Handle>> {
    let loops = loops_at(s, q)?;
    let mut out = Vec::new();
    for l in loops {
        out.push(s.add_entity(lwpoly(l.vertices, true))?);
    }
    Ok(out)
}

/// Most internal points one BOUNDARY call accepts (checked before the list is copied).
const MAX_BOUNDARY_POINTS: usize = 256;
/// Most polylines one BOUNDARY call may create. Each pick adds polylines the next pick traces
/// against, so this bounds the aggregate tracing work of the batch.
const MAX_BOUNDARY_OUTPUT: usize = 1024;

fn run_boundary(s: &mut Session, p: &Value) -> Result<Value> {
    let raw = p.get("points").and_then(Value::as_array).ok_or_else(|| bad("boundary", "`points` is required"))?;
    if raw.len() > MAX_BOUNDARY_POINTS {
        return Err(bad("boundary", format!("at most {MAX_BOUNDARY_POINTS} points per call (got {})", raw.len())));
    }
    let pts = points_param(p, "points").ok_or_else(|| bad("boundary", "`points` is required"))?;
    let before = s.state()?.doc.clone();
    let mut out = Vec::new();
    for q in pts {
        out.extend(boundary_at(s, q)?.into_iter().map(|h| h.hex()));
        if out.len() > MAX_BOUNDARY_OUTPUT {
            // The refused call leaves no partial result behind.
            s.state_mut()?.doc = before;
            return Err(bad("boundary", format!("more than {MAX_BOUNDARY_OUTPUT} boundary polylines in one call; split the work")));
        }
    }
    Ok(json!({ "handles": out, "message": format!("BOUNDARY created {} polyline(s)", out.len()) }))
}

/// REGION: each closed object becomes a region bounded by it (and replaces it).
fn run_region(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let mut n = 0;
    for h in hs {
        let vs = {
            let d = s.doc()?;
            d.entity(h).and_then(|e| closed_loop(d, e).map(|v| (v, e.common.clone())))
        };
        if let Some((vs, common)) = vs {
            let d = s.doc_mut()?;
            d.modify_entity(h, |e| {
                e.kind = EntityKind::Region(cadcraft_doc::Region::from_loop(vs));
                e.common = common;
            })?;
            n += 1;
        }
    }
    Ok(json!({ "message": format!("{n} Region(s) created.") }))
}

/// The origin sub-prompt's keywords (a point sets a new origin).
const ORIGIN_KW: [&str; 6] = ["Bottom left", "bottom Right", "Top left", "top riGht", "Center", "Use current"];

/// What a HATCH sub-prompt asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HatchAsk {
    Pattern,
    Scale,
    Angle,
    Origin,
}

struct HatchM {
    gradient: bool,
    settings: HatchSettings,
    /// Loops by pick (one hatch each when separate).
    loops: Vec<Vec<HatchLoop>>,
    asking: Option<HatchAsk>,
    /// "Select objects" mode instead of "Pick internal point"; the next HATCH starts in it again.
    selecting: bool,
}

impl HatchM {
    fn new(s: &Session, gradient: bool) -> Self {
        HatchM {
            gradient,
            settings: HatchSettings::from(s, &Value::Null, gradient),
            loops: Vec::new(),
            asking: None,
            selecting: s.last_used.hatch_select,
        }
    }
    fn set_mode(&mut self, s: &mut Session, selecting: bool) {
        self.selecting = selecting;
        s.last_used.hatch_select = selecting;
    }
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        add_hatches(s, &self.settings, std::mem::take(&mut self.loops))?;
        Ok(Step::Done)
    }
    fn set_origin(&mut self, s: &mut Session, origin: Option<Vec2>, mode: OriginMode) -> Result<()> {
        let h = &mut s.doc_mut()?.header;
        if let Some(o) = origin.filter(|o| o.is_finite()) {
            self.settings.origin = o;
            h.set("HPORIGIN", cadcraft_doc::HVal::Point(o.to3(0.0)));
        }
        self.settings.origin_mode = mode;
        h.set_i64("HPORIGINMODE", mode.index());
        Ok(())
    }
    fn answer(&mut self, s: &mut Session, a: HatchAsk, t: &str) -> Result<()> {
        match a {
            HatchAsk::Pattern => {
                let up = match patfile::typed_file(s, t.trim()) {
                    Some(n) if n.is_empty() => return Ok(()),
                    Some(n) => n,
                    None => t.trim().to_ascii_uppercase(),
                };
                if cadcraft_doc::library::pattern(&up).is_some() || patfile::user_pattern(&up).is_some() {
                    self.settings.pattern = up.clone();
                    s.doc_mut()?.header.set_str("HPNAME", &up);
                } else {
                    s.echo(format!("Unknown pattern {up}. Available: {}", patfile::names().join(", ")));
                }
            }
            HatchAsk::Scale => {
                if let Some(v) = super::machines::number(t).filter(|v| v.is_finite() && *v > 0.0) {
                    self.settings.scale = v;
                    s.doc_mut()?.header.set_f64("HPSCALE", v);
                }
            }
            HatchAsk::Angle => {
                // Turns the pattern from the X axis (not from ANGBASE), clockwise when ANGDIR is 1.
                if let Some(a) = s.angle_settings().rotation(t).filter(|a| a.is_finite()) {
                    self.settings.angle = a;
                    s.doc_mut()?.header.set_f64("HPANG", a);
                }
            }
            HatchAsk::Origin => {
                let mode = match t {
                    "Bottom left" => OriginMode::BottomLeft,
                    "bottom Right" => OriginMode::BottomRight,
                    "Top left" => OriginMode::TopLeft,
                    "top riGht" => OriginMode::TopRight,
                    "Center" => OriginMode::Center,
                    _ => OriginMode::Current,
                };
                self.set_origin(s, None, mode)?;
            }
        }
        Ok(())
    }
}

impl Interactive for HatchM {
    fn name(&self) -> &'static str {
        if self.gradient { "GRADIENT" } else { "HATCH" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!(
            "Current pattern: {}  Scale: {:.4}  Angle: {:.0}",
            self.settings.pattern,
            self.settings.scale,
            self.settings.angle.to_degrees()
        ));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        // The sub-prompts offer the current value: Enter keeps it.
        match self.asking {
            Some(HatchAsk::Pattern) => {
                return Prompt::new("Enter a pattern name", Accept::TEXT)
                    .default(self.settings.pattern.clone())
                    .picker(Picker::HatchPattern(self.settings.pattern.clone()));
            }
            Some(HatchAsk::Scale) => {
                return Prompt::new("Specify a scale for the hatch pattern", Accept::NUMBER).default(format!("{:.4}", self.settings.scale));
            }
            Some(HatchAsk::Angle) => {
                let (au, ap) = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
                // Offered as it would be typed: clockwise is positive when ANGDIR is 1.
                let a = if s.angle_settings().clockwise { -self.settings.angle } else { self.settings.angle };
                let d = crate::units::format_angle(a, au, ap);
                return Prompt::new("Specify an angle for the hatch pattern", Accept::NUMBER).default(d);
            }
            Some(HatchAsk::Origin) => return Prompt::new("Specify the pattern origin", Accept::POINT).kw(&ORIGIN_KW).default("Use current"),
            None => {}
        }
        // The pattern picker stays offered while boundaries are picked: a choice sets the pattern.
        let picker = |p: Prompt| if self.gradient { p } else { p.picker(Picker::HatchPattern(self.settings.pattern.clone())) };
        if self.selecting {
            return picker(Prompt::new("Select objects", Accept::SELECT).kw(&[
                "picK internal point",
                "Undo",
                "seTtings",
                "sCale",
                "Angle",
                "Origin",
                "sEparate",
            ]));
        }
        picker(Prompt::new("Pick internal point", Accept::POINT).kw(&[
            "Select objects",
            "Undo",
            "seTtings",
            "Pattern",
            "sCale",
            "Angle",
            "Origin",
            "sEparate",
        ]))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(a) = self.asking.take() {
            match &i {
                Input::Point(p) if a == HatchAsk::Origin => self.set_origin(s, Some(*p), OriginMode::Current)?,
                Input::Text(t) | Input::Keyword(t) => self.answer(s, a, t)?,
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) if !self.selecting => {
                match loops_at(s, p) {
                    Ok(l) => {
                        s.echo(format!("Analyzing the selected data... {} loop(s)", l.len()));
                        self.loops.push(l);
                    }
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Continue)
            }
            Input::Pick(hs) => {
                match loops_from_handles(s, &hs) {
                    Ok(l) => {
                        s.echo(format!("{} found", hs.len()));
                        if self.settings.separate {
                            self.loops.extend(l.into_iter().map(|l| vec![HatchLoop { outer: true, ..l }]));
                        } else {
                            self.loops.push(l);
                        }
                    }
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) => {
                match k.as_str() {
                    "Select objects" => self.set_mode(s, true),
                    "picK internal point" => self.set_mode(s, false),
                    "Undo" => self.loops.clear(),
                    "Pattern" | "seTtings" => self.asking = Some(HatchAsk::Pattern),
                    "sCale" => self.asking = Some(HatchAsk::Scale),
                    "Angle" => self.asking = Some(HatchAsk::Angle),
                    "Origin" => self.asking = Some(HatchAsk::Origin),
                    "sEparate" => {
                        self.settings.separate = !self.settings.separate;
                        s.doc_mut()?.header.set_i64("HPSEPARATE", i64::from(self.settings.separate));
                        s.echo(if self.settings.separate { "Separate hatches: on" } else { "Separate hatches: off" });
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            // A pattern chosen in the picker while boundaries are picked.
            Input::Text(t) if !self.gradient => {
                self.answer(s, HatchAsk::Pattern, &t)?;
                Ok(Step::Continue)
            }
            // Enter ends boundary picking in either mode and creates the hatch.
            Input::Enter => self.finish(s),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        if self.loops.is_empty() {
            return Vec::new();
        }
        vec![self.settings.hatch(self.loops.iter().flatten().cloned().collect())]
    }
}

struct BoundaryM {
    region: bool,
}

impl Interactive for BoundaryM {
    fn name(&self) -> &'static str {
        if self.region { "REGION" } else { "BOUNDARY" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Pick internal point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                match boundary_at(s, p) {
                    Ok(hs) => s.echo(format!("BOUNDARY created {} polyline(s)", hs.len())),
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The drawing's only hatch and its handle (hatches go to the back of the draw order, so not the last entity).
    fn only_hatch(s: &Session) -> (String, Hatch) {
        let d = s.doc().unwrap();
        d.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((e.handle.hex(), h.clone())) } else { None }).unwrap()
    }

    #[test]
    fn hatchedit_pattern_replaces_gradient() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("gradient", &json!({ "points": [[5, 5]], "color1": "1", "color2": "3" })).unwrap();
        let (h, hatch) = only_hatch(&s);
        assert!(hatch.gradient.is_some());
        // Scale-only and angle-only edits keep the gradient.
        s.execute("hatchedit", &json!({ "handles": [h.clone()], "scale": 2, "angle": 30 })).unwrap();
        assert!(only_hatch(&s).1.gradient.is_some());
        s.execute("hatchedit", &json!({ "handles": [h], "pattern": "ANSI31" })).unwrap();
        let hatch = only_hatch(&s).1;
        assert_eq!(hatch.pattern, "ANSI31");
        assert!(!hatch.solid);
        assert!(hatch.gradient.is_none());
    }

    #[test]
    fn hatchedit_solid_replaces_gradient() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("gradient", &json!({ "points": [[5, 5]], "color1": "1", "color2": "3" })).unwrap();
        let (h, before) = only_hatch(&s);
        assert!(before.gradient.is_some());
        s.execute("hatchedit", &json!({ "handles": [h.clone()], "pattern": "SOLID" })).unwrap();
        let (h2, hatch) = only_hatch(&s);
        assert_eq!(h2, h);
        assert!(hatch.solid);
        assert!(hatch.gradient.is_none(), "SOLID is a plain fill, not the gradient");
        assert_eq!(hatch.loops, before.loops);
    }

    #[test]
    fn the_pattern_picker_is_offered_and_its_choice_sets_the_pattern() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.start("hatch").unwrap();
        let picker = s.current_prompt().and_then(|p| p.picker);
        assert_eq!(picker, Some(Picker::HatchPattern(HatchSettings::from(&s, &Value::Null, false).pattern)));
        // A choice made in the picker while boundaries are picked.
        s.input(Input::Text("AR-CONC".into())).unwrap();
        assert_eq!(s.current_prompt().and_then(|p| p.picker), Some(Picker::HatchPattern("AR-CONC".into())));
        // And at the pattern prompt.
        s.input(Input::Keyword("Pattern".into())).unwrap();
        assert!(s.current_prompt().unwrap().picker.is_some());
        s.input(Input::Text("BRICK".into())).unwrap();
        s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
        s.input(Input::Enter).unwrap();
        assert_eq!(only_hatch(&s).1.pattern, "BRICK");
        // An unknown name keeps the pattern.
        s.start("hatch").unwrap();
        s.input(Input::Text("NO-SUCH".into())).unwrap();
        assert_eq!(s.current_prompt().and_then(|p| p.picker), Some(Picker::HatchPattern("BRICK".into())));
        // GRADIENT offers no pattern picker.
        s.cancel();
        s.start("gradient").unwrap();
        assert!(s.current_prompt().unwrap().picker.is_none());
    }

    #[test]
    fn unknown_pattern_error_and_catalog_name_the_pat_file_route() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let e = s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "GRAVEL" })).unwrap_err();
        assert!(e.to_string().contains("patternFile"), "{e}");
        let spec = specs().into_iter().find(|c| c.id == "hatch").unwrap();
        assert!(spec.params.contains("patternFile") && spec.params.contains("patternText"), "{}", spec.params);
    }

    #[test]
    fn json_hatch_refuses_an_undefined_pattern() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let e = s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "NOPE" })).unwrap_err();
        assert!(e.to_string().contains("unknown pattern NOPE"), "{e}");
        assert_eq!(s.doc().unwrap().model.len(), 1, "only the rectangle");
        for name in ["ansi31", "SOLID"] {
            s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": name })).unwrap();
        }
        assert_eq!(s.doc().unwrap().model.len(), 3);
    }

    #[test]
    fn pattern_choices_list_the_library_with_its_lines() {
        let all = pattern_choices();
        assert_eq!(all.first().map(|c| c.name.as_str()), Some("SOLID"));
        assert!(all[0].lines.is_empty());
        let ansi31 = all.iter().find(|c| c.name == "ANSI31").unwrap();
        assert_eq!(ansi31.lines.len(), 1);
        assert!(!ansi31.description.is_empty());
    }

    #[test]
    fn repeated_picks_and_handles_in_one_region_do_not_produce_duplicate_loops() {
        // One internal point
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap();
        let (_, h1) = only_hatch(&s);
        assert_eq!(h1.loops.len(), 1);

        // Two different points in the same rectangle
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("hatch", &json!({ "points": [[2, 2], [8, 8]], "pattern": "SOLID" })).unwrap();
        let (_, h2) = only_hatch(&s);
        assert_eq!(h2.loops.len(), 1, "two points in same region must deduplicate loops");

        // The same point twice
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("hatch", &json!({ "points": [[5, 5], [5, 5]], "pattern": "SOLID" })).unwrap();
        let (_, h3) = only_hatch(&s);
        assert_eq!(h3.loops.len(), 1, "identical points must deduplicate loops");

        // Three points in the same rectangle
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("hatch", &json!({ "points": [[2, 2], [5, 5], [8, 8]], "pattern": "SOLID" })).unwrap();
        let (_, h4) = only_hatch(&s);
        assert_eq!(h4.loops.len(), 1);

        // Same boundary handle twice
        let mut s = Session::new();
        let r = s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let handle = r["handle"].as_str().unwrap();
        s.execute("hatch", &json!({ "handles": [handle, handle], "pattern": "SOLID" })).unwrap();
        let (_, h5) = only_hatch(&s);
        assert_eq!(h5.loops.len(), 1, "repeated boundary handle must deduplicate loops");

        // Two points in disjoint rectangles: both regions preserved
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("rectang", &json!({ "p1": [20, 0], "p2": [30, 10] })).unwrap();
        s.execute("hatch", &json!({ "points": [[5, 5], [25, 5]], "pattern": "SOLID" })).unwrap();
        let (_, h6) = only_hatch(&s);
        assert_eq!(h6.loops.len(), 2, "disjoint rectangles must both be kept");

        // Gradient with duplicate points in the same rectangle
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("gradient", &json!({ "points": [[2, 2], [8, 8]], "color1": "1", "color2": "3" })).unwrap();
        let (_, hg) = only_hatch(&s);
        assert_eq!(hg.loops.len(), 1, "gradient with repeated picks in same region must deduplicate loops");
    }
}
