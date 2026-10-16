//! HATCH, GRADIENT, BOUNDARY, REGION: boundaries from pick points or selected closed objects.

mod patfile;
pub use patfile::{PatternChoice, pattern_choices};
pub(super) use patfile::{lines_for, run_patterns};

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, EntityKind, Gradient, Handle, Hatch, HatchLoop, Space, entity_bounds};
use cadcraft_geom::{Bounds2, PolyVertex, Polyline, Vec2, point_in_polygon};
use serde_json::{Value, json};

use super::helpers::lwpoly;
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Picker, Prompt, Result, Session, Step};

mod assoc;
mod edit;
mod pick;
pub(crate) use assoc::update as refill_associative;
pub use pick::{HatchPicks, PickMode};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("hatch", "Hatch...", run_hatch).menu(&["Draw", "Hatch..."]).alias(&["h", "bh", "-hatch"]).params("{points?: [[x,y]] (internal points) and/or handles?: [hex] (closed objects), pattern?: \"ANSI31\", scale?, angle? (degrees), associative? (HPASSOC), separate? (HPSEPARATE: a hatch per point/object), origin?: [x,y] (HPORIGIN) | originMode?: \"current\"|\"bottomLeft\"|\"bottomRight\"|\"topLeft\"|\"topRight\"|\"center\" (HPORIGINMODE), style?: \"normal\"|\"outer\"|\"ignore\" (HPISLANDDETECTION)} — gaps up to HPGAPTOL close").interactive(|s| Ok(Box::new(HatchM::new(s, false)))),
        CommandSpec::new("gradient", "Gradient...", run_gradient).menu(&["Draw", "Gradient..."]).alias(&["gd"]).params("{points? and/or handles?, name?: \"LINEAR\"|\"CYLINDER\"|\"CURVED\"|\"SPHERICAL\"|\"HEMISPHERICAL\" or INV… (colours swapped), color1?, color2?, angle? (degrees), centered? (default true)}").interactive(|s| Ok(Box::new(HatchM::new(s, true)))),
        CommandSpec::new("boundary", "Boundary...", run_boundary).menu(&["Draw", "Boundary..."]).alias(&["bo", "bpoly"]).params("{points: [[x,y]]}").interactive(|_| Ok(Box::new(BoundaryM { region: false }))),
        CommandSpec::new("region", "Region", run_region).menu(&["Draw", "Region"]).alias(&["reg"]).params("{handles?}"),
        CommandSpec::new("hatchedit", "Hatch Edit", edit::run_hatchedit).menu(&["Modify", "Object", "Hatch..."]).alias(&["he", "-hatchedit"]).params("{handles?, pattern?, scale?, angle?, gradient?: {name?, color1?, color2?, angle? (degrees), centered?} (makes or changes a gradient fill; `pattern` turns it back), associative?: bool (false unlinks; true links each loop to a closed object of the same shape, or to `boundary` objects), boundary?: [hex], addPoints?: [[x,y]], addBoundary?: [hex], removeLoops?: [zero-based index] (geometry edits require one hatch), style?, origin? | originMode?, separate?: true, recreate?: \"polyline\"|\"region\", associate?: bool (link to the recreated boundary)}").interactive(|s| Ok(Box::new(edit::HatchEditM::new(s)))),
        // The Hatch Editor tab's buttons (contextual tab of a selected hatch).
        CommandSpec::new("hatchedit.points", "Pick Points", edit::run_hatchedit)
            .params("{handles, addPoints: [[x,y]]}: interactively, pick internal points until Enter; each adds the boundary around it")
            .interactive(|s| Ok(Box::new(edit::editor_button(s, "hatchedit.points")))),
        CommandSpec::new("hatchedit.select", "Select Boundary Objects", edit::run_hatchedit)
            .params("{handles, addBoundary: [hex]}: interactively, select closed objects to add as boundaries")
            .interactive(|s| Ok(Box::new(edit::editor_button(s, "hatchedit.select")))),
        CommandSpec::new("hatchedit.remove", "Remove Boundaries", edit::run_hatchedit)
            .params("{handles, removeAt: [[x,y]] | removeLoops: [index]}: interactively, click the boundaries to remove")
            .interactive(|s| Ok(Box::new(edit::editor_button(s, "hatchedit.remove")))),
        CommandSpec::new("hatchedit.recreate", "Recreate Boundary", edit::run_hatchedit)
            .params("{handles, recreate: \"polyline\"|\"region\", associate?}")
            .interactive(|s| Ok(Box::new(edit::editor_button(s, "hatchedit.recreate")))),
        CommandSpec::new("hatchedit.origin", "Set Origin", edit::run_hatchedit)
            .params("{handles, origin: [x,y] | originMode, storeOrigin?: bool (HPORIGIN)}: interactively, pick the new origin")
            .interactive(|s| Ok(Box::new(edit::editor_button(s, "hatchedit.origin")))),
        CommandSpec::new("hatchedit.match", "Match Properties", edit::run_hatchedit)
            .params("{handles, inherit: hex (source hatch), inheritOrigin?: bool (HPINHERIT)}: interactively, select the source hatch")
            .interactive(|s| Ok(Box::new(edit::editor_button(s, "hatchedit.match")))),
        CommandSpec::new("hatchedit.showboundary", "Display Boundary Objects", edit::run_show_boundary)
            .params("{handles?}: selects the hatches' boundary objects too, so their grips show"),
        CommandSpec::new("hatchedit.pick", "Select Hatch", run_pick_hatch)
            .params("{handles}: selects those hatches (the Ribbon then shows the Hatch Editor); interactively, select a hatch")
            .noundo()
            .interactive(|_| Ok(Box::new(PickHatchM))),
        CommandSpec::new("hatchboundaryset", "Boundary Set", run_boundary_set)
            .params("{handles?: [hex] (the new boundary set), clear?: true (use the current viewport)}")
            .noundo()
            .interactive(|_| Ok(Box::new(BoundarySetM))),
        CommandSpec::new("hatchpick", "Hatch Pick", pick::run_hatchpick)
            .params("{points?: [[x,y]] (internal points), handles?: [hex] (closed objects)} (typed: Points, Objects, oRigin or Hatch, then the picks): the Hatch and Gradient dialog's picks, kept in the session; changes nothing")
            .enabled(has_doc)
            .noundo()
            .interactive(|_| Ok(Box::new(pick::HatchPickM::default()))),
    ]
}

