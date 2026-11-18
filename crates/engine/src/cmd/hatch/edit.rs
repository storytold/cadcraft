//! HATCHEDIT and -HATCHEDIT: pattern, scale and angle, island style, pattern origin,
//! associativity (unlink, or link to boundary objects), separate hatches and recreated
//! boundaries of existing hatches.

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, EntityKind, Gradient, Handle, Hatch, HatchLoop, entity_bounds};
use cadcraft_geom::{Bounds2, Polyline, Vec2, point_in_polygon};
use serde_json::{Value, json};

use super::assoc::{detach, same_shape};
use super::{GRADIENT_NAMES, ORIGIN_KW, OriginMode, closed_loop, dedup_loops, gradient_name, island_style, loops_at, loops_from_handles};
use crate::cmd::helpers::lwpoly;
use crate::cmd::{bad, bool_or, point_param, points_param, size_param, str_param, targets};
use crate::{Accept, Input, Interactive, Picker, Prompt, Result, Session, Step};

/// Upper bound on hatches edited by one call.
const MAX_EDITED: usize = 10_000;
/// Upper bound on loops looked up when linking hatches to boundary objects in one call (each
/// lookup scans the hatch's space), and on the loops of a hatch split into separate hatches.
const MAX_LOOPS: usize = 2000;

fn hatches(s: &Session, p: &Value) -> Result<Vec<Handle>> {
    let hs = targets(s, p)?;
    let d = s.doc()?;
    let out: Vec<Handle> = hs.into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_)))).take(MAX_EDITED).collect();
    if out.is_empty() {
        return Err(bad("hatchedit", "select one or more hatches"));
    }
    Ok(out)
}

fn outline(l: &HatchLoop) -> Vec<Vec2> {
    Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(1e-3)
}

/// Link the loops of `h` to closed objects of the same shape: the `boundary` objects when given,
/// otherwise any in the hatch's space. Returns the number of loops left without an object.
fn link(d: &Drawing, handle: Handle, h: &mut Hatch, boundary: Option<&[Handle]>, budget: &mut usize) -> usize {
    let space = d.space_of(handle);
    let store = space.as_ref().and_then(|sp| d.space(sp));
    let mut missing = 0;
    for l in &mut h.loops {
        if boundary.is_none() && !l.sources.is_empty() {
            continue;
        }
        if *budget == 0 {
            missing += 1;
            continue;
        }
        *budget -= 1;
        let lb = Bounds2::from_points(outline(l));
        let eps = 1e-4 * (1.0 + lb.width() + lb.height());
        let fits = |e: &cadcraft_doc::Entity| {
            let eb = entity_bounds(d, e, 0);
            e.handle != handle
                && eb.min.dist(lb.min) <= eps
                && eb.max.dist(lb.max) <= eps
                && closed_loop(d, e).is_some_and(|vs| same_shape(&vs, &l.vertices))
        };
        let found = match boundary {
            Some(hs) => hs.iter().filter_map(|h| d.entity(*h)).find(|e| fits(e)).map(|e| e.handle),
            None => store.and_then(|st| st.iter().find(|e| fits(e)).map(|e| e.handle)),
        };
        match found {
            Some(src) => l.sources = vec![src],
            None => missing += 1,
        }
    }
    h.associative = missing == 0 && h.loops.iter().all(|l| !l.sources.is_empty());
    if !h.associative && missing == 0 {
        missing = h.loops.len();
    }
    missing
}

