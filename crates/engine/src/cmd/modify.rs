//! Modify commands: ERASE, MOVE, COPY, ROTATE, SCALE, MIRROR, OFFSET, TRIM, EXTEND, FILLET,
//! CHAMFER, EXPLODE, STRETCH, ARRAY, DRAWORDER, BREAK, JOIN.

use cadcraft_doc::{Entity, EntityKind, Handle, Prim};
use cadcraft_geom::{
    Arc, Circle, Line, Mat3, PolyVertex, Polyline, Segment, TAU, Vec2, angle_in_sweep, ccw_sweep, intersect_ext, line_line_infinite, norm_angle,
};
use serde_json::{Value, json};

use super::helpers::*;
use super::machines::{SelOutcome, SelectPhase, SelectRun, number};
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("erase", "Erase", run_erase)
            .menu(&["Modify", "Erase"])
            .alias(&["e", "delete"])
            .params("{handles?: [hex]} (default: selection)")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Erase)))),
        CommandSpec::new("move", "Move", run_move)
            .menu(&["Modify", "Move"])
            .alias(&["m"])
            .params("{handles?, from?: [x,y], to?: [x,y] | delta: [dx,dy]}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Move)))),
        CommandSpec::new("copy", "Copy", run_copy)
            .menu(&["Modify", "Copy"])
            .alias(&["co", "cp"])
            .params("{handles?, delta: [dx,dy] | from,to, count?: n}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Copy)))),
        CommandSpec::new("rotate", "Rotate", run_rotate)
            .menu(&["Modify", "Rotate"])
            .alias(&["ro"])
            .params("{handles?, base: [x,y], angle (degrees), copy?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Rotate)))),
        CommandSpec::new("scale", "Scale", run_scale)
            .menu(&["Modify", "Scale"])
            .alias(&["sc"])
            .params("{handles?, base: [x,y], factor, copy?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Scale)))),
        CommandSpec::new("mirror", "Mirror", run_mirror)
            .menu(&["Modify", "Mirror"])
            .alias(&["mi"])
            .params("{handles?, p1, p2, erase?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Mirror)))),
        CommandSpec::new("stretch", "Stretch", run_stretch)
            .menu(&["Modify", "Stretch"])
            .alias(&["s"])
            .params("{window: [[x,y],[x,y]], delta: [dx,dy]}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Stretch)))),
        CommandSpec::new("offset", "Offset", run_offset)
            .menu(&["Modify", "Offset"])
            .alias(&["o"])
            .params("{handle, distance, side: [x,y]}")
            .interactive(|s| Ok(Box::new(OffsetM::new(s)))),
        CommandSpec::new("trim", "Trim", run_trim)
            .menu(&["Modify", "Trim"])
            .alias(&["tr"])
            .params("{handle, pick: [x,y], edges?: [hex]}")
            .interactive(|_| Ok(Box::new(TrimM { extend: false }))),
        CommandSpec::new("extend", "Extend", run_extend)
            .menu(&["Modify", "Extend"])
            .alias(&["ex"])
            .params("{handle, pick: [x,y], edges?: [hex]}")
            .interactive(|_| Ok(Box::new(TrimM { extend: true }))),
        CommandSpec::new("fillet", "Fillet", run_fillet)
            .menu(&["Modify", "Fillet"])
            .alias(&["f"])
            .params("{h1, p1, h2, p2, radius?} (lines, arcs, circles) | {handle, polyline: true, radius?}")
            .interactive(|s| Ok(Box::new(FilletM::new(s, false)))),
        CommandSpec::new("chamfer", "Chamfer", run_chamfer)
            .menu(&["Modify", "Chamfer"])
            .alias(&["cha"])
            .params("{h1, p1, h2, p2, d1?, d2?}")
            .interactive(|s| Ok(Box::new(FilletM::new(s, true)))),
        CommandSpec::new("explode", "Explode", run_explode)
            .menu(&["Modify", "Explode"])
            .alias(&["x"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Explode)))),
        CommandSpec::new("arrayrect", "Rectangular Array", run_arrayrect)
            .menu(&["Modify", "Array", "Rectangular Array"])
            .params("{handles?, rows, cols, rowSpacing, colSpacing}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::ArrayRect)))),
        CommandSpec::new("arraypolar", "Polar Array", run_arraypolar)
            .menu(&["Modify", "Array", "Polar Array"])
            .params("{handles?, center, count, angle? (degrees, default 360), rotate?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::ArrayPolar)))),
        CommandSpec::new("draworder.front", "Bring to Front", run_front)
            .menu(&["Tools", "Draw Order", "Bring to Front"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("draworder.front", "DRAWORDER")))),
        CommandSpec::new("draworder.back", "Send to Back", run_back)
            .menu(&["Tools", "Draw Order", "Send to Back"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("draworder.back", "DRAWORDER")))),
        CommandSpec::new("break", "Break", run_break)
            .menu(&["Modify", "Break"])
            .alias(&["br"])
            .params("{handle, p1, p2}")
            .interactive(|_| Ok(Box::new(BreakM::default()))),
        CommandSpec::new("breakatpoint", "Break At Point", run_breakat).menu(&["Modify", "Break At Point"]).params("{handle, at}"),
        CommandSpec::new("join", "Join", run_join)
            .menu(&["Modify", "Join"])
            .alias(&["j"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Join)))),
        CommandSpec::new("overkill", "Delete Duplicate Objects", run_overkill).menu(&["Modify", "Delete Duplicate Objects"]).params("{handles?}"),
    ]
}

// ---------------- core operations ----------------

pub(crate) fn transform_entities(s: &mut Session, hs: &[Handle], m: &Mat3, copy: bool) -> Result<Vec<Handle>> {
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for h in hs {
        let Some(sp) = d.space_of(*h) else { continue };
        if copy {
            let Some(e) = d.entity(*h).map(|e| (**e).clone()) else { continue };
            let nh = d.new_handle();
            let mut ne = Entity { handle: nh, ..e };
            ne.kind.transform(m);
            if let Some(st) = d.space_mut(&sp) {
                st.push(ne);
                out.push(nh);
            }
        } else {
            let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
            if locked {
                continue;
            }
            d.modify_entity(*h, |e| e.kind.transform(m))?;
            out.push(*h);
        }
    }
    Ok(out)
}

fn erase(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
        if !locked && d.remove_entity(*h).is_some() {
            n += 1;
        }
    }
    s.set_selection(Vec::new());
    Ok(n)
}

fn run_erase(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = erase(s, &hs)?;
    Ok(json!({ "erased": n }))
}

fn delta_of(cmd: &str, p: &Value) -> Result<Vec2> {
    if let Some(d) = point_param(p, "delta") {
        return Ok(d);
    }
    let a = point_req(cmd, p, "from")?;
    let b = point_req(cmd, p, "to")?;
    Ok(b - a)
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = delta_of("move", p)?;
    let moved = transform_entities(s, &hs, &Mat3::translate(d), false)?;
    Ok(json!({ "moved": moved.len() }))
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = delta_of("copy", p)?;
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 10_000);
    let mut out = Vec::new();
    for k in 1..=n {
        out.extend(transform_entities(s, &hs, &Mat3::translate(d * k as f64), true)?.into_iter().map(|h| h.hex()));
    }
    Ok(json!({ "handles": out }))
}

