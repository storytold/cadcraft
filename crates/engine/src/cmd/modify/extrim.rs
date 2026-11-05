//! EXTRIM (Express Tools): every object that crosses one cutting edge loses its part on the side
//! picked. The edge is traced a hair's breadth away on that side, and each object that crossing
//! line meets is trimmed there, with the picked object as the only cutting edge. Objects that don't
//! cross the edge, and the edge itself, stay as they are.

use std::cell::RefCell;

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, entity_bounds};
use cadcraft_geom::{Bounds2, Line, Vec2, line_line, point_in_polygon};
use serde_json::{Value, json};

use super::offset::offset_points;
use super::{Edges, trim};
use crate::cmd::helpers::lwpoly;
use crate::cmd::{CommandSpec, bad, point_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most points of the line traced along the edge.
const MAX_FENCE: usize = 10_000;
/// Most objects one call trims.
const MAX_OBJECTS: usize = 10_000;
/// Most stretches one call removes (an object can lose several).
const MAX_STRETCHES: usize = 10_000;
/// Most objects the preview draws (beyond that it shows nothing, to keep the cursor smooth).
const MAX_PREVIEW: usize = 2000;
/// The traced line's distance from the edge, as a fraction of the edge's size.
const TRACE_GAP: f64 = 1e-5;

pub(super) fn spec() -> CommandSpec {
    CommandSpec::new("extrim", "Extended Trim", run_extrim)
        .menu(&["Tools", "Express Tools", "Extended Trim"])
        .params("{edge: hex (line, arc, circle, ellipse, polyline or spline), side: [x,y] (inside or outside a closed edge, an arc's circle or an elliptical arc's ellipse; one side of an open edge)} → trimmed, handles, skipped: {locked, other, uncut}")
        .interactive(|s| Ok(Box::new(ExtrimM::new(s))))
}

/// Whether objects of this kind can be the cutting edge.
fn edge_kind(k: &EntityKind) -> bool {
    matches!(
        k,
        EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::Circle(_) | EntityKind::Ellipse(_) | EntityKind::LwPolyline(_) | EntityKind::Spline(_)
    )
}

/// Whether TRIM can cut objects of this kind.
fn trimmable(k: &EntityKind) -> bool {
    edge_kind(k)
}

/// Whether the object closes on itself, from what it is rather than from where its ends lie: an
/// open polyline drawn back to its start is still open.
fn closed_kind(k: &EntityKind) -> bool {
    match k {
        EntityKind::Circle(_) => true,
        EntityKind::LwPolyline(p) => p.closed,
        EntityKind::Ellipse(e) => {
            cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end }.is_full()
        }
        EntityKind::Spline(sp) => sp.closed,
        _ => false,
    }
}

/// Consecutive polylines joined end to end (an object's primitives come in drawing order).
fn chains(polys: Vec<Vec<Vec2>>, tol: f64) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    for p in polys.into_iter().filter(|p| p.len() >= 2) {
        match out.last_mut() {
            Some(cur) if cur.last().zip(p.first()).is_some_and(|(a, b)| a.near(*b, tol)) => cur.extend(p.into_iter().skip(1)),
            _ => out.push(p),
        }
    }
    out
}

/// Twice the signed area of a closed polyline (positive: counter-clockwise).
fn signed_area(pts: &[Vec2]) -> f64 {
    pts.windows(2).filter_map(|w| Some(w.first()?.cross(*w.get(1)?))).sum()
}

/// Which side of the edge's direction the trace runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

impl Side {
    /// The trace's offset sign along the edge's left normal.
    fn factor(self) -> f64 {
        match self {
            Side::Left => 1.0,
            Side::Right => -1.0,
        }
    }
}

/// The full circle or ellipse an arc or elliptical arc lies on: inside and outside it, as OFFSET
/// takes them, decide the side even for a pick past the arc's ends.
#[derive(Clone, Copy)]
enum Round {
    Circle { center: Vec2, radius: f64 },
    Ellipse { center: Vec2, major: Vec2, ratio: f64 },
}

