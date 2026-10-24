//! FILLET and CHAMFER with polyline segments (#466): two straight segments of one polyline
//! (adjacent, or with one segment between them, which the corner replaces), or a segment of a
//! polyline with a line or with another polyline's segment, which joins the polyline. A closed
//! polyline opens at its first vertex, as in AutoCAD; an arc segment joins at radius 0 where the
//! other object crosses it.

use cadcraft_doc::{EntityKind, Handle, LwPolyline};
use cadcraft_geom::{Circle, Line, PolyVertex, Polyline, Segment, Vec2, arc_to_bulge, bulge_to_arc, line_circle, line_line_infinite};

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
    p1: Vec2,
    p2: Vec2,
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

/// One side of a corner between two objects: the vertices kept, ordered so the corner comes
/// after the last, which is the kept end of the picked straight segment.
struct Run {
    vs: Vec<PolyVertex>,
    /// Whether the polyline was turned round to put the corner at its end.
    reversed: bool,
}

impl Run {
    fn inner(&self) -> Vec2 {
        self.vs.last().map(|v| v.p).unwrap_or_default()
    }
}

/// The run of a line kept on the picked side of the corner `x`.
fn line_run(ln: Line, pick: Vec2, x: Vec2) -> Run {
    let keep = if (ln.a - x).dot(pick - x) >= (ln.b - x).dot(pick - x) { ln.a } else { ln.b };
    Run { vs: vec![PolyVertex::new(keep)], reversed: false }
}

/// A closed polyline as an open one that starts and ends at its first vertex (the closing
/// segment becomes the last): joined to another object, AutoCAD opens it there. An open one as
/// it is.
fn opened(pl: &LwPolyline) -> LwPolyline {
    let mut o = pl.clone();
    if pl.closed
        && let Some(first) = pl.vertices.first()
    {
        o.vertices.push(PolyVertex { bulge: 0.0, ..*first });
        o.closed = false;
    }
    o
}

/// The picked segment of an open polyline.
fn picked_seg(pl: &LwPolyline, pick: Vec2) -> Result<(usize, Segment)> {
    let segs = Polyline { vertices: pl.vertices.clone(), closed: false }.segments();
    let k = picked_segment(&segs, pick).ok_or_else(|| other("Select a polyline segment."))?;
    segs.get(k).map(|sg| (k, *sg)).ok_or_else(|| other("Select a polyline segment."))
}

/// The picked straight segment of an open polyline, as a line.
fn picked_line(pl: &LwPolyline, pick: Vec2) -> Result<(usize, Line)> {
    match picked_seg(pl, pick)? {
        (k, Segment::Line(l)) => Ok((k, l)),
        _ => Err(other("Fillet and chamfer work on straight polyline segments.")),
    }
}

/// The run of a polyline kept on the picked side of the corner `x` on its segment `k`: what lies
/// beyond the corner on the other side goes, as in AutoCAD (an object crossing a segment keeps
/// the picked part of it and drops the rest of the polyline past the crossing).
fn pline_run(pl: &LwPolyline, k: usize, seg: Line, pick: Vec2, x: Vec2) -> Run {
    let toward_start = (seg.a - x).dot(pick - x) >= (seg.b - x).dot(pick - x);
    if toward_start {
        Run { vs: pl.vertices.get(..=k).map(<[PolyVertex]>::to_vec).unwrap_or_default(), reversed: false }
    } else {
        let tail = pl.vertices.get(k + 1..).unwrap_or_default();
        Run { vs: curves::reverse_vertices(tail, false), reversed: true }
    }
}

