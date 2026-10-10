//! Shared curve geometry for the M1/M2 draw and modify commands: arc constructions, tangent
//! circles (TTR/TTT), length parametrisation of curves, spline splitting and picking helpers.

use cadcraft_doc::{Entity, EntityKind, Handle, Prim};
use cadcraft_geom::{Arc, Circle, EPS, Line, PolyVertex, Polyline, Segment, Spline, TAU, Vec2, circle_circle, line_circle, line_line_infinite};

use crate::{EngineError, Result, Session};

/// Hard cap on generated vertices/points from user-sized inputs.
pub const MAX_GEN: usize = 20_000;

/// A keyword-only prompt: spaces separate inputs (like Enter), as at AutoCAD's option prompts.
pub const KW: crate::Accept = crate::Accept { point: false, number: false, text: false, select: false, enter: true };

// ---------------- picking ----------------

pub fn pick_aperture(s: &Session) -> f64 {
    s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5
}

/// The object under `p` in the current space.
pub fn pick_at(s: &Session, p: Vec2) -> Option<Handle> {
    let space = s.space();
    crate::select::pick(s.doc().ok()?, &space, p, pick_aperture(s))
}

pub fn entity(s: &Session, h: Handle) -> Result<Entity> {
    s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))
}

pub fn handle_value(v: &serde_json::Value) -> Option<Handle> {
    v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))
}

pub fn handle_param(p: &serde_json::Value, key: &str) -> Option<Handle> {
    p.get(key).and_then(handle_value)
}

/// Is the entity on a locked layer?
pub fn is_locked(s: &Session, h: Handle) -> bool {
    s.doc().ok().is_some_and(|d| d.entity(h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked))
}

// ---------------- arc constructions ----------------

pub(crate) fn finite_arc(a: Arc) -> Option<Arc> {
    (a.center.is_finite() && a.radius.is_finite() && a.radius > EPS && a.start.is_finite() && a.end.is_finite()).then_some(a)
}

/// Arc from start, center and a signed included angle (CCW positive).
pub fn arc_sca(st: Vec2, c: Vec2, angle: f64) -> Option<Arc> {
    let r = st.dist(c);
    if !angle.is_finite() || angle.abs() < 1e-12 {
        return None;
    }
    let a0 = c.angle_to(st);
    let a = angle.clamp(-TAU, TAU);
    finite_arc(if a > 0.0 { Arc::new(c, r, a0, a0 + a) } else { Arc::new(c, r, a0 + a, a0) })
}

/// Arc from start, center and chord length (positive: CCW minor arc, negative: CCW major arc).
pub fn arc_scl(st: Vec2, c: Vec2, chord: f64) -> Option<Arc> {
    let r = st.dist(c);
    if r <= EPS || !chord.is_finite() || chord.abs() < 1e-12 || chord.abs() > 2.0 * r + 1e-9 {
        return None;
    }
    let theta = 2.0 * (chord.abs() / (2.0 * r)).clamp(-1.0, 1.0).asin();
    let sweep = if chord > 0.0 { theta } else { TAU - theta };
    arc_sca(st, c, sweep)
}

/// Arc from start and end points with a signed included angle (CCW positive).
pub fn arc_sea(st: Vec2, en: Vec2, angle: f64) -> Option<Arc> {
    if !angle.is_finite() || angle.abs() < 1e-9 || angle.abs() >= TAU - 1e-9 || st.near(en, EPS) {
        return None;
    }
    let (a, b, th) = if angle > 0.0 { (st, en, angle) } else { (en, st, -angle) };
    let chord = a.dist(b);
    let r = chord / (2.0 * (th / 2.0).sin());
    let h = (chord / 2.0) / (th / 2.0).tan();
    let c = a.mid(b) + (b - a).normalized().perp() * h;
    finite_arc(Arc::new(c, r.abs(), c.angle_to(a), c.angle_to(b)))
}