fn run_rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_req("rotate", p, "base")?;
    let a = f64_req("rotate", p, "angle")?.to_radians();
    let r = transform_entities(s, &hs, &Mat3::rotate_about(base, a), bool_or(p, "copy", false))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_scale(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_req("scale", p, "base")?;
    let f = f64_req("scale", p, "factor")?;
    if f <= 0.0 {
        return Err(bad("scale", "factor must be positive"));
    }
    let r = transform_entities(s, &hs, &Mat3::scale_about(base, f), bool_or(p, "copy", false))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let a = point_req("mirror", p, "p1")?;
    let b = point_req("mirror", p, "p2")?;
    if a.near(b, 1e-12) {
        return Err(bad("mirror", "mirror line points coincide"));
    }
    let erase_src = bool_or(p, "erase", false);
    let r = transform_entities(s, &hs, &Mat3::mirror(a, b), !erase_src)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// Move the vertices of `e` that lie inside `bx` by `d` (STRETCH).
fn stretch_entity(e: &mut Entity, bx: &cadcraft_geom::Bounds2, d: Vec2) {
    let mv = |p: &mut cadcraft_geom::Vec3| {
        if bx.contains(p.xy()) {
            p.x += d.x;
            p.y += d.y;
        }
    };
    match &mut e.kind {
        EntityKind::Line(l) => {
            mv(&mut l.a);
            mv(&mut l.b);
        }
        EntityKind::LwPolyline(pl) => {
            for v in &mut pl.vertices {
                if bx.contains(v.p) {
                    v.p += d;
                }
            }
        }
        EntityKind::Arc(a) => {
            // Arcs keep their shape: move when the centre is inside.
            if bx.contains(a.center.xy()) {
                a.center.x += d.x;
                a.center.y += d.y;
            }
        }
        EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(mv),
        EntityKind::Dimension(dm) => {
            for p in [&mut dm.p13, &mut dm.p14, &mut dm.defpt, &mut dm.text_mid] {
                mv(p);
            }
            dm.block = None;
        }
        k => {
            let g = k.grips();
            if g.first().is_some_and(|p| bx.contains(*p)) {
                k.transform(&Mat3::translate(d));
            }
        }
    }
}

fn run_stretch(s: &mut Session, p: &Value) -> Result<Value> {
    let w = points_param(p, "window").ok_or_else(|| bad("stretch", "`window` [[x,y],[x,y]] is required"))?;
    let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return Err(bad("stretch", "window needs 2 corners")) };
    let bx = cadcraft_geom::Bounds2::new(*a, *b);
    let d = delta_of("stretch", p)?;
    let space = s.space();
    let mut hs = crate::select::select_window(s.doc()?, &space, bx, true);
    hs.retain(|h| !super::curves::is_locked(s, *h));
    let doc = s.doc_mut()?;
    for h in &hs {
        doc.modify_entity(*h, |e| stretch_entity(e, &bx, d))?;
    }
    Ok(json!({ "stretched": hs.len() }))
}

/// Offset an entity by `dist` towards `side`. Returns the new geometry.
pub(crate) fn offset_kind(k: &EntityKind, dist: f64, side: Vec2) -> Option<EntityKind> {
    match k {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let sgn = if ln.side(side) >= 0.0 { 1.0 } else { -1.0 };
            let o = ln.offset(dist * sgn);
            Some(line(o.a, o.b))
        }
        EntityKind::Circle(c) => {
            let inside = side.dist(c.center.xy()) < c.radius;
            let r = if inside { c.radius - dist } else { c.radius + dist };
            (r > 1e-12).then(|| circle(c.center.xy(), r))
        }
        EntityKind::Arc(a) => {
            let inside = side.dist(a.center.xy()) < a.radius;
            let r = if inside { a.radius - dist } else { a.radius + dist };
            (r > 1e-12).then(|| arc(&Arc::new(a.center.xy(), r, a.start, a.end)))
        }
        EntityKind::Ellipse(e) => {
            // Approximate with a polyline offset of the tessellation.
            let ge = cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end };
            let inside = {
                let mut pts = Vec::new();
                ge.tessellate(e.major.xy().len() * 1e-3, &mut pts);
                cadcraft_geom::point_in_polygon(&pts, side)
            };
            let mut pts = Vec::new();
            ge.tessellate(e.major.xy().len() * 1e-3, &mut pts);
            let off = offset_points(&pts, if inside { -dist } else { dist }, ge.is_full());
            Some(lwpoly(off.into_iter().map(PolyVertex::new).collect(), ge.is_full()))
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            // Side: compare with the nearest segment.
            let segs = pl.segments();
            let nearest = segs.iter().min_by(|a, b| a.closest(side).dist(side).total_cmp(&b.closest(side).dist(side)))?;
            let c = nearest.closest(side);
            let t = nearest.tangent(0.5);
            let left = t.cross(side - c) > 0.0;
            let d = if left { dist } else { -dist };
            offset_polyline(&pl, d).map(|vs| {
                let mut k = lwpoly(vs, p.closed);
                if let EntityKind::LwPolyline(n) = &mut k {
                    n.const_width = p.const_width;
                }
                k
            })
        }
        EntityKind::Spline(sp) => {
            let pts = sp.tessellate(1e-3);
            let n = pts.len();
            let mid = pts.get(n / 2).copied()?;
            let next = pts.get((n / 2 + 1).min(n.saturating_sub(1))).copied()?;
            let left = (next - mid).cross(side - mid) > 0.0;
            let off = offset_points(&pts, if left { dist } else { -dist }, sp.closed);
            Some(EntityKind::Spline(cadcraft_geom::Spline::from_fit_points(&decimate(&off, 40))))
        }
        EntityKind::XLine(r) | EntityKind::Ray(r) => {
            let ln = Line::new(r.base.xy(), r.base.xy() + r.dir.xy());
            let sgn = if ln.side(side) >= 0.0 { 1.0 } else { -1.0 };
            let o = ln.offset(dist * sgn);
            let rl = cadcraft_doc::RayLine { base: v3(o.a), dir: r.dir };
            Some(if matches!(k, EntityKind::XLine(_)) { EntityKind::XLine(rl) } else { EntityKind::Ray(rl) })
        }
        _ => None,
    }
}

fn decimate(pts: &[Vec2], max: usize) -> Vec<Vec2> {
    if pts.len() <= max {
        return pts.to_vec();
    }
    let step = pts.len() as f64 / max as f64;
    let mut v: Vec<Vec2> = (0..max).filter_map(|i| pts.get((i as f64 * step) as usize).copied()).collect();
    if let Some(l) = pts.last() {
        v.push(*l);
    }
    v
}

fn offset_points(pts: &[Vec2], d: f64, closed: bool) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or_default();
        let prev = if i > 0 {
            pts.get(i - 1)
        } else if closed {
            pts.get(n.saturating_sub(2))
        } else {
            None
        };
        let next = if i + 1 < n {
            pts.get(i + 1)
        } else if closed {
            pts.get(1)
        } else {
            None
        };
        let t = match (prev, next) {
            (Some(a), Some(b)) => (*b - *a).normalized(),
            (None, Some(b)) => (*b - p).normalized(),
            (Some(a), None) => (p - *a).normalized(),
            _ => Vec2::X,
        };
        out.push(p + t.perp() * d);
    }
    out
}

/// Offset a polyline: offset each segment, then join neighbours at their intersections.
pub(crate) fn offset_polyline(pl: &Polyline, d: f64) -> Option<Vec<PolyVertex>> {
    let segs: Vec<Segment> = pl.segments().iter().filter_map(|s| s.offset(d)).collect();
    if segs.is_empty() {
        return None;
    }
    let n = segs.len();
    let mut verts: Vec<PolyVertex> = Vec::new();
    for i in 0..n {
        let s = segs.get(i)?;
        let start = if i == 0 && !pl.closed {
            s.start()
        } else {
            let prev = segs.get((i + n - 1) % n)?;
            join_point(prev, s).unwrap_or(s.start())
        };
        let bulge = match s {
            Segment::Line(_) => 0.0,
            Segment::Arc { arc, ccw } => {
                let sw = arc.sweep();
                cadcraft_geom::arc_to_bulge(if *ccw { sw } else { -sw })
            }
        };
        verts.push(PolyVertex { p: start, bulge, start_width: 0.0, end_width: 0.0 });
    }
    if !pl.closed {
        verts.push(PolyVertex::new(segs.last()?.end()));
    }
    // Recompute arc bulges where endpoints moved (approximation: keep sweep sign).
    Some(verts)
}

