//! Inquiry: DIST, ID, LIST, AREA, drawing inspection for agents.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::{Entity, EntityKind, Handle};
use cadcraft_geom::{Polyline, Vec2, Vec3};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, f64_req, point_req, point_value, points_param, targets};

use crate::units::{format_angle, format_distance};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

mod arcs;
mod loops;
mod proxy;
mod regions;
mod shapes;
mod usage;

/// The inquiry commands: distances, points, listing, area, time and status, the drawing and
/// entity queries an agent reads, SHAPES, USAGE, LOOPS, COUNT and the region commands.
pub fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
        CommandSpec::new("dist", "Distance", run_dist)
            .menu(&["Tools", "Inquiry", "Distance"])
            .alias(&["di"])
            .params("{p1, p2}")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(DistM::default()))),
        CommandSpec::new("id", "ID Point", run_id)
            .menu(&["Tools", "Inquiry", "ID Point"])
            .params("{at}")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(IdM))),
        CommandSpec::new("list", "List", run_list)
            .menu(&["Tools", "Inquiry", "List"])
            .alias(&["li", "ls"])
            .params("{handles?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ListM::default()))),
        CommandSpec::new("area", "Area", run_area)
            .menu(&["Tools", "Inquiry", "Area"])
            .alias(&["aa"])
            .params("{points: [[x,y],...]} | {handle}")
            .noundo()
            .interactive(|_| Ok(Box::new(AreaM::default()))),
        CommandSpec::new("time", "Time", run_time).menu(&["Tools", "Inquiry", "Time"]).noundo(),
        CommandSpec::new("status", "Status", run_status).menu(&["Tools", "Inquiry", "Status"]).noundo(),
        CommandSpec::new("drawing.inspect", "Inspect Drawing", run_inspect).params("{entities?: bool, limit?: n}").noundo(),
        CommandSpec::new("drawing.readability", "Drawing Readability", run_readability)
            .params("{} → {modelEntities, layoutEntities, blockDefs, anonymousDefs, anonymousRefs, missingBlockRefs, \
                     proxy: {acadProxy, ole2Frame, arcDimension, other, withDrawable, classes: [{class, app?, count, drawable, records, unparsed, drawableChunks}]}, \
                     textsInModel, textsInLayouts, textsInBlocks (per placed reference), textsInBlockDefs (per definition), \
                     unreachable: [{kind, count, reason}], truncated, selfCheck, message}")
            .noundo(),
        CommandSpec::new("entities", "Query Entities", run_entities)
            .params("{type?, layer?, selected?: bool (default false; true = current selection in active space), window?: [[x,y],[x,y]], crossing?: bool (default true; false = entirely inside), limit?, offset?} → {count (all matches), returned, offset, entities}")
            .noundo(),
        CommandSpec::new("shapes", "Analyze Shapes", shapes::run).params(shapes::PARAMS).noundo(),
        CommandSpec::new("usage", "Analyze Usage", usage::run).params(usage::PARAMS).noundo(),
        CommandSpec::new("loops", "Implied Loops", loops::run).params(loops::PARAMS).noundo(),
        CommandSpec::new("texts", "Query Texts", run_texts)
            .params("{contains?: text (any case), layer?, window?: [[x,y],[x,y]] (by insertion point), \
                     scope?: \"space\" (default) | \"nested\" (also the texts inside the blocks the space's references draw, \
                     once per placed copy; attribute definitions are not drawn, a reference's own attributes are), \
                     limit? (default 200), offset?} \
                     → {count (all matches), returned, offset, truncated, texts: [{handle, type, layer, raw (as stored, format codes intact), \
                     text (as drawn: formatting and %% codes removed), at, height, rotation (degrees), style, tag? (attributes), \
                     source: \"space\" | \"block:<name>\", depth (0 in the space, 1 inside a placed reference, deeper in a nested block)}]}")
            .noundo(),
        CommandSpec::new("countlist", "Count List", count::run_countlist)
            .params("{space?: all (default)|current|model|<layout>, nested?: bool} → per-block counts, as COUNT")
            .noundo(),
        CommandSpec::new("counttable", "Count Table", count::run_counttable)
            .params("{at: [x,y], space?: current (default)|model|all|<layout>, nested?: bool} → {handle}: a table of the block counts")
            .interactive(|_| Ok(count::table_machine())),
        CommandSpec::new("count", "Count", count::run_count)
            .menu(&["Tools", "Count"])
            .params(count::PARAMS)
            .interactive(|_| Ok(count::machine()))
            .noundo(),
    ];
    v.extend(regions::specs());
    v
}

/// Format a distance with the drawing's `LUNITS` and `LUPREC` (decimal units, four places by
/// default).
fn fmt_d(s: &Session, v: f64) -> String {
    let h = s.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
    format_distance(v, h.0, h.1)
}
/// Format an angle with the drawing's `AUNITS` and `AUPREC` (decimal degrees, whole degrees by
/// default).
fn fmt_a(s: &Session, v: f64) -> String {
    let h = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
    format_angle(v, h.0, h.1)
}

/// Schema of the analysis self-check block (#1440). Fields may be added within a version; a
/// rename or a changed meaning bumps it.
pub(super) const SELF_CHECK_SCHEMA: u64 = 1;

/// How far past its bound a value may be before it is implausible: a relative slack for the
/// rounding of two ways of measuring the same thing.
const IMPLAUSIBLE_TOLERANCE: f64 = 1e-9;

/// How many objects of one type an analysis skipped, and every distinct reason it gave.
#[derive(Default)]
struct SkipTally {
    count: u64,
    reasons: BTreeSet<String>,
}

/// The self-check every analysis result carries (#1440): how many objects it found, how many it
/// analysed, what it skipped and why, the caps it hit with their values, and values so
/// implausible that they point at a bug rather than at the drawing.
#[derive(Default)]
pub(super) struct SelfCheck {
    found: u64,
    analyzed: u64,
    skipped: BTreeMap<String, SkipTally>,
    caps: BTreeMap<String, u64>,
    implausible: Vec<Value>,
}

impl SelfCheck {
    /// An empty self-check: nothing found, analysed or skipped yet.
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Count objects the analysis found (matched its filters).
    pub(super) fn found(&mut self, n: u64) {
        self.found = self.found.saturating_add(n);
    }

    /// Count objects the analysis actually analysed.
    pub(super) fn analyzed(&mut self, n: u64) {
        self.analyzed = self.analyzed.saturating_add(n);
    }

    /// Count `n` objects of `ty` skipped, with the reason.
    pub(super) fn skip(&mut self, ty: &str, reason: &str, n: u64) {
        let skip = self.skipped.entry(ty.to_string()).or_default();
        skip.count = skip.count.saturating_add(n);
        skip.reasons.insert(reason.to_string());
    }

    /// Record a cap the analysis hit, with its value (e.g. `limit`, 200).
    pub(super) fn cap(&mut self, name: &str, value: u64) {
        self.caps.insert(name.to_string(), value);
    }

    /// Flag `value` as implausible when it is past `bound`, with the rule that makes it so. A
    /// non-finite `bound` means no bound applies (an empty drawing has no extents). Values within
    /// `IMPLAUSIBLE_TOLERANCE` of the bound are not flagged: they are the rounding of two ways
    /// of measuring the same thing (a border rectangle's shoelace area against the extents'
    /// `width * height`), not a bug.
    pub(super) fn implausible(&mut self, handle: Handle, ty: &str, field: &str, value: f64, bound: f64, rule: &str) {
        if value.is_finite() && bound.is_finite() && value > bound + bound.abs() * IMPLAUSIBLE_TOLERANCE {
            self.implausible.push(json!({
                "handle": handle.hex(),
                "type": ty,
                "field": field,
                "value": value,
                "bound": bound,
                "reason": rule,
            }));
        }
    }

    /// The skip map in the shape analyses have always reported: type → {count, reason}.
    pub(super) fn skipped_json(&self) -> Value {
        Value::Object(
            self.skipped
                .iter()
                .map(|(ty, skip)| {
                    let reason = skip.reasons.iter().map(String::as_str).collect::<Vec<_>>().join("; ");
                    (ty.clone(), json!({ "count": skip.count, "reason": reason }))
                })
                .collect(),
        )
    }

    /// The self-check as the JSON object every analysis returns: schema, counts, skip map, caps
    /// and implausible values.
    pub(super) fn to_json(&self) -> Value {
        json!({
            "schema": SELF_CHECK_SCHEMA,
            "found": self.found,
            "analyzed": self.analyzed,
            "skipped": self.skipped_json(),
            "caps": self.caps,
            "implausible": self.implausible,
        })
    }
}

/// The angle of `d` up from the XY plane.
fn angle_from_xy(d: Vec3) -> f64 {
    d.z.atan2(d.xy().len())
}

fn dist_text(s: &Session, a: Vec3, b: Vec3) -> String {
    let d = b - a;
    format!(
        "Distance = {},  Angle in XY Plane = {},  Angle from XY Plane = {}\nDelta X = {},  Delta Y = {},   Delta Z = {}",
        fmt_d(s, (b - a).len()),
        fmt_a(s, d.xy().angle()),
        fmt_a(s, angle_from_xy(d)),
        fmt_d(s, d.x),
        fmt_d(s, d.y),
        fmt_d(s, d.z)
    )
}

