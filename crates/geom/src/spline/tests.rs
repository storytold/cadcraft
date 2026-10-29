use super::*;

/// `basis` before it evaluated on the stack, kept as the reference.
fn basis_ref(span: usize, u: f64, p: usize, knots: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    n[0] = 1.0;
    for j in 1..=p {
        let k = |i: isize| knots.get(i.max(0) as usize).copied().unwrap_or(0.0);
        left[j] = u - k(span as isize + 1 - j as isize);
        right[j] = k(span as isize + j as isize) - u;
        let mut saved = 0.0;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let tmp = if den.abs() > 1e-300 { n[r] / den } else { 0.0 };
            n[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        n[j] = saved;
    }
    n
}

/// `eval` before it evaluated on the stack, kept as the reference.
fn eval_ref(s: &Spline, t: f64) -> Vec2 {
    let n = s.control.len() - 1;
    let (lo, hi) = s.domain();
    let t = t.clamp(lo, hi);
    let span = find_span(n, s.degree, t, &s.knots);
    let b = basis_ref(span, t, s.degree, &s.knots);
    let mut p = Vec2::ZERO;
    let mut wsum = 0.0;
    for (k, nb) in b.iter().enumerate() {
        let idx = span + k - s.degree;
        let w = if s.weights.is_empty() { 1.0 } else { s.weights[idx] };
        p += s.control[idx] * (nb * w);
        wsum += nb * w;
    }
    if wsum.abs() > 1e-300 { p / wsum } else { p }
}

/// Deterministic pseudo-random numbers in `0..1`.
fn noise(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// Splines of degree 1..=11 over 14 control points: clamped and unclamped (uniform) knots,
/// unit and random weights.
fn samples() -> Vec<Spline> {
    let mut seed = 7;
    let mut out = Vec::new();
    for degree in 1..=11 {
        let control: Vec<Vec2> = (0..14).map(|i| Vec2::new(i as f64 * 3.0 + noise(&mut seed), 10.0 * noise(&mut seed) - 5.0)).collect();
        let weights: Vec<f64> = (0..14).map(|_| 0.5 + 1.5 * noise(&mut seed)).collect();
        let clamped = Spline::from_control(control.clone(), degree);
        let uniform = Spline { knots: (0..control.len() + degree + 1).map(|i| i as f64 * 0.25).collect(), ..clamped.clone() };
        for base in [clamped, uniform] {
            out.push(Spline { weights: weights.clone(), ..base.clone() });
            out.push(base);
        }
    }
    out
}

#[test]
fn stack_basis_matches_the_heap_basis_bit_for_bit() {
    for s in samples() {
        assert!(s.is_valid());
        let (lo, hi) = s.domain();
        for i in -20..=1020 {
            let t = lo + (hi - lo) * i as f64 / 1000.0;
            let got = s.eval(t);
            let want = eval_ref(&s, t);
            assert_eq!((got.x.to_bits(), got.y.to_bits()), (want.x.to_bits(), want.y.to_bits()), "degree {} at {t}: {got:?} vs {want:?}", s.degree);
            let span = find_span(s.control.len() - 1, s.degree, t.clamp(lo, hi), &s.knots);
            let b = basis(span, t.clamp(lo, hi), s.degree, &s.knots);
            let r = basis_ref(span, t.clamp(lo, hi), s.degree, &s.knots);
            assert!(b.iter().zip(&r).all(|(a, b)| a.to_bits() == b.to_bits()) && b.len() == r.len(), "degree {}", s.degree);
        }
    }
}

#[test]
fn basis_into_needs_room_for_every_function() {
    let knots = clamped_uniform_knots(6, 3);
    let (mut n, mut left, mut right) = ([0.5; 3], [0.0; 4], [0.0; 4]);
    basis_into(3, 0.5, 3, &knots, &mut n, &mut left, &mut right);
    assert_eq!(n, [0.5; 3]);
}

/// A cubic through 300 points of a sine wave, open or closed.
fn wiggly(closed: bool) -> Spline {
    let fit: Vec<Vec2> = (0..300)
        .map(|i| {
            let x = i as f64 / 299.0;
            Vec2::new(x * 100.0, 10.0 * (x * 60.0).sin())
        })
        .collect();
    Spline::from_fit(&fit, closed)
}

/// The box from 4000 evaluations, which `bounds` approximates.
fn fine_box(s: &Spline) -> Bounds2 {
    uniform_box(s, MAX_SAMPLES)
}

/// The box of `n + 1` samples evenly spaced in parameter over the domain, as `bounds` sampled
/// before it spread its samples over the knot spans.
fn uniform_box(s: &Spline, n: usize) -> Bounds2 {
    let (lo, hi) = s.domain();
    Bounds2::from_points((0..=n).map(|i| s.eval(lo + (hi - lo) * i as f64 / n as f64)))
}

/// A degree-2 spline whose fourth control point, the only one off the x axis, is supported
/// on the single knot span [0.5003, 0.5004) (double knots either side): the curve is flat
/// except for a peak of 50 at t = 0.50035, inside a span of width 1e-4 that no point of a
/// uniform grid of up to 4000 hits (0.50025 and 0.5005 are the nearest).
fn narrow_span() -> Spline {
    let control = (0..7).map(|i| Vec2::new(i as f64, if i == 3 { 100.0 } else { 0.0 })).collect();
    Spline { degree: 2, knots: vec![0.0, 0.0, 0.0, 0.5003, 0.5003, 0.5004, 0.5004, 1.0, 1.0, 1.0], control, ..Spline::default() }
}

/// A span held to the floor samples its peak off the grid: the sag stays under 1% of the
/// spline's size, which is what `bounds` promises for it.
#[test]
fn an_off_grid_peak_in_a_narrow_span_is_within_a_percent() {
    let mut s = narrow_span();
    // y(s) = 200 s (1 - s) + 30 s² on the narrow span: its maximum is at s = 10 / 17, between
    // the samples of an eight-segment grid.
    s.control[4].y = 30.0;
    assert!(s.is_valid());
    let (a, b) = (0.5003, 0.5004);
    let peak = (0..=10_000).map(|i| s.eval(a + (b - a) * i as f64 / 10_000.0)).fold(Vec2::ZERO, |m, p| if p.y > m.y { p } else { m });
    assert!(peak.y > 58.0 && peak.y < 59.0, "{peak:?}");
    let hull = Bounds2::from_points(s.control.iter().copied());
    let diag = hull.min.dist(hull.max);
    let got = s.bounds();
    assert!(got.max.y <= peak.y + 1e-9 * diag && got.max.y >= peak.y - 1e-2 * diag, "{got:?} vs peak {peak:?}");
    // Four segments, the floor before, missed it by more than that.
    let four = (0..=4).map(|i| s.eval(a + (b - a) * i as f64 / 4.0)).fold(f64::MIN, |m, p| m.max(p.y));
    assert!(four < peak.y - 1e-2 * diag, "{four} vs {peak:?}");
}

#[test]
fn bounds_see_an_extreme_inside_a_narrow_knot_span() {
    let s = narrow_span();
    assert!(s.is_valid());
    let (a, b) = (0.5003, 0.5004);
    let peak = (0..=1000).map(|i| s.eval(a + (b - a) * i as f64 / 1000.0)).fold(Vec2::ZERO, |m, p| if p.y > m.y { p } else { m });
    assert!((peak.x - 3.0).abs() < 1e-9 && (peak.y - 50.0).abs() < 1e-9, "{peak:?}");
    let hull = Bounds2::from_points(s.control.iter().copied());
    let diag = hull.min.dist(hull.max);
    // One uniform grid, at the count `bounds` used to take or at the cap, steps over the span
    // and sees only the flat parts of the curve.
    let old = s.segments_for(s.polygon_length() / (diag * 1e-3)).max((s.control.len() - s.degree) * 4).min(MAX_SAMPLES);
    for n in [old, MAX_SAMPLES] {
        let grid = uniform_box(&s, n);
        assert!(grid.max.y < 1e-9 && !grid.contains(peak), "{n} segments: {grid:?}");
    }
    let got = s.bounds();
    assert!(got.expand(1e-9 * diag).contains(peak), "{got:?} misses {peak:?}");
    assert!((got.max.y - peak.y).abs() <= 1e-9 * diag, "{got:?}");
}

#[test]
fn bounds_keep_their_cap_over_many_spans() {
    // 1500 spans: eight per span would pass the cap, so two; 2500 spans: more than the cap
    // allows two for, so the control box, which contains the curve. A smooth curve stays
    // within the margin either way.
    for n in [1502, 2502] {
        let control: Vec<Vec2> = (0..n).map(|i| Vec2::from_angle(i as f64 / n as f64 * std::f64::consts::TAU) * 50.0).collect();
        let hull = Bounds2::from_points(control.iter().copied());
        let s = Spline::from_control(control, 2);
        let (b, want) = (s.bounds(), fine_box(&s));
        if n > MAX_SAMPLES / 2 {
            assert_eq!(b, hull, "{n} control points");
            assert!(b.contains(want.min) && b.contains(want.max), "{b:?} vs {want:?}");
        }
        let diag = want.min.dist(want.max);
        let err =
            [b.min.x - want.min.x, b.min.y - want.min.y, b.max.x - want.max.x, b.max.y - want.max.y].into_iter().map(f64::abs).fold(0.0, f64::max);
        assert!(err <= 1e-3 * diag, "{n} control points: {b:?} vs {want:?}");
    }
}

#[test]
fn bounds_keep_their_cap_over_huge_knot_spans() {
    // Knots a hostile edit can produce (lengthening by 1e308): a span's count must not
    // overflow into a boundless loop or a sum that panics.
    let control = vec![Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 0.0), Vec2::new(4.0, 1.0)];
    for knots in [
        vec![0.0, 0.0, 0.0, 1e308, 1.5e308, 1.5e308, 1.5e308],
        vec![0.0, 0.0, 0.0, f64::MAX, f64::MAX, f64::MAX, f64::MAX],
        vec![-1e308, -1e308, -1e308, 0.0, 1e308, 1e308, 1e308],
    ] {
        let s = Spline { knots, ..Spline::from_control(control.clone(), 2) };
        assert!(s.is_valid());
        let b = s.bounds();
        assert!(b.is_empty() || Bounds2::from_points(control.iter().copied()).expand(1e-9).contains_box(&b), "{b:?}");
    }
}

fn scaled(s: &Spline, k: f64) -> Spline {
    Spline { control: s.control.iter().map(|c| *c * k).collect(), fit: s.fit.iter().map(|c| *c * k).collect(), ..s.clone() }
}

#[test]
fn bounds_stay_within_a_small_margin_of_the_finely_sampled_box() {
    let mut all = samples();
    all.push(wiggly(false));
    all.push(wiggly(true));
    all.push(Spline::from_fit_points_closed(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]));
    for s in all {
        // Whatever the drawing's units: the same relative margin at any scale, including
        // splines far smaller than any absolute tolerance floor.
        for k in [1e-12, 1e-9, 1e-6, 1.0, 1e6] {
            let s = scaled(&s, k);
            let (b, want) = (s.bounds(), fine_box(&s));
            let diag = want.min.dist(want.max);
            let err = [b.min.x - want.min.x, b.min.y - want.min.y, b.max.x - want.max.x, b.max.y - want.max.y]
                .into_iter()
                .map(f64::abs)
                .fold(0.0, f64::max);
            assert!(err <= 1e-3 * diag, "degree {} with {} control points at scale {k}: {b:?} vs {want:?}", s.degree, s.control.len());
            // The curve lies in its control points' convex hull (positive weights), up to rounding.
            assert!(Bounds2::from_points(s.control.iter().copied()).expand(1e-9 * diag).contains_box(&b), "degree {} at scale {k}", s.degree);
        }
    }
}