fn join_point(a: &Segment, b: &Segment) -> Option<Vec2> {
    if a.end().near(b.start(), 1e-9) {
        return Some(b.start());
    }
    let hits = intersect_ext(a, b, true);
    hits.into_iter().min_by(|x, y| x.dist(a.end()).total_cmp(&y.dist(a.end())))
}

fn run_offset(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("offset", "`handle` is required"))?;
    let dist = f64_req("offset", p, "distance")?;
    let side = point_req("offset", p, "side")?;
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| bad("offset", "no such object"))?;
    let k = offset_kind(&e.kind, dist.abs(), side).ok_or_else(|| bad("offset", "cannot offset that object"))?;
    let space = s.space();
    let d = s.doc_mut()?;
    let nh = d.new_handle();
    if let Some(st) = d.space_mut(&space) {
        st.push(Entity { handle: nh, common: e.common.clone(), kind: k });
    }
    Ok(json!({ "handle": nh.hex() }))
}

// ---------------- trim / extend ----------------

/// All primitives of the cutting edges (excluding `exclude`).
fn edge_segments(s: &Session, edges: Option<&[Handle]>, exclude: Handle) -> Result<Vec<Segment>> {
    let d = s.doc()?;
    let space = s.space();
    let store = d.space(&space).ok_or(EngineError::NoDocument)?;
    let mut out = Vec::new();
    for e in store.iter() {
        if e.handle == exclude || !d.is_visible(e) {
            continue;
        }
        if let Some(es) = edges
            && !es.contains(&e.handle)
        {
            continue;
        }
        for p in e.kind.prims() {
            out.extend(prim_to_segments(&p));
        }
    }
    Ok(out)
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

/// Trim `h` at the piece containing `pick`. Returns the handles that replace it.
pub(crate) fn trim(s: &mut Session, h: Handle, pick: Vec2, edges: Option<&[Handle]>) -> Result<Vec<Handle>> {
    if super::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let cut = edge_segments(s, edges, h)?;
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
        EntityKind::LwPolyline(p) => trim_polyline(p, &cut, pick)?,
        k => super::modify2::trim_other(k, &cut, pick)?,
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

/// Sub-polyline between length parameters `a` and `b` (open polylines).
fn sub_polyline(segs: &[Segment], a: f64, b: f64) -> Vec<PolyVertex> {
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

fn trim_polyline(p: &cadcraft_doc::LwPolyline, cut: &[Segment], pick: Vec2) -> Result<Vec<EntityKind>> {
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
        let (lo, hi) = (
            if lo.is_finite() { lo } else { ts.last().copied().unwrap_or(0.0) - total },
            if hi.is_finite() { hi } else { ts.first().copied().unwrap_or(total) + total },
        );
        let doubled: Vec<Segment> = segs.iter().chain(segs.iter()).copied().collect();
        let (a, b) = if hi > total { (hi - total, lo + total) } else { (hi, lo + total) };
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

fn param_on(s: &Segment, p: Vec2) -> f64 {
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
pub(crate) fn extend(s: &mut Session, h: Handle, pick: Vec2, edges: Option<&[Handle]>) -> Result<()> {
    if super::curves::is_locked(s, h) {
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
        k => super::modify2::extend_other(k, &cut, pick)?,
    };
    s.doc_mut()?.modify_entity(h, |e| e.kind = new_kind)?;
    Ok(())
}

fn edges_param(p: &Value) -> Option<Vec<Handle>> {
    p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect())
}

fn run_trim(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("trim", "`handle` is required"))?;
    let pick = point_req("trim", p, "pick")?;
    let edges = edges_param(p);
    let r = trim(s, h, pick, edges.as_deref())?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_extend(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("extend", "`handle` is required"))?;
    let pick = point_req("extend", p, "pick")?;
    let edges = edges_param(p);
    extend(s, h, pick, edges.as_deref())?;
    ok()
}

// ---------------- fillet / chamfer ----------------

fn as_line(e: &Entity) -> Option<Line> {
    match &e.kind {
        EntityKind::Line(l) => Some(Line::new(l.a.xy(), l.b.xy())),
        _ => None,
    }
}

/// Fillet (or chamfer) two lines. `p1`/`p2` mark the kept side of each line.
pub(crate) fn fillet_lines(
    s: &mut Session,
    h1: Handle,
    p1: Vec2,
    h2: Handle,
    p2: Vec2,
    radius: f64,
    chamfer: Option<(f64, f64)>,
) -> Result<Option<Handle>> {
    if super::curves::is_locked(s, h1) || super::curves::is_locked(s, h2) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let d = s.doc()?;
    let e1 = d.entity(h1).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let e2 = d.entity(h2).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let (Some(l1), Some(l2)) = (as_line(&e1), as_line(&e2)) else {
        if chamfer.is_none() {
            return super::modify2::fillet_curves(s, h1, p1, h2, p2, radius);
        }
        return Err(EngineError::Other("Chamfer currently works on lines.".into()));
    };
    let (x, _, _) = line_line_infinite(l1.a, l1.b, l2.a, l2.b).ok_or_else(|| EngineError::Other("Lines are parallel.".into()))?;
    // Keep the far end on the picked side of the corner.
    let keep = |l: &Line, p: Vec2| -> Vec2 {
        let ta = (l.a - x).dot(p - x);
        let tb = (l.b - x).dot(p - x);
        if ta >= tb { l.a } else { l.b }
    };
    let k1 = keep(&l1, p1);
    let k2 = keep(&l2, p2);
    let u1 = (k1 - x).normalized();
    let u2 = (k2 - x).normalized();
    let mut new_arc = None;
    let (t1, t2) = if let Some((d1, d2)) = chamfer {
        (x + u1 * d1, x + u2 * d2)
    } else if radius <= 1e-12 {
        (x, x)
    } else {
        let half = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if half.sin().abs() < 1e-12 {
            return Err(EngineError::Other("Lines are parallel.".into()));
        }
        let dist = radius / half.tan();
        let t1 = x + u1 * dist;
        let t2 = x + u2 * dist;
        let bis = (u1 + u2).normalized();
        let c = x + bis * (radius / half.sin());
        let a1 = c.angle_to(t1);
        let a2 = c.angle_to(t2);
        let ar = if ccw_sweep(a1, a2) <= std::f64::consts::PI { Arc::new(c, radius, a1, a2) } else { Arc::new(c, radius, a2, a1) };
        new_arc = Some(ar);
        (t1, t2)
    };
    let common = e1.common.clone();
    let doc = s.doc_mut()?;
    doc.modify_entity(h1, |e| e.kind = line(k1, t1))?;
    doc.modify_entity(h2, |e| e.kind = line(k2, t2))?;
    let space = s.space();
    let doc = s.doc_mut()?;
    let extra = match (chamfer, new_arc) {
        (Some(_), _) if !t1.near(t2, 1e-12) => Some(line(t1, t2)),
        (None, Some(a)) => Some(arc(&a)),
        _ => None,
    };
    if let Some(k) = extra {
        let nh = doc.new_handle();
        if let Some(st) = doc.space_mut(&space) {
            st.push(Entity { handle: nh, common, kind: k });
        }
        return Ok(Some(nh));
    }
    Ok(None)
}

fn h_param(p: &Value, k: &str) -> Option<Handle> {
    p.get(k).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

fn run_fillet(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "polyline", false) {
        let h = h_param(p, "handle").or_else(|| h_param(p, "h1")).ok_or_else(|| bad("fillet", "`handle` (polyline) is required"))?;
        let r = f64_or(p, "radius", s.doc()?.header.f64("FILLETRAD", 0.0));
        let n = super::modify2::fillet_polyline(s, h, r)?;
        return Ok(json!({ "filleted": n }));
    }
    let h1 = h_param(p, "h1").ok_or_else(|| bad("fillet", "`h1` is required"))?;
    let h2 = h_param(p, "h2").ok_or_else(|| bad("fillet", "`h2` is required"))?;
    let r = f64_or(p, "radius", s.doc()?.header.f64("FILLETRAD", 0.0));
    let p1 = point_param(p, "p1").unwrap_or_default();
    let p2 = point_param(p, "p2").unwrap_or_default();
    let a = fillet_lines(s, h1, p1, h2, p2, r, None)?;
    Ok(json!({ "arc": a.map(|h| h.hex()) }))
}

fn run_chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let h1 = h_param(p, "h1").ok_or_else(|| bad("chamfer", "`h1` is required"))?;
    let h2 = h_param(p, "h2").ok_or_else(|| bad("chamfer", "`h2` is required"))?;
    let d1 = f64_or(p, "d1", s.doc()?.header.f64("CHAMFERA", 0.0));
    let d2 = f64_or(p, "d2", d1);
    let p1 = point_param(p, "p1").unwrap_or_default();
    let p2 = point_param(p, "p2").unwrap_or_default();
    let a = fillet_lines(s, h1, p1, h2, p2, 0.0, Some((d1, d2)))?;
    Ok(json!({ "line": a.map(|h| h.hex()) }))
}

// ---------------- explode / array / order / break / join ----------------

pub(crate) fn explode_kind(d: &cadcraft_doc::Drawing, e: &Entity) -> Option<Vec<Entity>> {
    match &e.kind {
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            Some(
                pl.segments()
                    .into_iter()
                    .map(|s| {
                        let k = match s {
                            Segment::Line(l) => line(l.a, l.b),
                            Segment::Arc { arc: a, .. } => arc(&a),
                        };
                        Entity { handle: Handle(0), common: e.common.clone(), kind: k }
                    })
                    .collect(),
            )
        }
        EntityKind::Insert(ins) => {
            let blk = d.block(&ins.block)?;
            let m = ins.transform(blk.base.xy());
            Some(
                blk.entities
                    .iter()
                    .map(|be| {
                        let mut ne = (**be).clone();
                        ne.kind.transform(&m);
                        if ne.common.layer == "0" {
                            ne.common.layer = e.common.layer.clone();
                        }
                        if ne.common.color == cadcraft_color::Color::ByBlock {
                            ne.common.color = e.common.color;
                        }
                        ne
                    })
                    .collect(),
            )
        }
        EntityKind::Polyline3d(p) => Some(
            p.points
                .windows(2)
                .filter_map(|w| {
                    let kind = EntityKind::Line(cadcraft_doc::Line { a: *w.first()?, b: *w.get(1)? });
                    Some(Entity { handle: Handle(0), common: e.common.clone(), kind })
                })
                .collect(),
        ),
        EntityKind::MText(m) => Some(
            cadcraft_fonts::plain_mtext(&m.contents)
                .split('\n')
                .enumerate()
                .map(|(i, l)| {
                    let at = m.insert.xy() + Vec2::from_angle(m.rotation - std::f64::consts::FRAC_PI_2) * (m.height * (1.0 + 5.0 / 3.0 * i as f64));
                    Entity {
                        handle: Handle(0),
                        common: e.common.clone(),
                        kind: EntityKind::Text(cadcraft_doc::Text {
                            insert: v3(at),
                            align_pt: None,
                            height: m.height,
                            value: l.into(),
                            rotation: m.rotation,
                            width_factor: 1.0,
                            oblique: 0.0,
                            style: m.style.clone(),
                            halign: Default::default(),
                            valign: Default::default(),
                        }),
                    }
                })
                .collect(),
        ),
        EntityKind::Dimension(dm) => {
            let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
            let g = cadcraft_render::dimension_geometry(dm, &style, d.header.f64("DIMSCALE", 1.0));
            let mut v: Vec<Entity> = g
                .lines
                .iter()
                .filter_map(|l| {
                    if l.len() == 2 {
                        Some(Entity { handle: Handle(0), common: e.common.clone(), kind: line(*l.first()?, *l.get(1)?) })
                    } else {
                        Some(Entity {
                            handle: Handle(0),
                            common: e.common.clone(),
                            kind: lwpoly(l.iter().map(|p| PolyVertex::new(*p)).collect(), false),
                        })
                    }
                })
                .collect();
            for t in &g.fills {
                if let [a, b, c] = t.as_slice() {
                    v.push(Entity {
                        handle: Handle(0),
                        common: e.common.clone(),
                        kind: EntityKind::Solid(cadcraft_doc::Solid { corners: [v3(*a), v3(*b), v3(*c), v3(*c)] }),
                    });
                }
            }
            let th = style.text_height * style.scale.max(1e-9);
            v.push(Entity {
                handle: Handle(0),
                common: e.common.clone(),
                kind: EntityKind::MText(cadcraft_doc::MText {
                    insert: v3(g.text_pos),
                    height: th,
                    width: 0.0,
                    attach: 5,
                    rotation: 0.0,
                    style: style.text_style.clone(),
                    contents: g.value.clone(),
                    line_spacing: 1.0,
                }),
            });
            Some(v)
        }
        _ => None,
    }
}

fn explode(s: &mut Session, hs: &[Handle]) -> Result<Vec<Handle>> {
    let space = s.space();
    let mut out = Vec::new();
    for h in hs {
        if super::curves::is_locked(s, *h) {
            continue;
        }
        let Some(e) = s.doc()?.entity(*h).map(|e| (**e).clone()) else { continue };
        let Some(parts) = explode_kind(s.doc()?, &e) else { continue };
        let d = s.doc_mut()?;
        d.remove_entity(*h);
        for mut p in parts {
            p.handle = d.new_handle();
            out.push(p.handle);
            if let Some(st) = d.space_mut(&space) {
                st.push(p);
            }
        }
    }
    s.set_selection(Vec::new());
    Ok(out)
}

fn run_explode(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let r = explode(s, &hs)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_arrayrect(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(3).clamp(1, 1000);
    let cols = p.get("cols").and_then(Value::as_u64).unwrap_or(4).clamp(1, 1000);
    let rs = f64_or(p, "rowSpacing", 1.0);
    let cs = f64_or(p, "colSpacing", 1.0);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if r == 0 && c == 0 {
                continue;
            }
            out.extend(transform_entities(s, &hs, &Mat3::translate(Vec2::new(cs * c as f64, rs * r as f64)), true)?);
        }
    }
    Ok(json!({ "created": out.len() }))
}

