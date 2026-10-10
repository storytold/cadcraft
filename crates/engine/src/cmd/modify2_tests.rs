use cadcraft_doc::{EntityKind, Handle, Space};
use cadcraft_geom::{Arc, Polyline, Vec2};
use serde_json::{Value, json};

use crate::Session;

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

fn kind(s: &Session, h: &str) -> EntityKind {
    s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind.clone()
}

fn h(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    a.near(Vec2::new(x, y), 1e-6)
}

fn pline(s: &mut Session, pts: Value, closed: bool) -> String {
    h(&s.execute("pline", &json!({"vertices": pts, "closed": closed})).unwrap())
}

fn poly(k: &EntityKind) -> cadcraft_doc::LwPolyline {
    match k {
        EntityKind::LwPolyline(p) => p.clone(),
        k => panic!("not a polyline: {k:?}"),
    }
}

fn arc_of(k: &EntityKind) -> Arc {
    match k {
        EntityKind::Arc(a) => Arc::new(a.center.xy(), a.radius, a.start, a.end),
        k => panic!("not an arc: {k:?}"),
    }
}

// ---------------- PEDIT ----------------

#[test]
fn pedit_json_options() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [10, 0], [10, 10], [0, 10]]), false);
    s.execute("pedit", &json!({"handle": p, "option": "close"})).unwrap();
    assert!(poly(&kind(&s, &p)).closed);
    s.execute("pedit", &json!({"handle": p, "option": "open"})).unwrap();
    assert!(!poly(&kind(&s, &p)).closed);
    s.execute("pedit", &json!({"handle": p, "option": "width", "width": 0.5})).unwrap();
    assert_eq!(poly(&kind(&s, &p)).const_width, 0.5);
    s.execute("pedit", &json!({"handle": p, "option": "fit"})).unwrap();
    let f = poly(&kind(&s, &p));
    assert_eq!(f.vertices.len(), 7);
    assert!(f.vertices.iter().take(6).all(|v| v.bulge.abs() > 1e-6), "fit curve is all arcs");
    // Tangent continuity at an original vertex.
    let segs = Polyline { vertices: f.vertices.clone(), closed: false }.segments();
    assert!(segs[1].tangent(1.0).near(segs[2].tangent(0.0), 1e-6));
    assert!(near(f.vertices[2].p, 10.0, 0.0), "original vertices are kept");
    s.execute("pedit", &json!({"handle": p, "option": "decurve"})).unwrap();
    assert!(poly(&kind(&s, &p)).vertices.iter().all(|v| v.bulge == 0.0));
    let q = pline(&mut s, json!([[0, 20], [10, 20], [10, 30], [0, 30]]), false);
    s.execute("pedit", &json!({"handle": q, "option": "spline"})).unwrap();
    let sp = poly(&kind(&s, &q));
    assert_eq!(sp.vertices.len(), 25);
    assert!(near(sp.vertices[0].p, 0.0, 20.0) && near(sp.vertices[24].p, 0.0, 30.0));
    s.execute("pedit", &json!({"handle": q, "option": "reverse"})).unwrap();
    assert!(near(poly(&kind(&s, &q)).vertices[0].p, 0.0, 30.0));
    s.execute("pedit", &json!({"handle": q, "option": "ltypegen"})).unwrap();
    assert!(poly(&kind(&s, &q)).plinegen);
    // Vertex editing.
    let r = pline(&mut s, json!([[0, 50], [5, 50], [10, 50]]), false);
    s.execute("pedit", &json!({"handle": r, "option": "vertex", "action": "move", "index": 1, "to": [5, 55]})).unwrap();
    assert!(near(poly(&kind(&s, &r)).vertices[1].p, 5.0, 55.0));
    s.execute("pedit", &json!({"handle": r, "option": "vertex", "action": "insert", "index": 0, "to": [2, 52]})).unwrap();
    assert_eq!(poly(&kind(&s, &r)).vertices.len(), 4);
    s.execute("pedit", &json!({"handle": r, "option": "vertex", "action": "width", "index": 0, "startWidth": 1, "endWidth": 0})).unwrap();
    assert_eq!(poly(&kind(&s, &r)).vertices[0].start_width, 1.0);
    assert!(s.execute("pedit", &json!({"handle": r, "option": "vertex", "index": 99, "to": [0, 0]})).is_err());
    assert!(s.execute("pedit", &json!({"handle": r, "option": "bogus"})).is_err());
    // A line is converted, then joined with a touching arc and polyline.
    let l = h(&s.execute("line", &json!({"points": [[0, 100], [10, 100]]})).unwrap());
    let a = h(&s.execute("arc", &json!({"center": [10, 105], "radius": 5, "start": -90, "end": 90})).unwrap());
    let far = h(&s.execute("line", &json!({"points": [[50, 50], [60, 60]]})).unwrap());
    let tail = pline(&mut s, json!([[10, 110], [0, 110]]), false);
    let r = s.execute("pedit", &json!({"handle": l, "option": "join", "handles2": [a, tail, far]})).unwrap();
    assert_eq!(r["joined"], 2);
    let j = poly(&kind(&s, &l));
    assert_eq!(j.vertices.len(), 4);
    assert!(j.vertices[1].bulge > 0.9);
    assert!(s.doc().unwrap().entity(Handle::parse_hex(&far).unwrap()).is_some(), "unconnected objects stay");
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap());
    assert!(s.execute("pedit", &json!({"handle": c, "option": "close"})).is_err());
}

