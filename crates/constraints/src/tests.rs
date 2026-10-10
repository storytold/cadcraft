use cadcraft_doc::{Arc as DArc, Circle as DCircle, Common, DistAxis, EntityKind, Line as DLine, LwPolyline, Point as DPoint, Space, Sub};
use cadcraft_geom::{PolyVertex, Vec3};

use super::*;

fn v(x: f64, y: f64) -> Vec3 {
    Vec3::new(x, y, 0.0)
}
fn add_e(d: &mut Drawing, k: EntityKind) -> Handle {
    d.add(&Space::Model, Common::default(), k).unwrap()
}
fn line(d: &mut Drawing, a: (f64, f64), b: (f64, f64)) -> Handle {
    add_e(d, EntityKind::Line(DLine { a: v(a.0, a.1), b: v(b.0, b.1) }))
}
fn circle(d: &mut Drawing, c: (f64, f64), r: f64) -> Handle {
    add_e(d, EntityKind::Circle(DCircle { center: v(c.0, c.1), radius: r }))
}
fn arc(d: &mut Drawing, c: (f64, f64), r: f64, s: f64, e: f64) -> Handle {
    add_e(d, EntityKind::Arc(DArc { center: v(c.0, c.1), radius: r, start: s, end: e }))
}
fn con(kind: ConstraintKind, refs: &[GeomRef]) -> Constraint {
    Constraint { id: 0, kind, refs: refs.to_vec(), name: String::new(), expr: String::new() }
}
fn dim(kind: ConstraintKind, refs: &[GeomRef], name: &str, expr: &str) -> Constraint {
    Constraint { id: 0, kind, refs: refs.to_vec(), name: name.into(), expr: expr.into() }
}
fn w(h: Handle) -> GeomRef {
    GeomRef::whole(h)
}
fn s(h: Handle, sub: Sub) -> GeomRef {
    GeomRef::new(h, sub)
}
fn ln(d: &Drawing, h: Handle) -> (Vec2, Vec2) {
    match &d.entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        _ => panic!(),
    }
}
fn circ(d: &Drawing, h: Handle) -> (Vec2, f64) {
    match &d.entity(h).unwrap().kind {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        EntityKind::Arc(c) => (c.center.xy(), c.radius),
        _ => panic!(),
    }
}
fn ok(d: &mut Drawing, c: Constraint) -> u32 {
    add(d, c, &SolveOptions::default()).unwrap()
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn coincident_points_and_point_on_curve() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    let b = line(&mut d, (10.3, 0.4), (10.0, 8.0));
    ok(&mut d, con(ConstraintKind::Coincident, &[s(a, Sub::End), s(b, Sub::Start)]));
    let (la, lb) = (ln(&d, a), ln(&d, b));
    assert!(la.1.near(lb.0, 1e-7));
    // The second object moved, the first stayed (minimum movement prefers equal split, but both
    // ends ended up together).
    let c = circle(&mut d, (5.0, 6.0), 2.0);
    let p = add_e(&mut d, EntityKind::Point(DPoint { p: v(5.0, 9.0), angle: 0.0 }));
    ok(&mut d, con(ConstraintKind::Coincident, &[w(p), w(c)]));
    let (cc, r) = circ(&d, c);
    let pp = match &d.entity(p).unwrap().kind {
        EntityKind::Point(q) => q.p.xy(),
        _ => panic!(),
    };
    assert!(near(pp.dist(cc), r));
}

#[test]
fn horizontal_vertical_parallel_perpendicular() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.5));
    let b = line(&mut d, (0.0, 2.0), (0.3, 9.0));
    let c = line(&mut d, (5.0, 5.0), (9.0, 6.0));
    let e = line(&mut d, (5.0, -5.0), (6.0, -1.0));
    ok(&mut d, con(ConstraintKind::Horizontal, &[w(a)]));
    ok(&mut d, con(ConstraintKind::Vertical, &[w(b)]));
    ok(&mut d, con(ConstraintKind::Parallel, &[w(a), w(c)]));
    ok(&mut d, con(ConstraintKind::Perpendicular, &[w(a), w(e)]));
    let (a0, a1) = ln(&d, a);
    assert!(near(a0.y, a1.y));
    let (b0, b1) = ln(&d, b);
    assert!(near(b0.x, b1.x));
    let (c0, c1) = ln(&d, c);
    assert!(near(c0.y, c1.y));
    let (e0, e1) = ln(&d, e);
    assert!(near(e0.x, e1.x));
}

