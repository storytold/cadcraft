//! LOOPS: implied closed loops from loose LINE and ARC chains — perimeter, area and winding for
//! each closed loop, the gap and its location for each open chain, and branch junctions (three or
//! more ends meeting) reported but not resolved. Read-only.
//!
//! Boundaries are often drawn as separate LINEs and ARCs rather than one polyline, so an agent
//! cannot tell whether they form a closed loop or by how much a "closed" outline is open. The
//! endpoint chaining is quadratic in the number of endpoints; past `maxPairs` it reports `capped`
//! and analyzes nothing rather than guess from a partial pairing.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Arc as GeoArc, EPS, Vec2};
use serde_json::{Value, json};

use super::super::bad;

use crate::{Result, Session};

const CMD: &str = "loops";

/// Endpoint pairs compared before the quadratic chaining gives up (reported as `capped`).
const DEFAULT_PAIR_LIMIT: usize = 1_000_000;
/// The most pairs a caller may ask for: a bounded amount of work however large the drawing.
const MAX_PAIR_LIMIT: usize = 100_000_000;
/// Loops returned, and open chains and junctions kept, by default.
const DEFAULT_LIMIT: usize = 200;
/// The smallest default tolerance, for a drawing at the origin.
const MIN_TOLERANCE: f64 = 1e-12;

pub const PARAMS: &str = "{tolerance?: positive number (default 1e-9 × the extents diagonal), maxPairs? (endpoint pairs compared, default 1000000, at most 100000000), limit? (default 200), offset?} → \
     {schema: 1, tolerance, pairTests, pairTestsNeeded, pairLimit, capped, count (all closed loops), returned, offset, truncated, \
     loops: [{members: [handle], perimeter, area, winding: ccw|cw}], \
     openChains: {count, truncated, chains: [{members, gap, at: [x, y], ends: [[x, y], [x, y]], length}]}, \
     junctions: {count, truncated, junctions: [{at: [x, y], ends, members}]}, skipped: {<type>: {count, reason}}}: \
     chains LINE and ARC endpoints that meet within the tolerance; junctions (three or more ends meeting) are reported, not resolved";

/// Why a LINE or ARC cannot take part in a chain.
type Skipped = BTreeMap<&'static str, (usize, BTreeSet<&'static str>)>;

/// One chainable entity, oriented as drawn (a line a→b, an arc CCW start→end).
struct Piece {
    handle: Handle,
    start: Vec2,
    end: Vec2,
    /// The arc, when the piece is one.
    arc: Option<GeoArc>,
}

impl Piece {
    fn at(&self, which: usize) -> Vec2 {
        if which == 0 { self.start } else { self.end }
    }
    fn length(&self) -> f64 {
        match self.arc {
            Some(a) => a.radius * a.sweep(),
            None => self.start.dist(self.end),
        }
    }
}

/// An entity endpoint in the chain graph. Each piece has one at index `2 * piece` and one at
/// `2 * piece + 1`.
#[derive(Clone, Copy)]
struct End {
    piece: usize,
    at: Vec2,
}

/// A piece traversed one way round a chain.
struct Traversed {
    start: Vec2,
    end: Vec2,
    /// The arc and whether the traversal follows its CCW direction.
    arc: Option<(GeoArc, bool)>,
}

