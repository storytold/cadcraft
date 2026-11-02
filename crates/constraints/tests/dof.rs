//! Degrees of freedom, redundancy hints, ellipse/spline/block geometry and Smooth (G2).

use std::sync::Arc;

use cadcraft_constraints::{DofStatus, SolveError, SolveOptions, add, all_satisfied, dof, solve};
use cadcraft_doc::{
    Arc as DArc, Block, Circle as DCircle, Common, Constraint, ConstraintKind as K, DistAxis, Drawing, Ellipse as DEllipse, Entity, EntityKind,
    GeomRef, Handle, Insert, Line as DLine, Point as DPoint, Space, Sub,
};
use cadcraft_geom::{Spline as GSpline, Vec2, Vec3};

fn v(x: f64, y: f64) -> Vec3 {
    Vec3::new(x, y, 0.0)
}
fn put(d: &mut Drawing, k: EntityKind) -> Handle {
    d.add(&Space::Model, Common::default(), k).unwrap()
}
fn line(d: &mut Drawing, a: (f64, f64), b: (f64, f64)) -> Handle {
    put(d, EntityKind::Line(DLine { a: v(a.0, a.1), b: v(b.0, b.1) }))
}
fn circle(d: &mut Drawing, c: (f64, f64), r: f64) -> Handle {
    put(d, EntityKind::Circle(DCircle { center: v(c.0, c.1), radius: r }))
}
fn point(d: &mut Drawing, p: (f64, f64)) -> Handle {
    put(d, EntityKind::Point(DPoint { p: v(p.0, p.1), angle: 0.0 }))
}
fn ellipse(d: &mut Drawing, c: (f64, f64), major: (f64, f64), ratio: f64, start: f64, end: f64) -> Handle {
    put(d, EntityKind::Ellipse(DEllipse { center: v(c.0, c.1), major: v(major.0, major.1), ratio, start, end }))
}
fn spline(d: &mut Drawing, pts: &[(f64, f64)]) -> Handle {
    let s = GSpline::from_control(pts.iter().map(|p| Vec2::new(p.0, p.1)).collect(), 3);
    put(d, EntityKind::Spline(s))
}
fn con(kind: K, refs: &[GeomRef]) -> Constraint {
    Constraint { id: 0, kind, refs: refs.to_vec(), name: String::new(), expr: String::new() }
}
fn dim(kind: K, refs: &[GeomRef], expr: &str) -> Constraint {
    Constraint { id: 0, kind, refs: refs.to_vec(), name: String::new(), expr: expr.into() }
}
fn w(h: Handle) -> GeomRef {
    GeomRef::whole(h)
}
fn s(h: Handle, sub: Sub) -> GeomRef {
    GeomRef::new(h, sub)
}
fn ok(d: &mut Drawing, c: Constraint) -> u32 {
    let k = c.kind;
    add(d, c, &SolveOptions::default()).unwrap_or_else(|e| panic!("{k:?}: {e}"))
}
fn ends(d: &Drawing, h: Handle) -> (Vec2, Vec2) {
    match &d.entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        _ => panic!("not a line"),
    }
}
fn ell(d: &Drawing, h: Handle) -> cadcraft_geom::Ellipse {
    match &d.entity(h).unwrap().kind {
        EntityKind::Ellipse(e) => cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end },
        _ => panic!("not an ellipse"),
    }
}
fn spl(d: &Drawing, h: Handle) -> GSpline {
    match &d.entity(h).unwrap().kind {
        EntityKind::Spline(s) => s.clone(),
        _ => panic!("not a spline"),
    }
}
fn obj_dof(d: &Drawing, h: Handle) -> Option<usize> {
    let r = dof(d, &[h]).unwrap();
    r.objects.iter().find(|o| o.handle == h.hex()).and_then(|o| o.dof)
}

/// Signed curvature by sampling a curve near a parameter (central differences).
fn curvature(f: impl Fn(f64) -> Vec2, t: f64, h: f64) -> f64 {
    let (a, b, c) = (f(t - h), f(t), f(t + h));
    let d1 = (c - a) * (0.5 / h);
    let d2 = (c - b * 2.0 + a) * (1.0 / (h * h));
    d1.cross(d2) / d1.len().powi(3)
}

