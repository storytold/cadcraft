//! TRIM, EXTEND, JOIN and LENGTHEN on elliptical arcs and splines (#466).

use std::f64::consts::{FRAC_PI_2, PI};

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::{Ellipse, Spline, Vec2};
use serde_json::{Value, json};

fn h(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn kind(s: &Session, h: &str) -> EntityKind {
    s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind.clone()
}

fn spline(s: &Session, h: &str) -> Spline {
    match kind(s, h) {
        EntityKind::Spline(sp) => sp,
        k => panic!("not a spline: {k:?}"),
    }
}

fn ellipse(s: &Session, h: &str) -> Ellipse {
    match kind(s, h) {
        EntityKind::Ellipse(e) => Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end },
        k => panic!("not an ellipse: {k:?}"),
    }
}

fn ends(sp: &Spline) -> (Vec2, Vec2) {
    let (lo, hi) = sp.domain();
    (sp.eval(lo), sp.eval(hi))
}

fn samples(sp: &Spline, n: usize) -> Vec<Vec2> {
    let (lo, hi) = sp.domain();
    (0..=n).map(|i| sp.eval(lo + (hi - lo) * i as f64 / n as f64)).collect()
}

/// Distance from `p` to the spline (dense sampling, then a ternary search round the best sample).
fn dist_to(sp: &Spline, p: Vec2) -> f64 {
    let (lo, hi) = sp.domain();
    let n = 4000;
    let step = (hi - lo) / n as f64;
    let best = (0..=n).map(|i| lo + step * i as f64).min_by(|a, b| sp.eval(*a).dist(p).total_cmp(&sp.eval(*b).dist(p))).unwrap();
    let (mut a, mut b) = ((best - step).max(lo), (best + step).min(hi));
    for _ in 0..100 {
        let (m1, m2) = (a + (b - a) / 3.0, b - (b - a) / 3.0);
        if sp.eval(m1).dist(p) < sp.eval(m2).dist(p) {
            b = m2;
        } else {
            a = m1;
        }
    }
    sp.eval((a + b) / 2.0).dist(p)
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.iter().count()
}

fn length(s: &mut Session, h: &str) -> f64 {
    s.execute("lengthen", &json!({ "handle": h })).unwrap()["length"].as_f64().unwrap()
}

/// An S-shaped open cubic from (0,0) to (9,0); it leaves its end heading along (1,1).
fn s_curve(s: &mut Session) -> String {
    h(&s.execute("spline", &json!({ "control": [[0, 0], [3, 3], [6, -3], [9, 0]] })).unwrap())
}

// ---------------- EXTEND ----------------

#[test]
fn extend_runs_a_spline_on_along_its_end_tangent() {
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    s.execute("line", &json!({ "points": [[12, -10], [12, 20]] })).unwrap();
    let before = spline(&s, &sp);
    s.execute("extend", &json!({ "handle": sp, "pick": [8.9, 0] })).unwrap();
    let after = spline(&s, &sp);
    let (a, b) = ends(&after);
    assert!(a.near(Vec2::ZERO, 1e-9), "{a:?}");
    assert!(b.near(Vec2::new(12.0, 3.0), 1e-9), "{b:?}");
    // The original curve is still there, unchanged.
    for p in samples(&before, 20) {
        assert!(dist_to(&after, p) < 1e-9, "{p:?}");
    }
    // The start end extends backwards along its tangent, (-1,-1) here.
    s.execute("line", &json!({ "points": [[-10, -3], [10, -3]] })).unwrap();
    s.execute("extend", &json!({ "handle": sp, "pick": [0.1, 0.1] })).unwrap();
    assert!(ends(&spline(&s, &sp)).0.near(Vec2::new(-3.0, -3.0), 1e-9));
    // Nothing ahead: an error, the spline unchanged.
    let now = spline(&s, &sp);
    assert!(s.execute("extend", &json!({ "handle": sp, "pick": [11.9, 2.9] })).is_err());
    assert_eq!(spline(&s, &sp), now);
}

