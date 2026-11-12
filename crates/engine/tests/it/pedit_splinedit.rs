//! PEDIT Edit vertex Break / Straighten / Tangent / Regen and the SPLINEDIT Fit data and Edit
//! vertex options, at the command line and in the JSON form (#554).

use cadcraft_doc::{EntityKind, LwPolyline};
use cadcraft_engine::Session;
use cadcraft_geom::{Spline, Vec2};
use serde_json::{Value, json};

fn h(v: &Value) -> String {
    v["handle"].as_str().or_else(|| v["handles"][0].as_str()).unwrap().to_string()
}

fn kind(s: &Session, hex: &str) -> EntityKind {
    let h = cadcraft_doc::Handle::parse_hex(hex).unwrap();
    s.doc().unwrap().entity(h).unwrap().kind.clone()
}

fn polys(s: &Session) -> Vec<LwPolyline> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::LwPolyline(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

fn spline(s: &Session, hex: &str) -> Spline {
    let EntityKind::Spline(sp) = kind(s, hex) else { panic!("not a spline") };
    sp
}

fn pts(p: &LwPolyline) -> Vec<(f64, f64)> {
    p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()
}

fn pline(s: &mut Session, points: Value, closed: bool) -> String {
    h(&s.execute("pline", &json!({ "points": points, "closed": closed })).unwrap())
}

/// Start direction of the segment that leaves vertex `v`.
fn leaving(p: &LwPolyline, at: Vec2) -> Vec2 {
    let i = p.vertices.iter().position(|v| v.p.near(at, 1e-9)).unwrap();
    let (a, b) = (p.vertices[i], p.vertices[(i + 1) % p.vertices.len()]);
    let chord = (b.p - a.p).normalized();
    let turn = -2.0 * a.bulge.atan();
    Vec2::new(chord.x * turn.cos() - chord.y * turn.sin(), chord.x * turn.sin() + chord.y * turn.cos())
}

fn has_corner(sp: &Spline) -> bool {
    let (lo, hi) = sp.domain();
    sp.knots.iter().any(|k| *k > lo && *k < hi && sp.knots.iter().filter(|x| (*x - k).abs() < 1e-12).count() >= sp.degree)
}

// ---------------- PEDIT ----------------

#[test]
fn edit_vertex_break_splits_an_open_polyline_and_undo_joins_it() {
    let mut s = Session::new();
    pline(&mut s, json!([[0, 0], [10, 0], [10, 10], [0, 10]]), false);
    s.cmdline("pedit 5,0 e n b n g").unwrap();
    let mut ps = polys(&s);
    ps.sort_by(|a, b| a.vertices.len().cmp(&b.vertices.len()).then(a.vertices[0].p.x.total_cmp(&b.vertices[0].p.x)));
    assert_eq!(ps.len(), 2, "{}", s.prompt_text());
    assert_eq!(pts(&ps[0]), vec![(0.0, 0.0), (10.0, 0.0)]);
    assert_eq!(pts(&ps[1]), vec![(10.0, 10.0), (0.0, 10.0)]);
    assert!(s.prompt_text().contains("vertex editing option"), "back at the vertex menu: {}", s.prompt_text());
    s.cmdline("x u").unwrap();
    let ps = polys(&s);
    assert_eq!(ps.len(), 1, "undo erases the split-off piece");
    assert_eq!(ps[0].vertices.len(), 4);
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn break_json_opens_a_closed_polyline_and_refuses_the_ends() {
    let mut s = Session::new();
    let r = pline(&mut s, json!([[0, 0], [10, 0], [10, 10], [0, 10]]), true);
    let out = s.execute("pedit", &json!({ "handle": r, "option": "vertex", "action": "break", "index": 1, "index2": 2 })).unwrap();
    assert_eq!(out["handles"].as_array().unwrap().len(), 1);
    let EntityKind::LwPolyline(p) = kind(&s, &r) else { panic!() };
    assert!(!p.closed);
    assert_eq!(pts(&p), vec![(10.0, 10.0), (0.0, 10.0), (0.0, 0.0), (10.0, 0.0)], "the segment 1-2 is gone");

    let o = pline(&mut s, json!([[0, 0], [10, 0], [20, 0]]), false);
    // The same vertex in the middle splits there.
    let out = s.execute("pedit", &json!({ "handle": o, "option": "vertex", "action": "break", "index": 1 })).unwrap();
    assert_eq!(out["handles"].as_array().unwrap().len(), 2);
    let o2 = pline(&mut s, json!([[0, 0], [10, 0], [20, 0]]), false);
    for (i, j) in [(0, 0), (2, 2), (0, 2)] {
        assert!(s.execute("pedit", &json!({ "handle": o2, "option": "vertex", "action": "break", "index": i, "index2": j })).is_err(), "{i} {j}");
    }
    assert!(s.execute("pedit", &json!({ "handle": o2, "option": "vertex", "action": "break", "index": 0, "index2": 99 })).is_err());
    let EntityKind::LwPolyline(p) = kind(&s, &o2) else { panic!() };
    assert_eq!(p.vertices.len(), 3, "a refused break changes nothing");
}

#[test]
fn edit_vertex_straighten_removes_the_vertices_between_the_markers() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [5, 3], [10, 0], [15, 3], [20, 0]]), false);
    s.cmdline("pedit 2.5,1.5 e s n n g x").unwrap();
    s.cmdline("").unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &p) else { panic!() };
    assert_eq!(pts(&pl), vec![(0.0, 0.0), (10.0, 0.0), (15.0, 3.0), (20.0, 0.0)]);
    // JSON, and the same vertex twice straightens the following arc.
    s.execute("pedit", &json!({ "handle": p, "option": "fit" })).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &p) else { panic!() };
    assert!(pl.vertices[0].bulge != 0.0);
    s.execute("pedit", &json!({ "handle": p, "option": "vertex", "action": "straighten", "index": 0 })).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &p) else { panic!() };
    assert_eq!(pl.vertices[0].bulge, 0.0);
    let n = pl.vertices.len();
    s.execute("pedit", &json!({ "handle": p, "option": "vertex", "action": "straighten", "index": 3, "index2": 0 })).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &p) else { panic!() };
    assert_eq!(pl.vertices.len(), n - 2, "vertices 1 and 2 are gone");
    assert_eq!(pl.vertices[0].bulge, 0.0);
}