/// A straight segment of an open polyline with a line or another open polyline's: the corner
/// is cut and the other object joins the polyline (`hp`), which keeps its handle and direction.
#[allow(clippy::too_many_arguments)]
fn polyline_and_other(
    s: &mut Session,
    (hp, pl, pp): (Handle, &LwPolyline, Vec2),
    (ho, ko, po): (Handle, &EntityKind, Vec2),
    polyline_first: bool,
    r: f64,
    chamfer: Option<ChamferDistances>,
    trim: bool,
) -> Result<Option<Handle>> {
    let pl = &opened(pl);
    let other_opened = match ko {
        EntityKind::LwPolyline(op) => Some(opened(op)),
        _ => None,
    };
    let other_seg = match (ko, &other_opened) {
        (EntityKind::Line(l), _) => (None, Line::new(l.a.xy(), l.b.xy())),
        (EntityKind::LwPolyline(_), Some(op)) => {
            let (ok, os) = picked_line(op, po)?;
            (Some((op, ok)), os)
        }
        _ => return Err(other("A polyline fillets or chamfers with a line, or with another segment of itself.")),
    };
    let (other_pl, oseg) = other_seg;
    let (k, seg) = match picked_seg(pl, pp)? {
        (k, Segment::Line(l)) => (k, l),
        (k, Segment::Arc { .. }) => {
            if r > 1e-12 || chamfer.is_some() {
                return Err(other("A polyline's arc segment fillets with radius 0 only."));
            }
            if !trim {
                return Ok(None);
            }
            return arc_segment_and_other(s, (hp, pl, k, pp), (ho, other_pl, oseg, po));
        }
    };
    let x = line_line_infinite(seg.a, seg.b, oseg.a, oseg.b).map(|r| r.0).ok_or_else(|| other("Lines are parallel."))?;
    let mine = pline_run(pl, k, seg, pp, x);
    let theirs = match other_pl {
        Some((op, ok)) => pline_run(op, ok, oseg, po, x),
        None => line_run(oseg, po, x),
    };
    let side = |keep: Vec2| ((keep - x).normalized(), keep.dist(x));
    let c = if polyline_first {
        corner(x, side(mine.inner()), side(theirs.inner()), r, chamfer)?
    } else {
        let c = corner(x, side(theirs.inner()), side(mine.inner()), r, chamfer)?;
        Corner { t1: c.t2, t2: c.t1, bulge: -c.bulge }
    };
    if !trim {
        return add_loose(s, hp, &c);
    }
    let tmpl = pl.vertices.get(k + 1).copied().unwrap_or_default();
    let mut vs = mine.vs;
    if let Some(l) = vs.last_mut() {
        l.bulge = 0.0;
    }
    push_point(&mut vs, PolyVertex { p: c.t1, bulge: c.bulge, ..tmpl });
    push_point(&mut vs, PolyVertex { p: c.t2, bulge: 0.0, ..tmpl });
    // The other object's run, from its kept end at the corner out to its far end.
    let back = curves::reverse_vertices(&theirs.vs, false);
    for v in back {
        push_point(&mut vs, if other_pl.is_some() { v } else { PolyVertex { p: v.p, bulge: 0.0, ..tmpl } });
    }
    if mine.reversed {
        vs = curves::reverse_vertices(&vs, false);
    }
    set_kind(s, hp, EntityKind::LwPolyline(LwPolyline { vertices: vs, ..pl.clone() }))?;
    s.doc_mut()?.remove_entity(ho);
    Ok(Some(hp))
}

