//! Object snaps (OSNAP), polar tracking and ortho.

mod curves;
pub mod iso;
pub mod tracking;
mod viewport;

pub use viewport::ViewportPoint;

use cadcraft_doc::{Drawing, EntityKind, Insert, MAX_BLOCK_DEPTH, Prim, Space};
use cadcraft_geom::{Bounds2, Circle, EPS, Ellipse, Mat3, PI, Segment, Vec2, intersect_ext};
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
    /// Not an OSMODE bit: snaps also find hatch objects (OSNAPHATCH = 1).
    pub const HATCH: u32 = 1 << 16;
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
    /// Model coordinates when this paper-space hit is inside a layout viewport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ViewportPoint>,
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
    /// A circle, or the circle an arc lies on; an arc's sweep is carried by [`SnapCurve::Arc`].
    Circle { center: Vec2, radius: f64 },
    /// An arc: the points of `Circle { center, radius }` swept CCW from `start` to `end` (radians).
    Arc { center: Vec2, radius: f64, start: f64, end: f64 },
    /// The infinite line through `a` and `b` (a line, xline or ray).
    Line { a: Vec2, b: Vec2 },
}

impl SnapCurve {
    /// Whether `p` lies on the drawn curve: always on a circle or a line, and only within the
    /// sweep on an arc. Tangent and perpendicular solutions on an arc's circle that fall off the
    /// arc are filtered out with this, so they never snap into the gap.
    pub fn contains(&self, p: Vec2) -> bool {
        match self {
            // A zero sweep is how a full circle is stored (see `fix_circle`): the whole circle.
            SnapCurve::Arc { center, radius, start, end } => {
                (*start - *end).abs() < 1e-12 || cadcraft_geom::Arc::new(*center, *radius, *start, *end).contains_angle(center.angle_to(p))
            }
            _ => true,
        }
    }
    /// Points on the curve whose tangent passes through `p` (a circle's or an arc's).
    fn tangents(&self, p: Vec2) -> Vec<Vec2> {
        match self {
            SnapCurve::Circle { center, radius } | SnapCurve::Arc { center, radius, .. } => {
                Circle::new(*center, *radius).tangent_points(p).into_iter().filter(|t| self.contains(*t)).collect()
            }
            SnapCurve::Line { .. } => Vec::new(),
        }
    }
    /// Points on the curve whose perpendicular passes through `p`; `picked` is where the curve
    /// was picked, kept when `p` is the center of a circle or arc (every radius is perpendicular).
    fn perpendiculars(&self, p: Vec2, picked: Vec2) -> Vec<Vec2> {
        match self {
            SnapCurve::Circle { center, radius } | SnapCurve::Arc { center, radius, .. } => {
                let u = (p - *center).normalized();
                if u == Vec2::ZERO {
                    vec![Circle::new(*center, *radius).closest(picked)]
                } else {
                    [*center + u * *radius, *center - u * *radius].into_iter().filter(|q| self.contains(*q)).collect()
                }
            }
            SnapCurve::Line { a, b } => line_dir(*a, *b).map(|_| vec![cadcraft_geom::Line::new(*a, *b).project(p)]).unwrap_or_default(),
        }
    }
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

/// The point on `curve` whose tangent has direction `dir`, picking the solution
/// nearest `hint`. Used to resolve a deferred TAN first point when the next point
/// is typed as `@distance<angle`.
pub fn tangent_point_in_direction(curve: &SnapCurve, dir: f64, hint: Vec2) -> Option<Vec2> {
    match curve {
        SnapCurve::Circle { center, radius } => {
            let n = dir + std::f64::consts::FRAC_PI_2;
            let c1 = *center + Vec2::new(n.cos(), n.sin()) * *radius;
            let c2 = *center - Vec2::new(n.cos(), n.sin()) * *radius;
            Some(if c1.dist(hint) < c2.dist(hint) { c1 } else { c2 })
        }
        SnapCurve::Arc { center, radius, start, end } => {
            let n = dir + std::f64::consts::FRAC_PI_2;
            let c1 = *center + Vec2::new(n.cos(), n.sin()) * *radius;
            let c2 = *center - Vec2::new(n.cos(), n.sin()) * *radius;
            let on = cadcraft_geom::Arc::new(*center, *radius, *start, *end);
            let ok = |c: Vec2| on.contains_angle(center.angle_to(c));
            let (c1, c2) = (ok(c1).then_some(c1), ok(c2).then_some(c2));
            match (c1, c2) {
                (Some(a), Some(b)) => Some(if a.dist(hint) < b.dist(hint) { a } else { b }),
                (a, b) => a.or(b),
            }
        }
        _ => None,
    }
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
    osnap_with_neighborhood(d, space, ix, cursor, aperture, osmode, base, deferred).0
}

/// Find the best object snap and retain the local-space primitives gathered for that query.
#[allow(clippy::too_many_arguments)]
pub(crate) fn osnap_with_neighborhood(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
    deferred: bool,
) -> (Option<SnapHit>, Option<Vec<Prim>>) {
    if osmode == 0 || osmode & mode::OFF != 0 || !cursor.is_finite() || !(aperture.is_finite() && aperture > 0.0) {
        return (None, None);
    }
    let (mut best, near_prims) = match osnap_in_with_prims(d, space, ix, cursor, aperture, osmode, base, deferred, &[], None) {
        Some((hit, prims)) => (hit, Some(prims)),
        None => (None, None),
    };
    let views = viewport::near(d, space, cursor, aperture);
    if views.is_empty() {
        return (best, near_prims);
    }
    let model = Space::Model;
    let model_ix = crate::spatial::index(d, &model);
    for view in views {
        let model_cursor = view.to_model(cursor);
        let model_aperture = aperture / view.scale;
        let model_base = base.map(|p| view.to_model(p));
        if !model_cursor.is_finite() || !(model_aperture.is_finite() && model_aperture > 0.0) || model_base.is_some_and(|p| !p.is_finite()) {
            continue;
        }
        let hit = osnap_in(
            d,
            &model,
            &model_ix,
            model_cursor,
            model_aperture,
            osmode,
            model_base,
            deferred,
            &view.viewport.frozen_layers,
            Some(view.model_clip),
        )
        .and_then(|h| view.hit(h));
        if let Some(hit) = hit
            && best.as_ref().is_none_or(|b| snap_score(hit.mode, hit.point, cursor, aperture) < snap_score(b.mode, b.point, cursor, aperture))
        {
            best = Some(hit);
        }
    }
    (best, near_prims)
}

#[allow(clippy::too_many_arguments)]
fn osnap_in(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
    deferred: bool,
    frozen: &[String],
    clip: Option<Bounds2>,
) -> Option<SnapHit> {
    osnap_in_with_prims(d, space, ix, cursor, aperture, osmode, base, deferred, frozen, clip).and_then(|(hit, _)| hit)
}

#[allow(clippy::too_many_arguments)]
fn osnap_in_with_prims(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
    deferred: bool,
    frozen: &[String],
    clip: Option<Bounds2>,
) -> Option<(Option<SnapHit>, Vec<Prim>)> {
    let (mut cands, near_prims) = gather_in(d, space, ix, cursor, aperture, osmode, frozen)?;
    // Intersections between nearby primitives; Apparent Intersection also where their extensions
    // meet (lines extended, arcs as full circles: in 2D, the "apparent" crossing of objects
    // that don't reach each other).
    if osmode & (mode::INT | mode::APP) != 0 {
        let mut segs: Vec<Vec<Segment>> = near_prims.iter().take(200).map(prim_segments).collect();
        let probe = finite_intersection_probe(cursor, aperture);
        for (i, segments) in segs.iter_mut().enumerate() {
            let preserves_app = osmode & mode::APP != 0 && near_prims.get(i).is_some_and(extendable);
            if !preserves_app {
                segments.retain(|segment| segment_may_intersect_probe(segment, probe.as_ref()));
            }
        }
        let real = if osmode & mode::INT != 0 { mode::INT } else { mode::APP };
        for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                // Extending a tessellated ellipse or spline chord means nothing.
                let extend = osmode & mode::APP != 0 && [i, j].iter().all(|k| near_prims.get(*k).is_some_and(extendable));
                for a in segs.get(i).into_iter().flatten() {
                    for b in segs.get(j).into_iter().flatten() {
                        for x in intersect_ext(&fix_circle(a), &fix_circle(b), false) {
                            if x.dist(cursor) <= aperture {
                                cands.push((real, x));
                            }
                        }
                        if extend {
                            for x in intersect_ext(a, b, true) {
                                if x.is_finite() && x.dist(cursor) <= aperture {
                                    cands.push((mode::APP, x));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let hit = finish(cands, &near_prims, cursor, aperture, osmode, base, deferred, clip);
    Some((hit, near_prims))
}

fn finite_intersection_probe(cursor: Vec2, aperture: f64) -> Option<Bounds2> {
    if !(cursor.is_finite() && aperture.is_finite() && aperture > 0.0) {
        return None;
    }
    let probe = Bounds2::new(cursor, cursor).expand(aperture);
    if !finite_bounds(&probe) {
        return None;
    }
    // The finite line intersection accepts a 1e-9 parameter slack; retain that slack in the broad phase.
    let scale = cursor.x.abs().max(cursor.y.abs()).max(aperture).max(1.0);
    let padded = probe.expand(EPS * scale);
    finite_bounds(&padded).then_some(padded)
}

fn segment_may_intersect_probe(segment: &Segment, probe: Option<&Bounds2>) -> bool {
    let Some(probe) = probe else { return true };
    let bounds = segment.bounds();
    if !finite_bounds(&bounds) {
        return true;
    }
    let width = bounds.width();
    let height = bounds.height();
    if !(width.is_finite() && height.is_finite()) {
        return true;
    }
    let scale = bounds.min.x.abs().max(bounds.min.y.abs()).max(bounds.max.x.abs()).max(bounds.max.y.abs()).max(width).max(height).max(1.0);
    let padded = bounds.expand(EPS * scale);
    !finite_bounds(&padded) || padded.intersects(probe)
}

fn finite_bounds(bounds: &Bounds2) -> bool {
    !bounds.is_empty() && bounds.min.is_finite() && bounds.max.is_finite()
}

/// Whether a primitive has a meaningful extension (a line, an arc, a circle, an xline or ray).
pub(crate) fn extendable(p: &Prim) -> bool {
    matches!(p, Prim::Seg(_) | Prim::Circle(_) | Prim::Infinite { .. })
}

/// The snap candidates of `osmode` and the primitives within `aperture` of `cursor`; `None`
/// when the drawing can't be searched.
pub(crate) fn gather(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
) -> Option<(Vec<(u32, Vec2)>, Vec<Prim>)> {
    gather_in(d, space, ix, cursor, aperture, osmode, &[])
}

fn gather_in(
    d: &Drawing,
    space: &Space,
    ix: &Option<std::sync::Arc<crate::spatial::SpatialIndex>>,
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    frozen: &[String],
) -> Option<(Vec<(u32, Vec2)>, Vec<Prim>)> {
    let probe = Bounds2::new(cursor, cursor).expand(aperture);
    let mut cands: Vec<(u32, Vec2)> = Vec::new();
    let mut near_prims: Vec<Prim> = Vec::new();
    let mut count = 0usize;
    let mut budget = MAX_BLOCK_LEAVES;
    for (e, known) in crate::select::candidates(d, space, ix, &probe.expand(aperture), true, false)? {
        if !d.is_visible(e) || frozen.iter().any(|l| l.eq_ignore_ascii_case(&e.common.layer)) {
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
                EntityKind::Text(t) => cands.push((mode::INS, t.justify_point())),
                EntityKind::MText(t) => cands.push((mode::INS, t.insert.xy())),
                EntityKind::Tolerance(t) => cands.push((mode::INS, t.insert.xy())),
                EntityKind::Insert(i) => {
                    cands.push((mode::INS, i.insert.xy()));
                    cands.extend(i.attribs.iter().filter(|a| !a.invisible).map(|a| (mode::INS, a.text.justify_point())));
                }
                EntityKind::AttDef(a) => cands.push((mode::INS, a.text.justify_point())),
                // A multileader: its text's insertion point, or its block content's.
                EntityKind::MLeader(m) => {
                    cands.extend(m.text.iter().map(|t| (mode::INS, t.insert.xy())));
                    cands.extend(m.block.iter().map(|b| (mode::INS, b.insert.xy())));
                }
                _ => {}
            }
        }
        let mut sink = Sink { osmode, aperture, probe, cands: &mut cands, near: &mut near_prims };
        sink.object(&e.kind, None);
        // A block reference snaps to the objects inside it, placed by its transform.
        if let EntityKind::Insert(ins) = &e.kind {
            block_leaves(d, ins, &Mat3::IDENTITY, 0, &mut budget, frozen, &mut |k, m| sink.object(k, Some(m)));
        }
        if let EntityKind::Dimension(dm) = &e.kind {
            sink.dimension(d, dm, &mut budget, frozen);
        }
    }
    Some((cands, near_prims))
}

/// Picks the best of the gathered candidates, adding the base-dependent and deferred snaps.
#[allow(clippy::too_many_arguments)]
fn finish(
    mut cands: Vec<(u32, Vec2)>,
    near_prims: &[Prim],
    cursor: Vec2,
    aperture: f64,
    osmode: u32,
    base: Option<Vec2>,
    deferred: bool,
    clip: Option<Bounds2>,
) -> Option<SnapHit> {
    // Perpendicular / tangent from the base point.
    if let Some(bp) = base {
        for p in near_prims {
            if osmode & mode::PER != 0 {
                match p {
                    Prim::Seg(Segment::Line(l)) => cands.push((mode::PER, l.project(bp))),
                    // Both antipodes, filtered to the arc's sweep: a perpendicular in the gap is
                    // not offered, and the far side is still found.
                    Prim::Seg(Segment::Arc { arc, .. }) => {
                        let curve = SnapCurve::Arc { center: arc.center, radius: arc.radius, start: arc.start, end: arc.end };
                        cands.extend(curve.perpendiculars(bp, bp).into_iter().map(|q| (mode::PER, q)));
                    }
                    Prim::Circle(c) => cands.push((mode::PER, c.closest(bp))),
                    Prim::Infinite { base: b0, dir, .. } => cands.push((mode::PER, *b0 + *dir * (bp - *b0).dot(*dir))),
                    Prim::Ellipse(e) => cands.extend(curves::ellipse_perpendiculars(e, bp).into_iter().map(|q| (mode::PER, q))),
                    Prim::Spline(s) => cands.extend(curves::spline_perpendiculars(s, bp).into_iter().map(|q| (mode::PER, q))),
                    _ => {}
                }
            }
            if osmode & mode::TAN != 0 {
                let pts = match p {
                    // Only the tangent points that lie on the arc itself (not its circle).
                    Prim::Seg(Segment::Arc { arc, .. }) => Circle::new(arc.center, arc.radius)
                        .tangent_points(bp)
                        .into_iter()
                        .filter(|t| arc.contains_angle(arc.center.angle_to(*t)))
                        .collect(),
                    Prim::Circle(c) => c.tangent_points(bp),
                    Prim::Ellipse(e) => curves::ellipse_tangents(e, bp),
                    Prim::Spline(s) => curves::spline_tangents(s, bp),
                    _ => Vec::new(),
                };
                cands.extend(pts.into_iter().map(|t| (mode::TAN, t)));
            }
        }
    }
    let visible = |p: Vec2| p.is_finite() && clip.is_none_or(|c| c.contains(p));
    let best = cands.iter().filter(|(_, p)| visible(*p) && p.dist(cursor) <= aperture).min_by(|a, b| {
        // Prefer more specific modes when nearly equidistant.
        let da = snap_score(a.0, a.1, cursor, aperture);
        let db = snap_score(b.0, b.1, cursor, aperture);
        da.total_cmp(&db)
    });
    if let Some((m, p)) = best {
        return Some(SnapHit { point: *p, mode: *m, name: name_of(*m), viewport: None, deferred: None });
    }
    if base.is_none()
        && deferred
        && let Some(h) = deferred_hit(near_prims, cursor, aperture, osmode, clip)
    {
        return Some(h);
    }
    if osmode & mode::NEA != 0 {
        // The closest point of all nearby objects, whatever their drawing order.
        let mut nb: Option<(f64, Vec2)> = None;
        let mut consider = |c: Vec2| {
            let dd = c.dist(cursor);
            if visible(c) && dd <= aperture && nb.is_none_or(|(bd, _)| dd < bd) {
                nb = Some((dd, c));
            }
        };
        for p in near_prims {
            for s in prim_segments(p) {
                consider(fix_circle(&s).closest(cursor));
            }
            if let Prim::Circle(c) = p {
                consider(c.closest(cursor));
            }
        }
        if let Some((_, p)) = nb {
            return Some(SnapHit { point: p, mode: mode::NEA, name: "Nearest", viewport: None, deferred: None });
        }
    }
    None
}

fn snap_score(mode: u32, point: Vec2, cursor: Vec2, aperture: f64) -> f64 {
    point.dist(cursor) - if mode == mode::INT || mode == mode::END { aperture * 0.15 } else { 0.0 }
}

/// Upper bound on the block-reference objects one snap query looks at (hostile files).
const MAX_BLOCK_LEAVES: usize = 20_000;

/// Collects snap candidates and the primitives near the cursor.
struct Sink<'a> {
    osmode: u32,
    aperture: f64,
    probe: Bounds2,
    cands: &'a mut Vec<(u32, Vec2)>,
    near: &'a mut Vec<Prim>,
}

impl Sink<'_> {
    /// The snap points of one object; `xf` places an object from inside a block reference
    /// (whose primitives far from the cursor are skipped).
    fn object(&mut self, kind: &EntityKind, xf: Option<&Mat3>) {
        let osmode = self.osmode;
        // OSNAPHATCH = 0: hatches are left out.
        if osmode & mode::HATCH == 0 && matches!(kind, EntityKind::Hatch(_)) {
            return;
        }
        let at = |p: Vec2| xf.map_or(p, |m| m.apply(p));
        if osmode & mode::END != 0
            && let EntityKind::Solid(s) | EntityKind::Trace(s) = kind
        {
            self.cands.extend(s.corners.iter().map(|c| (mode::END, at(c.xy()))));
        }
        for p in kind.prims() {
            let p = match xf {
                Some(m) => {
                    let p = xform_prim(&p, m);
                    if !prim_near(&p, &self.probe, self.aperture) {
                        continue;
                    }
                    p
                }
                None => p,
            };
            self.primitive(kind, p);
        }
        if osmode & mode::GCEN != 0 {
            self.closed_centroid(kind, &at);
        }
    }

    fn primitive(&mut self, kind: &EntityKind, p: Prim) {
        let osmode = self.osmode;
        let cands = &mut *self.cands;
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
                    // Only the quadrant points the arc passes through.
                    for k in 0..4 {
                        let t = k as f64 * PI / 2.0;
                        if cadcraft_geom::angle_in_sweep(t, el.start, el.end) {
                            cands.push((mode::QUA, el.at_param(t)));
                        }
                    }
                }
                if osmode & mode::END != 0 && !el.is_full() {
                    cands.push((mode::END, el.at_param(el.start)));
                    cands.push((mode::END, el.at_param(el.end)));
                }
                if osmode & mode::MID != 0
                    && let Some(m) = curves::ellipse_mid(el)
                {
                    cands.push((mode::MID, m));
                }
            }
            Prim::Spline(s) => {
                if osmode & mode::END != 0 && !s.closed {
                    let (lo, hi) = s.domain();
                    cands.push((mode::END, s.eval(lo)));
                    cands.push((mode::END, s.eval(hi)));
                }
                if osmode & mode::MID != 0
                    && let Some(m) = curves::spline_mid(s)
                {
                    cands.push((mode::MID, m));
                }
            }
            Prim::Point(pt) => {
                if osmode & mode::NOD != 0 && matches!(kind, EntityKind::Point(_)) {
                    cands.push((mode::NOD, *pt));
                }
            }
            Prim::Infinite { base, .. } => {
                if osmode & mode::END != 0 && matches!(kind, EntityKind::Ray(_)) {
                    cands.push((mode::END, *base));
                }
            }
            Prim::Fill(_) => {}
        }
        self.near.push(p);
    }

    fn closed_centroid(&mut self, kind: &EntityKind, at: &impl Fn(Vec2) -> Vec2) {
        // The centroid of the area a closed polyline or spline encloses.
        let pts = match kind {
            EntityKind::LwPolyline(pl) if pl.closed => {
                cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: true }.tessellate(self.aperture / 10.0)
            }
            EntityKind::Spline(s) if s.closed => s.tessellate(1e-3),
            _ => Vec::new(),
        };
        if let Some(c) = centroid(&pts) {
            self.cands.push((mode::GCEN, at(c)));
        }
    }
}

