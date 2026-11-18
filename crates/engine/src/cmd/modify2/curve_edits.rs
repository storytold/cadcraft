//! EXTEND, LENGTHEN, JOIN, FILLET and closed-spline TRIM for elliptical arcs and splines (#466).
//!
//! The curve mathematics (knot insertion, joining, tangent extensions, arc length) lives in
//! `cadcraft_geom::Spline` / `cadcraft_geom::Ellipse`; this module applies it to entities.

use cadcraft_doc::{Entity, EntityKind, Handle};
use cadcraft_geom::{
    Arc, Circle, CornerFit, Ellipse, Line, Polyline, Segment, Spline, TAU, TanCurve, Vec2, angle_in_sweep, ccw_sweep, intersect_ext, norm_angle,
    tangent_circles,
};

use super::{LenMode, curves, ellipse_kind, ellipse_of, other, set_kind};
use crate::{Result, Session};

/// EXTEND a spline's end nearest `pick` along its end tangent to the nearest boundary edge.
pub(super) fn extend_spline(sp: &Spline, cut: &[Segment], pick: Vec2) -> Result<EntityKind> {
    if sp.closed {
        return Err(other("Cannot extend a closed spline."));
    }
    if !sp.is_valid() {
        return Err(other("Invalid spline."));
    }
    let (lo, hi) = sp.domain();
    let at_end = pick.dist(sp.eval(hi)) <= pick.dist(sp.eval(lo));
    let (end, dir) = if at_end { (sp.eval(hi), sp.unit_tangent(hi)) } else { (sp.eval(lo), -sp.unit_tangent(lo)) };
    if dir == Vec2::ZERO {
        return Err(other("The spline has no direction at that end."));
    }
    let ray = Segment::Line(Line::new(end, end + dir * 1e8));
    let d = cut.iter().flat_map(|c| intersect_ext(&ray, c, false)).map(|x| (x - end).dot(dir)).filter(|d| *d > 1e-9).fold(f64::INFINITY, f64::min);
    if !d.is_finite() {
        return Err(other("Object does not intersect an edge."));
    }
    sp.extended(at_end, d).map(EntityKind::Spline).ok_or_else(|| other("Cannot extend that spline."))
}

/// TRIM a closed spline at the hits `ts` (sorted parameters) around the pick parameter `tp`:
/// the piece between the two hits around the pick goes, the rest stays as one open spline.
pub(super) fn trim_closed_spline(sp: &Spline, ts: &[f64], tp: f64) -> Result<Vec<EntityKind>> {
    let (Some(&first), Some(&last)) = (ts.first(), ts.last()) else {
        return Err(other("Spline must intersect cutting edges at two or more points."));
    };
    if ts.len() < 2 {
        return Err(other("Spline must intersect cutting edges at two or more points."));
    }
    let fail = || other("Cannot trim that spline.");
    let (lo_d, hi_d) = sp.domain();
    let kept = if tp > first && tp < last {
        // The removed piece lies inside the domain: keep the run from the next hit round the
        // seam to the previous one.
        let lo = ts.iter().copied().filter(|t| *t < tp).fold(first, f64::max);
        let hi = ts.iter().copied().filter(|t| *t > tp).fold(last, f64::min);
        let tail = sp.clamped().and_then(|c| c.sub_between(hi, hi_d)).ok_or_else(fail)?;
        let head = sp.clamped().and_then(|c| c.sub_between(lo_d, lo)).ok_or_else(fail)?;
        tail.joined(&head).ok_or_else(fail)?
    } else {
        // The removed piece crosses the seam.
        sp.sub_between(first, last).ok_or_else(fail)?
    };
    Ok(vec![EntityKind::Spline(Spline { closed: false, ..kept })])
}

fn too_short() -> crate::EngineError {
    other("Lengthening would make the object zero length or negative.")
}