#[test]
fn collinear_concentric_equal() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    let b = line(&mut d, (12.0, 0.5), (20.0, -0.3));
    ok(&mut d, con(ConstraintKind::Collinear, &[w(a), w(b)]));
    let (a0, a1) = ln(&d, a);
    let (b0, b1) = ln(&d, b);
    let dir = (a1 - a0).normalized();
    assert!(dir.cross(b0 - a0).abs() < 1e-6 && dir.cross(b1 - a0).abs() < 1e-6);

    let c1 = circle(&mut d, (0.0, 20.0), 3.0);
    let c2 = arc(&mut d, (0.5, 20.4), 5.0, 0.0, 2.0);
    ok(&mut d, con(ConstraintKind::Concentric, &[w(c1), w(c2)]));
    assert!(circ(&d, c1).0.near(circ(&d, c2).0, 1e-7));
    ok(&mut d, con(ConstraintKind::Equal, &[w(c1), w(c2)]));
    assert!(near(circ(&d, c1).1, circ(&d, c2).1));
    ok(&mut d, con(ConstraintKind::Equal, &[w(a), w(b)]));
    let (a0, a1) = ln(&d, a);
    let (b0, b1) = ln(&d, b);
    assert!(near(a0.dist(a1), b0.dist(b1)));
}

#[test]
fn tangent_and_smooth() {
    let mut d = Drawing::default();
    let l = line(&mut d, (-10.0, 0.0), (10.0, 0.0));
    let c = circle(&mut d, (0.0, 3.5), 3.0);
    ok(&mut d, con(ConstraintKind::Tangent, &[w(l), w(c)]));
    let (cc, r) = circ(&d, c);
    let (a, b) = ln(&d, l);
    assert!(near((b - a).normalized().cross(cc - a).abs(), r));
    // Circle–circle external.
    let c2 = circle(&mut d, (8.0, 3.5), 4.0);
    ok(&mut d, con(ConstraintKind::Tangent, &[w(c), w(c2)]));
    let ((p, r1), (q, r2)) = (circ(&d, c), circ(&d, c2));
    assert!(near(p.dist(q), r1 + r2));
    // Smooth behaves as tangent.
    let l2 = line(&mut d, (20.0, 0.0), (30.0, 1.0));
    let a2 = arc(&mut d, (25.0, 3.0), 2.5, 0.0, 3.0);
    ok(&mut d, con(ConstraintKind::Smooth, &[w(l2), w(a2)]));
    let ((ca, ra), (x0, x1)) = (circ(&d, a2), ln(&d, l2));
    assert!(near((x1 - x0).normalized().cross(ca - x0).abs(), ra));
}

#[test]
fn symmetric_fix() {
    let mut d = Drawing::default();
    let axis = line(&mut d, (0.0, -10.0), (0.0, 10.0));
    ok(&mut d, con(ConstraintKind::Fix, &[w(axis)]));
    let a = line(&mut d, (-5.0, 0.0), (-3.0, 4.0));
    let b = line(&mut d, (5.5, 0.3), (2.0, 4.5));
    ok(&mut d, con(ConstraintKind::Symmetric, &[w(a), w(b), w(axis)]));
    assert_eq!(ln(&d, axis), (Vec2::new(0.0, -10.0), Vec2::new(0.0, 10.0)));
    let (a0, a1) = ln(&d, a);
    let (b0, b1) = ln(&d, b);
    assert!(a0.near(Vec2::new(-b0.x, b0.y), 1e-6) && a1.near(Vec2::new(-b1.x, b1.y), 1e-6));
    // Symmetric points.
    let p = add_e(&mut d, EntityKind::Point(DPoint { p: v(-2.0, 7.0), angle: 0.0 }));
    let q = add_e(&mut d, EntityKind::Point(DPoint { p: v(2.5, 6.0), angle: 0.0 }));
    ok(&mut d, con(ConstraintKind::Symmetric, &[w(p), w(q), w(axis)]));
}

