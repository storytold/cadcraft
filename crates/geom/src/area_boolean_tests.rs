use super::*;

fn rect(x: f64, y: f64, w: f64, h: f64) -> Polyline {
    Polyline {
        vertices: [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)].map(PolyVertex::new).to_vec(),
        closed: true,
    }
}

fn circle(x: f64, y: f64, r: f64) -> Polyline {
    Polyline { vertices: vec![PolyVertex::with_bulge(Vec2::new(x + r, y), 1.0), PolyVertex::with_bulge(Vec2::new(x - r, y), 1.0)], closed: true }
}

fn reverse(p: &Polyline) -> Polyline {
    let mut vertices = Vec::new();
    for i in (0..p.vertices.len()).rev() {
        let prev = (i + p.vertices.len() - 1) % p.vertices.len();
        vertices.push(PolyVertex::with_bulge(p.vertices[i].p, -p.vertices[prev].bulge));
    }
    Polyline { vertices, closed: true }
}

fn area(ps: &[Polyline]) -> f64 {
    ps.iter().map(Polyline::area).sum()
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8 * b.abs().max(1.0), "{a} != {b}");
}

#[test]
fn rectangles_cross_touch_and_coincide() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    for b in [rect(5.0, 5.0, 10.0, 10.0), rect(10.0, 0.0, 10.0, 10.0), rect(10.0, 10.0, 10.0, 10.0), a.clone(), reverse(&a)] {
        let bb = b.bounds();
        let overlap = (10.0 - bb.min.x).max(0.0) * (10.0 - bb.min.y).max(0.0);
        for (op, expected) in [(Operation::Union, 200.0 - overlap), (Operation::Difference, 100.0 - overlap), (Operation::Intersection, overlap)] {
            let ps = combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap();
            close(area(&ps), expected);
            assert_eq!(ps.is_empty(), expected == 0.0);
        }
    }
}

#[test]
fn shared_partial_edges_and_different_vertex_partitions() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    let b = rect(10.0, 2.0, 5.0, 6.0);
    close(area(&combine(std::slice::from_ref(&a), &[b], Operation::Union).unwrap()), 130.0);
    let mut b = reverse(&a);
    b.vertices.insert(1, PolyVertex::new(Vec2::new(5.0, 10.0)));
    close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), Operation::Union).unwrap()), 100.0);
    assert!(combine(&[a], &[b], Operation::Difference).unwrap().is_empty());
}

#[test]
fn circular_lens_keeps_exact_arcs_in_both_directions() {
    let a = circle(0.0, 0.0, 5.0);
    let lens = 50.0 * 0.6_f64.acos() - 24.0;
    for b in [circle(6.0, 0.0, 5.0), reverse(&circle(6.0, 0.0, 5.0))] {
        for (op, expected) in [(Operation::Intersection, lens), (Operation::Union, 50.0 * PI - lens), (Operation::Difference, 25.0 * PI - lens)] {
            let ps = combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap();
            close(area(&ps), expected);
            assert!(ps.iter().flat_map(|p| &p.vertices).all(|v| v.bulge != 0.0));
            assert!(ps.iter().map(|p| p.vertices.len()).sum::<usize>() <= 6);
        }
    }
}

#[test]
fn circular_segment_crossings_are_analytic() {
    let a = circle(0.0, 0.0, 5.0);
    let b = rect(3.0, -10.0, 10.0, 20.0);
    let ps = combine(&[a], &[b], Operation::Intersection).unwrap();
    close(area(&ps), 25.0 * 0.6_f64.acos() - 12.0);
    assert_eq!(ps.len(), 1);
    let vs = &ps[0].vertices;
    assert!(vs.iter().any(|v| v.p.near(Vec2::new(3.0, 4.0), 1e-9)));
    assert!(vs.iter().any(|v| v.p.near(Vec2::new(3.0, -4.0), 1e-9)));
    assert!(vs.iter().any(|v| v.bulge != 0.0));
}

#[test]
fn tangent_circles_and_coincident_arcs() {
    let a = circle(0.0, 0.0, 5.0);
    for b in [circle(10.0, 0.0, 5.0), reverse(&circle(10.0, 0.0, 5.0))] {
        let ps = combine(std::slice::from_ref(&a), std::slice::from_ref(&b), Operation::Union).unwrap();
        close(area(&ps), 50.0 * PI);
        assert_eq!(ps.len(), 2);
        assert!(combine(std::slice::from_ref(&a), &[b], Operation::Intersection).unwrap().is_empty());
    }
    let b = Polyline {
        vertices: (0..4).map(|i| PolyVertex::with_bulge(Vec2::polar(Vec2::ZERO, 5.0, i as f64 * PI / 2.0), (PI / 8.0).tan())).collect(),
        closed: true,
    };
    close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), Operation::Union).unwrap()), 25.0 * PI);
    assert!(combine(&[a], &[b], Operation::Difference).unwrap().is_empty());
}

#[test]
fn cut_holes_islands_and_disconnected_results() {
    let outer = rect(0.0, 0.0, 10.0, 10.0);
    let hole = reverse(&rect(2.0, 2.0, 6.0, 6.0));
    let a = vec![outer, hole];
    let strip = rect(4.0, -1.0, 2.0, 12.0);
    let cut = combine(&a, &[strip], Operation::Difference).unwrap();
    close(area(&cut), 56.0);
    assert_eq!(cut.len(), 2);
    assert!(cut.iter().all(|p| p.area() > 0.0));
    let inside = circle(5.0, 5.0, 1.0);
    close(area(&combine(&a, &[inside], Operation::Union).unwrap()), 64.0 + PI);
}