#[test]
fn join_keeps_selected_objects_outside_the_chain() {
    let mut s = Session::new();
    let a = h(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = h(&s.execute("line", &json!({"points": [[5, 0], [5, 5]]})).unwrap());
    let far = h(&s.execute("line", &json!({"points": [[50, 50], [60, 60]]})).unwrap());
    let c = h(&s.execute("circle", &json!({"center": [50, 50], "radius": 2})).unwrap());
    let r = s.execute("join", &json!({"handles": [a, b, far, c]})).unwrap();
    let joined = r["handle"].as_str().unwrap().to_string();
    assert_eq!(poly(&kind(&s, &joined)).vertices.len(), 3);
    let d = s.doc().unwrap();
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).is_none());
    assert!(d.entity(Handle::parse_hex(&b).unwrap()).is_none());
    assert!(d.entity(Handle::parse_hex(&far).unwrap()).is_some(), "unconnected line stays");
    assert!(d.entity(Handle::parse_hex(&c).unwrap()).is_some(), "unrelated circle stays");
}

#[test]
fn pedit_command_line() {
    let mut s = Session::new();
    pline(&mut s, json!([[0, 0], [10, 0], [10, 10]]), false);
    s.cmdline("pedit 5,0 c").unwrap();
    assert!(poly(&last(&s)).closed);
    s.cmdline("w 0.25").unwrap();
    s.cmdline("f").unwrap();
    assert!(poly(&last(&s)).vertices.len() > 3);
    s.cmdline("d").unwrap();
    assert_eq!(poly(&last(&s)).vertices.len(), 3, "decurve restores the frame");
    s.cmdline("u").unwrap();
    assert!(poly(&last(&s)).vertices.len() > 3, "undo the decurve");
    s.cmdline("u u").unwrap();
    s.cmdline("o").unwrap();
    assert!(!poly(&last(&s)).closed);
    s.cmdline("e n m 10,-5 x").unwrap();
    assert!(near(poly(&last(&s)).vertices[1].p, 10.0, -5.0));
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.state().unwrap().undo.len(), 2, "one undo step for the PEDIT session");
    // Line conversion and join.
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[5, 0], [5, 5]]})).unwrap();
    s.cmdline("pedit 2,0 y j 5,3").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert_eq!(poly(&last(&s)).vertices.len(), 3);
}

// ---------------- SPLINEDIT ----------------

#[test]
fn splinedit_json_and_command_line() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({"fit": [[0, 0], [2, 2], [4, 0], [6, 2]]})).unwrap());
    s.execute("splinedit", &json!({"handle": sp, "option": "close"})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if x.closed && x.fit.len() == 5));
    s.execute("splinedit", &json!({"handle": sp, "option": "open"})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if !x.closed && x.fit.len() == 4));
    s.execute("splinedit", &json!({"handle": sp, "option": "reverse"})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if near(x.fit[0], 6.0, 2.0)));
    s.execute("splinedit", &json!({"handle": sp, "option": "move", "index": 0, "to": [7, 3]})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if near(x.fit[0], 7.0, 3.0)));
    s.execute("splinedit", &json!({"handle": sp, "option": "refit", "fit": [[0, 0], [1, 1], [2, 0]]})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if x.fit.len() == 3));
    s.execute("splinedit", &json!({"handle": sp, "option": "purge"})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if x.fit.is_empty()));
    assert!(s.execute("splinedit", &json!({"handle": sp, "option": "refit"})).is_err());
    s.execute("splinedit", &json!({"handle": sp, "option": "move", "index": 1, "to": [1, 5]})).unwrap();
    s.execute("splinedit", &json!({"handle": sp, "option": "polyline", "precision": 4})).unwrap();
    assert!(matches!(kind(&s, &sp), EntityKind::LwPolyline(p) if p.vertices.len() > 4));
    assert!(s.execute("splinedit", &json!({"handle": sp, "option": "close"})).is_err());

    let mut s = Session::new();
    s.execute("spline", &json!({"control": [[0, 0], [2, 3], [4, 0], [6, 3]]})).unwrap();
    s.cmdline("splinedit 0,0 c").unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(x) if x.closed));
    s.cmdline("o r u").unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(x) if !x.closed && near(x.control[0], 0.0, 0.0)));
    s.cmdline("f p p").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::LwPolyline(_)));
}

