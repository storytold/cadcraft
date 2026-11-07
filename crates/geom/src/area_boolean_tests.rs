use super::*;

/// Make a counter-clockwise rectangle with a known analytic area.
fn rect(x: f64, y: f64, w: f64, h: f64) -> Polyline {
    Polyline {
        vertices: [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)].map(PolyVertex::new).to_vec(),
        closed: true,
    }
}

/// Represent a circle exactly with two semicircular bulge edges.
fn circle(x: f64, y: f64, r: f64) -> Polyline {
    Polyline { vertices: vec![PolyVertex::with_bulge(Vec2::new(x + r, y), 1.0), PolyVertex::with_bulge(Vec2::new(x - r, y), 1.0)], closed: true }
}

/// Reverse a closed loop and its bulges without changing the represented area.
fn reverse(p: &Polyline) -> Polyline {
    let mut vertices = Vec::new();
    for i in (0..p.vertices.len()).rev() {
        let prev = (i + p.vertices.len() - 1) % p.vertices.len();
        vertices.push(PolyVertex::with_bulge(p.vertices[i].p, -p.vertices[prev].bulge));
    }
    Polyline { vertices, closed: true }
}

/// Signed sum of result loops: outer boundaries add area and holes subtract it.
fn area(ps: &[Polyline]) -> f64 {
    ps.iter().map(Polyline::area).sum()
}

/// Compare areas with a relative tolerance and a small absolute floor.
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8 * b.abs().max(1.0), "{a} != {b}");
}

/// Boolean areas match analytic rectangle overlaps, including shared sides and corners.
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

/// Coincident boundaries deduplicate even when only part of an edge is shared.
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

/// Circle intersections match the analytic lens area with either input winding.
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

/// Clipping a circle preserves its arc and the exact line/circle crossing coordinates.
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

/// Point contacts retain separate filled components; coincident circular boundaries cancel.
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

/// Boolean operations respect hole parity and can leave several disconnected outer loops.
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

/// Normalization keeps the same known area at remote coordinates and different scales.
#[test]
fn translated_and_scaled_known_answers() {
    for (origin, scale) in [(1e7, 1.0), (0.0, 1e-6), (0.0, 1e50)] {
        let a = rect(origin, origin, 10.0 * scale, 10.0 * scale);
        let b = rect(origin + 5.0 * scale, origin + 5.0 * scale, 10.0 * scale, 10.0 * scale);
        close(area(&combine(&[a], &[b], Operation::Union).unwrap()) / scale / scale, 175.0);
    }
}

/// Rotated circle pairs satisfy the analytic lens formula through containment and tangency.
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

/// Rotating a circle/line tangency must neither open the result nor create spurious area.
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

/// Concave outlines and arcs larger than a semicircle retain their known clipped areas.
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

/// Sweep rectangle placements through crossings and contacts with analytic overlap areas.
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

/// Invalid numbers and excessive input or geometric work fail without panicking.
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

/// A near-straight arc can overlap a thin strip even when circle arithmetic misses the
/// crossings. Reject unresolved geometry instead of reporting an empty intersection.
#[test]
fn ill_conditioned_arcs_return_precision_before_classification() {
    for bulge in [1e-6, -1e-6, 1e-8, -1e-8, 1e-9, -1e-9] {
        let a = Polyline {
            vertices: vec![
                PolyVertex::with_bulge(Vec2::new(0.0, 0.5), bulge),
                PolyVertex::new(Vec2::new(1.0, 0.5)),
                PolyVertex::new(Vec2::new(1.0, 1.0)),
                PolyVertex::new(Vec2::new(0.0, 1.0)),
            ],
            closed: true,
        };
        let b = rect(0.01, 0.0, 0.1, 0.5 - 0.15 * bulge.abs());
        for op in [Operation::Intersection, Operation::Union, Operation::Difference] {
            assert_eq!(combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op), Err(PRECISION), "{bulge}, {op:?}");
        }
    }
}

