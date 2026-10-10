//! Object snaps (OSNAP), polar tracking and ortho.

use cadcraft_doc::{Drawing, EntityKind, Prim, Space};
use cadcraft_geom::{Bounds2, Circle, PI, Segment, Vec2, intersect_ext};
use serde::Serialize;

/// OSMODE bits.
pub mod mode {
    pub const END: u32 = 1;
    pub const MID: u32 = 2;
    pub const CEN: u32 = 4;
    pub const NOD: u32 = 8;
    pub const QUA: u32 = 16;
    pub const INT: u32 = 32;
    pub const INS: u32 = 64;
    pub const PER: u32 = 128;
    pub const TAN: u32 = 256;
    pub const NEA: u32 = 512;
    pub const GCEN: u32 = 1024;
    pub const APP: u32 = 2048;
    pub const EXT: u32 = 4096;
    pub const PAR: u32 = 8192;
    /// Running snaps temporarily off (F3).
    pub const OFF: u32 = 16384;
    pub const ALL: [(u32, &str); 14] = [
        (END, "Endpoint"),
        (MID, "Midpoint"),
        (CEN, "Center"),
        (GCEN, "Geometric Center"),
        (NOD, "Node"),
        (QUA, "Quadrant"),
        (INT, "Intersection"),
        (EXT, "Extension"),
        (INS, "Insertion"),
        (PER, "Perpendicular"),
        (TAN, "Tangent"),
        (NEA, "Nearest"),
        (APP, "Apparent Intersection"),
        (PAR, "Parallel"),
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapHit {
    pub point: Vec2,
    pub mode: u32,
    pub name: &'static str,
    /// A deferred tangent/perpendicular: `point` is only provisional (the nearest point on the
    /// curve) until the other end of the line is known. See [`resolve_deferred`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deferred: Option<Deferred>,
}

impl SnapHit {
    /// The command input a pick at this snap gives: a deferred snap or a plain point.
    pub fn input(&self) -> crate::Input {
        match self.deferred {
            Some(d) => crate::Input::Deferred(d),
            None => crate::Input::Point(self.point),
        }
    }
}

/// The curve a deferred snap refers to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SnapCurve {
    /// A circle, or the circle an arc lies on.
    Circle { center: Vec2, radius: f64 },
    /// The infinite line through `a` and `b` (a line, xline or ray).
    Line { a: Vec2, b: Vec2 },
}

/// A deferred tangent ([`mode::TAN`]) or perpendicular ([`mode::PER`]) snap, picked while there
/// is no base point yet (the first point of a line). The real point depends on the other end of
/// the line, so it is resolved later with [`resolve_deferred`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deferred {
    pub mode: u32,
    pub curve: SnapCurve,
    /// Where the curve was picked (the nearest point on it); chooses between solutions.
    pub at: Vec2,
}

/// One end of a line being solved: a known point or a deferred snap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineEnd {
    Point(Vec2),
    Deferred(Deferred),
}

fn name_of(m: u32) -> &'static str {
    mode::ALL.iter().find(|(b, _)| *b == m).map(|(_, n)| *n).unwrap_or("Snap")
}

fn prim_segments(p: &Prim) -> Vec<Segment> {
    match p {
        Prim::Seg(s) => vec![*s],
        Prim::Circle(c) => vec![Segment::Arc { arc: cadcraft_geom::Arc::new(c.center, c.radius, 0.0, 0.0), ccw: true }],
        Prim::Infinite { base, dir, .. } => vec![Segment::Line(cadcraft_geom::Line::new(*base - *dir * 1e7, *base + *dir * 1e7))],
        Prim::Ellipse(e) => {
            let mut pts = Vec::new();
            e.tessellate(e.major.len() * 1e-3, &mut pts);
            pts.windows(2).filter_map(|w| Some(Segment::Line(cadcraft_geom::Line::new(*w.first()?, *w.get(1)?)))).collect()
        }
        Prim::Spline(s) => {
            let pts = s.tessellate(1e-3);
            pts.windows(2).filter_map(|w| Some(Segment::Line(cadcraft_geom::Line::new(*w.first()?, *w.get(1)?)))).collect()
        }
        Prim::Fill(f) => {
            let n = f.len();
            (0..n).filter_map(|i| Some(Segment::Line(cadcraft_geom::Line::new(*f.get(i)?, *f.get((i + 1) % n)?)))).collect()
        }
        Prim::Point(_) => Vec::new(),
    }
}

