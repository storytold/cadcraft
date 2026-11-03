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

/// Join lines/arcs/polylines that share endpoints into one polyline (or merge collinear lines).
pub(crate) fn join(s: &mut Session, hs: &[Handle]) -> Result<Option<Handle>> {
    if let Some(h) = crate::cmd::modify2::join_curves(s, hs)? {
        return Ok(Some(h));
    }
    if hs.iter().any(|h| crate::cmd::curves::is_locked(s, *h)) {
        return Err(EngineError::Other("An object is on a locked layer.".into()));
    }
    let hs = joinable_picks(s.doc()?, hs);
    let hs = &hs[..];
    if let Some(h) = join_on_source(s, hs)? {
        return Ok(Some(h));
    }
    let d = s.doc()?;
    // Each object JOIN can take, as the pieces it is made of: a line or an arc is one piece, an
    // open polyline its segments in order, each keeping its vertices, bulge and widths exactly.
    let mut units: Vec<Vec<Piece>> = Vec::new();
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        let piece = |ends: (Vec2, Vec2), bulge: f64, widths: Option<(f64, f64)>| Piece { owner: *h, ends, bulge, widths };
        let unit: Vec<Piece> = match &e.kind {
            // A line of no length joins as AutoCAD 2024 joins it, a repeated vertex.
            EntityKind::Line(l) => vec![piece((l.a.xy(), l.b.xy()), 0.0, None)],
            EntityKind::Arc(a) => {
                let arc = Arc::new(a.center.xy(), a.radius, a.start, a.end);
                vec![piece((arc.start_point(), arc.end_point()), cadcraft_geom::arc_to_bulge(arc.sweep()), None)]
            }
            // Repeated vertices included: AutoCAD 2024 keeps them.
            EntityKind::LwPolyline(p) if !p.closed => p
                .vertices
                .windows(2)
                .map(|w| {
                    let widths = if p.const_width > 0.0 { (p.const_width, p.const_width) } else { (w[0].start_width, w[0].end_width) };
                    piece((w[0].p, w[1].p), w[0].bulge, Some(widths))
                })
                .collect(),
            _ => Vec::new(),
        };
        if !unit.is_empty() {
            units.push(unit);
        }
    }
    if units.iter().map(Vec::len).sum::<usize>() < 2 {
        return Err(EngineError::Other("Select at least two objects to join.".into()));
    }
    // A new polyline takes the source's properties.
    let first = units.first().and_then(|u| u.first()).map(|p| p.owner);
    let common = first.and_then(|h| d.entity(h)).map(|e| e.common.clone());
    let tol = 1e-6;
    let links = chain_units(units, tol);
    // One object alone joins nothing, even a polyline whose ends meet: AutoCAD 2024 leaves it open
    // ("0 segments added to polyline").
    let closed = links.first().zip(links.last()).is_some_and(|(a, b)| a.ends.0.near(b.ends.1, tol));
    let one_object = links.first().is_some_and(|f| links.iter().all(|l| l.owner == f.owner));
    if links.len() < 2 || one_object {
        return Err(EngineError::Other("Objects are not connected.".into()));
    }
    let used = |h: &Handle| links.iter().any(|l| l.owner == *h);
    // A polyline's own segments keep their widths.
    let mut vs: Vec<PolyVertex> = links
        .iter()
        .map(|l| {
            let (start_width, end_width) = l.widths.unwrap_or_default();
            PolyVertex { p: l.ends.0, bulge: l.bulge, start_width, end_width }
        })
        .collect();
    if !closed && let Some(l) = links.last() {
        vs.push(PolyVertex::new(l.ends.1));
    }
    // A new polyline goes in the source's space, whichever space is current.
    let space = first.and_then(|h| d.space_of(h)).unwrap_or_else(|| s.space());
    let doc = s.doc_mut()?;
    // A polyline source takes the other pieces and keeps its handle (AutoCAD 2024: "1 segments
    // added to polyline").
    if let Some(src) = first
        && let Some(EntityKind::LwPolyline(sp)) = doc.entity(src).map(|e| &e.kind)
        && let Some(onto) = onto_polyline(sp, src, &links, closed)
    {
        for h in hs.iter().filter(|h| **h != src && used(h)) {
            doc.remove_entity(*h);
        }
        doc.modify_entity(src, |e| e.kind = EntityKind::LwPolyline(onto))?;
        s.set_selection(Vec::new());
        return Ok(Some(src));
    }
    for h in hs.iter().filter(|h| used(h)) {
        doc.remove_entity(*h);
    }
    let nh = doc.new_handle();
    if let Some(st) = doc.space_mut(&space) {
        st.push(Entity { handle: nh, common: common.unwrap_or_default(), kind: lwpoly(vs, closed) });
    }
    s.set_selection(Vec::new());
    Ok(Some(nh))
}