#[test]
fn rectangle_dof_counts_down_to_fully_constrained() {
    let mut d = Drawing::default();
    let l = [
        line(&mut d, (0.0, 0.0), (10.0, 0.2)),
        line(&mut d, (10.0, 0.2), (10.1, 5.0)),
        line(&mut d, (10.1, 5.0), (0.0, 5.0)),
        line(&mut d, (0.0, 5.0), (0.0, 0.0)),
    ];
    let r = dof(&d, &[]).unwrap();
    assert_eq!((r.status, r.dof, r.message.as_str()), (DofStatus::Unconstrained, Some(0), "No constraints"));
    // An unconstrained line has its four parameters.
    assert_eq!(obj_dof(&d, l[0]), Some(4));
    for i in 0..4 {
        ok(&mut d, con(K::Coincident, &[s(l[i], Sub::End), s(l[(i + 1) % 4], Sub::Start)]));
    }
    ok(&mut d, con(K::Horizontal, &[w(l[0])]));
    ok(&mut d, con(K::Horizontal, &[w(l[2])]));
    ok(&mut d, con(K::Vertical, &[w(l[1])]));
    ok(&mut d, con(K::Vertical, &[w(l[3])]));
    // 16 parameters − 12 equations: translation, width, height.
    let r = dof(&d, &[]).unwrap();
    assert_eq!((r.dof, r.status, r.redundant), (Some(4), DofStatus::Under, 0));
    assert_eq!(r.objects.len(), 4);
    assert!(r.objects.iter().all(|o| o.params == 4 && o.dof == Some(3) && o.status == DofStatus::Under), "{r:?}");
    ok(&mut d, con(K::Fix, &[s(l[0], Sub::Start)]));
    assert_eq!(dof(&d, &[]).unwrap().dof, Some(2));
    ok(&mut d, dim(K::Distance(DistAxis::Aligned), &[w(l[0])], "12"));
    // The bottom edge is now fixed; the others still stretch with the height.
    assert_eq!(obj_dof(&d, l[0]), Some(0));
    assert_eq!(obj_dof(&d, l[2]), Some(1));
    ok(&mut d, dim(K::Distance(DistAxis::Aligned), &[w(l[1])], "4"));
    let r = dof(&d, &[]).unwrap();
    assert_eq!((r.dof, r.status, r.message.as_str()), (Some(0), DofStatus::Fully, "Fully constrained"));
    assert!(r.objects.iter().all(|o| o.status == DofStatus::Fully));
    // A fully constrained rectangle refuses one more dimension, naming the constraints it repeats.
    let e = add(&mut d, dim(K::Distance(DistAxis::Aligned), &[w(l[3])], "4"), &SolveOptions::default()).unwrap_err();
    let SolveError::Redundant { ids } = e else { panic!("{e:?}") };
    assert!(!ids.is_empty());
}

#[test]
fn redundant_hint_names_the_duplicated_constraints() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.1));
    let b = line(&mut d, (0.0, 5.0), (10.0, 5.2));
    let c = line(&mut d, (20.0, 0.0), (20.0, 9.0));
    let ha = ok(&mut d, con(K::Horizontal, &[w(a)]));
    let hb = ok(&mut d, con(K::Horizontal, &[w(b)]));
    let vc = ok(&mut d, con(K::Vertical, &[w(c)]));
    ok(&mut d, con(K::Coincident, &[s(b, Sub::End), s(c, Sub::Start)]));
    let before = d.clone();
    let e = add(&mut d, con(K::Parallel, &[w(a), w(b)]), &SolveOptions::default()).unwrap_err();
    let SolveError::Redundant { ids } = &e else { panic!("{e:?}") };
    assert!(ids.contains(&ha) && ids.contains(&hb), "{ids:?}");
    assert!(!ids.contains(&vc), "{ids:?}");
    assert!(e.to_string().contains(&format!("#{ha}")), "{e}");
    assert_eq!(d, before);
}

#[test]
fn over_constrained_drawing_is_reported() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    ok(&mut d, con(K::Horizontal, &[w(a)]));
    // A file can hold a duplicate the command would have refused.
    d.constraints.push(Constraint { id: 99, ..con(K::Horizontal, &[w(a)]) });
    let r = dof(&d, &[]).unwrap();
    assert_eq!((r.status, r.redundant), (DofStatus::Over, 1));
    assert!(r.message.starts_with("Over-constrained"), "{}", r.message);
    assert_eq!(r.objects[0].status, DofStatus::Over);
}

