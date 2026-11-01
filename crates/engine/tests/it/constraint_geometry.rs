//! Geometric constraints on ellipses, splines and block references, Smooth (G2) and the
//! degrees-of-freedom report (`constraints.dof`, `constraints.inspect`) (#546).

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

fn last(s: &Session) -> Handle {
    s.doc().unwrap().model.iter().last().map(|e| e.handle).unwrap()
}

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn add(s: &mut Session, cmd: &str, p: Value) -> Handle {
    run(s, cmd, p);
    last(s)
}

fn kind(s: &Session, h: Handle) -> EntityKind {
    s.doc().unwrap().entity(h).unwrap().kind.clone()
}

#[test]
fn ellipse_points_and_tangent_line_from_json() {
    let mut s = Session::new();
    let e = add(&mut s, "ellipse", json!({ "center": [0, 0], "major": [6, 0.4], "ratio": 0.5 }));
    let l = add(&mut s, "line", json!({ "points": [[-8, 3.5], [8, 3.2]] }));
    run(&mut s, "gchorizontal", json!({ "h1": e.hex() }));
    run(&mut s, "gctangent", json!({ "h1": l.hex(), "h2": e.hex() }));
    // An axis end point by name, and a picked one.
    let m = add(&mut s, "line", json!({ "points": [[6.3, 0.2], [9, 4]] }));
    run(&mut s, "gccoincident", json!({ "h1": e.hex(), "p1": "v0", "h2": m.hex(), "p2": "start" }));
    let EntityKind::Ellipse(el) = kind(&s, e) else { panic!() };
    assert!(el.major.y.abs() < 1e-9);
    let EntityKind::Line(ml) = kind(&s, m) else { panic!() };
    assert!(ml.a.xy().near(el.center.xy() + el.major.xy(), 1e-7));
    let EntityKind::Line(tl) = kind(&s, l) else { panic!() };
    let n = (tl.b.xy() - tl.a.xy()).normalized().perp();
    let minor = el.major.xy().perp() * el.ratio;
    let support = (n.dot(el.major.xy()).powi(2) + n.dot(minor).powi(2)).sqrt();
    assert!(((el.center.xy() - tl.a.xy()).dot(n).abs() - support).abs() < 1e-7);
    // Concentric accepts an ellipse.
    let c = add(&mut s, "circle", json!({ "center": [0.4, 0.3], "radius": 1 }));
    run(&mut s, "gcconcentric", json!({ "h1": e.hex(), "h2": c.hex() }));
    let i = run(&mut s, "constraints.inspect", json!({}));
    assert_eq!(i["satisfied"], json!(true));
    assert_eq!(i["glyphs"].as_array().unwrap().len(), 4);
    assert!(i["dof"]["dof"].as_u64().is_some(), "{}", i["dof"]);
}

#[test]
fn spline_smooth_to_arc_is_g2_and_reports_dof() {
    let mut s = Session::new();
    let a = add(&mut s, "arc", json!({ "center": [0, 3], "radius": 3, "start": 180, "end": 270 }));
    let sp = add(&mut s, "spline", json!({ "control": [[0.2, 0.1], [3, 0.4], [6, 2], [9, 2]], "degree": 3 }));
    run(&mut s, "gcfix", json!({ "h1": a.hex() }));
    let c = run(&mut s, "gcsmooth", json!({ "h1": sp.hex(), "h2": a.hex() }));
    assert_eq!(c["type"], json!("Smooth"));
    let EntityKind::Spline(g) = kind(&s, sp) else { panic!() };
    let (lo, _) = g.domain();
    assert!(g.eval(lo).near(Vec2::new(0.0, 0.0), 1e-7));
    // Spline: 8 parameters less 4 equations (point, tangent, curvature).
    let r = run(&mut s, "constraints.dof", json!({ "handles": [sp.hex()] }));
    assert_eq!(r["dof"], json!(4), "{r}");
    assert_eq!(r["status"], json!("under"));
    assert_eq!(r["objects"][0]["params"], json!(8));
    // Fix the rest: fully constrained.
    for v in ["v2", "v3"] {
        run(&mut s, "gcfix", json!({ "h1": sp.hex(), "p1": v }));
    }
    let r = run(&mut s, "constraints.dof", json!({}));
    assert_eq!((r["dof"].clone(), r["status"].clone()), (json!(0), json!("fully")), "{r}");
    assert_eq!(r["message"], json!("Fully constrained"));
    // One more is refused as redundant.
    let e = s.execute("gcfix", &json!({ "h1": sp.hex(), "p1": "start" })).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
}

