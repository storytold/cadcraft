//! The filled band of a wide polyline segment and the corners where bands meet.
//!
//! Drawing a polyline with widths and measuring its extents both build on these, so the extents
//! cover every corner that is drawn.

use crate::{EPS, Vec2, line_line_infinite};

/// Miters longer than this many half widths are drawn bevelled (as PDF's default miter limit).
pub const MITER_LIMIT: f64 = 10.0;

/// The band's edge points (left, right of the direction of travel) at each sample of a segment's
/// centre line, its width tapering from `w0` at the first sample to `w1` at the last.
pub fn band_edges(pts: &[Vec2], w0: f64, w1: f64) -> Vec<(Vec2, Vec2)> {
    let m = pts.len();
    let last = m.saturating_sub(1).max(1) as f64;
    pts.iter()
        .enumerate()
        .map(|(k, q)| {
            let w = (w0 + (w1 - w0) * (k as f64 / last)) / 2.0;
            let dir = if k + 1 < m { pts.get(k + 1).map(|n| *n - *q) } else { pts.get(k.wrapping_sub(1)).map(|pv| *q - *pv) }
                .unwrap_or(Vec2::X)
                .normalized();
            let nrm = dir.perp() * w;
            (*q + nrm, *q - nrm)
        })
        .collect()
}

/// One end of a segment's band: its centre point and the sample next to it inside the segment,
/// with the band's edge points (left, right of the centre line) at both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandEnd {
    pub centre: Vec2,
    pub toward: Vec2,
    pub edge: (Vec2, Vec2),
    pub inner_edge: (Vec2, Vec2),
    pub half_width: f64,
}

/// The start and the finish of a segment's band, from its samples `pts` and their `edges`
/// ([`band_edges`]). None for a segment of no length (a repeated vertex): it has no direction to
/// join along, so the segments on either side of it meet at the same point and join each other.
pub fn band_ends(pts: &[Vec2], edges: &[(Vec2, Vec2)], w0: f64, w1: f64) -> Option<(BandEnd, BandEnd)> {
    let m = pts.len();
    let end = |c: usize, inner: usize, w: f64| {
        let (centre, toward) = (*pts.get(c)?, *pts.get(inner)?);
        (centre.dist(toward) > EPS).then_some(())?;
        Some(BandEnd { centre, toward, edge: *edges.get(c)?, inner_edge: *edges.get(inner)?, half_width: w / 2.0 })
    };
    Some((end(0, 1, w0)?, end(m.checked_sub(1)?, m.checked_sub(2)?, w1)?))
}

/// The outside of a corner, where the band arriving at a vertex meets the one leaving it: the
/// vertex, the arriving band's outer edge point, where the two outer edges meet (`tip`, for a
/// miter) and the leaving band's outer edge point. Without a tip the corner is bevelled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandJoin {
    pub centre: Vec2,
    pub from: Vec2,
    pub tip: Option<Vec2>,
    pub to: Vec2,
}

/// The corner between the band arriving at `a` and the one leaving from `b`: their outer edges
/// are extended until they meet (a miter), or joined straight across (a bevel) when they would
/// meet further out than [`MITER_LIMIT`] half widths. Each edge keeps its own direction, so
/// tapered segments meet where their edges do. None when the bands have no width or run straight
/// on (or straight back).
fn band_join(a: &BandEnd, b: &BandEnd) -> Option<BandJoin> {
    let c = a.centre;
    let (d0, d1) = ((a.centre - a.toward).normalized(), (b.toward - b.centre).normalized());
    let scale = a.half_width.max(b.half_width);
    let turn = d0.cross(d1);
    if scale <= 0.0 || turn.abs() <= 1e-12 {
        return None;
    }
    // The outer side of the turn: right of a left turn, left of a right turn.
    let pick = |e: (Vec2, Vec2)| if turn > 0.0 { e.1 } else { e.0 };
    let (a0, a1, b0, b1) = (pick(a.inner_edge), pick(a.edge), pick(b.edge), pick(b.inner_edge));
    let tip = line_line_infinite(a0, a1, b1, b0)
        .filter(|&(tip, t, u)| t >= 1.0 - 1e-9 && u >= 1.0 - 1e-9 && tip.dist(c) <= MITER_LIMIT * scale)
        .map(|(tip, ..)| tip);
    Some(BandJoin { centre: c, from: a1, tip, to: b0 })
}

/// The corners of a polyline whose segments' bands have `ends` ([`band_ends`]), in order: one
/// between each segment and the next, and for a `closed` polyline one between the last and the
/// first.
pub fn band_joins(ends: &[(BandEnd, BandEnd)], closed: bool) -> Vec<BandJoin> {
    let mut joins: Vec<BandJoin> = ends
        .windows(2)
        .filter_map(|pair| match pair {
            [(_, arriving), (leaving, _)] => band_join(arriving, leaving),
            _ => None,
        })
        .collect();
    if closed
        && ends.len() > 1
        && let (Some(last), Some(first)) = (ends.last(), ends.first())
    {
        joins.extend(band_join(&last.1, &first.0));
    }
    joins
}
