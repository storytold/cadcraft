//! MEASUREGEOM's Quick option: the 2D geometry around the cursor measured as it moves. The
//! cursor's horizontal and vertical axes reach out to the first object each meets: a line it
//! meets shows its length, an arc or circle its radius, two parallel lines met on one axis the
//! distance between them, and lines met by both axes that join the angle between them (a square
//! marks a right angle). A click inside a closed area shows its area and perimeter, islands
//! included; Shift+click adds an area to the ones shown or takes it away.

use cadcraft_doc::color::{Color, Rgb};
use cadcraft_doc::{Entity, EntityKind, HAlign, Handle, Hatch, HatchLoop, Text, VAlign};
use cadcraft_geom::{Bounds2, Line, PolyVertex, Polyline, Segment, Vec2, intersect_segments};
use serde_json::{Value, json};

use super::super::helpers::lwpoly;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most objects looked at for one cursor position: past it the rest are left out, so a crowded
/// view still answers every frame.
const MAX_OBJECTS: usize = 4000;
/// Most segments of one object followed (a long tessellated spline).
const MAX_SEGMENTS: usize = 20_000;
/// Most areas measured together with Shift+click.
pub(crate) const MAX_AREAS: usize = 256;
/// The readouts' colour: orange, apart from the drawing's usual colours.
const MARK: Rgb = Rgb(0xff, 0x8c, 0x1a);
/// Measured areas are shaded with green stripes this far apart (pixels).
const AREA: Rgb = Rgb(0x2e, 0xb8, 0x4b);
const STRIPES: &str = "ANSI31";
const STRIPE_PX: f64 = 6.0;
/// Label height, tick size and right-angle square, in pixels (they keep their size on screen).
const LABEL_PX: f64 = 12.0;
const SQUARE_PX: f64 = 8.0;
/// Two directions closer than this (radians) are parallel, two angles this close to 90° right.
const PARALLEL: f64 = 1e-9;
const RIGHT: f64 = 1e-7;

/// What one axis direction from the cursor meets first.
#[derive(Clone, Copy, Debug)]
struct Hit {
    at: Vec2,
    seg: Segment,
    handle: Handle,
}

/// One measurement Quick shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reading {
    /// A line (or polyline line segment) an axis meets: its length, shown at its middle.
    Length { handle: Handle, a: Vec2, b: Vec2, length: f64 },
    /// An arc or circle an axis meets: its radius, shown where the axis meets it.
    Radius { handle: Handle, at: Vec2, center: Vec2, radius: f64 },
    /// Two parallel lines met on one axis: the distance between them, across the axis' span.
    Gap { a: Vec2, b: Vec2, distance: f64 },
    /// Two joined lines met by the two axes: the angle between them at their common end, in
    /// degrees (`right` when it is 90°).
    Angle { vertex: Vec2, a: Vec2, b: Vec2, degrees: f64, right: bool },
}

/// The cursor's axes, each from the first object met on one side to the first on the other (or
/// to the edge of the view), and the readings they give.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Readout {
    pub axes: Vec<(Vec2, Vec2)>,
    pub readings: Vec<Reading>,
}

/// The view's half width and half height in drawing units.
fn half_view(s: &Session) -> (f64, f64) {
    let v = s.state().map(crate::DocState::view).unwrap_or_default();
    let (w, h) = s.viewport_px;
    let aspect = if h > 0.0 && w > 0.0 { w / h } else { 1.0 };
    (v.height * aspect / 2.0, v.height / 2.0)
}