#[test]
fn ellipse_constraints() {
    let mut d = Drawing::default();
    let e = ellipse(&mut d, (0.0, 0.0), (6.0, 0.5), 0.5, 0.0, std::f64::consts::TAU);
    assert_eq!(obj_dof(&d, e), Some(5));
    // Horizontal applies to the major axis.
    ok(&mut d, con(K::Horizontal, &[w(e)]));
    assert!(ell(&d, e).major.y.abs() < 1e-9);
    // A point on the ellipse.
    let p = point(&mut d, (0.3, 3.4));
    ok(&mut d, con(K::Coincident, &[w(p), w(e)]));
    let g = ell(&d, e);
    let pp = match &d.entity(p).unwrap().kind {
        EntityKind::Point(q) => q.p.xy(),
        _ => unreachable!(),
    };
    let (a, b) = (g.major.len(), g.major.len() * g.ratio);
    let q = pp - g.center;
    assert!(((q.x / a).powi(2) + (q.y / b).powi(2) - 1.0).abs() < 1e-7);
    // Concentric with a circle.
    let c = circle(&mut d, (0.5, -0.4), 1.0);
    ok(&mut d, con(K::Concentric, &[w(e), w(c)]));
    let cc = match &d.entity(c).unwrap().kind {
        EntityKind::Circle(k) => k.center.xy(),
        _ => unreachable!(),
    };
    assert!(cc.near(ell(&d, e).center, 1e-7));
    // A line tangent to the ellipse.
    let l = line(&mut d, (-10.0, 3.6), (10.0, 3.2));
    ok(&mut d, con(K::Tangent, &[w(l), w(e)]));
    let (la, lb) = ends(&d, l);
    let g = ell(&d, e);
    let n = (lb - la).normalized().perp();
    let support = (n.dot(g.major).powi(2) + n.dot(g.minor()).powi(2)).sqrt();
    assert!(((g.center - la).dot(n).abs() - support).abs() < 1e-7);
    // An axis end point coincides with a line end.
    let m = line(&mut d, (6.2, 0.3), (9.0, 5.0));
    ok(&mut d, con(K::Coincident, &[s(e, Sub::Vertex(0)), s(m, Sub::Start)]));
    let g = ell(&d, e);
    assert!(ends(&d, m).0.near(g.center + g.major, 1e-7));
    assert!(all_satisfied(&d));
}

#[test]
fn circle_tangent_to_ellipse_and_elliptical_arc_ends() {
    let mut d = Drawing::default();
    let e = ellipse(&mut d, (0.0, 0.0), (5.0, 0.0), 0.6, 0.0, std::f64::consts::TAU);
    ok(&mut d, con(K::Fix, &[w(e)]));
    let c = circle(&mut d, (7.5, 0.4), 2.0);
    ok(&mut d, con(K::Tangent, &[w(c), w(e)]));
    let (cc, r) = match &d.entity(c).unwrap().kind {
        EntityKind::Circle(k) => (k.center.xy(), k.radius),
        _ => unreachable!(),
    };
    // Closest distance from the circle's centre to the ellipse equals the radius.
    let g = ell(&d, e);
    let near = (0..20000).map(|i| g.at_param(i as f64 * std::f64::consts::TAU / 20000.0).dist(cc)).fold(f64::INFINITY, f64::min);
    assert!((near - r).abs() < 1e-3, "{near} vs {r}");
    assert!(all_satisfied(&d));
    // Elliptical arc ends are points.
    let arc = ellipse(&mut d, (20.0, 0.0), (4.0, 0.0), 0.5, 0.0, std::f64::consts::PI);
    assert_eq!(obj_dof(&d, arc), Some(7));
    let l = line(&mut d, (24.3, 0.2), (30.0, 0.0));
    ok(&mut d, con(K::Coincident, &[s(arc, Sub::Start), s(l, Sub::Start)]));
    let g = ell(&d, arc);
    assert!(ends(&d, l).0.near(g.at_param(g.start), 1e-7));
}