/// The picks JOIN can take, each once, in the order picked: lines, arcs and open polylines. The
/// first of them is the source (AutoCAD 2024 passes over a circle picked first and leaves it).
fn joinable_picks(d: &cadcraft_doc::Drawing, hs: &[Handle]) -> Vec<Handle> {
    let mut seen = std::collections::HashSet::new();
    let joinable = |h: &Handle| {
        d.entity(*h).is_some_and(|e| match &e.kind {
            EntityKind::Line(_) => true,
            EntityKind::Arc(a) => a.radius > cadcraft_geom::EPS,
            EntityKind::LwPolyline(p) => !p.closed,
            _ => false,
        })
    };
    hs.iter().copied().filter(|h| joinable(h) && seen.insert(*h)).collect()
}

/// Whether JOIN would make the picked arcs a circle. AutoCAD 2024 asks first ("Combined arc
/// segments form a circle.  Convert to circle? [Yes/No] <Yes>:"), and No leaves every arc as it
/// is (measured).
pub(crate) fn join_makes_circle(s: &Session, hs: &[Handle]) -> bool {
    let Ok(d) = s.doc() else { return false };
    // Elliptical arcs and splines join another way, and a locked object stops the join.
    let other_way = |h: &Handle| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Ellipse(_) | EntityKind::Spline(_)));
    if hs.iter().any(|h| other_way(h) || crate::cmd::curves::is_locked(s, *h)) {
        return false;
    }
    let picks = joinable_picks(d, hs);
    let arcs: Vec<&cadcraft_doc::Arc> = picks
        .iter()
        .filter_map(|h| match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::Arc(a)) => Some(a),
            _ => None,
        })
        .collect();
    let tol = 1e-6;
    match &arcs[..] {
        [g, rest @ ..] if !rest.is_empty() && arcs.len() == picks.len() => arc_reach(g, rest, tol).is_some_and(|far| far > cadcraft_geom::TAU - tol),
        _ => false,
    }
}

/// How far round the source arc `g`'s circle, counterclockwise from its start, it and the
/// `others` reach; `None` when one of them isn't on that circle (less than `tol` off its centre
/// and radius). An arc that starts even a hair before the source's start reaches on round past
/// it: AutoCAD 2024 makes a circle of 7°-100° and an arc stored from 7° less 5e-16 rad, and
/// leaves 7°-100° with one from exactly 7° or 5e-7 rad after it.
fn arc_reach(g: &cadcraft_doc::Arc, others: &[&cadcraft_doc::Arc], tol: f64) -> Option<f64> {
    let same_circle =
        |o: &cadcraft_doc::Arc| o.center.xy().dist(g.center.xy()) < tol && (o.radius - g.radius).abs() < tol && (o.center.z - g.center.z).abs() < tol;
    let reach = |o: &cadcraft_doc::Arc| norm_angle(o.start - g.start) + cadcraft_geom::ccw_sweep(o.start, o.end);
    others.iter().all(|o| same_circle(o)).then(|| others.iter().map(|o| reach(o)).fold(reach(g), f64::max))
}

/// The chain JOIN builds from the first object: each next object with an end on one of the
/// chain's ends joins whole, turned round where needed, as AutoCAD 2024 joins them, until a join
/// closes the chain; one that the chain meets only at an inner vertex of a polyline stays out.
/// Ends are compared at the objects' own end points, not ones worked out again from arcs.
fn chain_units(mut units: Vec<Vec<Piece>>, tol: f64) -> Vec<Piece> {
    let ends = |u: &[Piece]| u.first().zip(u.last()).map(|(a, b)| (a.ends.0, b.ends.1));
    if units.is_empty() {
        return Vec::new();
    }
    // A source that already ends where it starts still takes more, unless it has no length at all
    // (AutoCAD 2024, measured with a polyline that ends where it starts and with a line of no
    // length).
    let mut chain: std::collections::VecDeque<Piece> = units.remove(0).into();
    if chain.iter().all(|p| p.ends.0.near(p.ends.1, tol)) {
        return chain.into();
    }
    while let (Some(start), Some(end)) = (chain.front().map(|p| p.ends.0), chain.back().map(|p| p.ends.1)) {
        let meets = |u: &Vec<Piece>| ends(u).is_some_and(|(a, b)| a.near(end, tol) || b.near(end, tol) || a.near(start, tol) || b.near(start, tol));
        let Some(i) = units.iter().position(meets) else { break };
        let unit = units.remove(i);
        let Some((a, b)) = ends(&unit) else { break };
        // Where an object that meets the chain at more than one place goes, as AutoCAD 2024 puts
        // it (measured): a polyline by its first vertex if it can, and before the chain rather
        // than after it; a line or an arc turned round if it can, its end on the chain's end
        // before its start on the chain's start.
        let polyline = unit.first().is_some_and(|p| p.widths.is_some());
        let (before, turned) = if polyline {
            if a.near(start, tol) {
                (true, true)
            } else if a.near(end, tol) {
                (false, false)
            } else {
                (b.near(start, tol), !b.near(start, tol))
            }
        } else if b.near(end, tol) {
            (false, true)
        } else if a.near(start, tol) {
            (true, true)
        } else {
            (!a.near(end, tol), false)
        };
        match (before, turned) {
            (false, false) => chain.extend(unit),
            (false, true) => chain.extend(unit.into_iter().rev().map(Piece::flipped)),
            (true, false) => unit.into_iter().rev().for_each(|p| chain.push_front(p)),
            (true, true) => unit.into_iter().for_each(|p| chain.push_front(p.flipped())),
        }
        // A chain that a join closes takes nothing more.
        if chain.front().zip(chain.back()).is_some_and(|(f, l)| f.ends.0.near(l.ends.1, tol)) {
            break;
        }
    }
    chain.into()
}

