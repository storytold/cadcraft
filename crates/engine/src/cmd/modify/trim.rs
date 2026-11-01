use super::{
    Arc, EngineError, Entity, EntityKind, Handle, Line, PolyVertex, Polyline, Prim, Result, Segment, Session, TAU, Vec2, angle_in_sweep, arc,
    ccw_sweep, intersect_ext, line, lwpoly, norm_angle,
};

// ---------------- trim / extend ----------------

/// The cutting (TRIM) or boundary (EXTEND) edges.
#[derive(Clone, Copy, Default)]
pub(crate) struct Edges<'a> {
    /// Only these objects; `None`: every visible object.
    pub only: Option<&'a [Handle]>,
    /// Implied edges (EDGEMODE 1): lines and the end segments of open polylines count as running
    /// on without end, arcs as whole circles.
    pub implied: bool,
    /// Quick mode (TRIMEXTENDMODE 1), where `only: None` means every object but the one trimmed.
    /// A trimmed polyline that is one of the edges cuts itself too: picked as one, or with every
    /// object in Standard mode.
    pub quick: bool,
}

/// All primitives of the cutting edges (excluding `exclude`).
fn edge_segments(s: &Session, edges: Edges, exclude: Handle) -> Result<Vec<Segment>> {
    let d = s.doc()?;
    let space = s.space();
    let store = d.space(&space).ok_or(EngineError::NoDocument)?;
    let mut out = Vec::new();
    for e in store.iter() {
        if e.handle == exclude || !d.is_visible(e) {
            continue;
        }
        if let Some(es) = edges.only
            && !es.contains(&e.handle)
        {
            continue;
        }
        if edges.implied
            && let Some(segs) = implied_edge(&e.kind)
        {
            out.extend(segs);
            continue;
        }
        for p in e.kind.prims() {
            out.extend(prim_to_segments(&p));
        }
    }
    Ok(out)
}

/// `k` as an implied edge (EDGEMODE 1), for lines, arcs and open polylines.
fn implied_edge(k: &EntityKind) -> Option<Vec<Segment>> {
    const FAR: f64 = 1e8;
    let whole = |a: &Arc| Segment::Arc { arc: Arc { start: 0.0, end: TAU - 1e-12, ..*a }, ccw: true };
    // Run a segment on past its start and/or its end.
    let run_on = |sg: Segment, back: bool, fwd: bool| match sg {
        Segment::Line(l) => {
            let d = (l.b - l.a).normalized();
            Segment::Line(Line::new(if back { l.a - d * FAR } else { l.a }, if fwd { l.b + d * FAR } else { l.b }))
        }
        Segment::Arc { arc, .. } => whole(&arc),
    };
    match k {
        EntityKind::Line(l) => Some(vec![run_on(Segment::Line(Line::new(l.a.xy(), l.b.xy())), true, true)]),
        EntityKind::Arc(a) => Some(vec![whole(&Arc::new(a.center.xy(), a.radius, a.start, a.end))]),
        EntityKind::LwPolyline(p) if !p.closed => {
            let mut segs = Polyline { vertices: p.vertices.clone(), closed: false }.segments();
            let last = segs.len().checked_sub(1)?;
            for (i, sg) in segs.iter_mut().enumerate() {
                if i == 0 || i == last {
                    *sg = run_on(*sg, i == 0, i == last);
                }
            }
            Some(segs)
        }
        _ => None,
    }
}

fn prim_to_segments(p: &Prim) -> Vec<Segment> {
    match p {
        Prim::Seg(s) => vec![*s],
        Prim::Circle(c) => vec![Segment::Arc { arc: Arc { center: c.center, radius: c.radius, start: 0.0, end: TAU - 1e-12 }, ccw: true }],
        Prim::Infinite { base, dir, .. } => vec![Segment::Line(Line::new(*base - *dir * 1e8, *base + *dir * 1e8))],
        Prim::Ellipse(e) => {
            let mut pts = Vec::new();
            e.tessellate(e.major.len() * 1e-4, &mut pts);
            pts.windows(2).filter_map(|w| Some(Segment::Line(Line::new(*w.first()?, *w.get(1)?)))).collect()
        }
        Prim::Spline(sp) => sp.tessellate(1e-4).windows(2).filter_map(|w| Some(Segment::Line(Line::new(*w.first()?, *w.get(1)?)))).collect(),
        Prim::Fill(f) => {
            let n = f.len();
            (0..n).filter_map(|i| Some(Segment::Line(Line::new(*f.get(i)?, *f.get((i + 1) % n)?)))).collect()
        }
        Prim::Point(_) => Vec::new(),
    }
}