// ---------------- LENGTHEN ----------------

#[test]
fn lengthen_json() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    assert_eq!(s.execute("lengthen", &json!({"handle": l})).unwrap()["length"], 10.0);
    s.execute("lengthen", &json!({"handle": l, "delta": 2})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if (x.b.x - 12.0).abs() < 1e-9));
    s.execute("lengthen", &json!({"handle": l, "delta": -2, "pick": [0, 0]})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if (x.a.x - 2.0).abs() < 1e-9));
    s.execute("lengthen", &json!({"handle": l, "percent": 50})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if (x.b.x - 7.0).abs() < 1e-9));
    s.execute("lengthen", &json!({"handle": l, "total": 3})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if (x.b.x - 5.0).abs() < 1e-9));
    s.execute("lengthen", &json!({"handle": l, "to": [9, 4]})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if (x.b.x - 9.0).abs() < 1e-9 && x.b.y == 0.0));
    assert!(s.execute("lengthen", &json!({"handle": l, "delta": -100})).is_err());
    assert!(s.execute("lengthen", &json!({"handle": l, "deltaAngle": 10})).is_err());
    let a = h(&s.execute("arc", &json!({"center": [0, 0], "radius": 1, "start": 0, "end": 90})).unwrap());
    s.execute("lengthen", &json!({"handle": a, "deltaAngle": 90})).unwrap();
    assert!((arc_of(&kind(&s, &a)).sweep() - std::f64::consts::PI).abs() < 1e-9);
    s.execute("lengthen", &json!({"handle": a, "totalAngle": 45, "pick": [1, 0]})).unwrap();
    let g = arc_of(&kind(&s, &a));
    assert!((g.sweep() - std::f64::consts::FRAC_PI_4).abs() < 1e-9 && near(g.end_point(), -1.0, 0.0));
    s.execute("lengthen", &json!({"handle": a, "total": std::f64::consts::FRAC_PI_2})).unwrap();
    assert!((arc_of(&kind(&s, &a)).sweep() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert!(s.execute("lengthen", &json!({"handle": a, "percent": 1000})).is_err());
    let p = pline(&mut s, json!([[0, 10], [5, 10], [5, 15]]), false);
    s.execute("lengthen", &json!({"handle": p, "delta": 5})).unwrap();
    assert!(near(poly(&kind(&s, &p)).vertices[2].p, 5.0, 20.0));
    s.execute("lengthen", &json!({"handle": p, "total": 20, "pick": [0, 10]})).unwrap();
    assert!(near(poly(&kind(&s, &p)).vertices[0].p, -5.0, 10.0));
}

#[test]
fn lengthen_command_line() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.cmdline("lengthen 5,0").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Current length: 10.0000")));
    s.cmdline("de 2 9,0 9,0 u").unwrap();
    assert!(matches!(last(&s), EntityKind::Line(x) if (x.b.x - 12.0).abs() < 1e-9));
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    s.cmdline("lengthen t 4 1,0").unwrap();
    s.cmdline("").unwrap();
    assert!(matches!(last(&s), EntityKind::Line(x) if (x.a.x - 8.0).abs() < 1e-9));
    s.cmdline("lengthen p 50 11,0").unwrap();
    s.cmdline("").unwrap();
    assert!(matches!(last(&s), EntityKind::Line(x) if (x.b.x - 10.0).abs() < 1e-9));
    s.cmdline("lengthen dy 9.5,0 20,3").unwrap();
    s.cmdline("").unwrap();
    assert!(matches!(last(&s), EntityKind::Line(x) if (x.b.x - 20.0).abs() < 1e-9));
    s.execute("arc", &json!({"center": [50, 0], "radius": 1, "start": 0, "end": 90})).unwrap();
    s.cmdline("lengthen de a 90 50,1").unwrap();
    s.cmdline("").unwrap();
    assert!((arc_of(&last(&s)).sweep() - std::f64::consts::PI).abs() < 1e-9);
}