/// The loops of `h` in groups that fill separate areas: each loop not inside another one
/// (counting nesting, so an island's island starts a new area) with the islands directly in it.
fn areas(h: &Hatch) -> Vec<Vec<HatchLoop>> {
    let polys: Vec<Vec<Vec2>> = h.loops.iter().map(outline).collect();
    let first: Vec<Option<Vec2>> = polys.iter().map(|p| p.first().copied()).collect();
    let area = |i: usize| polys.get(i).map_or(0.0, |p| cadcraft_geom::shoelace(p).abs());
    // The loops containing each loop, smallest first.
    let parents: Vec<Vec<usize>> = (0..polys.len())
        .map(|i| {
            let mut ps: Vec<usize> = (0..polys.len())
                .filter(|&j| j != i && first.get(i).copied().flatten().zip(polys.get(j)).is_some_and(|(q, pj)| point_in_polygon(pj, q)))
                .collect();
            ps.sort_by(|a, b| area(*a).total_cmp(&area(*b)));
            ps
        })
        .collect();
    let mut groups: Vec<(usize, Vec<HatchLoop>)> = Vec::new();
    for (i, ps) in parents.iter().enumerate() {
        if ps.len() % 2 == 0
            && let Some(l) = h.loops.get(i)
        {
            groups.push((i, vec![HatchLoop { outer: true, ..l.clone() }]));
        }
    }
    for (i, ps) in parents.iter().enumerate() {
        if ps.len() % 2 == 1
            && let (Some(l), Some(top)) = (h.loops.get(i), ps.first())
            && let Some(g) = groups.iter_mut().find(|g| g.0 == *top)
        {
            g.1.push(HatchLoop { outer: false, ..l.clone() });
        }
    }
    groups.into_iter().map(|g| g.1).collect()
}

/// Split a hatch into one hatch per separate area. Returns the new hatches.
fn separate(s: &mut Session, handle: Handle) -> Result<Vec<Handle>> {
    let (h, common, space) = {
        let d = s.doc()?;
        let Some(e) = d.entity(handle) else { return Ok(Vec::new()) };
        let EntityKind::Hatch(h) = &e.kind else { return Ok(Vec::new()) };
        (h.clone(), e.common.clone(), d.space_of(handle))
    };
    if h.loops.len() > MAX_LOOPS {
        return Err(bad("hatchedit", format!("a hatch with more than {MAX_LOOPS} loops can't be separated")));
    }
    let mut groups = areas(&h).into_iter();
    let (Some(first), Some(space)) = (groups.next(), space) else { return Ok(Vec::new()) };
    let rest: Vec<Vec<HatchLoop>> = groups.collect();
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for loops in rest {
        let mut nh = Hatch { loops, ..h.clone() };
        nh.associative = h.associative && nh.loops.iter().all(|l| !l.sources.is_empty());
        let n = d.add(&space, common.clone(), EntityKind::Hatch(nh))?;
        if let Some(st) = d.space_mut(&space) {
            st.send_to_back(n);
        }
        out.push(n);
    }
    let kept = Hatch { loops: first, ..h };
    d.modify_entity(handle, |e| e.kind = EntityKind::Hatch(kept))?;
    Ok(out)
}

/// Pattern, scale and angle (degrees) from the parameters.
fn properties(h: &mut Hatch, p: &Value) {
    if let Some(v) = str_param(p, "pattern").map(str::trim).filter(|v| !v.is_empty()) {
        h.pattern = v.to_ascii_uppercase();
        h.solid = h.pattern == "SOLID";
        // A named pattern, SOLID included, replaces the gradient fill; the renderer draws the
        // gradient first when it is set.
        h.gradient = None;
        if !h.solid {
            // A library pattern is drawn from the library; a loaded .pat pattern brings its lines;
            // any other name is the hatch's own and keeps its definition.
            if cadcraft_doc::library::pattern(&h.pattern).is_some() {
                h.pattern_lines.clear();
            } else if let Some(user) = super::patfile::user_pattern(&h.pattern) {
                h.pattern_lines = user.lines;
            }
        }
    }
    if let Some(x) = p.get("scale").and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0) {
        h.scale = x;
    }
    if let Some(x) = p.get("angle").and_then(Value::as_f64).filter(|x| x.is_finite()) {
        h.angle = x.to_radians();
    }
}

/// HATCHEDIT's `gradient`: the fields to change, all checked before any hatch is touched.
struct GradientEdit {
    name: Option<&'static str>,
    color1: Option<Color>,
    color2: Option<Color>,
    /// Radians.
    angle: Option<f64>,
    centered: Option<bool>,
}