/// Arc from start and end points with the tangent direction at the start.
pub fn arc_sed(st: Vec2, en: Vec2, dir: Vec2) -> Option<Arc> {
    let d = dir.normalized();
    if d == Vec2::ZERO || st.near(en, EPS) {
        return None;
    }
    let n = d.perp();
    let v = en - st;
    let den = 2.0 * n.dot(v);
    if den.abs() < 1e-12 * v.len().max(1.0) {
        return None;
    }
    let t = v.len2() / den;
    let c = st + n * t;
    let r = c.dist(st);
    finite_arc(if t > 0.0 { Arc::new(c, r, c.angle_to(st), c.angle_to(en)) } else { Arc::new(c, r, c.angle_to(en), c.angle_to(st)) })
}

/// Arc from start and end points with a radius (CCW; negative radius gives the major arc).
pub fn arc_ser(st: Vec2, en: Vec2, radius: f64) -> Option<Arc> {
    let r = radius.abs();
    let chord = st.dist(en);
    if !radius.is_finite() || chord < EPS || r < chord / 2.0 - 1e-9 {
        return None;
    }
    let h = (r * r - chord * chord / 4.0).max(0.0).sqrt();
    let left = (en - st).normalized().perp();
    let c = if radius > 0.0 { st.mid(en) + left * h } else { st.mid(en) - left * h };
    finite_arc(Arc::new(c, r, c.angle_to(st), c.angle_to(en)))
}

/// End point and outgoing unit tangent of the last line/arc/open polyline in the current space
/// (ARC "Continue" and LINE continuation).
pub fn last_curve_end(s: &Session) -> Option<(Vec2, Vec2)> {
    let d = s.doc().ok()?;
    let space = s.space();
    let store = d.space(&space)?;
    for e in store.iter().collect::<Vec<_>>().into_iter().rev() {
        match &e.kind {
            EntityKind::Line(l) => {
                let dir = (l.b.xy() - l.a.xy()).normalized();
                if dir != Vec2::ZERO {
                    return Some((l.b.xy(), dir));
                }
            }
            EntityKind::Arc(a) => {
                let g = Arc::new(a.center.xy(), a.radius, a.start, a.end);
                return Some((g.end_point(), Vec2::from_angle(g.end).perp()));
            }
            EntityKind::LwPolyline(p) if !p.closed => {
                let segs = Polyline { vertices: p.vertices.clone(), closed: false }.segments();
                if let Some(sg) = segs.last() {
                    return Some((sg.end(), sg.tangent(1.0)));
                }
            }
            _ => {}
        }
    }
    None
}

/// Signed bulge of the arc from `a` through `m` to `b` (0 when collinear).
pub fn bulge_through(a: Vec2, m: Vec2, b: Vec2) -> f64 {
    let Some(arc) = Arc::from_3_points(a, m, b) else { return 0.0 };
    let sw = arc.sweep();
    if arc.start_point().near(a, 1e-6 * arc.radius.max(1.0)) { cadcraft_geom::arc_to_bulge(sw) } else { cadcraft_geom::arc_to_bulge(-sw) }
}

/// Bulge of the arc from `a` to `b` leaving `a` along `tangent`.
pub fn bulge_from_tangent(a: Vec2, b: Vec2, tangent: Vec2) -> f64 {
    let c = b - a;
    let t = tangent.normalized();
    if c.len() < EPS || t == Vec2::ZERO {
        return 0.0;
    }
    let alpha = t.cross(c).atan2(t.dot(c));
    cadcraft_geom::arc_to_bulge(2.0 * alpha)
}

// ---------------- tangent circles ----------------

/// A tangency target: an infinite line or a full circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TanObj {
    Line(Line),
    Circle(Circle),
}

