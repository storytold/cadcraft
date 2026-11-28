//! Exact arcs in traced hatch boundaries (#1167).
//!
//! The boundary search works on flattened curves, so a loop traced around an internal point
//! comes out as chords: a hatch in a circle had 158 straight edges and the wrong area. Each
//! traced edge knows the input polyline it lies on, and each input polyline is one piece of one
//! object (a line, a circle, an arc or one segment of a polyline), so nothing is guessed from
//! nearby geometry: a run of edges on the same circular piece becomes one bulged vertex again,
//! and the corners move onto the exact curves (where a line or another circle crosses).

use cadcraft_doc::{Entity, EntityKind, Prim};
use cadcraft_geom::{Circle, Line, PolyVertex, Segment, Vec2, arc_to_bulge, circle_circle, line_circle, shoelace};

/// The exact curve behind one input polyline.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Piece {
    Line(Line),
    Circle(Circle),
}

/// The exact piece behind each of `e`'s `n` outline polylines (one per primitive, in order, as
/// `select::hit_polylines` makes them), or `None` for each when `e` isn't made of lines and
/// circular arcs or the count doesn't match.
pub(crate) fn pieces_of(e: &Entity, n: usize) -> Vec<Option<Piece>> {
    if !matches!(e.kind, EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::Circle(_) | EntityKind::LwPolyline(_)) {
        return vec![None; n];
    }
    let prims = e.kind.prims();
    if prims.len() != n {
        return vec![None; n];
    }
    prims
        .iter()
        .map(|p| match p {
            Prim::Seg(Segment::Line(l)) => Some(Piece::Line(*l)),
            Prim::Seg(Segment::Arc { arc, .. }) => Some(Piece::Circle(Circle::new(arc.center, arc.radius))),
            Prim::Circle(c) => Some(Piece::Circle(*c)),
            _ => None,
        })
        .collect()
}

fn same_circle(a: &Circle, b: &Circle) -> bool {
    a.center == b.center && a.radius == b.radius
}

