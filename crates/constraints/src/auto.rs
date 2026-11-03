//! AUTOCONSTRAIN: infer constraints from geometry that already nearly satisfies them; and
//! CONSTRAINTINFER's inference for what a command just placed ([`infer_placed`]).

use cadcraft_doc::{Constraint, ConstraintKind, Drawing, EntityKind, GeomRef, Handle, Sub};
use cadcraft_geom::{Line, Vec2};

/// Most objects considered at once.
pub const MAX_OBJECTS: usize = 200;
/// Most candidates returned.
pub const MAX_CANDIDATES: usize = 2000;

/// Inference options.
#[derive(Clone, Debug)]
pub struct InferOptions {
    pub distance_tolerance: f64,
    /// Degrees.
    pub angle_tolerance: f64,
    /// Constraint type names allowed (`Coincident`, `Horizontal`, …); empty = all.
    pub types: Vec<String>,
}

impl Default for InferOptions {
    fn default() -> Self {
        InferOptions { distance_tolerance: 0.05, angle_tolerance: 1.0, types: Vec::new() }
    }
}

struct Geo {
    points: Vec<(GeomRef, Vec2)>,
    lines: Vec<(GeomRef, Vec2, Vec2)>,
    curves: Vec<(GeomRef, Vec2, f64)>,
}

fn collect(d: &Drawing, handles: &[Handle]) -> Geo {
    let mut g = Geo { points: Vec::new(), lines: Vec::new(), curves: Vec::new() };
    for &h in handles.iter().take(MAX_OBJECTS) {
        let Some(e) = d.entity(h) else { continue };
        match &e.kind {
            EntityKind::Line(l) => {
                let (a, b) = (l.a.xy(), l.b.xy());
                if a.is_finite() && b.is_finite() {
                    g.points.push((GeomRef::new(h, Sub::Start), a));
                    g.points.push((GeomRef::new(h, Sub::End), b));
                    g.lines.push((GeomRef::whole(h), a, b));
                }
            }
            EntityKind::Arc(a) => {
                let ga = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
                if ga.center.is_finite() && a.radius.is_finite() {
                    g.points.push((GeomRef::new(h, Sub::Start), ga.start_point()));
                    g.points.push((GeomRef::new(h, Sub::End), ga.end_point()));
                    g.curves.push((GeomRef::whole(h), ga.center, a.radius));
                }
            }
            EntityKind::Circle(c) if c.center.is_finite() && c.radius.is_finite() => {
                g.curves.push((GeomRef::whole(h), c.center.xy(), c.radius));
            }
            EntityKind::Point(p) if p.p.is_finite() => {
                g.points.push((GeomRef::whole(h), p.p.xy()));
            }
            EntityKind::LwPolyline(p) => {
                let n = p.vertices.len();
                if !(2..=4096).contains(&n) {
                    continue;
                }
                if !p.closed
                    && let (Some(f), Some(l)) = (p.vertices.first(), p.vertices.last())
                {
                    g.points.push((GeomRef::new(h, Sub::Vertex(0)), f.p));
                    g.points.push((GeomRef::new(h, Sub::Vertex((n - 1) as u32)), l.p));
                }
                let segs = if p.closed { n } else { n - 1 };
                for i in 0..segs {
                    let (Some(a), Some(b)) = (p.vertices.get(i), p.vertices.get((i + 1) % n)) else { continue };
                    if a.bulge.abs() < 1e-9 && a.p.is_finite() && b.p.is_finite() {
                        g.lines.push((GeomRef::new(h, Sub::Segment(i as u32)), a.p, b.p));
                    }
                }
            }
            _ => {}
        }
    }
    g.points.truncate(1000);
    g.lines.truncate(1000);
    g.curves.truncate(1000);
    g
}

fn ang_between(d1: Vec2, d2: Vec2) -> f64 {
    // Acute angle between two directions, radians in [0, π/2].
    let a = d1.cross(d2).atan2(d1.dot(d2)).abs();
    if a > std::f64::consts::FRAC_PI_2 { std::f64::consts::PI - a } else { a }
}