#[test]
fn spline_points_tangent_and_smooth_g2() {
    let mut d = Drawing::default();
    // Arc to the left of the spline's start.
    let a = put(&mut d, EntityKind::Arc(DArc { center: v(0.0, 3.0), radius: 3.0, start: std::f64::consts::PI, end: 1.5 * std::f64::consts::PI }));
    ok(&mut d, con(K::Fix, &[w(a)]));
    let sp = spline(&mut d, &[(0.1, 0.1), (3.0, 0.4), (6.0, 2.0), (9.0, 2.0)]);
    assert_eq!(obj_dof(&d, sp), Some(8));
    ok(&mut d, con(K::Smooth, &[w(sp), w(a)]));
    let g = spl(&d, sp);
    let (lo, _) = g.domain();
    // Joined at the arc's end (0, 0), tangent along +x, curvature 1/3 like the arc.
    assert!(g.eval(lo).near(Vec2::new(0.0, 0.0), 1e-7), "{:?}", g.eval(lo));
    let t = (g.eval(lo + 1e-6) - g.eval(lo)).normalized();
    assert!(t.near(Vec2::new(1.0, 0.0), 1e-4), "{t:?}");
    let k = curvature(|u| g.eval(u), lo + 1e-3, 1e-4);
    assert!((k - 1.0 / 3.0).abs() < 2e-2, "curvature {k}");
    assert!(all_satisfied(&d));
    // The spline's control points are constraint points.
    let p = point(&mut d, (9.5, 2.5));
    ok(&mut d, con(K::Coincident, &[s(sp, Sub::Vertex(3)), w(p)]));
    let pp = match &d.entity(p).unwrap().kind {
        EntityKind::Point(q) => q.p.xy(),
        _ => unreachable!(),
    };
    assert!(spl(&d, sp).control[3].near(pp, 1e-7));
    // Smooth to a line: the far end ends straight.
    let l = line(&mut d, (9.2, 2.1), (15.0, 3.0));
    ok(&mut d, con(K::Smooth, &[w(sp), w(l)]));
    let g = spl(&d, sp);
    let (_, hi) = g.domain();
    assert!(g.eval(hi).near(ends(&d, l).0, 1e-7));
    let k = curvature(|u| g.eval(u), hi - 1e-3, 1e-4);
    assert!(k.abs() < 2e-2, "curvature at the line {k}");
    assert!(all_satisfied(&d));
}

#[test]
fn spline_tangent_to_circle_and_fit_spline_smooth() {
    let mut d = Drawing::default();
    let c = circle(&mut d, (0.0, 0.0), 2.0);
    ok(&mut d, con(K::Fix, &[w(c)]));
    let sp = spline(&mut d, &[(2.1, 0.3), (2.5, 3.0), (5.0, 4.0), (8.0, 4.0)]);
    ok(&mut d, con(K::Tangent, &[w(sp), w(c)]));
    let g = spl(&d, sp);
    let (lo, _) = g.domain();
    let p = g.eval(lo);
    assert!((p.len() - 2.0).abs() < 1e-7);
    let t = (g.eval(lo + 1e-7) - p).normalized();
    assert!(t.dot(p.normalized()).abs() < 1e-4);
    // A fit-point spline keeps its fit points and is rebuilt through them.
    let fit: Vec<Vec2> = [(20.0, 0.0), (23.0, 2.0), (27.0, 1.0), (30.0, 3.0)].iter().map(|p| Vec2::new(p.0, p.1)).collect();
    let fs = put(&mut d, EntityKind::Spline(GSpline::from_fit_points(&fit)));
    let l = line(&mut d, (10.0, -1.0), (19.8, 0.2));
    ok(&mut d, con(K::Fix, &[w(l)]));
    ok(&mut d, con(K::Smooth, &[w(fs), w(l)]));
    let g = spl(&d, fs);
    assert_eq!(g.fit.len(), 4);
    assert!(g.fit[0].near(Vec2::new(19.8, 0.2), 1e-7));
    let (lo, _) = g.domain();
    assert!(g.eval(lo).near(g.fit[0], 1e-9));
    assert!(all_satisfied(&d));
}