/// Find the best object snap near `cursor` within `aperture` (world units).
///
/// Tangent and perpendicular snaps need the base point (the other end of the line). With no
/// base and `deferred` set (the prompt says it can resolve them, `Prompt.deferred`),
/// they snap as *deferred*: the hit carries the curve and is resolved once the other end is
/// known. Deferred hits only win where no other snap does (they are as unspecific as Nearest).
pub fn osnap(d: &Drawing, space: &Space, cursor: Vec2, aperture: f64, osmode: u32, base: Option<Vec2>, deferred: bool) -> Option<SnapHit> {
    if osmode == 0 || osmode & mode::OFF != 0 {
        return None;
    }
    let ix = crate::spatial::index(d, space);
    osnap_with(d, space, &ix, cursor, aperture, osmode, base, deferred)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn osnap_with(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
    deferred: bool,
) -> Option<SnapHit> {
    if osmode == 0 || osmode & mode::OFF != 0 {
        return None;
    }
    let probe = Bounds2::new(cursor, cursor).expand(aperture);
    let mut cands: Vec<(u32, Vec2)> = Vec::new();
    let mut near_prims: Vec<Prim> = Vec::new();
    let mut count = 0usize;
    for (e, known) in crate::select::candidates(d, space, ix, &probe.expand(aperture), true, false)? {
        if !d.is_visible(e) {
            continue;
        }
        let inf = crate::select::is_infinite(e);
        if !inf && !crate::select::bounds_of(d, e, known).expand(aperture).intersects(&probe) {
            continue;
        }
        count += 1;
        if count > 2000 {
            break;
        }
        if osmode & mode::INS != 0 {
            match &e.kind {
                EntityKind::Text(t) => cands.push((mode::INS, t.insert.xy())),
                EntityKind::MText(t) => cands.push((mode::INS, t.insert.xy())),
                EntityKind::Insert(i) => cands.push((mode::INS, i.insert.xy())),
                _ => {}
            }
        }
        if osmode & mode::END != 0
            && let EntityKind::Solid(s) | EntityKind::Trace(s) = &e.kind
        {
            cands.extend(s.corners.iter().map(|c| (mode::END, c.xy())));
        }
        for p in e.kind.prims() {
            match &p {
                Prim::Seg(s) => {
                    if osmode & mode::END != 0 {
                        cands.push((mode::END, s.start()));
                        cands.push((mode::END, s.end()));
                    }
                    if osmode & mode::MID != 0 {
                        cands.push((mode::MID, s.mid()));
                    }
                    if let Segment::Arc { arc, .. } = s {
                        if osmode & mode::CEN != 0 {
                            cands.push((mode::CEN, arc.center));
                        }
                        if osmode & mode::QUA != 0 {
                            for k in 0..4 {
                                let a = k as f64 * PI / 2.0;
                                if arc.contains_angle(a) {
                                    cands.push((mode::QUA, Vec2::polar(arc.center, arc.radius, a)));
                                }
                            }
                        }
                    }
                }
                Prim::Circle(c) => {
                    if osmode & mode::CEN != 0 {
                        cands.push((mode::CEN, c.center));
                    }
                    if osmode & mode::QUA != 0 {
                        for k in 0..4 {
                            cands.push((mode::QUA, Vec2::polar(c.center, c.radius, k as f64 * PI / 2.0)));
                        }
                    }
                }
                Prim::Ellipse(el) => {
                    if osmode & mode::CEN != 0 {
                        cands.push((mode::CEN, el.center));
                    }
                    if osmode & mode::QUA != 0 {
                        for k in 0..4 {
                            cands.push((mode::QUA, el.at_param(k as f64 * PI / 2.0)));
                        }
                    }
                    if osmode & mode::END != 0 && !el.is_full() {
                        cands.push((mode::END, el.at_param(el.start)));
                        cands.push((mode::END, el.at_param(el.end)));
                    }
                }
                Prim::Spline(s) => {
                    if osmode & mode::END != 0 && !s.closed {
                        let (lo, hi) = s.domain();
                        cands.push((mode::END, s.eval(lo)));
                        cands.push((mode::END, s.eval(hi)));
                    }
                }
                Prim::Point(pt) => {
                    if osmode & mode::NOD != 0 && matches!(e.kind, EntityKind::Point(_)) {
                        cands.push((mode::NOD, *pt));
                    }
                }
                Prim::Infinite { base, .. } => {
                    if osmode & mode::END != 0 && matches!(e.kind, EntityKind::Ray(_)) {
                        cands.push((mode::END, *base));
                    }
                }
                Prim::Fill(_) => {}
            }
            near_prims.push(p);
        }
        if osmode & mode::GCEN != 0
            && let EntityKind::LwPolyline(pl) = &e.kind
            && pl.closed
        {
            let pts = cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: true }.tessellate(aperture / 10.0);
            if let Some(c) = centroid(&pts) {
                cands.push((mode::GCEN, c));
            }
        }
    }
    // Intersections between nearby primitives.
    if osmode & (mode::INT | mode::APP) != 0 {
        let segs: Vec<Vec<Segment>> = near_prims.iter().take(200).map(prim_segments).collect();
        for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                for a in segs.get(i).into_iter().flatten() {
                    for b in segs.get(j).into_iter().flatten() {
                        for x in intersect_ext(&fix_circle(a), &fix_circle(b), false) {
                            if x.dist(cursor) <= aperture {
                                cands.push((mode::INT, x));
                            }
                        }
                    }
                }
            }
        }
    }
    // Perpendicular / tangent from the base point.
    if let Some(bp) = base {
        for p in &near_prims {
            if osmode & mode::PER != 0 {
                match p {
                    Prim::Seg(Segment::Line(l)) => cands.push((mode::PER, l.project(bp))),
                    Prim::Seg(Segment::Arc { arc, .. }) => cands.push((mode::PER, Circle::new(arc.center, arc.radius).closest(bp))),
                    Prim::Circle(c) => cands.push((mode::PER, c.closest(bp))),
                    Prim::Infinite { base: b0, dir, .. } => cands.push((mode::PER, *b0 + *dir * (bp - *b0).dot(*dir))),
                    _ => {}
                }
            }
            if osmode & mode::TAN != 0 {
                let circ = match p {
                    Prim::Seg(Segment::Arc { arc, .. }) => Some(Circle::new(arc.center, arc.radius)),
                    Prim::Circle(c) => Some(*c),
                    _ => None,
                };
                if let Some(c) = circ {
                    cands.extend(c.tangent_points(bp).into_iter().map(|t| (mode::TAN, t)));
                }
            }
        }
    }
    let best = cands.iter().filter(|(_, p)| p.dist(cursor) <= aperture).min_by(|a, b| {
        // Prefer more specific modes when nearly equidistant.
        let da = a.1.dist(cursor) - if a.0 == mode::INT || a.0 == mode::END { aperture * 0.15 } else { 0.0 };
        let db = b.1.dist(cursor) - if b.0 == mode::INT || b.0 == mode::END { aperture * 0.15 } else { 0.0 };
        da.total_cmp(&db)
    });
    if let Some((m, p)) = best {
        return Some(SnapHit { point: *p, mode: *m, name: name_of(*m), deferred: None });
    }
    if base.is_none()
        && deferred
        && let Some(h) = deferred_hit(&near_prims, cursor, aperture, osmode)
    {
        return Some(h);
    }
    if osmode & mode::NEA != 0 {
        let mut nb: Option<(f64, Vec2)> = None;
        for p in &near_prims {
            for s in prim_segments(p) {
                let c = fix_circle(&s).closest(cursor);
                let dd = c.dist(cursor);
                if dd <= aperture && nb.is_none_or(|(bd, _)| dd < bd) {
                    nb = Some((dd, c));
                }
            }
            if let Prim::Circle(c) = p {
                let q = c.closest(cursor);
                if q.dist(cursor) <= aperture {
                    nb = Some((q.dist(cursor), q));
                }
            }
        }
        if let Some((_, p)) = nb {
            return Some(SnapHit { point: p, mode: mode::NEA, name: "Nearest", deferred: None });
        }
    }
    None
}