/// Candidate constraints, most important first (coincident, concentric, tangent, horizontal,
/// vertical, collinear, perpendicular, parallel, equal). Apply them one by one with
/// [`crate::add`], skipping the ones it refuses as redundant.
pub fn infer(d: &Drawing, handles: &[Handle], o: &InferOptions) -> Vec<Constraint> {
    let tol = if o.distance_tolerance.is_finite() { o.distance_tolerance.clamp(0.0, 1e9) } else { 0.05 };
    let atol = if o.angle_tolerance.is_finite() { o.angle_tolerance.clamp(0.0, 45.0).to_radians() } else { 1f64.to_radians() };
    // Resolve the allowed type names into a set once, so membership is O(1) per candidate instead
    // of re-scanning the whole (attacker-controllable) `o.types` list for every geometric match
    // (CWE-407). Empty `o.types` means "all types".
    let allowed: std::collections::HashSet<String> = o.types.iter().map(|t| t.to_ascii_lowercase()).collect();
    let allow = |k: ConstraintKind| allowed.is_empty() || allowed.contains(&k.name().to_ascii_lowercase());
    let g = collect(d, handles);
    let mut out: Vec<Constraint> = Vec::new();
    let mut push = |kind: ConstraintKind, refs: Vec<GeomRef>| {
        if out.len() < MAX_CANDIDATES && allow(kind) {
            out.push(Constraint { id: 0, kind, refs, name: String::new(), expr: String::new() });
        }
    };
    let pairs = |n: usize| (0..n).flat_map(move |i| (i + 1..n).map(move |j| (i, j)));
    // Coincident end points of different objects.
    for (i, j) in pairs(g.points.len()) {
        let (Some((ra, a)), Some((rb, b))) = (g.points.get(i), g.points.get(j)) else { continue };
        if ra.handle != rb.handle && a.dist(*b) <= tol {
            push(ConstraintKind::Coincident, vec![*ra, *rb]);
        }
    }
    // Concentric and tangent circles.
    for (i, j) in pairs(g.curves.len()) {
        let (Some((ra, ca, r1)), Some((rb, cb, r2))) = (g.curves.get(i), g.curves.get(j)) else { continue };
        let dist = ca.dist(*cb);
        if dist <= tol {
            push(ConstraintKind::Concentric, vec![*ra, *rb]);
        } else if (dist - (r1 + r2)).abs() <= tol || (dist - (r1 - r2).abs()).abs() <= tol {
            push(ConstraintKind::Tangent, vec![*ra, *rb]);
        }
    }
    for (rl, a, b) in &g.lines {
        for (rc, c, r) in &g.curves {
            if rl.handle == rc.handle {
                continue;
            }
            let ln = Line::new(*a, *b);
            let t = ln.param_of(*c);
            if (ln.dist(*c) - r).abs() <= tol && (-1e-9..=1.0 + 1e-9).contains(&t) && Line::new(*a, *b).len() > tol {
                push(ConstraintKind::Tangent, vec![*rl, *rc]);
            }
        }
    }
    // Horizontal / vertical.
    for (r, a, b) in &g.lines {
        let dvec = *b - *a;
        if dvec.len() <= tol {
            continue;
        }
        if ang_between(dvec, Vec2::X) <= atol {
            push(ConstraintKind::Horizontal, vec![*r]);
        } else if ang_between(dvec, Vec2::Y) <= atol {
            push(ConstraintKind::Vertical, vec![*r]);
        }
    }
    // Line pairs.
    let connected = |a1: Vec2, b1: Vec2, a2: Vec2, b2: Vec2| [a2, b2].iter().any(|p| p.dist(a1) <= tol || p.dist(b1) <= tol);
    let mut parallel = Vec::new();
    let mut equal = Vec::new();
    for (i, j) in pairs(g.lines.len()) {
        let (Some((r1, a1, b1)), Some((r2, a2, b2))) = (g.lines.get(i), g.lines.get(j)) else { continue };
        let (d1, d2) = (*b1 - *a1, *b2 - *a2);
        if d1.len() <= tol || d2.len() <= tol {
            continue;
        }
        let ang = ang_between(d1, d2);
        let l1 = Line::new(*a1, *b1);
        if ang <= atol {
            if l1.dist(*a2) <= tol && l1.dist(*b2) <= tol {
                push(ConstraintKind::Collinear, vec![*r1, *r2]);
            } else {
                parallel.push((*r1, *r2));
            }
        } else if (std::f64::consts::FRAC_PI_2 - ang).abs() <= atol && connected(*a1, *b1, *a2, *b2) {
            push(ConstraintKind::Perpendicular, vec![*r1, *r2]);
        }
        if (d1.len() - d2.len()).abs() <= tol {
            equal.push((*r1, *r2));
        }
    }
    for (a, b) in parallel {
        push(ConstraintKind::Parallel, vec![a, b]);
    }
    for (a, b) in equal {
        push(ConstraintKind::Equal, vec![a, b]);
    }
    for (i, j) in pairs(g.curves.len()) {
        let (Some((ra, _, r1)), Some((rb, _, r2))) = (g.curves.get(i), g.curves.get(j)) else { continue };
        if (r1 - r2).abs() <= tol {
            push(ConstraintKind::Equal, vec![*ra, *rb]);
        }
    }
    out
}