#[test]
fn extend_refuses_closed_splines() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({ "fit": [[0, 0], [4, 0], [4, 4], [0, 4]], "closed": true })).unwrap());
    s.execute("line", &json!({ "points": [[10, -10], [10, 10]] })).unwrap();
    assert!(s.execute("extend", &json!({ "handle": sp, "pick": [4, 2] })).is_err());
}

// ---------------- TRIM ----------------

#[test]
fn trimming_a_closed_spline_leaves_one_open_spline() {
    for (pick, right) in [([10.0, 5.0], false), ([0.0, 5.0], true)] {
        let mut s = Session::new();
        let sp = h(&s.execute("spline", &json!({ "fit": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true })).unwrap());
        s.execute("line", &json!({ "points": [[5, -5], [5, 15]] })).unwrap();
        let r = s.execute("trim", &json!({ "handle": sp, "pick": pick })).unwrap();
        let hs = r["handles"].as_array().unwrap();
        assert_eq!(hs.len(), 1, "{pick:?}: {r}");
        let piece = spline(&s, hs[0].as_str().unwrap());
        assert!(!piece.closed);
        let (a, b) = ends(&piece);
        assert!((a.x - 5.0).abs() < 1e-6 && (b.x - 5.0).abs() < 1e-6, "{a:?} {b:?}");
        // Only the half away from the pick is left.
        for p in samples(&piece, 100) {
            if right {
                assert!(p.x > 5.0 - 1e-6, "{pick:?}: {p:?}");
            } else {
                assert!(p.x < 5.0 + 1e-6, "{pick:?}: {p:?}");
            }
        }
    }
}

// ---------------- LENGTHEN ----------------

/// A quarter of the ellipse x²/16 + y²/4 = 1, from (4,0) to (0,2).
fn quarter_ellipse(s: &mut Session) -> String {
    h(&s.execute("ellipse", &json!({ "center": [0, 0], "major": [4, 0], "ratio": 0.5, "start": 0, "end": 90 })).unwrap())
}

#[test]
fn lengthen_elliptical_arcs_along_the_ellipse() {
    let mut s = Session::new();
    let e = quarter_ellipse(&mut s);
    let l0 = length(&mut s, &e);
    assert!((l0 - ellipse(&s, &e).length()).abs() < 1e-9);
    // DElta at the end nearest the pick (the (0,2) end); the start stays.
    s.execute("lengthen", &json!({ "handle": e, "pick": [0, 2], "delta": 1.0 })).unwrap();
    assert!((length(&mut s, &e) - (l0 + 1.0)).abs() < 1e-6);
    assert!(ellipse(&s, &e).start.abs() < 1e-12);
    // Total, Percent; picking the start end moves the start.
    s.execute("lengthen", &json!({ "handle": e, "pick": [0, 2], "total": l0 })).unwrap();
    assert!((ellipse(&s, &e).end - FRAC_PI_2).abs() < 1e-6);
    s.execute("lengthen", &json!({ "handle": e, "pick": [4, 0], "percent": 200 })).unwrap();
    assert!((length(&mut s, &e) - 2.0 * l0).abs() < 1e-6);
    assert!((ellipse(&s, &e).end - FRAC_PI_2).abs() < 1e-6);
    assert!((ellipse(&s, &e).start - 1.5 * PI).abs() < 1e-6, "{}", ellipse(&s, &e).start);
    // DYnamic / To: the end follows the point round the ellipse.
    s.execute("lengthen", &json!({ "handle": e, "pick": [0, 2], "to": [-4, 0] })).unwrap();
    assert!((ellipse(&s, &e).end - PI).abs() < 1e-9);
    // Too long (a full ellipse), too short, angle options: errors, the arc unchanged.
    let before = ellipse(&s, &e);
    for p in [json!({ "total": 1000.0 }), json!({ "delta": -1000.0 }), json!({ "deltaAngle": 10 }), json!({ "percent": f64::MIN_POSITIVE })] {
        let mut q = p.clone();
        q["handle"] = json!(e);
        q["pick"] = json!([0, 2]);
        assert!(s.execute("lengthen", &q).is_err(), "{q}");
        assert_eq!(ellipse(&s, &e), before);
    }
}