#[test]
fn edit_vertex_tangent_steers_fit() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [5, 5], [10, 0]]), false);
    s.cmdline("pedit 2,2 e n t 90 x f").unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &p) else { panic!() };
    assert!(leaving(&pl, Vec2::new(5.0, 5.0)).near(Vec2::Y, 1e-6), "{:?}", leaving(&pl, Vec2::new(5.0, 5.0)));
    // A point gives the direction from the vertex; Regen changes nothing.
    s.cmdline("d e n t 10,5 r x f").unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &p) else { panic!() };
    assert!(leaving(&pl, Vec2::new(5.0, 5.0)).near(Vec2::X, 1e-6));
    s.cmdline("").unwrap();
    assert!(s.running.is_none());

    // JSON: tangents with the fit option.
    let q = pline(&mut s, json!([[0, 0], [5, 5], [10, 0]]), false);
    s.execute("pedit", &json!({ "handle": q, "option": "fit", "tangents": [{ "index": 1, "angle": 90 }] })).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, &q) else { panic!() };
    assert!(leaving(&pl, Vec2::new(5.0, 5.0)).near(Vec2::Y, 1e-6));
    s.execute("pedit", &json!({ "handle": q, "option": "vertex", "action": "regen", "index": 0 })).unwrap();
    for bad in [json!([{ "index": 9, "angle": 0 }]), json!([{ "index": 0 }]), json!([{ "index": 0, "direction": [0, 0] }]), json!("x")] {
        assert!(s.execute("pedit", &json!({ "handle": q, "option": "fit", "tangents": bad })).is_err(), "{bad}");
    }
}

// ---------------- SPLINEDIT ----------------

fn fit_spline(s: &mut Session) -> String {
    h(&s.execute("spline", &json!({ "fit": [[0, 0], [2, 2], [4, 0], [6, 2]] })).unwrap())
}