/// Measure the geometry around `c`.
pub(crate) fn readout(s: &Session, c: Vec2) -> Readout {
    let mut out = Readout::default();
    if s.doc().is_err() || !c.is_finite() {
        return out;
    }
    let (hw, hh) = half_view(s);
    let reach = (hw + hh).max(s.pixel_size());
    if !reach.is_finite() {
        return out;
    }
    let dirs = [Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0)];
    let rays: Vec<Segment> = dirs.iter().map(|&u| Segment::Line(Line::new(c, c + u * reach))).collect();
    let min_t = s.pixel_size() * 0.5;
    let mut best: [Option<(f64, Hit)>; 4] = [None; 4];
    for e in near_axes(s, c, reach, min_t) {
        let Some(chain) = super::super::curves::Chain::of(&e.kind) else { continue };
        for seg in chain.segs.iter().take(MAX_SEGMENTS) {
            for (k, (ray, u)) in rays.iter().zip(dirs).enumerate() {
                for p in intersect_segments(ray, seg) {
                    let t = (p - c).dot(u);
                    let closer = best.get(k).and_then(|b| b.as_ref()).is_none_or(|(bt, _)| t < *bt);
                    if t > min_t
                        && closer
                        && let Some(slot) = best.get_mut(k)
                    {
                        *slot = Some((t, Hit { at: p, seg: *seg, handle: e.handle }));
                    }
                }
            }
        }
    }
    let hit = |k: usize| best.get(k).and_then(|b| b.map(|(_, h)| h));
    let end = |k: usize, u: Vec2| hit(k).map_or(c + u * reach, |h| h.at);
    out.axes.push((end(1, dirs[1]), end(0, dirs[0])));
    out.axes.push((end(3, dirs[3]), end(2, dirs[2])));
    // Each object met: its length or radius, once.
    for k in 0..4 {
        let Some(h) = hit(k) else { continue };
        let reading = match h.seg {
            Segment::Line(l) => Reading::Length { handle: h.handle, a: l.a, b: l.b, length: l.len() },
            Segment::Arc { arc, .. } => Reading::Radius { handle: h.handle, at: h.at, center: arc.center, radius: arc.radius },
        };
        if !out.readings.contains(&reading) {
            out.readings.push(reading);
        }
    }
    // Parallel lines met on one axis: the distance between them.
    for (k1, k2) in [(1, 0), (3, 2)] {
        if let (Some(a), Some(b)) = (hit(k1), hit(k2))
            && let (Segment::Line(la), Segment::Line(lb)) = (a.seg, b.seg)
            && let Some(distance) = parallel_distance(la, lb)
        {
            out.readings.push(Reading::Gap { a: a.at, b: b.at, distance });
        }
    }
    // Joined lines met by the two axes: the angle at the end they share.
    let tol = s.pixel_size() * 1e-3;
    for kh in [0, 1] {
        for kv in [2, 3] {
            if let (Some(a), Some(b)) = (hit(kh), hit(kv))
                && let (Segment::Line(la), Segment::Line(lb)) = (a.seg, b.seg)
                && let Some((vertex, pa, pb)) = joined(la, lb, tol)
            {
                let degrees = super::vertex_angle(vertex, pa, pb);
                let reading = Reading::Angle { vertex, a: pa, b: pb, degrees, right: (degrees - 90.0).abs().to_radians() < RIGHT };
                if !out.readings.contains(&reading) {
                    out.readings.push(reading);
                }
            }
        }
    }
    out
}

/// The visible objects of the current space that may meet the cursor's axes: those whose bounds
/// touch a band `pad` wide along either axis, `reach` long each way from `c` (at most
/// [`MAX_OBJECTS`]). A large space is looked up in its spatial index, a small one scanned.
fn near_axes(s: &Session, c: Vec2, reach: f64, pad: f64) -> Vec<std::sync::Arc<Entity>> {
    let Ok(d) = s.doc() else { return Vec::new() };
    let space = s.space();
    let bands = [
        Bounds2::new(c - Vec2::new(reach, 0.0), c + Vec2::new(reach, 0.0)).expand(pad),
        Bounds2::new(c - Vec2::new(0.0, reach), c + Vec2::new(0.0, reach)).expand(pad),
    ];
    let touches = |b: &Bounds2| bands.iter().any(|band| band.intersects(b));
    let mut out = Vec::new();
    match crate::spatial::index(d, &space) {
        Some(ix) => {
            let mut found: Vec<u32> = bands.iter().flat_map(|b| ix.query(b, false).unwrap_or_default()).collect();
            found.sort_unstable();
            found.dedup();
            out.extend(found.into_iter().filter_map(|i| ix.entity(i)).filter(|e| d.is_visible(e)).take(MAX_OBJECTS).cloned());
        }
        None => {
            let Some(store) = d.space(&space) else { return out };
            for e in store.iter() {
                if out.len() >= MAX_OBJECTS {
                    break;
                }
                if d.is_visible(e) && touches(&cadcraft_doc::entity_bounds(d, e, 0)) {
                    out.push(e.clone());
                }
            }
        }
    }
    out
}

