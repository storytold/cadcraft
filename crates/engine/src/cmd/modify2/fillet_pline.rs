//! FILLET and CHAMFER with polyline segments (#466): two straight segments of one polyline
//! (adjacent, or with one segment between them, which the corner replaces), or a straight end
//! segment of an open polyline with a line, which joins the polyline.

use cadcraft_doc::{EntityKind, Handle, LwPolyline};
use cadcraft_geom::{Line, PolyVertex, Polyline, Segment, Vec2, arc_to_bulge, bulge_to_arc, line_line_infinite};

use super::{curves, other, push_entity, set_kind};
use crate::cmd::helpers::{arc, line};
use crate::{Result, Session};

/// CHAMFER's distances back from a corner along the first and second picked objects, given the
/// unit directions from the corner towards their kept sides.
pub(crate) type ChamferDistances<'a> = &'a dyn Fn(Vec2, Vec2) -> Result<(f64, f64)>;

/// A cut corner: the new end on each side and the bulge from the first to the second (0 for a
/// chamfer or a sharp corner).
struct Corner {
    t1: Vec2,
    t2: Vec2,
    bulge: f64,
}

/// The corner at `x` between runs leaving it along `u1` (first pick) and `u2` (second pick),
/// whose kept ends are `room1` and `room2` away.
fn corner(x: Vec2, (u1, room1): (Vec2, f64), (u2, room2): (Vec2, f64), r: f64, chamfer: Option<ChamferDistances>) -> Result<Corner> {
    if u1 == Vec2::ZERO || u2 == Vec2::ZERO {
        return Err(other("The segments meet at a kept end; nothing to cut."));
    }
    let (d1, d2, bulge) = if let Some(f) = chamfer {
        let (d1, d2) = f(u1, u2)?;
        (d1, d2, 0.0)
    } else if r <= 1e-12 {
        (0.0, 0.0, 0.0)
    } else {
        let half = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if half < 1e-9 || (std::f64::consts::FRAC_PI_2 - half).abs() < 1e-9 {
            return Err(other("The segments are parallel or in line."));
        }
        let d = r / half.tan();
        // Travelling in along -u1 and out along u2.
        let turn = (-u1).cross(u2).atan2((-u1).dot(u2));
        (d, d, arc_to_bulge(turn))
    };
    if !(d1.is_finite() && d2.is_finite()) || d1 > room1 + 1e-9 || d2 > room2 + 1e-9 {
        return Err(other(if chamfer.is_some() { "The chamfer distances are too large." } else { "Radius is too large." }));
    }
    Ok(Corner { t1: x + u1 * d1, t2: x + u2 * d2, bulge })
}

fn push_point(vs: &mut Vec<PolyVertex>, v: PolyVertex) {
    match vs.last_mut() {
        Some(l) if l.p.near(v.p, 1e-12) => l.bulge = v.bulge,
        _ => vs.push(v),
    }
}

/// The index of the polyline segment nearest `p`.
fn picked_segment(segs: &[Segment], p: Vec2) -> Option<usize> {
    (0..segs.len()).min_by(|a, b| {
        let d = |i: &usize| segs.get(*i).map_or(f64::INFINITY, |s| s.closest(p).dist(p));
        d(a).total_cmp(&d(b))
    })
}

/// With TRIMMODE 0: the arc (or chamfer line) on its own, both objects left as they are.
fn add_loose(s: &mut Session, near: Handle, c: &Corner) -> Result<Option<Handle>> {
    let e = curves::entity(s, near)?;
    let space = s.doc()?.space_of(near).unwrap_or_else(|| s.space());
    let k = match bulge_to_arc(c.t1, c.t2, c.bulge) {
        Some((a, _)) => arc(&a),
        None if !c.t1.near(c.t2, 1e-12) => line(c.t1, c.t2),
        None => return Ok(None),
    };
    Ok(Some(push_entity(s, &space, e.common.clone(), k)?))
}