// ---------------- ALIGN / BLEND / REVERSE ----------------

#[test]
fn align_json_and_command_line() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [2, 0]]})).unwrap());
    s.execute("align", &json!({"handles": [l], "s1": [0, 0], "d1": [5, 5], "s2": [2, 0], "d2": [5, 9], "scale": true})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if near(x.a.xy(), 5.0, 5.0) && near(x.b.xy(), 5.0, 9.0)));
    s.execute("align", &json!({"handles": [l], "s1": [5, 5], "d1": [0, 0]})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if near(x.a.xy(), 0.0, 0.0) && near(x.b.xy(), 0.0, 4.0)));

    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [2, 0]]})).unwrap();
    s.cmdline("align 1,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("0,0 10,10 2,0 10,11").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Line(x) if near(x.a.xy(), 10.0, 10.0) && near(x.b.xy(), 10.0, 12.0)));
    // One pair only: a move.
    s.cmdline("align 10,11").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("10,10 0,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Line(x) if near(x.a.xy(), 0.0, 0.0)));
}

#[test]
fn blend_json_and_command_line() {
    let mut s = Session::new();
    let a = h(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = h(&s.execute("line", &json!({"points": [[10, 3], [15, 3]]})).unwrap());
    let r = s.execute("blend", &json!({"h1": a, "p1": [4, 0], "h2": b, "p2": [11, 3]})).unwrap();
    match kind(&s, &h(&r)) {
        EntityKind::Spline(sp) => {
            assert_eq!(sp.degree, 3);
            assert!(near(sp.control[0], 5.0, 0.0) && near(sp.control[3], 10.0, 3.0));
            assert!(sp.control[1].y.abs() < 1e-9 && sp.control[1].x > 5.0, "tangent to the first line");
            assert!((sp.control[2].y - 3.0).abs() < 1e-9 && sp.control[2].x < 10.0, "tangent to the second line");
        }
        k => panic!("{k:?}"),
    }
    let r = s.execute("blend", &json!({"h1": a, "p1": [1, 0], "h2": b, "p2": [14, 3], "continuity": "smooth"})).unwrap();
    assert!(matches!(kind(&s, &h(&r)), EntityKind::Spline(sp) if sp.degree == 5 && near(sp.control[0], 0.0, 0.0)));
    let c = h(&s.execute("circle", &json!({"center": [30, 0], "radius": 1})).unwrap());
    assert!(s.execute("blend", &json!({"h1": a, "p1": [1, 0], "h2": c, "p2": [31, 0]})).is_err());

    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap();
    s.execute("arc", &json!({"center": [10, 0], "radius": 2, "start": 90, "end": 180})).unwrap();
    s.cmdline("blend con s 4,0 8,0").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.degree == 5 && near(*sp.control.last().unwrap(), 8.0, 0.0)));
}

#[test]
fn reverse_json_and_command_line() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let p = pline(&mut s, json!([{"p": [0, 5], "bulge": 1}, [2, 5], [4, 6]]), false);
    let sp = h(&s.execute("spline", &json!({"control": [[0, 10], [1, 12], [3, 12], [4, 10]]})).unwrap());
    let p3 = h(&s.execute("3dpoly", &json!({"points": [[0, 20, 0], [1, 20, 1]]})).unwrap());
    let c = h(&s.execute("circle", &json!({"center": [0, 30], "radius": 1})).unwrap());
    let before = Polyline { vertices: poly(&kind(&s, &p)).vertices, closed: false }.tessellate(1e-3);
    let r = s.execute("reverse", &json!({"handles": [l, p, sp, p3, c]})).unwrap();
    assert_eq!(r["reversed"], 4);
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if near(x.a.xy(), 5.0, 0.0)));
    let after = Polyline { vertices: poly(&kind(&s, &p)).vertices, closed: false }.tessellate(1e-3);
    assert!(after.first().unwrap().near(*before.last().unwrap(), 1e-9));
    let mid_b = before[before.len() / 2];
    assert!(after.iter().any(|q| q.near(mid_b, 1e-2)), "same shape");
    assert!(matches!(kind(&s, &sp), EntityKind::Spline(x) if near(x.eval(x.domain().0), 4.0, 10.0)));
    assert!(matches!(kind(&s, &p3), EntityKind::Polyline3d(x) if x.points[0].z == 1.0));

    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap();
    s.cmdline("reverse 2,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Line(x) if near(x.a.xy(), 5.0, 0.0)));
}