impl Round {
    fn of(k: &EntityKind) -> Option<Round> {
        match k {
            EntityKind::Arc(a) => Some(Round::Circle { center: a.center.xy(), radius: a.radius }),
            EntityKind::Ellipse(e) => Some(Round::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio }),
            _ => None,
        }
    }
    fn center(self) -> Vec2 {
        match self {
            Round::Circle { center, .. } | Round::Ellipse { center, .. } => center,
        }
    }
    fn inside(self, p: Vec2) -> bool {
        match self {
            Round::Circle { center, radius } => p.dist(center) < radius,
            Round::Ellipse { center, major, ratio } => {
                let a = major.len();
                let b = a * ratio.abs();
                let u = major.normalized();
                let d = p - center;
                let (x, y) = (d.dot(u) * b, d.dot(u.perp()) * a);
                x * x + y * y < (a * b) * (a * b)
            }
        }
    }
}

/// The cutting edge traced along, with the scale of the trim.
#[derive(Clone)]
struct Edge {
    handle: Handle,
    pts: Vec<Vec2>,
    closed: bool,
    /// An arc's circle (an elliptical arc's ellipse), and whether its inside is to the left of
    /// the edge's direction.
    round: Option<(Round, bool)>,
    /// Distance of the traced line from the edge.
    gap: f64,
}

impl Edge {
    /// The cutting edge `h`: a line, arc, circle, ellipse, polyline or spline of the current space.
    fn new(s: &Session, h: Handle) -> Result<Edge> {
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad("extrim", format!("no object {}", h.hex())))?;
        if !edge_kind(&e.kind) {
            return Err(bad("extrim", "the cutting edge must be a line, arc, circle, ellipse, polyline or spline"));
        }
        if d.space_of(h).as_ref() != Some(&s.space()) || !d.is_visible(e) {
            return Err(bad("extrim", format!("object {} is not a visible object of the current space", h.hex())));
        }
        let b = entity_bounds(d, e, 0);
        let size = b.width() + b.height();
        if !size.is_finite() || size <= 0.0 {
            return Err(bad("extrim", "the cutting edge has no length"));
        }
        let closed = closed_kind(&e.kind);
        // A finer trace for a small edge; a coarser one when a long or curved edge would need too
        // many points. The trace stays within a twentieth of the gap of the true edge, so the
        // traced line never strays to the other side.
        let mut gap = (size * TRACE_GAP).max(1e-9);
        for _ in 0..6 {
            let Some(pts) = chains(crate::select::hit_polylines(d, e, gap / 20.0), gap / 20.0).into_iter().next() else { break };
            if pts.len() <= MAX_FENCE {
                // The centre lies on the inside of the arc's bend.
                let mid = (pts.len() / 2).max(1);
                let inside_left = |r: Round| match (pts.get(mid - 1), pts.get(mid)) {
                    (Some(a), Some(b)) => (*b - *a).cross(r.center() - *a) > 0.0,
                    _ => true,
                };
                let round = if closed { None } else { Round::of(&e.kind) }.map(|r| (r, inside_left(r)));
                return Ok(Edge { handle: h, pts, closed, round, gap });
            }
            gap *= 4.0;
        }
        Err(bad("extrim", "the cutting edge is too complex"))
    }

    /// Which side the trace runs on for a pick at `p`; `None` for a point on the edge.
    fn side(&self, p: Vec2) -> Option<Side> {
        if !p.is_finite() {
            return None;
        }
        let segs = || self.pts.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?)));
        let (near, dist) = segs().map(|l| (l, l.dist(p))).min_by(|a, b| a.1.total_cmp(&b.1))?;
        if dist <= self.gap {
            return None;
        }
        let left = match self.round {
            _ if self.closed => {
                // Inside a counter-clockwise outline is to its left.
                point_in_polygon(&self.pts, p) == (signed_area(&self.pts) > 0.0)
            }
            Some((round, inside_left)) => round.inside(p) == inside_left,
            None => (near.b - near.a).cross(p - near.closest(p)) > 0.0,
        };
        Some(if left { Side::Left } else { Side::Right })
    }

    /// The line traced beside the edge on `side`.
    fn trace(&self, side: Side) -> Vec<Vec2> {
        offset_points(&self.pts, self.gap * side.factor(), self.closed)
    }
}

/// One stretch of an object the trim removes: where it is picked, and its outline.
struct Stretch {
    pick: Vec2,
    parts: Vec<Vec<Vec2>>,
}

/// Where the trim would cut: each object the trace crosses, with the stretches it loses, and
/// what it leaves alone.
#[derive(Default)]
struct Plan {
    work: Vec<(Handle, Vec<Stretch>)>,
    locked: usize,
    other: usize,
    /// Objects the edge crosses but no trim can cut there (a circle crossed once).
    uncut: Vec<Handle>,
}