/// The distance between two parallel lines; `None` when they are not parallel.
fn parallel_distance(a: Line, b: Line) -> Option<f64> {
    let (ua, ub) = ((a.b - a.a).normalized(), (b.b - b.a).normalized());
    if !(ua.is_finite() && ub.is_finite()) || ua.cross(ub).abs() > PARALLEL {
        return None;
    }
    let distance = ua.cross(b.a - a.a).abs();
    (distance.is_finite() && distance > 0.0).then_some(distance)
}

/// The end two lines share (within `tol`) and the other end of each; `None` when they don't join.
fn joined(a: Line, b: Line, tol: f64) -> Option<(Vec2, Vec2, Vec2)> {
    for (va, oa) in [(a.a, a.b), (a.b, a.a)] {
        for (vb, ob) in [(b.a, b.b), (b.b, b.a)] {
            if va.near(vb, tol) && !oa.near(va, tol) && !ob.near(vb, tol) {
                return Some((va, oa, ob));
            }
        }
    }
    None
}

/// A closed area measured by a click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuickArea {
    /// The outer loop first, then the islands.
    pub loops: Vec<HatchLoop>,
    pub area: f64,
    pub perimeter: f64,
}

impl QuickArea {
    /// The area around `p`: inside its outer loop and outside its islands. `None` when `p` is in
    /// no closed area.
    pub(crate) fn at(s: &Session, p: Vec2) -> Option<QuickArea> {
        let mut loops = super::super::hatch::loops_at(s, p).ok()?;
        // An area bounded by one closed object is measured by its exact outline (a circle's
        // arcs, not the flattened outline the boundary was traced on).
        if let Ok(d) = s.doc()
            && let Some(outer) = loops.first_mut()
            && let [h] = outer.sources.as_slice()
            && let Some(vs) = d.entity(*h).and_then(|e| super::super::hatch::closed_loop(d, e))
        {
            outer.vertices = vs;
        }
        let poly = |l: &HatchLoop| Polyline { vertices: l.vertices.clone(), closed: true };
        let (mut area, mut perimeter) = (0.0, 0.0);
        for l in &loops {
            let a = poly(l).area().abs();
            area += if l.outer { a } else { -a };
            perimeter += poly(l).segments().iter().map(Segment::len).sum::<f64>();
        }
        (area.is_finite() && perimeter.is_finite() && area > 0.0).then(|| QuickArea { loops, area: area.max(0.0), perimeter })
    }
    /// Whether `p` lies in this area (inside its outer loop and in none of its islands).
    fn contains(&self, p: Vec2) -> bool {
        let inside = |l: &HatchLoop| cadcraft_geom::point_in_polygon(&Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(1e-6), p);
        self.loops.iter().all(|l| if l.outer { inside(l) } else { !inside(l) })
    }
}

/// The areas shown after a click at `p` in an area: that area alone, or with `shift` the areas
/// shown so far with it added (or taken away, when one of them was clicked again).
pub(crate) fn click(areas: &mut Vec<QuickArea>, s: &Session, p: Vec2, shift: bool) -> bool {
    if shift && let Some(i) = areas.iter().position(|a| a.contains(p)) {
        areas.remove(i);
        return true;
    }
    let Some(a) = QuickArea::at(s, p) else { return false };
    if !shift {
        areas.clear();
    }
    if areas.len() < MAX_AREAS {
        areas.push(a);
    }
    true
}

/// The area and perimeter of `areas` together.
pub(crate) fn totals(areas: &[QuickArea]) -> (f64, f64) {
    areas.iter().fold((0.0, 0.0), |(a, p), q| (a + q.area, p + q.perimeter))
}

/// The command-line lines for `areas`: each area, then the total when there are several.
fn area_lines(s: &Session, areas: &[QuickArea]) -> Vec<String> {
    let f = |v: f64| super::super::inquiry::fmt_d(s, v);
    let mut lines: Vec<String> = areas.iter().map(|a| format!("Area = {}, Perimeter = {}", f(a.area), f(a.perimeter))).collect();
    if areas.len() > 1 {
        let (a, p) = totals(areas);
        lines.push(format!("Total area = {}, Total perimeter = {}", f(a), f(p)));
    }
    lines
}