fn run_arraypolar(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let c = point_req("arraypolar", p, "center")?;
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(6).clamp(1, 10_000);
    let total = f64_or(p, "angle", 360.0).to_radians();
    let rotate = bool_or(p, "rotate", true);
    let step = if (total - TAU).abs() < 1e-9 { total / n as f64 } else { total / (n.saturating_sub(1).max(1)) as f64 };
    let mut out = Vec::new();
    for k in 1..n {
        let a = step * k as f64;
        let m = if rotate {
            Mat3::rotate_about(c, a)
        } else {
            // Translate only: move the reference point around the circle.
            let d = s.doc()?;
            let r = hs.first().and_then(|h| d.entity(*h)).map(|e| cadcraft_doc::entity_bounds(d, e, 0).center()).unwrap_or(c);
            Mat3::translate(r.rotate_about(c, a) - r)
        };
        out.extend(transform_entities(s, &hs, &m, true)?);
    }
    Ok(json!({ "created": out.len() }))
}

fn run_front(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let space = s.space();
    let d = s.doc_mut()?;
    if let Some(st) = d.space_mut(&space) {
        for h in hs {
            st.bring_to_front(h);
        }
    }
    ok()
}

fn run_back(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let space = s.space();
    let d = s.doc_mut()?;
    if let Some(st) = d.space_mut(&space) {
        for h in hs.into_iter().rev() {
            st.send_to_back(h);
        }
    }
    ok()
}