/// A negative weight voids the convex hull property: a degree-1 spline is then not its
/// control polygon, so it is sampled like any other.
#[test]
fn a_degree_one_spline_with_a_negative_weight_is_sampled() {
    let mut s = Spline::from_control(vec![Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)], 1);
    s.weights = vec![1.0, -0.5];
    assert!(s.is_valid());
    let far = s.eval(0.75);
    assert!((far.x - 4.0).abs() < 1e-9, "{far:?}");
    let b = s.bounds();
    assert!(b.contains(far), "{b:?} misses {far:?}");
    s.weights = vec![1.0, 0.5];
    assert_eq!(s.bounds(), Bounds2::new(Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)));
}

/// Zero weights evaluate to the origin, so a spline whose control points coincide is not its
/// control box either; it is sampled.
#[test]
fn zero_weights_are_sampled_not_boxed() {
    let mut s = Spline::from_control(vec![Vec2::new(3.0, 4.0); 2], 1);
    s.weights = vec![0.0, 0.0];
    assert!(s.is_valid());
    assert_eq!(s.eval(0.5), Vec2::ZERO);
    let b = s.bounds();
    assert!(b.contains(Vec2::ZERO), "{b:?}");
}

#[test]
fn degenerate_splines_get_their_control_box() {
    let point = Spline::from_control(vec![Vec2::new(3.0, 4.0); 4], 3);
    assert_eq!(point.bounds(), Bounds2::new(Vec2::new(3.0, 4.0), Vec2::new(3.0, 4.0)));
    let invalid = Spline { knots: vec![0.0; 3], ..Spline::from_control(vec![Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(5.0, -1.0)], 2) };
    assert!(!invalid.is_valid());
    assert_eq!(invalid.bounds(), Bounds2::new(Vec2::new(0.0, -1.0), Vec2::new(5.0, 2.0)));
    let hostile = Spline::from_control(vec![Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, f64::INFINITY), Vec2::new(2.0, 2.0)], 2);
    assert_eq!(hostile.bounds(), Bounds2::new(Vec2::new(2.0, 2.0), Vec2::new(2.0, 2.0)));
    // A non-finite control point among finite ones poisons every sample: the box of the
    // finite control points, not the empty box.
    for bad in [Vec2::new(f64::NAN, 5.0), Vec2::new(5.0, f64::NAN), Vec2::new(f64::INFINITY, 5.0), Vec2::new(5.0, f64::NEG_INFINITY)] {
        let s = Spline::from_control(vec![Vec2::ZERO, bad, Vec2::new(10.0, 10.0), Vec2::new(20.0, 0.0)], 3);
        assert!(s.is_valid());
        assert_eq!(s.bounds(), Bounds2::new(Vec2::ZERO, Vec2::new(20.0, 10.0)), "{bad:?}");
    }
    // A non-finite weight poisons the samples the same way.
    for bad in [f64::NAN, f64::INFINITY] {
        let mut s = Spline::from_control(vec![Vec2::ZERO, Vec2::new(5.0, 5.0), Vec2::new(10.0, 10.0), Vec2::new(20.0, 0.0)], 3);
        s.weights = vec![1.0, bad, 1.0, 1.0];
        assert!(s.is_valid());
        assert_eq!(s.bounds(), Bounds2::new(Vec2::ZERO, Vec2::new(20.0, 10.0)), "weight {bad:?}");
    }
    assert!(Spline::default().bounds().is_empty());
}

#[test]
fn a_huge_degree_spline_is_invalid_and_cheap_to_tessellate() {
    // A HATCH/DXF spline edge can declare a very high degree with matching knots. Such a
    // spline is treated as invalid, so eval/tessellate fall back to the control polygon
    // instead of doing O(degree^2) work per sample (CWE-407, rows 18/28).
    let degree = 50_000usize;
    let control = vec![Vec2::ZERO; degree + 2];
    let knots = clamped_uniform_knots(control.len(), degree);
    let s = Spline { degree, knots, control: control.clone(), ..Spline::default() };
    assert!(!s.is_valid(), "a degree-{degree} spline must be rejected as invalid");
    // Returns the control polygon rather than sampling, so this is fast.
    assert_eq!(s.tessellate(1e-3).len(), control.len());
}

#[test]
fn a_normal_low_degree_spline_is_valid() {
    let s = Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 0.0), Vec2::new(3.0, 1.0)], 3);
    assert!(s.is_valid());
    assert!(s.tessellate(1e-3).len() > 4);
}