#[test]
fn fit_data_options_at_the_command_line() {
    let mut s = Session::new();
    let sp = fit_spline(&mut s);
    s.cmdline("splinedit 0,0 f a 2,2 3,1").unwrap();
    assert_eq!(spline(&s, &sp).fit[2], Vec2::new(3.0, 1.0), "added after the picked fit point");
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("d 3,1").unwrap();
    assert_eq!(spline(&s, &sp).fit.len(), 4);
    s.cmdline("").unwrap();
    s.cmdline("m n 2,3 x").unwrap();
    assert_eq!(spline(&s, &sp).fit[1], Vec2::new(2.0, 3.0));
    s.cmdline("l 0.5").unwrap();
    assert_eq!(spline(&s, &sp).fit_opts.tolerance, 0.5);
    s.cmdline("t 0,5 s").unwrap();
    let o = spline(&s, &sp).fit_opts;
    assert!(o.start_tangent.is_some_and(|t| t.near(Vec2::Y, 1e-9)) && o.end_tangent.is_none());
    s.cmdline("l 0").unwrap();
    s.cmdline("k 5,1").unwrap();
    let k = spline(&s, &sp);
    assert_eq!(k.fit.len(), 6, "a kink is a repeated fit point");
    assert!(has_corner(&k));
    s.cmdline("").unwrap();
    s.cmdline("c").unwrap();
    assert!(spline(&s, &sp).closed);
    s.cmdline("o x u").unwrap();
    assert!(spline(&s, &sp).closed, "Undo takes back the last edit");
    s.cmdline("x").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn fit_data_json_options() {
    let mut s = Session::new();
    let sp = fit_spline(&mut s);
    let run = |s: &mut Session, p: Value| {
        let mut p = p;
        p["handle"] = json!(sp);
        s.execute("splinedit", &p)
    };
    run(&mut s, json!({ "option": "fitadd", "index": 0, "to": [1, 1] })).unwrap();
    assert_eq!(spline(&s, &sp).fit[1], Vec2::new(1.0, 1.0));
    run(&mut s, json!({ "option": "fitadd", "index": 0, "to": [-1, 0], "before": true })).unwrap();
    assert_eq!(spline(&s, &sp).fit[0], Vec2::new(-1.0, 0.0));
    run(&mut s, json!({ "option": "fitdelete", "index": 0 })).unwrap();
    run(&mut s, json!({ "option": "fitmove", "index": 1, "to": [1, 2] })).unwrap();
    assert_eq!(spline(&s, &sp).fit.len(), 5);
    run(&mut s, json!({ "option": "tangents", "start": [1, 0], "end": [0, -3] })).unwrap();
    assert_eq!(spline(&s, &sp).fit_opts.end_tangent, Some(-Vec2::Y));
    run(&mut s, json!({ "option": "tangents", "start": null })).unwrap();
    assert!(spline(&s, &sp).fit_opts.start_tangent.is_none());
    run(&mut s, json!({ "option": "kink", "at": [2, 2] })).unwrap();
    assert!(has_corner(&spline(&s, &sp)), "a kink at an existing fit point");
    run(&mut s, json!({ "option": "fitdelete", "index": 3 })).unwrap();
    assert!(!has_corner(&spline(&s, &sp)), "deleting the kink point removes both copies");
    run(&mut s, json!({ "option": "tolerance", "tolerance": 0.2 })).unwrap();
    for bad in [
        json!({ "option": "fitadd", "index": 99, "to": [0, 0] }),
        json!({ "option": "fitdelete", "index": 99 }),
        json!({ "option": "tolerance", "tolerance": -1 }),
        json!({ "option": "tangents", "start": [0, 0] }),
        json!({ "option": "kink" }),
        json!({ "option": "weight", "index": 0, "weight": 0 }),
        json!({ "option": "weight", "index": 0, "weight": -2 }),
        json!({ "option": "vertexmove", "index": 999, "to": [0, 0] }),
    ] {
        assert!(run(&mut s, bad.clone()).is_err(), "{bad}");
    }
    // Two fit points left: no fewer.
    let two = h(&s.execute("spline", &json!({ "fit": [[0, 0], [5, 5]] })).unwrap());
    assert!(s.execute("splinedit", &json!({ "handle": two, "option": "fitdelete", "index": 0 })).is_err());
    // Without fit data, and closed splines have no end tangents.
    let cv = h(&s.execute("spline", &json!({ "control": [[0, 0], [2, 3], [4, 0], [6, 3]] })).unwrap());
    assert!(s.execute("splinedit", &json!({ "handle": cv, "option": "fitadd", "index": 0, "to": [1, 1] })).is_err());
    run(&mut s, json!({ "option": "close" })).unwrap();
    assert!(run(&mut s, json!({ "option": "tangents", "start": [1, 0] })).is_err());
}

#[test]
fn closed_splines_honour_the_fit_tolerance() {
    let mut s = Session::new();
    let fit: Vec<[f64; 2]> = (0..24)
        .map(|i| {
            let a = i as f64 / 24.0 * std::f64::consts::TAU;
            let r = 5.0 + if i % 2 == 0 { 0.02 } else { -0.02 };
            [a.cos() * r, a.sin() * r]
        })
        .collect();
    let sp = h(&s.execute("spline", &json!({ "fit": fit, "closed": true })).unwrap());
    let exact = spline(&s, &sp);
    assert!(exact.closed && exact.is_periodic());
    s.execute("splinedit", &json!({ "handle": sp, "option": "tolerance", "tolerance": 0.1 })).unwrap();
    let t = spline(&s, &sp);
    assert!(t.is_periodic() && t.control.len() < exact.control.len(), "{} vs {}", t.control.len(), exact.control.len());
    assert_eq!(t.fit_opts.tolerance, 0.1);
    assert_eq!(t.fit.len(), 24);
}

#[test]
fn edit_vertex_move_weight_and_delete() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({ "control": [[0, 0], [2, 3], [4, 0], [6, 3], [8, 0]] })).unwrap());
    s.cmdline("splinedit 0,0 e m n 2,4 x").unwrap();
    assert_eq!(spline(&s, &sp).control[1], Vec2::new(2.0, 4.0));
    s.cmdline("w n n 2").unwrap();
    assert_eq!(spline(&s, &sp).weights[2], 2.0);
    assert!(s.prompt_text().contains("current = 2.0000"), "{}", s.prompt_text());
    s.cmdline("x d 8,0").unwrap();
    assert_eq!(spline(&s, &sp).control.len(), 4);
    assert!(spline(&s, &sp).is_valid());
    // Degree 3 needs four control points.
    s.cmdline("0,0").unwrap();
    assert!(s.log.iter().any(|l| l.contains("at least 4 control points")), "{:?}", s.log.last());
    assert_eq!(spline(&s, &sp).control.len(), 4);
    s.cmdline("").unwrap();
    s.cmdline("x p 2").unwrap();
    assert!(s.running.is_none());
    let EntityKind::LwPolyline(p) = kind(&s, &sp) else { panic!("converted") };
    assert_eq!(p.vertices.len(), 2 * 4 + 1, "precision 2 over four control points");
}

