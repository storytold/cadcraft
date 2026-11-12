use super::{
    Arc, EntityKind, Handle, LwPolyline, PolyVertex, Result, Session, TanObj, Vec2, arc, ccw_sweep, curve_edits, curves, line, line_circle,
    line_line_infinite, other, push_entity, set_kind,
};

// =====================================================================================
// FILLET extensions (arcs, circles, polylines)
// =====================================================================================

/// How a fillet operand is trimmed: the new kind and the arrival direction at the tangent point.
pub(super) fn trim_for_fillet(k: &EntityKind, pick: Vec2, t: Vec2) -> Option<(Option<EntityKind>, Option<Vec2>)> {
    match k {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let keep = if (a - t).dot(pick - t) >= (b - t).dot(pick - t) { a } else { b };
            let dir = (t - keep).normalized();
            if dir == Vec2::ZERO {
                return Some((None, None));
            }
            Some((Some(if keep == a { line(a, t) } else { line(t, b) }), Some(dir)))
        }
        EntityKind::Arc(ar) => {
            let g = Arc::new(ar.center.xy(), ar.radius, ar.start, ar.end);
            let ta = g.center.angle_to(t);
            let ccw_tan = (t - g.center).perp().normalized();
            let ap = g.center.angle_to(pick);
            let keep_start_side = if g.contains_angle(ta) {
                // Keep the side of the tangent point that holds the pick.
                cadcraft_geom::angle_in_sweep(ap, g.start, ta)
            } else {
                // Extend the nearer end.
                ccw_sweep(g.end, ta) <= ccw_sweep(ta, g.start)
            };
            if keep_start_side {
                let na = Arc::new(g.center, g.radius, g.start, ta);
                Some((Some(arc(&na)), Some(ccw_tan)))
            } else {
                let na = Arc::new(g.center, g.radius, ta, g.end);
                Some((Some(arc(&na)), Some(-ccw_tan)))
            }
        }
        EntityKind::Circle(_) => Some((None, None)),
        _ => None,
    }
}

/// Fillet two curves (lines, arcs, circles; elliptical arcs and splines in `curve_edits`) with
/// radius `r`. Returns the new arc, if any.
pub(crate) fn fillet_curves(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, r: f64) -> Result<Option<Handle>> {
    if curves::is_locked(s, h1) || curves::is_locked(s, h2) {
        return Err(other("The object is on a locked layer."));
    }
    let e1 = curves::entity(s, h1)?;
    let e2 = curves::entity(s, h2)?;
    if [&e1, &e2].iter().any(|e| matches!(e.kind, EntityKind::Ellipse(_) | EntityKind::Spline(_))) {
        return curve_edits::fillet_curvy(s, h1, p1, h2, p2, r, &e1, &e2);
    }
    let unsupported = || other("Fillet works on lines, arcs, circles, elliptical arcs and splines (and polylines with the Polyline option).");
    let o1 = TanObj::from_kind(&e1.kind, p1).filter(|_| matches!(e1.kind, EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::Circle(_)));
    let o2 = TanObj::from_kind(&e2.kind, p2).filter(|_| matches!(e2.kind, EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::Circle(_)));
    let (Some(o1), Some(o2)) = (o1, o2) else { return Err(unsupported()) };
    if h1 == h2 {
        return Err(other("Cannot fillet an object to itself."));
    }
    let (t1, t2, fc) = if r <= 1e-12 {
        // Sharp corner: the intersection nearest the picks.
        let hits: Vec<Vec2> = match (o1, o2) {
            (TanObj::Line(a), TanObj::Line(b)) => line_line_infinite(a.a, a.b, b.a, b.b).map(|x| vec![x.0]).unwrap_or_default(),
            (TanObj::Line(l), TanObj::Circle(c)) | (TanObj::Circle(c), TanObj::Line(l)) => line_circle(&l, &c).into_iter().map(|x| x.0).collect(),
            (TanObj::Circle(a), TanObj::Circle(b)) => cadcraft_geom::circle_circle(&a, &b),
        };
        let x = hits
            .into_iter()
            .min_by(|a, b| (a.dist(p1) + a.dist(p2)).total_cmp(&(b.dist(p1) + b.dist(p2))))
            .ok_or_else(|| other("The objects do not intersect."))?;
        (x, x, None)
    } else {
        let c = curves::ttr(o1, p1, o2, p2, r).ok_or_else(|| other("Radius is too large."))?;
        (o1.touch(&c), o2.touch(&c), Some(c))
    };
    let (n1, d1) = trim_for_fillet(&e1.kind, p1, t1).ok_or_else(unsupported)?;
    let (n2, d2) = trim_for_fillet(&e2.kind, p2, t2).ok_or_else(unsupported)?;
    let new_arc = fc.map(|c| {
        let a1 = c.center.angle_to(t1);
        let a2 = c.center.angle_to(t2);
        let ccw1 = (t1 - c.center).perp().normalized();
        let ccw2 = (t2 - c.center).perp().normalized();
        let ccw = match (d1, d2) {
            (Some(d), _) => ccw1.dot(d) > 0.0,
            (None, Some(d)) => ccw2.dot(-d) > 0.0,
            _ => ccw_sweep(a1, a2) <= std::f64::consts::PI,
        };
        if ccw { Arc::new(c.center, c.radius, a1, a2) } else { Arc::new(c.center, c.radius, a2, a1) }
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
            Ok(Some(push_entity(s, &space, e1.common.clone(), arc(&a))?))
        }
        None => Ok(None),
    }
}

