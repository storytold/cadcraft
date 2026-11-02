//! Region booleans (UNION, SUBTRACT, INTERSECT) and MASSPROP.
//!
//! The booleans handle regions whose boundaries don't cross or touch: each loop then lies wholly
//! inside or outside the other region, and the result keeps the loops that bound it (union: the
//! loops outside the other region; subtract: the first region's loops outside the second and the
//! second's inside the first; intersect: the loops inside the other region). Overlapping
//! boundaries need curve splitting and report "not available yet".

use cadcraft_doc::{EntityKind, Handle, HatchLoop, MAX_REGION_LOOPS, Region};
use cadcraft_geom::{Line, Vec2, line_line, point_in_polygon};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase, SelectRun};
use super::super::{CommandSpec, bad, targets};

use super::SelfCheck;
use crate::{EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// The region commands: UNION, SUBTRACT and INTERSECT combine regions, MASSPROP measures them.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("union", "Union", run_union)
            .menu(&["Modify", "Solid Editing", "Union"])
            .alias(&["uni"])
            .params("{handles?} (regions)")
            .interactive(|_| Ok(Box::new(SelectRun::new("union", "UNION")))),
        CommandSpec::new("subtract", "Subtract", run_subtract)
            .menu(&["Modify", "Solid Editing", "Subtract"])
            .alias(&["su"])
            .params("{from: [handles], remove: [handles]} (regions)")
            .interactive(|_| Ok(Box::new(SubtractM::default()))),
        CommandSpec::new("intersect", "Intersect", run_intersect)
            .menu(&["Modify", "Solid Editing", "Intersect"])
            .alias(&["in"])
            .params("{handles?} (regions)")
            .interactive(|_| Ok(Box::new(SelectRun::new("intersect", "INTERSECT")))),
        CommandSpec::new("massprop", "Region/Mass Properties", run_massprop)
            .menu(&["Tools", "Inquiry", "Region/Mass Properties"])
            .params("{handles?} (regions) → properties of each, plus `selfCheck` (found/analyzed/skipped with reasons, caps, implausible values)")
            .noundo()
            .interactive(|_| Ok(Box::new(SelectRun::new("massprop", "MASSPROP")))),
    ]
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// Most segment pairs compared when checking two regions for crossing boundaries.
const MAX_PAIRS: usize = 50_000_000;

/// The regions among `hs` (others are left alone).
fn regions(s: &Session, hs: &[Handle]) -> Result<Vec<(Handle, Region)>> {
    let d = s.doc()?;
    Ok(hs
        .iter()
        .filter_map(|h| match &d.entity(*h)?.kind {
            EntityKind::Region(r) => Some((*h, r.clone())),
            _ => None,
        })
        .collect())
}

/// Each loop of a region as points, with a chord tolerance for both regions' size.
fn tess(r: &Region, tol: f64) -> Vec<Vec<Vec2>> {
    r.tessellate(tol)
}

/// A point on a loop's boundary (its first edge's middle).
fn probe(l: &[Vec2]) -> Option<Vec2> {
    let n = l.len();
    (0..n).find_map(|i| {
        let (a, b) = (*l.get(i)?, *l.get((i + 1) % n)?);
        (a.dist(b) > 1e-12).then(|| (a + b) * 0.5)
    })
}

