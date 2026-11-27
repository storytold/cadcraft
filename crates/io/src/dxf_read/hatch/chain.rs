//! Joining the edges of a HATCH edge path into loops.
//!
//! The DXF Reference lists a boundary path's edges one after another: group 72 gives each edge's
//! type, and the edge's own groups follow. Some files list the edges out of order, or store some
//! of them end to start. Read in file order, the vertex list of such a path jumps from edge to
//! edge, and the even-odd fill then fills the polygon those jumps make instead of the outline.

use std::collections::VecDeque;
use std::ops::Range;

use super::{PolyVertex, Vec2};

/// Points this close are one vertex where an edge meets the next. It is the tolerance the edge
/// readers used before edges were chained, so a path whose edges run on is read as before.
const SAME_POINT: f64 = 1e-9;

/// The share of a loop's extent within which two edge ends meet.
///
/// It only decides which edge follows which: no point is moved, and two ends that meet but
/// differ keep both points, so a real gap that small stays a segment of its own length. A
/// millionth is far above the rounding of the values a file stores (an arc's ends are computed
/// from its centre, radius and angles), and far below what a picture can show: a loop drawn
/// 10,000 pixels wide puts it at a hundredth of a pixel.
const MEET_SHARE: f64 = 1e-6;

/// The vertices of an edge path, from its edges in file order. Each edge is a range of `points`,
/// read on its own: its first point, the points between (a bulge on the vertex before each arc)
/// and its last point.
///
/// A path whose edges run on, each starting where the one before ends, is joined in file order,
/// as it always was. Otherwise the edges are chained into runs:
/// - A run starts with the first edge not yet used, as the file stores it.
/// - An edge with an end at the run's last point goes after it, turned around when that end is
///   its last point. Once none is left, an edge with an end at the run's first point goes before
///   it, turned around when that end is its first point. Of several, the one listed first.
/// - An edge that meets no run starts a new one.
///
/// A closed run, one that ends where it starts, is a loop of its own. The runs left open make up
/// one loop together, in the order of their first edges, each joined straight to the next as a
/// path read in file order is (a path with missing edges then still closes as before). The vertex
/// list goes from the first loop's start to each later loop and back, and closes each later loop
/// straight from its last point to its first. An even-odd fill crosses each of those jumps
/// twice, so it fills each loop as a loop of its own.
pub(super) fn chain(points: &[PolyVertex], edges: &[Range<usize>]) -> Vec<PolyVertex> {
    let mut out = Vec::with_capacity(points.len().saturating_add(edges.len()));
    let Some(frame) = Frame::of(points).filter(|frame| !runs_on(points, edges, frame.tol)) else {
        for edge in edges {
            append(&mut out, edge_points(points, edge), false);
        }
        return out;
    };
    let (order, run_ends) = runs(&frame, points, edges);
    let span = |run: &[(usize, bool)]| {
        let first = run.first().and_then(|&(edge, turned)| placed_ends(points, edges, edge, turned))?;
        let last = run.last().and_then(|&(edge, turned)| placed_ends(points, edges, edge, turned))?;
        Some((first.0, last.1))
    };
    // Each loop as the runs it is made of, ranges of `order`.
    let mut loops: Vec<Vec<Range<usize>>> = Vec::new();
    let (mut open, mut from): (Option<usize>, usize) = (None, 0);
    for &to in &run_ends {
        let run = from..to;
        from = to;
        let closed = span(order.get(run.clone()).unwrap_or_default()).is_some_and(|(start, end)| end.near(start, frame.tol));
        match open.and_then(|i| loops.get_mut(i)) {
            Some(runs) if !closed => runs.push(run),
            _ => {
                open = open.or((!closed).then_some(loops.len()));
                loops.push(vec![run]);
            }
        }
    }
    let mut anchor = None;
    for runs in &loops {
        let placed: Vec<(usize, bool)> = runs.iter().flat_map(|run| order.get(run.clone()).unwrap_or_default().iter().copied()).collect();
        let Some((start, end)) = span(&placed) else { continue };
        let later = anchor.is_some();
        if let Some(anchor) = anchor {
            extend(&mut out, std::iter::once(PolyVertex::new(anchor)));
        } else {
            anchor = Some(start);
        }
        for (edge, turned) in placed {
            append(&mut out, edges.get(edge).map_or(&[][..], |edge| edge_points(points, edge)), turned);
        }
        if later && !end.near(start, frame.tol) {
            extend(&mut out, std::iter::once(PolyVertex::new(start)));
        }
    }
    out
}