/// Two segments of one polyline.
fn same_polyline(
    s: &mut Session,
    h: Handle,
    pl: &LwPolyline,
    (p1, p2): (Vec2, Vec2),
    r: f64,
    chamfer: Option<ChamferDistances>,
    trim: bool,
) -> Result<Option<Handle>> {
    let segs = Polyline { vertices: pl.vertices.clone(), closed: pl.closed }.segments();
    let n = pl.vertices.len();
    let ns = segs.len();
    let (Some(a), Some(b)) = (picked_segment(&segs, p1), picked_segment(&segs, p2)) else { return Err(other("Select two segments.")) };
    if a == b {
        return Err(other("Select two different segments of the polyline."));
    }
    let next = |k: usize| if pl.closed { Some((k + 1) % ns) } else { (k + 1 < ns).then_some(k + 1) };
    // (first segment, second segment, segments between, picked in that order?)
    let (i, j, gap, in_order) = if next(a) == Some(b) {
        (a, b, 0, true)
    } else if next(b) == Some(a) {
        (b, a, 0, false)
    } else if next(a).and_then(next) == Some(b) {
        (a, b, 1, true)
    } else if next(b).and_then(next) == Some(a) {
        (b, a, 1, false)
    } else {
        return Err(other("The segments must be adjacent or separated by one segment."));
    };
    let (Some(Segment::Line(si)), Some(Segment::Line(sj))) = (segs.get(i), segs.get(j)) else {
        return Err(other("Fillet and chamfer work on straight polyline segments."));
    };
    let x =
        if gap == 0 { si.b } else { line_line_infinite(si.a, si.b, sj.a, sj.b).map(|r| r.0).ok_or_else(|| other("The segments are parallel."))? };
    let side = |keep: Vec2| ((keep - x).normalized(), keep.dist(x));
    // CHAMFER's first distance goes on the first picked segment.
    let c = if in_order {
        corner(x, side(si.a), side(sj.b), r, chamfer)?
    } else {
        let c = corner(x, side(sj.b), side(si.a), r, chamfer)?;
        Corner { t1: c.t2, t2: c.t1, bulge: -c.bulge }
    };
    if !trim {
        return add_loose(s, h, &c);
    }
    // Rotate a closed polyline so the first segment starts at vertex 0 (nothing wraps).
    let mut vs = pl.vertices.clone();
    if pl.closed {
        vs.rotate_left(i % n.max(1));
    }
    let (i, j) = if pl.closed { (0, (j + ns - i) % ns) } else { (i, j) };
    let tmpl = vs.get(i + 1).copied().unwrap_or_default();
    let mut out: Vec<PolyVertex> = vs.get(..=i).map(<[PolyVertex]>::to_vec).unwrap_or_default();
    push_point(&mut out, PolyVertex { p: c.t1, bulge: c.bulge, ..tmpl });
    push_point(&mut out, PolyVertex { p: c.t2, bulge: 0.0, ..tmpl });
    for v in vs.iter().skip(j + 1) {
        push_point(&mut out, *v);
    }
    set_kind(s, h, EntityKind::LwPolyline(LwPolyline { vertices: out, ..pl.clone() }))?;
    Ok(Some(h))
}