/// A segment in the chain JOIN builds: the object it came from, its end points as the object has
/// them (turned round when it runs backwards in the chain), its bulge and, for a polyline's own
/// segment, its widths.
#[derive(Clone, Copy)]
struct Piece {
    owner: Handle,
    ends: (Vec2, Vec2),
    bulge: f64,
    widths: Option<(f64, f64)>,
}

impl Piece {
    fn flipped(self) -> Self {
        Piece { ends: (self.ends.1, self.ends.0), bulge: -self.bulge, widths: self.widths.map(|(a, b)| (b, a)), ..self }
    }
}

/// The open polyline `sp` (handle `src`) with the chain's other pieces added before and after
/// it, as AutoCAD 2024 JOIN makes it (measured with `entget`): its own vertices stay exactly as
/// they are, and so do its elevation, linetype generation and, when only lines and arcs join,
/// its constant width; another polyline's segments keep their bulges and widths; a line or arc
/// takes the widths of the source's vertex at the end it joins (first or last); and when another
/// polyline joins, the end vertex's widths are cleared.
///
/// `None` when the source's segments aren't one run of the chain (they always are: the chain
/// starts from the source and takes its segments in order).
fn onto_polyline(sp: &cadcraft_doc::LwPolyline, src: Handle, links: &[Piece], closed: bool) -> Option<cadcraft_doc::LwPolyline> {
    let run: Vec<usize> = (0..links.len()).filter(|i| links[*i].owner == src).collect();
    let (&first, &last) = (run.first()?, run.last()?);
    if sp.closed || run.len() != last - first + 1 {
        return None;
    }
    let other_polyline = links.iter().any(|l| l.owner != src && l.widths.is_some());
    let mut own = sp.vertices.clone();
    if other_polyline && sp.const_width > 0.0 {
        // The constant width goes, so spell it out on the source's own segments.
        for v in &mut own {
            (v.start_width, v.end_width) = (sp.const_width, sp.const_width);
        }
    }
    let widths = |v: &PolyVertex| (v.start_width, v.end_width);
    let (head, tail) = (widths(own.first()?), widths(own.last()?));
    let vertex = |i: usize, fill: (f64, f64)| {
        let (start_width, end_width) = links[i].widths.unwrap_or(fill);
        PolyVertex { p: links[i].ends.0, bulge: links[i].bulge, start_width, end_width }
    };
    let mut vs: Vec<PolyVertex> = (0..first).map(|i| vertex(i, head)).collect();
    vs.extend(own);
    if last + 1 < links.len() {
        // The source's end vertex starts the first piece after it.
        let v = vertex(last + 1, tail);
        if let Some(end) = vs.last_mut() {
            (end.bulge, end.start_width, end.end_width) = (v.bulge, v.start_width, v.end_width);
        }
        vs.extend((last + 2..links.len()).map(|i| vertex(i, tail)));
        vs.push(PolyVertex { p: links.last()?.ends.1, start_width: tail.0, end_width: tail.1, ..Default::default() });
    }
    if other_polyline && let Some(end) = vs.last_mut() {
        (end.start_width, end.end_width) = (0.0, 0.0);
    }
    if closed {
        // The last vertex is the first one again.
        vs.pop();
    }
    Some(cadcraft_doc::LwPolyline { vertices: vs, closed, const_width: if other_polyline { 0.0 } else { sp.const_width }, ..sp.clone() })
}