#[test]
fn lengthen_open_splines() {
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let before = spline(&s, &sp);
    let l0 = length(&mut s, &sp);
    // Longer: runs on along the end tangent.
    s.execute("lengthen", &json!({ "handle": sp, "pick": [9, 0], "delta": 2f64.sqrt() })).unwrap();
    assert!(ends(&spline(&s, &sp)).1.near(Vec2::new(10.0, 1.0), 1e-9));
    assert!((length(&mut s, &sp) - l0 - 2f64.sqrt()).abs() < 1e-6);
    // Shorter from the start: the far end stays put.
    s.execute("lengthen", &json!({ "handle": sp, "pick": [0, 0], "total": 5.0 })).unwrap();
    let now = spline(&s, &sp);
    assert!((length(&mut s, &sp) - 5.0).abs() < 1e-6);
    assert!(ends(&now).1.near(Vec2::new(10.0, 1.0), 1e-9));
    assert!(dist_to(&before, ends(&now).0) < 1e-9, "the new start lies on the old curve");
    // To a point on the curve: cut there.
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let mid = before.eval(0.5);
    s.execute("lengthen", &json!({ "handle": sp, "pick": [9, 0], "to": [mid.x, mid.y] })).unwrap();
    assert!(ends(&spline(&s, &sp)).1.near(mid, 1e-6));
    // Closed splines and nonsense lengths are refused.
    let c = h(&s.execute("spline", &json!({ "fit": [[0, 0], [4, 0], [4, 4]], "closed": true })).unwrap());
    assert!(s.execute("lengthen", &json!({ "handle": c, "delta": 1 })).is_err());
    assert!(s.execute("lengthen", &json!({ "handle": sp, "pick": [9, 0], "delta": -1e9 })).is_err());
}

#[test]
fn shortening_a_tightly_bent_spline_reaches_the_requested_length() {
    // #705: a fixed quadrature left this cubic 0.137 longer than asked, whatever the mode.
    let control = json!([[2.164, -9.484], [8.964, -13.012], [-16.428, -1.276], [18.58, -18.08]]);
    let half = 26.708_124_545_291_4 / 2.0;
    for mode in [json!({ "total": half }), json!({ "delta": -half }), json!({ "percent": 50 })] {
        let mut s = Session::new();
        let sp = h(&s.execute("spline", &json!({ "degree": 3, "control": control })).unwrap());
        let before = spline(&s, &sp);
        assert!((length(&mut s, &sp) - 2.0 * half).abs() < 1e-6, "{mode}");
        let mut q = mode.clone();
        q["handle"] = json!(sp);
        s.execute("lengthen", &q).unwrap();
        let got = length(&mut s, &sp);
        assert!((got - half).abs() < 1e-6, "{mode}: {got}");
        let now = spline(&s, &sp);
        assert!(ends(&now).0.near(ends(&before).0, 1e-9), "{mode}");
        assert!(dist_to(&before, ends(&now).1) < 1e-9, "{mode}: the new end lies on the old curve");
    }
}