impl Sink<'_> {
    /// A dimension snaps to what it draws, as in AutoCAD (into its block): the objects of its
    /// block when the drawing has one (read from a file), else the lines generated from it
    /// (Endpoint on their ends); Node on its definition points.
    fn dimension(&mut self, d: &Drawing, dm: &cadcraft_doc::Dimension, budget: &mut usize, frozen: &[String]) {
        match dm.block.as_ref().and_then(|n| d.block(n)) {
            Some(blk) => {
                // The block holds world geometry: a reference at its own base point places it as is.
                let ins = Insert {
                    block: blk.name.clone(),
                    insert: blk.base,
                    scale: cadcraft_geom::Vec3::new(1.0, 1.0, 1.0),
                    rotation: 0.0,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                };
                block_leaves(d, &ins, &Mat3::IDENTITY, 0, budget, frozen, &mut |k, m| self.object(k, Some(m)));
            }
            None => {
                let g = cadcraft_render::dimension_in(d, dm);
                for line in &g.lines {
                    let (Some(a), Some(b)) = (line.first(), line.last()) else { continue };
                    if self.osmode & mode::END != 0 {
                        self.cands.extend([(mode::END, *a), (mode::END, *b)]);
                    }
                    if self.osmode & mode::MID != 0 && line.len() == 2 {
                        self.cands.push((mode::MID, (*a + *b) * 0.5));
                    }
                    for w in line.windows(2) {
                        if let [p, q] = w {
                            let s = Prim::Seg(Segment::Line(cadcraft_geom::Line::new(*p, *q)));
                            if prim_near(&s, &self.probe, self.aperture) {
                                self.near.push(s);
                            }
                        }
                    }
                }
            }
        }
        if self.osmode & mode::NOD != 0 {
            self.cands.extend(dim_def_points(dm).into_iter().map(|p| (mode::NOD, p)));
        }
    }
}