/// Fillet every corner between two straight segments of a polyline. Returns the count.
pub(crate) fn fillet_polyline(s: &mut Session, h: Handle, r: f64) -> Result<usize> {
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let e = curves::entity(s, h)?;
    let EntityKind::LwPolyline(pl) = &e.kind else { return Err(other("Select a 2D polyline.")) };
    let vs = &pl.vertices;
    let n = vs.len();
    if n < 3 {
        return Ok(0);
    }
    let at = |i: usize| vs.get(i % n).copied().unwrap_or_default();
    // Distance cut back from each corner (0 = not filleted).
    let mut cut = vec![0.0f64; n];
    let mut turn = vec![0.0f64; n];
    for (i, c) in cut.iter_mut().enumerate() {
        if !pl.closed && (i == 0 || i + 1 == n) {
            continue;
        }
        let prev = at(i + n - 1);
        let cur = at(i);
        let next = at(i + 1);
        if prev.bulge.abs() > 1e-12 || cur.bulge.abs() > 1e-12 {
            continue;
        }
        let u1 = (prev.p - cur.p).normalized();
        let u2 = (next.p - cur.p).normalized();
        if u1 == Vec2::ZERO || u2 == Vec2::ZERO {
            continue;
        }
        let half = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if half < 1e-9 || (std::f64::consts::FRAC_PI_2 - half).abs() < 1e-9 {
            continue;
        }
        *c = if r <= 1e-12 { 0.0 } else { r / half.tan() };
        if let Some(t) = turn.get_mut(i) {
            *t = (-u1).cross(u2).atan2((-u1).dot(u2));
        }
    }
    // Drop fillets that do not fit their segments.
    for i in 0..n {
        let j = (i + 1) % n;
        let seg = at(i).p.dist(at(j).p);
        let (ci, cj) = (cut.get(i).copied().unwrap_or(0.0), cut.get(j).copied().unwrap_or(0.0));
        if ci + cj > seg + 1e-9 {
            if let Some(x) = cut.get_mut(i) {
                *x = 0.0;
            }
            if let Some(x) = cut.get_mut(j) {
                *x = 0.0;
            }
        }
    }
    let mut out: Vec<PolyVertex> = Vec::with_capacity(n * 2);
    let mut count = 0;
    for i in 0..n {
        let v = at(i);
        let c = cut.get(i).copied().unwrap_or(0.0);
        if c <= 1e-12 {
            out.push(v);
            continue;
        }
        let prev = at(i + n - 1).p;
        let next = at(i + 1).p;
        let t1 = v.p + (prev - v.p).normalized() * c;
        let t2 = v.p + (next - v.p).normalized() * c;
        let b = cadcraft_geom::arc_to_bulge(turn.get(i).copied().unwrap_or(0.0));
        out.push(PolyVertex { p: t1, bulge: b, ..v });
        out.push(PolyVertex { p: t2, bulge: 0.0, ..v });
        count += 1;
    }
    set_kind(s, h, EntityKind::LwPolyline(LwPolyline { vertices: out, ..pl.clone() }))?;
    Ok(count)
}