#[test]
fn two_line_angle_over_180_is_rejected() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    let b = line(&mut d, (0.0, 0.0), (0.0, 5.0));
    let before = (ln(&d, a), ln(&d, b));
    assert!(add(&mut d, dim(ConstraintKind::Angular, &[w(a), w(b)], "", "200"), &SolveOptions::default()).is_err());
    assert!(d.constraints.is_empty());
    assert_eq!((ln(&d, a), ln(&d, b)), before);
    ok(&mut d, dim(ConstraintKind::Angular, &[w(a), w(b)], "", "180"));
    assert!(all_satisfied(&d));
}

#[test]
fn dimensional_constraints() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 1.0));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Aligned), &[w(a)], "", "12"));
    let (a0, a1) = ln(&d, a);
    assert!(near(a0.dist(a1), 12.0));
    let b = line(&mut d, (0.0, 5.0), (7.0, 9.0));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Horizontal), &[w(b)], "", "4"));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Vertical), &[w(b)], "", "d1/4"));
    let (b0, b1) = ln(&d, b);
    assert!(near(b1.x - b0.x, 4.0) && near(b1.y - b0.y, 3.0));
    let c = line(&mut d, (20.0, 0.0), (30.0, 2.0));
    let e = line(&mut d, (20.0, 0.0), (25.0, 6.0));
    ok(&mut d, dim(ConstraintKind::Angular, &[w(c), w(e)], "", "45"));
    let ((c0, c1), (e0, e1)) = (ln(&d, c), ln(&d, e));
    let (u, t) = ((c1 - c0).normalized(), (e1 - e0).normalized());
    assert!(near(u.cross(t).atan2(u.dot(t)).to_degrees(), 45.0));
    let k = circle(&mut d, (0.0, 30.0), 3.0);
    ok(&mut d, dim(ConstraintKind::Radius, &[w(k)], "", "5"));
    assert!(near(circ(&d, k).1, 5.0));
    let k2 = arc(&mut d, (20.0, 30.0), 3.0, 0.0, 1.0);
    ok(&mut d, dim(ConstraintKind::Diameter, &[w(k2)], "", "rad1*2+2"));
    assert!(near(circ(&d, k2).1, 6.0));
    let names: Vec<String> = d.constraints.iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["d1", "d2", "d3", "ang1", "rad1", "dia1"]);
    // Measured default value.
    let f = line(&mut d, (0.0, 50.0), (3.0, 54.0));
    let id = ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Aligned), &[w(f)], "", ""));
    assert_eq!(d.constraints.iter().find(|c| c.id == id).unwrap().expr, "5");
    assert!(all_satisfied(&d));
}