/// Whether each edge starts where the one before it ends.
fn runs_on(points: &[PolyVertex], edges: &[Range<usize>], tol: f64) -> bool {
    edges.windows(2).all(|pair| match pair {
        [before, after] => ends(points, before).zip(ends(points, after)).is_some_and(|((_, end), (start, _))| end.near(start, tol)),
        _ => true,
    })
}

/// The edges in the order they are chained, each with whether it is turned around, and where
/// each run ends in that list.
fn runs(frame: &Frame, points: &[PolyVertex], edges: &[Range<usize>]) -> (Vec<(usize, bool)>, Vec<usize>) {
    let mut index = EndIndex::new(frame, points, edges);
    let mut used = vec![false; edges.len()];
    let (mut order, mut run_ends) = (Vec::with_capacity(edges.len()), Vec::new());
    let mut run = VecDeque::new();
    for first in 0..edges.len() {
        let Some((mut head, mut tail)) = edges.get(first).and_then(|edge| ends(points, edge)) else { continue };
        if !take(&mut used, first) {
            continue;
        }
        run.push_back((first, false));
        while let Some((edge, at_last)) = index.find(frame, tail, &used) {
            let Some((start, end)) = edges.get(edge).and_then(|e| ends(points, e)) else { break };
            if !take(&mut used, edge) {
                break;
            }
            run.push_back((edge, at_last));
            tail = if at_last { start } else { end };
        }
        while let Some((edge, at_last)) = index.find(frame, head, &used) {
            let Some((start, end)) = edges.get(edge).and_then(|e| ends(points, e)) else { break };
            if !take(&mut used, edge) {
                break;
            }
            run.push_front((edge, !at_last));
            head = if at_last { start } else { end };
        }
        order.extend(run.drain(..));
        run_ends.push(order.len());
    }
    (order, run_ends)
}

/// Marks an edge used; false when it already was.
fn take(used: &mut [bool], edge: usize) -> bool {
    used.get_mut(edge).is_some_and(|used| !std::mem::replace(used, true))
}

/// Where a loop's edge ends are looked up: the lower corner of the box around its finite points,
/// and the distance within which two ends meet, which is also the side of a lookup cell.
struct Frame {
    low: Vec2,
    tol: f64,
}

impl Frame {
    /// None when no point is finite, or the points span more than a double holds.
    fn of(points: &[PolyVertex]) -> Option<Frame> {
        let mut finite = points.iter().map(|v| v.p).filter(|p| p.is_finite());
        let first = finite.next()?;
        let (low, high) = finite.fold((first, first), |(low, high), p| (low.min(p), high.max(p)));
        let extent = (high.x - low.x).max(high.y - low.y);
        extent.is_finite().then(|| Frame { low, tol: (extent * MEET_SHARE).max(SAME_POINT) })
    }

    /// The cell a point is in. The box is at most a million cells across (`extent / tol`), so
    /// the conversions are exact.
    fn cell(&self, p: Vec2) -> Option<(i64, i64)> {
        p.is_finite().then(|| (((p.x - self.low.x) / self.tol).floor() as i64, ((p.y - self.low.y) / self.tol).floor() as i64))
    }
}