/// A polyline's arc segment `k` with a line, or another polyline's straight segment, that
/// crosses it (FILLET with radius 0): the arc is cut where they cross, nearest the picks, and
/// keeps its picked side; the other object's picked side joins the polyline there. Neither is
/// extended: a line that misses the arc is refused, as in AutoCAD.
fn arc_segment_and_other(
    s: &mut Session,
    (hp, pl, k, pp): (Handle, &LwPolyline, usize, Vec2),
    (ho, other_pl, oseg, po): (Handle, Option<(&LwPolyline, usize)>, Line, Vec2),
) -> Result<Option<Handle>> {
    let vs = &pl.vertices;
    let (Some(start), Some(end)) = (vs.get(k).copied(), vs.get(k + 1).copied()) else { return Err(other("Select a polyline segment.")) };
    let (ar, ccw) = bulge_to_arc(start.p, end.p, start.bulge).ok_or_else(|| other("Select a polyline segment."))?;
    let total = (4.0 * start.bulge.atan()).abs();
    let a0 = ar.center.angle_to(start.p);
    // How far along the arc, from its start in its own direction, a point lies (radians).
    let along = |q: Vec2| {
        let d = ar.center.angle_to(q) - a0;
        let d = if ccw { d } else { -d };
        d.rem_euclid(std::f64::consts::TAU)
    };
    let eps = 1e-9 * (1.0 + total);
    let x = line_circle(&oseg, &Circle::new(ar.center, ar.radius))
        .into_iter()
        .map(|h| h.0)
        .filter(|q| q.is_finite() && along(*q) > eps && along(*q) < total - eps)
        .min_by(|a, b| (a.dist(pp) + a.dist(po)).total_cmp(&(b.dist(pp) + b.dist(po))))
        .ok_or_else(|| other("The line does not cross the polyline's arc segment."))?;
    let tx = along(x);
    let sub = |sweep: f64| arc_to_bulge(start.bulge.signum() * sweep);
    let theirs = match other_pl {
        Some((op, ok)) => pline_run(op, ok, oseg, po, x),
        None => line_run(oseg, po, x),
    };
    let tmpl = end;
    let plain = |v: PolyVertex| if other_pl.is_some() { v } else { PolyVertex { p: v.p, bulge: 0.0, ..tmpl } };
    let mut out: Vec<PolyVertex> = Vec::new();
    // Where the pick lies along the arc: one a little outside its sweep (within the pick box)
    // counts at the nearer end, not always at the far one.
    let picked = {
        let a = along(pp);
        if a > total && std::f64::consts::TAU - a < a - total { 0.0 } else { a.min(total) }
    };
    if picked <= tx {
        // Keep the start of the arc: the polyline up to the crossing, then the other object.
        out.extend(vs.get(..k).unwrap_or_default().iter().copied());
        out.push(PolyVertex { bulge: sub(tx), ..start });
        push_point(&mut out, PolyVertex { p: x, bulge: 0.0, ..tmpl });
        for v in curves::reverse_vertices(&theirs.vs, false) {
            push_point(&mut out, plain(v));
        }
    } else {
        // Keep the end of the arc: the other object, then the polyline from the crossing on.
        for v in &theirs.vs {
            push_point(&mut out, plain(*v));
        }
        if let Some(l) = out.last_mut() {
            l.bulge = 0.0;
        }
        push_point(&mut out, PolyVertex { p: x, bulge: sub(total - tx), ..tmpl });
        out.extend(vs.get(k + 1..).unwrap_or_default().iter().copied());
    }
    set_kind(s, hp, EntityKind::LwPolyline(LwPolyline { vertices: out, ..pl.clone() }))?;
    s.doc_mut()?.remove_entity(ho);
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
    let made = match (&e1.kind, &e2.kind) {
        (EntityKind::LwPolyline(pl), _) if h1 == h2 => same_polyline(s, h1, pl, p1, p2, r, chamfer, trim)?,
        // The first polyline picked keeps the result.
        (EntityKind::LwPolyline(pl), k) if matches!(k, EntityKind::Line(_) | EntityKind::LwPolyline(_)) => {
            polyline_and_other(s, (h1, pl, p1), (h2, k, p2), true, r, chamfer, trim)?
        }
        (k @ EntityKind::Line(_), EntityKind::LwPolyline(pl)) => polyline_and_other(s, (h2, pl, p2), (h1, k, p1), false, r, chamfer, trim)?,
        (EntityKind::LwPolyline(_), _) | (_, EntityKind::LwPolyline(_)) => {
            return Err(other("A polyline fillets or chamfers with a line, or with another segment of itself."));
        }
        _ => return Ok(None),
    };
    Ok(Some(made))
}