/// Whether two sets of loops have boundaries that cross, touch or overlap.
fn boundaries_meet(a: &[Vec<Vec2>], b: &[Vec<Vec2>], tol: f64) -> Result<bool> {
    let edges = |ls: &[Vec<Vec2>]| -> Vec<Line> {
        ls.iter().flat_map(|l| (0..l.len()).filter_map(move |i| Some(Line::new(*l.get(i)?, *l.get((i + 1) % l.len())?)))).collect()
    };
    let (ea, eb) = (edges(a), edges(b));
    if ea.len().saturating_mul(eb.len()) > MAX_PAIRS {
        return Err(other("These regions are too complex to combine."));
    }
    for x in &ea {
        for y in &eb {
            if line_line(x, y).is_some() || x.dist(y.a) < tol || y.dist(x.a) < tol {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Inside the area of loops `ls` (odd parity).
fn inside(ls: &[Vec<Vec2>], p: Vec2) -> bool {
    ls.iter().filter(|l| point_in_polygon(l, p)).count() % 2 == 1
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Union,
    Subtract,
    Intersect,
}

/// `a` op `b` for regions whose boundaries don't meet; `None` when they do (overlapping
/// regions are not available yet).
fn combine(a: &Region, b: &Region, op: Op) -> Result<Option<Region>> {
    let size = a.bounds().union(&b.bounds());
    let tol = ((size.width() + size.height()) * 1e-6).max(1e-12);
    let (ta, tb) = (tess(a, tol), tess(b, tol));
    if boundaries_meet(&ta, &tb, tol * 10.0)? {
        return Ok(None);
    }
    let keep = |loops: &[HatchLoop], pts: &[Vec<Vec2>], other: &[Vec<Vec2>], want_inside: bool| -> Vec<HatchLoop> {
        loops.iter().zip(pts).filter(|(_, p)| probe(p).is_some_and(|q| inside(other, q) == want_inside)).map(|(l, _)| l.clone()).collect()
    };
    let mut loops = match op {
        Op::Union => [keep(&a.loops, &ta, &tb, false), keep(&b.loops, &tb, &ta, false)].concat(),
        Op::Subtract => [keep(&a.loops, &ta, &tb, false), keep(&b.loops, &tb, &ta, true)].concat(),
        Op::Intersect => [keep(&a.loops, &ta, &tb, true), keep(&b.loops, &tb, &ta, true)].concat(),
    };
    if loops.len() > MAX_REGION_LOOPS {
        return Err(other("Too many loops."));
    }
    loops.retain(|l| l.vertices.len() >= 2);
    let mut r = Region { loops };
    r.renest();
    Ok(Some(r))
}

const OVERLAP: &str = "Combining regions whose boundaries overlap is not available yet.";

/// Replace the first region with `r` (or erase it when empty) and erase the others.
fn apply(s: &mut Session, keep: Handle, r: Region, erase: &[Handle]) -> Result<()> {
    let d = s.doc_mut()?;
    for h in erase.iter().filter(|h| **h != keep) {
        d.remove_entity(*h);
    }
    if r.loops.is_empty() {
        d.remove_entity(keep);
    } else {
        d.modify_entity(keep, |e| e.kind = EntityKind::Region(r))?;
    }
    Ok(())
}

fn fold(s: &mut Session, hs: &[Handle], op: Op) -> Result<Value> {
    let rs = regions(s, hs)?;
    let Some(((h0, first), rest)) = rs.split_first() else { return Err(other("Select at least one region.")) };
    let mut acc = first.clone();
    for (_, r) in rest {
        acc = combine(&acc, r, op)?.ok_or_else(|| other(OVERLAP))?;
    }
    let all: Vec<Handle> = rs.iter().map(|(h, _)| *h).collect();
    let empty = acc.loops.is_empty();
    apply(s, *h0, acc, &all)?;
    Ok(
        json!({ "handle": if empty { Value::Null } else { json!(h0.hex()) }, "message": if empty { "The result is empty: the regions were erased." } else { "" } }),
    )
}

fn run_union(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    fold(s, &hs, Op::Union)
}

fn run_intersect(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    fold(s, &hs, Op::Intersect)
}

fn handles(p: &Value, key: &str) -> Result<Vec<Handle>> {
    let a = p.get(key).and_then(Value::as_array).ok_or_else(|| bad("subtract", format!("`{key}` (array of handles) is required")))?;
    a.iter().map(|v| super::super::curves::handle_value(v).ok_or_else(|| bad("subtract", format!("`{key}`: {v} is not a handle")))).collect()
}

fn subtract(s: &mut Session, from: &[Handle], remove: &[Handle]) -> Result<Value> {
    let (a, b) = (regions(s, from)?, regions(s, remove)?);
    let Some(((h0, first), rest)) = a.split_first() else { return Err(other("Select regions to subtract from.")) };
    let mut acc = first.clone();
    for (_, r) in rest {
        acc = combine(&acc, r, Op::Union)?.ok_or_else(|| other(OVERLAP))?;
    }
    for (h, r) in &b {
        if a.iter().any(|(x, _)| x == h) {
            return Err(other("A region can't be subtracted from itself."));
        }
        acc = combine(&acc, r, Op::Subtract)?.ok_or_else(|| other(OVERLAP))?;
    }
    let all: Vec<Handle> = a.iter().chain(&b).map(|(h, _)| *h).collect();
    let empty = acc.loops.is_empty();
    apply(s, *h0, acc, &all)?;
    Ok(
        json!({ "handle": if empty { Value::Null } else { json!(h0.hex()) }, "message": if empty { "The result is empty: the regions were erased." } else { "" } }),
    )
}

fn run_subtract(s: &mut Session, p: &Value) -> Result<Value> {
    let (from, remove) = (handles(p, "from")?, handles(p, "remove")?);
    subtract(s, &from, &remove)
}

#[derive(Default)]
struct SubtractM {
    sel: SelectPhase,
    from: Option<Vec<Handle>>,
}

impl Interactive for SubtractM {
    fn name(&self) -> &'static str {
        "SUBTRACT"
    }
    /// Tell the user what to select first: the regions to subtract from.
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo("Select regions to subtract from ..");
        Ok(Step::Continue)
    }
    /// Prompt for the current selection phase (regions to subtract from, then regions to remove).
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    /// The first selection is the regions to subtract from; the second is subtracted from them.
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => match self.from.take() {
                None => {
                    self.from = Some(hs);
                    self.sel = SelectPhase::default();
                    s.set_selection(Vec::new());
                    s.echo("Select regions to subtract ..");
                    Ok(Step::Continue)
                }
                Some(from) => {
                    let r = subtract(s, &from, &hs)?;
                    if let Some(m) = r.get("message").and_then(Value::as_str).filter(|m| !m.is_empty()) {
                        s.echo(m.to_string());
                    }
                    s.set_selection(Vec::new());
                    Ok(Step::Done)
                }
            },
        }
    }
}