pub fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let limit = count("limit", DEFAULT_LIMIT)?;
    let offset = count("offset", 0)?;
    let pair_limit = count("maxPairs", DEFAULT_PAIR_LIMIT)?.min(MAX_PAIR_LIMIT);
    let tolerance = given("tolerance")
        .map(|v| v.as_f64().filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| bad(CMD, "`tolerance` must be a positive number")))
        .transpose()?;
    let s: &Session = s;
    let st = s.state()?;
    let d = &st.doc;
    let tol = tolerance.unwrap_or_else(|| {
        let ext = d.extents(&st.space);
        if ext.is_empty() {
            return MIN_TOLERANCE;
        }
        let diag = ext.min.dist(ext.max);
        if diag.is_finite() { (diag * 1e-9).max(MIN_TOLERANCE) } else { MIN_TOLERANCE }
    });
    let mut skipped: Skipped = BTreeMap::new();
    let Some(store) = d.space(&st.space) else {
        return Ok(empty_report(offset, tol, 0, 0, pair_limit, false, skipped));
    };
    let pieces: Vec<Piece> = store.iter().filter_map(|e| chainable(e.handle, &e.kind, &mut skipped)).collect();
    let mut ends: Vec<End> = Vec::with_capacity(pieces.len().saturating_mul(2));
    for (i, piece) in pieces.iter().enumerate() {
        ends.push(End { piece: i, at: piece.start });
        ends.push(End { piece: i, at: piece.end });
    }
    let n = ends.len();
    let needed = n.saturating_mul(n.saturating_sub(1)) / 2;
    if needed > pair_limit {
        return Ok(empty_report(offset, tol, 0, needed, pair_limit, true, skipped));
    }

    // Endpoints closer than the tolerance meet. Transitive components of three or more ends are
    // branch junctions; a component of exactly two is a simple joint.
    let mut parent = merge_ends(&ends, tol);
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    // Junctions are reported and their pieces left out of the chains: not resolved.
    let mut excluded = vec![false; pieces.len()];
    let (joints, junctions) = sort_groups(&groups, &ends, &pieces, &mut excluded);
    let link = joint_links(joints, &ends, &pieces, &excluded);

    let mut visited = vec![false; pieces.len()];
    let mut loops: Vec<Value> = Vec::new();
    let mut open: Vec<Value> = Vec::new();
    // Paths first: a free end marks a chain. What is left with both ends linked is a cycle.
    open_chains(&pieces, &excluded, &link, &mut visited, &mut open);
    closed_loops(&pieces, &excluded, &link, &mut visited, &mut loops);

    let total = loops.len();
    let page: Vec<Value> = loops.into_iter().skip(offset).take(limit).collect();
    let returned = page.len();
    let truncated = returned < total.saturating_sub(offset);
    let open_total = open.len();
    let open_page: Vec<Value> = open.into_iter().take(limit).collect();
    let open_truncated = open_page.len() < open_total;
    let junction_total = junctions.len();
    let junction_page: Vec<Value> = junctions.into_iter().take(limit).collect();
    let junction_truncated = junction_page.len() < junction_total;
    Ok(json!({
        "schema": 1,
        "tolerance": tol,
        "pairTests": needed,
        "pairTestsNeeded": needed,
        "pairLimit": pair_limit,
        "capped": false,
        "count": total,
        "returned": returned,
        "offset": offset,
        "truncated": truncated,
        "loops": page,
        "openChains": { "count": open_total, "truncated": open_truncated, "chains": open_page },
        "junctions": { "count": junction_total, "truncated": junction_truncated, "junctions": junction_page },
        "skipped": skipped_json(skipped),
    }))
}

/// A report with nothing analyzed: an empty drawing, or one past the pair cap.
/// Union-find over the ends: ends closer than `tol` share a root. Returns each end's parent.
fn merge_ends(ends: &[End], tol: f64) -> Vec<usize> {
    let n = ends.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        let Some(a) = ends.get(i) else { continue };
        for j in (i + 1)..n {
            let Some(b) = ends.get(j) else { continue };
            if a.at.dist(b.at) <= tol {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                if ra != rb
                    && let Some(slot) = parent.get_mut(ra)
                {
                    *slot = rb;
                }
            }
        }
    }
    parent
}

/// Each group of two ends is a joint (end indices); each of three or more is a junction, reported
/// with its pieces marked `excluded`.
fn sort_groups(groups: &BTreeMap<usize, Vec<usize>>, ends: &[End], pieces: &[Piece], excluded: &mut [bool]) -> (Vec<(usize, usize)>, Vec<Value>) {
    let mut joints: Vec<(usize, usize)> = Vec::new();
    let mut junctions: Vec<Value> = Vec::new();
    for group in groups.values() {
        match group.as_slice() {
            &[a, b] => joints.push((a, b)),
            [_, _, ..] => {
                let scale = group.len() as f64;
                let mut at = Vec2::ZERO;
                let mut members: Vec<Handle> = Vec::new();
                for &e in group {
                    let Some(end) = ends.get(e) else { continue };
                    at += end.at / scale;
                    let Some(piece) = pieces.get(end.piece) else { continue };
                    if !members.contains(&piece.handle) {
                        members.push(piece.handle);
                    }
                    if let Some(slot) = excluded.get_mut(end.piece) {
                        *slot = true;
                    }
                }
                junctions.push(json!({
                    "at": [at.x, at.y],
                    "ends": group.len(),
                    "members": members.iter().map(|h| h.hex()).collect::<Vec<_>>(),
                }));
            }
            _ => {}
        }
    }
    (joints, junctions)
}