/// The definition points of a dimension that its kind uses.
fn dim_def_points(dm: &cadcraft_doc::Dimension) -> Vec<Vec2> {
    use cadcraft_doc::DimKind;
    let pts = match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned | DimKind::ArcLength => vec![dm.defpt, dm.p13, dm.p14],
        DimKind::Radius | DimKind::Diameter => vec![dm.defpt, dm.p15],
        DimKind::Angular3P => vec![dm.defpt, dm.p13, dm.p14, dm.p15],
        DimKind::Angular => vec![dm.defpt, dm.p13, dm.p14, dm.p15, dm.p16],
        DimKind::Ordinate { .. } => vec![dm.p13, dm.p14],
    };
    pts.into_iter().map(|p| p.xy()).filter(|p| p.is_finite()).collect()
}

/// Calls `f` with each object drawn by a block reference and the transform placing it, nested
/// references expanded (within [`MAX_BLOCK_DEPTH`] levels and `budget` objects); objects that
/// aren't drawn (hidden, on a layer that is off or frozen, attribute definitions) are left out.
fn block_leaves(d: &Drawing, ins: &Insert, m: &Mat3, depth: usize, budget: &mut usize, frozen: &[String], f: &mut dyn FnMut(&EntityKind, &Mat3)) {
    if depth >= MAX_BLOCK_DEPTH {
        return;
    }
    let Some(blk) = d.block(&ins.block) else { return };
    if blk.entities.is_empty() {
        return;
    }
    let (cols, rows) = (ins.cols.clamp(1, 10_000), ins.rows.clamp(1, 10_000));
    for r in 0..rows {
        for c in 0..cols {
            // Each array cell spends budget too, so a huge array of tiny blocks stays bounded.
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let off = Vec2::new(ins.col_spacing * f64::from(c), ins.row_spacing * f64::from(r)).rotate(ins.rotation);
            let mm = m.then_before(Mat3::translate(off).then_before(ins.transform(blk.base.xy())));
            for be in blk.entities.iter() {
                if *budget == 0 {
                    return;
                }
                *budget -= 1;
                // Objects on layer 0 take the reference's layer, which is visible.
                if !be.common.visible
                    || (be.common.layer != "0" && (!d.is_visible(be) || frozen.iter().any(|l| l.eq_ignore_ascii_case(&be.common.layer))))
                {
                    continue;
                }
                match &be.kind {
                    EntityKind::AttDef(_) => {}
                    EntityKind::Insert(inner) => block_leaves(d, inner, &mm, depth + 1, budget, frozen, f),
                    k => f(k, &mm),
                }
            }
        }
    }
}