#[test]
fn lengthen_at_the_command_line_on_an_elliptical_arc() {
    let mut s = Session::new();
    let e = quarter_ellipse(&mut s);
    let l0 = ellipse(&s, &e).length();
    s.script("LENGTHEN DE 1 0,2\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!((ellipse(&s, &e).length() - l0 - 1.0).abs() < 1e-6);
}

// ---------------- JOIN ----------------

fn arc_on_ellipse(s: &mut Session, start: f64, end: f64) -> String {
    h(&s.execute("ellipse", &json!({ "center": [1, 1], "major": [4, 0], "ratio": 0.5, "start": start, "end": end })).unwrap())
}

#[test]
fn join_merges_elliptical_arcs_on_one_ellipse() {
    // Touching arcs: one larger arc.
    let mut s = Session::new();
    let a = arc_on_ellipse(&mut s, 0.0, 90.0);
    let b = arc_on_ellipse(&mut s, 90.0, 180.0);
    let r = s.execute("join", &json!({ "handles": [a, b] })).unwrap();
    assert_eq!(h(&r), a, "the source arc is kept");
    let m = ellipse(&s, &a);
    assert!(m.start.abs() < 1e-9 && (m.end - PI).abs() < 1e-9, "{m:?}");
    assert_eq!(count(&s), 1);
    // A gap closes; the widest gap stays open.
    let mut s = Session::new();
    let a = arc_on_ellipse(&mut s, 0.0, 90.0);
    let b = arc_on_ellipse(&mut s, 120.0, 270.0);
    s.execute("join", &json!({ "handles": [a, b] })).unwrap();
    let m = ellipse(&s, &a);
    assert!(m.start.abs() < 1e-9 && (m.end - 1.5 * PI).abs() < 1e-9, "{m:?}");
    // Two halves: the full ellipse. The same ellipse with its major axis the other way round
    // (parameters shifted by half a turn) counts.
    let mut s = Session::new();
    let a = arc_on_ellipse(&mut s, 0.0, 180.0);
    let b = h(&s.execute("ellipse", &json!({ "center": [1, 1], "major": [-4, 0], "ratio": 0.5, "start": 0, "end": 180 })).unwrap());
    s.execute("join", &json!({ "handles": [a, b] })).unwrap();
    assert!(ellipse(&s, &a).is_full());
    assert_eq!(count(&s), 1);
}

#[test]
fn join_names_an_elliptical_arc_twice_without_losing_it() {
    // The same arc twice is one object: nothing to join, and the arc stays.
    let mut s = Session::new();
    let a = arc_on_ellipse(&mut s, 0.0, 90.0);
    let before = ellipse(&s, &a);
    assert!(s.execute("join", &json!({ "handles": [a, a] })).is_err());
    assert_eq!(count(&s), 1);
    assert_eq!(ellipse(&s, &a), before);
    // Repeating the source next to a second arc still merges the two into the source.
    let mut s = Session::new();
    let a = arc_on_ellipse(&mut s, 0.0, 90.0);
    let b = arc_on_ellipse(&mut s, 90.0, 180.0);
    let r = s.execute("join", &json!({ "handles": [a, a, b] })).unwrap();
    assert_eq!(h(&r), a);
    let m = ellipse(&s, &a);
    assert!(m.start.abs() < 1e-9 && (m.end - PI).abs() < 1e-9, "{m:?}");
    assert_eq!(count(&s), 1);
}

#[test]
fn join_makes_one_spline_from_lines_arcs_polylines_and_splines() {
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let before = spline(&s, &sp);
    let line = h(&s.execute("line", &json!({ "points": [[-5, 0], [0, 0]] })).unwrap());
    // An arc leaving (9,0) and a polyline (with an arc segment) after it, picked in any order.
    let arc = h(&s.execute("arc", &json!({ "center": [9, 3], "radius": 3, "start": 270, "end": 0 })).unwrap());
    let pl = h(&s.execute("pline", &json!({ "vertices": [[12, 3], { "p": [12, 6], "bulge": 0.5 }, [14, 6]] })).unwrap());
    let far = h(&s.execute("line", &json!({ "points": [[50, 50], [60, 60]] })).unwrap());
    s.execute("join", &json!({ "handles": [line, pl, sp, arc, far] })).unwrap();
    let j = spline(&s, &line);
    let (a, b) = ends(&j);
    assert!(a.near(Vec2::new(-5.0, 0.0), 1e-9), "{a:?}");
    assert!(b.near(Vec2::new(14.0, 6.0), 1e-9), "{b:?}");
    // Every joined piece lies on the result, exactly for the arc.
    for p in samples(&before, 20) {
        assert!(dist_to(&j, p) < 1e-9, "{p:?}");
    }
    for k in 0..=8 {
        let t = -FRAC_PI_2 + FRAC_PI_2 * k as f64 / 8.0;
        assert!(dist_to(&j, Vec2::new(9.0 + 3.0 * t.cos(), 3.0 + 3.0 * t.sin())) < 1e-9);
    }
    // The unconnected line stays; the others are consumed.
    assert_eq!(count(&s), 2);
    assert!(matches!(kind(&s, &far), EntityKind::Line(_)));
    assert!(s.doc().unwrap().entity(Handle::parse_hex(&sp).unwrap()).is_none());
}

#[test]
fn join_at_the_command_line_names_the_result() {
    let mut s = Session::new();
    s_curve(&mut s);
    s.execute("line", &json!({ "points": [[9, 0], [12, 0]] })).unwrap();
    s.script("JOIN ALL\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l.contains("2 objects joined into 1 spline")), "{:?}", s.log);
    assert!(matches!(s.doc().unwrap().model.last().unwrap().kind, EntityKind::Spline(_)));
    assert_eq!(count(&s), 1);
}

#[test]
fn join_refuses_unconnected_curves() {
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let l = h(&s.execute("line", &json!({ "points": [[20, 0], [30, 0]] })).unwrap());
    assert!(s.execute("join", &json!({ "handles": [sp, l] })).is_err());
    assert_eq!(count(&s), 2);
    // Arcs on different ellipses that do not touch.
    let a = arc_on_ellipse(&mut s, 0.0, 90.0);
    let b = h(&s.execute("ellipse", &json!({ "center": [40, 40], "major": [4, 0], "ratio": 0.5, "start": 0, "end": 90 })).unwrap());
    assert!(s.execute("join", &json!({ "handles": [a, b] })).is_err());
}

#[test]
fn hostile_curve_edits_do_not_panic() {
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let e = quarter_ellipse(&mut s);
    // A degenerate spline (all control points equal) and a huge-degree one.
    let d = h(&s.execute("spline", &json!({ "control": [[1, 1], [1, 1], [1, 1], [1, 1]] })).unwrap());
    s.execute("line", &json!({ "points": [[0, -50], [0, 50]] })).unwrap();
    for target in [&sp, &e, &d] {
        for p in [
            json!({ "delta": 1e308 }),
            json!({ "delta": f64::MAX }),
            json!({ "total": 1e-300 }),
            json!({ "percent": 1e300 }),
            json!({ "to": [1e308, -1e308] }),
            json!({ "to": [0, 0] }),
        ] {
            let mut q = p.clone();
            q["handle"] = json!(target);
            let _ = s.execute("lengthen", &q);
        }
        let _ = s.execute("extend", &json!({ "handle": target, "pick": [1e308, 1e308] }));
        let _ = s.execute("trim", &json!({ "handle": target, "pick": [-1e308, 0] }));
        for r in [0.0, 1e-300, 1e308] {
            let _ = s.execute("fillet", &json!({ "h1": target, "p1": [1e308, 0], "h2": sp, "p2": [0, 0], "radius": r }));
        }
        let _ = s.execute("join", &json!({ "handles": [target, sp, e, d] }));
    }
}

// ---------------- FILLET / CHAMFER ----------------

fn line_ends(s: &Session, h: &str) -> (Vec2, Vec2) {
    match kind(s, h) {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

/// The fillet arc made last: centre, radius and end points.
fn last_arc(s: &Session) -> (Vec2, f64, Vec2, Vec2) {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Arc(a) => {
            let c = a.center.xy();
            (c, a.radius, c + Vec2::from_angle(a.start) * a.radius, c + Vec2::from_angle(a.end) * a.radius)
        }
        k => panic!("not an arc: {k:?}"),
    }
}

fn ellipse_point(deg: f64) -> [f64; 2] {
    let t = deg.to_radians();
    [4.0 * t.cos(), 2.0 * t.sin()]
}

/// The upper half of x²/16 + y²/4 = 1 and the line y = 1 across it (crossing at θ = 30°).
fn ellipse_and_line(s: &mut Session) -> (String, String) {
    let e = h(&s.execute("ellipse", &json!({ "center": [0, 0], "major": [4, 0], "ratio": 0.5, "start": 0, "end": 180 })).unwrap());
    let l = h(&s.execute("line", &json!({ "points": [[-6, 1], [6, 1]] })).unwrap());
    (e, l)
}

/// The fillet arc meets each object at a touch point, tangentially.
fn assert_tangent(at: Vec2, center: Vec2, tangent: Vec2) {
    assert!((at - center).normalized().dot(tangent.normalized()).abs() < 1e-6, "{at:?} {center:?} {tangent:?}");
}

#[test]
fn fillet_line_and_elliptical_arc() {
    let mut s = Session::new();
    let (e, l) = ellipse_and_line(&mut s);
    // Picks on the parts to keep: the line right of the corner, the arc below it.
    let r = s.execute("fillet", &json!({ "h1": l, "p1": [5, 1], "h2": e, "p2": ellipse_point(5.0), "radius": 0.5 })).unwrap();
    assert!(r["arc"].is_string(), "{r}");
    let (c, rad, a0, a1) = last_arc(&s);
    assert!((rad - 0.5).abs() < 1e-12 && (c.y - 0.5).abs() < 1e-9, "{c:?}");
    // The line keeps its right end and now starts at the touch point above the centre.
    let (la, lb) = line_ends(&s, &l);
    assert!(lb.near(Vec2::new(6.0, 1.0), 1e-12));
    assert!(la.near(Vec2::new(c.x, 1.0), 1e-9), "{la:?} {c:?}");
    // The arc keeps its start and ends at the other touch point, tangent to the fillet.
    let g = ellipse(&s, &e);
    assert!(g.start.abs() < 1e-12, "{g:?}");
    let te = g.at_param(g.end);
    assert!((te.dist(c) - 0.5).abs() < 1e-9);
    assert_tangent(te, c, g.derivative(g.end));
    // The fillet arc runs between the two touch points.
    assert!((a0.near(la, 1e-9) && a1.near(te, 1e-9)) || (a0.near(te, 1e-9) && a1.near(la, 1e-9)), "{a0:?} {a1:?}");
    assert_eq!(count(&s), 3);
}

#[test]
fn fillet_radius_zero_extends_an_elliptical_arc_to_a_line() {
    let mut s = Session::new();
    let e = h(&s.execute("ellipse", &json!({ "center": [0, 0], "major": [4, 0], "ratio": 0.5, "start": 0, "end": 20 })).unwrap());
    let l = h(&s.execute("line", &json!({ "points": [[-6, 1], [6, 1]] })).unwrap());
    s.execute("fillet", &json!({ "h1": e, "p1": ellipse_point(10.0), "h2": l, "p2": [5, 1], "radius": 0 })).unwrap();
    assert!((ellipse(&s, &e).end - PI / 6.0).abs() < 1e-9, "{:?}", ellipse(&s, &e));
    assert!(line_ends(&s, &l).0.near(Vec2::new(2.0 * 3f64.sqrt(), 1.0), 1e-9));
    assert_eq!(count(&s), 2);
}

#[test]
fn fillet_spline_and_line() {
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let l = h(&s.execute("line", &json!({ "points": [[4.5, -5], [4.5, 5]] })).unwrap());
    let before = spline(&s, &sp);
    let pick = before.eval(0.15);
    s.execute("fillet", &json!({ "h1": sp, "p1": [pick.x, pick.y], "h2": l, "p2": [4.5, 4], "radius": 0.3 })).unwrap();
    let (c, rad, _, _) = last_arc(&s);
    assert!((rad - 0.3).abs() < 1e-12);
    // The spline keeps its start and ends at a touch point on the old curve.
    let now = spline(&s, &sp);
    let (a, b) = ends(&now);
    assert!(a.near(Vec2::ZERO, 1e-9));
    assert!((b.dist(c) - 0.3).abs() < 1e-9 && dist_to(&before, b) < 1e-9);
    assert_tangent(b, c, now.derivative(now.domain().1));
    // The line keeps its top and starts level with the centre.
    let (la, lb) = line_ends(&s, &l);
    assert!(lb.near(Vec2::new(4.5, 5.0), 1e-12) && (la.y - c.y).abs() < 1e-9 && (c.x - 4.5).abs() > 0.29);
    // Radius 0: they meet where they cross.
    let mut s = Session::new();
    let sp = s_curve(&mut s);
    let l = h(&s.execute("line", &json!({ "points": [[4.5, -5], [4.5, 5]] })).unwrap());
    s.execute("fillet", &json!({ "h1": sp, "p1": [pick.x, pick.y], "h2": l, "p2": [4.5, 4], "radius": 0 })).unwrap();
    assert!(ends(&spline(&s, &sp)).1.near(Vec2::new(4.5, 0.0), 1e-9));
    assert!(line_ends(&s, &l).0.near(Vec2::new(4.5, 0.0), 1e-9));
}

#[test]
fn fillet_at_the_command_line_with_no_trim() {
    let mut s = Session::new();
    let (e, l) = ellipse_and_line(&mut s);
    let (eb, lb) = (ellipse(&s, &e), line_ends(&s, &l));
    let [x, y] = ellipse_point(5.0);
    s.script(&format!("FILLET T N R 0.5 5,1 {x},{y}\n")).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(count(&s), 3, "{:?}", s.log);
    assert_eq!(ellipse(&s, &e), eb);
    assert_eq!(line_ends(&s, &l), lb);
    assert!((last_arc(&s).1 - 0.5).abs() < 1e-12);
}

#[test]
fn fillet_failures_leave_the_drawing_alone() {
    let mut s = Session::new();
    let (e, l) = ellipse_and_line(&mut s);
    let (eb, lb) = (ellipse(&s, &e), line_ends(&s, &l));
    // Invalid radii, the same object twice: errors.
    assert!(s.execute("fillet", &json!({ "h1": l, "p1": [5, 1], "h2": e, "p2": ellipse_point(5.0), "radius": -1 })).is_err());
    assert!(s.execute("fillet", &json!({ "h1": e, "p1": [4, 0], "h2": e, "p2": [0, 2], "radius": 1 })).is_err());
    assert_eq!(ellipse(&s, &e), eb);
    assert_eq!(line_ends(&s, &l), lb);
    // CHAMFER does not take ellipses or splines, and says so.
    let err = s.execute("chamfer", &json!({ "h1": l, "p1": [5, 1], "h2": e, "p2": ellipse_point(5.0), "d1": 1 })).unwrap_err();
    assert!(err.to_string().contains("cannot chamfer"), "{err}");
    // No circle of that radius touches a spline within its ends and a far line.
    let sp = s_curve(&mut s);
    let far = h(&s.execute("line", &json!({ "points": [[100, -5], [100, 5]] })).unwrap());
    let before = spline(&s, &sp);
    let err = s.execute("fillet", &json!({ "h1": sp, "p1": [9, 0], "h2": far, "p2": [100, 0], "radius": 0.1 })).unwrap_err();
    assert!(err.to_string().contains("No fillet"), "{err}");
    assert_eq!(spline(&s, &sp), before);
}

// ---------------- FILLET / CHAMFER with polyline segments ----------------

fn pline(s: &mut Session, pts: &[[f64; 2]], closed: bool) -> String {
    h(&s.execute("pline", &json!({ "vertices": pts, "closed": closed })).unwrap())
}

/// Vertices as (x, y, bulge), rounded.
fn verts(s: &Session, h: &str) -> Vec<(f64, f64, f64)> {
    let r = |v: f64| (v * 1e9).round() / 1e9;
    match kind(s, h) {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (r(v.p.x), r(v.p.y), r(v.bulge))).collect(),
        k => panic!("not a polyline: {k:?}"),
    }
}

/// Bulge of a quarter turn, counter-clockwise.
fn quarter() -> f64 {
    ((PI / 8.0).tan() * 1e9).round() / 1e9
}

#[test]
fn fillet_and_chamfer_two_segments_of_one_polyline() {
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.execute("fillet", &json!({ "h1": p, "p1": [5, 0], "h2": p, "p2": [10, 5], "radius": 2 })).unwrap();
    assert_eq!(verts(&s, &p), vec![(0.0, 0.0, 0.0), (8.0, 0.0, quarter()), (10.0, 2.0, 0.0), (10.0, 10.0, 0.0)]);
    assert_eq!(count(&s), 1);
    // CHAMFER: the first distance goes on the first segment picked.
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.execute("chamfer", &json!({ "h1": p, "p1": [10, 5], "h2": p, "p2": [5, 0], "d1": 1, "d2": 2 })).unwrap();
    assert_eq!(verts(&s, &p), vec![(0.0, 0.0, 0.0), (8.0, 0.0, 0.0), (10.0, 1.0, 0.0), (10.0, 10.0, 0.0)]);
    // One segment between them: the corner replaces it (radius 0: a sharp corner).
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [11.0, 1.0], [11.0, 10.0]], false);
    s.execute("fillet", &json!({ "h1": p, "p1": [5, 0], "h2": p, "p2": [11, 5], "radius": 0 })).unwrap();
    assert_eq!(verts(&s, &p), vec![(0.0, 0.0, 0.0), (11.0, 0.0, 0.0), (11.0, 10.0, 0.0)]);
    // A closed polyline's corner across its start vertex.
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], true);
    s.execute("fillet", &json!({ "h1": p, "p1": [5, 0], "h2": p, "p2": [0, 5], "radius": 1 })).unwrap();
    let v = verts(&s, &p);
    assert_eq!(v.len(), 5, "{v:?}");
    assert!(v.contains(&(0.0, 1.0, quarter())) && v.contains(&(1.0, 0.0, 0.0)), "{v:?}");
    assert!(!v.iter().any(|(x, y, _)| *x == 0.0 && *y == 0.0));
}