/// Segments of `pts` with their bounds, bucketed in a grid so crossings are found without
/// testing every pair.
struct Grid {
    segs: Vec<(Line, Bounds2)>,
    bounds: Bounds2,
    n: usize,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(pts: &[Vec2]) -> Grid {
        let segs: Vec<(Line, Bounds2)> =
            pts.windows(2).filter_map(|w| Some((Line::new(*w.first()?, *w.get(1)?), Bounds2::from_points([*w.first()?, *w.get(1)?])))).collect();
        let bounds = Bounds2::from_points(pts.iter().copied());
        // About as many cells as segments.
        let n = ((segs.len() as f64).sqrt().ceil() as usize).clamp(1, 128);
        let mut g = Grid { segs: Vec::new(), bounds, n, cells: vec![Vec::new(); n * n] };
        for (i, (_, b)) in segs.iter().enumerate() {
            for c in g.cells_of(b) {
                if let Some(cell) = g.cells.get_mut(c) {
                    cell.push(i);
                }
            }
        }
        g.segs = segs;
        g
    }
    /// The cells `b` overlaps.
    fn cells_of(&self, b: &Bounds2) -> Vec<usize> {
        let w = self.bounds.width().max(1e-12);
        let h = self.bounds.height().max(1e-12);
        let n = self.n;
        let col = |x: f64| ((((x - self.bounds.min.x) / w) * n as f64).floor().max(0.0) as usize).min(n - 1);
        let row = |y: f64| ((((y - self.bounds.min.y) / h) * n as f64).floor().max(0.0) as usize).min(n - 1);
        let (c0, c1, r0, r1) = (col(b.min.x), col(b.max.x), row(b.min.y), row(b.max.y));
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| r * n + c)).collect()
    }
    /// Where segment `l` crosses the grid's segments.
    fn crossings(&self, l: &Line) -> Vec<Vec2> {
        let lb = Bounds2::from_points([l.a, l.b]);
        if !lb.intersects(&self.bounds) {
            return Vec::new();
        }
        let mut seen = std::collections::HashSet::new();
        self.cells_of(&lb)
            .into_iter()
            .filter_map(|c| self.cells.get(c))
            .flatten()
            .filter(|i| seen.insert(**i))
            .filter_map(|i| self.segs.get(*i))
            .filter(|(_, b)| b.intersects(&lb))
            .filter_map(|(s, _)| line_line(l, s))
            .collect()
    }
}

/// An object outline measured along its length, for cutting stretches out of it.
struct Outline<'a> {
    pts: &'a [Vec2],
    /// Distance along the outline to each point.
    at: Vec<f64>,
}

impl<'a> Outline<'a> {
    fn new(pts: &'a [Vec2]) -> Outline<'a> {
        let mut at = Vec::with_capacity(pts.len());
        let mut t = 0.0;
        at.push(t);
        for w in pts.windows(2) {
            if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                t += a.dist(*b);
            }
            at.push(t);
        }
        Outline { pts, at }
    }
    fn total(&self) -> f64 {
        self.at.last().copied().unwrap_or(0.0)
    }
    /// The distances along the outline where it crosses `grid`, sorted, each once.
    fn crossings(&self, grid: &Grid) -> Vec<f64> {
        let mut out = Vec::new();
        for (i, w) in self.pts.windows(2).enumerate() {
            let (Some(a), Some(b), Some(t0)) = (w.first(), w.get(1), self.at.get(i)) else { continue };
            out.extend(grid.crossings(&Line::new(*a, *b)).into_iter().map(|c| t0 + a.dist(c)));
        }
        out.sort_by(f64::total_cmp);
        // A crossing at a vertex shows up on both segments that meet there.
        let eps = self.total() * 1e-9;
        out.dedup_by(|a, b| (*a - *b).abs() <= eps);
        out
    }
    /// The part of the outline between distances `from` and `to`.
    fn part(&self, from: f64, to: f64) -> Vec<Vec2> {
        let mut out = Vec::new();
        let first = self.at.partition_point(|t| *t < from).saturating_sub(1);
        for i in first..self.pts.len().saturating_sub(1) {
            let (Some(a), Some(b), Some(t0), Some(t1)) = (self.pts.get(i), self.pts.get(i + 1), self.at.get(i), self.at.get(i + 1)) else { break };
            if *t0 > to {
                break;
            }
            if *t1 < from || t1 <= t0 {
                continue;
            }
            let p = |t: f64| *a + (*b - *a) * ((t.clamp(*t0, *t1) - t0) / (t1 - t0));
            if out.is_empty() {
                out.push(p(from));
            }
            out.push(p(to.min(*t1)));
        }
        out
    }
}