impl TanObj {
    /// From an entity and the pick point (polylines use the picked segment).
    pub fn from_kind(k: &EntityKind, pick: Vec2) -> Option<TanObj> {
        match k {
            EntityKind::Line(l) => {
                let ln = Line::new(l.a.xy(), l.b.xy());
                (ln.len() > EPS).then_some(TanObj::Line(ln))
            }
            EntityKind::Circle(c) => Some(TanObj::Circle(Circle::new(c.center.xy(), c.radius))),
            EntityKind::Arc(a) => Some(TanObj::Circle(Circle::new(a.center.xy(), a.radius))),
            EntityKind::Ray(r) | EntityKind::XLine(r) => Some(TanObj::Line(Line::new(r.base.xy(), r.base.xy() + r.dir.xy()))),
            EntityKind::LwPolyline(p) => {
                let segs = Polyline { vertices: p.vertices.clone(), closed: p.closed }.segments();
                let sg = segs.iter().min_by(|a, b| a.closest(pick).dist(pick).total_cmp(&b.closest(pick).dist(pick)))?;
                Some(match sg {
                    Segment::Line(l) => TanObj::Line(*l),
                    Segment::Arc { arc, .. } => TanObj::Circle(Circle::new(arc.center, arc.radius)),
                })
            }
            _ => None,
        }
    }
    /// The point where a circle `c` tangent to this object touches it.
    pub fn touch(&self, c: &Circle) -> Vec2 {
        match self {
            TanObj::Line(l) => l.project(c.center),
            TanObj::Circle(k) => {
                let d = (c.center - k.center).normalized();
                let d = if d == Vec2::ZERO { Vec2::X } else { d };
                let p1 = k.center + d * k.radius;
                let p2 = k.center - d * k.radius;
                if (p1.dist(c.center) - c.radius).abs() <= (p2.dist(c.center) - c.radius).abs() { p1 } else { p2 }
            }
        }
    }
    /// Tangency residual of circle `(c, r)`: zero when tangent.
    fn residual(&self, c: Vec2, r: f64) -> f64 {
        match self {
            TanObj::Line(l) => l.dist_inf(c) - r,
            TanObj::Circle(k) => {
                let d = c.dist(k.center);
                (d - (k.radius + r)).abs().min((d - (k.radius - r).abs()).abs())
            }
        }
    }
    fn offsets(&self, r: f64) -> Vec<TanObj> {
        match self {
            TanObj::Line(l) => vec![TanObj::Line(l.offset(r)), TanObj::Line(l.offset(-r))],
            TanObj::Circle(k) => {
                let mut v = vec![TanObj::Circle(Circle::new(k.center, k.radius + r))];
                if (k.radius - r).abs() > EPS {
                    v.push(TanObj::Circle(Circle::new(k.center, (k.radius - r).abs())));
                }
                v
            }
        }
    }
}

trait InfDist {
    fn dist_inf(&self, p: Vec2) -> f64;
}
impl InfDist for Line {
    fn dist_inf(&self, p: Vec2) -> f64 {
        self.project(p).dist(p)
    }
}

fn intersect_tan(a: &TanObj, b: &TanObj) -> Vec<Vec2> {
    match (a, b) {
        (TanObj::Line(x), TanObj::Line(y)) => line_line_infinite(x.a, x.b, y.a, y.b).map(|r| vec![r.0]).unwrap_or_default(),
        (TanObj::Line(l), TanObj::Circle(c)) | (TanObj::Circle(c), TanObj::Line(l)) => line_circle(l, c).into_iter().map(|x| x.0).collect(),
        (TanObj::Circle(x), TanObj::Circle(y)) => circle_circle(x, y),
    }
}

/// Score a candidate: how far its tangent points are from the picks.
fn score(c: &Circle, objs: &[(TanObj, Vec2)]) -> f64 {
    objs.iter().map(|(o, p)| o.touch(c).dist(*p)).sum()
}