// ---------------- NCOPY / CHSPACE / FLATTEN ----------------

fn make_block(s: &mut Session) -> String {
    let a = h(&s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap());
    let c = h(&s.execute("circle", &json!({"center": [3, 0], "radius": 0.5})).unwrap());
    s.execute("block", &json!({"name": "PART", "base": [0, 0], "handles": [a, c], "keep": "delete"})).unwrap();
    h(&s.execute("insert", &json!({"name": "PART", "at": [10, 10], "rotation": 90})).unwrap())
}

#[test]
fn ncopy_json_and_command_line() {
    let mut s = Session::new();
    let ins = make_block(&mut s);
    let r = s.execute("ncopy", &json!({"handle": ins, "pick": [10, 13]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    assert!(matches!(last(&s), EntityKind::Circle(c) if near(c.center.xy(), 10.0, 13.0)));
    let r = s.execute("ncopy", &json!({"handle": ins, "delta": [5, 0]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    assert!(matches!(last(&s), EntityKind::Circle(c) if near(c.center.xy(), 15.0, 13.0)));
    let line = h(&s.execute("line", &json!({"points": [[50, 50], [51, 50]]})).unwrap());
    assert!(s.execute("ncopy", &json!({"handle": line})).is_err());

    let mut s = Session::new();
    make_block(&mut s);
    s.cmdline("ncopy 10,10.5").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("0,0 20,0").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Line(l) if near(l.a.xy(), 30.0, 10.0) && near(l.b.xy(), 30.0, 11.0)));
}

#[test]
fn chspace_json_and_command_line() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    // A viewport on Layout1 showing model (0,0) at paper (5,5) at scale 1:2.
    let vp = cadcraft_doc::Viewport {
        center: cadcraft_geom::Vec3::new(5.0, 5.0, 0.0),
        width: 8.0,
        height: 6.0,
        view_center: Vec2::ZERO,
        view_height: 12.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
    };
    s.doc_mut().unwrap().add(&Space::Paper("Layout1".into()), Default::default(), EntityKind::Viewport(vp)).unwrap();
    let r = s.execute("chspace", &json!({"handles": [l]})).unwrap();
    assert_eq!(r["moved"], 1);
    assert_eq!(r["to"], "Layout1");
    let hh = Handle::parse_hex(&l).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.space_of(hh), Some(Space::Paper("Layout1".into())));
    assert!(matches!(&d.entity(hh).unwrap().kind, EntityKind::Line(x) if near(x.a.xy(), 5.0, 5.0) && near(x.b.xy(), 10.0, 5.0)));
    // Back to model space from the layout.
    s.state_mut().unwrap().space = Space::Paper("Layout1".into());
    s.execute("chspace", &json!({"handles": [l]})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.space_of(hh), Some(Space::Model));
    assert!(matches!(&d.entity(hh).unwrap().kind, EntityKind::Line(x) if near(x.b.xy(), 10.0, 0.0)));
    s.state_mut().unwrap().space = Space::Model;
    assert!(s.execute("chspace", &json!({"handles": [l], "to": "Nope"})).is_err());

    s.cmdline("chspace 5,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().space_of(hh), Some(Space::Paper("Layout1".into())));
    assert!(s.log.iter().any(|l| l.contains("changed from MODEL space to PAPER space")));
}

#[test]
fn flatten_json_and_command_line() {
    let mut s = Session::new();
    let p3 = h(&s.execute("3dpoly", &json!({"points": [[0, 0, 5], [1, 0, 6], [1, 1, 7]]})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[0, 5], [3, 5]]})).unwrap());
    s.doc_mut()
        .unwrap()
        .modify_entity(Handle::parse_hex(&l).unwrap(), |e| {
            if let EntityKind::Line(x) = &mut e.kind {
                x.a.z = 4.0;
            }
            e.common.thickness = 2.0;
        })
        .unwrap();
    let r = s.execute("flatten", &json!({"handles": [p3, l]})).unwrap();
    assert_eq!(r["flattened"], 2);
    assert!(matches!(kind(&s, &p3), EntityKind::LwPolyline(p) if p.vertices.len() == 3));
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if x.a.z == 0.0));
    assert_eq!(s.doc().unwrap().entity(Handle::parse_hex(&l).unwrap()).unwrap().common.thickness, 0.0);

    let mut s = Session::new();
    s.execute("3dpoly", &json!({"points": [[0, 0, 5], [4, 0, 6]]})).unwrap();
    s.cmdline("flatten 2,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::LwPolyline(_)));
}