/// The stretches of one object outline the trim removes on the traced side: between the edge
/// crossings either side of each place the trace meets the outline. Every crossing is found once.
/// `None` for a closed outline the edge crosses only once, which no trim can open.
fn stretches_of(pts: &[Vec2], closed: bool, edge: &Grid, trace: &Grid) -> Option<Vec<Vec<Vec<Vec2>>>> {
    let o = Outline::new(pts);
    let total = o.total();
    let eps = total * 1e-9;
    let mut cuts = o.crossings(edge);
    if closed {
        // The seam: the outline's start and end are one point, so a crossing there is one cut.
        for t in &mut cuts {
            if *t >= total - eps {
                *t = 0.0;
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() <= eps);
    } else {
        // An object that only touches the edge with an end isn't cut there, as TRIM has it.
        cuts.retain(|t| *t > eps && *t < total - eps);
    }
    let hits = o.crossings(trace);
    if cuts.is_empty() || hits.is_empty() {
        return Some(Vec::new());
    }
    if closed && cuts.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    // Each stretch once, however many times the trace meets it.
    let mut done = std::collections::HashSet::new();
    for tp in hits {
        // The edge crossings either side, as indices into `cuts`.
        let k = cuts.partition_point(|t| *t < tp);
        let lo = k.checked_sub(1);
        let hi = (k < cuts.len()).then_some(k);
        let (lo, hi) = if closed { (lo.or(cuts.len().checked_sub(1)), hi.or(Some(0))) } else { (lo, hi) };
        if !done.insert((lo, hi)) {
            continue;
        }
        let at = |i: Option<usize>| i.and_then(|i| cuts.get(i).copied());
        out.push(match (at(lo), at(hi)) {
            // Across the seam of a closed outline.
            (Some(a), Some(b)) if closed && a >= b => vec![o.part(a, total), o.part(0.0, b)],
            (Some(a), Some(b)) => vec![o.part(a, b)],
            (None, Some(b)) => vec![o.part(0.0, b)],
            (Some(a), None) => vec![o.part(a, total)],
            (None, None) => continue,
        });
    }
    Some(out)
}

/// The trim of everything crossing `edge` on `side`, worked out without changing anything.
fn plan(d: &Drawing, space: &cadcraft_doc::Space, edge: &Edge, side: Side) -> Result<Plan> {
    let trace = edge.trace(side);
    let trace_grid = Grid::new(&trace);
    let edge_grid = Grid::new(&edge.pts);
    let tol = edge.gap / 20.0;
    let mut p = Plan::default();
    let mut stretches_total = 0;
    for h in crate::select::select_fence(d, space, &trace) {
        if h == edge.handle {
            continue;
        }
        let Some(e) = d.entity(h) else { continue };
        if !trimmable(&e.kind) {
            p.other += 1;
            continue;
        }
        if d.layer(&e.common.layer).is_some_and(|l| l.locked) || crate::cmd::blocks::refedit::locked(d, h) {
            p.locked += 1;
            continue;
        }
        let outlines = chains(crate::select::hit_polylines(d, e, tol), tol);
        // A closed object is one closed outline.
        let closed = closed_kind(&e.kind) && outlines.len() == 1;
        let mut stretches: Vec<Stretch> = Vec::new();
        for pts in &outlines {
            let Some(parts) = stretches_of(pts, closed, &edge_grid, &trace_grid) else {
                p.uncut.push(h);
                continue;
            };
            for parts in parts {
                // Trimmed at the middle of the stretch, well clear of the edge, so the cut
                // doesn't depend on how closely the trim follows a curved edge.
                let Some(pick) = parts.iter().max_by(|a, b| arc_length(a).total_cmp(&arc_length(b))).and_then(|q| midpoint(q)) else { continue };
                stretches.push(Stretch { pick, parts });
            }
        }
        stretches_total += stretches.len();
        if !stretches.is_empty() {
            p.work.push((h, stretches));
        }
        if p.work.len() > MAX_OBJECTS {
            return Err(bad("extrim", format!("more than {MAX_OBJECTS} objects cross the cutting edge; trim a smaller area")));
        }
        if stretches_total > MAX_STRETCHES {
            return Err(bad("extrim", format!("more than {MAX_STRETCHES} stretches would be trimmed; trim a smaller area")));
        }
    }
    Ok(p)
}

fn arc_length(pts: &[Vec2]) -> f64 {
    pts.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).sum()
}