/// MASSPROP: area, perimeter, bounding box, centroid, moments of inertia and principal directions
/// for each selected region, plus a self-check of what was measured, skipped and found
/// implausible.
fn run_massprop(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let rs = regions(s, &hs)?;
    if rs.is_empty() {
        return Err(other("Select one or more regions."));
    }
    let mut check = SelfCheck::new();
    let d = s.doc()?;
    let extents = d.extents(&s.space());
    // No visible extents means no bound to compare an area against (`implausible` skips an
    // infinite bound).
    let extents_area = if extents.is_empty() { f64::INFINITY } else { extents.width() * extents.height() };
    for h in &hs {
        check.found(1);
        match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::Region(_)) => check.analyzed(1),
            Some(kind) => check.skip(kind.type_name(), "not a region", 1),
            None => check.skip("Missing", "no such object", 1),
        }
    }
    let f = |v: f64| super::fmt_d(s, v);
    let mut out = Vec::new();
    let mut lines = Vec::new();
    for (h, r) in &rs {
        let m = r.props();
        let c = m.centroid;
        let diagonal = if m.bounds.is_empty() { 0.0 } else { m.bounds.min.dist(m.bounds.max) };
        check.implausible(*h, "Region", "perimeter", m.perimeter, diagonal * 10.0, "perimeter is more than 10 x the bounding-box diagonal");
        check.implausible(*h, "Region", "area", m.area, extents_area, "area is larger than the drawing extents");
        // Moments about the centroid, and the principal moments and directions.
        let [ixc, iyc, ixyc] = m.centroid_moments;
        let mid = (ixc + iyc) / 2.0;
        let rad = (((ixc - iyc) / 2.0).powi(2) + ixyc * ixyc).sqrt();
        let (i1, i2) = (mid - rad, mid + rad);
        let ang = 0.5 * (-2.0 * ixyc).atan2(ixc - iyc);
        let gyr = |i: f64| if m.area > 0.0 { (i / m.area).abs().sqrt() } else { 0.0 };
        out.push(json!({
            "handle": h.hex(), "area": m.area, "perimeter": m.perimeter,
            "boundingBox": [[m.bounds.min.x, m.bounds.min.y], [m.bounds.max.x, m.bounds.max.y]],
            "centroid": [c.x, c.y], "momentsOfInertia": [m.ixx, m.iyy], "productOfInertia": m.ixy,
            "radiiOfGyration": [gyr(m.ixx), gyr(m.iyy)], "principalMoments": [i1, i2],
            "principalDirections": [[ang.cos(), ang.sin()], [-ang.sin(), ang.cos()]],
        }));
        lines.push(format!(
            "----------------   REGIONS   ----------------\nArea:                    {}\nPerimeter:               {}\nBounding box:         X: {}  --  {}\n                      Y: {}  --  {}\nCentroid:             X: {}\n                      Y: {}\nMoments of inertia:   X: {}\n                      Y: {}\nProduct of inertia:  XY: {}\nRadii of gyration:    X: {}\n                      Y: {}\nPrincipal moments and X-Y directions about centroid:\n                      I: {} along [{:.4} {:.4}]\n                      J: {} along [{:.4} {:.4}]",
            f(m.area),
            f(m.perimeter),
            f(m.bounds.min.x),
            f(m.bounds.max.x),
            f(m.bounds.min.y),
            f(m.bounds.max.y),
            f(c.x),
            f(c.y),
            f(m.ixx),
            f(m.iyy),
            f(m.ixy),
            f(gyr(m.ixx)),
            f(gyr(m.iyy)),
            f(i1),
            ang.cos(),
            ang.sin(),
            f(i2),
            -ang.sin(),
            ang.cos(),
        ));
    }
    Ok(json!({ "regions": out, "selfCheck": check.to_json(), "message": lines.join("\n") }))
}