// ---------------- FILLET extensions ----------------

#[test]
fn fillet_line_arc_and_arcs() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[-10, 0], [10, 0]]})).unwrap());
    let a = h(&s.execute("arc", &json!({"center": [0, 4], "radius": 3, "start": 180, "end": 360})).unwrap());
    // Arc bottom is at y=1; a fillet of radius 1 between the line (below) and the arc.
    let r = s.execute("fillet", &json!({"h1": l, "p1": [8, 0], "h2": a, "p2": [3, 4], "radius": 1})).unwrap();
    let fa = arc_of(&kind(&s, r["arc"].as_str().unwrap()));
    assert!((fa.radius - 1.0).abs() < 1e-9 && (fa.center.y - 1.0).abs() < 1e-9 && fa.center.x > 0.0, "{fa:?}");
    // Tangent points: the line now ends under the fillet centre; the arc ends where it touches.
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if (x.a.x - fa.center.x).abs() < 1e-9 && x.b.x == 10.0));
    let ga = arc_of(&kind(&s, &a));
    assert!((ga.start_point().dist(fa.center) - 1.0).abs() < 1e-6 || (ga.end_point().dist(fa.center) - 1.0).abs() < 1e-6);
    // Endpoints connect.
    assert!(fa.start_point().near(Vec2::new(fa.center.x, 0.0), 1e-6) || fa.end_point().near(Vec2::new(fa.center.x, 0.0), 1e-6));

    // Two arcs.
    let mut s = Session::new();
    let a1 = h(&s.execute("arc", &json!({"center": [0, 0], "radius": 2, "start": 0, "end": 90})).unwrap());
    let a2 = h(&s.execute("arc", &json!({"center": [5, 0], "radius": 2, "start": 90, "end": 180})).unwrap());
    let r = s.execute("fillet", &json!({"h1": a1, "p1": [1.4, 1.4], "h2": a2, "p2": [3.6, 1.4], "radius": 1})).unwrap();
    let fa = arc_of(&kind(&s, r["arc"].as_str().unwrap()));
    assert!((fa.center.x - 2.5).abs() < 1e-9 && fa.center.y > 0.0);
    let g1 = arc_of(&kind(&s, &a1));
    let g2 = arc_of(&kind(&s, &a2));
    assert!(
        fa.start_point().near(g1.start_point(), 1e-6) || fa.start_point().near(g1.end_point(), 1e-6) || fa.end_point().near(g1.end_point(), 1e-6)
    );
    assert!(
        fa.start_point().near(g2.start_point(), 1e-6) || fa.end_point().near(g2.start_point(), 1e-6) || fa.end_point().near(g2.end_point(), 1e-6)
    );
    // Zero radius: sharp corner between a line and an arc.
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[-10, 0], [-5, 0]]})).unwrap());
    let a = h(&s.execute("arc", &json!({"center": [0, 0], "radius": 3, "start": 90, "end": 150})).unwrap());
    s.execute("fillet", &json!({"h1": l, "p1": [-8, 0], "h2": a, "p2": [0, 3], "radius": 0})).unwrap();
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if near(x.b.xy(), -3.0, 0.0)));
    assert!(near(arc_of(&kind(&s, &a)).end_point(), -3.0, 0.0));
    // Unsupported pairs report an error.
    let t = h(&s.execute("text", &json!({"at": [0, 0], "text": "x"})).unwrap());
    assert!(s.execute("fillet", &json!({"h1": l, "p1": [-8, 0], "h2": t, "p2": [0, 0], "radius": 1})).is_err());
}