/// The link each end takes from the joints. A joint into an excluded piece is not a link, and
/// a line whose own ends meet within a huge tolerance is a spur, not a zero-area loop.
fn joint_links(joints: Vec<(usize, usize)>, ends: &[End], pieces: &[Piece], excluded: &[bool]) -> Vec<Option<usize>> {
    let mut link: Vec<Option<usize>> = vec![None; ends.len()];
    for (a, b) in joints {
        let (Some(ea), Some(eb)) = (ends.get(a), ends.get(b)) else { continue };
        if ea.piece == eb.piece && pieces.get(ea.piece).is_some_and(|p| p.arc.is_none()) {
            continue;
        }
        if excluded.get(ea.piece).copied().unwrap_or(true) || excluded.get(eb.piece).copied().unwrap_or(true) {
            continue;
        }
        if let Some(slot) = link.get_mut(a) {
            *slot = Some(b);
        }
        if let Some(slot) = link.get_mut(b) {
            *slot = Some(a);
        }
    }
    link
}

fn empty_report(offset: usize, tol: f64, pair_tests: usize, pair_tests_needed: usize, pair_limit: usize, capped: bool, skipped: Skipped) -> Value {
    json!({
        "schema": 1,
        "tolerance": tol,
        "pairTests": pair_tests,
        "pairTestsNeeded": pair_tests_needed,
        "pairLimit": pair_limit,
        "capped": capped,
        "count": 0,
        "returned": 0,
        "offset": offset,
        "truncated": false,
        "loops": [],
        "openChains": { "count": 0, "truncated": false, "chains": [] },
        "junctions": { "count": 0, "truncated": false, "junctions": [] },
        "skipped": skipped_json(skipped),
    })
}

fn skipped_json(skipped: Skipped) -> Value {
    let m: BTreeMap<&str, Value> =
        skipped.into_iter().map(|(k, (n, why))| (k, json!({ "count": n, "reason": why.into_iter().collect::<Vec<_>>().join("; ") }))).collect();
    json!(m)
}

fn skip(skipped: &mut Skipped, ty: &'static str, why: &'static str) {
    let entry = skipped.entry(ty).or_default();
    entry.0 += 1;
    entry.1.insert(why);
}