/// Trim `h` at the piece containing `pick`. Returns what it became: its pieces, `h` first
/// ([`replace_with_pieces`]).
pub(crate) fn trim(s: &mut Session, h: Handle, pick: Vec2, edges: Edges) -> Result<Vec<Handle>> {
    if crate::cmd::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let cut = edge_segments(s, edges, h)?;
    let itself = edges.only.map_or(!edges.quick, |o| o.contains(&h));
    let pieces: Vec<EntityKind> = match &e.kind {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let mut ts: Vec<f64> = cut
                .iter()
                .flat_map(|c| intersect_ext(&Segment::Line(ln), c, false))
                .map(|x| ln.param_of(x))
                .filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9)
                .collect();
            ts.sort_by(f64::total_cmp);
            let tp = ln.param_of(pick).clamp(0.0, 1.0);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(0.0, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(1.0, f64::min);
            if lo == 0.0 && hi == 1.0 {
                return Err(EngineError::Other("Object does not intersect a cutting edge.".into()));
            }
            let mut v = Vec::new();
            if lo > 0.0 {
                v.push(line(ln.a, ln.at(lo)));
            }
            if hi < 1.0 {
                v.push(line(ln.at(hi), ln.b));
            }
            v
        }
        EntityKind::Arc(a) => {
            let ga = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let seg = Segment::Arc { arc: ga, ccw: true };
            let sweep = ga.sweep();
            let param = |p: Vec2| norm_angle(ga.center.angle_to(p) - ga.start);
            let mut ts: Vec<f64> =
                cut.iter().flat_map(|c| intersect_ext(&seg, c, false)).map(param).filter(|t| *t > 1e-9 && *t < sweep - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            let tp = param(pick).min(sweep);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(0.0, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(sweep, f64::min);
            if lo == 0.0 && hi == sweep {
                return Err(EngineError::Other("Object does not intersect a cutting edge.".into()));
            }
            let mut v = Vec::new();
            if lo > 0.0 {
                v.push(arc(&Arc::new(ga.center, ga.radius, ga.start, ga.start + lo)));
            }
            if hi < sweep {
                v.push(arc(&Arc::new(ga.center, ga.radius, ga.start + hi, ga.end)));
            }
            v
        }
        EntityKind::Circle(c) => {
            let full = Segment::Arc { arc: Arc { center: c.center.xy(), radius: c.radius, start: 0.0, end: TAU - 1e-12 }, ccw: true };
            let mut angs: Vec<f64> = cut.iter().flat_map(|x| intersect_ext(&full, x, false)).map(|p| c.center.xy().angle_to(p)).collect();
            angs.sort_by(f64::total_cmp);
            angs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            if angs.len() < 2 {
                return Err(EngineError::Other("Circle must intersect cutting edges at two or more points.".into()));
            }
            let ap = c.center.xy().angle_to(pick);
            // The removed arc runs between the two intersections around the pick.
            let n = angs.len();
            let mut keep = None;
            for i in 0..n {
                let a0 = angs[i];
                let a1 = angs[(i + 1) % n];
                if angle_in_sweep(ap, a0, a1) {
                    keep = Some(arc(&Arc::new(c.center.xy(), c.radius, a1, a0)));
                }
            }
            keep.into_iter().collect()
        }
        EntityKind::LwPolyline(p) => trim_polyline(p, &cut, pick, itself)?,
        k => crate::cmd::modify2::trim_other(k, &cut, pick)?,
    };
    replace_with_pieces(s, h, pieces)
}

/// Put `pieces` in place of object `h` as AutoCAD 2024's BREAK and TRIM leave them: the first
/// piece (the one holding the object's start, or the only piece of a circle or closed polyline)
/// is the object itself, keeping its handle, its place in the drawing order, its groups and its
/// extended data; the others are new objects with its properties, added at the end of its space
/// (`entlast`); with no pieces the object is erased. Returns the pieces' handles. (A line broken
/// in its middle keeps its handle on the piece from its start point, the piece to its end point is
/// new; a circle broken or trimmed becomes an arc with the circle's handle.)
///
/// Each piece keeps what the object had beyond its outline ([`inherit`]). Dimensions attached to
/// the object follow [`crate::assoc::follow_pieces`]. Its parametric constraints are removed: its
/// new shape no longer meets them, and solving them again would move the objects they tie it to.
pub(super) fn replace_with_pieces(s: &mut Session, h: Handle, pieces: Vec<EntityKind>) -> Result<Vec<Handle>> {
    let d = s.doc_mut()?;
    if pieces.is_empty() {
        d.remove_entity(h);
        return Ok(Vec::new());
    }
    let (Some(space), Some(old)) = (d.space_of(h), d.entity(h).cloned()) else {
        return Err(EngineError::Other("no such object".into()));
    };
    let mut pieces = pieces.into_iter().map(|k| inherit(&old.kind, k));
    let Some(first) = pieces.next() else { return Ok(Vec::new()) };
    d.modify_entity(h, |e| e.kind = first)?;
    let mut out = vec![h];
    for kind in pieces {
        let handle = d.new_handle();
        if let Some(st) = d.space_mut(&space) {
            st.push(Entity { handle, common: old.common.clone(), kind });
            out.push(handle);
        }
    }
    crate::assoc::follow_pieces(d, &old.kind, &out);
    let n = d.constraints.len();
    d.constraints.retain(|c| c.refs.iter().all(|r| r.handle != h));
    let removed = n - d.constraints.len();
    if removed > 0 {
        s.echo(format!("{removed} constraint(s) of the broken or trimmed object removed."));
    }
    Ok(out)
}

/// `piece`, cut from `source`, with what it keeps of it as AutoCAD 2024's BREAK and TRIM leave
/// pieces: a line's Z along it, an arc's (or a circle's) centre Z, and a polyline's elevation,
/// constant width and linetype generation.
fn inherit(source: &EntityKind, mut piece: EntityKind) -> EntityKind {
    match (source, &mut piece) {
        (EntityKind::Line(s), EntityKind::Line(p)) => {
            let ln = Line::new(s.a.xy(), s.b.xy());
            if ln.len() > 0.0 {
                let z = |t: f64| s.a.z + (s.b.z - s.a.z) * t;
                p.a.z = z(ln.param_of(p.a.xy()));
                p.b.z = z(ln.param_of(p.b.xy()));
            }
        }
        (EntityKind::Arc(s), EntityKind::Arc(p)) => p.center.z = s.center.z,
        (EntityKind::Circle(s), EntityKind::Arc(p)) => p.center.z = s.center.z,
        (EntityKind::LwPolyline(s), EntityKind::LwPolyline(p)) => {
            p.elevation = s.elevation;
            p.const_width = s.const_width;
            p.plinegen = s.plinegen;
        }
        _ => {}
    }
    piece
}

/// Sub-polyline between length parameters `a` and `b` (open polylines).
pub(super) fn sub_polyline(segs: &[Segment], a: f64, b: f64) -> Vec<PolyVertex> {
    let mut out: Vec<PolyVertex> = Vec::new();
    let mut acc = 0.0;
    for s in segs {
        let len = s.len();
        let s0 = acc;
        let s1 = acc + len;
        acc = s1;
        if s1 <= a + 1e-12 || s0 >= b - 1e-12 || len <= 1e-15 {
            continue;
        }
        let t0 = ((a - s0) / len).clamp(0.0, 1.0);
        let t1 = ((b - s0) / len).clamp(0.0, 1.0);
        let p0 = s.at(t0);
        let bulge = match s {
            Segment::Line(_) => 0.0,
            Segment::Arc { arc, ccw } => {
                let sw = arc.sweep() * (t1 - t0);
                cadcraft_geom::arc_to_bulge(if *ccw { sw } else { -sw })
            }
        };
        if out.last().is_none_or(|l| !l.p.near(p0, 1e-9)) {
            out.push(PolyVertex { p: p0, bulge, ..Default::default() });
        } else if let Some(l) = out.last_mut() {
            l.bulge = bulge;
        }
        out.push(PolyVertex::new(s.at(t1)));
    }
    out
}

/// Upper bounds on the segments of a polyline searched for crossings with itself (the pairs grow
/// with the square) and on the crossings kept (hostile files); a longer polyline still cuts itself
/// at its vertices.
const MAX_SELF_CROSSING_SEGMENTS: usize = 2048;
const MAX_SELF_CROSSINGS: usize = 10_000;

/// Where a polyline cuts itself, as lengths along it: the vertices between its segments (every
/// vertex of a closed one, its start included) and the points where two of its segments cross.
/// AutoCAD 2024 trims a polyline that is one of the cutting edges at these.
fn self_cuts(segs: &[Segment], closed: bool) -> Vec<f64> {
    let mut starts = Vec::with_capacity(segs.len());
    let mut acc = 0.0;
    for s in segs {
        starts.push(acc);
        acc += s.len();
    }
    let mut ts: Vec<f64> = starts.iter().copied().skip(usize::from(!closed)).collect();
    if segs.len() > MAX_SELF_CROSSING_SEGMENTS {
        return ts;
    }
    let inside = |t: f64| t > 1e-9 && t < 1.0 - 1e-9;
    let parts: Vec<_> = segs.iter().zip(starts).map(|(s, t0)| (s, s.bounds(), t0)).collect();
    let mut crossings = 0;
    for (i, (a, box_a, a0)) in parts.iter().enumerate() {
        for (b, box_b, b0) in parts.iter().skip(i + 1) {
            if !box_a.intersects(box_b) {
                continue;
            }
            for x in intersect_ext(a, b, false) {
                let (ta, tb) = (param_on(a, x), param_on(b, x));
                if inside(ta) && inside(tb) {
                    ts.push(a0 + ta * a.len());
                    ts.push(b0 + tb * b.len());
                    crossings += 1;
                    if crossings >= MAX_SELF_CROSSINGS {
                        return ts;
                    }
                }
            }
        }
    }
    ts
}

/// Trim a polyline at the piece containing `pick`, between the cuts by `cut` and, when it is one
/// of the cutting edges (`itself`), by its own vertices and crossings ([`self_cuts`]).
fn trim_polyline(p: &cadcraft_doc::LwPolyline, cut: &[Segment], pick: Vec2, itself: bool) -> Result<Vec<EntityKind>> {
    let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
    let segs = pl.segments();
    let total: f64 = segs.iter().map(Segment::len).sum();
    let mut ts: Vec<f64> = Vec::new();
    let mut acc = 0.0;
    let mut tp = 0.0;
    let mut best = f64::INFINITY;
    for s in &segs {
        let len = s.len();
        for x in cut.iter().flat_map(|c| intersect_ext(s, c, false)) {
            ts.push(acc + param_on(s, x) * len);
        }
        let c = s.closest(pick);
        if c.dist(pick) < best {
            best = c.dist(pick);
            tp = acc + param_on(s, c) * len;
        }
        acc += len;
    }
    ts.retain(|t| *t > 1e-9 && *t < total - 1e-9);
    if itself {
        // A closed polyline also cuts itself at its start vertex.
        ts.extend(self_cuts(&segs, p.closed).into_iter().filter(|t| (*t > 1e-9 || p.closed) && *t < total - 1e-9));
    }
    ts.sort_by(f64::total_cmp);
    if ts.is_empty() {
        return Err(EngineError::Other("Object does not intersect a cutting edge.".into()));
    }
    let lo = ts.iter().copied().filter(|t| *t < tp).fold(f64::NEG_INFINITY, f64::max);
    let hi = ts.iter().copied().filter(|t| *t > tp).fold(f64::INFINITY, f64::min);
    let mk = |vs: Vec<PolyVertex>| lwpoly(vs, false);
    let mut out = Vec::new();
    if p.closed {
        // Keep the complementary run from hi around to lo.
        // The pick lies before the first cut or after the last: the piece wraps past the start
        // vertex, so the run to keep is the stretch between the first and the last cut.
        let (lo, hi) = (
            if lo.is_finite() { lo } else { ts.last().copied().unwrap_or(0.0) - total },
            if hi.is_finite() { hi } else { ts.first().copied().unwrap_or(total) + total },
        );
        let doubled: Vec<Segment> = segs.iter().chain(segs.iter()).copied().collect();
        let (a, mut b) = if hi > total { (hi - total, lo) } else { (hi, lo + total) };
        // A single cut leaves the whole outline, opened there.
        if b <= a + 1e-9 {
            b += total;
        }
        out.push(mk(sub_polyline(&doubled, a, b)));
    } else {
        if lo.is_finite() {
            out.push(mk(sub_polyline(&segs, 0.0, lo)));
        }
        if hi.is_finite() {
            out.push(mk(sub_polyline(&segs, hi, total)));
        }
    }
    Ok(out.into_iter().filter(|k| matches!(k, EntityKind::LwPolyline(l) if l.vertices.len() >= 2)).collect())
}

pub(super) fn param_on(s: &Segment, p: Vec2) -> f64 {
    match s {
        Segment::Line(l) => l.param_of(p).clamp(0.0, 1.0),
        Segment::Arc { arc, ccw } => {
            let t = norm_angle(arc.center.angle_to(p) - arc.start) / arc.sweep().max(1e-15);
            let t = t.clamp(0.0, 1.0);
            if *ccw { t } else { 1.0 - t }
        }
    }
}

/// Extend the end of `h` nearest `pick` to the nearest boundary edge.
pub(crate) fn extend(s: &mut Session, h: Handle, pick: Vec2, edges: Edges) -> Result<()> {
    if crate::cmd::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let cut = edge_segments(s, edges, h)?;
    let new_kind = match &e.kind {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let from_b = pick.dist(b) < pick.dist(a);
            let (fixed, end) = if from_b { (a, b) } else { (b, a) };
            let dir = (end - fixed).normalized();
            let ray = Segment::Line(Line::new(end, end + dir * 1e8));
            let best = cut
                .iter()
                .flat_map(|c| intersect_ext(&ray, c, false))
                .filter(|x| (*x - end).dot(dir) > 1e-9)
                .min_by(|x, y| x.dist(end).total_cmp(&y.dist(end)));
            let np = best.ok_or_else(|| EngineError::Other("Object does not intersect an edge.".into()))?;
            if from_b { line(a, np) } else { line(np, b) }
        }
        EntityKind::Arc(a) => {
            let ga = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let from_end = pick.dist(ga.end_point()) < pick.dist(ga.start_point());
            let full = Segment::Arc { arc: Arc { start: 0.0, end: TAU - 1e-12, ..ga }, ccw: true };
            let hits: Vec<f64> =
                cut.iter().flat_map(|c| intersect_ext(&full, c, false)).map(|x| ga.center.angle_to(x)).filter(|t| !ga.contains_angle(*t)).collect();
            let pick_ang = if from_end {
                hits.iter().copied().min_by(|x, y| ccw_sweep(ga.end, *x).total_cmp(&ccw_sweep(ga.end, *y)))
            } else {
                hits.iter().copied().min_by(|x, y| ccw_sweep(*x, ga.start).total_cmp(&ccw_sweep(*y, ga.start)))
            };
            let t = pick_ang.ok_or_else(|| EngineError::Other("Object does not intersect an edge.".into()))?;
            if from_end { arc(&Arc::new(ga.center, ga.radius, ga.start, t)) } else { arc(&Arc::new(ga.center, ga.radius, t, ga.end)) }
        }
        EntityKind::LwPolyline(p) if !p.closed && p.vertices.len() >= 2 => {
            let mut vs = p.vertices.clone();
            let n = vs.len();
            let from_end = vs.last().is_some_and(|l| pick.dist(l.p) < vs.first().map(|f| pick.dist(f.p)).unwrap_or(f64::INFINITY));
            let (i_end, i_prev) = if from_end { (n - 1, n - 2) } else { (0, 1) };
            let end = vs.get(i_end).map(|v| v.p).unwrap_or_default();
            let prev = vs.get(i_prev).map(|v| v.p).unwrap_or_default();
            let dir = (end - prev).normalized();
            let ray = Segment::Line(Line::new(end, end + dir * 1e8));
            let np = cut
                .iter()
                .flat_map(|c| intersect_ext(&ray, c, false))
                .filter(|x| (*x - end).dot(dir) > 1e-9)
                .min_by(|x, y| x.dist(end).total_cmp(&y.dist(end)))
                .ok_or_else(|| EngineError::Other("Object does not intersect an edge.".into()))?;
            if let Some(v) = vs.get_mut(i_end) {
                v.p = np;
            }
            lwpoly(vs, false)
        }
        k => crate::cmd::modify2::extend_other(k, &cut, pick)?,
    };
    s.doc_mut()?.modify_entity(h, |e| e.kind = new_kind)?;
    Ok(())
}