/// The finite edge ends of a loop by cell. Ends in the same or the next cell meet: ends within
/// `tol` of each other always do, ends more than three times `tol` apart never.
struct EndIndex {
    /// Each end's cell and id (twice its edge's index, plus one for the edge's last point),
    /// sorted.
    ends: Vec<((i64, i64), usize)>,
    /// At a cell's first entry: the cell's first entry that may still be unused.
    next: Vec<usize>,
}

impl EndIndex {
    fn new(frame: &Frame, points: &[PolyVertex], edges: &[Range<usize>]) -> EndIndex {
        let mut ends: Vec<((i64, i64), usize)> = edges
            .iter()
            .enumerate()
            .filter_map(|(i, edge)| ends(points, edge).map(|(start, end)| (i, start, end)))
            .flat_map(|(i, start, end)| [(frame.cell(start), 2 * i), (frame.cell(end), 2 * i + 1)])
            .filter_map(|(cell, id)| cell.map(|cell| (cell, id)))
            .collect();
        ends.sort_unstable();
        let next = (0..ends.len()).collect();
        EndIndex { ends, next }
    }

    /// The unused edge listed first with an end in `p`'s cell or one next to it, and whether
    /// that end is the edge's last point (its first point when both are there).
    fn find(&mut self, frame: &Frame, p: Vec2, used: &[bool]) -> Option<(usize, bool)> {
        let (x, y) = frame.cell(p)?;
        let id = (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy))).filter_map(|cell| self.first_unused(cell, used)).min()?;
        Some((id / 2, id % 2 == 1))
    }

    /// The first end in `cell` whose edge is unused. Used ends are skipped once for good.
    fn first_unused(&mut self, cell: (i64, i64), used: &[bool]) -> Option<usize> {
        let first = self.ends.partition_point(|(c, _)| *c < cell);
        let mut at = self.next.get(first).copied()?;
        let found = loop {
            match self.ends.get(at) {
                Some(&(c, id)) if c == cell => {
                    if !used.get(id / 2).copied().unwrap_or(true) {
                        break Some(id);
                    }
                    at += 1;
                }
                _ => break None,
            }
        };
        if let Some(next) = self.next.get_mut(first) {
            *next = at;
        }
        found
    }
}

fn edge_points<'a>(points: &'a [PolyVertex], edge: &Range<usize>) -> &'a [PolyVertex] {
    points.get(edge.clone()).unwrap_or_default()
}

/// An edge's first and last point.
fn ends(points: &[PolyVertex], edge: &Range<usize>) -> Option<(Vec2, Vec2)> {
    let points = edge_points(points, edge);
    Some((points.first()?.p, points.last()?.p))
}

/// An edge's first and last point as placed, turned around or not.
fn placed_ends(points: &[PolyVertex], edges: &[Range<usize>], edge: usize, turned: bool) -> Option<(Vec2, Vec2)> {
    let (start, end) = ends(points, edges.get(edge)?)?;
    Some(if turned { (end, start) } else { (start, end) })
}

/// Appends an edge's vertices. Turned around, its points come in reverse order, and each arc's
/// bulge is negated and moved to the vertex that now starts the arc.
fn append(out: &mut Vec<PolyVertex>, edge: &[PolyVertex], turned: bool) {
    if turned {
        let before = edge.iter().rev().skip(1).map(Some).chain([None]);
        extend(out, edge.iter().rev().zip(before).map(|(v, before)| PolyVertex::with_bulge(v.p, before.map_or(0.0, |b| -b.bulge))));
    } else {
        extend(out, edge.iter().copied());
    }
}

/// Appends vertices, leaving out the first when the list already ends there; its bulge then goes
/// on the list's last vertex.
fn extend(out: &mut Vec<PolyVertex>, mut vertices: impl Iterator<Item = PolyVertex>) {
    if let Some(first) = vertices.next() {
        match out.last_mut() {
            Some(last) if last.p.near(first.p, SAME_POINT) => last.bulge = first.bulge,
            _ => out.push(first),
        }
    }
    out.extend(vertices);
}