/// Break `h` between two points (or at one point when p1 == p2).
pub(crate) fn break_entity(s: &mut Session, h: Handle, p1: Vec2, p2: Vec2) -> Result<Vec<Handle>> {
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let pieces: Vec<EntityKind> = match &e.kind {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let (mut t1, mut t2) = (ln.param_of(p1).clamp(0.0, 1.0), ln.param_of(p2).clamp(0.0, 1.0));
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            let mut v = Vec::new();
            if t1 > 1e-12 {
                v.push(line(ln.a, ln.at(t1)));
            }
            if t2 < 1.0 - 1e-12 {
                v.push(line(ln.at(t2), ln.b));
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
            vec![arc(&Arc::new(c.center.xy(), c.radius, a2, a1))]
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
                v.push(arc(&Arc::new(ga.center, ga.radius, ga.start, ga.start + t1)));
            }
            if t2 < ga.sweep() - 1e-12 {
                v.push(arc(&Arc::new(ga.center, ga.radius, ga.start + t2, ga.end)));
            }
            v
        }
        EntityKind::LwPolyline(p) if !p.closed => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: false };
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
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            let mut v = Vec::new();
            if t1 > 1e-12 {
                v.push(lwpoly(sub_polyline(&segs, 0.0, t1), false));
            }
            if t2 < total - 1e-12 {
                v.push(lwpoly(sub_polyline(&segs, t2, total), false));
            }
            v
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

fn run_break(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("break", "`handle` is required"))?;
    let p1 = point_req("break", p, "p1")?;
    let p2 = point_param(p, "p2").unwrap_or(p1);
    let r = break_entity(s, h, p1, p2)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_breakat(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("breakatpoint", "`handle` is required"))?;
    let at = point_req("breakatpoint", p, "at")?;
    let r = break_entity(s, h, at, at)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// Join lines/arcs/polylines that share endpoints into one polyline (or merge collinear lines).
pub(crate) fn join(s: &mut Session, hs: &[Handle]) -> Result<Option<Handle>> {
    let d = s.doc()?;
    let mut segs: Vec<Segment> = Vec::new();
    // Source entity of each segment in `segs`, so only entities that end up in the chain are removed.
    let mut owners: Vec<Handle> = Vec::new();
    let mut common = None;
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        common.get_or_insert_with(|| e.common.clone());
        match &e.kind {
            EntityKind::Line(l) => segs.push(Segment::Line(Line::new(l.a.xy(), l.b.xy()))),
            EntityKind::Arc(a) => segs.push(Segment::Arc { arc: Arc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true }),
            EntityKind::LwPolyline(p) if !p.closed => segs.extend(Polyline { vertices: p.vertices.clone(), closed: false }.segments()),
            _ => {}
        }
        owners.resize(segs.len(), *h);
    }
    if segs.len() < 2 {
        return Err(EngineError::Other("Select at least two objects to join.".into()));
    }
    // Chain greedily from the first segment.
    let mut chain = vec![segs.remove(0)];
    let mut used = vec![owners.remove(0)];
    let tol = 1e-6;
    loop {
        let start = chain.first().map(Segment::start).unwrap_or_default();
        let end = chain.last().map(Segment::end).unwrap_or_default();
        let Some(i) =
            segs.iter().position(|s| s.start().near(end, tol) || s.end().near(end, tol) || s.start().near(start, tol) || s.end().near(start, tol))
        else {
            break;
        };
        let s0 = segs.remove(i);
        used.push(owners.remove(i));
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
    if chain.len() < 2 {
        return Err(EngineError::Other("Objects are not connected.".into()));
    }
    let closed = chain.first().zip(chain.last()).is_some_and(|(a, b)| a.start().near(b.end(), tol));
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
    let space = s.space();
    let doc = s.doc_mut()?;
    for h in hs.iter().filter(|h| used.contains(*h)) {
        doc.remove_entity(*h);
    }
    let nh = doc.new_handle();
    if let Some(st) = doc.space_mut(&space) {
        st.push(Entity { handle: nh, common: common.unwrap_or_default(), kind: lwpoly(vs, closed) });
    }
    s.set_selection(Vec::new());
    Ok(Some(nh))
}

fn run_join(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let r = join(s, &hs)?;
    Ok(json!({ "handle": r.map(|h| h.hex()) }))
}

fn run_overkill(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = if p.get("handles").is_some() { targets(s, p)? } else { s.doc()?.space(&s.space()).map(|st| st.handles()).unwrap_or_default() };
    let d = s.doc()?;
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut dup = Vec::new();
    for h in hs {
        let Some(e) = d.entity(h) else { continue };
        let key = (serde_json::to_string(&e.kind).unwrap_or_default(), serde_json::to_string(&e.common).unwrap_or_default());
        if seen.contains(&key) {
            dup.push(h);
        } else {
            seen.push(key);
        }
    }
    let n = erase(s, &dup)?;
    Ok(json!({ "deleted": n, "message": format!("{n} duplicate(s) deleted") }))
}

// ---------------- interactive ----------------

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Erase,
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Stretch,
    Explode,
    ArrayRect,
    ArrayPolar,
    Join,
}

/// Select objects, then collect the points/values the operation needs.
struct SelectThen {
    op: Op,
    sel: SelectPhase,
    objs: Vec<Handle>,
    pts: Vec<Vec2>,
    reference: bool,
    /// ROTATE Reference: the reference angle (radians), once known. Its first point, and the
    /// first of the two new-angle points, are kept in `ref_from`.
    ref_angle: Option<f64>,
    /// ROTATE Reference: the new angle is given by two points (the `Points` option).
    ref_points: bool,
    /// SCALE Reference: first point of a reference length given by two points.
    ref_from: Option<Vec2>,
    /// SCALE Reference: the reference length, once known.
    ref_len: Option<f64>,
    copy_mode: bool,
    window: Option<cadcraft_geom::Bounds2>,
    ask_erase: bool,
}

