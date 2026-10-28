//! Inquiry: DIST, ID, LIST, AREA, drawing inspection for agents.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::{Entity, EntityKind, Handle};
use cadcraft_geom::{Polyline, Vec2, Vec3};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, f64_req, point_req, point_value, points_param, targets};

use crate::units::{format_angle, format_distance};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

mod loops;
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
        CommandSpec::new("entities", "Query Entities", run_entities)
            .params("{type?, layer?, selected?: bool (default false; true = current selection in active space), window?: [[x,y],[x,y]], crossing?: bool (default true; false = entirely inside), limit?, offset?} → {count (all matches), returned, offset, entities}")
            .noundo(),
        CommandSpec::new("shapes", "Analyze Shapes", shapes::run).params(shapes::PARAMS).noundo(),
        CommandSpec::new("usage", "Analyze Usage", usage::run).params(usage::PARAMS).noundo(),
        CommandSpec::new("loops", "Implied Loops", loops::run).params(loops::PARAMS).noundo(),
        CommandSpec::new("texts", "Query Texts", run_texts)
            .params("{contains?: text (any case), layer?, window?: [[x,y],[x,y]] (by insertion point), limit? (default 200), offset?} → {count (all matches), returned, offset, texts: [{handle, type, layer, text, at, height, rotation (degrees), style, tag? (attributes)}]}: the readable text of TEXT, MTEXT (formatting removed), block attributes and multileaders")
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

fn entity_summary(e: &cadcraft_doc::Entity) -> Value {
    json!({ "handle": e.handle.hex(), "type": e.kind.type_name(), "layer": e.common.layer, "color": e.common.color.name(), "transparency": super::props::transparency_value(e.common.transparency), "geometry": e.kind })
}

fn run_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state()?;
    let d = &st.doc;
    let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
    let space = st.space.clone();
    let ext = d.extents(&space);
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    if let Some(store) = d.space(&space) {
        for e in store.iter() {
            *counts.entry(e.kind.type_name()).or_default() += 1;
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
    });
    if bool_or(p, "entities", true)
        && let (Some(o), Some(store)) = (out.as_object_mut(), d.space(&space))
    {
        o.insert("entities".into(), Value::Array(store.iter().take(limit).map(|e| entity_summary(e)).collect()));
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
    let page: Vec<Value> = matches.iter().skip(offset).take(limit).map(|e| entity_summary(e)).collect();
    // `count` is every match; `returned` is this page of them (`offset`, `limit`).
    Ok(json!({ "count": matches.len(), "returned": page.len(), "offset": offset, "entities": page }))
}

/// TEXTS: the readable text of the space's texts, multiline texts, block attributes and
/// multileaders, with where they are, filtered by content, layer and a window around their
/// insertion points. Read-only (FIND selects and can replace; this only reports).
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
    let Some(store) = st.doc.space(&st.space) else { return Ok(json!({ "count": 0, "returned": 0, "offset": offset, "texts": [] })) };
    let mut found: Vec<Value> = Vec::new();
    for e in store.iter() {
        if layer.as_ref().is_some_and(|l| e.common.layer.to_lowercase() != *l) {
            continue;
        }
        let mut push = |ty: &str, value: String, at: cadcraft_geom::Vec3, height: f64, rotation: f64, style: &str, tag: Option<&str>| {
            let value = value.trim().to_string();
            if value.is_empty()
                || contains.as_ref().is_some_and(|c| !value.to_lowercase().contains(c.as_str()))
                || window.as_ref().is_some_and(|w| !w.contains(Vec2::new(at.x, at.y)))
            {
                return;
            }
            let mut v = json!({
                "handle": e.handle.hex(), "type": ty, "layer": e.common.layer, "text": value,
                "at": [at.x, at.y], "height": height, "rotation": rotation.to_degrees(), "style": style,
            });
            if let (Some(t), Some(o)) = (tag, v.as_object_mut()) {
                o.insert("tag".into(), json!(t));
            }
            found.push(v);
        };
        match &e.kind {
            EntityKind::Text(t) => push("Text", t.value.clone(), t.insert, t.height, t.rotation, &t.style, None),
            EntityKind::MText(t) => push("MText", cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style, None),
            EntityKind::Insert(i) => {
                for a in i.attribs.iter().filter(|a| !a.invisible) {
                    push("Attribute", a.text.value.clone(), a.text.insert, a.text.height, a.text.rotation, &a.text.style, Some(&a.tag));
                }
            }
            EntityKind::MLeader(m) => {
                if let Some(t) = &m.text {
                    push("MLeader", cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style, None);
                }
            }
            _ => {}
        }
    }
    let n = found.len();
    let page: Vec<Value> = found.into_iter().skip(offset).take(limit).collect();
    Ok(json!({ "count": n, "returned": page.len(), "offset": offset, "texts": page }))
}

mod count;

#[derive(Default)]
pub(super) struct DistM {
    first: Option<Vec2>,
    first_source: Option<crate::snap::ViewportPoint>,
    /// Multiple points: the points so far, the first included.
    multi: Option<Vec<Vec2>>,
    sources: Vec<Option<crate::snap::ViewportPoint>>,
    /// Asking for the length of a segment (the Length option).
    length: bool,
}

/// Total length of the segments through `pts`.
fn path_len(pts: &[Vec2]) -> f64 {
    pts.windows(2)
        .map(|w| match w {
            [a, b] => a.dist(*b),
            _ => 0.0,
        })
        .sum()
}

