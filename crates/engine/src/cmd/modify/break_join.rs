use super::trim::{param_on, sub_polyline};
use super::{
    Accept, Arc, EngineError, Entity, EntityKind, Handle, Input, Interactive, Line, PolyVertex, Polyline, Prompt, Result, Segment, Session, Step,
    Value, Vec2, arc, bad, json, lwpoly, norm_angle, point_param, point_req, targets,
};

/// Gives a rebuilt arc piece its source centre Z, or a rebuilt polyline piece its source elevation.
fn with_z(mut k: EntityKind, z: f64) -> EntityKind {
    match &mut k {
        EntityKind::Arc(a) => a.center.z = z,
        EntityKind::LwPolyline(p) => p.elevation = z,
        _ => {}
    }
    k
}

/// Break `h` between two points (or at one point when p1 == p2).
pub(crate) fn break_entity(s: &mut Session, h: Handle, p1: Vec2, p2: Vec2) -> Result<Vec<Handle>> {
    if crate::cmd::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let pieces: Vec<EntityKind> = match &e.kind {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let (mut t1, mut t2) = (ln.param_of(p1).clamp(0.0, 1.0), ln.param_of(p2).clamp(0.0, 1.0));
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            let at3 = |t: f64| ln.at(t).to3(l.a.z + (l.b.z - l.a.z) * t);
            let mut v = Vec::new();
            if t1 > 1e-12 {
                v.push(EntityKind::Line(cadcraft_doc::Line { a: l.a, b: at3(t1) }));
            }
            if t2 < 1.0 - 1e-12 {
                v.push(EntityKind::Line(cadcraft_doc::Line { a: at3(t2), b: l.b }));
            }
            v
        }
        EntityKind::Circle(c) => {
            if p1.near(p2, 1e-12) {
                return Err(EngineError::Other("Arc cannot be full 360 degrees.".into()));
            }
            let a1 = c.center.xy().angle_to(p1);
            let a2 = c.center.xy().angle_to(p2);
            // AutoCAD removes the CCW portion from the first to the second point.
            vec![with_z(arc(&Arc::new(c.center.xy(), c.radius, a2, a1)), c.center.z)]
        }
        EntityKind::Arc(a) => {
            let ga = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let par = |p: Vec2| norm_angle(ga.center.angle_to(p) - ga.start).min(ga.sweep());
            let (mut t1, mut t2) = (par(p1), par(p2));
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            let mut v = Vec::new();
            if t1 > 1e-12 {
                v.push(with_z(arc(&Arc::new(ga.center, ga.radius, ga.start, ga.start + t1)), a.center.z));
            }
            if t2 < ga.sweep() - 1e-12 {
                v.push(with_z(arc(&Arc::new(ga.center, ga.radius, ga.start + t2, ga.end)), a.center.z));
            }
            v
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            let segs = pl.segments();
            let total: f64 = segs.iter().map(Segment::len).sum();
            let lp = |q: Vec2| {
                let mut acc = 0.0;
                let mut best = (f64::INFINITY, 0.0);
                for s in &segs {
                    let c = s.closest(q);
                    let dd = c.dist(q);
                    if dd < best.0 {
                        best = (dd, acc + param_on(s, c) * s.len());
                    }
                    acc += s.len();
                }
                best.1
            };
            let (mut t1, mut t2) = (lp(p1), lp(p2));
            if p.closed {
                if (t1 - t2).abs() < 1e-9 {
                    return Err(EngineError::Other("Cannot break a closed polyline at a single point.".into()));
                }
                // Like a circle: the stretch from the first point to the second, along the
                // vertices, goes; what is left runs from the second point round to the first.
                let doubled: Vec<Segment> = segs.iter().chain(segs.iter()).copied().collect();
                let end = if t1 > t2 { t1 } else { t1 + total };
                vec![with_z(lwpoly(sub_polyline(&doubled, t2, end), false), p.elevation)]
            } else {
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                let mut v = Vec::new();
                if t1 > 1e-12 {
                    v.push(with_z(lwpoly(sub_polyline(&segs, 0.0, t1), false), p.elevation));
                }
                if t2 < total - 1e-12 {
                    v.push(with_z(lwpoly(sub_polyline(&segs, t2, total), false), p.elevation));
                }
                v
            }
        }
        _ => return Err(EngineError::Other("Cannot break that object.".into())),
    };
    let common = e.common.clone();
    let space = s.space();
    let d = s.doc_mut()?;
    d.remove_entity(h);
    let mut out = Vec::new();
    for k in pieces {
        let nh = d.new_handle();
        if let Some(st) = d.space_mut(&space) {
            st.push(Entity { handle: nh, common: common.clone(), kind: k });
            out.push(nh);
        }
    }
    Ok(out)
}