/// LENGTHEN an elliptical arc along its ellipse at the end nearest `pick`.
pub(super) fn lengthen_ellipse(e: &cadcraft_doc::Ellipse, pick: Option<Vec2>, mode: LenMode) -> Result<EntityKind> {
    let g = ellipse_of(e);
    if g.is_full() {
        return Err(other("Cannot lengthen a closed ellipse."));
    }
    let at_end = pick.is_none_or(|p| p.dist(g.at_param(g.end)) <= p.dist(g.at_param(g.start)));
    let len = g.length();
    let target = match mode {
        LenMode::Delta(d) => len + d,
        LenMode::Percent(p) => len * p / 100.0,
        LenMode::Total(t) => t,
        LenMode::To(q) => {
            let t = g.param_of(q);
            let sweep = if at_end { cadcraft_geom::ccw_sweep(g.start, t) } else { cadcraft_geom::ccw_sweep(t, g.end) };
            if !(sweep > 1e-9 && sweep < TAU - 1e-9) {
                return Err(too_short());
            }
            return Ok(ellipse_kind(&if at_end { Ellipse { end: t, ..g } } else { Ellipse { start: t, ..g } }));
        }
        LenMode::DeltaAngle(_) | LenMode::TotalAngle(_) => return Err(other("Angle options apply to arcs.")),
    };
    if !(target.is_finite() && target > 1e-12) {
        return Err(too_short());
    }
    let full = Ellipse { start: 0.0, end: TAU, ..g }.length();
    if target >= full - 1e-9 * full.max(1.0) {
        return Err(other("The new elliptical arc would be a full ellipse or longer."));
    }
    let t = if at_end { g.param_at_length(g.start, target) } else { g.param_at_length(g.end, -target) };
    let t = norm_angle(t.ok_or_else(too_short)?);
    Ok(ellipse_kind(&if at_end { Ellipse { end: t, ..g } } else { Ellipse { start: t, ..g } }))
}

/// LENGTHEN an open spline at the end nearest `pick`: shorter cuts it along the curve, longer
/// runs it on along its end tangent.
pub(super) fn lengthen_spline(sp: &Spline, pick: Option<Vec2>, mode: LenMode) -> Result<EntityKind> {
    if sp.closed {
        return Err(other("Cannot lengthen a closed spline."));
    }
    if !sp.is_valid() {
        return Err(other("Invalid spline."));
    }
    let (lo, hi) = sp.domain();
    let at_end = pick.is_none_or(|p| p.dist(sp.eval(hi)) <= p.dist(sp.eval(lo)));
    let len = sp.length();
    let target = match mode {
        LenMode::Delta(d) => len + d,
        LenMode::Percent(p) => len * p / 100.0,
        LenMode::Total(t) => t,
        LenMode::To(q) => {
            let (end_t, dir) = if at_end { (hi, sp.unit_tangent(hi)) } else { (lo, -sp.unit_tangent(lo)) };
            let t = curves::spline_param_of(sp, q);
            let beyond = (q - sp.eval(end_t)).dot(dir);
            if (t - end_t).abs() <= 1e-9 * (hi - lo) && beyond > 1e-9 {
                len + beyond
            } else if at_end {
                sp.length_between(lo, t)
            } else {
                sp.length_between(t, hi)
            }
        }
        LenMode::DeltaAngle(_) | LenMode::TotalAngle(_) => return Err(other("Angle options apply to arcs.")),
    };
    if !(target.is_finite() && target > 1e-9) {
        return Err(too_short());
    }
    let fail = || other("Cannot lengthen that spline.");
    let out = if target > len + 1e-12 {
        sp.extended(at_end, target - len).ok_or_else(fail)?
    } else if target >= len - 1e-12 {
        sp.clone()
    } else if at_end {
        sp.sub_between(lo, sp.param_at_length(lo, target)).ok_or_else(fail)?
    } else {
        sp.sub_between(sp.param_at_length(hi, -target), hi).ok_or_else(fail)?
    };
    Ok(EntityKind::Spline(out))
}

/// Parameter offset that maps `b`'s parameters onto `a`'s when both are the same ellipse
/// (`a_param = b_param + offset`).
fn same_ellipse(a: &Ellipse, b: &Ellipse) -> Option<f64> {
    let tol = 1e-6 * a.major.len().max(1.0);
    if !a.center.near(b.center, tol) || (a.ratio - b.ratio).abs() > 1e-9 || (a.major.len() - b.major.len()).abs() > tol {
        return None;
    }
    if (a.ratio - 1.0).abs() <= 1e-9 {
        return Some(b.major.angle() - a.major.angle());
    }
    if a.major.near(b.major, tol) {
        Some(0.0)
    } else if a.major.near(-b.major, tol) {
        Some(std::f64::consts::PI)
    } else {
        None
    }
}