/// Whether `p` comes within `aperture` of the box `probe`.
fn prim_near(p: &Prim, probe: &Bounds2, aperture: f64) -> bool {
    let b = match p {
        Prim::Seg(s) => s.bounds(),
        Prim::Circle(c) => c.bounds(),
        Prim::Ellipse(e) => e.bounds(),
        Prim::Spline(s) => Bounds2::from_points(s.control.iter().copied()),
        Prim::Point(q) => Bounds2::new(*q, *q),
        Prim::Infinite { .. } => return true,
        Prim::Fill(f) => Bounds2::from_points(f.iter().copied()),
    };
    b.expand(aperture).intersects(probe)
}

/// The rotation-and-uniform-scale factor of `m`'s linear part (mirroring allowed), or `None`
/// when it distorts shapes (non-uniform scale or shear).
fn similarity_scale(m: &Mat3) -> Option<f64> {
    let (u, v) = (Vec2::new(m.a, m.b), Vec2::new(m.c, m.d));
    let (lu, lv) = (u.len(), v.len());
    let tol = 1e-9 * lu.max(lv);
    ((lu - lv).abs() <= tol && u.dot(v).abs() <= tol * lu.max(lv) && lu > 0.0).then_some(lu)
}

/// `p` placed by the affine transform `m`, exactly: circles and arcs under a non-uniform scale
/// become ellipses and elliptical arcs.
fn xform_prim(p: &Prim, m: &Mat3) -> Prim {
    let mirror = m.is_mirroring();
    match p {
        Prim::Seg(Segment::Line(l)) => Prim::Seg(Segment::Line(cadcraft_geom::Line::new(m.apply(l.a), m.apply(l.b)))),
        Prim::Seg(Segment::Arc { arc, ccw }) => match similarity_scale(m) {
            Some(s) => {
                let c = m.apply(arc.center);
                let (a0, a1) = (c.angle_to(m.apply(arc.start_point())), c.angle_to(m.apply(arc.end_point())));
                let (start, end) = if mirror { (a1, a0) } else { (a0, a1) };
                Prim::Seg(Segment::Arc { arc: cadcraft_geom::Arc::new(c, arc.radius * s, start, end), ccw: *ccw != mirror })
            }
            None => {
                let e = Ellipse { center: arc.center, major: Vec2::X * arc.radius, ratio: 1.0, start: arc.start, end: arc.end };
                Prim::Ellipse(xform_ellipse(&e, m))
            }
        },
        Prim::Circle(c) => match similarity_scale(m) {
            Some(s) => Prim::Circle(Circle::new(m.apply(c.center), c.radius * s)),
            None => Prim::Ellipse(xform_ellipse(&Ellipse::full(c.center, Vec2::X * c.radius, 1.0), m)),
        },
        Prim::Ellipse(e) => Prim::Ellipse(xform_ellipse(e, m)),
        Prim::Spline(s) => {
            let mut s = s.clone();
            s.control.iter_mut().chain(s.fit.iter_mut()).for_each(|q| *q = m.apply(*q));
            Prim::Spline(s)
        }
        Prim::Point(q) => Prim::Point(m.apply(*q)),
        Prim::Infinite { base, dir, ray } => Prim::Infinite { base: m.apply(*base), dir: m.apply_vec(*dir).normalized(), ray: *ray },
        Prim::Fill(f) => Prim::Fill(f.iter().map(|q| m.apply(*q)).collect()),
    }
}