/// The nearest deferred tangent/perpendicular within `aperture` of `cursor`.
fn deferred_hit(prims: &[Prim], cursor: Vec2, aperture: f64, osmode: u32) -> Option<SnapHit> {
    let mut best: Option<(f64, SnapHit)> = None;
    for p in prims {
        let curve = match p {
            Prim::Circle(c) => SnapCurve::Circle { center: c.center, radius: c.radius },
            Prim::Seg(Segment::Arc { arc, .. }) => SnapCurve::Circle { center: arc.center, radius: arc.radius },
            Prim::Seg(Segment::Line(l)) => SnapCurve::Line { a: l.a, b: l.b },
            Prim::Infinite { base, dir, .. } => SnapCurve::Line { a: *base, b: *base + *dir },
            _ => continue,
        };
        let (m, name, at) = match curve {
            SnapCurve::Circle { center, radius } if osmode & mode::TAN != 0 => {
                (mode::TAN, "Deferred Tangent", Circle::new(center, radius).closest(cursor))
            }
            SnapCurve::Circle { center, radius } if osmode & mode::PER != 0 => {
                (mode::PER, "Deferred Perpendicular", Circle::new(center, radius).closest(cursor))
            }
            SnapCurve::Line { a, b } if osmode & mode::PER != 0 => {
                // Pick on the drawn extent: a segment's own span, an xline/ray anywhere.
                let l = cadcraft_geom::Line::new(a, b);
                (mode::PER, "Deferred Perpendicular", if matches!(p, Prim::Seg(_)) { l.closest(cursor) } else { l.project(cursor) })
            }
            _ => continue,
        };
        let dd = at.dist(cursor);
        if at.is_finite() && dd <= aperture && best.is_none_or(|(bd, _)| dd < bd) {
            best = Some((dd, SnapHit { point: at, mode: m, name, deferred: Some(Deferred { mode: m, curve, at }) }));
        }
    }
    best.map(|(_, h)| h)
}