#[test]
fn fillet_and_chamfer_a_line_into_a_polyline() {
    // The polyline's last segment with a line: the line joins the polyline.
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 5.0], [0.0, 0.0], [8.0, 0.0]], false);
    let l = h(&s.execute("line", &json!({ "points": [[10, 2], [10, 10]] })).unwrap());
    s.execute("fillet", &json!({ "h1": p, "p1": [4, 0], "h2": l, "p2": [10, 8], "radius": 1 })).unwrap();
    assert_eq!(verts(&s, &p), vec![(0.0, 5.0, 0.0), (0.0, 0.0, 0.0), (9.0, 0.0, quarter()), (10.0, 1.0, 0.0), (10.0, 10.0, 0.0)]);
    assert_eq!(count(&s), 1);
    // Its first segment, the line picked first, CHAMFER distances in pick order.
    let mut s = Session::new();
    let p = pline(&mut s, &[[8.0, 0.0], [0.0, 0.0], [0.0, 5.0]], false);
    let l = h(&s.execute("line", &json!({ "points": [[10, 2], [10, 10]] })).unwrap());
    s.execute("chamfer", &json!({ "h1": l, "p1": [10, 8], "h2": p, "p2": [4, 0], "d1": 1, "d2": 2 })).unwrap();
    assert_eq!(verts(&s, &p), vec![(10.0, 10.0, 0.0), (10.0, 1.0, 0.0), (8.0, 0.0, 0.0), (0.0, 0.0, 0.0), (0.0, 5.0, 0.0)]);
    assert_eq!(count(&s), 1);
}