/// A rectangle of four lines: coincident corners, horizontal/vertical sides, a fixed corner,
/// width and height parameters. Changing a parameter moves the geometry.
#[test]
fn rectangle_chain_with_parameters() {
    let mut d = Drawing::default();
    let bottom = line(&mut d, (0.0, 0.0), (10.2, 0.3));
    let right = line(&mut d, (10.0, 0.1), (9.8, 5.0));
    let top = line(&mut d, (10.1, 5.2), (0.2, 4.9));
    let left = line(&mut d, (0.0, 5.0), (0.1, -0.2));
    for (p, q) in [(bottom, right), (right, top), (top, left), (left, bottom)] {
        ok(&mut d, con(ConstraintKind::Coincident, &[s(p, Sub::End), s(q, Sub::Start)]));
    }
    ok(&mut d, con(ConstraintKind::Horizontal, &[w(bottom)]));
    ok(&mut d, con(ConstraintKind::Horizontal, &[w(top)]));
    ok(&mut d, con(ConstraintKind::Vertical, &[w(left)]));
    ok(&mut d, con(ConstraintKind::Vertical, &[w(right)]));
    ok(&mut d, con(ConstraintKind::Fix, &[s(bottom, Sub::Start)]));
    set_parameter(&mut d, "width", "10", &SolveOptions::default()).unwrap();
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Horizontal), &[w(bottom)], "w", "width"));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Vertical), &[w(right)], "h", "w/2"));
    // Equal opposite sides are implied, so adding one is redundant.
    let e = add(&mut d, con(ConstraintKind::Equal, &[w(bottom), w(top)]), &SolveOptions::default());
    assert!(matches!(e, Err(SolveError::Redundant { .. })), "{e:?}");
    let corners = |d: &Drawing| [ln(d, bottom).0, ln(d, bottom).1, ln(d, top).0, ln(d, top).1];
    let c = corners(&d);
    let o = c[0];
    assert!((c[1] - o).near(Vec2::new(10.0, 0.0), 1e-6), "{c:?}");
    assert!((c[2] - o).near(Vec2::new(10.0, 5.0), 1e-6), "{c:?}");
    assert!((c[3] - o).near(Vec2::new(0.0, 5.0), 1e-6), "{c:?}");
    set_parameter(&mut d, "width", "20", &SolveOptions::default()).unwrap();
    let c = corners(&d);
    assert!(c[0].near(o, 1e-12), "fixed corner moved");
    assert!((c[2] - o).near(Vec2::new(20.0, 10.0), 1e-6), "{c:?}");
    assert!(all_satisfied(&d));
    // A bad expression is refused and leaves the drawing alone.
    let before = d.clone();
    assert!(set_parameter(&mut d, "width", "w*2", &SolveOptions::default()).is_err());
    assert!(set_parameter(&mut d, "width", "1/0", &SolveOptions::default()).is_err());
    assert!(set_parameter(&mut d, "width", "(", &SolveOptions::default()).is_err());
    assert_eq!(d, before);
    let table = parameter_table(&d);
    assert_eq!(table.len(), 3);
    assert!(table.iter().any(|r| r.name == "h" && r.value == Some(10.0)));
}

#[test]
fn polyline_rectangle() {
    let mut d = Drawing::default();
    let vs = [(0.0, 0.0), (8.0, 0.4), (8.3, 3.0), (-0.2, 3.1)].iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect();
    let p = add_e(&mut d, EntityKind::LwPolyline(LwPolyline { vertices: vs, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }));
    for i in [0, 2] {
        ok(&mut d, con(ConstraintKind::Horizontal, &[s(p, Sub::Segment(i))]));
    }
    for i in [1, 3] {
        ok(&mut d, con(ConstraintKind::Vertical, &[s(p, Sub::Segment(i))]));
    }
    ok(&mut d, con(ConstraintKind::Fix, &[s(p, Sub::Vertex(0))]));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Aligned), &[s(p, Sub::Segment(0))], "", "6"));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Aligned), &[s(p, Sub::Segment(1))], "", "2"));
    let EntityKind::LwPolyline(pl) = &d.entity(p).unwrap().kind else { panic!() };
    let pts: Vec<Vec2> = pl.vertices.iter().map(|v| v.p).collect();
    // Vertex 0 was fixed where the earlier constraints left it.
    assert!((pts[2] - pts[0]).near(Vec2::new(6.0, 2.0), 1e-6), "{pts:?}");
}