#[test]
fn control_edits_drop_fit_data_and_keep_closed_splines_closed() {
    let mut s = Session::new();
    let sp = fit_spline(&mut s);
    s.execute("splinedit", &json!({ "handle": sp, "option": "vertexmove", "index": 1, "to": [1, 5] })).unwrap();
    let x = spline(&s, &sp);
    assert!(x.fit.is_empty() && x.control[1] == Vec2::new(1.0, 5.0));

    let c = h(&s.execute("spline", &json!({ "fit": [[0, 0], [4, 0], [4, 4], [0, 4], [-2, 2]], "closed": true })).unwrap());
    let before = spline(&s, &c);
    assert!(before.is_periodic());
    let u = before.control.len() - before.degree;
    s.execute("splinedit", &json!({ "handle": c, "option": "vertexmove", "index": 0, "to": [-1, -1] })).unwrap();
    let x = spline(&s, &c);
    assert_eq!(x.control[0], x.control[u], "the periodic repeat moves too");
    s.execute("splinedit", &json!({ "handle": c, "option": "weight", "index": 1, "weight": 3 })).unwrap();
    let x = spline(&s, &c);
    assert_eq!((x.weights[1], x.weights[1 + u]), (3.0, 3.0));
    s.execute("splinedit", &json!({ "handle": c, "option": "vertexdelete", "index": 2 })).unwrap();
    let x = spline(&s, &c);
    assert!(x.is_valid() && x.is_periodic() && x.control.len() == before.control.len() - 1 && x.weights.len() == x.control.len());
    let (lo, hi) = x.domain();
    assert!(x.eval(lo).near(x.eval(hi), 1e-9), "still closed");
    assert!(s.execute("splinedit", &json!({ "handle": c, "option": "vertexdelete", "index": 99 })).is_err());
}