/// The image of an ellipse (or elliptical arc) under an affine transform: the conjugate
/// semi-diameters `A·major`, `A·minor` give the new principal axes; parameters shift with them
/// (and reverse under a mirroring transform).
fn xform_ellipse(e: &Ellipse, m: &Mat3) -> Ellipse {
    let center = m.apply(e.center);
    let (am, an) = (m.apply_vec(e.major), m.apply_vec(e.minor()));
    // |am·cos t + an·sin t| is largest at t0: the new major axis.
    let t0 = 0.5 * (2.0 * am.dot(an)).atan2(am.dot(am) - an.dot(an));
    let (s0, c0) = t0.sin_cos();
    let (u, v) = (am * c0 + an * s0, an * c0 - am * s0);
    let ratio = if u.len() > 0.0 { (v.len() / u.len()).min(1.0) } else { 1.0 };
    if e.is_full() {
        return Ellipse::full(center, u, ratio);
    }
    let (start, end) = if u.cross(v) >= 0.0 { (e.start - t0, e.end - t0) } else { (t0 - e.end, t0 - e.start) };
    Ellipse { center, major: u, ratio, start: cadcraft_geom::norm_angle(start), end: cadcraft_geom::norm_angle(end) }
}

/// The nearest deferred tangent/perpendicular within `aperture` of `cursor`.
fn deferred_hit(prims: &[Prim], cursor: Vec2, aperture: f64, osmode: u32, clip: Option<Bounds2>) -> Option<SnapHit> {
    let mut best: Option<(f64, SnapHit)> = None;
    for p in prims {
        let curve = match p {
            Prim::Circle(c) => SnapCurve::Circle { center: c.center, radius: c.radius },
            Prim::Seg(Segment::Arc { arc, .. }) => SnapCurve::Arc { center: arc.center, radius: arc.radius, start: arc.start, end: arc.end },
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
            SnapCurve::Arc { center, radius, start, end } if osmode & mode::TAN != 0 => {
                (mode::TAN, "Deferred Tangent", cadcraft_geom::Arc::new(center, radius, start, end).closest(cursor))
            }
            SnapCurve::Arc { center, radius, start, end } if osmode & mode::PER != 0 => {
                (mode::PER, "Deferred Perpendicular", cadcraft_geom::Arc::new(center, radius, start, end).closest(cursor))
            }
            SnapCurve::Line { a, b } if osmode & mode::PER != 0 => {
                // Pick on the drawn extent: a segment's own span, an xline/ray anywhere.
                let l = cadcraft_geom::Line::new(a, b);
                (mode::PER, "Deferred Perpendicular", if matches!(p, Prim::Seg(_)) { l.closest(cursor) } else { l.project(cursor) })
            }
            _ => continue,
        };
        let dd = at.dist(cursor);
        if at.is_finite() && clip.is_none_or(|c| c.contains(at)) && dd <= aperture && best.is_none_or(|(bd, _)| dd < bd) {
            best = Some((dd, SnapHit { point: at, mode: m, name, viewport: None, deferred: Some(Deferred { mode: m, curve, at }) }));
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
            let cands: Vec<Vec2> = match d.mode {
                mode::TAN => d.curve.tangents(p),
                mode::PER => d.curve.perpendiculars(p, d.at),
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
    // The math below is on the circle an arc lies on; the sweep is checked afterwards.
    let circle_of = |c: SnapCurve| match c {
        SnapCurve::Circle { center, radius } | SnapCurve::Arc { center, radius, .. } => C { center, radius },
        other => other,
    };
    let pairs = match ((d1.mode, circle_of(d1.curve)), (d2.mode, circle_of(d2.curve))) {
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
    };
    // A solution on an arc's circle that falls off the arc is no solution: the line would touch
    // the circle, not the arc that was picked.
    Some(pairs.into_iter().filter(|(p, q)| d1.curve.contains(*p) && d2.curve.contains(*q)).collect())
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

/// Grid snap: the nearest point of the grid of `unit` spacing through `origin`, turned by
/// `angle` (radians) about it.
pub fn grid_snap(p: Vec2, unit: Vec2, origin: Vec2, angle: f64) -> Vec2 {
    let angle = if angle.is_finite() { angle } else { 0.0 };
    let q = (p - origin).rotate(-angle);
    let sx = if unit.x > 1e-12 { (q.x / unit.x).round() * unit.x } else { q.x };
    let sy = if unit.y > 1e-12 { (q.y / unit.y).round() * unit.y } else { q.y };
    let s = origin + Vec2::new(sx, sy).rotate(angle);
    if s.is_finite() { s } else { p }
}

/// The snap grid's origin (SNAPBASE) and rotation in radians (SNAPANG, in degrees) of a drawing.
pub fn grid_frame(d: &Drawing) -> (Vec2, f64) {
    let base = d.header.point("SNAPBASE").map(|p| p.xy()).filter(|p| p.is_finite()).unwrap_or(Vec2::ZERO);
    (base, d.header.f64("SNAPANG", 0.0).to_radians())
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
        assert_eq!(grid_snap(Vec2::new(0.74, 1.26), Vec2::new(0.5, 0.5), Vec2::ZERO, 0.0), Vec2::new(0.5, 1.5));
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

    /// A drawing with one arc: center (20,0), radius 2, swept CCW from 0° to 90° (the first
    /// quadrant only, so the rest of its circle is the gap).
    fn arc_drawing() -> Drawing {
        let mut d = Drawing::new_imperial();
        d.add(
            &Space::Model,
            Common::default(),
            EntityKind::Arc(cadcraft_doc::Arc { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0, start: 0.0, end: std::f64::consts::FRAC_PI_2 }),
        )
        .unwrap();
        d
    }

    fn on_arc(p: Vec2) -> bool {
        let a = Vec2::new(20.0, 0.0).angle_to(p);
        cadcraft_geom::angle_in_sweep(a, 0.0, std::f64::consts::FRAC_PI_2) && (p.dist(Vec2::new(20.0, 0.0)) - 2.0).abs() < 1e-9
    }

    #[test]
    fn arc_tangent_only_on_the_arc_not_its_circle() {
        let d = arc_drawing();
        // From (20,5), the circle's tangents touch at about ±23.6° from the vertical through the
        // center: one lands at 90+23.6 = 113.6° (the gap), the other at 90-23.6 = 66.4° (on the
        // arc). Only the one on the arc may snap.
        let base = Some(Vec2::new(20.0, 5.0));
        let t = osnap(&d, &Space::Model, Vec2::new(20.0 + 2.0, 0.0), 8.0, mode::TAN, base, false).expect("the arc's tangent");
        assert!(on_arc(t.point), "tangent must lie on the arc, got {:?}", t.point);
        // The other solution is on the circle but off the arc, so it is not offered.
        let off = Vec2::polar(Vec2::new(20.0, 0.0), 2.0, 113.6f64.to_radians());
        assert!(!on_arc(off));
        let none = osnap(&d, &Space::Model, off, 0.05, mode::TAN, base, false);
        assert!(none.is_none(), "a tangent in the arc's gap must not snap, got {none:?}");
    }

    #[test]
    fn arc_perpendicular_clamps_to_the_arc() {
        let d = arc_drawing();
        // The foot of the perpendicular from below lands at 270°, in the gap. The opposite
        // antipode at (20,2) is on the arc. The cursor is 5 units away from it, so the aperture
        // is a little larger to keep the distance test off the boundary.
        let h = osnap(&d, &Space::Model, Vec2::new(20.0, -3.0), 5.5, mode::PER, Some(Vec2::new(20.0, -6.0)), false)
            .expect("the perpendicular on the arc");
        assert!(on_arc(h.point), "perpendicular must stay on the arc, got {:?}", h.point);
        assert!(h.point.near(Vec2::new(20.0, 2.0), 1e-9), "the opposite antipode on the arc, got {:?}", h.point);
    }

    #[test]
    fn arc_deferred_tangent_keeps_its_sweep() {
        let d = arc_drawing();
        let at = Vec2::polar(Vec2::new(20.0, 0.0), 2.0, 45f64.to_radians());
        let h = osnap(&d, &Space::Model, at, 0.5, mode::TAN, None, true).expect("deferred tangent on the arc");
        assert_eq!((h.mode, h.name), (mode::TAN, "Deferred Tangent"));
        let df = h.deferred.unwrap();
        // The sweep travels with the curve, so a later solve stays on the arc.
        assert!(matches!(df.curve, SnapCurve::Arc { .. }), "the arc's sweep must be kept: {:?}", df.curve);
        // A deferred tangent solves to a point on the arc, not into its gap.
        let solved = resolve_deferred(LineEnd::Deferred(df), pt(10.0, 10.0)).expect("a tangent from (10,10)");
        // "In the order given": the deferred end solves first.
        assert!(on_arc(solved.0), "resolved tangent must lie on the arc, got {:?}", solved.0);
    }

    #[test]
    fn deferred_tangent_pairs_with_arcs() {
        // The arc is the top half of a circle at (0,0) r=5.
        let arc = SnapCurve::Arc { center: Vec2::new(0.0, 0.0), radius: 5.0, start: 0.0, end: std::f64::consts::PI };
        // The circle is at (10,0) r=5.
        let circle = SnapCurve::Circle { center: Vec2::new(10.0, 0.0), radius: 5.0 };

        let d_arc = Deferred { mode: mode::TAN, curve: arc, at: Vec2::new(0.0, 5.0) };
        let d_circle = Deferred { mode: mode::TAN, curve: circle, at: Vec2::new(10.0, 5.0) };

        // Common tangents of the carrier circles: two outer (y=5 and y=-5) and two inner.
        // The y=-5 tangent touches the arc's circle at (0,-5), which is in the gap.
        // The y=5 tangent touches at (0,5), on the arc.
        // The inner tangents touch the left circle at angles that are also not on the top half.
        // Only the y=5 solution survives the arc's sweep.
        let solved = resolve_deferred(LineEnd::Deferred(d_arc), LineEnd::Deferred(d_circle)).unwrap();
        assert!(solved.0.near(Vec2::new(0.0, 5.0), 1e-9), "expected the belt tangent on the top arc, got {:?}", solved.0);
        assert!(solved.1.near(Vec2::new(10.0, 5.0), 1e-9));
    }
}