/// JOIN onto the first object (the source) as AutoCAD 2024 does when the others all lie on its
/// line or its circle: gaps and overlaps are bridged and the source keeps its handle.
///
/// - Lines on the source's line make one line, from the source's start end to the farthest
///   point the way it runs (and back past its start if one reaches behind it).
/// - Arcs on the source's circle join counterclockwise from the source's start to the farthest
///   end; arcs that reach all the way round make a circle, a new object in their place.
///
/// `None` when the objects aren't all of one of those kinds.
fn join_on_source(s: &mut Session, hs: &[Handle]) -> Result<Option<Handle>> {
    let d = s.doc()?;
    let ents: Vec<std::sync::Arc<Entity>> = hs.iter().filter_map(|h| d.entity(*h).cloned()).collect();
    let [src, rest @ ..] = &ents[..] else { return Ok(None) };
    if rest.is_empty() || ents.len() != hs.len() {
        return Ok(None);
    }
    // AutoCAD 2024 takes lines less than 1e-6 off the source's line, and arcs less than 1e-6
    // off its centre and radius, at any size of coordinates (measured at 0, 1000 and 2.4e6), and
    // makes a circle of arcs that come round to less than 1e-6 radians from the source's start.
    let tol = 1e-6;
    let kind = match &src.kind {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let len = a.dist(b);
            let lines: Vec<&cadcraft_doc::Line> = rest
                .iter()
                .filter_map(|e| match &e.kind {
                    EntityKind::Line(o) => Some(o),
                    _ => None,
                })
                .collect();
            let level = |z: f64| (z - l.a.z).abs() < tol;
            if lines.len() != rest.len() || len <= 0.0 || !level(l.b.z) || !lines.iter().all(|o| level(o.a.z) && level(o.b.z)) {
                return Ok(None);
            }
            let u = (b - a) * (1.0 / len);
            let on_line = |p: Vec2| (p - a).cross(u).abs() < tol;
            if !lines.iter().all(|o| on_line(o.a.xy()) && on_line(o.b.xy())) {
                return Ok(None);
            }
            let ts = lines.iter().flat_map(|o| [o.a.xy(), o.b.xy()]).map(|p| (p - a).dot(u)).chain([0.0, len]);
            let (t0, t1) = ts.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
            let at = |t: f64| (a + u * t).to3(l.a.z);
            EntityKind::Line(cadcraft_doc::Line { a: at(t0), b: if t1 == len { l.b } else { at(t1) } })
        }
        EntityKind::Arc(g) => {
            let arcs: Vec<&cadcraft_doc::Arc> = rest
                .iter()
                .filter_map(|e| match &e.kind {
                    EntityKind::Arc(o) => Some(o),
                    _ => None,
                })
                .collect();
            let Some(far) = arc_reach(g, &arcs, tol).filter(|_| arcs.len() == rest.len()) else { return Ok(None) };
            if far > cadcraft_geom::TAU - tol {
                let circle = Entity {
                    handle: Handle(0),
                    common: src.common.clone(),
                    kind: EntityKind::Circle(cadcraft_doc::Circle { center: g.center, radius: g.radius }),
                };
                // In the source's space, whichever space is current.
                let space = d.space_of(src.handle).unwrap_or_else(|| s.space());
                let doc = s.doc_mut()?;
                for h in hs {
                    doc.remove_entity(*h);
                }
                let nh = doc.new_handle();
                if let Some(st) = doc.space_mut(&space) {
                    st.push(Entity { handle: nh, ..circle });
                }
                s.set_selection(Vec::new());
                return Ok(Some(nh));
            }
            let end = if far > cadcraft_geom::ccw_sweep(g.start, g.end) { norm_angle(g.start + far) } else { g.end };
            EntityKind::Arc(cadcraft_doc::Arc { end, ..g.clone() })
        }
        _ => return Ok(None),
    };
    let src_h = src.handle;
    let doc = s.doc_mut()?;
    for h in hs.iter().filter(|h| **h != src_h) {
        doc.remove_entity(*h);
    }
    doc.modify_entity(src_h, |e| e.kind = kind)?;
    s.set_selection(Vec::new());
    Ok(Some(src_h))
}

pub(super) fn run_join(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if p.get("close").and_then(Value::as_bool) == Some(true) {
        let h = crate::cmd::modify2::join_close(s, &hs)?;
        return Ok(json!({ "handle": h.hex() }));
    }
    let r = join(s, &hs)?;
    Ok(json!({ "handle": r.map(|h| h.hex()) }))
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