/// The point halfway along `pts`.
fn midpoint(pts: &[Vec2]) -> Option<Vec2> {
    let mut left = arc_length(pts) / 2.0;
    for w in pts.windows(2) {
        let (a, b) = (*w.first()?, *w.get(1)?);
        let l = a.dist(b);
        if l >= left && l > 0.0 {
            return Some(a + (b - a) * (left / l));
        }
        left -= l;
    }
    pts.last().copied()
}

/// The length of an object's drawn outline.
fn length(d: &Drawing, e: &Entity, tol: f64) -> f64 {
    crate::select::hit_polylines(d, e, tol).iter().flat_map(|pl| pl.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?)))).sum()
}

pub(super) fn run_extrim(s: &mut Session, p: &Value) -> Result<Value> {
    let h = match p.get("edge") {
        Some(Value::String(t)) => {
            Handle::parse_hex(t).ok_or_else(|| bad("extrim", "`edge` must be an object handle (a hex string such as \"1A2\")"))?
        }
        // One cutting edge: several objects would leave which one cuts to chance.
        None | Some(Value::Null) => match targets(s, p)?.as_slice() {
            [] => return Err(bad("extrim", "`edge` is required")),
            [h] => *h,
            _ => return Err(bad("extrim", "give exactly one cutting edge")),
        },
        Some(_) => return Err(bad("extrim", "`edge` must be an object handle (a hex string such as \"1A2\")")),
    };
    let point = point_param(p, "side").ok_or_else(|| bad("extrim", "`side` ([x, y]) is required"))?;
    let edge = Edge::new(s, h)?;
    let side = edge.side(point).ok_or_else(|| bad("extrim", "pick the side away from the cutting edge"))?;
    trim_side(s, &edge, side)
}

/// Trim every object crossing `edge` on `side`: one undo step, everything checked first.
fn trim_side(s: &mut Session, edge: &Edge, side: Side) -> Result<Value> {
    let space = s.space();
    let plan = plan(s.doc()?, &space, edge, side)?;
    let edges = Edges { only: Some(std::slice::from_ref(&edge.handle)), implied: false };
    let tol = edge.gap / 20.0;
    let mut trimmed = 0;
    let mut created = Vec::new();
    // Objects some planned cut failed on, reported rather than counted as done.
    let mut uncut = plan.uncut;
    for (h, stretches) in plan.work {
        let mut live = vec![h];
        let mut cut = false;
        for x in stretches.into_iter().map(|st| st.pick) {
            // A trim replaces the object with pieces: the pick goes to the piece it lies on (none:
            // that part is already gone). The pick is on the outline traced within `tol`, so 3 tol
            // allows for that and for rounding, and is far less than the distance to any other
            // piece, since a stretch's middle is well away from the edge where pieces meet.
            let target = {
                let d = s.doc()?;
                live.iter()
                    .filter_map(|l| d.entity(*l).map(|e| (*l, crate::select::entity_distance(d, e, x, tol))))
                    .filter(|(_, dist)| *dist <= 3.0 * tol)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(l, _)| l)
            };
            let Some(t) = target else { continue };
            let Ok(pieces) = trim(s, t, x, edges) else {
                if !uncut.contains(&h) {
                    uncut.push(h);
                }
                continue;
            };
            cut = true;
            live.retain(|l| *l != t);
            for n in pieces {
                // Never leave a piece of no length behind.
                let d = s.doc()?;
                if d.entity(n).is_some_and(|e| length(d, e, tol) <= tol) {
                    s.doc_mut()?.remove_entity(n);
                } else {
                    live.push(n);
                }
            }
        }
        if cut {
            trimmed += 1;
            created.extend(live.into_iter().filter(|l| *l != h));
        }
    }
    let mut message = format!("{trimmed} object(s) trimmed.");
    if plan.locked > 0 {
        message.push_str(&format!(" {} on locked layers left as they are.", plan.locked));
    }
    if plan.other > 0 {
        message.push_str(&format!(" {} that can't be trimmed (blocks, hatches, text…) left as they are.", plan.other));
    }
    if !uncut.is_empty() {
        message.push_str(&format!(" {} could not be cut there and left as they are.", uncut.len()));
    }
    Ok(json!({
        "trimmed": trimmed,
        "handles": created.iter().map(|h| h.hex()).collect::<Vec<_>>(),
        "skipped": { "locked": plan.locked, "other": plan.other, "uncut": uncut.len() },
        "message": message,
    }))
}