impl GradientEdit {
    /// Make `h` a gradient fill (from the default LINEAR blue-to-white one when it has none) and
    /// set the given fields.
    fn apply(&self, h: &mut Hatch) {
        let mut g = h.gradient.clone().unwrap_or(Gradient {
            name: "LINEAR".into(),
            color1: Color::Index(5),
            color2: Color::Index(7),
            angle: 0.0,
            centered: true,
        });
        if let Some(n) = self.name {
            g.name = n.into();
        }
        if let Some(c) = self.color1 {
            g.color1 = c;
        }
        if let Some(c) = self.color2 {
            g.color2 = c;
        }
        if let Some(a) = self.angle {
            g.angle = a;
        }
        if let Some(c) = self.centered {
            g.centered = c;
        }
        h.gradient = Some(g);
        h.pattern = "SOLID".into();
        h.solid = true;
        h.pattern_lines.clear();
    }
}

fn gradient_edit(p: &Value) -> Result<Option<GradientEdit>> {
    let g = match p.get("gradient") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(g)) => g,
        Some(_) => return Err(bad("hatchedit", "`gradient` is an object {name?, color1?, color2?, angle?, centered?}")),
    };
    let field = |k: &str| g.get(k).filter(|v| !v.is_null());
    let name = match field("name") {
        None => None,
        Some(v) => Some(
            v.as_str()
                .and_then(gradient_name)
                .ok_or_else(|| bad("hatchedit", format!("unknown gradient `name`; use one of {}", GRADIENT_NAMES.join(", "))))?,
        ),
    };
    let color = |k: &str| -> Result<Option<Color>> {
        match field(k) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .and_then(Color::parse)
                .map(Some)
                .ok_or_else(|| bad("hatchedit", format!("`gradient.{k}` is a colour such as \"5\", \"red\" or \"255,128,0\""))),
        }
    };
    let angle = match field("angle") {
        None => None,
        Some(v) => Some(
            v.as_f64().map(f64::to_radians).filter(|a| a.is_finite()).ok_or_else(|| bad("hatchedit", "`gradient.angle` is a number of degrees"))?,
        ),
    };
    let centered = match field("centered") {
        None => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad("hatchedit", "`gradient.centered` is true or false"))?),
    };
    Ok(Some(GradientEdit { name, color1: color("color1")?, color2: color("color2")?, angle, centered }))
}

/// An explicit `scale` must be positive and an explicit `angle` a number, checked before anything
/// is edited so a refused request changes nothing (absent and null leave them as they are).
fn check_numbers(p: &Value) -> Result<()> {
    size_param("hatchedit", p, "scale", false)?;
    match p.get("angle") {
        None | Some(Value::Null) => Ok(()),
        Some(v) if v.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Some(_) => Err(bad("hatchedit", "`angle` must be a number of degrees")),
    }
}

/// Boundary changes are prepared before any hatch is edited, so a bad pick cannot leave a
/// partially changed drawing. Loop numbers refer to the hatch before this request.
fn boundary_changes(s: &Session, p: &Value, hs: &[Handle]) -> Result<(Vec<HatchLoop>, Vec<usize>)> {
    let given = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    if !["addPoints", "addBoundary", "removeLoops"].into_iter().any(given) {
        return Ok((Vec::new(), Vec::new()));
    }
    if hs.len() != 1 {
        return Err(bad("hatchedit", "edit the boundaries of one hatch at a time"));
    }
    let mut added = Vec::new();
    if given("addPoints") {
        if s.doc()?.space_of(hs[0]) != Some(s.space()) {
            return Err(bad("hatchedit", "open the hatch's space before adding points"));
        }
        let points = points_param(p, "addPoints").ok_or_else(|| bad("hatchedit", "`addPoints` is an array of finite [x, y] points"))?;
        if points.len() > MAX_LOOPS {
            return Err(bad("hatchedit", format!("at most {MAX_LOOPS} points")));
        }
        for q in points {
            if !q.is_finite() {
                return Err(bad("hatchedit", "`addPoints` must contain finite points"));
            }
            added.extend(loops_at(s, q)?);
            if added.len() > MAX_LOOPS {
                return Err(bad("hatchedit", format!("at most {MAX_LOOPS} added loops")));
            }
        }
    }
    if given("addBoundary") {
        let raw =
            p.get("addBoundary").and_then(Value::as_array).ok_or_else(|| bad("hatchedit", "`addBoundary` is an array of closed object handles"))?;
        if raw.len() > MAX_LOOPS {
            return Err(bad("hatchedit", format!("at most {MAX_LOOPS} boundary objects")));
        }
        let d = s.doc()?;
        let space = d.space_of(hs[0]);
        let mut handles = Vec::with_capacity(raw.len());
        for value in raw {
            let h = value.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad("hatchedit", "invalid `addBoundary` handle"))?;
            let e = d.entity(h).ok_or_else(|| bad("hatchedit", format!("boundary object {} not found", h.hex())))?;
            if d.space_of(h) != space || closed_loop(d, e).is_none() {
                return Err(bad("hatchedit", format!("boundary object {} must be closed and in the hatch's space", h.hex())));
            }
            handles.push(h);
        }
        if !handles.is_empty() {
            added.extend(loops_from_handles(s, &handles)?);
        }
    }
    let mut removed = Vec::new();
    if given("removeLoops") {
        let raw =
            p.get("removeLoops").and_then(Value::as_array).ok_or_else(|| bad("hatchedit", "`removeLoops` is an array of zero-based loop numbers"))?;
        if raw.len() > MAX_LOOPS {
            return Err(bad("hatchedit", format!("at most {MAX_LOOPS} loop numbers")));
        }
        let count = hs
            .first()
            .and_then(|h| s.doc().ok()?.entity(*h))
            .and_then(|e| match &e.kind {
                EntityKind::Hatch(h) => Some(h.loops.len()),
                _ => None,
            })
            .unwrap_or(0);
        for value in raw {
            let index = value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < count)
                .ok_or_else(|| bad("hatchedit", "`removeLoops` contains an invalid loop number"))?;
            if !removed.contains(&index) {
                removed.push(index);
            }
        }
    }
    Ok((added, removed))
}

