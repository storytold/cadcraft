//! SPLINE's Knots, start/end Tangency, toLerance and Object options, at the command line and in
//! the JSON form (#408).

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use cadcraft_geom::{KnotParam, Spline, Vec2};
use serde_json::json;

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()).unwrap()
}

fn last_spline(s: &Session) -> Spline {
    let EntityKind::Spline(sp) = last(s) else { panic!("no spline") };
    sp
}

fn logged(s: &Session, text: &str) -> bool {
    s.log.iter().any(|l| l.contains(text))
}

const FIT: &str = "0,0 1,0.2 5,3 6,3.1 9,0";

#[test]
fn knots_option_sets_the_parametrisation_and_is_remembered() {
    let mut s = Session::new();
    s.cmdline(&format!("spline k u {FIT}")).unwrap();
    s.cmdline("").unwrap();
    let sp = last_spline(&s);
    assert_eq!(sp.fit_opts.knots, KnotParam::Uniform);
    assert_eq!(sp.fit.len(), 5);
    assert_eq!(sp.infer_knot_param(), KnotParam::Uniform);
    // The next SPLINE starts with it, and says so.
    s.cmdline(&format!("spline {FIT}")).unwrap();
    s.cmdline("").unwrap();
    assert!(logged(&s, "Knots=Uniform"));
    assert_eq!(last_spline(&s).fit_opts.knots, KnotParam::Uniform);
    // Square root, then Enter at the Knots prompt keeps the current choice.
    s.cmdline("spline k s k").unwrap();
    s.cmdline("").unwrap();
    s.cmdline(FIT).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(last_spline(&s).fit_opts.knots, KnotParam::SqrtChord);
    // A closed spline keeps it too.
    s.cmdline("spline k c 0,0 4,0 4,3 0,3 c").unwrap();
    let sp = last_spline(&s);
    assert!(sp.closed && sp.is_periodic() && sp.fit_opts.knots == KnotParam::Chord);
    // Not a parametrisation: reported, the question stays.
    s.cmdline("spline k xyz").unwrap();
    assert!(logged(&s, "Invalid option keyword."));
    assert!(s.running.is_some());
    s.cmdline("u 0,0 5,5 10,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(last_spline(&s).fit_opts.knots, KnotParam::Uniform);
}

#[test]
fn tangency_options_fix_the_end_directions() {
    let mut s = Session::new();
    // Start tangent straight up, end tangent straight down; the end tangent finishes the spline.
    s.cmdline("spline 0,0 t 0,5 10,0 t 10,-5").unwrap();
    assert!(s.running.is_none(), "the end tangent ends the command");
    let sp = last_spline(&s);
    assert_eq!(sp.fit, [Vec2::ZERO, Vec2::new(10.0, 0.0)]);
    assert_eq!(sp.fit_opts.start_tangent, Some(Vec2::Y));
    assert_eq!(sp.fit_opts.end_tangent, Some(Vec2::new(0.0, -1.0)));
    let (lo, hi) = sp.domain();
    let h = 1e-7;
    assert!((sp.eval(lo + h) - sp.eval(lo)).normalized().near(Vec2::Y, 1e-4));
    assert!((sp.eval(hi) - sp.eval(hi - h)).normalized().near(Vec2::new(0.0, -1.0), 1e-4));
    assert!(sp.eval((lo + hi) / 2.0).y > 1.0, "the curve bows up between the tangents");

    // A typed angle works as a direction; Enter at the end tangent leaves that end free.
    s.cmdline("spline 0,0 t 90 5,5 10,0 t").unwrap();
    s.cmdline("").unwrap();
    let sp = last_spline(&s);
    assert!(sp.fit_opts.start_tangent.is_some_and(|t| t.near(Vec2::Y, 1e-9)));
    assert_eq!(sp.fit_opts.end_tangent, None);
    assert_eq!(sp.fit.len(), 3);

    // The tangent point may not be the fit point itself.
    s.cmdline("spline 0,0 t 0,0").unwrap();
    assert!(logged(&s, "The tangent point must differ from the fit point."));
    s.cmdline("*cancel*").ok();
}

#[test]
fn tolerance_option_approximates_the_fit_points() {
    let mut s = Session::new();
    let pts: Vec<String> = (0..30)
        .map(|i| {
            let t = i as f64 / 29.0;
            format!("{:.4},{:.4}", t * 10.0, (t * 3.0).sin() * 2.0 + if i % 2 == 0 { 0.01 } else { -0.01 })
        })
        .collect();
    s.cmdline(&format!("spline {} l 0.1 {}", pts[0], pts[1..].join(" "))).unwrap();
    s.cmdline("").unwrap();
    let sp = last_spline(&s);
    assert_eq!(sp.fit_opts.tolerance, 0.1);
    assert_eq!(sp.fit.len(), 30, "the fit points are kept");
    assert!(sp.control.len() < 30, "fewer control points than an interpolation: {}", sp.control.len());
    let poly = sp.tessellate(1e-4);
    for f in &sp.fit {
        let d = poly.iter().map(|p| p.dist(*f)).fold(f64::INFINITY, f64::min);
        assert!(d <= 0.1 + 1e-3, "fit point {f:?} is {d} away");
    }
    // A negative tolerance is refused and asked again.
    s.cmdline("spline 0,0 l -1").unwrap();
    assert!(logged(&s, "Requires a non-negative number."));
    s.cmdline("0 5,5 10,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(last_spline(&s).fit_opts.tolerance, 0.0);
}

#[test]
fn object_option_converts_polylines() {
    let mut s = Session::new();
    // A spline-fit polyline: PEDIT Spline leaves straight segments along the curve.
    let h = s.execute("pline", &json!({"vertices": [[0, 0], [10, 10], [20, 0], [30, 10]]})).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("pedit", &json!({"handle": h, "option": "spline"})).unwrap();
    let EntityKind::LwPolyline(pl) = last(&s) else { panic!("no polyline") };
    let n = pl.vertices.len();
    s.cmdline("spline o").unwrap();
    s.set_selection(Vec::new());
    s.cmdline(&format!("{},{}", pl.vertices[1].p.x, pl.vertices[1].p.y)).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(logged(&s, "1 object(s) converted to splines"));
    let doc = s.doc().unwrap();
    assert_eq!(doc.model.iter().count(), 1, "converted in place");
    let EntityKind::Spline(sp) = last(&s) else { panic!("not converted") };
    assert_eq!(sp.fit.len(), n);
    assert!(sp.eval(0.0).near(Vec2::ZERO, 1e-9) && sp.eval(1.0).near(Vec2::new(30.0, 10.0), 1e-9));

    // JSON form: a closed polyline becomes a closed spline; arcs are refused.
    let sq = s.execute("pline", &json!({"vertices": [[0, 20], [10, 20], [10, 30], [0, 30]], "closed": true})).unwrap()["handle"].clone();
    let r = s.execute("spline", &json!({"object": sq})).unwrap();
    assert_eq!(r["handles"][0], sq);
    let EntityKind::Spline(sp) = last(&s) else { panic!("not converted") };
    assert!(sp.closed && sp.is_periodic() && sp.fit.len() == 4);
    let arc = s.execute("pline", &json!({"vertices": [{"p": [0, 50], "bulge": 1.0}, [10, 50]]})).unwrap()["handle"].clone();
    let e = s.execute("spline", &json!({"object": arc})).unwrap_err().to_string();
    assert!(e.contains("arc segments"), "{e}");
    let line = s.execute("line", &json!({"points": [[0, 60], [5, 60]]})).unwrap()["handles"][0].clone();
    assert!(s.execute("spline", &json!({"object": line})).is_err());
}

#[test]
fn json_form_takes_the_fit_options() {
    let mut s = Session::new();
    let fit = json!([[0, 0], [1, 0.2], [5, 3], [6, 3.1], [9, 0]]);
    s.execute("spline", &json!({"fit": fit, "knots": "sqrt", "startTangent": [0, 2], "endTangent": [1, 0]})).unwrap();
    let sp = last_spline(&s);
    assert_eq!(sp.fit_opts.knots, KnotParam::SqrtChord);
    assert_eq!(sp.fit_opts.start_tangent, Some(Vec2::Y));
    assert_eq!(sp.control.len(), 7, "five fit points and two tangents");
    s.execute("spline", &json!({"fit": fit, "closed": true, "knots": "uniform"})).unwrap();
    assert!(last_spline(&s).is_periodic());
    for bad in [json!({"fit": fit, "knots": "zigzag"}), json!({"fit": fit, "tolerance": -1}), json!({"fit": fit, "startTangent": [0, 0]})] {
        assert!(s.execute("spline", &bad).is_err(), "{bad}");
    }
    // Editing a fit point keeps the options (grips and SPLINEDIT refit the same way).
    let h = s.execute("spline", &json!({"fit": fit, "knots": "uniform", "startTangent": [0, 1]})).unwrap()["handle"].clone();
    s.execute("splinedit", &json!({"handle": h, "option": "move", "index": 2, "to": [5, 4]})).unwrap();
    let sp = last_spline(&s);
    assert_eq!(sp.fit_opts.knots, KnotParam::Uniform);
    assert_eq!(sp.fit_opts.start_tangent, Some(Vec2::Y));
    // Reversing swaps and turns the tangents.
    s.execute("splinedit", &json!({"handle": h, "option": "reverse"})).unwrap();
    let sp = last_spline(&s);
    assert_eq!(sp.fit_opts.start_tangent, None);
    assert_eq!(sp.fit_opts.end_tangent, Some(-Vec2::Y));
}