/// Circle of radius `r` tangent to two objects, nearest the pick points.
pub fn ttr(o1: TanObj, p1: Vec2, o2: TanObj, p2: Vec2, r: f64) -> Option<Circle> {
    if !(r.is_finite() && r > EPS) {
        return None;
    }
    let objs = [(o1, p1), (o2, p2)];
    let mut best: Option<(f64, Circle)> = None;
    for a in o1.offsets(r) {
        for b in o2.offsets(r) {
            for c in intersect_tan(&a, &b) {
                let ci = Circle::new(c, r);
                if !c.is_finite() || o1.residual(c, r).abs() > 1e-6 * r.max(1.0) || o2.residual(c, r).abs() > 1e-6 * r.max(1.0) {
                    continue;
                }
                let sc = score(&ci, &objs);
                if best.is_none_or(|(b, _)| sc < b) {
                    best = Some((sc, ci));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Solve a 3×3 linear system (Cramer's rule).
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(&m);
    if !d.is_finite() || d.abs() < 1e-14 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut mk = m;
        for (row, bv) in mk.iter_mut().zip(b.iter()) {
            row[k] = *bv;
        }
        *o = det(&mk) / d;
    }
    Some(out)
}

/// A signed tangency equation f(x, y, r) = 0 with its gradient.
#[derive(Clone, Copy)]
enum Eq3 {
    /// n·p + c = s r (unit normal n).
    Line { n: Vec2, c: f64, s: f64 },
    /// |p − k| = R + s r (s = ±1), or r − R when `enclose`.
    Circle { k: Vec2, rr: f64, s: f64, enclose: bool },
}

impl Eq3 {
    fn eval(&self, x: [f64; 3]) -> (f64, [f64; 3]) {
        let p = Vec2::new(x[0], x[1]);
        let r = x[2];
        match *self {
            Eq3::Line { n, c, s } => (n.dot(p) + c - s * r, [n.x, n.y, -s]),
            Eq3::Circle { k, rr, s, enclose } => {
                let d = p - k;
                let l = d.len().max(1e-12);
                let g = [d.x / l, d.y / l];
                if enclose { (l - (r - rr), [g[0], g[1], -1.0]) } else { (l - (rr + s * r), [g[0], g[1], -s]) }
            }
        }
    }
}

fn eq_variants(o: &TanObj) -> Vec<Eq3> {
    match o {
        TanObj::Line(l) => {
            let n = (l.b - l.a).normalized().perp();
            let c = -n.dot(l.a);
            vec![Eq3::Line { n, c, s: 1.0 }, Eq3::Line { n, c, s: -1.0 }]
        }
        TanObj::Circle(k) => vec![
            Eq3::Circle { k: k.center, rr: k.radius, s: 1.0, enclose: false },
            Eq3::Circle { k: k.center, rr: k.radius, s: -1.0, enclose: false },
            Eq3::Circle { k: k.center, rr: k.radius, s: 0.0, enclose: true },
        ],
    }
}

/// Circle tangent to three objects, nearest the pick points (Newton on all sign combinations).
pub fn ttt(objs: [(TanObj, Vec2); 3]) -> Option<Circle> {
    let [(o1, p1), (o2, p2), (o3, p3)] = objs;
    let mut starts: Vec<[f64; 3]> = Vec::new();
    let cen = (p1 + p2 + p3) / 3.0;
    let rad = (cen.dist(p1) + cen.dist(p2) + cen.dist(p3)) / 3.0;
    starts.push([cen.x, cen.y, rad.max(1e-3)]);
    if let Some(c) = Circle::from_3_points(p1, p2, p3) {
        starts.push([c.center.x, c.center.y, c.radius]);
    }
    let mut best: Option<(f64, Circle)> = None;
    let scale = rad.max(p1.dist(p2)).max(1e-6);
    for e1 in eq_variants(&o1) {
        for e2 in eq_variants(&o2) {
            for e3 in eq_variants(&o3) {
                for st in &starts {
                    let mut x = *st;
                    let mut converged = false;
                    for _ in 0..60 {
                        let (f1, g1) = e1.eval(x);
                        let (f2, g2) = e2.eval(x);
                        let (f3, g3) = e3.eval(x);
                        if f1.abs().max(f2.abs()).max(f3.abs()) < 1e-11 * scale {
                            converged = true;
                            break;
                        }
                        let Some(dx) = solve3([g1, g2, g3], [-f1, -f2, -f3]) else { break };
                        for (xi, di) in x.iter_mut().zip(dx) {
                            *xi += di;
                        }
                        if !x.iter().all(|v| v.is_finite()) {
                            break;
                        }
                    }
                    if !converged || x[2] <= 1e-9 * scale || x[2] > 1e6 * scale {
                        continue;
                    }
                    let c = Circle::new(Vec2::new(x[0], x[1]), x[2]);
                    let sc = score(&c, &objs);
                    if best.is_none_or(|(b, _)| sc < b - 1e-12) {
                        best = Some((sc, c));
                    }
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

// ---------------- curves as segment chains ----------------

/// A curve flattened to lines/arcs (exact for lines, arcs, circles and polylines; tessellated for
/// ellipses, splines and 3D polylines).
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub segs: Vec<Segment>,
    pub closed: bool,
}

impl Chain {
    pub fn of(k: &EntityKind) -> Option<Chain> {
        let tess = |pts: Vec<Vec2>, closed: bool| Chain {
            segs: pts.windows(2).filter_map(|w| Some(Segment::Line(Line::new(*w.first()?, *w.get(1)?)))).filter(|s| s.len() > 1e-15).collect(),
            closed,
        };
        let c = match k {
            EntityKind::Line(l) => Chain { segs: vec![Segment::Line(Line::new(l.a.xy(), l.b.xy()))], closed: false },
            EntityKind::Arc(a) => {
                Chain { segs: vec![Segment::Arc { arc: Arc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true }], closed: false }
            }
            EntityKind::Circle(c) => {
                let o = c.center.xy();
                Chain {
                    segs: vec![
                        Segment::Arc { arc: Arc::new(o, c.radius, 0.0, std::f64::consts::PI), ccw: true },
                        Segment::Arc { arc: Arc::new(o, c.radius, std::f64::consts::PI, TAU), ccw: true },
                    ],
                    closed: true,
                }
            }
            EntityKind::LwPolyline(p) => Chain { segs: Polyline { vertices: p.vertices.clone(), closed: p.closed }.segments(), closed: p.closed },
            EntityKind::Polyline3d(p) => {
                let mut pts: Vec<Vec2> = p.points.iter().map(|q| q.xy()).collect();
                if p.closed
                    && let Some(f) = pts.first().copied()
                {
                    pts.push(f);
                }
                tess(pts, p.closed)
            }
            EntityKind::Ellipse(e) => {
                let ge = cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end };
                let mut pts = Vec::new();
                ge.tessellate(e.major.xy().len() * 1e-4, &mut pts);
                tess(pts, ge.is_full())
            }
            EntityKind::Spline(sp) => tess(sp.tessellate(1e-4), sp.closed),
            _ => return None,
        };
        (!c.segs.is_empty()).then_some(c)
    }
    pub fn len(&self) -> f64 {
        self.segs.iter().map(Segment::len).sum()
    }
    pub fn start(&self) -> Option<Vec2> {
        self.segs.first().map(Segment::start)
    }
    pub fn end(&self) -> Option<Vec2> {
        self.segs.last().map(Segment::end)
    }
    /// Point and unit tangent at arc length `d` from the start (clamped).
    pub fn at_length(&self, d: f64) -> Option<(Vec2, Vec2)> {
        let mut acc = 0.0;
        for s in &self.segs {
            let l = s.len();
            if d <= acc + l + 1e-12 && l > 0.0 {
                let t = ((d - acc) / l).clamp(0.0, 1.0);
                return Some((s.at(t), s.tangent(t)));
            }
            acc += l;
        }
        self.segs.last().map(|s| (s.end(), s.tangent(1.0)))
    }
    /// Arc length of the point on the chain nearest `p`.
    pub fn length_of(&self, p: Vec2) -> f64 {
        let mut acc = 0.0;
        let mut best = (f64::INFINITY, 0.0);
        for s in &self.segs {
            let c = s.closest(p);
            let dd = c.dist(p);
            if dd < best.0 {
                best = (dd, acc + seg_param(s, c) * s.len());
            }
            acc += s.len();
        }
        best.1
    }
    /// Reversed copy.
    pub fn reversed(&self) -> Chain {
        Chain { segs: self.segs.iter().rev().map(Segment::reversed).collect(), closed: self.closed }
    }
    /// Polyline vertices for the chain.
    pub fn vertices(&self) -> Vec<PolyVertex> {
        let mut vs: Vec<PolyVertex> = self
            .segs
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
        if !self.closed
            && let Some(e) = self.end()
        {
            vs.push(PolyVertex::new(e));
        }
        vs
    }
}

/// Parameter (0..1 along travel) of a point on a segment.
pub fn seg_param(s: &Segment, p: Vec2) -> f64 {
    match s {
        Segment::Line(l) => l.param_of(p).clamp(0.0, 1.0),
        Segment::Arc { arc, ccw } => {
            let t = (cadcraft_geom::norm_angle(arc.center.angle_to(p) - arc.start) / arc.sweep().max(1e-15)).clamp(0.0, 1.0);
            if *ccw { t } else { 1.0 - t }
        }
    }
}

/// Distance from `p` to an entity's geometry (prims; text and blocks by insertion point).
pub fn kind_distance(k: &EntityKind, p: Vec2) -> f64 {
    k.prims()
        .iter()
        .map(|pr| match pr {
            Prim::Seg(s) => s.closest(p).dist(p),
            Prim::Circle(c) => c.closest(p).dist(p),
            Prim::Ellipse(e) => e.closest(p).dist(p),
            Prim::Spline(sp) => {
                let pts = sp.tessellate(1e-3);
                pts.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?).closest(p).dist(p))).fold(f64::INFINITY, f64::min)
            }
            Prim::Point(q) => q.dist(p),
            Prim::Infinite { base, dir, .. } => Line::new(*base, *base + *dir).project(p).dist(p),
            Prim::Fill(f) => {
                let n = f.len();
                (0..n).filter_map(|i| Some(Line::new(*f.get(i)?, *f.get((i + 1) % n)?).closest(p).dist(p))).fold(f64::INFINITY, f64::min)
            }
        })
        .fold(f64::INFINITY, f64::min)
}

// ---------------- splines ----------------

/// Insert knot `t` once (Boehm), in homogeneous coordinates.
fn insert_knot(sp: &Spline, t: f64) -> Option<Spline> {
    if !sp.is_valid() {
        return None;
    }
    let p = sp.degree;
    let n = sp.control.len();
    let w = |i: usize| if sp.weights.is_empty() { 1.0 } else { sp.weights.get(i).copied().unwrap_or(1.0) };
    // Span k with knots[k] <= t < knots[k+1].
    let (lo, hi) = sp.domain();
    let t = t.clamp(lo, hi);
    let mut k = p;
    for i in p..n {
        if sp.knots.get(i).copied()? <= t {
            k = i;
        }
    }
    let mut ctrl: Vec<(Vec2, f64)> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let pt = if i + p <= k {
            let c = sp.control.get(i).copied()?;
            (c * w(i), w(i))
        } else if i > k {
            let c = sp.control.get(i - 1).copied()?;
            (c * w(i - 1), w(i - 1))
        } else {
            let ki = sp.knots.get(i).copied()?;
            let kip = sp.knots.get(i + p).copied()?;
            let a = if (kip - ki).abs() < 1e-300 { 0.0 } else { (t - ki) / (kip - ki) };
            let c0 = sp.control.get(i - 1).copied()?;
            let c1 = sp.control.get(i).copied()?;
            let (h0, w0) = (c0 * w(i - 1), w(i - 1));
            let (h1, w1) = (c1 * w(i), w(i));
            (h0 * (1.0 - a) + h1 * a, w0 * (1.0 - a) + w1 * a)
        };
        ctrl.push(pt);
    }
    let mut knots = sp.knots.clone();
    knots.insert(k + 1, t);
    let rational = !sp.weights.is_empty();
    Some(Spline {
        degree: p,
        knots,
        control: ctrl.iter().map(|(h, w)| if w.abs() > 1e-300 { *h / *w } else { *h }).collect(),
        weights: if rational { ctrl.iter().map(|(_, w)| *w).collect() } else { Vec::new() },
        fit: Vec::new(),
        closed: false,
    })
}

/// Split a spline at parameter `t` into (left, right). `None` at the ends.
pub fn split_spline(sp: &Spline, t: f64) -> Option<(Spline, Spline)> {
    let (lo, hi) = sp.domain();
    if !(t > lo + 1e-12 && t < hi - 1e-12) {
        return None;
    }
    let mut s = sp.clone();
    s.fit.clear();
    let mult = |s: &Spline| s.knots.iter().filter(|k| (**k - t).abs() < 1e-12).count();
    let mut guard = 0;
    while mult(&s) < s.degree && guard < 64 {
        s = insert_knot(&s, t)?;
        guard += 1;
    }
    let p = s.degree;
    // Knots equal to t now have multiplicity p; first index of t:
    let first = s.knots.iter().position(|k| (*k - t).abs() < 1e-12)?;
    // Left: control points 0..first (count = first), knots 0..first+p ... + t
    let left_n = first;
    let mut lk: Vec<f64> = s.knots.get(..first)?.to_vec();
    lk.extend(std::iter::repeat_n(t, p + 1));
    let lc = s.control.get(..left_n)?.to_vec();
    let lw = if s.weights.is_empty() { Vec::new() } else { s.weights.get(..left_n)?.to_vec() };
    let right_start = first.checked_sub(1)?;
    let mut rk = vec![t; p + 1];
    rk.extend(s.knots.get(first + p..)?.iter().copied());
    let rc = s.control.get(right_start..)?.to_vec();
    let rw = if s.weights.is_empty() { Vec::new() } else { s.weights.get(right_start..)?.to_vec() };
    let l = Spline { degree: p, knots: lk, control: lc, weights: lw, fit: Vec::new(), closed: false };
    let r = Spline { degree: p, knots: rk, control: rc, weights: rw, fit: Vec::new(), closed: false };
    (l.is_valid() && r.is_valid()).then_some((l, r))
}

/// The part of a spline between parameters `a < b`.
pub fn sub_spline(sp: &Spline, a: f64, b: f64) -> Option<Spline> {
    let (lo, hi) = sp.domain();
    let mut s = sp.clone();
    s.fit.clear();
    s.closed = false;
    if b < hi - 1e-12 {
        s = split_spline(&s, b)?.0;
    }
    if a > lo + 1e-12 {
        s = split_spline(&s, a)?.1;
    }
    Some(s)
}

/// Parameter of the point on the spline nearest `p`.
pub fn spline_param_of(sp: &Spline, p: Vec2) -> f64 {
    let (lo, hi) = sp.domain();
    let n = 400;
    let mut best = (f64::INFINITY, lo);
    for i in 0..=n {
        let t = lo + (hi - lo) * i as f64 / n as f64;
        let d = sp.eval(t).dist(p);
        if d < best.0 {
            best = (d, t);
        }
    }
    let step = (hi - lo) / n as f64;
    let (mut a, mut b) = ((best.1 - step).max(lo), (best.1 + step).min(hi));
    for _ in 0..60 {
        let m1 = a + (b - a) / 3.0;
        let m2 = b - (b - a) / 3.0;
        if sp.eval(m1).dist(p) < sp.eval(m2).dist(p) {
            b = m2;
        } else {
            a = m1;
        }
    }
    (a + b) / 2.0
}

/// Reverse a spline's direction (control, weights, fit points and knots).
pub fn reverse_spline(sp: &Spline) -> Spline {
    let (lo, hi) = (sp.knots.first().copied().unwrap_or(0.0), sp.knots.last().copied().unwrap_or(1.0));
    Spline {
        degree: sp.degree,
        knots: sp.knots.iter().rev().map(|k| lo + hi - k).collect(),
        control: sp.control.iter().rev().copied().collect(),
        weights: sp.weights.iter().rev().copied().collect(),
        fit: sp.fit.iter().rev().copied().collect(),
        closed: sp.closed,
    }
}

/// Reverse an LWPOLYLINE's vertex order, keeping its shape (bulges and widths move with segments).
pub fn reverse_vertices(vs: &[PolyVertex], closed: bool) -> Vec<PolyVertex> {
    let n = vs.len();
    if n < 2 {
        return vs.to_vec();
    }
    (0..n)
        .map(|k| {
            let p = vs.get(n - 1 - k).map(|v| v.p).unwrap_or_default();
            let seg = (2 * n - 2 - k) % n; // old segment index reversed into new segment k
            let has_seg = closed || k + 1 < n;
            match (has_seg, vs.get(seg)) {
                (true, Some(o)) => PolyVertex { p, bulge: -o.bulge, start_width: o.end_width, end_width: o.start_width },
                _ => PolyVertex::new(p),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_builders() {
        let a = arc_sca(Vec2::new(1.0, 0.0), Vec2::ZERO, std::f64::consts::FRAC_PI_2).unwrap();
        assert!(a.end_point().near(Vec2::new(0.0, 1.0), 1e-9));
        let b = arc_sea(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), std::f64::consts::PI).unwrap();
        assert!(b.center.near(Vec2::ZERO, 1e-9));
        assert!(b.mid_point().near(Vec2::new(0.0, 1.0), 1e-9));
        let c = arc_sed(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::Y).unwrap();
        assert!(c.mid_point().near(Vec2::new(0.0, 1.0), 1e-9));
        let d = arc_ser(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), 1.0).unwrap();
        assert!(d.center.near(Vec2::ZERO, 1e-9));
        let e = arc_scl(Vec2::new(1.0, 0.0), Vec2::ZERO, 2f64.sqrt()).unwrap();
        assert!(e.end_point().near(Vec2::new(0.0, 1.0), 1e-9));
        assert!(arc_scl(Vec2::new(1.0, 0.0), Vec2::ZERO, 5.0).is_none());
        assert!((bulge_through(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0)) - 1.0).abs() < 1e-9);
        assert!((bulge_through(Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0)) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn tangent_circles() {
        let l1 = TanObj::Line(Line::new(Vec2::ZERO, Vec2::new(10.0, 0.0)));
        let l2 = TanObj::Line(Line::new(Vec2::ZERO, Vec2::new(0.0, 10.0)));
        let c = ttr(l1, Vec2::new(3.0, 0.0), l2, Vec2::new(0.0, 3.0), 1.0).unwrap();
        assert!(c.center.near(Vec2::new(1.0, 1.0), 1e-9));
        let l3 = TanObj::Line(Line::new(Vec2::new(4.0, 0.0), Vec2::new(0.0, 3.0)));
        let inc = ttt([(l1, Vec2::new(2.0, 0.0)), (l2, Vec2::new(0.0, 1.0)), (l3, Vec2::new(2.0, 1.5))]).unwrap();
        assert!(inc.center.near(Vec2::new(1.0, 1.0), 1e-6), "{inc:?}");
        assert!((inc.radius - 1.0).abs() < 1e-6);
        let k = TanObj::Circle(Circle::new(Vec2::new(5.0, 0.0), 2.0));
        let c2 = ttr(l2, Vec2::new(0.0, 0.0), k, Vec2::new(3.0, 0.0), 1.5).unwrap();
        assert!((c2.center.x - 1.5).abs() < 1e-9);
    }

    #[test]
    fn spline_split_and_reverse() {
        let sp = Spline::from_control(vec![Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 2.0), Vec2::new(4.0, 0.0)], 3);
        let (lo, hi) = sp.domain();
        let t = lo + (hi - lo) * 0.3;
        let (l, r) = split_spline(&sp, t).unwrap();
        let p = sp.eval(t);
        assert!(l.eval(l.domain().1).near(p, 1e-9));
        assert!(r.eval(r.domain().0).near(p, 1e-9));
        assert!(r.eval(r.domain().1).near(Vec2::new(4.0, 0.0), 1e-9));
        let mid = sp.eval(lo + (hi - lo) * 0.6);
        assert!(r.eval(r.domain().0 + (r.domain().1 - r.domain().0) * (0.3 / 0.7)).near(mid, 1e-9));
        let rv = reverse_spline(&sp);
        assert!(rv.eval(rv.domain().0).near(Vec2::new(4.0, 0.0), 1e-9));
        assert!((spline_param_of(&sp, p) - t).abs() < 1e-6);
        let sub = sub_spline(&sp, lo + 0.2 * (hi - lo), lo + 0.8 * (hi - lo)).unwrap();
        assert!(sub.eval(sub.domain().0).near(sp.eval(lo + 0.2 * (hi - lo)), 1e-9));
    }

    #[test]
    fn reverse_polyline_vertices() {
        let vs = vec![PolyVertex::with_bulge(Vec2::ZERO, 0.5), PolyVertex::new(Vec2::new(1.0, 0.0)), PolyVertex::new(Vec2::new(1.0, 1.0))];
        let r = reverse_vertices(&vs, false);
        assert!(r[0].p.near(Vec2::new(1.0, 1.0), 0.0));
        assert_eq!(r[0].bulge, 0.0);
        assert!((r[1].bulge + 0.5).abs() < 1e-12);
        assert_eq!(r[2].bulge, 0.0);
        let r2 = reverse_vertices(&r, false);
        assert_eq!(r2, vs);
    }
}