/// A point parameter with its Z: the third number when given, else the Z of what is drawn
/// there ([`z_at`]).
fn point3_req(s: &Session, cmd: &str, p: &Value, key: &str) -> Result<Vec3> {
    let q = point_req(cmd, p, key)?;
    let z = p.get(key).and_then(|v| v.get(2)).and_then(Value::as_f64).filter(|z| z.is_finite());
    Ok(Vec3::new(q.x, q.y, z.unwrap_or_else(|| z_at(s, q))))
}

fn run_dist(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point3_req(s, "dist", p, "p1")?;
    let b = point3_req(s, "dist", p, "p2")?;
    let d = b - a;
    if !d.len().is_finite() {
        return Err(bad("dist", "the distance is too large"));
    }
    s.distance = d.len();
    s.last_point = a.xy();
    Ok(json!({
        "distance": d.len(),
        "angle": d.xy().angle().to_degrees(),
        "angleFromXY": angle_from_xy(d).to_degrees(),
        "dx": d.x,
        "dy": d.y,
        "dz": d.z,
        "message": dist_text(s, a, b),
    }))
}

fn id_text(s: &Session, a: Vec3) -> String {
    format!("X = {}     Y = {}     Z = {}", fmt_d(s, a.x), fmt_d(s, a.y), fmt_d(s, a.z))
}

fn run_id(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point3_req(s, "id", p, "at")?;
    s.last_point = a.xy();
    Ok(json!({ "x": a.x, "y": a.y, "z": a.z, "message": id_text(s, a) }))
}

/// The Z of what is drawn at `p`: the object a pick finds there, at that point, or the current
/// elevation (ELEVATION) when there is none. Points come from the canvas in 2D; the object
/// under a snapped point gives its Z back, as AutoCAD's ID and DIST report a snapped point's.
pub(crate) fn z_at(s: &Session, p: Vec2) -> f64 {
    let Ok(d) = s.doc() else { return 0.0 };
    let elevation = d.header.f64("ELEVATION", 0.0);
    let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
    let space = s.space();
    crate::select::pick(d, &space, p, ap)
        .and_then(|h| d.entity(h))
        .and_then(|e| entity_z(e, p))
        .filter(|z| z.is_finite())
        .unwrap_or(if elevation.is_finite() { elevation } else { 0.0 })
}

/// The Z of `e` at the point of it nearest `p` (seen from above), when it has one there.
fn entity_z(e: &Entity, p: Vec2) -> Option<f64> {
    let on_segment = |a: Vec3, b: Vec3| {
        let d = b.xy() - a.xy();
        let l2 = d.dot(d);
        let t = if l2 > 0.0 { ((p - a.xy()).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        (a.z + (b.z - a.z) * t, (a.xy() + d * t).dist(p))
    };
    let along = |pts: &[Vec3], closed: bool| {
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        (0..segs)
            .filter_map(|i| Some(on_segment(*pts.get(i)?, *pts.get((i + 1) % n)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(z, _)| z)
            .or_else(|| pts.first().map(|q| q.z))
    };
    let n = e.common.extrusion;
    let plan = n.x.abs() < 1e-9 && n.y.abs() < 1e-9 && n.z != 0.0;
    match &e.kind {
        EntityKind::Point(q) => Some(q.p.z),
        EntityKind::Line(l) => Some(on_segment(l.a, l.b).0),
        EntityKind::Polyline3d(pl) => along(&pl.points, pl.closed),
        // Elevation is in the polyline's OCS: down Z for a mirrored one.
        EntityKind::LwPolyline(pl) if plan => Some(if n.z < 0.0 { -pl.elevation } else { pl.elevation }),
        // Read in world coordinates already.
        EntityKind::Circle(c) => Some(c.center.z),
        EntityKind::Arc(a) => Some(a.center.z),
        EntityKind::Text(t) | EntityKind::AttDef(cadcraft_doc::Attrib { text: t, .. }) => Some(t.insert.z),
        EntityKind::MText(t) => Some(t.insert.z),
        EntityKind::Insert(i) => Some(i.insert.z),
        EntityKind::Face3d(f) => face_z(&f.corners, p).or_else(|| along(&f.corners, true)),
        // Drawn as its faces' visible edges: the Z on the nearest one.
        EntityKind::PolyfaceMesh(m) => m
            .faces3d()
            .iter()
            .flat_map(cadcraft_doc::Face3d::visible_edges)
            .map(|(a, b)| on_segment(a, b))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(z, _)| z),
        _ => None,
    }
}

/// The Z at `p` on a 3D face: on the plane of the triangle of its corners that holds `p`.
fn face_z(c: &[Vec3; 4], p: Vec2) -> Option<f64> {
    let tri = |a: Vec3, b: Vec3, d: Vec3| -> Option<f64> {
        let n = (b - a).cross(d - a);
        if n.z.abs() < 1e-12 * (1.0 + n.len()) {
            return None;
        }
        let inside = cadcraft_geom::point_in_polygon(&[a.xy(), b.xy(), d.xy()], p);
        inside.then(|| a.z - (n.x * (p.x - a.x) + n.y * (p.y - a.y)) / n.z)
    };
    tri(c[0], c[1], c[2]).or_else(|| if c[3] == c[2] { None } else { tri(c[0], c[2], c[3]) })
}

fn list_text(s: &Session, h: Handle) -> String {
    let Ok(d) = s.doc() else { return String::new() };
    let Some(e) = d.entity(h) else { return String::new() };
    let space = if d.model.contains(h) { "Model space" } else { "Paper space" };
    let mut t = format!(
        "                  {}     Layer: \"{}\"\n                            Space: {}\n                   Handle = {}",
        e.kind.dxf_name(),
        e.common.layer,
        space,
        h.hex()
    );
    match &e.kind {
        EntityKind::Line(l) => {
            t.push_str(&format!(
                "\n              from point, X={}  Y={}  Z={}\n                to point, X={}  Y={}  Z={}",
                fmt_d(s, l.a.x),
                fmt_d(s, l.a.y),
                fmt_d(s, l.a.z),
                fmt_d(s, l.b.x),
                fmt_d(s, l.b.y),
                fmt_d(s, l.b.z)
            ));
            t.push_str(&format!(
                "\n          Length = {},  Angle in XY Plane = {}",
                fmt_d(s, (l.b - l.a).len()),
                fmt_a(s, l.a.xy().angle_to(l.b.xy()))
            ));
            let d = l.b - l.a;
            t.push_str(&format!("\n          Delta X = {},  Delta Y = {},  Delta Z = {}", fmt_d(s, d.x), fmt_d(s, d.y), fmt_d(s, d.z)));
        }
        EntityKind::Circle(c) => {
            t.push_str(&format!(
                "\n            center point, X={}  Y={}  Z={}\n            radius {}\n     circumference {}\n             area {}",
                fmt_d(s, c.center.x),
                fmt_d(s, c.center.y),
                fmt_d(s, c.center.z),
                fmt_d(s, c.radius),
                fmt_d(s, c.radius * cadcraft_geom::TAU),
                fmt_d(s, c.radius * c.radius * cadcraft_geom::PI)
            ));
        }
        EntityKind::Arc(a) => {
            t.push_str(&format!(
                "\n            center point, X={}  Y={}  Z={}\n            radius {}\n            start angle {}\n              end angle {}",
                fmt_d(s, a.center.x),
                fmt_d(s, a.center.y),
                fmt_d(s, a.center.z),
                fmt_d(s, a.radius),
                fmt_a(s, a.start),
                fmt_a(s, a.end)
            ));
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            t.push_str(&format!("\n            {}\n    Constant width    {}", if p.closed { "Closed" } else { "Open" }, fmt_d(s, p.const_width)));
            if p.closed {
                t.push_str(&format!("\n             area   {}\n        perimeter   {}", fmt_d(s, pl.area().abs()), fmt_d(s, pl.len())));
            } else {
                t.push_str(&format!("\n           length   {}", fmt_d(s, pl.len())));
            }
            // The elevation, in world Z (down Z for a mirrored polyline).
            let z = entity_z(e, Vec2::ZERO).unwrap_or(p.elevation);
            t.push_str(&format!("\n        elevation   {}", fmt_d(s, z)));
            for v in &p.vertices {
                t.push_str(&format!("\n          at point  X={}  Y={}  Z={}", fmt_d(s, v.p.x), fmt_d(s, v.p.y), fmt_d(s, z)));
            }
        }
        EntityKind::Text(tx) => {
            t.push_str(&format!(
                "\n            Style = \"{}\"\n       start point, X={}  Y={}  Z={}\n          height {}\n           text {}\n        rotation angle {}",
                tx.style,
                fmt_d(s, tx.insert.x),
                fmt_d(s, tx.insert.y),
                fmt_d(s, tx.insert.z),
                fmt_d(s, tx.height),
                tx.value,
                fmt_a(s, tx.rotation)
            ));
        }
        other => {
            if let Ok(js) = serde_json::to_string(other) {
                t.push_str(&format!("\n          {js}"));
            }
        }
    }
    t
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let texts: Vec<String> = hs.iter().map(|h| list_text(s, *h)).collect();
    Ok(json!({ "message": texts.join("\n\n") }))
}

pub(super) fn area_of(pts: &[Vec2]) -> (f64, f64) {
    let a = cadcraft_geom::shoelace(pts).abs();
    let n = pts.len();
    let per: f64 = (0..n).filter_map(|i| Some(pts.get(i)?.dist(*pts.get((i + 1) % n)?))).sum();
    (a, per)
}

/// `points` as given, `None` when it is absent; a `points` that is not a list of points is an
/// error, never a fall back to the selection.
fn given_points(p: &Value, cmd: &str) -> Result<Option<Vec<Vec2>>> {
    match p.get("points") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => points_param(p, "points").map(Some).ok_or_else(|| bad(cmd, "`points` must be a list of [x, y] points")),
    }
}

fn run_area(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, per) = if let Some(pts) = given_points(p, "area")? {
        area_of(&pts)
    } else {
        let h = targets(s, p)?.first().copied().ok_or_else(|| bad("area", "`points` or `handle` is required"))?;
        object_area(s, h)?
    };
    if let Some(what) = too_large(a, per) {
        return Err(bad("area", format!("the {what} is too large")));
    }
    s.area = a;
    s.perimeter = per;
    Ok(json!({ "area": a, "perimeter": per, "message": area_text(s, a, per) }))
}

fn area_text(s: &Session, a: f64, per: f64) -> String {
    format!("Area = {}, Perimeter = {}", fmt_d(s, a), fmt_d(s, per))
}

/// MEASUREGEOM's Volume option: the area of a polygon of three or more points, or of a 2D object
/// as AREA measures it (an open polyline as if closed), times the absolute value of a height.
pub(super) fn run_volume(s: &mut Session, p: &Value) -> Result<Value> {
    let a = if let Some(pts) = given_points(p, "volume.measure")? {
        if pts.len() < 3 {
            return Err(bad("volume.measure", "`points` needs 3 or more [x,y] points"));
        }
        area_of(&pts).0
    } else {
        let h = targets(s, p)?.first().copied().ok_or_else(|| bad("volume.measure", "`points` or `handle` is required"))?;
        object_area(s, h)?.0
    };
    let height = f64_req("volume.measure", p, "height")?;
    let v = a * height.abs();
    if !v.is_finite() {
        return Err(bad("volume.measure", "the volume is too large"));
    }
    Ok(json!({ "volume": v, "area": a, "height": height, "message": volume_text(s, v) }))
}

fn volume_text(s: &Session, v: f64) -> String {
    format!("Volume = {}", fmt_volume(s, v))
}

/// Format a volume as a number in the drawing's `LUPREC`. Engineering and architectural units
/// show it as a decimal number, not in feet and inches.
fn fmt_volume(s: &Session, v: f64) -> String {
    let (u, p) = s.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
    format_distance(v, if matches!(u, 3 | 4) { 2 } else { u }, p)
}

/// "area" or "perimeter" when that value is past f64's range, `None` when both can be reported.
fn too_large(a: f64, per: f64) -> Option<&'static str> {
    if !a.is_finite() {
        Some("area")
    } else if !per.is_finite() {
        Some("perimeter")
    } else {
        None
    }
}