/// Largest distance from samples of `a` to the curve `b` (a shape comparison).
fn shape_gap(a: &Spline, b: &Spline) -> f64 {
    let (lo, hi) = a.domain();
    let (blo, bhi) = b.domain();
    let bs: Vec<Vec2> = (0..=2000).map(|i| b.eval(blo + (bhi - blo) * i as f64 / 2000.0)).collect();
    (0..=100)
        .map(|i| a.eval(lo + (hi - lo) * i as f64 / 100.0))
        .map(|p| bs.iter().map(|q| q.dist(p)).fold(f64::INFINITY, f64::min))
        .fold(0.0, f64::max)
}

#[test]
fn join_adds_connected_curves_and_undo_brings_them_back() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({ "fit": [[0, 0], [2, 2], [4, 0]] })).unwrap());
    s.execute("line", &json!({ "points": [[4, 0], [8, 0]] })).unwrap();
    s.cmdline("splinedit 0,0 j 6,0").unwrap();
    s.cmdline("").unwrap();
    let j = spline(&s, &sp);
    let (lo, hi) = j.domain();
    assert!(j.eval(lo).near(Vec2::ZERO, 1e-9) && j.eval(hi).near(Vec2::new(8.0, 0.0), 1e-9));
    assert_eq!(s.doc().unwrap().model.len(), 1, "the line is joined in");
    s.cmdline("u").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2, "undo brings the line back");
    assert!(spline(&s, &sp).eval(spline(&s, &sp).domain().1).near(Vec2::new(4.0, 0.0), 1e-9));
    s.cmdline("x").unwrap();
    assert!(s.running.is_none());

    // JSON; closed splines and unconnected objects are refused.
    let l = h(&s.execute("line", &json!({ "points": [[0, 0], [-3, 0]] })).unwrap());
    let r = s.execute("splinedit", &json!({ "handle": sp, "option": "join", "handles2": [l] })).unwrap();
    assert_eq!(r["joined"], 1);
    let far = h(&s.execute("line", &json!({ "points": [[50, 50], [60, 60]] })).unwrap());
    assert!(s.execute("splinedit", &json!({ "handle": sp, "option": "join", "handles2": [far] })).is_err());
    assert!(s.execute("splinedit", &json!({ "handle": sp, "option": "join" })).is_err());
    let c = h(&s.execute("spline", &json!({ "fit": [[0, 0], [4, 0], [4, 4], [0, 4]], "closed": true })).unwrap());
    assert!(s.execute("splinedit", &json!({ "handle": c, "option": "join", "handles2": [far] })).is_err());
}

#[test]
fn edit_vertex_add_and_elevate_order_keep_the_shape() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({ "control": [[0, 0], [2, 3], [4, 0], [6, 3], [8, 0]] })).unwrap());
    let before = spline(&s, &sp);
    s.cmdline("splinedit 0,0 e a 4,1.5").unwrap();
    let a = spline(&s, &sp);
    assert_eq!(a.control.len(), 6);
    assert!(shape_gap(&before, &a) < 1e-6 && shape_gap(&a, &before) < 1e-6);
    s.cmdline("").unwrap();
    s.cmdline("e 5").unwrap();
    let e = spline(&s, &sp);
    assert_eq!(e.degree, 4);
    assert!(shape_gap(&before, &e) < 1e-6 && shape_gap(&e, &before) < 1e-6);
    assert!(s.prompt_text().contains("vertex editing option"), "{}", s.prompt_text());
    s.cmdline("x x").unwrap();
    assert!(s.running.is_none());

    // JSON: a periodic spline stays closed; the order only goes up, to 26 at most.
    let c = h(&s.execute("spline", &json!({ "fit": [[0, 0], [4, 0], [4, 4], [0, 4]], "closed": true })).unwrap());
    let cb = spline(&s, &c);
    s.execute("splinedit", &json!({ "handle": c, "option": "vertexadd", "at": [4, 2] })).unwrap();
    let ca = spline(&s, &c);
    assert!(ca.closed && ca.is_valid() && shape_gap(&cb, &ca) < 1e-6 && shape_gap(&ca, &cb) < 1e-6);
    s.execute("splinedit", &json!({ "handle": c, "option": "elevate", "order": 6 })).unwrap();
    assert!(spline(&s, &c).closed && spline(&s, &c).degree == 5);
    for order in [0, 3, 27, 1000] {
        assert!(s.execute("splinedit", &json!({ "handle": c, "option": "elevate", "order": order })).is_err(), "{order}");
    }
    assert!(s.execute("splinedit", &json!({ "handle": c, "option": "vertexadd" })).is_err());
}