/// The readings as JSON (MEASUREGEOM `{mode: "quick"}`).
pub(crate) fn readout_json(r: &Readout) -> Value {
    let pt = |p: Vec2| json!([p.x, p.y]);
    let readings: Vec<Value> = r
        .readings
        .iter()
        .map(|m| match m {
            Reading::Length { handle, a, b, length } => {
                json!({ "type": "length", "handle": handle.hex(), "from": pt(*a), "to": pt(*b), "length": length })
            }
            Reading::Radius { handle, at, center, radius } => {
                json!({ "type": "radius", "handle": handle.hex(), "at": pt(*at), "center": pt(*center), "radius": radius })
            }
            Reading::Gap { a, b, distance } => json!({ "type": "gap", "from": pt(*a), "to": pt(*b), "distance": distance }),
            Reading::Angle { vertex, a, b, degrees, right } => {
                json!({ "type": "angle", "vertex": pt(*vertex), "from": pt(*a), "to": pt(*b), "degrees": degrees, "right": right })
            }
        })
        .collect();
    json!({ "axes": r.axes.iter().map(|(a, b)| json!([pt(*a), pt(*b)])).collect::<Vec<_>>(), "readings": readings })
}

/// MEASUREGEOM `{mode: "quick", at: [x, y]}` measures around a point; with `areas: [[x, y], ...]`
/// it measures the closed areas around those points together, each area once (a second point in
/// an area already measured adds nothing; Shift+click there would take it away).
pub(crate) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(pts) = super::points_param(p, "areas") {
        if pts.len() > MAX_AREAS {
            return Err(super::bad("measuregeom", format!("at most {MAX_AREAS} areas at a time")));
        }
        let mut areas: Vec<QuickArea> = Vec::new();
        for q in pts {
            if areas.iter().any(|a| a.contains(q)) {
                continue;
            }
            let a = QuickArea::at(s, q).ok_or_else(|| super::bad("measuregeom", format!("no closed area around {}, {}", q.x, q.y)))?;
            areas.push(a);
        }
        let (area, perimeter) = totals(&areas);
        s.area = area;
        s.perimeter = perimeter;
        let message = area_lines(s, &areas).join("\n");
        return Ok(
            json!({ "areas": areas.iter().map(|a| json!({ "area": a.area, "perimeter": a.perimeter })).collect::<Vec<_>>(), "area": area, "perimeter": perimeter, "message": message }),
        );
    }
    let at =
        super::point_param(p, "at").ok_or_else(|| super::bad("measuregeom", "quick needs `at` (a point) or `areas` (points in closed areas)"))?;
    Ok(readout_json(&readout(s, at)))
}

/// Quick at the command line: the readout follows the cursor; a click measures the area under
/// it, Shift+click adds or takes away an area; the other options switch to them.
#[derive(Default)]
pub(crate) struct QuickM {
    areas: Vec<QuickArea>,
}

/// The keywords of Quick's prompt, as MEASUREGEOM offers them while it measures.
pub(crate) const KEYWORDS: [&str; 7] = ["Distance", "Radius", "Angle", "ARea", "Volume", "Quick", "eXit"];

impl Interactive for QuickM {
    fn name(&self) -> &'static str {
        "MEASUREGEOM"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Move cursor", Accept::POINT).kw(&KEYWORDS).default("eXit")
    }
    /// A click measures the closed area under it (Shift: adds or takes it away); Enter or eXit
    /// ends.
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                // A snap may have moved the click onto an edge: the cursor, beside it, is still
                // inside.
                let shift = s.shift_down;
                let snapped = s.cursor != p && s.cursor.dist(p) <= super::super::curves::pick_aperture(s) * 2.0;
                if !click(&mut self.areas, s, p, shift) && !(snapped && click(&mut self.areas, s, s.cursor, shift)) {
                    s.echo("No closed area there.");
                    return Ok(Step::Continue);
                }
                let (area, perimeter) = totals(&self.areas);
                s.area = area;
                s.perimeter = perimeter;
                for l in area_lines(s, &self.areas) {
                    s.echo(l);
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k != "eXit" && k != "Quick" => Ok(Step::Continue),
            Input::Cancel => Ok(Step::Cancel),
            _ => Ok(Step::Done),
        }
    }
    /// The axes, the readings and the measured areas, in their own colours.
    fn preview_objects(&self, s: &Session, cursor: Vec2) -> Vec<Entity> {
        marks(s, &readout(s, cursor), &self.areas)
    }
}