/// Solve a line whose ends may be deferred tangent/perpendicular snaps: returns its two
/// endpoints (in the order given), or `None` when there is no such line (a point inside the
/// circle it should be tangent to, concentric circles, non-parallel lines both perpendicular to
/// it, degenerate or non-finite geometry). Among several solutions, the one whose deferred
/// points lie closest to where the curves were picked wins (as AutoCAD does).
pub fn resolve_deferred(a: LineEnd, b: LineEnd) -> Option<(Vec2, Vec2)> {
    match (a, b) {
        (LineEnd::Point(p), LineEnd::Point(q)) => (p.is_finite() && q.is_finite()).then_some((p, q)),
        (LineEnd::Point(_), LineEnd::Deferred(_)) => resolve_deferred(b, a).map(|(q, p)| (p, q)),
        (LineEnd::Deferred(d), LineEnd::Point(p)) => {
            if !p.is_finite() {
                return None;
            }
            let cands: Vec<Vec2> = match (d.mode, d.curve) {
                (mode::TAN, SnapCurve::Circle { center, radius }) => Circle::new(center, radius).tangent_points(p),
                (mode::PER, SnapCurve::Circle { center, radius }) => {
                    let u = (p - center).normalized();
                    if u == Vec2::ZERO {
                        // Every radius is perpendicular: keep the picked point.
                        vec![Circle::new(center, radius).closest(d.at)]
                    } else {
                        vec![center + u * radius, center - u * radius]
                    }
                }
                (mode::PER, SnapCurve::Line { a, b }) => line_dir(a, b).map(|_| vec![cadcraft_geom::Line::new(a, b).project(p)]).unwrap_or_default(),
                _ => Vec::new(),
            };
            pick_best(cands.into_iter().map(|t| (t, p)), d.at, None)
        }
        (LineEnd::Deferred(d1), LineEnd::Deferred(d2)) => {
            let pairs = deferred_pairs(&d1, &d2).or_else(|| deferred_pairs(&d2, &d1).map(|v| v.into_iter().map(|(q, p)| (p, q)).collect()))?;
            pick_best(pairs.into_iter(), d1.at, Some(d2.at))
        }
    }
}