#[test]
fn polyline_fillets_at_the_command_line_and_with_no_trim() {
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.script("FILLET R 2 5,0 10,5\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(verts(&s, &p).len(), 4, "{:?}", s.log);
    // No trim: the polyline stays, the arc is drawn on its own.
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    let before = verts(&s, &p);
    s.script("FILLET T N R 2 5,0 10,5\n").unwrap();
    assert_eq!(verts(&s, &p), before);
    let (c, r, _, _) = last_arc(&s);
    assert!(c.near(Vec2::new(8.0, 2.0), 1e-9) && (r - 2.0).abs() < 1e-9, "{c:?} {r}");
}

#[test]
fn polyline_fillet_refusals_leave_it_alone() {
    let mut s = Session::new();
    let p = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 20.0]], false);
    let before = verts(&s, &p);
    let c = h(&s.execute("circle", &json!({ "center": [30, 0], "radius": 1 })).unwrap());
    let l = h(&s.execute("line", &json!({ "points": [[20, -5], [20, 5]] })).unwrap());
    for q in [
        // Segments too far apart, the same segment, a circle, a middle segment with a line, too big.
        json!({ "h1": p, "p1": [5, 0], "h2": p, "p2": [0, 15], "radius": 1 }),
        json!({ "h1": p, "p1": [5, 0], "h2": p, "p2": [6, 0], "radius": 1 }),
        json!({ "h1": p, "p1": [5, 0], "h2": c, "p2": [31, 0], "radius": 1 }),
        json!({ "h1": p, "p1": [10, 5], "h2": l, "p2": [20, 4], "radius": 1 }),
        json!({ "h1": p, "p1": [5, 0], "h2": p, "p2": [10, 5], "radius": 100 }),
    ] {
        assert!(s.execute("fillet", &q).is_err(), "{q}");
        assert_eq!(verts(&s, &p), before);
    }
    assert_eq!(count(&s), 3);
}