impl SelectThen {
    fn new(op: Op) -> Self {
        SelectThen {
            op,
            sel: SelectPhase::default(),
            objs: Vec::new(),
            pts: Vec::new(),
            reference: false,
            ref_angle: None,
            ref_points: false,
            ref_from: None,
            ref_len: None,
            copy_mode: false,
            window: None,
            ask_erase: false,
        }
    }
    fn apply(&mut self, s: &mut Session) -> Result<Step> {
        // Immediate operations once selection is done.
        match self.op {
            Op::Erase => {
                let n = erase(s, &self.objs)?;
                let _ = n;
                Ok(Step::Done)
            }
            Op::Explode => {
                explode(s, &self.objs)?;
                Ok(Step::Done)
            }
            Op::Join => {
                match join(s, &self.objs) {
                    Ok(_) => {
                        // Objects outside the joined chain are kept, so count only the ones JOIN consumed.
                        let n = s.doc().map(|d| self.objs.iter().filter(|h| d.entity(**h).is_none()).count()).unwrap_or(0);
                        s.echo(format!("{n} objects joined into 1 polyline"))
                    }
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Done)
            }
            Op::ArrayRect => {
                let objs = self.objs.clone();
                let d = s.doc()?;
                let b = objs
                    .iter()
                    .filter_map(|h| d.entity(*h).map(|e| cadcraft_doc::entity_bounds(d, e, 0)))
                    .fold(cadcraft_geom::Bounds2::EMPTY, |a, b| a.union(&b));
                let cs = b.width() * 1.5 + 1e-9;
                let rs = b.height() * 1.5 + 1e-9;
                run_arrayrect(
                    s,
                    &json!({ "handles": objs.iter().map(|h| h.hex()).collect::<Vec<_>>(), "rows": 3, "cols": 4, "rowSpacing": rs, "colSpacing": cs }),
                )?;
                s.echo("Type = Rectangular  Associative = No (3 rows × 4 columns)");
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }

    /// SCALE Reference: reference length (typed, or two points), then new length (typed, or a
    /// point measured from the base point). The scale factor is new length / reference length.
    fn scale_reference(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Done) };
        match (self.ref_len, self.ref_from, i) {
            (None, None, Input::Point(p)) => {
                self.ref_from = Some(p);
                Ok(Step::Continue)
            }
            (None, None, Input::Text(t)) => {
                self.ref_len = Some(require_length(number(&t))?);
                Ok(Step::Continue)
            }
            (None, None, Input::Enter) => {
                self.ref_len = Some(1.0);
                Ok(Step::Continue)
            }
            (None, Some(a), Input::Point(p)) => {
                self.ref_len = Some(require_length(Some(a.dist(p)))?);
                Ok(Step::Continue)
            }
            (Some(r), _, Input::Point(p)) => self.scale_by(s, base, base.dist(p) / r),
            (Some(r), _, Input::Text(t)) => {
                let n = require_length(number(&t))?;
                self.scale_by(s, base, n / r)
            }
            (_, _, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }

    /// ROTATE Reference: reference angle (typed, or two points), then the new angle (typed, a
    /// point measured from the base point, or two points after `Points`). The objects turn by the
    /// new angle minus the reference angle.
    fn rotate_reference(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Done) };
        let angle =
            |t: &str| crate::units::parse_angle(t).filter(|a| a.is_finite()).ok_or_else(|| EngineError::Other("Requires an angle or point.".into()));
        let two_points = |a: Vec2, p: Vec2| {
            if a.near(p, 1e-12) { Err(EngineError::Other("The two points must differ.".into())) } else { Ok(a.angle_to(p)) }
        };
        let Some(r) = self.ref_angle else {
            match (self.ref_from, i) {
                (None, Input::Point(p)) => self.ref_from = Some(p),
                (None, Input::Text(t)) => self.ref_angle = Some(angle(&t)?),
                (None, Input::Enter) => self.ref_angle = Some(0.0),
                (Some(a), Input::Point(p)) => {
                    self.ref_angle = Some(two_points(a, p)?);
                    self.ref_from = None;
                }
                _ => {}
            }
            return Ok(Step::Continue);
        };
        let new = match (self.ref_points, self.ref_from, i) {
            (false, _, Input::Keyword(k)) if k == "Points" => {
                self.ref_points = true;
                return Ok(Step::Continue);
            }
            (false, _, Input::Point(p)) => base.angle_to(p),
            (false, _, Input::Text(t)) => angle(&t)?,
            (false, _, Input::Enter) => 0.0,
            (true, None, Input::Point(p)) => {
                self.ref_from = Some(p);
                return Ok(Step::Continue);
            }
            (true, Some(a), Input::Point(p)) => two_points(a, p)?,
            _ => return Ok(Step::Continue),
        };
        transform_entities(s, &self.objs, &Mat3::rotate_about(base, new - r), self.copy_mode)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    /// The prompt while ROTATE Reference collects its angles.
    fn rotate_reference_prompt(&self) -> Prompt {
        match (self.ref_angle, self.ref_points, self.ref_from) {
            (None, _, None) => Prompt::new("Specify the reference angle", Accept::POINT_OR_NUMBER).default("0"),
            (None, _, Some(p)) | (Some(_), true, Some(p)) => Prompt::new("Specify second point", Accept::POINT).base(p),
            (Some(_), false, _) => {
                Prompt::new("Specify the new angle", Accept::POINT_OR_NUMBER).kw(&["Points"]).default("0").base_opt(self.pts.first().copied())
            }
            (Some(_), true, None) => Prompt::new("Specify first point", Accept::POINT),
        }
    }

    /// Rubber band while ROTATE Reference collects its angles: the objects turned to the cursor
    /// once the reference angle is known, else the line from the first point.
    fn rotate_reference_preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(base), Ok(d)) = (self.pts.first().copied(), s.doc()) else { return Vec::new() };
        match (self.ref_angle, self.ref_points, self.ref_from) {
            (Some(r), false, _) => {
                let m = Mat3::rotate_about(base, base.angle_to(c) - r);
                let mut out: Vec<EntityKind> = self
                    .objs
                    .iter()
                    .take(500)
                    .filter_map(|h| d.entity(*h))
                    .map(|e| {
                        let mut k = e.kind.clone();
                        k.transform(&m);
                        k
                    })
                    .collect();
                out.push(line(base, c));
                out
            }
            (_, _, Some(a)) => vec![line(a, c)],
            _ => Vec::new(),
        }
    }

    fn scale_by(&mut self, s: &mut Session, base: Vec2, f: f64) -> Result<Step> {
        let f = require_length(Some(f))?;
        transform_entities(s, &self.objs, &Mat3::scale_about(base, f), self.copy_mode)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

/// A finite length greater than zero (rejects 0, negatives, NaN and infinity).
fn positive_length(v: Option<f64>) -> Option<f64> {
    v.filter(|f| f.is_finite() && *f > 1e-12)
}

/// [`positive_length`], or the error that makes the prompt ask again.
fn require_length(v: Option<f64>) -> Result<f64> {
    positive_length(v).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))
}