/// Candidate lines between two deferred snaps (`None` when this order isn't handled; the
/// caller retries swapped).
fn deferred_pairs(d1: &Deferred, d2: &Deferred) -> Option<Vec<(Vec2, Vec2)>> {
    use SnapCurve::{Circle as C, Line as L};
    Some(match ((d1.mode, d1.curve), (d2.mode, d2.curve)) {
        ((mode::TAN, C { center: c1, radius: r1 }), (mode::TAN, C { center: c2, radius: r2 })) => {
            Circle::new(c1, r1).common_tangents(&Circle::new(c2, r2))
        }
        // Perpendicular to a circle = through its center.
        ((mode::TAN, C { center, radius }), (mode::PER, C { center: k, radius: rk })) => Circle::new(center, radius)
            .tangent_points(k)
            .into_iter()
            .flat_map(|t| {
                let u = (t - k).normalized();
                [(t, k + u * rk), (t, k - u * rk)]
            })
            .collect(),
        // Perpendicular to a line: the tangent runs along the line's normal.
        ((mode::TAN, C { center, radius }), (mode::PER, L { a, b })) => {
            let Some(v) = line_dir(a, b) else { return Some(Vec::new()) };
            let l = cadcraft_geom::Line::new(a, b);
            [center + v * radius, center - v * radius].into_iter().map(|t| (t, l.project(t))).collect()
        }
        ((mode::PER, C { center: k1, radius: r1 }), (mode::PER, C { center: k2, radius: r2 })) => {
            let u = (k2 - k1).normalized();
            if u == Vec2::ZERO {
                return Some(Vec::new());
            }
            let mut v = Vec::new();
            for s1 in [1.0, -1.0] {
                for s2 in [1.0, -1.0] {
                    v.push((k1 + u * (r1 * s1), k2 + u * (r2 * s2)));
                }
            }
            v
        }
        ((mode::PER, C { center: k, radius: rk }), (mode::PER, L { a, b })) => {
            let Some(v) = line_dir(a, b) else { return Some(Vec::new()) };
            let f = cadcraft_geom::Line::new(a, b).project(k);
            let u = if f.dist(k) > cadcraft_geom::EPS { (f - k).normalized() } else { v.perp() };
            vec![(k + u * rk, f), (k - u * rk, f)]
        }
        ((mode::PER, L { a: a1, b: b1 }), (mode::PER, L { a: a2, b: b2 })) => {
            // Only parallel lines share a perpendicular; it passes through the first pick.
            let (Some(v1), Some(v2)) = (line_dir(a1, b1), line_dir(a2, b2)) else { return Some(Vec::new()) };
            if v1.cross(v2).abs() > 1e-9 {
                return Some(Vec::new());
            }
            let p = cadcraft_geom::Line::new(a1, b1).project(d1.at);
            vec![(p, cadcraft_geom::Line::new(a2, b2).project(p))]
        }
        _ => return None,
    })
}

/// Unit direction of the line through `a` and `b`, if it has one.
fn line_dir(a: Vec2, b: Vec2) -> Option<Vec2> {
    let v = (b - a).normalized();
    (v != Vec2::ZERO && v.is_finite()).then_some(v)
}

/// The finite, non-degenerate candidate closest to the picks.
fn pick_best(cands: impl Iterator<Item = (Vec2, Vec2)>, at1: Vec2, at2: Option<Vec2>) -> Option<(Vec2, Vec2)> {
    cands
        .filter(|(p, q)| p.is_finite() && q.is_finite() && p.dist(*q) > cadcraft_geom::EPS)
        .map(|(p, q)| (p.dist(at1) + at2.map_or(0.0, |a| q.dist(a)), (p, q)))
        .filter(|(cost, _)| cost.is_finite())
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, l)| l)
}

/// Full circles are stored as zero-sweep arcs in `prim_segments`; give them a full sweep.
fn fix_circle(s: &Segment) -> Segment {
    match *s {
        Segment::Arc { arc, ccw } if (arc.start - arc.end).abs() < 1e-15 => {
            Segment::Arc { arc: cadcraft_geom::Arc { start: 0.0, end: cadcraft_geom::TAU - 1e-12, ..arc }, ccw }
        }
        other => other,
    }
}

fn centroid(pts: &[Vec2]) -> Option<Vec2> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let mut a = 0.0;
    let mut c = Vec2::ZERO;
    for i in 0..n {
        let (p, q) = (pts.get(i)?, pts.get((i + 1) % n)?);
        let cr = p.cross(*q);
        a += cr;
        c += (*p + *q) * cr;
    }
    if a.abs() < 1e-12 { None } else { Some(c / (3.0 * a)) }
}

/// Constrain `p` relative to `base` to the nearest multiple of 90° (ortho).
pub fn ortho(base: Vec2, p: Vec2) -> Vec2 {
    let d = p - base;
    if d.x.abs() >= d.y.abs() { Vec2::new(p.x, base.y) } else { Vec2::new(base.x, p.y) }
}