/// Area and perimeter of a closed object (AREA's Object option).
pub(super) fn object_area(s: &Session, h: Handle) -> Result<(f64, f64)> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| bad("area", "no such object"))?;
    Ok(match &e.kind {
        EntityKind::Circle(c) => (c.radius * c.radius * cadcraft_geom::PI, c.radius * cadcraft_geom::TAU),
        EntityKind::LwPolyline(pl) => {
            // An open polyline is measured as if closed for the area, but its perimeter is its own length (AutoCAD's AREA help).
            let g = Polyline { vertices: pl.vertices.clone(), closed: true };
            let drawn = Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
            (g.area().abs(), drawn.len())
        }
        EntityKind::Ellipse(el) => {
            let arc = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            if !arc.is_full() {
                return Err(bad("area", "object has no area"));
            }
            let a = el.major.xy().len();
            let b = a * el.ratio;
            // The perimeter integrated around the ellipse, not a closed-form estimate (those
            // fall short as the ellipse flattens: 0.3% on a 100:1 one).
            (cadcraft_geom::PI * a * b, arc.arc_length_from(0.0, cadcraft_geom::TAU))
        }
        EntityKind::Hatch(hh) => {
            // Odd parity, as drawn: a loop nested inside an odd number of other loops is an unfilled island.
            let geoms: Vec<Polyline> = hh.loops.iter().map(|l| Polyline { vertices: l.vertices.clone(), closed: true }).collect();
            let polys: Vec<Vec<Vec2>> = geoms.iter().map(|g| g.tessellate(1e-3)).collect();
            let mut total = (0.0, 0.0);
            for (i, g) in geoms.iter().enumerate() {
                let probe = polys.get(i).and_then(|p| p.first().copied());
                let depth = probe.map_or(0, |pt| polys.iter().enumerate().filter(|(j, q)| *j != i && cadcraft_geom::point_in_polygon(q, pt)).count());
                total.0 += if depth % 2 == 0 { g.area().abs() } else { -g.area().abs() };
                total.1 += g.len();
            }
            total
        }
        EntityKind::Region(r) => (r.area(), r.perimeter()),
        // Like an open polyline: the area closed by the chord, the perimeter its own length.
        EntityKind::Spline(sp) if sp.is_valid() => {
            let b = sp.bounds();
            let pts = sp.tessellate_spans((b.min.dist(b.max) * 1e-7).max(1e-12));
            // A length past f64's range is too large to report, as a polyline's is, not 0.
            (cadcraft_geom::shoelace(&pts).abs(), sp.checked_length().unwrap_or(f64::INFINITY))
        }
        _ => return Err(bad("area", "object has no area")),
    })
}

fn run_time(s: &mut Session, _p: &Value) -> Result<Value> {
    let rev = s.state()?.revision;
    Ok(json!({ "message": format!("Drawing revision: {rev}\nEdits recorded: {}", s.state()?.undo.len()) }))
}

fn run_status(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let ext = d.extents(&cadcraft_doc::Space::Model);
    let msg = format!(
        "{} objects in {}\nModel space limits are X: {}  Y: {}  (Off)\nModel space uses X: {}  Y: {}\n                 X: {}  Y: {}\nCurrent layer: {}\nCurrent color: {}\nCurrent linetype: {}",
        d.entity_count(),
        s.state()?.title,
        fmt_d(s, d.header.point("LIMMIN").map(|p| p.x).unwrap_or(0.0)),
        fmt_d(s, d.header.point("LIMMIN").map(|p| p.y).unwrap_or(0.0)),
        fmt_d(s, ext.min.x),
        fmt_d(s, ext.min.y),
        fmt_d(s, ext.max.x),
        fmt_d(s, ext.max.y),
        d.header.str("CLAYER", "0"),
        cadcraft_color::Color::from_aci(d.header.i64("CECOLOR", 256) as i16).name(),
        d.header.str("CELTYPE", "ByLayer"),
    );
    Ok(json!({ "message": msg }))
}

/// An entity as the query commands report it. An unmodelled entity with graphics data carries a
/// `proxy` object: its class and the frames of its graphics cache, counted by type — what tells a
/// proxy that draws nothing (transforms and state only) from one with real content.
pub(super) fn entity_summary(d: &cadcraft_doc::Drawing, e: &cadcraft_doc::Entity) -> Value {
    let mut v = json!({ "handle": e.handle.hex(), "type": e.kind.type_name(), "layer": e.common.layer, "color": e.common.color.name(), "transparency": super::props::transparency_value(e.common.transparency), "geometry": e.kind });
    if let cadcraft_doc::EntityKind::Unknown(u) = &e.kind
        && let Some(p) = proxy::summary(d, u)
        && let Some(o) = v.as_object_mut()
    {
        o.insert("proxy".into(), p);
    }
    v
}