impl Interactive for SelectThen {
    fn name(&self) -> &'static str {
        match self.op {
            Op::Erase => "ERASE",
            Op::Move => "MOVE",
            Op::Copy => "COPY",
            Op::Rotate => "ROTATE",
            Op::Scale => "SCALE",
            Op::Mirror => "MIRROR",
            Op::Stretch => "STRETCH",
            Op::Explode => "EXPLODE",
            Op::ArrayRect => "ARRAYRECT",
            Op::ArrayPolar => "ARRAYPOLAR",
            Op::Join => "JOIN",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if self.op == Op::Stretch {
            s.set_selection(Vec::new());
            s.echo("Select objects to stretch by crossing-window or crossing-polygon...");
            return Ok(Step::Continue);
        }
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            return self.apply(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.op == Op::Stretch && self.window.is_none() {
            return match self.pts.first() {
                None => Prompt::new("Select objects (crossing window first corner)", Accept::POINT),
                Some(p) => Prompt::new("Specify opposite corner", Accept::POINT).base(*p),
            };
        }
        if !self.sel.done && self.op != Op::Stretch {
            return self.sel.prompt();
        }
        if self.ask_erase {
            return Prompt::new("Erase source objects?", Accept::TEXT).kw(&["Yes", "No"]).default("No");
        }
        let n = self.pts.len();
        let base = self.pts.first().copied();
        let pts_from = if self.op == Op::Stretch { 2 } else { 0 };
        let k = n.saturating_sub(pts_from);
        let bp = if self.op == Op::Stretch { self.pts.get(2).copied() } else { base };
        if self.op == Op::Scale && self.reference {
            return match (self.ref_len, self.ref_from) {
                (Some(_), _) => Prompt::new("Specify new length", Accept::POINT_OR_NUMBER).base_opt(base),
                (None, Some(p)) => Prompt::new("Specify second point", Accept::POINT).base(p),
                (None, None) => Prompt::new("Specify reference length", Accept::POINT_OR_NUMBER).default("1"),
            };
        }
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference_prompt();
        }
        match (self.op, k) {
            (Op::Move | Op::Copy | Op::Stretch, 0) => Prompt::new("Specify base point", Accept::POINT).kw(&["Displacement"]),
            (Op::Move | Op::Stretch, _) => Prompt::new("Specify second point or <use first point as displacement>", Accept::POINT).base_opt(bp),
            (Op::Copy, _) => Prompt::new("Specify second point", Accept::POINT).kw(&["Array", "Exit", "Undo"]).base_opt(base),
            (Op::Rotate | Op::Scale, 0) => Prompt::new("Specify base point", Accept::POINT),
            (Op::Rotate, _) => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).kw(&["Copy", "Reference"]).default("0").base_opt(base),
            (Op::Scale, _) => Prompt::new("Specify scale factor", Accept::POINT_OR_NUMBER).kw(&["Copy", "Reference"]).base_opt(base),
            (Op::Mirror, 0) => Prompt::new("Specify first point of mirror line", Accept::POINT),
            (Op::Mirror, _) => Prompt::new("Specify second point of mirror line", Accept::POINT).base_opt(base),
            (Op::ArrayPolar, 0) => Prompt::new("Specify center point of array", Accept::POINT).kw(&["Base point", "Axis of rotation"]),
            (Op::ArrayPolar, _) => Prompt::new("Enter number of items", Accept::NUMBER).default("6"),
            _ => Prompt::new("", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.op == Op::Stretch && self.window.is_none() {
            if let Input::Point(p) = i {
                self.pts.push(p);
                if self.pts.len() == 2 {
                    let bx = cadcraft_geom::Bounds2::new(self.pts[0], self.pts[1]);
                    self.window = Some(bx);
                    let space = s.space();
                    self.objs = crate::select::select_window(s.doc()?, &space, bx, true);
                    self.objs.retain(|h| !super::curves::is_locked(s, *h));
                    s.echo(format!("{} found", self.objs.len()));
                    s.set_selection(self.objs.clone());
                    self.sel.done = true;
                    if self.objs.is_empty() {
                        return Ok(Step::Done);
                    }
                }
            }
            return Ok(Step::Continue);
        }
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    return self.apply(s);
                }
            }
        }
        if self.ask_erase {
            let yes = matches!(&i, Input::Keyword(k) if k == "Yes") || matches!(&i, Input::Text(t) if t.trim().to_ascii_lowercase().starts_with('y'));
            let (a, b) = (self.pts[0], self.pts[1]);
            transform_entities(s, &self.objs, &Mat3::mirror(a, b), !yes)?;
            s.set_selection(Vec::new());
            return Ok(Step::Done);
        }
        let off = if self.op == Op::Stretch { 2 } else { 0 };
        let k = self.pts.len() - off;
        if self.op == Op::Scale && self.reference {
            return self.scale_reference(s, i);
        }
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference(s, i);
        }
        match (self.op, k, i) {
            (_, _, Input::Keyword(kw)) if kw == "Copy" => {
                self.copy_mode = true;
                s.echo("Rotating/scaling a copy of the selected objects.");
                Ok(Step::Continue)
            }
            (Op::Scale | Op::Rotate, 1, Input::Keyword(kw)) if kw == "Reference" => {
                self.reference = true;
                Ok(Step::Continue)
            }
            (Op::Copy, _, Input::Keyword(kw)) if kw == "Exit" => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, 0, Input::Point(p)) => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            (Op::Move, 1, Input::Point(p)) => {
                let d = p - self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), false)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Move, 1, Input::Enter) => {
                let d = self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), false)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Stretch, 1, Input::Point(p)) => {
                let d = p - self.pts[2];
                if let Some(bx) = self.window {
                    let doc = s.doc_mut()?;
                    for h in &self.objs {
                        doc.modify_entity(*h, |e| stretch_entity(e, &bx, d))?;
                    }
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Copy, _, Input::Point(p)) => {
                let d = p - self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), true)?;
                Ok(Step::Continue)
            }
            (Op::Copy, _, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Rotate, 1, Input::Point(p)) => {
                let a = self.pts[0].angle_to(p);
                transform_entities(s, &self.objs, &Mat3::rotate_about(self.pts[0], a), self.copy_mode)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Rotate, 1, Input::Text(t)) => {
                let a = crate::units::parse_angle(&t).ok_or_else(|| EngineError::Other("Requires an angle or point.".into()))?;
                transform_entities(s, &self.objs, &Mat3::rotate_about(self.pts[0], a), self.copy_mode)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Scale, 1, Input::Point(p)) => {
                let f = self.pts[0].dist(p);
                if f > 1e-12 {
                    transform_entities(s, &self.objs, &Mat3::scale_about(self.pts[0], f), self.copy_mode)?;
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Scale, 1, Input::Text(t)) => {
                let f = number(&t).filter(|f| *f > 0.0).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))?;
                transform_entities(s, &self.objs, &Mat3::scale_about(self.pts[0], f), self.copy_mode)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Mirror, 1, Input::Point(p)) => {
                if p.near(self.pts[0], 1e-12) {
                    return Err(EngineError::Other("Mirror line points must differ.".into()));
                }
                self.pts.push(p);
                self.ask_erase = true;
                Ok(Step::Continue)
            }
            (Op::ArrayPolar, 1, inp) => {
                let n = match inp {
                    Input::Text(t) => {
                        t.trim().parse::<u64>().ok().filter(|n| *n >= 1).ok_or_else(|| EngineError::Other("Requires a positive integer.".into()))?
                    }
                    Input::Enter => 6,
                    _ => return Ok(Step::Continue),
                };
                let objs: Vec<String> = self.objs.iter().map(|h| h.hex()).collect();
                run_arraypolar(s, &json!({ "handles": objs, "center": [self.pts[0].x, self.pts[0].y], "count": n }))?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, _, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference_preview(s, c);
        }
        if !self.sel.done {
            return Vec::new();
        }
        let Some(base) = (if self.op == Op::Stretch { self.pts.get(2) } else { self.pts.first() }).copied() else { return Vec::new() };
        let m = match self.op {
            Op::Move | Op::Copy | Op::Stretch => Mat3::translate(c - base),
            Op::Rotate => Mat3::rotate_about(base, base.angle_to(c)),
            Op::Scale if !self.reference => Mat3::scale_about(base, base.dist(c).max(1e-9)),
            Op::Scale => match self.ref_len.and_then(|r| positive_length(Some(base.dist(c) / r))) {
                Some(f) => Mat3::scale_about(base, f),
                None if self.ref_len.is_some() => return Vec::new(),
                None => return self.ref_from.map(|a| vec![line(a, c)]).unwrap_or_default(),
            },
            Op::Mirror if !self.ask_erase => Mat3::mirror(base, c),
            _ => return Vec::new(),
        };
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut out: Vec<EntityKind> = self
            .objs
            .iter()
            .take(500)
            .filter_map(|h| d.entity(*h))
            .map(|e| {
                let mut k = e.kind.clone();
                if self.op == Op::Stretch {
                    let mut ee = (**e).clone();
                    if let Some(bx) = self.window {
                        stretch_entity(&mut ee, &bx, c - base);
                    }
                    return ee.kind;
                }
                k.transform(&m);
                k
            })
            .collect();
        out.push(line(base, c));
        out
    }
}

struct OffsetM {
    dist: Option<f64>,
    through: bool,
    picked: Option<Handle>,
}

impl OffsetM {
    fn new(s: &Session) -> Self {
        let _ = s;
        OffsetM { dist: None, through: false, picked: None }
    }
}