#[test]
fn conflicts_are_reported_and_nothing_moves() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 1.0));
    let h = ok(&mut d, con(ConstraintKind::Horizontal, &[w(a)]));
    let before = d.clone();
    // Horizontal and vertical on the same (non-degenerate) line, with a length: inconsistent.
    let l = ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Aligned), &[w(a)], "", "10"));
    let before2 = d.clone();
    let e = add(&mut d, con(ConstraintKind::Vertical, &[w(a)]), &SolveOptions::default());
    match e {
        Err(SolveError::Conflict { ids }) => {
            assert_eq!(ids[0], 3);
            assert!(ids.contains(&h) || ids.contains(&l), "{ids:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(d, before2);
    // Redundant: horizontal twice.
    assert!(matches!(add(&mut d, con(ConstraintKind::Horizontal, &[w(a)]), &SolveOptions::default()), Err(SolveError::Redundant { .. })));
    // Fixed line can't be made vertical.
    let mut d2 = before.clone();
    let b = line(&mut d2, (0.0, 0.0), (3.0, 4.0));
    ok(&mut d2, con(ConstraintKind::Fix, &[w(b)]));
    assert!(matches!(add(&mut d2, con(ConstraintKind::Vertical, &[w(b)]), &SolveOptions::default()), Err(SolveError::Conflict { .. })));
    // A direct solve of an inconsistent stored system fails without writing.
    let mut d3 = before.clone();
    d3.constraints.push(Constraint { id: 9, kind: ConstraintKind::Vertical, refs: vec![w(a)], name: String::new(), expr: String::new() });
    d3.constraints.push(dim(ConstraintKind::Distance(DistAxis::Aligned), &[w(a)], "d9", "10"));
    let snapshot = d3.clone();
    assert!(matches!(solve(&mut d3, &SolveOptions::default()), Err(SolveError::Conflict { .. })));
    assert_eq!(d3, snapshot);
}

#[test]
fn keep_option_moves_the_other_entity() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    let b = line(&mut d, (0.0, 2.0), (10.0, 4.0));
    add(&mut d, con(ConstraintKind::Parallel, &[w(a), w(b)]), &SolveOptions { keep: vec![a], prefer_move: vec![b] }).unwrap();
    let (a0, a1) = ln(&d, a);
    assert!(a0.near(Vec2::new(0.0, 0.0), 1e-3) && a1.near(Vec2::new(10.0, 0.0), 1e-3), "{a0:?} {a1:?}");
}

#[test]
fn invalid_references_and_purge() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    let c = circle(&mut d, (0.0, 5.0), 1.0);
    let t = add_e(
        &mut d,
        EntityKind::Text(cadcraft_doc::Text {
            insert: v(0.0, 0.0),
            align_pt: None,
            height: 1.0,
            value: "x".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        }),
    );
    let bad = |d: &mut Drawing, c: Constraint| assert!(add(d, c, &SolveOptions::default()).is_err());
    bad(&mut d, con(ConstraintKind::Horizontal, &[w(c)]));
    bad(&mut d, con(ConstraintKind::Parallel, &[w(a)]));
    bad(&mut d, con(ConstraintKind::Horizontal, &[w(t)]));
    bad(&mut d, con(ConstraintKind::Horizontal, &[w(Handle(0xFFFF))]));
    bad(&mut d, con(ConstraintKind::Coincident, &[s(a, Sub::Vertex(99)), s(a, Sub::Start)]));
    bad(&mut d, con(ConstraintKind::Radius, &[w(a)]));
    bad(&mut d, dim(ConstraintKind::Radius, &[w(c)], "", "-3"));
    bad(&mut d, dim(ConstraintKind::Radius, &[w(c)], "1bad", "3"));
    bad(&mut d, dim(ConstraintKind::Radius, &[w(c)], "", "nope*2"));
    bad(&mut d, con(ConstraintKind::Symmetric, &[w(a), w(c)]));
    bad(&mut d, con(ConstraintKind::Fix, &[]));
    assert!(d.constraints.is_empty());
    ok(&mut d, con(ConstraintKind::Horizontal, &[w(a)]));
    d.remove_entity(a);
    assert_eq!(purge_invalid(&mut d), vec![1]);
    assert!(d.constraints.is_empty());
}

#[test]
fn hostile_geometry_never_panics() {
    let mut d = Drawing::default();
    let zero = line(&mut d, (1.0, 1.0), (1.0, 1.0));
    let huge = line(&mut d, (1e300, -1e300), (-1e300, 1e300));
    let nan = line(&mut d, (f64::NAN, 0.0), (1.0, f64::INFINITY));
    let c0 = circle(&mut d, (0.0, 0.0), 0.0);
    let kinds = [
        ConstraintKind::Coincident,
        ConstraintKind::Collinear,
        ConstraintKind::Concentric,
        ConstraintKind::Fix,
        ConstraintKind::Parallel,
        ConstraintKind::Perpendicular,
        ConstraintKind::Horizontal,
        ConstraintKind::Vertical,
        ConstraintKind::Tangent,
        ConstraintKind::Smooth,
        ConstraintKind::Symmetric,
        ConstraintKind::Equal,
        ConstraintKind::Distance(DistAxis::Aligned),
        ConstraintKind::Distance(DistAxis::Horizontal),
        ConstraintKind::Angular,
        ConstraintKind::Radius,
        ConstraintKind::Diameter,
    ];
    let hs = [zero, huge, nan, c0, Handle(0), Handle(u64::MAX)];
    let subs = [Sub::Whole, Sub::Start, Sub::Mid, Sub::Center, Sub::Vertex(u32::MAX), Sub::Segment(u32::MAX)];
    for k in kinds {
        for (i, h) in hs.iter().enumerate() {
            for sub in subs {
                let refs = [s(*h, sub), w(hs[(i + 1) % hs.len()]), w(hs[(i + 2) % hs.len()])];
                for n in 0..=3 {
                    let _ = add(&mut d, dim(k, &refs[..n], "", "1e308"), &SolveOptions::default());
                    let _ = add(&mut d, dim(k, &refs[..n], "", ""), &SolveOptions::default());
                }
            }
        }
    }
    let _ = solve(&mut d, &SolveOptions::default());
    let _ = glyphs(&d);
    let _ = residuals(&d);
    let _ = parameter_table(&d);
    let _ = purge_invalid(&mut d);
}