/// Elliptical arcs on `src`'s ellipse (as start and sweep in `src`'s parameters) merged into one
/// arc: everything but the widest gap between them, or the whole ellipse when no gap is left.
fn merge_arcs(src: &Ellipse, arcs: &[(f64, f64)]) -> Ellipse {
    let tol = 1e-9;
    let mut iv: Vec<(f64, f64)> = arcs.iter().map(|(s, w)| (norm_angle(s - src.start), *w)).collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gaps: Vec<(f64, f64)> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    for (s, w) in iv {
        if end.is_finite() && s > end + tol {
            gaps.push((end, s));
        }
        end = if end.is_finite() { end.max(s + w) } else { s + w };
    }
    if end < TAU - tol {
        gaps.push((end, TAU));
    } else if end > TAU {
        // An arc running past the source's start covers the beginning of later gaps.
        let over = end - TAU;
        gaps = gaps.into_iter().map(|(a, b)| (a.max(over), b)).filter(|(a, b)| b - a > tol).collect();
    }
    match gaps.into_iter().max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0))) {
        Some((a, b)) => Ellipse { start: norm_angle(src.start + b), end: norm_angle(src.start + a), ..*src },
        None => Ellipse { start: 0.0, end: TAU, ..*src },
    }
}

/// A joinable entity as exact spline pieces (lines, arcs and polyline segments become splines).
fn spline_pieces(k: &EntityKind) -> Vec<Spline> {
    let arc = |c: Vec2, r: f64, start: f64, sweep: f64| Spline::from_conic_arc(c, Vec2::X * r, Vec2::Y * r, start, sweep);
    match k {
        EntityKind::Line(l) if !l.a.xy().near(l.b.xy(), 1e-12) => vec![Spline::from_line(l.a.xy(), l.b.xy(), 1)],
        EntityKind::Arc(a) => {
            let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
            arc(g.center, g.radius, g.start, g.sweep()).into_iter().collect()
        }
        EntityKind::LwPolyline(p) if !p.closed => Polyline { vertices: p.vertices.clone(), closed: false }
            .segments()
            .into_iter()
            .filter(|sg| sg.len() > 1e-12)
            .filter_map(|sg| match sg {
                Segment::Line(l) => Some(Spline::from_line(l.a, l.b, 1)),
                Segment::Arc { arc: a, ccw: true } => arc(a.center, a.radius, a.start, a.sweep()),
                Segment::Arc { arc: a, ccw: false } => arc(a.center, a.radius, a.end, -a.sweep()),
            })
            .collect(),
        EntityKind::Ellipse(e) => {
            let g = ellipse_of(e);
            if g.is_full() { Vec::new() } else { Spline::from_ellipse(&g).into_iter().collect() }
        }
        EntityKind::Spline(sp) if !sp.closed => sp.clamped().into_iter().collect(),
        _ => Vec::new(),
    }
}

fn ends(sp: &Spline) -> (Vec2, Vec2) {
    let (lo, hi) = sp.domain();
    (sp.eval(lo), sp.eval(hi))
}