fn edit_loops(h: &mut Hatch, added: &[HatchLoop], removed: &[usize]) -> Result<()> {
    if added.is_empty() && removed.is_empty() {
        return Ok(());
    }
    h.loops = h.loops.drain(..).enumerate().filter_map(|(i, l)| (!removed.contains(&i)).then_some(l)).collect();
    h.loops.extend_from_slice(added);
    dedup_loops(&mut h.loops);
    if h.loops.is_empty() {
        return Err(bad("hatchedit", "a hatch needs at least one boundary loop"));
    }
    if h.loops.len() > MAX_LOOPS {
        return Err(bad("hatchedit", format!("a hatch may have at most {MAX_LOOPS} loops")));
    }
    let outlines: Vec<Vec<Vec2>> = h.loops.iter().map(outline).collect();
    for (i, l) in h.loops.iter_mut().enumerate() {
        let p = outlines.get(i).and_then(|v| v.first()).copied();
        let depth = p.map_or(0, |q| outlines.iter().enumerate().filter(|(j, other)| *j != i && point_in_polygon(other, q)).count());
        l.outer = depth % 2 == 0;
    }
    Ok(())
}

pub(super) fn run_hatchedit(s: &mut Session, p: &Value) -> Result<Value> {
    check_numbers(p)?;
    let hs = hatches(s, p)?;
    let (added, removed) = boundary_changes(s, p, &hs)?;
    let style = match str_param(p, "style") {
        Some(t) => Some(island_style(t).ok_or_else(|| bad("hatchedit", "`style` is \"normal\", \"outer\" or \"ignore\""))?),
        None => None,
    };
    let origin = point_param(p, "origin").filter(|o| o.is_finite());
    let mode = match str_param(p, "originMode") {
        Some(t) => Some(OriginMode::parse(t).ok_or_else(|| bad("hatchedit", "unknown `originMode`"))?),
        None => None,
    };
    let recreate = match p.get("recreate") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(Value::String(t)) if t.eq_ignore_ascii_case("polyline") => true,
        Some(Value::String(t)) if t.eq_ignore_ascii_case("region") => {
            return Err(bad("hatchedit", "region boundaries are not available yet; use \"polyline\""));
        }
        Some(_) => return Err(bad("hatchedit", "`recreate` is \"polyline\"")),
    };
    let grad = gradient_edit(p)?;
    if grad.is_some() && str_param(p, "pattern").map(|v| v.trim().to_ascii_uppercase()).is_some_and(|v| !v.is_empty() && v != "SOLID") {
        return Err(bad("hatchedit", "give `pattern` or `gradient`, not both"));
    }
    // Like the prompt: a pattern must be one CADCraft defines, SOLID, or the hatches' own (a
    // pattern read from a file keeps its definition).
    if let Some(up) = str_param(p, "pattern").map(|v| v.trim().to_ascii_uppercase()).filter(|v| !v.is_empty())
        && up != "SOLID"
        && cadcraft_doc::library::pattern(&up).is_none()
        && super::patfile::user_pattern(&up).is_none()
    {
        let d = s.doc()?;
        let own = hs.iter().all(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(x)) if x.pattern == up));
        if !own {
            return Err(bad("hatchedit", format!("unknown pattern {up}")));
        }
    }
    let boundary: Option<Vec<Handle>> = match p.get("boundary") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).take(MAX_EDITED).collect()),
        Some(_) => return Err(bad("hatchedit", "`boundary` is an array of object handles")),
    };
    let associate = bool_or(p, "associate", false);
    let mut created = Vec::new();
    let mut notes = Vec::new();
    let mut budget = MAX_LOOPS;
    for handle in &hs {
        let Some(EntityKind::Hatch(mut h)) = s.doc()?.entity(*handle).map(|e| e.kind.clone()) else { continue };
        edit_loops(&mut h, &added, &removed)?;
        if h.associative && h.loops.iter().any(|l| l.sources.is_empty()) && p.get("associative").and_then(Value::as_bool) != Some(false) {
            let missing = link(s.doc()?, *handle, &mut h, None, &mut budget);
            if missing != 0 {
                detach(&mut h);
                notes.push(format!("Hatch {}: {missing} new boundary loop(s) could not be linked; it is now non-associative.", handle.hex()));
            }
        }
        properties(&mut h, p);
        if let Some(g) = &grad {
            g.apply(&mut h);
        }
        if let Some(st) = style {
            h.style = st;
        }
        if origin.is_some() || mode.is_some() {
            h.origin = mode.unwrap_or_default().origin(&h.loops, origin.unwrap_or(h.origin));
        }
        match p.get("associative").and_then(Value::as_bool) {
            Some(false) => detach(&mut h),
            Some(true) => {
                // All loops or none: a hatch with loops left over keeps what it had.
                let mut linked = h.clone();
                let missing = link(s.doc()?, *handle, &mut linked, boundary.as_deref(), &mut budget);
                if missing == 0 {
                    h = linked;
                } else {
                    notes.push(format!("Hatch {}: no boundary object found for {missing} loop(s); it stays non-associative.", handle.hex()));
                }
            }
            None => {}
        }
        if recreate {
            let mut new = Vec::new();
            for l in &h.loops {
                new.push(s.add_entity(lwpoly(l.vertices.clone(), true))?);
            }
            if associate {
                for (l, n) in h.loops.iter_mut().zip(&new) {
                    l.sources = vec![*n];
                }
                h.associative = true;
            }
            created.extend(new);
        }
        s.doc_mut()?.modify_entity(*handle, |e| e.kind = EntityKind::Hatch(h))?;
        if bool_or(p, "separate", false) {
            created.extend(separate(s, *handle)?);
        }
    }
    for n in &notes {
        s.echo(n.clone());
    }
    Ok(json!({ "changed": hs.len(), "handles": created.iter().map(|h| h.hex()).collect::<Vec<_>>(), "messages": notes }))
}