/// An open polyline's straight end segment with a line: the corner is cut and the line joins
/// the polyline.
#[allow(clippy::too_many_arguments)]
fn polyline_and_line(
    s: &mut Session,
    (hp, pl, pp): (Handle, &LwPolyline, Vec2),
    (hl, ln, lp): (Handle, Line, Vec2),
    polyline_first: bool,
    r: f64,
    chamfer: Option<ChamferDistances>,
    trim: bool,
) -> Result<Option<Handle>> {
    let segs = Polyline { vertices: pl.vertices.clone(), closed: pl.closed }.segments();
    let k = picked_segment(&segs, pp).ok_or_else(|| other("Select a polyline segment."))?;
    let last = segs.len().checked_sub(1).ok_or_else(|| other("Select a polyline segment."))?;
    if pl.closed || (k != 0 && k != last) {
        return Err(other("A line fillets or chamfers with an end segment of an open polyline."));
    }
    let at_end = match segs.get(k) {
        // A single segment: the end nearer the corner is cut.
        Some(sg) if k == 0 && k == last => {
            line_line_infinite(sg.start(), sg.end(), ln.a, ln.b).is_none_or(|(x, _, _)| x.dist(sg.end()) <= x.dist(sg.start()))
        }
        _ => k == last,
    };
    // Work at the polyline's end; turn it round if the first segment was picked.
    let mut vs = if at_end { pl.vertices.clone() } else { curves::reverse_vertices(&pl.vertices, false) };
    let n = vs.len();
    let (Some(inner), Some(end)) = (vs.get(n.wrapping_sub(2)).copied(), vs.get(n.wrapping_sub(1)).copied()) else {
        return Err(other("Select a polyline segment."));
    };
    if inner.bulge.abs() > 1e-12 {
        return Err(other("Fillet and chamfer work on straight polyline segments."));
    }
    let x = line_line_infinite(inner.p, end.p, ln.a, ln.b).map(|r| r.0).ok_or_else(|| other("Lines are parallel."))?;
    let keep_line = if (ln.a - x).dot(lp - x) >= (ln.b - x).dot(lp - x) { ln.a } else { ln.b };
    let side = |keep: Vec2| ((keep - x).normalized(), keep.dist(x));
    let c = if polyline_first {
        corner(x, side(inner.p), side(keep_line), r, chamfer)?
    } else {
        let c = corner(x, side(keep_line), side(inner.p), r, chamfer)?;
        Corner { t1: c.t2, t2: c.t1, bulge: -c.bulge }
    };
    if !trim {
        return add_loose(s, hp, &c);
    }
    vs.truncate(n - 1);
    push_point(&mut vs, PolyVertex { p: c.t1, bulge: c.bulge, ..end });
    push_point(&mut vs, PolyVertex { p: c.t2, bulge: 0.0, ..end });
    push_point(&mut vs, PolyVertex { p: keep_line, bulge: 0.0, ..end });
    if !at_end {
        vs = curves::reverse_vertices(&vs, false);
    }
    set_kind(s, hp, EntityKind::LwPolyline(LwPolyline { vertices: vs, ..pl.clone() }))?;
    s.doc_mut()?.remove_entity(hl);
    Ok(Some(hp))
}

/// FILLET (`chamfer` None, radius `r`) or CHAMFER where a polyline is picked: `None` when
/// neither object is a polyline. Returns the polyline that now holds the corner (or, with
/// `trim` false, the separate arc or line).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fillet_pline(
    s: &mut Session,
    h1: Handle,
    p1: Vec2,
    h2: Handle,
    p2: Vec2,
    r: f64,
    chamfer: Option<ChamferDistances>,
    trim: bool,
) -> Result<Option<Option<Handle>>> {
    let (e1, e2) = (curves::entity(s, h1)?, curves::entity(s, h2)?);
    if !matches!(e1.kind, EntityKind::LwPolyline(_)) && !matches!(e2.kind, EntityKind::LwPolyline(_)) {
        return Ok(None);
    }
    if curves::is_locked(s, h1) || curves::is_locked(s, h2) {
        return Err(other("The object is on a locked layer."));
    }
    let as_line = |k: &EntityKind| match k {
        EntityKind::Line(l) => Some(Line::new(l.a.xy(), l.b.xy())),
        _ => None,
    };
    let made = match (&e1.kind, &e2.kind) {
        (EntityKind::LwPolyline(pl), _) if h1 == h2 => same_polyline(s, h1, pl, (p1, p2), r, chamfer, trim)?,
        (EntityKind::LwPolyline(pl), k) if as_line(k).is_some() => {
            let ln = as_line(k).ok_or_else(|| other("Select a line."))?;
            polyline_and_line(s, (h1, pl, p1), (h2, ln, p2), true, r, chamfer, trim)?
        }
        (k, EntityKind::LwPolyline(pl)) if as_line(k).is_some() => {
            let ln = as_line(k).ok_or_else(|| other("Select a line."))?;
            polyline_and_line(s, (h2, pl, p2), (h1, ln, p1), false, r, chamfer, trim)?
        }
        (EntityKind::LwPolyline(_), _) | (_, EntityKind::LwPolyline(_)) => {
            return Err(other("A polyline fillets or chamfers with a line, or with another segment of itself."));
        }
        _ => return Ok(None),
    };
    Ok(Some(made))
}