/// Polar tracking: if the direction base→p is within `tol` radians of a multiple of `inc`,
/// return the point projected onto that ray and the angle.
pub fn polar(base: Vec2, p: Vec2, inc: f64, tol: f64) -> Option<(Vec2, f64)> {
    if inc <= 1e-9 {
        return None;
    }
    let d = p - base;
    let len = d.len();
    if len < 1e-12 {
        return None;
    }
    let a = d.angle();
    let k = (a / inc).round();
    let snapped = k * inc;
    if (a - snapped).abs() <= tol {
        let dir = Vec2::from_angle(snapped);
        Some((base + dir * d.dot(dir), cadcraft_geom::norm_angle(snapped)))
    } else {
        None
    }
}

/// Grid snap.
pub fn grid_snap(p: Vec2, unit: Vec2, origin: Vec2) -> Vec2 {
    let sx = if unit.x > 1e-12 { ((p.x - origin.x) / unit.x).round() * unit.x + origin.x } else { p.x };
    let sy = if unit.y > 1e-12 { ((p.y - origin.y) / unit.y).round() * unit.y + origin.y } else { p.y };
    Vec2::new(sx, sy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, Line};
    use cadcraft_geom::Vec3;

    fn drawing() -> Drawing {
        let mut d = Drawing::new_imperial();
        let l = |a: (f64, f64), b: (f64, f64)| EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
        d.add(&Space::Model, Common::default(), l((0.0, 0.0), (10.0, 0.0))).unwrap();
        d.add(&Space::Model, Common::default(), l((5.0, -5.0), (5.0, 5.0))).unwrap();
        d.add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0 })).unwrap();
        d
    }

    #[test]
    fn endpoint_and_mid() {
        let d = drawing();
        let h = osnap(&d, &Space::Model, Vec2::new(9.8, 0.1), 0.5, mode::END | mode::MID, None, false).unwrap();
        assert_eq!(h.point, Vec2::new(10.0, 0.0));
        let m = osnap(&d, &Space::Model, Vec2::new(5.1, 0.1), 0.5, mode::MID, None, false).unwrap();
        assert_eq!(m.point, Vec2::new(5.0, 0.0));
    }

    #[test]
    fn intersection() {
        let d = drawing();
        let h = osnap(&d, &Space::Model, Vec2::new(5.2, 0.2), 0.5, mode::INT, None, false).unwrap();
        assert!(h.point.near(Vec2::new(5.0, 0.0), 1e-9));
        assert_eq!(h.name, "Intersection");
    }

    #[test]
    fn center_quadrant_tangent() {
        let d = drawing();
        assert_eq!(osnap(&d, &Space::Model, Vec2::new(20.1, 0.1), 0.5, mode::CEN, None, false).unwrap().point, Vec2::new(20.0, 0.0));
        assert!(osnap(&d, &Space::Model, Vec2::new(20.0, 2.1), 0.5, mode::QUA, None, false).unwrap().point.near(Vec2::new(20.0, 2.0), 1e-9));
        let t = osnap(&d, &Space::Model, Vec2::new(19.9, 1.9), 1.0, mode::TAN, Some(Vec2::new(10.0, 0.0)), false);
        assert!(t.is_some());
    }

    #[test]
    fn perpendicular_from_base() {
        let d = drawing();
        let h = osnap(&d, &Space::Model, Vec2::new(3.0, 0.2), 0.5, mode::PER, Some(Vec2::new(3.0, 4.0)), false).unwrap();
        assert!(h.point.near(Vec2::new(3.0, 0.0), 1e-9));
    }

    #[test]
    fn ortho_and_polar() {
        assert_eq!(ortho(Vec2::ZERO, Vec2::new(5.0, 1.0)), Vec2::new(5.0, 0.0));
        assert_eq!(ortho(Vec2::ZERO, Vec2::new(1.0, 5.0)), Vec2::new(0.0, 5.0));
        let (p, a) = polar(Vec2::ZERO, Vec2::new(5.0, 5.1), 45f64.to_radians(), 3f64.to_radians()).unwrap();
        assert!((a - PI / 4.0).abs() < 1e-12);
        assert!((p.x - p.y).abs() < 1e-9);
        assert_eq!(grid_snap(Vec2::new(0.74, 1.26), Vec2::new(0.5, 0.5), Vec2::ZERO), Vec2::new(0.5, 1.5));
    }

    #[test]
    fn off_bit_disables() {
        let d = drawing();
        assert!(osnap(&d, &Space::Model, Vec2::new(9.8, 0.1), 0.5, mode::END | mode::OFF, None, false).is_none());
    }

    /// Distance from `c` to the infinite line through `p` and `q`.
    fn line_dist(c: Vec2, p: Vec2, q: Vec2) -> f64 {
        ((q - p).cross(c - p) / p.dist(q)).abs()
    }

    fn tan(center: (f64, f64), radius: f64, at: (f64, f64)) -> LineEnd {
        let curve = SnapCurve::Circle { center: Vec2::new(center.0, center.1), radius };
        LineEnd::Deferred(Deferred { mode: mode::TAN, curve, at: Vec2::new(at.0, at.1) })
    }

    fn pt(x: f64, y: f64) -> LineEnd {
        LineEnd::Point(Vec2::new(x, y))
    }

    #[test]
    fn deferred_tangent_without_base() {
        let d = drawing();
        let at = Vec2::new(20.0, 2.1);
        // Without a base, tangent snaps only as deferred, and only where the prompt resolves it.
        assert!(osnap(&d, &Space::Model, at, 0.5, mode::TAN, None, false).is_none());
        let h = osnap(&d, &Space::Model, at, 0.5, mode::TAN, None, true).unwrap();
        assert_eq!((h.mode, h.name), (mode::TAN, "Deferred Tangent"));
        assert!(h.point.near(Vec2::new(20.0, 2.0), 1e-9));
        let df = h.deferred.unwrap();
        assert_eq!(df.curve, SnapCurve::Circle { center: Vec2::new(20.0, 0.0), radius: 2.0 });
        assert_eq!(h.input(), crate::Input::Deferred(df));
        // With a base it is an ordinary tangent.
        let t = osnap(&d, &Space::Model, Vec2::new(19.9, 1.9), 1.0, mode::TAN, Some(Vec2::new(10.0, 0.0)), true).unwrap();
        assert!(t.deferred.is_none() && t.name == "Tangent");
        // A specific snap still wins over a deferred one.
        let q = osnap(&d, &Space::Model, at, 0.5, mode::TAN | mode::QUA, None, true).unwrap();
        assert_eq!(q.mode, mode::QUA);
        // Deferred perpendicular on a line.
        let p = osnap(&d, &Space::Model, Vec2::new(2.0, 0.2), 0.5, mode::PER, None, true).unwrap();
        assert_eq!(p.name, "Deferred Perpendicular");
        assert!(p.point.near(Vec2::new(2.0, 0.0), 1e-9));
    }

    #[test]
    fn resolve_tangent_to_free_point() {
        let (a, b) = resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), pt(20.0, 5.0)).unwrap();
        assert!(b.near(Vec2::new(20.0, 5.0), 1e-12));
        assert!((a.len() - 5.0).abs() < 1e-9 && (line_dist(Vec2::ZERO, a, b) - 5.0).abs() < 1e-9);
        assert!(a.y > 0.0, "the tangent point nearest the pick wins");
        let (a2, _) = resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, -5.0)), pt(20.0, 5.0)).unwrap();
        assert!(a2.y < 0.0);
        // Order of the ends is kept.
        let (p, q) = resolve_deferred(pt(20.0, 5.0), tan((0.0, 0.0), 5.0, (0.0, 5.0))).unwrap();
        assert!(p.near(Vec2::new(20.0, 5.0), 1e-12) && q.near(a, 1e-12));
    }

    #[test]
    fn resolve_belt_tangent_between_circles() {
        let (c1, r1, c2, r2) = (Vec2::ZERO, 10.0, Vec2::new(30.0, 0.0), 4.0);
        // Both picked on top: the outer (belt) tangent above.
        let (a, b) = resolve_deferred(tan((0.0, 0.0), r1, (0.0, 10.0)), tan((30.0, 0.0), r2, (30.0, 4.0))).unwrap();
        assert!((line_dist(c1, a, b) - r1).abs() < 1e-9 && (line_dist(c2, a, b) - r2).abs() < 1e-9);
        assert!((a.dist(c1) - r1).abs() < 1e-9 && (b.dist(c2) - r2).abs() < 1e-9);
        assert!(a.y > 0.0 && b.y > 0.0);
        // Top of one, bottom of the other: the crossed (inner) tangent.
        let (a, b) = resolve_deferred(tan((0.0, 0.0), r1, (0.0, 10.0)), tan((30.0, 0.0), r2, (30.0, -4.0))).unwrap();
        assert!((line_dist(c1, a, b) - r1).abs() < 1e-9 && (line_dist(c2, a, b) - r2).abs() < 1e-9);
        assert!(a.y > 0.0 && b.y < 0.0);
    }

    #[test]
    fn resolve_perpendicular_cases() {
        let per_line = |a: (f64, f64), b: (f64, f64), at: (f64, f64)| {
            let curve = SnapCurve::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
            LineEnd::Deferred(Deferred { mode: mode::PER, curve, at: Vec2::new(at.0, at.1) })
        };
        // Perpendicular from a free point onto a line (its extension counts).
        let (a, b) = resolve_deferred(per_line((0.0, 0.0), (10.0, 0.0), (5.0, 0.0)), pt(15.0, 7.0)).unwrap();
        assert!(a.near(Vec2::new(15.0, 0.0), 1e-9) && b.near(Vec2::new(15.0, 7.0), 1e-12));
        // Tangent to a circle and perpendicular to a line: runs along the line's normal.
        let (a, b) = resolve_deferred(tan((0.0, 10.0), 2.0, (2.0, 10.0)), per_line((0.0, 0.0), (10.0, 0.0), (2.0, 0.0))).unwrap();
        assert!(a.near(Vec2::new(2.0, 10.0), 1e-9) && b.near(Vec2::new(2.0, 0.0), 1e-9));
        // Non-parallel lines have no common perpendicular.
        assert!(resolve_deferred(per_line((0.0, 0.0), (10.0, 0.0), (1.0, 0.0)), per_line((0.0, 0.0), (0.0, 10.0), (0.0, 1.0))).is_none());
    }

    #[test]
    fn resolve_degenerate_input_is_none_not_panic() {
        // Point inside (or on) the circle: no tangent.
        assert!(resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), pt(1.0, 1.0)).is_none());
        assert!(resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), pt(5.0, 0.0)).is_none());
        // Concentric or nested circles: no common tangent.
        assert!(resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), tan((0.0, 0.0), 2.0, (0.0, 2.0))).is_none());
        assert!(resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), tan((1.0, 0.0), 2.0, (1.0, 2.0))).is_none());
        // Same circle twice.
        assert!(resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), tan((0.0, 0.0), 5.0, (0.0, -5.0))).is_none());
        // Zero radius: degenerates to a line through the point, without panicking.
        let _ = resolve_deferred(tan((0.0, 0.0), 0.0, (0.0, 0.0)), pt(10.0, 0.0));
        let _ = resolve_deferred(tan((0.0, 0.0), 0.0, (0.0, 0.0)), tan((10.0, 0.0), 0.0, (10.0, 0.0)));
        // Non-finite and huge numbers.
        for v in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e308, -1e308] {
            let _ = resolve_deferred(tan((v, 0.0), 5.0, (0.0, 5.0)), pt(20.0, v));
            let _ = resolve_deferred(tan((0.0, 0.0), v, (v, 5.0)), tan((v, v), 2.0, (0.0, v)));
            let r = resolve_deferred(tan((0.0, 0.0), 5.0, (0.0, 5.0)), pt(v, 0.0));
            assert!(r.is_none_or(|(a, b)| a.is_finite() && b.is_finite()));
        }
        // A degenerate line can't be perpendicular to anything.
        let curve = SnapCurve::Line { a: Vec2::ZERO, b: Vec2::ZERO };
        assert!(resolve_deferred(LineEnd::Deferred(Deferred { mode: mode::PER, curve, at: Vec2::ZERO }), pt(3.0, 3.0)).is_none());
        // Tangent to a line is meaningless.
        let curve = SnapCurve::Line { a: Vec2::ZERO, b: Vec2::X };
        assert!(resolve_deferred(LineEnd::Deferred(Deferred { mode: mode::TAN, curve, at: Vec2::ZERO }), pt(3.0, 3.0)).is_none());
    }
}