impl Interactive for OffsetM {
    fn name(&self) -> &'static str {
        "OFFSET"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        s.echo("Current settings: Erase source=No  Layer=Source  OFFSETGAPTYPE=0");
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let last = s.doc().map(|d| d.header.f64("OFFSETDIST", -1.0)).unwrap_or(-1.0);
        match (self.dist.is_some() || self.through, self.picked) {
            (false, _) => Prompt::new("Specify offset distance", Accept::POINT_OR_NUMBER).kw(&["Through", "Erase", "Layer"]).default(if last < 0.0 {
                "Through".to_string()
            } else {
                format!("{last:.4}")
            }),
            (true, None) => Prompt::new("Select object to offset", Accept::POINT).kw(&["Exit", "Undo"]),
            (true, Some(_)) if self.through => Prompt::new("Specify through point", Accept::POINT).kw(&["Exit", "Multiple", "Undo"]),
            (true, Some(_)) => Prompt::new("Specify point on side to offset", Accept::POINT).kw(&["Exit", "Multiple", "Undo"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.dist.is_none() && !self.through {
            match i {
                Input::Text(t) => {
                    let d = number(&t).filter(|d| *d > 0.0).ok_or_else(|| EngineError::Other("Requires a positive distance.".into()))?;
                    self.dist = Some(d);
                    s.doc_mut()?.header.set_f64("OFFSETDIST", d);
                }
                Input::Keyword(k) if k == "Through" => self.through = true,
                Input::Enter => {
                    let last = s.doc()?.header.f64("OFFSETDIST", -1.0);
                    if last > 0.0 {
                        self.dist = Some(last);
                    } else {
                        self.through = true;
                    }
                }
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match (self.picked, i) {
            (_, Input::Keyword(k)) if k == "Exit" => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (None, Input::Point(p)) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                match crate::select::pick(s.doc()?, &space, p, ap) {
                    Some(h) => {
                        self.picked = Some(h);
                        s.set_selection(vec![h]);
                    }
                    None => s.echo("*Invalid selection*"),
                }
                Ok(Step::Continue)
            }
            (Some(h), Input::Point(p)) => {
                let e = s.doc()?.entity(h).map(|e| (**e).clone());
                if let Some(e) = e {
                    let dist = match self.dist {
                        Some(d) => d,
                        None => {
                            let polys = crate::select::hit_polylines(s.doc()?, &e, 1e-4);
                            polys
                                .iter()
                                .flat_map(|pl| pl.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?).dist(p))))
                                .fold(f64::INFINITY, f64::min)
                        }
                    };
                    match offset_kind(&e.kind, dist, p) {
                        Some(k) => {
                            let space = s.space();
                            let d = s.doc_mut()?;
                            let nh = d.new_handle();
                            if let Some(st) = d.space_mut(&space) {
                                st.push(Entity { handle: nh, common: e.common.clone(), kind: k });
                            }
                        }
                        None => s.echo("Cannot offset that object."),
                    }
                }
                self.picked = None;
                s.set_selection(Vec::new());
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(h), Ok(d)) = (self.picked, s.doc()) else { return Vec::new() };
        let Some(e) = d.entity(h) else { return Vec::new() };
        let dist = match self.dist {
            Some(x) => x,
            None => crate::select::entity_distance(d, e, c, 1e-3),
        };
        offset_kind(&e.kind, dist, c).into_iter().collect()
    }
}

struct TrimM {
    extend: bool,
}

impl Interactive for TrimM {
    fn name(&self) -> &'static str {
        if self.extend { "EXTEND" } else { "TRIM" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        s.echo("Current settings: Projection=UCS, Edge=None, Mode=Quick");
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.extend {
            Prompt::new("Select object to extend or shift-select to trim", Accept::POINT).kw(&[
                "Boundary edges",
                "Fence",
                "Crossing",
                "mOde",
                "Project",
                "Undo",
            ])
        } else {
            Prompt::new("Select object to trim or shift-select to extend", Accept::POINT).kw(&[
                "cuTting edges",
                "Fence",
                "Crossing",
                "mOde",
                "Project",
                "eRase",
            ])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                let Some(h) = crate::select::pick(s.doc()?, &space, p, ap) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                let r = if self.extend { extend(s, h, p, None).map(|_| ()) } else { trim(s, h, p, None).map(|_| ()) };
                if let Err(e) = r {
                    s.echo(e.to_string());
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            Input::Keyword(k) => {
                s.echo(format!("{k}: not available yet"));
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
}

struct FilletM {
    chamfer: bool,
    first: Option<(Handle, Vec2)>,
    asking: bool,
    polyline: bool,
}

impl FilletM {
    fn new(_s: &Session, chamfer: bool) -> Self {
        FilletM { chamfer, first: None, asking: false, polyline: false }
    }
}

impl Interactive for FilletM {
    fn name(&self) -> &'static str {
        if self.chamfer { "CHAMFER" } else { "FILLET" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        let d = s.doc()?;
        let msg = if self.chamfer {
            format!("(TRIM mode) Current chamfer Dist1 = {:.4}, Dist2 = {:.4}", d.header.f64("CHAMFERA", 0.0), d.header.f64("CHAMFERB", 0.0))
        } else {
            format!("Current settings: Mode = TRIM, Radius = {:.4}", d.header.f64("FILLETRAD", 0.0))
        };
        s.echo(msg);
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking {
            return Prompt::new(if self.chamfer { "Specify first chamfer distance" } else { "Specify fillet radius" }, Accept::NUMBER);
        }
        if self.polyline {
            return Prompt::new("Select 2D polyline", Accept::POINT);
        }
        match (self.first, self.chamfer) {
            (None, false) => Prompt::new("Select first object", Accept::POINT).kw(&["Undo", "Polyline", "Radius", "Trim", "Multiple"]),
            (None, true) => {
                Prompt::new("Select first line", Accept::POINT).kw(&["Undo", "Polyline", "Distance", "Angle", "Trim", "mEthod", "Multiple"])
            }
            (Some(_), false) => Prompt::new("Select second object or shift-select to apply corner", Accept::POINT).kw(&["Radius"]),
            (Some(_), true) => Prompt::new("Select second line or shift-select to apply corner", Accept::POINT).kw(&["Distance", "Angle", "Method"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking {
            if let Input::Text(t) = &i {
                let v = number(t).filter(|v| *v >= 0.0).ok_or_else(|| EngineError::Other("Requires a non-negative distance.".into()))?;
                let d = s.doc_mut()?;
                if self.chamfer {
                    d.header.set_f64("CHAMFERA", v);
                    d.header.set_f64("CHAMFERB", v);
                } else {
                    d.header.set_f64("FILLETRAD", v);
                }
            }
            self.asking = false;
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(k) if k == "Radius" || k == "Distance" => {
                self.asking = true;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Polyline" && !self.chamfer => {
                self.polyline = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                let Some(h) = crate::select::pick(s.doc()?, &space, p, ap) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                if self.polyline {
                    let r = s.doc()?.header.f64("FILLETRAD", 0.0);
                    let n = super::modify2::fillet_polyline(s, h, r)?;
                    s.echo(format!("{n} lines were filleted"));
                    return Ok(Step::Done);
                }
                match self.first {
                    None => {
                        self.first = Some((h, p));
                        s.set_selection(vec![h]);
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) => {
                        let d = s.doc()?;
                        let r = d.header.f64("FILLETRAD", 0.0);
                        let ch = (d.header.f64("CHAMFERA", 0.0), d.header.f64("CHAMFERB", 0.0));
                        let res = fillet_lines(s, h1, p1, h, p, r, if self.chamfer { Some(ch) } else { None });
                        s.set_selection(Vec::new());
                        match res {
                            Ok(_) => Ok(Step::Done),
                            Err(e) => {
                                s.echo(e.to_string());
                                self.first = None;
                                Ok(Step::Continue)
                            }
                        }
                    }
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[derive(Default)]
struct BreakM {
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
                match crate::select::pick(s.doc()?, &space, p, ap) {
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

pub(crate) fn _unused(_: Circle) {}