/// JOIN for selections holding elliptical arcs or splines; `None` when there are neither (the
/// polyline join handles those). Elliptical arcs that all lie on one ellipse merge into a
/// larger arc or the full ellipse (gaps between them close). Otherwise the objects joined end
/// to end with the first one become one spline. The first joined object keeps its handle.
pub(crate) fn join_curves(s: &mut Session, hs: &[Handle]) -> Result<Option<Handle>> {
    // A handle named twice is one object: a repeat would join it with itself or remove the result.
    let mut seen = std::collections::HashSet::new();
    let hs: Vec<Handle> = hs.iter().copied().filter(|h| seen.insert(*h)).collect();
    let d = s.doc()?;
    let ents: Vec<Entity> = hs.iter().filter_map(|h| d.entity(*h).map(|e| (**e).clone())).collect();
    if !ents.iter().any(|e| matches!(e.kind, EntityKind::Ellipse(_) | EntityKind::Spline(_))) {
        return Ok(None);
    }
    if ents.iter().any(|e| curves::is_locked(s, e.handle)) {
        return Err(other("An object is on a locked layer."));
    }
    let ells: Vec<(Handle, Ellipse)> =
        ents.iter().filter_map(|e| if let EntityKind::Ellipse(x) = &e.kind { Some((e.handle, ellipse_of(x))) } else { None }).collect();
    if let Some((src_h, src)) = ells.first().copied()
        && ells.len() == ents.len()
        && ells.len() >= 2
    {
        let offsets: Option<Vec<f64>> = ells.iter().map(|(_, e)| same_ellipse(&src, e)).collect();
        if let Some(offsets) = offsets {
            let arcs: Vec<(f64, f64)> = ells.iter().zip(&offsets).map(|((_, e), o)| (e.start + o, e.sweep())).collect();
            let merged = merge_arcs(&src, &arcs);
            set_kind(s, src_h, ellipse_kind(&merged))?;
            let doc = s.doc_mut()?;
            for (h, _) in ells.iter().skip(1) {
                doc.remove_entity(*h);
            }
            s.set_selection(Vec::new());
            return Ok(Some(src_h));
        }
    }
    // A spline through the objects that connect end to end with the first one.
    let mut pool: Vec<(Handle, Spline)> = ents.iter().flat_map(|e| spline_pieces(&e.kind).into_iter().map(move |p| (e.handle, p))).collect();
    if pool.is_empty() {
        return Err(other("Select at least two objects to join."));
    }
    let (first_h, first) = pool.remove(0);
    let (mut head, mut tail) = ends(&first);
    let mut chain = std::collections::VecDeque::from([first]);
    let mut used = vec![first_h];
    let tol = 1e-6;
    let touches = |p: &Spline, head: Vec2, tail: Vec2| {
        let (a, b) = ends(p);
        a.near(tail, tol) || b.near(tail, tol) || a.near(head, tol) || b.near(head, tol)
    };
    while let Some(i) = pool.iter().position(|(_, p)| touches(p, head, tail)) {
        let (h, p) = pool.remove(i);
        let (a, b) = ends(&p);
        if a.near(tail, tol) {
            tail = b;
            chain.push_back(p);
        } else if b.near(tail, tol) {
            tail = a;
            chain.push_back(p.reversed());
        } else if b.near(head, tol) {
            head = a;
            chain.push_front(p);
        } else {
            head = b;
            chain.push_front(p.reversed());
        }
        used.push(h);
    }
    if chain.len() < 2 {
        return Err(other("Objects are not connected end to end."));
    }
    // A polyline joins whole or not at all.
    if pool.iter().any(|(h, _)| used.contains(h)) {
        return Err(other("Only part of a polyline connects; join it whole or explode it first."));
    }
    used.sort();
    used.dedup();
    let mut it = chain.into_iter();
    let mut out = it.next().ok_or_else(|| other("Nothing to join."))?;
    for p in it {
        out = out.joined(&p).ok_or_else(|| other("Cannot join these objects into a spline."))?;
    }
    let (a, b) = ends(&out);
    out.closed = a.near(b, tol);
    let keep = first_h;
    set_kind(s, keep, EntityKind::Spline(out))?;
    let doc = s.doc_mut()?;
    for h in used.iter().filter(|h| **h != keep) {
        doc.remove_entity(*h);
    }
    s.set_selection(Vec::new());
    Ok(Some(keep))
}

/// What JOIN's Close option makes of a single arc (its circle) or elliptical arc (its full
/// ellipse); `None` for anything else.
pub(crate) fn closed_kind(k: &EntityKind) -> Option<EntityKind> {
    match k {
        EntityKind::Arc(a) => Some(EntityKind::Circle(cadcraft_doc::Circle { center: a.center, radius: a.radius })),
        EntityKind::Ellipse(e) if !ellipse_of(e).is_full() => Some(ellipse_kind(&Ellipse { start: 0.0, end: TAU, ..ellipse_of(e) })),
        _ => None,
    }
}

/// JOIN Close: the single arc or elliptical arc in `hs` becomes its circle or full ellipse,
/// keeping its handle.
pub(crate) fn join_close(s: &mut Session, hs: &[Handle]) -> Result<Handle> {
    let mut seen = std::collections::HashSet::new();
    let hs: Vec<Handle> = hs.iter().copied().filter(|h| seen.insert(*h)).collect();
    let refuse = || other("Close works on a single arc or elliptical arc.");
    let [h] = hs[..] else { return Err(refuse()) };
    let k = s.doc()?.entity(h).and_then(|e| closed_kind(&e.kind)).ok_or_else(refuse)?;
    if curves::is_locked(s, h) {
        return Err(other("An object is on a locked layer."));
    }
    set_kind(s, h, k)?;
    s.set_selection(Vec::new());
    Ok(h)
}

// ---------------- FILLET with elliptical arcs and splines ----------------

/// An entity as a tangency target for FILLET (lines infinite, arcs as their circle, elliptical
/// arcs as their whole ellipse, splines within their domain).
fn tan_curve(k: &EntityKind) -> Option<TanCurve> {
    match k {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            (ln.len() > 1e-12).then_some(TanCurve::Line(ln))
        }
        EntityKind::Arc(a) => Some(TanCurve::Circle(Circle::new(a.center.xy(), a.radius))),
        EntityKind::Circle(c) => Some(TanCurve::Circle(Circle::new(c.center.xy(), c.radius))),
        EntityKind::Ellipse(e) => Some(TanCurve::Ellipse(Ellipse { start: 0.0, end: TAU, ..ellipse_of(e) })),
        EntityKind::Spline(sp) if sp.is_valid() => Some(TanCurve::Spline(sp.clone())),
        _ => None,
    }
}