#[test]
fn fillet_polyline_json_and_command_line() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [10, 0], [10, 10], [0, 10]]), true);
    let r = s.execute("fillet", &json!({"handle": p, "polyline": true, "radius": 1})).unwrap();
    assert_eq!(r["filleted"], 4);
    let pl = poly(&kind(&s, &p));
    assert_eq!(pl.vertices.len(), 8);
    let area = Polyline { vertices: pl.vertices.clone(), closed: true }.area();
    assert!((area - (100.0 - 4.0 + std::f64::consts::PI)).abs() < 1e-3, "{area}");
    // Too-large radius: corners that do not fit are skipped.
    let q = pline(&mut s, json!([[0, 20], [2, 20], [2, 22]]), false);
    assert_eq!(s.execute("fillet", &json!({"handle": q, "polyline": true, "radius": 5})).unwrap()["filleted"], 0);

    let mut s = Session::new();
    pline(&mut s, json!([[0, 0], [10, 0], [10, 10]]), false);
    s.cmdline("fillet r 2").unwrap();
    s.cmdline("p 5,0").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(poly(&last(&s)).vertices.len(), 4);
    assert!(s.log.iter().any(|l| l.contains("1 lines were filleted")));
}

#[test]
fn fillet_command_line_line_and_arc() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[-10, 0], [10, 0]]})).unwrap();
    s.execute("arc", &json!({"center": [0, 4], "radius": 3, "start": 180, "end": 360})).unwrap();
    s.cmdline("fillet r 1").unwrap();
    s.cmdline("8,0 3,4").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Arc(a) if (a.radius - 1.0).abs() < 1e-9));
}

// ---------------- TRIM / EXTEND for ellipses and splines ----------------

#[test]
fn trim_and_extend_ellipse_and_spline() {
    let mut s = Session::new();
    let e = h(&s.execute("ellipse", &json!({"center": [0, 0], "major": [4, 0], "ratio": 0.5})).unwrap());
    s.execute("line", &json!({"points": [[0, -5], [0, 5]]})).unwrap();
    let r = s.execute("trim", &json!({"handle": e, "pick": [4, 0]})).unwrap();
    let nh = r["handles"][0].as_str().unwrap().to_string();
    match kind(&s, &nh) {
        EntityKind::Ellipse(x) => {
            let ge = cadcraft_geom::Ellipse { center: x.center.xy(), major: x.major.xy(), ratio: x.ratio, start: x.start, end: x.end };
            assert!((ge.sweep() - std::f64::consts::PI).abs() < 1e-6);
            assert!(ge.at_param(ge.start + ge.sweep() / 2.0).x < 0.0, "the right half was removed");
        }
        k => panic!("{k:?}"),
    }
    // Extend the elliptical arc back to a new edge.
    s.execute("line", &json!({"points": [[3, -5], [3, 5]]})).unwrap();
    let mut s2 = Session::new();
    let ea = h(&s2.execute("ellipse", &json!({"center": [0, 0], "major": [4, 0], "ratio": 0.5, "start": 90, "end": 180})).unwrap());
    s2.execute("line", &json!({"points": [[2, -5], [2, 5]]})).unwrap();
    s2.execute("extend", &json!({"handle": ea, "pick": [0, 2]})).unwrap();
    match kind(&s2, &ea) {
        EntityKind::Ellipse(x) => {
            let ge = cadcraft_geom::Ellipse { center: x.center.xy(), major: x.major.xy(), ratio: x.ratio, start: x.start, end: x.end };
            assert!((ge.at_param(ge.start).x - 2.0).abs() < 1e-6, "{:?}", ge.at_param(ge.start));
        }
        k => panic!("{k:?}"),
    }
    // Splines.
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({"control": [[0, 0], [3, 3], [6, -3], [9, 0]]})).unwrap());
    s.execute("line", &json!({"points": [[4.5, -5], [4.5, 5]]})).unwrap();
    let r = s.execute("trim", &json!({"handle": sp, "pick": [8.9, 0.0]})).unwrap();
    let nh = r["handles"][0].as_str().unwrap().to_string();
    match kind(&s, &nh) {
        EntityKind::Spline(x) => {
            assert!(near(x.eval(x.domain().0), 0.0, 0.0));
            assert!((x.eval(x.domain().1).x - 4.5).abs() < 1e-6);
        }
        k => panic!("{k:?}"),
    }
    assert!(s.execute("extend", &json!({"handle": nh, "pick": [4.5, 0]})).is_err());
}