/// The loop `pts` (edge `i` from `pts[i]` to the next vertex, closed) with its circular runs
/// as bulged vertices. `edges` names each edge's input polyline, `pieces` each polyline's exact
/// curve; `tol` is the flattening tolerance the polylines were made with. Anything that doesn't
/// check out stays as traced.
pub(crate) fn with_arcs(pts: &[Vec2], edges: &[Option<usize>], pieces: &[Option<Piece>], tol: f64) -> Vec<PolyVertex> {
    let n = pts.len();
    let plain = || pts.iter().map(|p| PolyVertex::new(*p)).collect::<Vec<_>>();
    if n < 3 || edges.len() != n || !(tol.is_finite() && tol > 0.0) || pts.iter().any(|p| !p.is_finite()) {
        return plain();
    }
    let at = |i: usize| pts.get(i % n).copied().unwrap_or_default();
    let piece = |i: usize| edges.get(i % n).copied().flatten().and_then(|k| pieces.get(k).copied().flatten());
    // A chord of a curve flattened to `tol` stays within `tol` of it; corners split where two
    // chords cross may be a little further.
    let slack = tol * 4.0;
    let near_circle = |c: &Circle, q: Vec2| (c.center.dist(q) - c.radius).abs() <= slack;
    // The circle each edge lies on: its piece is a circle and both ends and the middle are on
    // it (the middle, so a long chord across the circle doesn't pass for an arc).
    let circ: Vec<Option<(usize, Circle)>> = (0..n)
        .map(|i| {
            let k = edges.get(i).copied().flatten()?;
            let Some(Piece::Circle(c)) = piece(i) else { return None };
            let (a, b) = (at(i), at(i + 1));
            (c.radius.is_finite() && c.radius > slack && near_circle(&c, a) && near_circle(&c, b) && near_circle(&c, a.mid(b))).then_some((k, c))
        })
        .collect();
    let key = |i: usize| circ.get(i % n).copied().flatten().map(|(k, _)| k);
    if circ.iter().all(Option::is_none) {
        return plain();
    }
    let ccw = shoelace(pts) > 0.0;
    // One whole circle: two half circles, as a hatch made by selecting the circle has.
    if let Some(Some((k0, c))) = circ.first().copied()
        && (0..n).all(|i| key(i) == Some(k0))
    {
        let b = if ccw { 1.0 } else { -1.0 };
        return vec![PolyVertex::with_bulge(c.center - Vec2::X * c.radius, b), PolyVertex::with_bulge(c.center + Vec2::X * c.radius, b)];
    }
    let line_of = |i: usize| match piece(i) {
        Some(Piece::Line(l)) if l.a != l.b && [at(i), at(i + 1)].iter().all(|q| ((*q - l.a).cross(l.b - l.a) / l.a.dist(l.b)).abs() <= slack) => {
            Some(l)
        }
        _ => None,
    };
    let onto = |c: &Circle, q: Vec2| {
        let d = q - c.center;
        if d.len() > 0.0 { c.center + d.normalized() * c.radius } else { q }
    };
    // Vertex `i` (between edges `i - 1` and `i`) on the exact curves: where the two pieces
    // cross, nearest the traced corner, or else moved onto the circle it is on.
    let corner = |i: usize| -> Vec2 {
        let p = at(i);
        let (prev, next) = (circ.get((i + n - 1) % n).copied().flatten(), circ.get(i % n).copied().flatten());
        let crossings: Vec<Vec2> = match (prev, next) {
            (Some((_, a)), Some((_, b))) if same_circle(&a, &b) => return onto(&a, p),
            (Some((_, a)), Some((_, b))) => circle_circle(&a, &b),
            (Some((_, c)), None) => line_of(i).map(|l| line_circle(&l, &c).into_iter().map(|x| x.0).collect()).unwrap_or_default(),
            (None, Some((_, c))) => line_of(i + n - 1).map(|l| line_circle(&l, &c).into_iter().map(|x| x.0).collect()).unwrap_or_default(),
            (None, None) => return p,
        };
        // Where two curves cross at a shallow angle, a corner off them by the flattening
        // tolerance can be off along them by up to about √(2·r·tolerance): the crossing nearest
        // the traced corner is taken within that reach, when no other crossing is as near.
        let r = prev.or(next).map_or(0.0, |(_, c)| c.radius);
        let reach = slack * 2.0 + (2.0 * r * slack).sqrt();
        let mut xs: Vec<(f64, Vec2)> = crossings.into_iter().filter(|x| x.is_finite()).map(|x| (x.dist(p), x)).collect();
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        match (xs.first(), xs.get(1)) {
            (Some(&(d, x)), second) if d <= reach && second.is_none_or(|&(d2, _)| d2 > 2.0 * d + slack) => x,
            _ => prev.or(next).map_or(p, |(_, c)| onto(&c, p)),
        }
    };
    // Start where a run begins, so no run wraps past the end.
    let Some(s) = (0..n).find(|&i| key(i) != key(i + n - 1)) else { return plain() };
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let e = (s + i) % n;
        let p = corner(e);
        let Some((k, c)) = circ.get(e).copied().flatten() else {
            out.push(PolyVertex::new(p));
            i += 1;
            continue;
        };
        // The angle the run turns through about the centre, edge by edge.
        let mut sweep = 0.0;
        let mut a = p;
        let mut j = i;
        while j < n && key(s + j) == Some(k) {
            let last = j + 1 >= n || key(s + j + 1) != Some(k);
            let b = if last { corner(s + j + 1) } else { at(s + j + 1) };
            let (u, v) = (a - c.center, b - c.center);
            sweep += u.cross(v).atan2(u.dot(v));
            a = b;
            j += 1;
        }
        let bulge = arc_to_bulge(sweep);
        out.push(PolyVertex::with_bulge(p, if bulge.is_finite() { bulge } else { 0.0 }));
        i = j;
    }
    if out.len() < 2 { plain() } else { out }
}