/// How a fillet operand is trimmed to the touch point (parameter `t`, point `at`): the new kind
/// (`None` = left as it is) and the direction it arrives at the touch point. Elliptical arcs
/// keep the side holding the pick, or extend round the ellipse from the nearer end; splines
/// keep the side holding the pick; whole ellipses, circles and closed splines stay whole.
fn fillet_trim(k: &EntityKind, pick: Vec2, t: f64, at: Vec2) -> Option<(Option<EntityKind>, Option<Vec2>)> {
    match k {
        EntityKind::Ellipse(e) => {
            let g = ellipse_of(e);
            if g.is_full() {
                return Some((None, None));
            }
            let ta = norm_angle(t);
            let tan = g.derivative(ta).normalized();
            let keep_start = if angle_in_sweep(ta, g.start, g.end) {
                angle_in_sweep(g.param_of(pick), g.start, ta)
            } else {
                ccw_sweep(g.end, ta) <= ccw_sweep(ta, g.start)
            };
            Some(if keep_start {
                (Some(ellipse_kind(&Ellipse { end: ta, ..g })), Some(tan))
            } else {
                (Some(ellipse_kind(&Ellipse { start: ta, ..g })), Some(-tan))
            })
        }
        EntityKind::Spline(sp) => {
            if sp.closed {
                return Some((None, None));
            }
            let (lo, hi) = sp.domain();
            let tan = sp.unit_tangent(t);
            Some(if curves::spline_param_of(sp, pick) <= t {
                (Some(EntityKind::Spline(sp.sub_between(lo, t)?)), Some(tan))
            } else {
                (Some(EntityKind::Spline(sp.sub_between(t, hi)?)), Some(-tan))
            })
        }
        _ => super::trim_for_fillet(k, pick, at),
    }
}

/// FILLET where one or both objects are an elliptical arc or a spline: the tangent circle comes
/// from `cadcraft_geom::tangent_circles` (the solution touching nearest the picks); each object
/// is trimmed or extended to its touch point. Radius 0 meets them at their crossing.
#[allow(clippy::too_many_arguments)]
pub(super) fn fillet_curvy(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, r: f64, e1: &Entity, e2: &Entity) -> Result<Option<Handle>> {
    let unsupported = || other("Fillet works on lines, arcs, circles, elliptical arcs and splines (and polylines with the Polyline option).");
    if h1 == h2 {
        return Err(other("Cannot fillet an object to itself."));
    }
    if !(r.is_finite() && r >= 0.0) {
        return Err(other("The fillet radius must be zero or more."));
    }
    let (Some(c1), Some(c2)) = (tan_curve(&e1.kind), tan_curve(&e2.kind)) else { return Err(unsupported()) };
    let score = |f: &CornerFit| f.p1.dist(p1) + f.p2.dist(p2);
    let best = tangent_circles(&c1, &c2, r, &[p1, p2])
        .into_iter()
        .filter(|f| score(f).is_finite())
        .min_by(|a, b| score(a).total_cmp(&score(b)))
        .ok_or_else(|| other(if r > 0.0 { "No fillet of that radius fits these objects." } else { "The objects do not intersect." }))?;
    let (n1, d1) = fillet_trim(&e1.kind, p1, best.t1, best.p1).ok_or_else(unsupported)?;
    let (n2, d2) = fillet_trim(&e2.kind, p2, best.t2, best.p2).ok_or_else(unsupported)?;
    let new_arc = (r > 0.0).then(|| {
        let c = best.center;
        let (a1, a2) = (c.angle_to(best.p1), c.angle_to(best.p2));
        let ccw = match (d1, d2) {
            (Some(d), _) => (best.p1 - c).perp().dot(d) > 0.0,
            (None, Some(d)) => (best.p2 - c).perp().dot(-d) > 0.0,
            _ => ccw_sweep(a1, a2) <= std::f64::consts::PI,
        };
        if ccw { Arc::new(c, r, a1, a2) } else { Arc::new(c, r, a2, a1) }
    });
    if let Some(k) = n1 {
        set_kind(s, h1, k)?;
    }
    if let Some(k) = n2 {
        set_kind(s, h2, k)?;
    }
    match new_arc {
        Some(a) => {
            let space = s.doc()?.space_of(h1).unwrap_or_else(|| s.space());
            Ok(Some(super::push_entity(s, &space, e1.common.clone(), crate::cmd::helpers::arc(&a))?))
        }
        None => Ok(None),
    }
}