fn run_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state()?;
    let d = &st.doc;
    let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
    let space = st.space.clone();
    let ext = d.extents(&space);
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    if let Some(store) = d.space(&space) {
        for e in store.iter() {
            // An unmodelled entity counts under its own DXF name (ACAD_PROXY_ENTITY, OLE2FRAME,
            // ARC_DIMENSION, …), not all together as "Proxy".
            let ty = match &e.kind {
                cadcraft_doc::EntityKind::Unknown(u) => u.dxf_type.clone(),
                _ => e.kind.type_name().to_string(),
            };
            *counts.entry(ty).or_default() += 1;
        }
    }
    let mut out = json!({
        "title": st.title,
        "path": st.path,
        "dirty": st.is_dirty(),
        "blockEditor": st.block_editor(),
        "space": space.tab_name(),
        "entityCount": d.entity_count(),
        "counts": counts,
        "extents": if ext.is_empty() { Value::Null } else { json!([[ext.min.x, ext.min.y], [ext.max.x, ext.max.y]]) },
        "layers": d.layers.iter().map(|l| json!({"name": l.name, "on": l.on, "frozen": l.frozen, "locked": l.locked, "color": l.color.name(), "linetype": l.linetype})).collect::<Vec<_>>(),
        "currentLayer": d.header.str("CLAYER", "0"),
        "linetypes": d.linetypes.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "textStyles": d.text_styles.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "dimStyles": d.dim_styles.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "blocks": d.blocks.keys().filter(|k| !k.starts_with('*')).cloned().collect::<Vec<_>>(),
        "layouts": d.layouts.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "selection": st.selection.iter().map(|h| h.hex()).collect::<Vec<_>>(),
        "undo": st.undo.iter().rev().take(20).map(|u| u.label.clone()).collect::<Vec<_>>(),
        "view": { "center": [st.view().center.x, st.view().center.y], "height": st.view().height },
        // Records of the file the drawing came from that the reader could not place.
        "skipped": { "count": d.skipped_records.values().fold(0usize, |a, n| a.saturating_add(*n)), "sections": d.skipped_records },
        // What reading the file left out, in words (a damaged DWG's unreadable object records).
        "readNotes": d.read_notes,
    });
    if bool_or(p, "entities", true)
        && let (Some(o), Some(store)) = (out.as_object_mut(), d.space(&space))
    {
        o.insert("entities".into(), Value::Array(store.iter().take(limit).map(|e| entity_summary(d, e)).collect()));
    }
    Ok(out)
}

fn run_entities(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "entities";
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let text = |k: &str| -> Result<Option<String>> {
        given(k).map(|v| v.as_str().map(str::to_ascii_lowercase).ok_or_else(|| bad(CMD, format!("`{k}` must be a string")))).transpose()
    };
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let ty = text("type")?;
    let layer = text("layer")?;
    let window = given("window")
        .map(|w| {
            let corners: Option<Vec<Vec2>> = w.as_array().filter(|a| a.len() == 2).and_then(|a| a.iter().map(point_value).collect());
            match corners.as_deref() {
                Some([a, b]) => Ok(cadcraft_geom::Bounds2::new(*a, *b)),
                _ => Err(bad(CMD, "`window` must be two corner points [[x0, y0], [x1, y1]]")),
            }
        })
        .transpose()?;
    let crossing = given("crossing").map_or(Ok(true), |v| v.as_bool().ok_or_else(|| bad(CMD, "`crossing` must be true or false")))?;
    let selected = given("selected").map_or(Ok(false), |v| v.as_bool().ok_or_else(|| bad(CMD, "`selected` must be true or false")))?;
    let limit = count("limit", 500)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let d = &st.doc;
    let Some(store) = d.space(&st.space) else { return Ok(json!({ "count": 0, "returned": 0, "entities": [] })) };
    let tol = window.as_ref().map(crate::select::window_tol).unwrap_or(0.0);
    let selection: std::collections::HashSet<Handle> = if selected { st.selection.iter().copied().collect() } else { Default::default() };
    let matches: Vec<&std::sync::Arc<cadcraft_doc::Entity>> = store
        .iter()
        .filter(|e| !selected || selection.contains(&e.handle))
        .filter(|e| ty.as_ref().is_none_or(|t| e.kind.type_name().to_ascii_lowercase() == *t || e.kind.dxf_name().to_ascii_lowercase() == *t))
        .filter(|e| layer.as_ref().is_none_or(|l| e.common.layer.to_ascii_lowercase() == *l))
        .filter(|e| window.as_ref().is_none_or(|w| crate::select::in_window(d, e, None, w, crossing, tol)))
        .collect();
    let page: Vec<Value> = matches.iter().skip(offset).take(limit).map(|e| entity_summary(d, e)).collect();
    // `count` is every match; `returned` is this page of them (`offset`, `limit`).
    Ok(json!({ "count": matches.len(), "returned": page.len(), "offset": offset, "entities": page }))
}

/// One readable text, wherever it is drawn from: its content as stored (`raw`) and as drawn
/// (`text`), where and how it is drawn, and where it came from (`source` "space", or the block a
/// reference drew it from; `depth` 0 in the space itself, 1 inside a placed reference, deeper in a
/// block that block holds).
#[derive(Clone)]
struct TextHit {
    handle: Handle,
    ty: &'static str,
    layer: String,
    raw: String,
    text: String,
    at: Vec3,
    height: f64,
    rotation: f64,
    style: String,
    tag: Option<String>,
    source: String,
    depth: u32,
}

/// `s` as drawn: its `%%d`-style control codes become the characters they stand for.
fn plain_text(s: &str) -> String {
    cadcraft_fonts::decode_controls(s).into_iter().map(|(c, _, _)| c).collect()
}

/// A text as it is stored: format codes (`\P`, `%%c`, …) intact.
fn raw_text(s: &str) -> String {
    s.to_string()
}

impl TextHit {
    /// The hit as TEXTS reports it.
    fn json(&self) -> Value {
        let mut v = json!({
            "handle": self.handle.hex(), "type": self.ty, "layer": self.layer,
            "raw": self.raw, "text": self.text,
            "at": [self.at.x, self.at.y], "height": self.height,
            "rotation": self.rotation.to_degrees(), "style": self.style,
            "source": self.source, "depth": self.depth,
        });
        if let (Some(t), Some(o)) = (&self.tag, v.as_object_mut()) {
            o.insert("tag".into(), json!(t));
        }
        v
    }
}