/// The parts the trim would remove, drawn red.
fn preview(d: &Drawing, space: &cadcraft_doc::Space, edge: &Edge, side: Side) -> Vec<Entity> {
    let Ok(plan) = plan(d, space, edge, side) else { return Vec::new() };
    if plan.work.len() > MAX_PREVIEW {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (h, stretches) in plan.work {
        let Some(e) = d.entity(h) else { continue };
        for part in stretches.into_iter().flat_map(|st| st.parts).filter(|p| p.len() >= 2) {
            let mut ent = Entity::new(Handle(0), lwpoly(part.into_iter().map(cadcraft_geom::PolyVertex::new).collect(), false));
            ent.common.color = Color::Index(1);
            ent.common.layer = e.common.layer.clone();
            out.push(ent);
        }
    }
    out
}

/// EXTRIM at the prompts: the cutting edge (or the one object selected beforehand), then the side,
/// with the parts that will go shown red as the cursor moves.
pub(super) struct ExtrimM {
    edge: Option<Edge>,
    /// The preview for each side (left, right), worked out once.
    previews: RefCell<[Option<Vec<Entity>>; 2]>,
}

impl ExtrimM {
    fn new(_s: &Session) -> Self {
        ExtrimM { edge: None, previews: RefCell::new([None, None]) }
    }
    fn set_edge(&mut self, s: &mut Session, h: Handle) -> bool {
        match Edge::new(s, h) {
            Ok(e) => {
                self.edge = Some(e);
                self.previews = RefCell::new([None, None]);
                true
            }
            Err(e) => {
                s.echo(e.to_string());
                false
            }
        }
    }
}

impl Interactive for ExtrimM {
    fn name(&self) -> &'static str {
        "EXTRIM"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // One object selected beforehand is the cutting edge.
        if s.settings.pickfirst
            && let [h] = s.selection().as_slice()
            && s.doc().ok().and_then(|d| d.entity(*h)).is_some_and(|e| edge_kind(&e.kind))
        {
            let h = *h;
            self.set_edge(s, h);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.edge {
            None => Prompt::new("Select the cutting edge (line, arc, circle, ellipse, polyline or spline)", Accept::SELECT),
            Some(_) => Prompt::new("Specify the side to trim on", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.edge, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (_, Input::Enter) => Ok(Step::Done),
            (None, Input::Pick(hs)) => {
                match hs.as_slice() {
                    [h] => {
                        self.set_edge(s, *h);
                    }
                    [] => s.echo("No object found."),
                    _ => s.echo("Select exactly one cutting edge."),
                }
                Ok(Step::Continue)
            }
            (Some(edge), Input::Point(p)) => {
                let Some(side) = edge.side(p) else {
                    s.echo("Pick the side away from the cutting edge.");
                    return Ok(Step::Continue);
                };
                let edge = edge.clone();
                s.set_selection(Vec::new());
                match trim_side(s, &edge, side) {
                    Ok(v) => s.echo(v.get("message").and_then(Value::as_str).unwrap_or_default().to_string()),
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview_objects(&self, s: &Session, cursor: Vec2) -> Vec<Entity> {
        let Some(edge) = &self.edge else { return Vec::new() };
        let Some(side) = edge.side(cursor) else { return Vec::new() };
        let slot = usize::from(side == Side::Right);
        let cached = self.previews.borrow().get(slot).cloned().flatten();
        let mut v = match cached {
            Some(v) => v,
            None => {
                let Ok(d) = s.doc() else { return Vec::new() };
                let v = preview(d, &s.space(), edge, side);
                if let Some(c) = self.previews.borrow_mut().get_mut(slot) {
                    *c = Some(v.clone());
                }
                v
            }
        };
        // A band a few pixels wide at the current zoom, so it shows over the object it covers.
        let width = 3.0 * s.pixel_size();
        for e in &mut v {
            if let EntityKind::LwPolyline(p) = &mut e.kind {
                p.const_width = width;
            }
        }
        v
    }
}