/// Tessellated outlines of every visible curve in the space (hatch boundary candidates), each
/// with the handle of the object it came from.
fn outlines(d: &Drawing, space: &Space, near: Option<Bounds2>) -> (Vec<Vec<Vec2>>, Vec<Handle>) {
    let Some(store) = d.space(space) else { return (Vec::new(), Vec::new()) };
    let ext = d.extents(space);
    let tol = (ext.width() + ext.height()).max(1e-9) / 20000.0;
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
/// forming a circle). Closed objects are islands on their own (see `closed_loop`).
fn island_curve(d: &Drawing, e: &Entity) -> bool {
    matches!(
        e.kind,
        EntityKind::Line(_)
            | EntityKind::Arc(_)
            | EntityKind::Ellipse(_)
            | EntityKind::LwPolyline(_)
            | EntityKind::Polyline3d(_)
            | EntityKind::Spline(_)
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
        // No Island Detection (the hatch fills its outer boundary as Ignore does).
        "ignore" | "i" | "2" | "none" => Some(2),
        _ => None,
    }
}

/// Boundary loops (outer + islands) around an internal point.
pub(crate) fn loops_at(s: &Session, p: Vec2) -> Result<Vec<HatchLoop>> {
    let d = s.doc()?;
    let space = s.space();
    let ext = d.extents(&space);
    let tol = ((ext.width() + ext.height()) * 1e-9).max(1e-9);
    let (mut polys, mut owners) = outlines(d, &space, None);
    // The Hatch Editor's boundary set: only its objects bound the hatch.
    if let Some(set) = &s.hatch_boundary_set {
        let keep: Vec<bool> = owners.iter().map(|h| set.contains(h)).collect();
        let mut k = keep.iter();
        polys.retain(|_| k.next().copied().unwrap_or(false));
        owners.retain(|h| set.contains(h));
    }
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
    /// HPDOUBLE: a user-defined pattern (`_USER`) crossed by a second family of lines.
    double: bool,
    /// HPCOLOR, HPLAYER, HPTRANSPARENCY: the new hatch's own (`None`: the current ones).
    color: Option<Color>,
    layer: Option<String>,
    transparency: Option<cadcraft_doc::Transparency>,
    /// HPBACKGROUNDCOLOR (`None`: no background).
    background: Option<Color>,
    /// HPANNOTATIVE.
    annotative: bool,
    /// HPDRAWORDER: 0 none, 1 back, 2 front, 3 behind the boundary, 4 in front of it.
    draw_order: i64,
}

/// A Hatch Creation setting held as text (HPCOLOR, HPLAYER…): "." or empty is "use current".
fn hp_text(s: &Session, name: &str) -> Option<String> {
    let v = s.doc().ok()?.header.str(name, ".");
    let v = v.trim();
    (!v.is_empty() && v != ".").then(|| v.to_string())
}

/// A pattern loaded from a .pat file.
pub(crate) fn is_user_pattern(name: &str) -> bool {
    patfile::user_pattern(name).is_some()
}

/// The Hatch Editor gallery's gradient tiles (GR_LINEAR…) and the gradient they draw.
pub(crate) const GRADIENT_TILES: [(&str, &str); 9] = [
    ("GR_LINEAR", "LINEAR"),
    ("GR_CYLIN", "CYLINDER"),
    ("GR_INVCYL", "INVCYLINDER"),
    ("GR_SPHER", "SPHERICAL"),
    ("GR_HEMISP", "HEMISPHERICAL"),
    ("GR_CURVED", "CURVED"),
    ("GR_INVSPH", "INVSPHERICAL"),
    ("GR_INVHEM", "INVHEMISPHERICAL"),
    ("GR_INVCUR", "INVCURVED"),
];

impl HatchSettings {
    fn from(s: &Session, p: &Value, gradient: bool) -> Self {
        let hdr = s.doc().map(|d| (d.header.str("HPNAME", "ANSI31"), d.header.f64("HPSCALE", 1.0), d.header.f64("HPANG", 0.0))).unwrap_or((
            "ANSI31".into(),
            1.0,
            0.0,
        ));
        let pattern = str_param(p, "pattern").map(str::to_ascii_uppercase).unwrap_or(hdr.0);
        // A gradient tile chosen in the Hatch Creation tab (HPNAME GR_…) makes a gradient too;
        // its colours and angle are GFCLR1, GFCLR2 and GFANG.
        let tile = GRADIENT_TILES.iter().find(|(t, _)| pattern.eq_ignore_ascii_case(t)).map(|(_, g)| *g);
        let gf_color = |name: &str, default: u8| hp_text(s, name).and_then(|t| Color::parse(&t)).unwrap_or(Color::Index(default));
        let g = (gradient || tile.is_some()).then(|| Gradient {
            name: str_param(p, "name").and_then(gradient_name).or(tile).unwrap_or("LINEAR").to_string(),
            color1: str_param(p, "color1").and_then(Color::parse).unwrap_or_else(|| gf_color("GFCLR1", 5)),
            color2: str_param(p, "color2").and_then(Color::parse).unwrap_or_else(|| gf_color("GFCLR2", 7)),
            angle: p.get("angle").and_then(Value::as_f64).unwrap_or_else(|| s.doc().map_or(0.0, |d| d.header.f64("GFANG", 0.0))).to_radians(),
            centered: bool_or(p, "centered", true),
        });
        let hp_int = |name: &str, default: i64| s.doc().map_or(default, |d| d.header.i64(name, default));
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
            double: bool_or(p, "double", hp_int("HPDOUBLE", 0) != 0),
            color: hp_text(s, "HPCOLOR").and_then(|t| Color::parse(&t)),
            layer: hp_text(s, "HPLAYER").filter(|l| s.doc().is_ok_and(|d| d.layer(l).is_some())),
            transparency: hp_text(s, "HPTRANSPARENCY").and_then(|t| super::props::parse_transparency(&json!(t))),
            background: hp_text(s, "HPBACKGROUNDCOLOR").filter(|t| !t.eq_ignore_ascii_case("none")).and_then(|t| Color::parse(&t)),
            annotative: hp_int("HPANNOTATIVE", 0) != 0,
            draw_order: hp_int("HPDRAWORDER", 1).clamp(0, 4),
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
            background: if solid { None } else { self.background },
            pattern_lines: if self.pattern.eq_ignore_ascii_case(cadcraft_doc::USER_PATTERN) {
                edit::user_lines(self.double)
            } else {
                patfile::lines_for(&self.pattern)
            },
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
        // HPBOUNDRETAIN: the boundary drawn as polylines (1) or regions (2), associated.
        let retain = s.doc().map_or(0, |d| d.header.i64("HPBOUNDRETAIN", 0));
        let mut loops = loops;
        if retain == 1 || retain == 2 {
            for l in &mut loops {
                let k = if retain == 2 {
                    EntityKind::Region(cadcraft_doc::Region::from_loop(l.vertices.clone()))
                } else {
                    lwpoly(l.vertices.clone(), true)
                };
                let b = s.add_entity(k)?;
                if st.associative {
                    l.sources = vec![b];
                }
            }
        }
        let sources: Vec<Handle> = loops.iter().flat_map(|l| l.sources.iter().copied()).collect();
        let h = s.add_entity(st.hatch(loops))?;
        // The Hatch Creation tab's colour, layer, transparency and annotative choice.
        let d = s.doc_mut()?;
        let anno = d.current_anno_scale();
        d.modify_entity(h, |e| {
            if let Some(c) = st.color {
                e.common.color = c;
            }
            if let Some(l) = &st.layer {
                e.common.layer.clone_from(l);
            }
            if let Some(t) = st.transparency {
                e.common.transparency = t;
            }
            if st.annotative {
                cadcraft_doc::set_annotative(e, true, &anno);
            }
        })?;
        // HPDRAWORDER (CADCraft's default: to the back, under every other object).
        use super::draworder::{Order, reorder};
        match st.draw_order {
            0 => {}
            2 => {
                reorder(s, &[h], Order::Front, &[])?;
            }
            3 | 4 if !sources.is_empty() => {
                reorder(s, &[h], if st.draw_order == 3 { Order::Under } else { Order::Above }, &sources)?;
            }
            _ => {
                let space = s.space();
                if let Some(store) = s.doc_mut()?.space_mut(&space) {
                    store.send_to_back(h);
                }
            }
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
        return Err(bad("hatch", format!("unknown pattern {up}")));
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

/// The hatches among `hs`.
fn only_hatches(s: &Session, hs: Vec<Handle>) -> Vec<Handle> {
    s.doc().map(|d| hs.into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_)))).collect()).unwrap_or_default()
}