/// The readable texts `e` holds in the space itself: a text, a multiline text, a multileader's,
/// and the visible attributes of a block reference (an attribute definition is not drawn — only a
/// reference's own attributes are, and those live on the reference).
fn texts_of(e: &Entity) -> Vec<TextHit> {
    let hit = |ty: &'static str, raw: String, text: String, at: Vec3, height: f64, rotation: f64, style: &str, tag: Option<String>| TextHit {
        handle: e.handle,
        ty,
        layer: e.common.layer.clone(),
        raw,
        text,
        at,
        height,
        rotation,
        style: style.to_string(),
        tag,
        source: "space".into(),
        depth: 0,
    };
    match &e.kind {
        EntityKind::Text(t) => vec![hit("Text", raw_text(&t.value), plain_text(&t.value), t.insert, t.height, t.rotation, &t.style, None)],
        EntityKind::MText(t) => {
            vec![hit("MText", raw_text(&t.contents), cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style, None)]
        }
        EntityKind::Insert(i) => i
            .attribs
            .iter()
            .filter(|a| !a.invisible)
            .map(|a| {
                hit(
                    "Attribute",
                    raw_text(&a.text.value),
                    plain_text(&a.text.value),
                    a.text.insert,
                    a.text.height,
                    a.text.rotation,
                    &a.text.style,
                    Some(a.tag.clone()),
                )
            })
            .collect(),
        EntityKind::MLeader(m) => match &m.text {
            Some(t) => {
                vec![hit("MLeader", raw_text(&t.contents), cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style, None)]
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// How far the texts a block reference tree can draw may go: past this many levels of nested
/// references the walk stops and says so (the drawing's own cap on nesting, `MAX_BLOCK_DEPTH`,
/// bounds it too).
const MAX_TEXT_DEPTH: u32 = 32;

/// Text hits the walk gives up collecting (a hostile drawing of self-nesting blocks); the counts
/// past it are partial, and the result says so.
const MAX_TEXT_HITS: usize = 250_000;

/// What the nested walk over a space's block references found: the texts its references draw,
/// and whether it had to give up, with why (a reference to a block the drawing does not define
/// draws nothing; DRAWING.READABILITY counts those separately).
struct NestedTexts {
    hits: Vec<TextHit>,
    truncated: bool,
    depth_capped: bool,
}

/// The nested walk's state: the drawing, the filter a hit passes, and the guards.
struct Walk<'a> {
    d: &'a cadcraft_doc::Drawing,
    keep: &'a dyn Fn(&TextHit) -> bool,
    out: Vec<TextHit>,
    truncated: bool,
    depth_capped: bool,
    work: u64,
}

impl Walk<'_> {
    /// A text transformed from the block it is drawn in into the space: its point through `m`, its
    /// size by the transform's scale, its direction along the transformed baseline.
    fn moved(mut t: TextHit, m: &cadcraft_geom::Mat3) -> TextHit {
        let at = m.apply(Vec2::new(t.at.x, t.at.y));
        t.at = Vec3::new(at.x, at.y, t.at.z);
        t.height *= m.scale_factor();
        t.rotation = m.apply_vec(Vec2::new(t.rotation.cos(), t.rotation.sin())).angle();
        t
    }

    /// A hit that passed the filter, transformed into the space; past the collection cap the walk
    /// gives up (and says so) rather than grow without bound.
    fn place(&mut self, h: TextHit, world: &cadcraft_geom::Mat3) {
        if self.out.len() >= MAX_TEXT_HITS {
            self.truncated = true;
            return;
        }
        self.out.push(Self::moved(h, world));
    }

    /// One block reference, at `level` (the depth it sits at: a top-level one is 0), placed by `m`
    /// out of the coordinates of `owner` (the space, or the block holding it): the copies a MINSERT
    /// grid places, the reference's own attributes (a top-level one's are the space's texts,
    /// already collected), and its block's objects one deeper.
    fn reference(&mut self, e: &Entity, level: u32, m: cadcraft_geom::Mat3, owner: &str) {
        let EntityKind::Insert(i) = &e.kind else { return };
        let Some(def) = self.d.block(&i.block).cloned() else { return };
        let jt = i.transform(def.base.xy());
        for row in 0..i.rows.max(1) {
            for col in 0..i.cols.max(1) {
                self.work += 1;
                if self.work > MAX_TEXT_HITS as u64 {
                    self.truncated = true;
                    return;
                }
                // The grid's offsets run along the reference's own axes, in the coordinates of the
                // block holding it, not scaled with it.
                let off = Vec2::new(f64::from(col) * i.col_spacing, f64::from(row) * i.row_spacing);
                let off = Vec2::new(off.x * i.rotation.cos() - off.y * i.rotation.sin(), off.x * i.rotation.sin() + off.y * i.rotation.cos());
                let world = m.then_before(cadcraft_geom::Mat3::translate(off)).then_before(jt);
                // A nested reference's own attributes are drawn where the reference sits; a
                // top-level one's are the space's own texts, already collected.
                if level >= 1 {
                    for a in i.attribs.iter().filter(|a| !a.invisible) {
                        let h = TextHit {
                            handle: e.handle,
                            ty: "Attribute",
                            layer: e.common.layer.clone(),
                            raw: raw_text(&a.text.value),
                            text: plain_text(&a.text.value),
                            at: a.text.insert,
                            height: a.text.height,
                            rotation: a.text.rotation,
                            style: a.text.style.clone(),
                            tag: Some(a.tag.clone()),
                            source: format!("block:{owner}"),
                            depth: level,
                        };
                        if (self.keep)(&h) {
                            self.place(h, &world);
                        }
                    }
                }
                let source = format!("block:{}", def.name);
                for inner in def.entities.iter() {
                    if let EntityKind::Insert(_) = &inner.kind {
                        if level + 1 > MAX_TEXT_DEPTH.min(cadcraft_doc::MAX_BLOCK_DEPTH as u32) {
                            self.depth_capped = true;
                            continue;
                        }
                        self.reference(inner, level + 1, world, &def.name);
                    } else {
                        for h in texts_in_definition(inner, &source, level + 1) {
                            if (self.keep)(&h) {
                                self.place(h, &world);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The texts the block references of `store` draw, one set per placed copy, transformed into the
/// space's coordinates — what the sheet shows beyond the space's own objects. Attribute
/// definitions are not drawn by a reference, so they never appear; the attributes a reference
/// carries itself do, at the depth the reference sits at.
fn nested_texts(d: &cadcraft_doc::Drawing, store: &cadcraft_doc::EntityStore, keep: &dyn Fn(&TextHit) -> bool) -> NestedTexts {
    let mut w = Walk { d, keep, out: Vec::new(), truncated: false, depth_capped: false, work: 0 };
    for e in store.iter() {
        if matches!(&e.kind, EntityKind::Insert(_)) {
            w.reference(e, 0, cadcraft_geom::Mat3::IDENTITY, "space");
        }
    }
    NestedTexts { hits: w.out, truncated: w.truncated, depth_capped: w.depth_capped }
}

/// The texts an entity of a block definition holds (attribute definitions excluded: a reference
/// does not draw them), tagged with the definition they sit in.
fn texts_in_definition(e: &Entity, source: &str, depth: u32) -> Vec<TextHit> {
    let hit = |ty: &'static str, raw: String, text: String, at: Vec3, height: f64, rotation: f64, style: &str| TextHit {
        handle: e.handle,
        ty,
        layer: e.common.layer.clone(),
        raw,
        text,
        at,
        height,
        rotation,
        style: style.to_string(),
        tag: None,
        source: source.to_string(),
        depth,
    };
    match &e.kind {
        EntityKind::Text(t) => vec![hit("Text", raw_text(&t.value), plain_text(&t.value), t.insert, t.height, t.rotation, &t.style)],
        EntityKind::MText(t) => {
            vec![hit("MText", raw_text(&t.contents), cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style)]
        }
        EntityKind::MLeader(m) => match &m.text {
            Some(t) => {
                vec![hit("MLeader", raw_text(&t.contents), cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style)]
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// TEXTS: the readable text of the space's texts, multiline texts, block attributes and
/// multileaders, with where they are, filtered by content, layer and a window around their
/// insertion points. Read-only (FIND selects and can replace; this only reports).
///
/// `scope: "nested"` also reads the texts inside the blocks the space's references draw, once per
/// placed copy — the number of them on the sheet, not the number of definitions that hold them.
fn run_texts(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "texts";
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let text = |k: &str| -> Result<Option<String>> {
        given(k).map(|v| v.as_str().map(str::to_lowercase).ok_or_else(|| bad(CMD, format!("`{k}` must be a string")))).transpose()
    };
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let contains = text("contains")?;
    let layer = text("layer")?;
    let scope = match p.get("scope").and_then(Value::as_str).unwrap_or("space") {
        "space" => false,
        "nested" => true,
        other => return Err(bad(CMD, format!("`scope` must be \"space\" or \"nested\", got {other:?}"))),
    };
    let window = given("window")
        .map(|w| {
            let corners: Option<Vec<Vec2>> = w.as_array().filter(|a| a.len() == 2).and_then(|a| a.iter().map(point_value).collect());
            match corners.as_deref() {
                Some([a, b]) => Ok(cadcraft_geom::Bounds2::new(*a, *b)),
                _ => Err(bad(CMD, "`window` must be two corner points [[x0, y0], [x1, y1]]")),
            }
        })
        .transpose()?;
    let limit = count("limit", 200)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let Some(store) = st.doc.space(&st.space) else {
        return Ok(json!({ "count": 0, "returned": 0, "offset": offset, "truncated": false, "texts": [] }));
    };
    let keep = |h: &TextHit| -> bool {
        !h.text.trim().is_empty()
            && layer.as_ref().is_none_or(|l| h.layer.to_lowercase() == *l)
            && contains.as_ref().is_none_or(|c| h.text.trim().to_lowercase().contains(c.as_str()))
            && window.as_ref().is_none_or(|w| w.contains(Vec2::new(h.at.x, h.at.y)))
    };
    let mut found: Vec<TextHit> = Vec::new();
    for e in store.iter() {
        found.extend(texts_of(e).into_iter().filter(|h| keep(h)));
    }
    let mut truncated = false;
    if scope {
        let n = nested_texts(&st.doc, store, &keep);
        found.extend(n.hits);
        truncated = n.truncated;
    }
    let n = found.len();
    let page: Vec<Value> = found.iter().skip(offset).take(limit).map(TextHit::json).collect();
    Ok(json!({ "count": n, "returned": page.len(), "offset": offset, "truncated": truncated, "texts": page }))
}

mod count;

/// DRAWING.READABILITY: what a reader can and cannot reach in this drawing — the counts behind
/// "read everything before concluding the drawing lacks it" (#1440's discipline, for the whole
/// drawing rather than one analysis): the entities, the block definitions (anonymous ones
/// included) and how far references reach, the proxies and whether their graphics draw anything,
/// the texts in the spaces against the ones inside blocks, and what stays out of reach, with the
/// reason.
fn run_readability(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.state()?;
    let d = &st.doc;
    let model_entities = d.model.len();
    let layout_entities: usize = d.layouts.iter().map(|l| l.entities.len()).sum();
    let block_defs = d.blocks.len();
    let anonymous_defs = d.blocks.values().filter(|b| b.anonymous || b.name.starts_with('*')).count();
    let block_entities: usize = d.blocks.values().map(|b| b.entities.len()).sum();

    // References as placed copies (a MINSERT grid counts per cell), nested ones included: the
    // anonymous ones the drawing reaches through, and the ones that point at no definition.
    fn is_anonymous(d: &cadcraft_doc::Drawing, name: &str) -> bool {
        d.block(name).is_some_and(|b| b.anonymous || b.name.starts_with('*'))
    }
    fn count_refs(d: &cadcraft_doc::Drawing, x: &mut count::Expander, name: &str, copies: u64, anonymous: &mut u64, missing: &mut u64) {
        if d.block(name).is_none() {
            *missing = missing.saturating_add(copies);
            return;
        }
        if is_anonymous(d, name) {
            *anonymous = anonymous.saturating_add(copies);
        }
        for (k, v) in x.expand(name, cadcraft_doc::MAX_BLOCK_DEPTH.saturating_sub(1)).iter() {
            if d.block(k).is_none() {
                *missing = missing.saturating_add(copies.saturating_mul(*v));
            } else if is_anonymous(d, k) {
                *anonymous = anonymous.saturating_add(copies.saturating_mul(*v));
            }
        }
    }
    let mut x = count::Expander::new(d);
    let mut anonymous_refs = 0u64;
    let mut missing_refs = 0u64;
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if let EntityKind::Insert(i) = &e.kind {
            let name = x.canonical(&i.block);
            let copies = u64::from(i.cols.max(1)) * u64::from(i.rows.max(1));
            count_refs(d, &mut x, &name, copies, &mut anonymous_refs, &mut missing_refs);
        }
    }

    // The proxies of the spaces: by class, and whether their graphics cache draws anything.
    #[derive(Default)]
    struct ClassTally {
        count: u64,
        with_drawable: u64,
        records: u64,
        unparsed: u64,
        histogram: BTreeMap<String, u64>,
    }
    let mut classes: BTreeMap<(String, String), ClassTally> = BTreeMap::new();
    let mut acad_proxy = 0u64;
    let mut ole_frames = 0u64;
    let mut arc_dimensions = 0u64;
    let mut other_unknowns = 0u64;
    let mut with_drawable = 0u64;
    let mut no_graphics = 0u64;
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        let EntityKind::Unknown(u) = &e.kind else { continue };
        match u.dxf_type.to_ascii_uppercase().as_str() {
            "ACAD_PROXY_ENTITY" => acad_proxy += 1,
            "OLE2FRAME" => ole_frames += 1,
            "ARC_DIMENSION" => arc_dimensions += 1,
            _ => other_unknowns += 1,
        }
        let Some(f) = proxy::frames(&u.tags) else {
            // A proxy without any graphics data can neither be read nor judged.
            no_graphics += 1;
            continue;
        };
        if f.drawable > 0 {
            with_drawable += 1;
        }
        let app = d.foreign.proxy_class(&u.tags).unwrap_or_default().to_string();
        let t = classes.entry((u.dxf_type.clone(), app)).or_default();
        t.count += 1;
        if f.drawable > 0 {
            t.with_drawable += 1;
        }
        t.records += f.records;
        t.unparsed += f.unparsed;
        for (k, v) in f.histogram {
            *t.histogram.entry(k).or_default() += v;
        }
    }

    // The texts: in the spaces themselves, and inside the blocks their references draw.
    let texts_in_model: usize = d.model.iter().map(|e| texts_of(e).len()).sum();
    let texts_in_layouts: usize = d.layouts.iter().flat_map(|l| l.entities.iter()).map(|e| texts_of(e).len()).sum();
    let texts_in_block_defs: usize = d.blocks.values().flat_map(|b| b.entities.iter()).map(|e| texts_in_definition(e, "", 1).len()).sum();
    let keep_all = |_: &TextHit| true;
    let walk = nested_texts(d, &d.model, &keep_all);
    let mut texts_in_blocks = walk.hits.len();
    let (mut truncated, mut depth_capped) = (walk.truncated, walk.depth_capped);
    for l in &d.layouts {
        let w = nested_texts(d, &l.entities, &keep_all);
        texts_in_blocks += w.hits.len();
        truncated |= w.truncated;
        depth_capped |= w.depth_capped;
    }

    // What stays out of reach, and why.
    let mut unreachable: Vec<Value> = Vec::new();
    let mut check = SelfCheck::new();
    check.found((model_entities + layout_entities) as u64);
    check.analyzed((model_entities + layout_entities) as u64);
    let out_of_reach = |list: &mut Vec<Value>, check: &mut SelfCheck, kind: &str, count: u64, reason: &str| {
        if count > 0 {
            list.push(json!({ "kind": kind, "count": count, "reason": reason }));
            check.skip(kind, reason, count);
        }
    };
    out_of_reach(&mut unreachable, &mut check, "missingBlock", missing_refs, "placed copies of a reference to a block the drawing does not define");
    let xref_not_loaded = d.blocks.values().filter(|b| b.xref_path.is_some() && b.entities.is_empty()).count() as u64;
    out_of_reach(&mut unreachable, &mut check, "xrefNotLoaded", xref_not_loaded, "an external reference whose drawing is not loaded");
    out_of_reach(&mut unreachable, &mut check, "proxyDrawable", with_drawable, "proxy graphics that draw; kept as raw records, not as entities");
    out_of_reach(&mut unreachable, &mut check, "proxyNoGraphics", no_graphics, "a proxy without graphics data: neither readable nor drawable");
    out_of_reach(&mut unreachable, &mut check, "ole2Frame", ole_frames, "an embedded OLE picture");
    let skipped_records: usize = d.skipped_records.values().sum();
    out_of_reach(&mut unreachable, &mut check, "skippedRecords", skipped_records as u64, "records the file reader could not place");
    if truncated {
        check.cap("textHits", MAX_TEXT_HITS as u64);
    }
    if depth_capped {
        check.cap("textDepth", u64::from(MAX_TEXT_DEPTH));
    }

    let message = format!(
        "{} entities ({} in model space), {} block definitions ({} anonymous, {} entities), {} anonymous references ({} to a missing definition)\n\
         proxies: {} ACAD_PROXY_ENTITY, {} OLE2FRAME, {} ARC_DIMENSION, {} other; {} with drawable graphics, {} without any\n\
         texts: {} in the spaces ({} in model space), {} inside blocks per placed reference, {} per definition\n\
         out of reach: {}",
        model_entities + layout_entities,
        model_entities,
        block_defs,
        anonymous_defs,
        block_entities,
        anonymous_refs,
        missing_refs,
        acad_proxy,
        ole_frames,
        arc_dimensions,
        other_unknowns,
        with_drawable,
        no_graphics,
        texts_in_model + texts_in_layouts,
        texts_in_model,
        texts_in_blocks,
        texts_in_block_defs,
        if unreachable.is_empty() { "nothing".to_string() } else { format!("{} kind(s)", unreachable.len()) },
    );
    Ok(json!({
        "modelEntities": model_entities,
        "layoutEntities": layout_entities,
        "blockDefs": block_defs,
        "anonymousDefs": anonymous_defs,
        "blockEntities": block_entities,
        "anonymousRefs": anonymous_refs,
        "missingBlockRefs": missing_refs,
        "proxy": {
            "acadProxy": acad_proxy,
            "ole2Frame": ole_frames,
            "arcDimension": arc_dimensions,
            "other": other_unknowns,
            "withDrawable": with_drawable,
            "noGraphics": no_graphics,
            "classes": classes.iter().map(|((class, app), t)| json!({
                "class": class, "app": app, "count": t.count, "drawable": t.with_drawable,
                "records": t.records, "unparsed": t.unparsed, "drawableChunks": t.histogram,
            })).collect::<Vec<_>>(),
        },
        "textsInModel": texts_in_model,
        "textsInLayouts": texts_in_layouts,
        "textsInBlocks": texts_in_blocks,
        "textsInBlockDefs": texts_in_block_defs,
        "unreachable": unreachable,
        "truncated": truncated,
        "selfCheck": check.to_json(),
        "message": message,
    }))
}

#[derive(Default)]
pub(super) struct DistM {
    first: Option<Vec2>,
    first_source: Option<crate::snap::ViewportPoint>,
    /// Multiple points: the points so far, the first included.
    multi: Option<Vec<Vec2>>,
    sources: Vec<Option<crate::snap::ViewportPoint>>,
    /// Asking for the length of a segment (the Length option).
    length: bool,
    /// The bulge of the segment that starts at each point (the Arc option).
    bulges: Vec<f64>,
    arcs: arcs::Arcs,
}

/// Total length of the segments through `pts`, each with its bulge in `bulges`.
fn path_len(pts: &[Vec2], bulges: &[f64]) -> f64 {
    if arcs::has_arcs(bulges) {
        return arcs::polyline(pts, bulges, false).len();
    }
    pts.windows(2)
        .map(|w| match w {
            [a, b] => a.dist(*b),
            _ => 0.0,
        })
        .sum()
}

/// The point `len` beyond the last of `pts`, continuing the last segment (along X after a single
/// point, along the tangent after an arc segment): the Length option of AREA and DIST's point
/// sequences.
fn extend_last(pts: &[Vec2], bulges: &[f64], len: f64) -> Option<Vec2> {
    let last = *pts.last()?;
    let p = last + arcs::tangent(pts, bulges) * len;
    p.is_finite().then_some(p)
}

fn length_prompt() -> Prompt {
    Prompt::new("Specify length of line", Accept::NUMBER)
}

/// Feed the Length option's answer: the next point, or `None` when the option was dismissed (Enter).
fn length_input(pts: &[Vec2], bulges: &[f64], i: Input) -> Result<Option<Vec2>> {
    let t = match i {
        Input::Text(t) => t,
        _ => return Ok(None),
    };
    let len = crate::units::parse_distance(&t).filter(|v| v.is_finite()).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
    Ok(extend_last(pts, bulges, len))
}

/// Where the paper point `p`, computed near `last` (an arc option's end point), lies in the model
/// space of the viewport `last` was picked through (the last of `sources`).
fn follow_source(sources: &[Option<crate::snap::ViewportPoint>], last: Vec2, p: Vec2) -> Option<crate::snap::ViewportPoint> {
    let source = sources.last().copied().flatten()?;
    let point = source.point + (p - last) / source.scale;
    (source.scale.abs() > 1e-12 && point.is_finite()).then_some(crate::snap::ViewportPoint { point, ..source })
}

/// Model units apply only when every point was snapped through one and the same viewport.
fn model_points(sources: &[Option<crate::snap::ViewportPoint>]) -> Option<Vec<Vec2>> {
    let viewport = sources.first().copied().flatten()?.viewport;
    sources.iter().map(|s| s.filter(|s| s.viewport == viewport).map(|s| s.point)).collect()
}

impl Interactive for DistM {
    fn name(&self) -> &'static str {
        "DIST"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.length {
            return length_prompt();
        }
        if let Some(p) = self.multi.as_deref().and_then(|pts| self.arcs.prompt(s, pts, &self.bulges, "")) {
            return p;
        }
        match (&self.multi, self.first) {
            (Some(pts), _) if pts.len() < 2 => {
                Prompt::new("Specify second point", Accept::POINT).kw(&["Arc", "Length", "Undo"]).base_opt(pts.last().copied())
            }
            (Some(pts), _) => Prompt::new("Specify next point", Accept::POINT)
                .kw(&["Arc", "Length", "Undo", "Total"])
                .default("Total")
                .base_opt(pts.last().copied()),
            (None, None) => Prompt::new("Specify first point", Accept::POINT),
            (None, Some(a)) => Prompt::new("Specify second point", Accept::POINT).kw(&["Multiple points"]).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(pts) = &mut self.multi {
            if self.length {
                let model = model_points(&self.sources);
                if let Some(p) = length_input(model.as_deref().unwrap_or(pts), &self.bulges, i)? {
                    let source = model.as_ref().and_then(|_| self.sources.last().copied().flatten());
                    let paper = match (source, pts.last()) {
                        (Some(source), Some(last)) => *last + (p - source.point) * source.scale,
                        _ => p,
                    };
                    if !paper.is_finite() {
                        return Err(bad("dist", "the next point is too large"));
                    }
                    arcs::push(pts, &mut self.bulges, paper, 0.0);
                    self.sources.push(source.map(|source| crate::snap::ViewportPoint { point: p, ..source }));
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts), &self.bulges))));
                }
                self.length = false;
                return Ok(Step::Continue);
            }
            if self.arcs.asking() {
                let last = pts.last().copied();
                // A picked end point keeps its own viewport; an end computed from the answers follows the
                // last point's. In model units, a typed radius or chord length is in model units too.
                let picked_end = match i {
                    Input::Point(p) if self.arcs.asks_end_point() => Some(p),
                    _ => None,
                };
                let i = match model_points(&self.sources).and(self.sources.last().copied().flatten()) {
                    Some(source) => self.arcs.scale_typed_size(i, source.scale),
                    None => i,
                };
                if self.arcs.answer(s, i, pts, &mut self.bulges)? {
                    // The arc can end off the picked point (CEnter projects it onto the circle).
                    let source = match (picked_end, last, pts.last()) {
                        (Some(p), _, Some(end)) => follow_source(&[s.picked_viewport], p, *end),
                        (None, Some(last), Some(end)) => follow_source(&self.sources, last, *end),
                        _ => None,
                    };
                    self.sources.push(source);
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts), &self.bulges))));
                }
                return Ok(Step::Continue);
            }
            match i {
                Input::Point(p) => {
                    self.arcs.point(pts, &mut self.bulges, p)?;
                    self.sources.push(s.picked_viewport);
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts), &self.bulges))));
                }
                Input::Keyword(k) if k == "Undo" && pts.len() > 1 => {
                    self.arcs.undo(pts, &mut self.bulges);
                    self.sources.pop();
                }
                Input::Keyword(k) if k == "Length" => self.length = true,
                Input::Keyword(k) if k == "Total" => return self.total(s, false),
                Input::Keyword(k) if k == "CLose" && self.arcs.on => {
                    self.arcs.close(pts, &mut self.bulges)?;
                    return self.total(s, true);
                }
                Input::Keyword(k) if !self.arcs.keyword(&k, pts) => {
                    s.echo(format!("{k}: not available yet"));
                }
                Input::Enter => return self.total(s, false),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match (self.first, i) {
            (None, Input::Point(p)) => {
                self.first = Some(p);
                self.first_source = s.picked_viewport;
                Ok(Step::Continue)
            }
            (Some(first), Input::Point(b)) => {
                let (a, b) = match (self.first_source, s.picked_viewport) {
                    (Some(first), Some(second)) if first.viewport == second.viewport => (first.point, second.point),
                    _ => (first, b),
                };
                let (za, zb) = if self.first_source.is_some() || s.picked_viewport.is_some() { (0.0, 0.0) } else { (z_at(s, a), z_at(s, b)) };
                let (a, b) = (Vec3::new(a.x, a.y, za), Vec3::new(b.x, b.y, zb));
                if !(b - a).len().is_finite() {
                    // The first point stays LASTPOINT, as after a measurement.
                    s.last_point = first;
                    return Err(bad("dist", "the distance is too large"));
                }
                let t = dist_text(s, a, b);
                for l in t.lines() {
                    s.echo(l.to_string());
                }
                // As in AutoCAD, the second point doesn't become LASTPOINT: the first stays.
                s.distance = (b - a).len();
                s.last_point = first;
                Ok(Step::Done)
            }
            (Some(a), Input::Keyword(k)) if k == "Multiple points" => {
                self.multi = Some(vec![a]);
                self.sources = vec![self.first_source];
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match &self.multi {
            Some(pts) => self.arcs.preview(pts, &self.bulges, c, false),
            None => self.first.map(|a| vec![super::helpers::line(a, c)]).unwrap_or_default(),
        }
    }
}