#[test]
fn translated_and_scaled_known_answers() {
    for (origin, scale) in [(1e7, 1.0), (0.0, 1e-6), (0.0, 1e50)] {
        let a = rect(origin, origin, 10.0 * scale, 10.0 * scale);
        let b = rect(origin + 5.0 * scale, origin + 5.0 * scale, 10.0 * scale, 10.0 * scale);
        close(area(&combine(&[a], &[b], Operation::Union).unwrap()) / scale / scale, 175.0);
    }
}

#[test]
fn circle_pairs_at_many_angles_match_the_lens_formula() {
    for i in 0..24 {
        let angle = i as f64 * TAU / 24.0;
        for distance in [0.0, 1.0, 2.0, 6.0, 8.0, 9.0] {
            let a = circle(0.0, 0.0, 5.0);
            let c = Vec2::from_angle(angle) * distance;
            let b = circle(c.x, c.y, 3.0);
            let lens = if distance <= 2.0 {
                9.0 * PI
            } else if distance >= 8.0 {
                0.0
            } else {
                let x = (distance * distance + 16.0) / (2.0 * distance);
                25.0 * (x / 5.0).acos() + 9.0 * ((distance - x) / 3.0).acos() - distance * (25.0 - x * x).sqrt()
            };
            for (op, expected) in [(Operation::Intersection, lens), (Operation::Union, 34.0 * PI - lens), (Operation::Difference, 25.0 * PI - lens)] {
                let ps = combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op)
                    .unwrap_or_else(|e| panic!("angle {angle}, distance {distance}, {op:?}: {e}"));
                close(area(&ps), expected);
            }
        }
    }
}

#[test]
fn a_circle_touches_a_straight_side_at_arbitrary_angles() {
    for i in 0..24 {
        let angle = i as f64 * TAU / 24.0;
        let a = circle(0.0, 0.0, 5.0);
        let mut b = rect(5.0, -2.0, 10.0, 4.0);
        for v in &mut b.vertices {
            v.p = v.p.rotate(angle);
        }
        for (op, expected) in [(Operation::Union, 25.0 * PI + 40.0), (Operation::Intersection, 0.0), (Operation::Difference, 25.0 * PI)] {
            let ps = combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap_or_else(|e| panic!("angle {angle}, {op:?}: {e}"));
            close(area(&ps), expected);
        }
    }
}

#[test]
fn concave_boundaries_and_major_arcs() {
    let l = Polyline {
        vertices: [(0.0, 0.0), (6.0, 0.0), (6.0, 2.0), (2.0, 2.0), (2.0, 6.0), (0.0, 6.0)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).to_vec(),
        closed: true,
    };
    let square = rect(1.0, 1.0, 4.0, 4.0);
    for (op, expected) in [(Operation::Union, 29.0), (Operation::Intersection, 7.0), (Operation::Difference, 13.0)] {
        close(area(&combine(std::slice::from_ref(&l), std::slice::from_ref(&square), op).unwrap()), expected);
    }
    // A 270-degree arc closed by its chord, clipped to the upper half-plane.
    let major = Polyline {
        vertices: vec![PolyVertex::with_bulge(Vec2::new(5.0, 0.0), (3.0 * PI / 8.0).tan()), PolyVertex::new(Vec2::new(0.0, -5.0))],
        closed: true,
    };
    let clip = rect(-10.0, 0.0, 20.0, 10.0);
    close(area(&combine(&[major], &[clip], Operation::Intersection).unwrap()), 12.5 * PI);
}

#[test]
fn many_known_rectangle_answers() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    for i in -4..=12 {
        for j in -4..=12 {
            let (x, y) = (i as f64 * 0.9, j as f64 * 0.9);
            let b = rect(x, y, 3.0, 4.0);
            let overlap = (10.0_f64.min(x + 3.0) - 0.0_f64.max(x)).max(0.0) * (10.0_f64.min(y + 4.0) - 0.0_f64.max(y)).max(0.0);
            for (op, expected) in [(Operation::Union, 112.0 - overlap), (Operation::Difference, 100.0 - overlap), (Operation::Intersection, overlap)]
            {
                close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap()), expected);
            }
        }
    }
}

#[test]
fn hostile_inputs_and_work_limits_return_errors() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut b = a.clone();
        b.vertices[0].p.x = n;
        assert!(combine(std::slice::from_ref(&a), &[b], Operation::Union).is_err());
        let mut b = a.clone();
        b.vertices[0].bulge = n;
        assert!(combine(std::slice::from_ref(&a), &[b], Operation::Union).is_err());
    }
    let too_many = Polyline { vertices: vec![PolyVertex::new(Vec2::ZERO); MAX_EDGES + 1], closed: true };
    assert_eq!(combine(&[too_many], &[], Operation::Union), Err(COMPLEX));
    // A modest-size pair whose crossings/classification could otherwise grow quadratically.
    let many: Vec<_> = (0..600).map(|i| rect(i as f64 * 2.0, 0.0, 1.0, 1.0)).collect();
    assert_eq!(combine(&many, &many, Operation::Union), Err(COMPLEX));
}