pub(super) fn run_break(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("break", "`handle` is required"))?;
    let p1 = point_req("break", p, "p1")?;
    let p2 = point_param(p, "p2").unwrap_or(p1);
    let r = break_entity(s, h, p1, p2)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

pub(super) fn run_breakat(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("breakatpoint", "`handle` is required"))?;
    let at = point_req("breakatpoint", p, "at")?;
    let r = break_entity(s, h, at, at)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// Join lines/arcs/polylines that share endpoints into polylines (or merge collinear lines).
pub(crate) fn join(s: &mut Session, hs: &[Handle]) -> Result<Vec<Handle>> {
    if let Some(h) = crate::cmd::modify2::join_curves(s, hs)? {
        return Ok(vec![h]);
    }
    if hs.iter().any(|h| crate::cmd::curves::is_locked(s, *h)) {
        return Err(EngineError::Other("An object is on a locked layer.".into()));
    }
    let d = s.doc()?;
    // Every segment with the entity it came from, so only entities that end up in a chain are removed.
    let mut pool: Vec<(Segment, Handle)> = Vec::new();
    let mut entity_common = std::collections::HashMap::new();
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        entity_common.insert(*h, e.common.clone());
        match &e.kind {
            EntityKind::Line(l) => pool.push((Segment::Line(Line::new(l.a.xy(), l.b.xy())), *h)),
            EntityKind::Arc(a) => pool.push((Segment::Arc { arc: Arc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true }, *h)),
            EntityKind::LwPolyline(p) if !p.closed => {
                for seg in (Polyline { vertices: p.vertices.clone(), closed: false }).segments() {
                    pool.push((seg, *h));
                }
            }
            _ => {}
        }
    }
    if pool.len() < 2 {
        return Err(EngineError::Other("Select at least two objects to join.".into()));
    }

    let tol = 1e-6;
    // Segments still available to chain, and how many each source entity contributed.
    let mut total: std::collections::HashMap<Handle, usize> = std::collections::HashMap::new();
    for (_, owner) in &pool {
        *total.entry(*owner).or_default() += 1;
    }
    let mut chains: Vec<(Vec<Segment>, Vec<Handle>)> = Vec::new();

    // Chain greedily from each remaining segment; a closed loop ends its chain.
    while !pool.is_empty() {
        let (first_seg, first_owner) = pool.remove(0);
        let mut chain = vec![first_seg];
        let mut used = vec![first_owner];

        loop {
            let start = chain.first().map(Segment::start).unwrap_or_default();
            let end = chain.last().map(Segment::end).unwrap_or_default();

            // Stop growing this chain once it closes into a loop.
            if chain.len() >= 2 && start.near(end, tol) {
                break;
            }

            let Some(i) = pool
                .iter()
                .position(|(s, _)| s.start().near(end, tol) || s.end().near(end, tol) || s.start().near(start, tol) || s.end().near(start, tol))
            else {
                break;
            };

            let (s0, owner) = pool.remove(i);
            used.push(owner);
            if s0.start().near(end, tol) {
                chain.push(s0);
            } else if s0.end().near(end, tol) {
                chain.push(s0.reversed());
            } else if s0.end().near(start, tol) {
                chain.insert(0, s0);
            } else {
                chain.insert(0, s0.reversed());
            }
        }

        // A polyline taken into a chain must be consumed whole: deleting a partly used entity
        // would lose the segments that stayed out (issue #949).
        let mut used_counts: std::collections::HashMap<Handle, usize> = std::collections::HashMap::new();
        for owner in &used {
            *used_counts.entry(*owner).or_default() += 1;
        }
        let fully_consumed = used_counts.iter().all(|(owner, count)| total.get(owner) == Some(count));

        let closed = chain.len() >= 2 && chain.first().zip(chain.last()).is_some_and(|(a, b)| a.start().near(b.end(), tol));
        let mut unique_owners = used.clone();
        unique_owners.sort();
        unique_owners.dedup();

        if fully_consumed && (unique_owners.len() >= 2 || closed) {
            chains.push((chain, used));
        }
    }

    if chains.is_empty() {
        return Err(EngineError::Other("Objects are not connected.".into()));
    }

    let space = s.space();
    let doc = s.doc_mut()?;
    let mut out = Vec::new();

    for (chain, used) in chains {
        let closed = chain.len() >= 2 && chain.first().zip(chain.last()).is_some_and(|(a, b)| a.start().near(b.end(), tol));
        let mut vs: Vec<PolyVertex> = chain
            .iter()
            .map(|s| PolyVertex {
                p: s.start(),
                bulge: match s {
                    Segment::Line(_) => 0.0,
                    Segment::Arc { arc, ccw } => cadcraft_geom::arc_to_bulge(if *ccw { arc.sweep() } else { -arc.sweep() }),
                },
                ..Default::default()
            })
            .collect();
        if !closed && let Some(l) = chain.last() {
            vs.push(PolyVertex::new(l.end()));
        }

        let mut unique_owners = used.clone();
        unique_owners.sort();
        unique_owners.dedup();
        for h in &unique_owners {
            doc.remove_entity(*h);
        }

        let common = used.first().and_then(|h| entity_common.get(h).cloned()).unwrap_or_default();
        let nh = doc.new_handle();
        if let Some(st) = doc.space_mut(&space) {
            st.push(Entity { handle: nh, common, kind: lwpoly(vs, closed) });
        }
        out.push(nh);
    }

    s.set_selection(Vec::new());
    Ok(out)
}