/// Entities that draw `r` and shade `areas`, sized for the current view.
fn marks(s: &Session, r: &Readout, areas: &[QuickArea]) -> Vec<Entity> {
    let px = s.pixel_size();
    let mut common = s.current_common();
    common.color = Color::True(MARK);
    common.linetype = "Continuous".into();
    let mut shade = common.clone();
    shade.color = Color::True(AREA);
    let entity = |common: &cadcraft_doc::Common, kind: EntityKind| Entity { handle: Handle(0), common: common.clone(), kind };
    let stripes = s.doc().ok().map(|d| stripe_scale(d, px)).unwrap_or(1.0);
    let mut out: Vec<Entity> = areas.iter().map(|a| entity(&shade, EntityKind::Hatch(striped(a.loops.clone(), stripes)))).collect();
    let line = |a: Vec2, b: Vec2| super::super::helpers::line(a, b);
    for (a, b) in &r.axes {
        out.push(entity(&common, line(*a, *b)));
    }
    let f = |v: f64| super::super::inquiry::fmt_d(s, v);
    for m in &r.readings {
        match *m {
            Reading::Length { a, b, length, .. } => {
                out.push(entity(&common, line(a, b)));
                out.push(entity(&common, along(a, b, f(length), px)));
            }
            Reading::Radius { at, radius, .. } => {
                out.push(entity(&common, label(at + Vec2::new(1.0, 1.0) * px * LABEL_PX, format!("R {}", f(radius)), px, 0.0)))
            }
            Reading::Gap { a, b, distance } => {
                let tick = (b - a).perp().normalized() * px * SQUARE_PX / 2.0;
                out.push(entity(&common, line(a - tick, a + tick)));
                out.push(entity(&common, line(b - tick, b + tick)));
                out.push(entity(&common, along(a, b, f(distance), px)));
            }
            Reading::Angle { vertex, a, b, degrees, right } => {
                let (ua, ub) = ((a - vertex).normalized(), (b - vertex).normalized());
                if right {
                    let k = px * SQUARE_PX;
                    let corner = vec![vertex + ua * k, vertex + (ua + ub) * k, vertex + ub * k];
                    out.push(entity(&common, lwpoly(corner.into_iter().map(PolyVertex::new).collect(), false)));
                }
                let bisector = (ua + ub).normalized();
                let at = vertex + if bisector.is_finite() { bisector } else { ua.perp() } * px * LABEL_PX * 2.5;
                out.push(entity(&common, label(at, format!("{}°", trim(degrees)), px, 0.0)));
            }
        }
    }
    out
}

/// An angle in degrees with up to four decimals and no trailing zeros.
fn trim(v: f64) -> String {
    let t = format!("{v:.4}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A label for the line from `a` to `b`: along it, reading left to right (or upwards), just above
/// its middle, as a dimension's text sits, so it never crosses the line.
fn along(a: Vec2, b: Vec2, value: String, px: f64) -> EntityKind {
    let mut angle = (b - a).angle();
    if angle > std::f64::consts::FRAC_PI_2 + 1e-9 {
        angle -= std::f64::consts::PI;
    } else if angle <= -std::f64::consts::FRAC_PI_2 + 1e-9 {
        angle += std::f64::consts::PI;
    }
    let above = Vec2::from_angle(angle).perp() * px * LABEL_PX * 0.9;
    label(a.mid(b) + above, value, px, angle)
}

/// A label centred on `at`, `LABEL_PX` pixels high, turned by `rotation` (radians).
fn label(at: Vec2, value: String, px: f64, rotation: f64) -> EntityKind {
    EntityKind::Text(Text {
        insert: at.to3(0.0),
        align_pt: Some(at.to3(0.0)),
        height: px * LABEL_PX,
        value,
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Center,
        valign: VAlign::Middle,
    })
}

/// The scale at which the diagonal stripes that shade a measured area lie [`STRIPE_PX`] pixels
/// apart (previews are drawn opaque, so a solid fill would hide what lies in the area).
fn stripe_scale(d: &cadcraft_doc::Drawing, px: f64) -> f64 {
    let spacing = cadcraft_doc::library::pattern_for(d, STRIPES)
        .and_then(|p| p.lines.first().map(|l| l.delta.1.abs()))
        .filter(|s| s.is_normal())
        .unwrap_or(0.125);
    let scale = px * STRIPE_PX / spacing;
    if scale.is_normal() { scale } else { 1.0 }
}

/// Diagonal stripes over `loops`, at `scale`.
fn striped(loops: Vec<HatchLoop>, scale: f64) -> Hatch {
    Hatch {
        pattern: STRIPES.into(),
        solid: false,
        loops,
        scale,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    }
}