/// The piece for a LINE or ARC, or `None` when the entity is not chainable.
fn chainable(handle: Handle, kind: &EntityKind, skipped: &mut Skipped) -> Option<Piece> {
    match kind {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            if !a.is_finite() || !b.is_finite() || a.dist(b) <= EPS {
                skip(skipped, "Line", "zero length");
                return None;
            }
            Some(Piece { handle, start: a, end: b, arc: None })
        }
        EntityKind::Arc(a) => {
            let (c, r) = (a.center.xy(), a.radius.abs());
            if !c.is_finite() || !a.radius.is_finite() || r <= EPS || !a.start.is_finite() || !a.end.is_finite() {
                skip(skipped, "Arc", "degenerate");
                return None;
            }
            let arc = GeoArc::new(c, r, a.start, a.end);
            let (start, end) = (arc.start_point(), arc.end_point());
            if !start.is_finite() || !end.is_finite() {
                skip(skipped, "Arc", "degenerate");
                return None;
            }
            Some(Piece { handle, start, end, arc: Some(arc) })
        }
        _ => None,
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while let Some(&p) = parent.get(x) {
        if p == x {
            return x;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(x) {
            *slot = gp;
        }
        x = gp;
    }
    x
}

/// Walk the cycles left after the open paths from a free end, and report their geometry.
fn closed_loops(pieces: &[Piece], excluded: &[bool], link: &[Option<usize>], visited: &mut [bool], out: &mut Vec<Value>) {
    for i in 0..pieces.len() {
        if visited.get(i).copied().unwrap_or(true) || excluded.get(i).copied().unwrap_or(true) {
            continue;
        }
        // Only a piece linked at both ends can be in a cycle.
        if link.get(i * 2).copied().flatten().is_none() || link.get(i * 2 + 1).copied().flatten().is_none() {
            continue;
        }
        let mut members: Vec<usize> = Vec::new();
        let mut forward: Vec<bool> = Vec::new();
        let (mut cur, mut entry) = (i, 0usize);
        let mut closed = false;
        loop {
            if visited.get(cur).copied().unwrap_or(true) || members.len() >= pieces.len() {
                break;
            }
            if let Some(slot) = visited.get_mut(cur) {
                *slot = true;
            }
            members.push(cur);
            forward.push(entry == 0);
            let exit = 1 - entry;
            let Some(q) = link.get(cur * 2 + exit).copied().flatten() else { break };
            let (next, which) = (q / 2, q % 2);
            if next == i && which == 0 {
                closed = true;
                break;
            }
            if visited.get(next).copied().unwrap_or(true) {
                break;
            }
            cur = next;
            entry = which;
        }
        if !closed || members.is_empty() {
            continue;
        }
        let segs = traversed(pieces, &members, &forward);
        let (perimeter, area, signed) = facts(&segs);
        out.push(json!({
            "members": members.iter().filter_map(|&m| pieces.get(m)).map(|p| p.handle.hex()).collect::<Vec<_>>(),
            "perimeter": perimeter,
            "area": area,
            "winding": if signed < 0.0 { "cw" } else { "ccw" },
        }));
    }
}

/// Walk paths from every free end: an open chain, with the gap between its two ends.
fn open_chains(pieces: &[Piece], excluded: &[bool], link: &[Option<usize>], visited: &mut [bool], out: &mut Vec<Value>) {
    for i in 0..pieces.len() {
        if visited.get(i).copied().unwrap_or(true) || excluded.get(i).copied().unwrap_or(true) {
            continue;
        }
        let Some(entry0) = [0usize, 1].into_iter().find(|w| link.get(i * 2 + w).copied().flatten().is_none()) else { continue };
        let mut members: Vec<usize> = Vec::new();
        let mut forward: Vec<bool> = Vec::new();
        let (mut cur, mut entry) = (i, entry0);
        let mut exit = entry0;
        loop {
            if visited.get(cur).copied().unwrap_or(true) || members.len() >= pieces.len() {
                break;
            }
            if let Some(slot) = visited.get_mut(cur) {
                *slot = true;
            }
            members.push(cur);
            forward.push(entry == 0);
            exit = 1 - entry;
            let Some(q) = link.get(cur * 2 + exit).copied().flatten() else { break };
            let (next, which) = (q / 2, q % 2);
            if visited.get(next).copied().unwrap_or(true) {
                break;
            }
            cur = next;
            entry = which;
        }
        let (Some(first_piece), Some(last_piece)) = (pieces.get(i), members.last().and_then(|&m| pieces.get(m))) else { continue };
        let (first, last) = (first_piece.at(entry0), last_piece.at(exit));
        let gap = first.dist(last);
        let at = first * 0.5 + last * 0.5;
        let length: f64 = members.iter().filter_map(|&m| pieces.get(m)).map(Piece::length).sum();
        out.push(json!({
            "members": members.iter().filter_map(|&m| pieces.get(m)).map(|p| p.handle.hex()).collect::<Vec<_>>(),
            "gap": gap,
            "at": [at.x, at.y],
            "ends": [[first.x, first.y], [last.x, last.y]],
            "length": length,
        }));
    }
}

fn traversed(pieces: &[Piece], members: &[usize], forward: &[bool]) -> Vec<Traversed> {
    members
        .iter()
        .zip(forward)
        .filter_map(|(&m, &fwd)| {
            let p = pieces.get(m)?;
            Some(Traversed { start: if fwd { p.start } else { p.end }, end: if fwd { p.end } else { p.start }, arc: p.arc.map(|a| (a, fwd)) })
        })
        .collect()
}

/// Perimeter, unsigned area and signed area (CCW positive, as SHAPES reports winding) of a closed
/// chain of lines and arcs. The closing chord between the last end and the first is the chain's gap
/// and counts in the perimeter, as an open polyline with matching ends does in SHAPES.
fn facts(segs: &[Traversed]) -> (f64, f64, f64) {
    let Some(first) = segs.first() else { return (0.0, 0.0, 0.0) };
    let perimeter: f64 = segs.iter().map(seg_len).sum::<f64>() + segs.last().map_or(0.0, |last| last.end.dist(first.start));
    let signed = signed_area(segs, first.start);
    (perimeter, signed.abs(), signed)
}

fn seg_len(t: &Traversed) -> f64 {
    match t.arc {
        Some((a, _)) => a.radius * a.sweep(),
        None => t.start.dist(t.end),
    }
}

/// Green's theorem over the closed path, measured about its first point so a small loop far from
/// the origin keeps its area. An arc contributes its exact circular integral, not a tessellation.
fn signed_area(segs: &[Traversed], origin: Vec2) -> f64 {
    let mut twice = 0.0;
    for t in segs {
        match t.arc {
            None => twice += (t.start - origin).cross(t.end - origin),
            Some((a, forward)) => {
                let sweep = a.sweep();
                let (from, to, d) = if forward { (a.start, a.start + sweep, sweep) } else { (a.end, a.end - sweep, -sweep) };
                let c = a.center - origin;
                twice += a.radius * a.radius * d + a.radius * (c.x * (to.sin() - from.sin()) - c.y * (to.cos() - from.cos()));
            }
        }
    }
    twice / 2.0
}