#[test]
fn block_reference_moves_rigidly() {
    let mut d = Drawing::default();
    let mut b = Block::new("BAR");
    let h = d.new_handle();
    b.entities.push(Entity { handle: h, common: Common::default(), kind: EntityKind::Line(DLine { a: v(0.0, 0.0), b: v(4.0, 0.0) }) });
    d.blocks.insert("BAR".into(), Arc::new(b));
    let ins = put(
        &mut d,
        EntityKind::Insert(Insert {
            block: "BAR".into(),
            insert: v(1.0, 1.0),
            scale: v(1.0, 1.0),
            rotation: 1.2,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    );
    assert_eq!(obj_dof(&d, ins), Some(3));
    let p0 = point(&mut d, (10.0, 10.0));
    let wall = line(&mut d, (10.0, 0.0), (10.0, 20.0));
    ok(&mut d, con(K::Fix, &[w(p0)]));
    ok(&mut d, con(K::Fix, &[w(wall)]));
    // The insertion point onto p0 leaves the turn; the block's far end (rigid point 1) onto the
    // wall takes it.
    ok(&mut d, con(K::Coincident, &[w(ins), w(p0)]));
    assert_eq!(obj_dof(&d, ins), Some(1));
    ok(&mut d, con(K::Coincident, &[s(ins, Sub::Vertex(1)), w(wall)]));
    let EntityKind::Insert(i) = &d.entity(ins).unwrap().kind else { panic!() };
    assert!(i.insert.xy().near(Vec2::new(10.0, 10.0), 1e-7));
    assert!((i.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-7, "{}", i.rotation);
    // Both points again would be one equation too many.
    let p1 = point(&mut d, (10.0, 14.0));
    ok(&mut d, con(K::Fix, &[w(p1)]));
    assert!(matches!(add(&mut d, con(K::Coincident, &[s(ins, Sub::Vertex(1)), w(p1)]), &SolveOptions::default()), Err(SolveError::Redundant { .. })));
    assert_eq!(dof(&d, &[]).unwrap().status, DofStatus::Fully);
}

#[test]
fn hostile_new_geometry_never_panics() {
    let mut d = Drawing::default();
    let bad_e = ellipse(&mut d, (0.0, 0.0), (0.0, 0.0), f64::NAN, 0.0, 1.0);
    let flat_e = ellipse(&mut d, (0.0, 0.0), (1e300, 0.0), 0.0, 0.0, 1.0);
    let broken = put(&mut d, EntityKind::Spline(GSpline { degree: 7, knots: vec![f64::NAN; 3], control: vec![Vec2::ZERO; 2], ..Default::default() }));
    let one = put(&mut d, EntityKind::Spline(GSpline::from_control(vec![Vec2::new(1.0, 1.0); 3], 2)));
    let missing = put(
        &mut d,
        EntityKind::Insert(Insert {
            block: "NOPE".into(),
            insert: v(0.0, 0.0),
            scale: v(0.0, f64::INFINITY),
            rotation: f64::NAN,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    );
    let l = line(&mut d, (0.0, 0.0), (0.0, 0.0));
    let hs = [bad_e, flat_e, broken, one, missing, l];
    let kinds = [K::Coincident, K::Concentric, K::Tangent, K::Smooth, K::Horizontal, K::Fix, K::Equal, K::Symmetric, K::Radius];
    let subs = [Sub::Whole, Sub::Start, Sub::End, Sub::Center, Sub::Vertex(0), Sub::Vertex(3), Sub::Vertex(u32::MAX)];
    for k in kinds {
        for (i, h) in hs.iter().enumerate() {
            for sub in subs {
                let refs = [s(*h, sub), w(hs[(i + 1) % hs.len()]), w(hs[(i + 2) % hs.len()])];
                for n in 0..=3 {
                    let _ = add(&mut d, dim(k, &refs[..n], "1"), &SolveOptions::default());
                }
            }
        }
    }
    let _ = solve(&mut d, &SolveOptions::default());
    let _ = dof(&d, &hs);
    let _ = dof(&d, &[Handle(0), Handle(u64::MAX)]);
}

/// Fit-point vertex constraints retain a known freedom count because they do not reconstruct the spline.
#[test]
fn vertex_constraints_on_large_fit_spline_do_not_charge_reconstruction() {
    let mut d = Drawing::default();
    let pts: Vec<_> = (0..500).map(|i| Vec2::new(i as f64, (i % 3) as f64)).collect();
    let mut sp = GSpline::from_control(pts.clone(), 3);
    sp.fit = pts;
    let h = put(&mut d, EntityKind::Spline(sp));
    for i in 0..5 {
        let p = point(&mut d, (i as f64, (i % 3) as f64));
        let mut c = con(K::Coincident, &[s(h, Sub::Vertex(i)), w(p)]);
        c.id = i + 1;
        d.constraints.push(c);
    }
    let report = dof(&d, &[]).unwrap();
    assert_eq!(report.status, DofStatus::Under);
    assert_eq!(report.dof, Some(1000));
}

/// Adds a constraint that the stored ellipse can only hold with its axes renumbered: refused,
/// with the drawing unchanged.
fn refused_on_minor_axis_end(p: (f64, f64)) {
    let mut d = Drawing::default();
    let e = ellipse(&mut d, (0.0, 0.0), (4.0, 0.0), 0.5, 0.0, std::f64::consts::TAU);
    ok(&mut d, con(K::Fix, &[s(e, Sub::Center)]));
    let p = point(&mut d, p);
    ok(&mut d, con(K::Fix, &[w(p)]));
    let before = d.clone();
    let r = add(&mut d, con(K::Coincident, &[s(e, Sub::Vertex(1)), w(p)]), &SolveOptions::default());
    assert!(matches!(r, Err(SolveError::Conflict { .. })), "{r:?}, ellipse {:?}", ell(&d, e));
    assert_eq!(d, before);
}

/// Pulling a minor axis end past the major axis length would need a ratio above 1, which the
/// stored ellipse holds only by swapping its axes (and so renumbering their ends).
#[test]
fn minor_axis_end_pulled_past_the_major_axis_is_refused() {
    refused_on_minor_axis_end((0.0, 6.0));
}

/// Pulling a minor axis end through the centre would need a negative ratio, which the stored
/// ellipse cannot hold: storing its absolute value would mirror the minor axis end.
#[test]
fn minor_axis_end_pulled_through_the_centre_is_refused() {
    refused_on_minor_axis_end((0.0, -1.0));
}

/// A circle-like ellipse whose solution makes the minor axis the longer one: stored with its axes
/// swapped, which constraints that name no axis end by index do not notice.
fn round_ellipse(d: &mut Drawing, start: f64, end: f64) -> Handle {
    let e = ellipse(d, (0.0, 0.0), (4.0, 0.0), 1.0, start, end);
    ok(d, con(K::Fix, &[s(e, Sub::Center)]));
    e
}

#[test]
fn tangent_that_swaps_a_round_ellipse_axes_is_kept() {
    let mut d = Drawing::default();
    let e = round_ellipse(&mut d, 0.0, std::f64::consts::TAU);
    let l = line(&mut d, (-10.0, 4.5), (10.0, 4.5));
    ok(&mut d, con(K::Fix, &[w(l)]));
    ok(&mut d, con(K::Tangent, &[w(l), w(e)]));
    assert!(ell(&d, e).ratio <= 1.0);
    assert!(all_satisfied(&d));
}

#[test]
fn point_on_curve_that_swaps_a_round_ellipse_axes_is_kept() {
    let mut d = Drawing::default();
    let e = round_ellipse(&mut d, 0.0, std::f64::consts::TAU);
    let p = point(&mut d, (0.0, 4.5));
    ok(&mut d, con(K::Fix, &[w(p)]));
    ok(&mut d, con(K::Coincident, &[w(p), w(e)]));
    assert!(ell(&d, e).ratio <= 1.0);
    assert!(all_satisfied(&d));
}

#[test]
fn elliptical_arc_start_that_swaps_its_axes_is_kept() {
    let mut d = Drawing::default();
    let e = round_ellipse(&mut d, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    let p = point(&mut d, (0.0, 4.5));
    ok(&mut d, con(K::Fix, &[w(p)]));
    ok(&mut d, con(K::Coincident, &[s(e, Sub::Start), w(p)]));
    assert!(ell(&d, e).ratio <= 1.0);
    assert!(all_satisfied(&d));
}

#[test]
fn tangent_circle_that_swaps_a_round_ellipse_axes_is_kept() {
    let mut d = Drawing::default();
    let e = round_ellipse(&mut d, 0.0, std::f64::consts::TAU);
    let c = circle(&mut d, (0.0, 6.0), 1.5);
    ok(&mut d, con(K::Fix, &[w(c)]));
    ok(&mut d, con(K::Tangent, &[w(c), w(e)]));
    assert!(ell(&d, e).ratio <= 1.0);
    assert!(all_satisfied(&d));
}