/// A shallow arc whose center and radius are still well conditioned must keep its bulge and
/// the known circular-segment area, rather than being rejected or straightened.
#[test]
fn a_resolvable_shallow_arc_stays_curved() {
    let bulge = 1e-3;
    let a = Polyline {
        vertices: vec![
            PolyVertex::with_bulge(Vec2::new(0.0, 0.5), bulge),
            PolyVertex::new(Vec2::new(1.0, 0.5)),
            PolyVertex::new(Vec2::new(1.0, 1.0)),
            PolyVertex::new(Vec2::new(0.0, 1.0)),
        ],
        closed: true,
    };
    let ps = combine(&[a], &[rect(-0.1, 0.0, 1.2, 0.5)], Operation::Intersection).unwrap();
    assert_eq!(ps.len(), 1);
    assert!(ps[0].vertices.iter().any(|v| v.bulge != 0.0));
    // Circular-segment area for a unit chord: b/3 + b^3/15 + O(b^5).
    let expected = bulge / 3.0 + bulge.powi(3) / 15.0;
    assert!((area(&ps) - expected).abs() < 1e-10, "{} != {expected}", area(&ps));
}

/// Imported contours routinely contain thousands of vertices, even without curved edges.
fn polygon_circle(x: f64, n: usize) -> Polyline {
    Polyline { vertices: (0..n).map(|i| PolyVertex::new(Vec2::polar(Vec2::new(x, 0.0), 5.0, TAU * i as f64 / n as f64))).collect(), closed: true }
}

/// Uncrossed loops require one classification each rather than a quadratic number of probes.
#[test]
fn large_disjoint_contours_keep_their_known_areas() {
    for n in [1000, 2000, 6000] {
        let a = polygon_circle(0.0, n);
        let b = polygon_circle(20.0, n);
        let expected = n as f64 * 12.5 * (TAU / n as f64).sin();
        for (op, want) in [(Operation::Union, 2.0 * expected), (Operation::Difference, expected), (Operation::Intersection, 0.0)] {
            close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap()), want);
        }
    }
}

/// Two crossings partition a large contour into a few runs, regardless of original vertices.
#[test]
fn a_large_crossed_contour_has_the_exact_polygon_half_area() {
    let n = 2000;
    let a = polygon_circle(0.0, n);
    let b = rect(0.0, -10.0, 10.0, 20.0);
    let half = n as f64 * 6.25 * (TAU / n as f64).sin();
    for (op, want) in [(Operation::Union, 200.0 + half), (Operation::Difference, half), (Operation::Intersection, half)] {
        close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap()), want);
    }
}

/// An unrelated flat arc must not fail either an uncrossed loop or an operation elsewhere.
#[test]
fn shallow_arcs_away_from_crossings_do_not_fail_the_boolean() {
    for bulge in [1e-6, -1e-6, 1e-8, -1e-8, 1e-10, -1e-10] {
        let mut a = rect(0.0, 0.0, 10.0, 10.0);
        a.vertices[0].bulge = bulge;
        let curved_area = 100.0 + 100.0 * (bulge / 3.0 + bulge.powi(3) / 15.0);
        for (b, overlap) in [(rect(20.0, 0.0, 10.0, 10.0), 0.0), (rect(5.0, 5.0, 10.0, 10.0), 25.0), (rect(2.0, 2.0, 2.0, 2.0), 4.0)] {
            for (op, want) in [
                (Operation::Union, curved_area + b.area() - overlap),
                (Operation::Difference, curved_area - overlap),
                (Operation::Intersection, overlap),
            ] {
                let ps = combine(std::slice::from_ref(&a), std::slice::from_ref(&b), op).unwrap();
                close(area(&ps), want);
                if matches!(op, Operation::Union) {
                    assert_eq!(ps.iter().flat_map(|p| &p.vertices).any(|v| v.bulge != 0.0), bulge.abs() >= 1e-8);
                }
            }
        }
    }
}

/// Rotating nested loops must not make an unrelated flat edge appear to intersect their
/// boundaries merely because conservative axis-aligned boxes overlap.
#[test]
fn shallow_arcs_in_rotated_nested_loops_still_work() {
    for angle in [0.37, PI * 0.5, 2.2] {
        let mut a = rect(0.0, 0.0, 10.0, 10.0);
        a.vertices[0].bulge = 1e-8;
        let mut b = rect(2.0, 2.0, 2.0, 2.0);
        for p in [&mut a, &mut b] {
            for v in &mut p.vertices {
                v.p = v.p.rotate(angle);
            }
        }
        for a in [a.clone(), reverse(&a)] {
            close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), Operation::Difference).unwrap()), 96.0 + 1e-6 / 3.0);
            close(area(&combine(std::slice::from_ref(&a), std::slice::from_ref(&b), Operation::Intersection).unwrap()), 4.0);
        }
    }
}