/// Constraint types CONSTRAINTINFER applies.
pub const INFER_TYPES: [ConstraintKind; 6] = [
    ConstraintKind::Coincident,
    ConstraintKind::Tangent,
    ConstraintKind::Perpendicular,
    ConstraintKind::Parallel,
    ConstraintKind::Horizontal,
    ConstraintKind::Vertical,
];

/// CONSTRAINTINFER: the constraints that the exact placement of `placed` (objects a command just
/// drew or edited) implies against each other and `others`: end points snapped onto end points,
/// midpoints, centres or onto objects (Coincident), tangent and perpendicular snaps, parallel
/// lines, and horizontal/vertical lines (ortho, polar). Snapped geometry is exact, so the
/// tolerances in `o` are expected to be tight. Only [`INFER_TYPES`] (and `o.types`, when set)
/// are returned, each involving a placed object.
pub fn infer_placed(d: &Drawing, placed: &[Handle], others: &[Handle], o: &InferOptions) -> Vec<Constraint> {
    let tol = if o.distance_tolerance.is_finite() { o.distance_tolerance.clamp(0.0, 1e9) } else { 0.0 };
    let mut all: Vec<Handle> = placed.iter().take(MAX_OBJECTS).copied().collect();
    for h in others {
        if all.len() >= MAX_OBJECTS {
            break;
        }
        if !all.contains(h) {
            all.push(*h);
        }
    }
    let mine = |c: &Constraint| c.refs.iter().any(|r| placed.contains(&r.handle));
    let mut out: Vec<Constraint> = infer(d, &all, o).into_iter().filter(|c| INFER_TYPES.contains(&c.kind) && mine(c)).collect();
    // Placed end points on other objects' midpoints, centres, or on the objects themselves.
    let allow = o.types.is_empty() || o.types.iter().any(|t| t.eq_ignore_ascii_case(ConstraintKind::Coincident.name()));
    if !allow {
        return out;
    }
    let g = collect(d, &all);
    // Point to point (midpoint, centre) first; point on an object last, so a tangent or
    // perpendicular snap keeps its tangency (an end on the curve it touches adds nothing then).
    let mut extra = Vec::new();
    let mut on = Vec::new();
    for (rp, p) in g.points.iter().filter(|(r, _)| placed.contains(&r.handle)) {
        if out.iter().any(|c| c.kind == ConstraintKind::Coincident && c.refs.contains(rp)) {
            continue;
        }
        let target = g
            .lines
            .iter()
            .filter(|(r, _, _)| r.handle != rp.handle)
            .find_map(|(r, a, b)| {
                let ln = Line::new(*a, *b);
                if a.mid(*b).dist(*p) <= tol && r.sub == Sub::Whole {
                    Some(GeomRef::new(r.handle, Sub::Mid))
                } else {
                    let t = ln.param_of(*p);
                    (ln.dist(*p) <= tol && (0.0..=1.0).contains(&t) && ln.len() > tol).then_some(*r)
                }
            })
            .or_else(|| {
                g.curves.iter().filter(|(r, _, _)| r.handle != rp.handle).find_map(|(r, c, rad)| {
                    if c.dist(*p) <= tol {
                        Some(GeomRef::new(r.handle, Sub::Center))
                    } else {
                        ((c.dist(*p) - rad).abs() <= tol && on_arc(d, r.handle, *c, *p)).then_some(*r)
                    }
                })
            });
        if let Some(t) = target
            && extra.len() + on.len() + out.len() < MAX_CANDIDATES
        {
            let c = Constraint { id: 0, kind: ConstraintKind::Coincident, refs: vec![*rp, t], name: String::new(), expr: String::new() };
            if t.sub == Sub::Whole { on.push(c) } else { extra.push(c) }
        }
    }
    // Perpendicular lines meeting in a T (a perpendicular snap onto the middle of a line).
    let atol = if o.angle_tolerance.is_finite() { o.angle_tolerance.clamp(0.0, 45.0).to_radians() } else { 0.0 };
    let allow_per = o.types.is_empty() || o.types.iter().any(|t| t.eq_ignore_ascii_case(ConstraintKind::Perpendicular.name()));
    for (r1, a1, b1) in g.lines.iter().filter(|(r, _, _)| placed.contains(&r.handle)) {
        for (r2, a2, b2) in g.lines.iter().filter(|(r, _, _)| r.handle != r1.handle) {
            let (l1, l2) = (Line::new(*a1, *b1), Line::new(*a2, *b2));
            let touches = |l: &Line, p: Vec2| l.dist(p) <= tol && (0.0..=1.0).contains(&l.param_of(p));
            let tee = [*a1, *b1].iter().any(|p| touches(&l2, *p)) || [*a2, *b2].iter().any(|p| touches(&l1, *p));
            let ang = ang_between(*b1 - *a1, *b2 - *a2);
            let dup = out.iter().any(|c| c.kind == ConstraintKind::Perpendicular && c.refs.contains(r1) && c.refs.contains(r2));
            if allow_per
                && tee
                && !dup
                && (std::f64::consts::FRAC_PI_2 - ang).abs() <= atol
                && l1.len() > tol
                && l2.len() > tol
                && out.len() < MAX_CANDIDATES
            {
                out.push(Constraint { id: 0, kind: ConstraintKind::Perpendicular, refs: vec![*r1, *r2], name: String::new(), expr: String::new() });
            }
        }
    }
    let n = out.iter().take_while(|c| c.kind == ConstraintKind::Coincident).count();
    out.splice(n..n, extra);
    out.extend(on);
    out
}

/// Does `p` (on the circle) lie within the arc's sweep (always for a circle)?
fn on_arc(d: &Drawing, h: Handle, c: Vec2, p: Vec2) -> bool {
    match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Arc(a)) => cadcraft_geom::angle_in_sweep((p - c).angle(), a.start, a.end),
        _ => true,
    }
}

/// Infer and apply constraints over `handles`; returns the ids added. Redundant or conflicting
/// candidates are skipped.
pub fn autoconstrain(d: &mut Drawing, handles: &[Handle], o: &InferOptions) -> Vec<u32> {
    let mut added = Vec::new();
    for c in infer(d, handles, o) {
        if let Ok(id) = crate::add(d, c, &crate::SolveOptions::default()) {
            added.push(id);
        }
    }
    added
}