pub(super) fn run_join(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if p.get("close").and_then(Value::as_bool) == Some(true) {
        let h = crate::cmd::modify2::join_close(s, &hs)?;
        return Ok(json!({ "handle": h.hex(), "handles": [h.hex()] }));
    }
    let rs = join(s, &hs)?;
    Ok(json!({
        "handle": rs.first().map(|h| h.hex()),
        "handles": rs.iter().map(|h| h.hex()).collect::<Vec<_>>()
    }))
}

#[derive(Default)]
pub(super) struct BreakM {
    obj: Option<(Handle, Vec2)>,
    /// The `First point` option: the next point replaces the selection pick as the first break point.
    first_point: bool,
}

impl Interactive for BreakM {
    fn name(&self) -> &'static str {
        "BREAK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.obj {
            None => Prompt::new("Select object", Accept::POINT),
            Some(_) if self.first_point => Prompt::new("Specify first break point", Accept::POINT),
            Some(_) => Prompt::new("Specify second break point", Accept::POINT).kw(&["First point"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.obj, i) {
            (None, Input::Point(p)) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                match s.pick_object(&space, p, ap)? {
                    Some(h) => {
                        self.obj = Some((h, p));
                        s.set_selection(vec![h]);
                    }
                    None => s.echo("*Invalid selection*"),
                }
                Ok(Step::Continue)
            }
            (Some(_), Input::Keyword(k)) if k == "First point" => {
                self.first_point = true;
                Ok(Step::Continue)
            }
            (Some((h, _)), Input::Point(p1)) if self.first_point => {
                self.obj = Some((h, p1));
                self.first_point = false;
                Ok(Step::Continue)
            }
            (Some((h, p1)), Input::Point(p2)) => {
                break_entity(s, h, p1, p2)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}