/// What a -HATCHEDIT prompt asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Option,
    AddPoint,
    AddObject,
    Remove,
    Style,
    Pattern,
    Scale,
    Angle,
    Origin,
    Recreate,
    RecreateLink,
    Link,
}

const OPTIONS: [&str; 10] = [
    "Disassociate",
    "Style",
    "Properties",
    "recreate Boundary",
    "ASsociate",
    "separate Hatches",
    "Origin",
    "add Point",
    "add Object",
    "remove Boundary",
];

/// -HATCHEDIT: select hatches, then one option.
pub(crate) struct HatchEditM {
    hatches: Vec<Handle>,
    ask: Ask,
    /// Properties typed so far (pattern, scale, angle in degrees).
    props: serde_json::Map<String, Value>,
}

impl HatchEditM {
    pub(crate) fn new(s: &Session) -> Self {
        let hatches = s
            .doc()
            .map(|d| s.selection().into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_)))).collect())
            .unwrap_or_default();
        HatchEditM { hatches, ask: Ask::Option, props: serde_json::Map::new() }
    }
    fn first(&self, s: &Session) -> Option<Hatch> {
        let d = s.doc().ok()?;
        self.hatches.iter().find_map(|h| match &d.entity(*h)?.kind {
            EntityKind::Hatch(x) => Some(x.clone()),
            _ => None,
        })
    }
    fn apply(&self, s: &mut Session, mut p: Value) -> Result<Step> {
        if let Some(o) = p.as_object_mut() {
            o.insert("handles".into(), json!(self.hatches.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        }
        match run_hatchedit(s, &p) {
            Ok(v) => {
                if let Some(n) = v.get("handles").and_then(Value::as_array).map(Vec::len).filter(|n| *n > 0) {
                    s.echo(format!("{n} object(s) created."));
                }
            }
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }
}

impl Interactive for HatchEditM {
    fn name(&self) -> &'static str {
        "HATCHEDIT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.hatches.is_empty() {
            return Prompt::new("Select hatch object", Accept::SELECT);
        }
        let h = self.first(s);
        match self.ask {
            Ask::Option => Prompt::new("Enter a hatch option", Accept::TEXT).kw(&OPTIONS),
            Ask::AddPoint => Prompt::new("Pick an internal point for the new boundary", Accept::POINT),
            Ask::AddObject => Prompt::new("Select closed boundary objects to add", Accept::SELECT),
            Ask::Remove => Prompt::new("Enter a boundary loop number to remove (starting at 1)", Accept::TEXT),
            Ask::Style => {
                let cur = match h.map_or(0, |h| h.style) {
                    1 => "Outer",
                    2 => "Ignore",
                    _ => "Normal",
                };
                Prompt::new("Island detection style", Accept::TEXT).kw(&["Ignore", "Outer", "Normal"]).default(cur)
            }
            Ask::Pattern => {
                let current = h.map(|h| h.pattern).unwrap_or_default();
                Prompt::new("Enter a pattern name", Accept::TEXT).default(current.clone()).picker(Picker::HatchPattern(current))
            }
            Ask::Scale => Prompt::new("Specify a scale for the hatch pattern", Accept::NUMBER).default(format!("{:.4}", h.map_or(1.0, |h| h.scale))),
            Ask::Angle => Prompt::new("Specify an angle for the hatch pattern", Accept::NUMBER)
                .default(format!("{:.0}", h.map_or(0.0, |h| h.angle.to_degrees()))),
            Ask::Origin => Prompt::new("Specify the pattern origin", Accept::POINT).kw(&ORIGIN_KW).default("Use current"),
            // Region boundaries come with REGION objects; polylines until then.
            Ask::Recreate => Prompt::new("Type of boundary object", Accept::TEXT).kw(&["Polyline"]).default("Polyline"),
            Ask::RecreateLink => Prompt::new("Associate the hatch with the new boundary?", Accept::TEXT).kw(&["Yes", "No"]).default("No"),
            Ask::Link => Prompt::new("Select boundary objects (Enter finds them)", Accept::SELECT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.hatches.is_empty() {
            return Ok(match i {
                Input::Pick(hs) => {
                    let d = s.doc()?;
                    self.hatches = hs.into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_)))).collect();
                    if self.hatches.is_empty() {
                        s.echo("No hatch selected.");
                    }
                    Step::Continue
                }
                Input::Enter | Input::Cancel => Step::Done,
                _ => Step::Continue,
            });
        }
        let text = match &i {
            Input::Keyword(t) | Input::Text(t) => Some(t.trim().to_string()),
            _ => None,
        };
        match (self.ask, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (Ask::Option, Input::Enter) => Ok(Step::Done),
            (Ask::Option, _) => {
                match text.as_deref() {
                    Some("Disassociate") => return self.apply(s, json!({ "associative": false })),
                    Some("separate Hatches") => return self.apply(s, json!({ "separate": true })),
                    Some("Style") => self.ask = Ask::Style,
                    Some("Properties") => self.ask = Ask::Pattern,
                    Some("recreate Boundary") => self.ask = Ask::Recreate,
                    Some("ASsociate") => self.ask = Ask::Link,
                    Some("Origin") => self.ask = Ask::Origin,
                    Some("add Point") => self.ask = Ask::AddPoint,
                    Some("add Object") => self.ask = Ask::AddObject,
                    Some("remove Boundary") => self.ask = Ask::Remove,
                    _ => s.echo("Choose one of the options."),
                }
                Ok(Step::Continue)
            }
            (Ask::AddPoint, Input::Point(p)) => self.apply(s, json!({ "addPoints": [[p.x, p.y]] })),
            (Ask::AddObject, Input::Pick(hs)) => self.apply(s, json!({ "addBoundary": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() })),
            (Ask::AddPoint | Ask::AddObject, Input::Enter) => Ok(Step::Done),
            (Ask::AddPoint | Ask::AddObject, _) => Ok(Step::Continue),
            (Ask::Remove, _) => {
                let Some(index) = text.as_deref().and_then(|t| t.parse::<usize>().ok()).and_then(|n| n.checked_sub(1)) else {
                    s.echo("Enter a positive boundary loop number.");
                    return Ok(Step::Continue);
                };
                self.apply(s, json!({ "removeLoops": [index] }))
            }
            (Ask::Style, Input::Enter) => Ok(Step::Done),
            (Ask::Style, _) => match text.as_deref().and_then(island_style) {
                Some(st) => {
                    let name = ["normal", "outer", "ignore"].get(usize::from(st)).copied().unwrap_or("normal");
                    self.apply(s, json!({ "style": name }))
                }
                None => Ok(Step::Continue),
            },
            (Ask::Pattern, _) => {
                if let Some(t) = text.filter(|t| !t.is_empty()) {
                    let up = t.to_ascii_uppercase();
                    if cadcraft_doc::library::pattern(&up).is_none() && super::patfile::user_pattern(&up).is_none() {
                        s.echo(format!("Unknown pattern {up}."));
                        return Ok(Step::Continue);
                    }
                    self.props.insert("pattern".into(), json!(up));
                }
                self.ask = Ask::Scale;
                Ok(Step::Continue)
            }
            (Ask::Scale, _) => {
                if let Some(t) = text {
                    match crate::cmd::machines::number(&t).filter(|v| v.is_finite() && *v > 0.0) {
                        Some(v) => {
                            self.props.insert("scale".into(), json!(v));
                        }
                        None => return Ok(Step::Continue),
                    }
                }
                self.ask = Ask::Angle;
                Ok(Step::Continue)
            }
            (Ask::Angle, _) => {
                if let Some(t) = text {
                    match s.angle_settings().rotation(&t).filter(|a| a.is_finite()) {
                        Some(a) => {
                            self.props.insert("angle".into(), json!(a.to_degrees()));
                        }
                        None => return Ok(Step::Continue),
                    }
                }
                self.apply(s, Value::Object(self.props.clone()))
            }
            (Ask::Origin, Input::Point(p)) => self.apply(s, json!({ "origin": [p.x, p.y], "originMode": "current" })),
            (Ask::Origin, Input::Enter) => Ok(Step::Done),
            (Ask::Origin, _) => {
                let mode = match text.as_deref() {
                    Some("Bottom left") => "bottomLeft",
                    Some("bottom Right") => "bottomRight",
                    Some("Top left") => "topLeft",
                    Some("top riGht") => "topRight",
                    Some("Center") => "center",
                    Some("Use current") => return Ok(Step::Done),
                    _ => return Ok(Step::Continue),
                };
                self.apply(s, json!({ "originMode": mode }))
            }
            (Ask::Recreate, _) => {
                self.ask = Ask::RecreateLink;
                Ok(Step::Continue)
            }
            (Ask::RecreateLink, _) => self.apply(s, json!({ "recreate": "polyline", "associate": text.as_deref() == Some("Yes") })),
            (Ask::Link, Input::Pick(hs)) => self.apply(s, json!({ "associative": true, "boundary": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() })),
            (Ask::Link, Input::Enter) => self.apply(s, json!({ "associative": true })),
            (Ask::Link, _) => Ok(Step::Continue),
        }
    }
}