/// The point `len` beyond the last of `pts`, continuing the last segment (along X after a single
/// point): the Length option of AREA and DIST's point sequences.
fn extend_last(pts: &[Vec2], len: f64) -> Option<Vec2> {
    let last = *pts.last()?;
    let dir = match pts.len().checked_sub(2).and_then(|i| pts.get(i)) {
        Some(prev) => (last - *prev).normalized(),
        None => Vec2::X,
    };
    let dir = if dir == Vec2::ZERO { Vec2::X } else { dir };
    let p = last + dir * len;
    p.is_finite().then_some(p)
}

fn length_prompt() -> Prompt {
    Prompt::new("Specify length of line", Accept::NUMBER)
}

/// Feed the Length option's answer: the next point, or `None` when the option was dismissed (Enter).
fn length_input(pts: &[Vec2], i: Input) -> Result<Option<Vec2>> {
    let t = match i {
        Input::Text(t) => t,
        _ => return Ok(None),
    };
    let len = crate::units::parse_distance(&t).filter(|v| v.is_finite()).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
    Ok(extend_last(pts, len))
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
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.length {
            return length_prompt();
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
                if let Some(p) = length_input(model.as_deref().unwrap_or(pts), i)? {
                    let source = model.as_ref().and_then(|_| self.sources.last().copied().flatten());
                    let paper = match (source, pts.last()) {
                        (Some(source), Some(last)) => *last + (p - source.point) * source.scale,
                        _ => p,
                    };
                    if !paper.is_finite() {
                        return Err(bad("dist", "the next point is too large"));
                    }
                    pts.push(paper);
                    self.sources.push(source.map(|source| crate::snap::ViewportPoint { point: p, ..source }));
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts)))));
                }
                self.length = false;
                return Ok(Step::Continue);
            }
            match i {
                Input::Point(p) => {
                    pts.push(p);
                    self.sources.push(s.picked_viewport);
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts)))));
                }
                Input::Keyword(k) if k == "Undo" => {
                    if pts.len() > 1 {
                        pts.pop();
                        self.sources.pop();
                    }
                }
                Input::Keyword(k) if k == "Length" => self.length = true,
                Input::Keyword(k) if k == "Total" => return self.total(s),
                Input::Keyword(k) => s.echo(format!("{k}: not available yet")),
                Input::Enter => return self.total(s),
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
            (Some(a), Input::Point(b)) => {
                let (a, b) = match (self.first_source, s.picked_viewport) {
                    (Some(first), Some(second)) if first.viewport == second.viewport => (first.point, second.point),
                    _ => (a, b),
                };
                let (za, zb) = if self.first_source.is_some() || s.picked_viewport.is_some() { (0.0, 0.0) } else { (z_at(s, a), z_at(s, b)) };
                let t = dist_text(s, Vec3::new(a.x, a.y, za), Vec3::new(b.x, b.y, zb));
                for l in t.lines() {
                    s.echo(l.to_string());
                }
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
            Some(pts) => {
                let mut v: Vec<cadcraft_geom::PolyVertex> = pts.iter().map(|p| cadcraft_geom::PolyVertex::new(*p)).collect();
                v.push(cadcraft_geom::PolyVertex::new(c));
                vec![super::helpers::lwpoly(v, false)]
            }
            None => self.first.map(|a| vec![super::helpers::line(a, c)]).unwrap_or_default(),
        }
    }
}

impl DistM {
    fn total(&self, s: &mut Session) -> Result<Step> {
        let model = model_points(&self.sources);
        let d = model.as_deref().or(self.multi.as_deref()).map_or(0.0, path_len);
        s.echo(format!("Distance = {}", fmt_d(s, d)));
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
    fn measured(&mut self, s: &mut Session, a: f64, per: f64) -> Step {
        if self.volume {
            self.pending.push(a);
            return Step::Continue;
        }
        s.echo(area_text(s, a, per));
        match self.mode {
            AreaMode::Single => return Step::Done,
            AreaMode::Add => self.total += a,
            AreaMode::Subtract => self.total -= a,
        }
        s.echo(format!("Total area = {}", fmt_d(s, self.total)));
        Step::Continue
    }
    /// Close the boundary picked so far (Enter or Total at the next-point prompt).
    fn close(&mut self, s: &mut Session) -> Step {
        let pts = std::mem::take(&mut self.pts);
        if pts.len() < 3 {
            return if self.mode == AreaMode::Single { Step::Done } else { Step::Continue };
        }
        let (a, per) = area_of(&pts);
        self.measured(s, a, per)
    }
}

impl Interactive for AreaM {
    fn name(&self) -> &'static str {
        if self.volume { "MEASUREGEOM" } else { "AREA" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
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
            if let Some(p) = length_input(&self.pts, i)? {
                self.pts.push(p);
            }
            self.length = false;
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => self.pts.push(p),
            Input::Keyword(k) => match k.as_str() {
                "Object" => self.object = true,
                "Add area" | "Add volume" => self.mode = AreaMode::Add,
                "Subtract area" | "Subtract volume" => self.mode = AreaMode::Subtract,
                "Undo" => {
                    self.pts.pop();
                }
                "Length" => self.length = true,
                "Total" => return Ok(self.close(s)),
                _ => s.echo(format!("{k}: not available yet")),
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
        let mut v: Vec<cadcraft_geom::PolyVertex> = self.pts.iter().map(|p| cadcraft_geom::PolyVertex::new(*p)).collect();
        v.push(cadcraft_geom::PolyVertex::new(c));
        vec![super::helpers::lwpoly(v, true)]
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