#[test]
fn block_reference_insertion_and_rigid_points() {
    let mut s = Session::new();
    let l = add(&mut s, "line", json!({ "points": [[0, 0], [4, 0]] }));
    run(&mut s, "block", json!({ "name": "BAR", "base": [0, 0], "handles": [l.hex()], "keep": "delete" }));
    let b = add(&mut s, "insert", json!({ "name": "BAR", "at": [1, 1], "rotation": 60 }));
    let p = add(&mut s, "point", json!({ "at": [10, 10] }));
    let wall = add(&mut s, "line", json!({ "points": [[10, 0], [10, 20]] }));
    run(&mut s, "gcfix", json!({ "h1": p.hex() }));
    run(&mut s, "gcfix", json!({ "h1": wall.hex() }));
    run(&mut s, "gccoincident", json!({ "h1": b.hex(), "p1": "object", "h2": p.hex() }));
    // Rigid point 1 is the block line's far end, picked by position.
    let EntityKind::Insert(i) = kind(&s, b) else { panic!() };
    let far = i.insert.xy() + Vec2::new(4.0, 0.0).rotate(i.rotation);
    run(&mut s, "gccoincident", json!({ "h1": b.hex(), "p1": [far.x, far.y], "h2": wall.hex(), "p2": "object" }));
    let EntityKind::Insert(i) = kind(&s, b) else { panic!() };
    assert!(i.insert.xy().near(Vec2::new(10.0, 10.0), 1e-7));
    assert!((i.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-7);
    let r = run(&mut s, "constraints.dof", json!({ "handle": b.hex() }));
    assert_eq!(r["objects"][0]["dof"], json!(0), "{r}");
    assert_eq!(r["objects"][0]["type"], json!("Block Reference"));
}

#[test]
fn dof_of_unconstrained_selection_and_hostile_params() {
    let mut s = Session::new();
    let l = add(&mut s, "line", json!({ "points": [[0, 0], [4, 0]] }));
    let r = run(&mut s, "constraints.dof", json!({ "handles": [l.hex()] }));
    assert_eq!((r["dof"].clone(), r["status"].clone()), (json!(4), json!("unconstrained")));
    let r = run(&mut s, "constraints.dof", json!({}));
    assert_eq!(r["message"], json!("No constraints"));
    for p in [json!({ "handles": "x" }), json!({ "handles": [1e300, -1, "zz", null] }), json!({ "handle": [] })] {
        let _ = s.execute("constraints.dof", &p);
    }
    let e = add(&mut s, "ellipse", json!({ "center": [0, 0], "major": [1e-9, 0], "ratio": 1e-6, "start": 10, "end": 20 }));
    for t in ["coincident", "tangent", "smooth", "concentric", "horizontal", "fix", "symmetric"] {
        for p in ["v9", "v4294967295", "start", "end", "mid", "center", "seg0", "object"] {
            let _ = s.execute("geomconstraint", &json!({ "type": t, "h1": e.hex(), "p1": p, "h2": l.hex(), "h3": l.hex() }));
        }
    }
    let _ = s.execute("constraints.dof", &json!({}));
    let _ = s.execute("constraints.inspect", &json!({}));
}