impl DistM {
    /// Report the total distance and end; `closed` adds the closing segment (CLose).
    fn total(&self, s: &mut Session, closed: bool) -> Result<Step> {
        let model = model_points(&self.sources);
        let d = model
            .as_deref()
            .or(self.multi.as_deref())
            .map_or(0.0, |pts| if closed { arcs::polyline(pts, &self.bulges, true).len() } else { path_len(pts, &self.bulges) });
        if !d.is_finite() {
            return Err(bad("dist", "the distance is too large"));
        }
        s.echo(format!("Distance = {}", fmt_d(s, d)));
        s.distance = d;
        Ok(Step::Done)
    }
}

struct IdM;

impl Interactive for IdM {
    fn name(&self) -> &'static str {
        "ID"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Input::Point(p) = i {
            let m = id_text(s, Vec3::new(p.x, p.y, z_at(s, p)));
            s.echo(m);
        }
        Ok(Step::Done)
    }
}

#[derive(Default)]
struct ListM {
    sel: super::machines::SelectPhase,
}

impl Interactive for ListM {
    fn name(&self) -> &'static str {
        "LIST"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = super::machines::SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            for h in hs {
                let t = list_text(s, h);
                for l in t.lines() {
                    s.echo(l.to_string());
                }
            }
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            super::machines::SelOutcome::More => Ok(Step::Continue),
            super::machines::SelOutcome::Empty => Ok(Step::Done),
            super::machines::SelOutcome::Done(hs) => {
                for h in hs {
                    let t = list_text(s, h);
                    for l in t.lines() {
                        s.echo(l.to_string());
                    }
                }
                Ok(Step::Done)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AreaMode {
    /// Measure one area and end.
    #[default]
    Single,
    /// Add each measured area to the running total.
    Add,
    /// Subtract each measured area from the running total.
    Subtract,
}

#[derive(Default)]
pub(super) struct AreaM {
    pts: Vec<Vec2>,
    mode: AreaMode,
    total: f64,
    /// Selecting objects (the Object option).
    object: bool,
    /// Asking for the length of a segment (the Length option).
    length: bool,
    /// Measuring volumes (MEASUREGEOM's Volume option): each area is multiplied by the absolute value of a height.
    volume: bool,
    /// Areas measured in volume mode that still wait for their height, oldest first.
    pending: Vec<f64>,
    /// The first point of a height given by two points.
    height_from: Option<Vec2>,
    /// The bulge of the segment that starts at each point (the Arc option).
    bulges: Vec<f64>,
    arcs: arcs::Arcs,
}

impl AreaM {
    /// MEASUREGEOM's Volume option: AREA's prompts, then a height for each area.
    pub(super) fn volume() -> Self {
        AreaM { volume: true, ..AreaM::default() }
    }
    fn mode_prefix(&self) -> &'static str {
        match self.mode {
            AreaMode::Single => "",
            AreaMode::Add => "(ADD mode) ",
            AreaMode::Subtract => "(SUBTRACT mode) ",
        }
    }
    /// The Add and Subtract keywords: "Add area" and "Subtract area", or "Add volume" and
    /// "Subtract volume".
    fn mode_keywords(&self) -> (&'static str, &'static str) {
        if self.volume { ("Add volume", "Subtract volume") } else { ("Add area", "Subtract area") }
    }
    /// The running total with `v` added or subtracted; in single mode, the total as it is.
    fn total_with(&self, v: f64) -> f64 {
        match self.mode {
            AreaMode::Single => self.total,
            AreaMode::Add => self.total + v,
            AreaMode::Subtract => self.total - v,
        }
    }
    /// Input while a height is pending: typed, or given by two points (their distance).
    fn height_input(&mut self, s: &mut Session, i: Input) -> Step {
        match (i, self.height_from) {
            (Input::Point(p), None) => self.height_from = Some(p),
            (Input::Point(p), Some(from)) => {
                self.height_from = None;
                return self.height(s, p.dist(from));
            }
            (Input::Text(t), None) => match crate::units::parse_distance(&t).filter(|v| v.is_finite()) {
                Some(h) => return self.height(s, h),
                None => s.echo("Requires a distance."),
            },
            (Input::Enter, None) => s.echo("Requires a height."),
            (Input::Enter, Some(_)) => s.echo("Requires a second point."),
            (Input::Text(_), Some(_)) => s.echo("Requires a point."),
            _ => {}
        }
        Step::Continue
    }

    /// Input while the objects to measure are picked.
    fn object_input(&mut self, s: &mut Session, i: Input) -> Step {
        match i {
            Input::Pick(hs) => {
                for h in hs {
                    match object_area(s, h) {
                        Ok((a, per)) => {
                            if self.measured(s, a, per) == Step::Done {
                                return Step::Done;
                            }
                            // A single volume takes the first object with an area.
                            if self.volume && self.mode == AreaMode::Single {
                                break;
                            }
                        }
                        Err(_) => s.echo("The selected object has no area."),
                    }
                }
            }
            Input::Enter => {
                self.object = false;
                if self.mode == AreaMode::Single {
                    return Step::Done;
                }
            }
            Input::Text(_) | Input::Keyword(_) => s.echo("*Invalid selection*"),
            _ => {}
        }
        Step::Continue
    }

    /// Measure the oldest pending area's volume with height `h`.
    fn height(&mut self, s: &mut Session, h: f64) -> Step {
        match self.pending.first().map(|a| a * h.abs()) {
            Some(v) if v.is_finite() && self.total_with(v).is_finite() => {
                self.pending.remove(0);
                self.volume_measured(s, v)
            }
            _ => {
                s.echo("The volume is too large.");
                Step::Continue
            }
        }
    }
    /// Report a volume (and the running total in add/subtract mode); ends the command in single mode.
    fn volume_measured(&mut self, s: &mut Session, v: f64) -> Step {
        s.echo(volume_text(s, v));
        if self.mode == AreaMode::Single {
            return Step::Done;
        }
        self.total = self.total_with(v);
        s.echo(format!("Total volume = {}", fmt_volume(s, self.total)));
        Step::Continue
    }
    /// Report an area (and the running total in add/subtract mode); ends the command in single mode.
    /// In volume mode the area waits for its height instead.
    /// An area, perimeter or total past f64's range is not reported or added to the total.
    fn measured(&mut self, s: &mut Session, a: f64, per: f64) -> Step {
        if self.volume {
            if !a.is_finite() {
                s.echo("The area is too large.");
                return if self.mode == AreaMode::Single { Step::Done } else { Step::Continue };
            }
            self.pending.push(a);
            return Step::Continue;
        }
        let total = self.total_with(a);
        if let Some(what) = too_large(a, per).or_else(|| (!total.is_finite()).then_some("total area")) {
            s.echo(format!("The {what} is too large."));
            return if self.mode == AreaMode::Single { Step::Done } else { Step::Continue };
        }
        s.echo(area_text(s, a, per));
        // AREA: this area, or in Add and Subtract mode the running total; PERIMETER: this area's.
        s.perimeter = per;
        if self.mode == AreaMode::Single {
            s.area = a;
            return Step::Done;
        }
        self.total = total;
        s.area = total;
        s.echo(format!("Total area = {}", fmt_d(s, self.total)));
        Step::Continue
    }
    /// Close the boundary picked so far (Enter or Total at the next-point prompt).
    /// An arc segment (the Arc option) can close an area of two points.
    fn close(&mut self, s: &mut Session) -> Step {
        let pts = std::mem::take(&mut self.pts);
        let bulges = std::mem::take(&mut self.bulges);
        self.arcs = arcs::Arcs::default();
        let curved = arcs::has_arcs(&bulges);
        if pts.len() < if curved { 2 } else { 3 } {
            return if self.mode == AreaMode::Single { Step::Done } else { Step::Continue };
        }
        let (a, per) = if curved {
            let g = arcs::polyline(&pts, &bulges, true);
            (g.area().abs(), g.len())
        } else {
            area_of(&pts)
        };
        self.measured(s, a, per)
    }
}

impl Interactive for AreaM {
    fn name(&self) -> &'static str {
        if self.volume { "MEASUREGEOM" } else { "AREA" }
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if !self.pending.is_empty() {
            return match self.height_from {
                Some(p) => Prompt::new("Specify second point", Accept::POINT).base(p),
                None => Prompt::new(format!("{}Specify height", self.mode_prefix()), Accept::POINT_OR_NUMBER),
            };
        }
        if self.object {
            return Prompt::new(format!("{}Select objects", self.mode_prefix()), Accept::SELECT);
        }
        if self.length {
            return length_prompt();
        }
        if let Some(p) = self.arcs.prompt(s, &self.pts, &self.bulges, self.mode_prefix()) {
            return p;
        }
        let next = format!("{}Specify next point", self.mode_prefix());
        let (add, subtract) = self.mode_keywords();
        match (self.pts.len(), self.mode) {
            (0, AreaMode::Single) => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", add, subtract]).default("Object"),
            (0, AreaMode::Add) => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", subtract]),
            (0, AreaMode::Subtract) => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", add]),
            (1, _) => Prompt::new(next, Accept::POINT).kw(&["Arc", "Length", "Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new(next, Accept::POINT).kw(&["Arc", "Length", "Undo", "Total"]).default("Total").base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.pending.is_empty() {
            return Ok(self.height_input(s, i));
        }
        if self.object {
            return Ok(self.object_input(s, i));
        }
        if self.length {
            if let Some(p) = length_input(&self.pts, &self.bulges, i)? {
                arcs::push(&mut self.pts, &mut self.bulges, p, 0.0);
            }
            self.length = false;
            return Ok(Step::Continue);
        }
        if self.arcs.asking() {
            self.arcs.answer(s, i, &mut self.pts, &mut self.bulges)?;
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => self.arcs.point(&mut self.pts, &mut self.bulges, p)?,
            Input::Keyword(k) => match k.as_str() {
                "Object" => self.object = true,
                "Add area" | "Add volume" => self.mode = AreaMode::Add,
                "Subtract area" | "Subtract volume" => self.mode = AreaMode::Subtract,
                "Undo" => self.arcs.undo(&mut self.pts, &mut self.bulges),
                "Length" => self.length = true,
                "Total" => return Ok(self.close(s)),
                "CLose" if self.arcs.on => {
                    self.arcs.close(&self.pts, &mut self.bulges)?;
                    return Ok(self.close(s));
                }
                k => {
                    if !self.arcs.keyword(k, &self.pts) {
                        s.echo(format!("{k}: not available yet"));
                    }
                }
            },
            Input::Enter if self.pts.is_empty() => match self.mode {
                // Enter takes the default, Object; in add/subtract mode it ends the command.
                AreaMode::Single => self.object = true,
                AreaMode::Add | AreaMode::Subtract => return Ok(Step::Done),
            },
            Input::Enter => return Ok(self.close(s)),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() || self.object || self.length {
            return Vec::new();
        }
        self.arcs.preview(&self.pts, &self.bulges, c, true)
    }
}

#[cfg(test)]
mod tests {
    use cadcraft_doc::Handle;

    use super::*;

    /// #1440 review: a value one ulp past its bound (a border rectangle's shoelace area against
    /// the extents' `width * height`) is rounding, not a bug; a value clearly past it still is.
    #[test]
    fn implausible_allows_rounding_but_not_a_real_excess() {
        let bound: f64 = 100.0;

        let mut check = SelfCheck::new();
        check.implausible(Handle::default(), "Polyline", "area", bound.next_up(), bound, "past the bound");
        assert!(check.implausible.is_empty(), "one ulp past the bound is rounding: {}", check.to_json());

        let mut check = SelfCheck::new();
        check.implausible(Handle::default(), "Polyline", "area", bound * (1.0 + 1e-6), bound, "past the bound");
        assert_eq!(check.implausible.len(), 1, "a real excess is still flagged: {}", check.to_json());

        // A bound that is not finite means no bound applies: the value is not flagged.
        let mut check = SelfCheck::new();
        check.implausible(Handle::default(), "Region", "area", 1e9, f64::INFINITY, "past the bound");
        assert!(check.implausible.is_empty(), "an infinite bound applies to nothing: {}", check.to_json());
    }
}