#[test]
fn glyphs_and_residuals() {
    let mut d = Drawing::default();
    let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    ok(&mut d, con(ConstraintKind::Horizontal, &[w(a)]));
    let g = glyphs(&d);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].kind, "Horizontal");
    assert!(g[0].satisfied);
    assert!(g[0].anchors[0].near(Vec2::new(5.0, 0.0), 1e-12));
    d.modify_entity(a, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.y = 3.0;
        }
    })
    .unwrap();
    assert!(!all_satisfied(&d));
    let r = solve(&mut d, &SolveOptions { keep: vec![], prefer_move: vec![] }).unwrap();
    assert_eq!(r.moved, vec![a]);
    assert!(all_satisfied(&d));
}

#[test]
fn autoconstrain_rectangle_of_lines() {
    let mut d = Drawing::default();
    let hs = vec![
        line(&mut d, (0.0, 0.0), (10.0, 0.01)),
        line(&mut d, (10.01, 0.0), (10.0, 5.0)),
        line(&mut d, (10.0, 5.0), (0.0, 5.02)),
        line(&mut d, (0.0, 5.0), (0.0, 0.0)),
    ];
    let ids = autoconstrain(&mut d, &hs, &InferOptions::default());
    let kinds: Vec<&str> = d.constraints.iter().map(|c| c.kind.name()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Coincident").count(), 4, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == "Horizontal").count(), 2, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == "Vertical").count(), 2, "{kinds:?}");
    // Perpendicular/parallel/equal are implied and were skipped as redundant.
    assert_eq!(ids.len(), 8, "{kinds:?}");
    assert!(all_satisfied(&d));
    let (a, b) = ln(&d, hs[0]);
    assert!(near(a.y, b.y));
    // Circles: concentric and equal.
    let c1 = circle(&mut d, (30.0, 0.0), 2.0);
    let c2 = circle(&mut d, (30.01, 0.0), 2.01);
    autoconstrain(&mut d, &[c1, c2], &InferOptions::default());
    assert!(circ(&d, c1).0.near(circ(&d, c2).0, 1e-7) && near(circ(&d, c1).1, circ(&d, c2).1));
    // Type filter.
    let l = line(&mut d, (50.0, 0.0), (60.0, 0.001));
    let only_v = InferOptions { types: vec!["Vertical".into()], ..Default::default() };
    assert!(autoconstrain(&mut d, &[l], &only_v).is_empty());
}

#[test]
fn set_parameter_rejects_out_of_domain_dimension() {
    let opts = SolveOptions::default();
    let mut d = Drawing::default();
    let l = line(&mut d, (0.0, 0.0), (10.0, 0.0));
    ok(&mut d, dim(ConstraintKind::Distance(DistAxis::Aligned), &[w(l)], "d1", "8"));
    let c = circle(&mut d, (0.0, 20.0), 5.0);
    ok(&mut d, dim(ConstraintKind::Radius, &[w(c)], "rad1", "3"));
    let before = d.clone();
    assert!(set_parameter(&mut d, "d1", "-5", &opts).is_err());
    assert!(set_parameter(&mut d, "rad1", "0", &opts).is_err());
    assert_eq!(d, before);
    set_parameter(&mut d, "d1", "6", &opts).unwrap();
    set_parameter(&mut d, "rad1", "2", &opts).unwrap();
    assert!(all_satisfied(&d));
}