/// HATCHEDIT typed with no hatch selected (Ribbon): select one, and the Hatch Editor opens.
fn run_pick_hatch(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = only_hatches(s, targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("hatchedit", "select one or more hatches"));
    }
    let n = hs.len();
    s.set_selection(hs);
    Ok(json!({ "selected": n }))
}

struct PickHatchM;

impl Interactive for PickHatchM {
    fn name(&self) -> &'static str {
        "HATCHEDIT"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Select hatch object", Accept::SELECT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Pick(hs) => {
                let hs = only_hatches(s, hs);
                if hs.is_empty() {
                    s.echo("No hatch selected.");
                    return Ok(Step::Continue);
                }
                s.set_selection(hs);
                Ok(Step::Done)
            }
            Input::Enter | Input::Cancel => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

/// HATCHBOUNDARYSET: the objects pick points look at (Hatch Editor > Boundaries > Select New
/// Boundary Set), or everything again with `clear`.
fn run_boundary_set(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "clear", false) {
        s.hatch_boundary_set = None;
        return Ok(json!({ "objects": Value::Null, "message": "Pick points use the current viewport." }));
    }
    let hs = targets(s, p)?;
    let n = hs.len();
    s.hatch_boundary_set = Some(hs);
    Ok(json!({ "objects": n, "message": format!("Boundary set: {n} object(s).") }))
}