#[test]
fn new_modify_commands_survive_garbage_input() {
    let ids = ["pedit", "splinedit", "lengthen", "align", "blend", "reverse", "ncopy", "chspace", "flatten", "fillet", "trim", "extend"];
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let sp = h(&s.execute("spline", &json!({"control": [[0, 1], [1, 2], [2, 1]]})).unwrap());
    let e = h(&s.execute("ellipse", &json!({"center": [5, 5], "major": [1, 0], "ratio": 0.5})).unwrap());
    for id in ids {
        for p in [
            json!({"handle": l, "option": "vertex", "action": "insert", "index": u64::MAX, "to": [1e308, 1e308]}),
            json!({"handle": sp, "option": "move", "index": 1e10, "to": [0, 0]}),
            json!({"handle": l, "delta": 1e308, "percent": -1, "to": [1e308, -1e308]}),
            json!({"handles": [l, sp, e], "s1": [0, 0], "d1": [0, 0], "s2": [0, 0], "d2": [1e308, 0], "scale": true}),
            json!({"h1": l, "p1": [0, 0], "h2": l, "p2": [0, 0], "radius": 1e308}),
            json!({"handle": e, "pick": [1e308, 0], "polyline": true}),
            json!({"handle": sp, "pick": [0, 0]}),
            json!({"handles": ["zz"], "to": ""}),
        ] {
            let _ = s.execute(id, &p);
        }
        s.start(id).unwrap();
        for t in ["5,0", "", "x", "1e999", "c", "e", "n", "m", "0,0", "5,5", "u", "x", "", ""] {
            if s.running.is_none() {
                break;
            }
            let _ = s.cmdline(t);
        }
        let _ = s.preview(Vec2::new(-1e300, 1e300));
        s.cancel();
    }
}

#[test]
fn arraypath_json_and_command_line() {
    let mut s = Session::new();
    let dot = h(&s.execute("circle", &json!({"center": [0, 1], "radius": 0.25})).unwrap());
    let path = h(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let r = s.execute("arraypath", &json!({"handles": [dot], "path": path, "count": 6})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 5);
    assert!(matches!(last(&s), EntityKind::Circle(c) if near(c.center.xy(), 10.0, 1.0)));
    let r = s.execute("arraypath", &json!({"handles": [dot], "path": path, "spacing": 4})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    // Aligned along an arc: the copy turns with the tangent.
    let arc = h(&s.execute("arc", &json!({"center": [0, 20], "radius": 5, "start": 0, "end": 180})).unwrap());
    let tick = h(&s.execute("line", &json!({"points": [[5, 20], [6, 20]]})).unwrap());
    s.execute("arraypath", &json!({"handles": [tick], "path": arc, "count": 2})).unwrap();
    assert!(matches!(last(&s), EntityKind::Line(l) if near(l.a.xy(), -5.0, 20.0) && near(l.b.xy(), -6.0, 20.0)));
    assert!(s.execute("arraypath", &json!({"handles": [dot], "path": dot})).is_err());

    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 1], "radius": 0.25})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.cmdline("arraypath 0.25,1").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("5,0 3").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(s.doc().unwrap().model.len(), 4);
}

#[test]
fn textedit_json_and_command_line() {
    let mut s = Session::new();
    let t = h(&s.execute("text", &json!({"at": [0, 0], "text": "old"})).unwrap());
    s.execute("textedit", &json!({"handle": t, "text": "new"})).unwrap();
    assert!(matches!(kind(&s, &t), EntityKind::Text(x) if x.value == "new"));
    let m = h(&s.execute("mtext", &json!({"at": [0, 5], "text": "a"})).unwrap());
    s.execute("textedit", &json!({"handle": m, "text": "x\ny"})).unwrap();
    assert!(matches!(kind(&s, &m), EntityKind::MText(x) if x.contents == "x\\Py"));
    let l = h(&s.execute("line", &json!({"points": [[0, 10], [1, 10]]})).unwrap());
    assert!(s.execute("textedit", &json!({"handle": l, "text": "x"})).is_err());
    s.cmdline("textedit 0,0").unwrap();
    s.cmdline("hello world").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(kind(&s, &t), EntityKind::Text(x) if x.value == "hello world"));
}

#[test]
fn explode_polyline3d_keeps_elevations() {
    let mut s = Session::new();
    let p = h(&s.execute("3dpoly", &json!({"points": [[0, 0, 5], [1, 0, 6], [1, 1, 7]]})).unwrap());
    s.execute("explode", &json!({"handles": [p]})).unwrap();
    let lines: Vec<(f64, f64)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.z, l.b.z)),
            _ => None,
        })
        .collect();
    assert_eq!(lines, vec![(5.0, 6.0), (6.0, 7.0)]);
}