/// HATCHBOUNDARYSET typed: select the objects of the new boundary set.
struct BoundarySetM;

impl Interactive for BoundarySetM {
    fn name(&self) -> &'static str {
        "HATCHBOUNDARYSET"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Select objects for the new boundary set", Accept::SELECT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Pick(hs) if !hs.is_empty() => {
                let n = hs.len();
                s.hatch_boundary_set = Some(hs);
                s.echo(format!("Boundary set: {n} object(s)."));
                Ok(Step::Done)
            }
            Input::Enter | Input::Cancel => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

/// Hatch Creation > Match Properties: the source hatch's pattern, scale, angle, colours, layer,
/// transparency and island style become the HP* settings of the hatch being made (and its origin
/// when HPINHERIT is 1).
fn inherit_settings(s: &mut Session, hs: &[Handle]) -> Result<()> {
    let d = s.doc()?;
    let Some(e) = hs.iter().filter_map(|h| d.entity(*h)).find(|e| matches!(e.kind, EntityKind::Hatch(_))) else {
        s.echo("Select a hatch object.");
        return Ok(());
    };
    let EntityKind::Hatch(h) = &e.kind else { return Ok(()) };
    let (h, common) = (h.clone(), e.common.clone());
    let origin = d.header.i64("HPINHERIT", 0) == 1;
    let color = |c: Color| match c {
        Color::True(rgb) => format!("{},{},{}", rgb.0, rgb.1, rgb.2),
        other => other.to_aci().to_string(),
    };
    let hd = &mut s.doc_mut()?.header;
    match &h.gradient {
        Some(g) => {
            let tile = GRADIENT_TILES.iter().find(|(_, n)| n.eq_ignore_ascii_case(&g.name)).map_or("GR_LINEAR", |(t, _)| *t);
            hd.set_str("HPNAME", tile);
            hd.set_str("GFCLR1", &color(g.color1));
            hd.set_str("GFCLR2", &color(g.color2));
            hd.set_f64("GFANG", g.angle.to_degrees());
        }
        None => {
            hd.set_str("HPNAME", if h.solid { "SOLID" } else { &h.pattern });
            hd.set_i64("HPDOUBLE", i64::from(h.is_double()));
        }
    }
    hd.set_f64("HPSCALE", h.scale);
    hd.set_f64("HPANG", h.angle);
    hd.set_i64("HPISLANDDETECTION", i64::from(h.style));
    hd.set_str("HPCOLOR", &color(common.color));
    hd.set_str("HPLAYER", &common.layer);
    hd.set_str("HPTRANSPARENCY", &common.transparency.name());
    hd.set_str("HPBACKGROUNDCOLOR", &h.background.map(color).unwrap_or_default());
    if origin {
        hd.set("HPORIGIN", cadcraft_doc::HVal::Point(h.origin.to3(0.0)));
        hd.set_i64("HPORIGINMODE", 0);
    }
    s.echo(format!("Properties of the {} hatch taken.", if h.solid { "SOLID".to_string() } else { h.pattern.clone() }));
    Ok(())
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
    /// Hatch Creation > Remove: a click removes the boundary loop nearest to it.
    Remove,
    /// Hatch Creation > Match Properties: the source hatch.
    Inherit,
    /// Hatch Creation > Select New Boundary Set.
    BoundarySet,
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
    /// Remove the picked boundary loop nearest to `q` (and its pick when it has no loop left).
    fn remove_near(&mut self, q: Vec2) {
        let mut best: Option<(usize, usize, f64)> = None;
        for (g, loops) in self.loops.iter().enumerate() {
            for (k, l) in loops.iter().enumerate() {
                let pts = Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(1e-3);
                let n = pts.len();
                for j in 0..n {
                    let (Some(a), Some(b)) = (pts.get(j), pts.get((j + 1) % n)) else { continue };
                    let ab = *b - *a;
                    let t = if ab.dot(ab) > 0.0 { ((q - *a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0) } else { 0.0 };
                    let d = (*a + ab * t).dist(q);
                    if best.is_none_or(|b| d < b.2) {
                        best = Some((g, k, d));
                    }
                }
            }
        }
        if let Some((g, k, _)) = best
            && let Some(loops) = self.loops.get_mut(g)
        {
            if k < loops.len() {
                loops.remove(k);
            }
            if loops.is_empty() {
                self.loops.remove(g);
            }
        }
    }
    /// The settings now: the Hatch Creation tab changes the HP* system variables while the
    /// command runs, and the preview and the hatch follow them.
    fn live(&self, s: &Session) -> HatchSettings {
        HatchSettings::from(s, &Value::Null, self.gradient)
    }
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        self.settings = self.live(s);
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
            // Picks, not typed answers.
            HatchAsk::Remove | HatchAsk::Inherit | HatchAsk::BoundarySet => {}
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
            Some(HatchAsk::Remove) => return Prompt::new("Select boundaries to remove", Accept::POINT),
            Some(HatchAsk::Inherit) => return Prompt::new("Select hatch object", Accept::SELECT),
            Some(HatchAsk::BoundarySet) => return Prompt::new("Select objects for the new boundary set", Accept::SELECT),
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
                Input::Point(p) if a == HatchAsk::Remove => {
                    self.remove_near(*p);
                    // Removing goes on until Enter.
                    self.asking = Some(HatchAsk::Remove);
                }
                Input::Pick(hs) if a == HatchAsk::Inherit => inherit_settings(s, hs)?,
                Input::Pick(hs) if a == HatchAsk::BoundarySet && !hs.is_empty() => {
                    s.echo(format!("Boundary set: {} object(s).", hs.len()));
                    s.hatch_boundary_set = Some(hs.clone());
                }
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
                    // The Hatch Creation tab's buttons (not offered at the prompt).
                    "Remove boundaries" => self.asking = Some(HatchAsk::Remove),
                    "Inherit properties" => self.asking = Some(HatchAsk::Inherit),
                    "new boundary Set" => self.asking = Some(HatchAsk::BoundarySet),
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
    fn preview(&self, s: &Session, _c: Vec2) -> Vec<EntityKind> {
        if self.loops.is_empty() {
            return Vec::new();
        }
        vec![self.live(s).hatch(self.loops.iter().flatten().cloned().collect())]
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
