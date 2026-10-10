use cadcraft_doc::EntityKind;
use cadcraft_geom::{Arc, Vec2};
use serde_json::json;

use crate::{Input, Session};

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

fn kinds(s: &Session) -> Vec<&'static str> {
    s.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect()
}

fn as_arc(k: &EntityKind) -> Arc {
    match k {
        EntityKind::Arc(a) => Arc::new(a.center.xy(), a.radius, a.start, a.end),
        other => panic!("not an arc: {other:?}"),
    }
}

fn as_circle(k: &EntityKind) -> (Vec2, f64) {
    match k {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        other => panic!("not a circle: {other:?}"),
    }
}

fn h(r: &serde_json::Value) -> String {
    r["handle"].as_str().unwrap().to_string()
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    a.near(Vec2::new(x, y), 1e-6)
}

// ---------------- ARC ----------------

#[test]
fn arc_variants_json() {
    let mut s = Session::new();
    let quarter =
        |a: &Arc| near(a.center, 0.0, 0.0) && (a.radius - 1.0).abs() < 1e-9 && near(a.start_point(), 1.0, 0.0) && near(a.end_point(), 0.0, 1.0);
    for (id, p) in [
        ("arc.sce", json!({"start": [1, 0], "center": [0, 0], "end": [0, 5]})),
        ("arc.sca", json!({"start": [1, 0], "center": [0, 0], "angle": 90})),
        ("arc.scl", json!({"start": [1, 0], "center": [0, 0], "length": 2f64.sqrt()})),
        ("arc.sea", json!({"start": [1, 0], "end": [0, 1], "angle": 90})),
        ("arc.sed", json!({"start": [1, 0], "end": [0, 1], "direction": 90})),
        ("arc.ser", json!({"start": [1, 0], "end": [0, 1], "radius": 1})),
        ("arc.cse", json!({"center": [0, 0], "start": [1, 0], "end": [0, 3]})),
        ("arc.csa", json!({"center": [0, 0], "start": [1, 0], "angle": 90})),
        ("arc.csl", json!({"center": [0, 0], "start": [1, 0], "length": 2f64.sqrt()})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(quarter(&as_arc(&last(&s))), "{id}: {:?}", last(&s));
    }
    // Negative angle: clockwise from the start.
    s.execute("arc.sca", &json!({"start": [1, 0], "center": [0, 0], "angle": -90})).unwrap();
    let a = as_arc(&last(&s));
    assert!(near(a.start_point(), 0.0, -1.0) && near(a.end_point(), 1.0, 0.0));
    // Negative radius / chord: the major arc.
    s.execute("arc.ser", &json!({"start": [1, 0], "end": [0, 1], "radius": -1})).unwrap();
    assert!((as_arc(&last(&s)).sweep() - 1.5 * std::f64::consts::PI).abs() < 1e-9);
    s.execute("arc.scl", &json!({"start": [1, 0], "center": [0, 0], "length": -(2f64.sqrt())})).unwrap();
    assert!((as_arc(&last(&s)).sweep() - 1.5 * std::f64::consts::PI).abs() < 1e-9);
    // Direction as a vector, and bad input.
    s.execute("arc.sed", &json!({"start": [1, 0], "end": [0, 1], "direction": [0, 1]})).unwrap();
    assert!(quarter(&as_arc(&last(&s))));
    assert!(s.execute("arc.ser", &json!({"start": [0, 0], "end": [10, 0], "radius": 1})).is_err());
    assert!(s.execute("arc.scl", &json!({"start": [1, 0], "center": [0, 0], "length": 5})).is_err());
    assert!(s.execute("arc.sea", &json!({"start": [1, 0], "end": [1, 0], "angle": 90})).is_err());
}

#[test]
fn arc_variants_command_line() {
    for (cmd, expect_quarter) in [
        ("arc.sce 1,0 0,0 0,2", true),
        ("arc.sca 1,0 0,0 90", true),
        ("arc.scl 1,0 0,0 1.41421356237", true),
        ("arc.sea 1,0 0,1 90", true),
        ("arc.sed 1,0 0,1 90", true),
        ("arc.ser 1,0 0,1 1", true),
        ("arc.cse 0,0 1,0 0,4", true),
        ("arc.csa 0,0 1,0 90", true),
        ("arc.csl 0,0 1,0 1.41421356237", true),
        // Angle/direction given by a point.
        ("arc.sed 1,0 0,1 1,5", true),
        ("arc.csa 0,0 1,0 0,7", true),
    ] {
        let mut s = Session::new();
        s.cmdline(cmd).unwrap();
        assert!(s.running.is_none(), "{cmd} still running: {}", s.prompt_text());
        let a = as_arc(&last(&s));
        if expect_quarter {
            assert!(near(a.center, 0.0, 0.0) && near(a.start_point(), 1.0, 0.0) && near(a.end_point(), 0.0, 1.0), "{cmd}: {a:?}");
        }
        assert_eq!(s.state().unwrap().undo.len(), 1, "{cmd}: one undo step");
    }
}

#[test]
fn arc_continue_from_last_line() {
    let mut s = Session::new();
    // Nothing to continue from: the command ends politely.
    s.cmdline("arc.continue").unwrap();
    assert!(s.running.is_none());
    assert!(s.execute("arc.continue", &json!({"end": [2, 1]})).is_err());
    s.cmdline("line 0,0 1,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("arc.continue 2,1").unwrap();
    let a = as_arc(&last(&s));
    assert!(near(a.center, 1.0, 1.0), "{a:?}");
    // JSON continues from that arc's end (2,1) heading +Y.
    s.execute("arc.continue", &json!({"end": [1, 2]})).unwrap();
    let b = as_arc(&last(&s));
    assert!(near(b.center, 1.0, 1.0), "{b:?}");
    assert!(s.execute("arc.continue", &json!({"end": [-5, 2]})).is_err(), "end on the tangent line");
    // Explicit start and direction.
    s.execute("arc.continue", &json!({"start": [0, 0], "direction": 0, "end": [1, 1]})).unwrap();
    assert!(near(as_arc(&last(&s)).center, 0.0, 1.0));
}

// ---------------- CIRCLE ----------------

#[test]
fn circle_variants_json_and_command_line() {
    let mut s = Session::new();
    s.execute("circle.cd", &json!({"center": [1, 1], "diameter": 4})).unwrap();
    assert_eq!(as_circle(&last(&s)).1, 2.0);
    s.execute("circle.2p", &json!({"p1": [0, 0], "p2": [6, 0]})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 3.0, 0.0) && (r - 3.0).abs() < 1e-9);
    s.execute("circle.3p", &json!({"p1": [1, 0], "p2": [0, 1], "p3": [-1, 0]})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 0.0, 0.0) && (r - 1.0).abs() < 1e-9);
    assert!(s.execute("circle.3p", &json!({"p1": [0, 0], "p2": [1, 0], "p3": [2, 0]})).is_err());
    assert!(s.execute("circle.cd", &json!({"center": [0, 0], "diameter": 0})).is_err());

    s.cmdline("circle.cd 5,5 3").unwrap();
    assert_eq!(as_circle(&last(&s)).1, 1.5);
    s.cmdline("circle.cd 5,5 5,9").unwrap();
    assert_eq!(as_circle(&last(&s)).1, 2.0);
    s.cmdline("circle.2p 0,0 0,4").unwrap();
    assert!(near(as_circle(&last(&s)).0, 0.0, 2.0));
    s.cmdline("circle.3p 0,0 2,2 4,0").unwrap();
    assert!(near(as_circle(&last(&s)).0, 2.0, 0.0));
    assert!(s.running.is_none());
}

fn line_h(s: &mut Session, pts: serde_json::Value) -> String {
    s.execute("line", &json!({ "points": pts })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

fn corner(s: &mut Session) -> (String, String) {
    (line_h(s, json!([[0, 0], [10, 0]])), line_h(s, json!([[0, 0], [0, 10]])))
}

#[test]
fn circle_ttr_json_and_command_line() {
    let mut s = Session::new();
    let (a, b) = corner(&mut s);
    s.execute("circle.ttr", &json!({"h1": a, "p1": [3, 0], "h2": b, "p2": [0, 3], "radius": 1})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 1.0, 1.0) && (r - 1.0).abs() < 1e-9);
    // Circle-circle and line-circle.
    let k = h(&s.execute("circle", &json!({"center": [20, 0], "radius": 2})).unwrap());
    let k2 = h(&s.execute("circle", &json!({"center": [26, 0], "radius": 2})).unwrap());
    s.execute("circle.ttr", &json!({"h1": k, "p1": [21, 1], "h2": k2, "p2": [25, 1], "radius": 2})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!((c.x - 23.0).abs() < 1e-9 && c.y > 0.0 && (r - 2.0).abs() < 1e-9, "{c:?}");
    assert!(s.execute("circle.ttr", &json!({"h1": k, "p1": [21, 1], "h2": k2, "p2": [25, 1], "radius": 0.1})).is_err());
    // Command line: picks, then the radius.
    s.cmdline("circle.ttr 5,0 0,5 2").unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 2.0, 2.0) && (r - 2.0).abs() < 1e-9, "{c:?} {r}");
    // CIRCLE's Ttr option.
    s.cmdline("circle ttr 6,0 0,6 0.5").unwrap();
    assert!(s.running.is_none());
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 0.5, 0.5) && (r - 0.5).abs() < 1e-9);
}

#[test]
fn circle_ttt_json_and_command_line() {
    let mut s = Session::new();
    let (a, b) = corner(&mut s);
    let hyp = s.execute("line", &json!({"points": [[4, 0], [0, 3]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("circle.ttt", &json!({"h1": a, "p1": [2, 0], "h2": b, "p2": [0, 1], "h3": hyp, "p3": [2, 1.5]})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 1.0, 1.0) && (r - 1.0).abs() < 1e-6, "{c:?} {r}");
    s.cmdline("circle.ttt 2,0 0,1 2,1.5").unwrap();
    assert!(s.running.is_none());
    let (c, _) = as_circle(&last(&s));
    assert!(near(c, 1.0, 1.0));
    // Three circles: the small circle in the middle of three touching circles.
    let mut s = Session::new();
    let r = 1.0;
    let cs: Vec<String> =
        [[0.0, 0.0], [2.0, 0.0], [1.0, 3f64.sqrt()]].iter().map(|c| h(&s.execute("circle", &json!({"center": c, "radius": r})).unwrap())).collect();
    let centroid = Vec2::new(1.0, 3f64.sqrt() / 3.0);
    let picks: Vec<Vec2> = [[0.0, 0.0], [2.0, 0.0], [1.0, 3f64.sqrt()]]
        .iter()
        .map(|c| Vec2::new(c[0], c[1]) + (centroid - Vec2::new(c[0], c[1])).normalized())
        .collect();
    s.execute(
        "circle.ttt",
        &json!({"h1": cs[0], "p1": [picks[0].x, picks[0].y], "h2": cs[1], "p2": [picks[1].x, picks[1].y], "h3": cs[2], "p3": [picks[2].x, picks[2].y]}),
    )
    .unwrap();
    let (c, rr) = as_circle(&last(&s));
    assert!(c.near(centroid, 1e-6) && (rr - (2.0 / 3f64.sqrt() - 1.0)).abs() < 1e-6, "{c:?} {rr}");
}

// ---------------- ELLIPSE ----------------

#[test]
fn ellipse_axis_and_arc() {
    let mut s = Session::new();
    s.execute("ellipse.axis", &json!({"p1": [0, 0], "p2": [4, 0], "distance": 1})).unwrap();
    match last(&s) {
        EntityKind::Ellipse(e) => {
            assert!(near(e.center.xy(), 2.0, 0.0) && (e.ratio - 0.5).abs() < 1e-9);
        }
        k => panic!("{k:?}"),
    }
    s.execute("ellipse.arc", &json!({"center": [0, 0], "major": [2, 0], "ratio": 0.5, "start": 0, "end": 90})).unwrap();
    match last(&s) {
        EntityKind::Ellipse(e) => assert!((e.end - std::f64::consts::FRAC_PI_2).abs() < 1e-9 && e.start.abs() < 1e-9),
        k => panic!("{k:?}"),
    }
    assert!(s.execute("ellipse.arc", &json!({"p1": [0, 0], "p2": [4, 0], "distance": 1})).is_err());
    s.cmdline("ellipse.axis 0,0 6,0 1").unwrap();
    assert!(matches!(last(&s), EntityKind::Ellipse(e) if (e.ratio - 1.0 / 3.0).abs() < 1e-9));
    s.cmdline("ellipse.arc 0,0 4,0 1 0 90").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Ellipse(e) if (e.end - e.start).abs() > 0.1 && (e.end - e.start).abs() < 6.0));
}

#[test]
fn ellipse_arc_typed_angles_are_true_angles() {
    let want = 2f64.atan2(1.0);
    // Horizontal and rotated major axis, ratio 0.5: typed 0 and 45 degrees match the JSON form.
    for line in ["ellipse.arc 0,0 10,0 2.5 0 45", "ellipse.arc 5,-5 5,5 2.5 0 45"] {
        let mut s = Session::new();
        s.cmdline(line).unwrap();
        assert!(matches!(last(&s), EntityKind::Ellipse(e) if e.start.abs() < 1e-9 && (e.end - want).abs() < 1e-9), "{line}");
    }
}

// ---------------- DIVIDE / MEASURE ----------------

#[test]
fn divide_and_measure_json() {
    let mut s = Session::new();
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    let r = s.execute("divide", &json!({"handle": l, "segments": 5})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 4);
    let pts: Vec<Vec2> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p.xy()) } else { None }).collect();
    assert!(near(pts[0], 2.0, 0.0) && near(pts[3], 8.0, 0.0));
    let r = s.execute("measure", &json!({"handle": l, "length": 3})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 3);
    // From the other end.
    s.execute("measure", &json!({"handle": l, "length": 4, "from": [10, 0]})).unwrap();
    assert!(matches!(last(&s), EntityKind::Point(p) if near(p.p.xy(), 2.0, 0.0)));
    // A circle divides into n points including the start.
    let c = h(&s.execute("circle", &json!({"center": [20, 0], "radius": 1})).unwrap());
    assert_eq!(s.execute("divide", &json!({"handle": c, "segments": 6})).unwrap()["handles"].as_array().unwrap().len(), 6);
    // Blocks, aligned with the object.
    let tick = s.execute("line", &json!({"points": [[0, -0.1], [0, 0.1]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("block", &json!({"name": "TICK", "base": [0, 0], "handles": [tick], "keep": "delete"})).unwrap();
    let arc = h(&s.execute("arc", &json!({"center": [0, 20], "radius": 2, "start": 0, "end": 180})).unwrap());
    let r = s.execute("divide", &json!({"handle": arc, "segments": 2, "block": "TICK"})).unwrap();
    let ins = s.doc().unwrap().entity(cadcraft_doc::Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()).unwrap().kind.clone();
    assert!(matches!(ins, EntityKind::Insert(i) if near(i.insert.xy(), 0.0, 22.0) && (i.rotation - std::f64::consts::PI).abs() < 1e-6));
    assert!(s.execute("divide", &json!({"handle": l, "segments": 3, "block": "NOPE"})).is_err());
    assert!(s.execute("divide", &json!({"handle": l, "segments": 1})).is_err());
    assert!(s.execute("measure", &json!({"handle": l, "length": 0})).is_err());
    assert!(s.execute("measure", &json!({"handle": l, "length": 1e-300})).is_err());
}

#[test]
fn divide_and_measure_command_line() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.cmdline("divide 5,0 4").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).iter().filter(|k| **k == "Point").count(), 3);
    s.cmdline("measure 9,0 2.5").unwrap();
    assert!(s.running.is_none());
    // Measured from the end nearest the pick.
    assert!(matches!(last(&s), EntityKind::Point(p) if near(p.p.xy(), 2.5, 0.0)));
    // Block option.
    let tick = s.execute("circle", &json!({"center": [50, 50], "radius": 0.1})).unwrap();
    s.execute("block", &json!({"name": "DOT", "base": [50, 50], "handles": [h(&tick)], "keep": "delete"})).unwrap();
    s.cmdline("divide 6,0 b DOT n 2").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Insert(i) if near(i.insert.xy(), 5.0, 0.0) && i.rotation == 0.0));
    // Invalid count re-prompts.
    s.cmdline("divide 6,0 1").unwrap();
    assert!(s.running.is_some());
    s.cancel();
}

// ---------------- REVCLOUD ----------------

#[test]
fn revcloud_json() {
    let mut s = Session::new();
    s.execute("revcloud", &json!({"p1": [0, 0], "p2": [4, 2], "arcLength": 1})).unwrap();
    match last(&s) {
        EntityKind::LwPolyline(p) => {
            assert!(p.closed);
            assert_eq!(p.vertices.len(), 12);
            assert!(p.vertices.iter().all(|v| v.bulge > 0.0));
        }
        k => panic!("{k:?}"),
    }
    s.execute("revcloud.polygonal", &json!({"points": [[0, 0], [0, 3], [3, 0]], "arcLength": 0.5})).unwrap();
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.vertices.len() > 10));
    s.execute("revcloud.freehand", &json!({"freehand": [[0, 0], [5, 0], [5, 5], [0, 5]], "arcLength": 2, "style": "calligraphy"})).unwrap();
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.vertices.len() == 10 && p.vertices[0].end_width > 0.0));
    let c = h(&s.execute("circle", &json!({"center": [10, 10], "radius": 1})).unwrap());
    let n = s.doc().unwrap().model.len();
    s.execute("revcloud", &json!({"handle": c, "arcLength": 0.5})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), n, "the circle is replaced");
    assert!(s.execute("revcloud", &json!({"points": [[0, 0], [1, 1], [2, 2]]})).is_err());
    assert!(s.execute("revcloud", &json!({"p1": [0, 0], "p2": [1e300, 1e300], "arcLength": 1e-300})).is_ok());
}

#[test]
fn revcloud_command_line() {
    let mut s = Session::new();
    s.cmdline("revcloud a 1").unwrap();
    s.cmdline("0,0 4,2").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.vertices.len() == 12));
    s.cmdline("revcloud.polygonal 0,0 4,0 4,3").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s), vec!["Polyline", "Polyline"]);
    s.cmdline("revcloud.freehand 0,0 3,0 3,3 0,3").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(kinds(&s).len(), 3);
    // Object option converts a closed polyline.
    s.execute("rectang", &json!({"p1": [10, 10], "p2": [12, 12]})).unwrap();
    s.cmdline("revcloud o 10,11").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).len(), 4);
}

// ---------------- WIPEOUT ----------------

#[test]
fn wipeout_json_and_command_line() {
    let mut s = Session::new();
    s.execute("wipeout", &json!({"points": [[0, 0], [2, 0], [2, 2]]})).unwrap();
    assert!(matches!(last(&s), EntityKind::Wipeout(w) if w.boundary.len() == 3));
    let r = h(&s.execute("rectang", &json!({"p1": [5, 5], "p2": [6, 6]})).unwrap());
    s.execute("wipeout", &json!({"handle": r, "erase": true})).unwrap();
    assert_eq!(kinds(&s), vec!["Wipeout", "Wipeout"]);
    s.execute("wipeout", &json!({"frames": "off"})).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("WIPEOUTFRAME", 1), 0);
    assert!(s.execute("wipeout", &json!({"points": [[0, 0], [1, 1]]})).is_err());
    assert!(s.execute("wipeout", &json!({"frames": "sideways"})).is_err());

    s.cmdline("wipeout 0,0 3,0 3,3").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).len(), 3);
    s.execute("rectang", &json!({"p1": [20, 20], "p2": [22, 22]})).unwrap();
    s.cmdline("wipeout p 20,21 y").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(kinds(&s), vec!["Wipeout"; 4]);
    s.cmdline("wipeout f on").unwrap();
    assert_eq!(s.doc().unwrap().header.i64("WIPEOUTFRAME", 0), 1);
}

// ---------------- XLINE ----------------

fn xline_dir(k: &EntityKind) -> (Vec2, Vec2) {
    match k {
        EntityKind::XLine(r) => (r.base.xy(), r.dir.xy()),
        k => panic!("{k:?}"),
    }
}

#[test]
fn xline_options_json() {
    let mut s = Session::new();
    s.execute("xline", &json!({"vertex": [0, 0], "start": [1, 0], "end": [0, 1]})).unwrap();
    let (_, d) = xline_dir(&last(&s));
    assert!(d.near(Vec2::new(1.0, 1.0).normalized(), 1e-9));
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("xline", &json!({"handle": l, "distance": 2, "side": [5, -1]})).unwrap();
    let (b, d) = xline_dir(&last(&s));
    assert!((b.y + 2.0).abs() < 1e-9 && d.near(Vec2::X, 1e-9));
    s.execute("xline", &json!({"handle": l, "through": [3, 7]})).unwrap();
    assert!(near(xline_dir(&last(&s)).0, 3.0, 7.0));
    s.execute("xline", &json!({"base": [1, 1], "ver": true})).unwrap();
    assert!(xline_dir(&last(&s)).1.near(Vec2::Y, 1e-9));
    s.execute("xline", &json!({"base": [1, 1], "angle": 30})).unwrap();
    s.execute("xline", &json!({"base": [1, 1], "through": [2, 2]})).unwrap();
    assert!(s.execute("xline", &json!({"base": [1, 1], "through": [1, 1]})).is_err());
}

#[test]
fn xline_options_command_line() {
    let mut s = Session::new();
    s.cmdline("xline b 0,0 1,0 0,1 -1,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(kinds(&s), vec!["Xline", "Xline"]);
    s.cmdline("xline a 45 3,3").unwrap();
    s.cmdline("").unwrap();
    assert!(xline_dir(&last(&s)).1.near(Vec2::new(1.0, 1.0).normalized(), 1e-9));
    s.execute("line", &json!({"points": [[20, 0], [30, 0]]})).unwrap();
    s.cmdline("xline o 2 25,0 25,5").unwrap();
    s.cmdline("").unwrap();
    let (b, _) = xline_dir(&last(&s));
    assert!((b.y - 2.0).abs() < 1e-9);
    s.cmdline("xline o t 25,0 0,9").unwrap();
    s.cmdline("").unwrap();
    assert!(near(xline_dir(&last(&s)).0, 0.0, 9.0));
    s.cmdline("xline a r 25,0 90 4,4").unwrap();
    s.cmdline("").unwrap();
    assert!(xline_dir(&last(&s)).1.near(Vec2::Y, 1e-9));
    s.cmdline("xline h 0,7").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

// ---------------- RECTANG ----------------

fn rect_pts(s: &Session) -> Vec<Vec2> {
    match last(s) {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| v.p).collect(),
        k => panic!("{k:?}"),
    }
}

#[test]
fn rectang_options_json() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 3]})).unwrap();
    assert!(near(rect_pts(&s)[2], 4.0, 3.0));
    s.execute("rectang", &json!({"p1": [0, 0], "dimensions": [3, 2], "p2": [-1, -1]})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, -3.0, -2.0) && near(b.max, 0.0, 0.0));
    s.execute("rectang", &json!({"p1": [0, 0], "area": 20, "length": 5})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.max, 5.0, 4.0));
    s.execute("rectang", &json!({"p1": [0, 0], "dimensions": [2, 1], "rotation": 90})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, -1.0, 0.0) && near(b.max, 0.0, 2.0), "{b:?}");
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 4], "fillet": 1, "width": 0.1, "thickness": 2})).unwrap();
    assert_eq!(rect_pts(&s).len(), 8);
    assert_eq!(s.doc().unwrap().model.last().unwrap().common.thickness, 2.0);
    assert!(s.execute("rectang", &json!({"p1": [0, 0], "area": -1, "length": 1})).is_err());
    assert!(s.execute("rectang", &json!({"p1": [0, 0], "p2": [0, 5]})).is_err());
}

#[test]
fn rectang_options_command_line() {
    let mut s = Session::new();
    s.cmdline("rectang 0,0 a 20 l 5").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(cadcraft_geom::Bounds2::from_points(rect_pts(&s)).max, 5.0, 4.0));
    s.cmdline("rectang 0,0 a 12 w 3").unwrap();
    assert!(near(cadcraft_geom::Bounds2::from_points(rect_pts(&s)).max, 4.0, 3.0));
    s.cmdline("rectang 0,0 d 3 2 -1,-1").unwrap();
    assert!(near(cadcraft_geom::Bounds2::from_points(rect_pts(&s)).min, -3.0, -2.0));
    s.cmdline("rectang 0,0 r 90 2,1").unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 0.0, 0.0) && near(b.max, 2.0, 1.0), "{b:?}");
    s.cmdline("rectang 0,0 r p 0,0 1,1 d 2 2 5,5").unwrap();
    let pts = rect_pts(&s);
    assert!(pts.iter().any(|p| near(*p, 0.0, 8f64.sqrt())), "{pts:?}");
    s.cmdline("rectang w 0.5 c 0.5 0.5 0,0 4,4").unwrap();
    match last(&s) {
        EntityKind::LwPolyline(p) => assert!(p.const_width == 0.5 && p.vertices.len() == 8),
        k => panic!("{k:?}"),
    }
    s.cmdline("rectang f 1 e 2 t 3 0,0 4,4").unwrap();
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => assert!(p.elevation == 2.0 && p.vertices.len() == 8),
        k => panic!("{k:?}"),
    }
}

// ---------------- 3DPOLY / MLINE / HELIX / SPLINE CV ----------------

#[test]
fn poly3d_json_and_command_line() {
    let mut s = Session::new();
    s.execute("3dpoly", &json!({"points": [[0, 0, 0], [1, 0, 2], [1, 1, 3]], "closed": true})).unwrap();
    assert!(matches!(last(&s), EntityKind::Polyline3d(p) if p.points.len() == 3 && p.points[2].z == 3.0 && p.closed));
    assert!(s.execute("3dpoly", &json!({"points": [[0, 0]]})).is_err());
    s.cmdline("3dpoly 0,0 1,0 1,1 u 2,2 c").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Polyline3d(p) if p.points.len() == 3 && p.closed));
    assert_eq!(s.state().unwrap().undo.len(), 2);
}

#[test]
fn mline_json_and_command_line() {
    let mut s = Session::new();
    let r = s.execute("mline", &json!({"points": [[0, 0], [10, 0], [10, 10]], "scale": 2, "justification": "zero"})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    let ys: Vec<f64> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.vertices[0].p.y) } else { None }).collect();
    assert!(ys.contains(&1.0) && ys.contains(&-1.0), "{ys:?}");
    assert!(s.execute("mline", &json!({"points": [[0, 0], [1, 0]], "justification": "middle"})).is_err());
    let mut s = Session::new();
    s.cmdline("mline j b s 3 0,0 10,0 10,5").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let ys: Vec<f64> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.vertices[0].p.y) } else { None }).collect();
    assert_eq!(ys.len(), 2);
    assert!(ys.contains(&0.0) && ys.contains(&3.0), "{ys:?}");
    s.cmdline("mline 0,20 5,20 5,25 c").unwrap();
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.closed));
}

#[test]
fn helix_json_and_command_line() {
    let mut s = Session::new();
    s.execute("helix", &json!({"center": [0, 0], "baseRadius": 2, "topRadius": 1, "height": 6, "turns": 2})).unwrap();
    match last(&s) {
        EntityKind::Polyline3d(p) => {
            assert!(near(p.points[0].xy(), 2.0, 0.0));
            let top = p.points.last().unwrap();
            assert!((top.z - 6.0).abs() < 1e-9 && near(top.xy(), 1.0, 0.0));
        }
        k => panic!("{k:?}"),
    }
    assert!(s.execute("helix", &json!({"center": [0, 0], "baseRadius": 1, "turns": 1e9})).is_err());
    s.cmdline("helix 0,0 1 2 t 4 w cw 5").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    match last(&s) {
        EntityKind::Polyline3d(p) => {
            let top = p.points.last().unwrap();
            assert!((top.z - 5.0).abs() < 1e-9 && near(top.xy(), 2.0, 0.0));
            assert!(p.points[1].y < 0.0, "clockwise twist");
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn spline_cv_json_and_command_line() {
    let mut s = Session::new();
    s.execute("spline.cv", &json!({"control": [[0, 0], [1, 2], [3, 2], [4, 0]], "degree": 2})).unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.degree == 2 && sp.fit.is_empty()));
    assert!(s.execute("spline.cv", &json!({"control": [[0, 0]]})).is_err());
    s.cmdline("spline.cv d 3 0,0 1,1 2,0 3,1").unwrap();
    s.cmdline("").unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.control.len() == 4 && sp.degree == 3));
    s.cmdline("spline.cv 0,0 1,1 2,0 c").unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.closed));
}

// ---------------- CENTER MARK / CENTER LINE ----------------

#[test]
fn centermark_and_centerline() {
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap());
    let r = s.execute("centermark", &json!({"handle": c})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    let e = (**s.doc().unwrap().model.last().unwrap()).clone();
    assert_eq!(e.common.linetype, "CENTER");
    assert!(s.doc().unwrap().linetype("CENTER").is_some());
    assert!(matches!(e.kind, EntityKind::Line(l) if (l.b.y - 2.12).abs() < 1e-9));
    let a = s.execute("line", &json!({"points": [[10, 0], [20, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    let b = s.execute("line", &json!({"points": [[20, 4], [10, 4]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("centerline", &json!({"h1": a, "h2": b})).unwrap();
    assert!(matches!(last(&s), EntityKind::Line(l) if (l.a.y - 2.0).abs() < 1e-9 && (l.a.x - 9.88).abs() < 1e-9));
    assert!(s.execute("centerline", &json!({"h1": a, "h2": c})).is_err());
    assert!(s.execute("centermark", &json!({"handle": a})).is_err());

    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    s.execute("arc", &json!({"center": [10, 0], "radius": 1, "start": 0, "end": 90})).unwrap();
    s.cmdline("centermark 2,0 11,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(kinds(&s).iter().filter(|k| **k == "Line").count(), 4);
    s.execute("line", &json!({"points": [[0, 10], [10, 10]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 12], [10, 12]]})).unwrap();
    s.cmdline("centerline 5,10 5,12").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Line(l) if (l.a.y - 11.0).abs() < 1e-9));
}

#[test]
fn new_draw_commands_survive_garbage_input() {
    let ids = [
        "arc.sce",
        "arc.sca",
        "arc.scl",
        "arc.sea",
        "arc.sed",
        "arc.ser",
        "arc.cse",
        "arc.csa",
        "arc.csl",
        "arc.continue",
        "circle.cd",
        "circle.2p",
        "circle.3p",
        "circle.ttr",
        "circle.ttt",
        "ellipse.axis",
        "ellipse.arc",
        "divide",
        "measure",
        "revcloud",
        "revcloud.polygonal",
        "revcloud.freehand",
        "wipeout",
        "3dpoly",
        "mline",
        "helix",
        "spline.cv",
        "centermark",
        "centerline",
        "xline",
        "rectang",
    ];
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    for id in ids {
        for p in [
            json!({"start": [0, 0], "center": [0, 0], "end": [0, 0], "angle": 1e308, "length": -1e308, "radius": 0, "direction": [0, 0]}),
            json!({"p1": [0, 0], "p2": [0, 0], "p3": [0, 0], "distance": -0.0, "diameter": 1e308, "radius": f64::MAX}),
            json!({"handle": "100", "segments": 1e18, "length": 1e-320, "block": ""}),
            json!({"points": [[0, 0], [1e308, 1e308], [-1e308, 1e308]], "arcLength": 1e-300, "scale": 1e308}),
            json!({"center": [0, 0], "baseRadius": 1e308, "turns": 499, "height": 1e308}),
            json!({"control": [[0, 0], [0, 0]], "degree": 1e9}),
            json!({"p1": [0, 0], "dimensions": [1e308, -1e308], "rotation": 1e308, "area": 1e308, "length": 1e-308}),
            json!({"h1": "100", "p1": [0, 0], "h2": "100", "p2": [0, 0], "h3": "100", "p3": [0, 0], "radius": 1}),
            json!({"vertex": [0, 0], "start": [0, 0], "end": [0, 0]}),
        ] {
            let _ = s.execute(id, &p);
        }
        // Interactive: garbage text and points.
        s.start(id).unwrap();
        for t in ["", "x", "1e999", "-5", "0,0", "0,0", "nan", "b", "a", "1", "2,2", ""] {
            if s.running.is_none() {
                break;
            }
            let _ = s.cmdline(t);
        }
        let _ = s.preview(Vec2::new(1e300, -1e300));
        s.cancel();
        let _ = s.input(Input::Cancel);
    }
}

#[test]
fn arc_command_options() {
    for (cmd, center) in [
        ("arc 1,0 0,1 -1,0", (0.0, 0.0)),
        ("arc c 0,0 1,0 0,1", (0.0, 0.0)),
        ("arc c 0,0 1,0 a 90", (0.0, 0.0)),
        ("arc c 0,0 1,0 l 1.41421356237", (0.0, 0.0)),
        ("arc 1,0 c 0,0 0,1", (0.0, 0.0)),
        ("arc 1,0 c 0,0 a 90", (0.0, 0.0)),
        ("arc 1,0 e 0,1 0,0", (0.0, 0.0)),
        ("arc 1,0 e 0,1 a 90", (0.0, 0.0)),
        ("arc 1,0 e 0,1 d 90", (0.0, 0.0)),
        ("arc 1,0 e 0,1 r 1", (0.0, 0.0)),
    ] {
        let mut s = Session::new();
        s.cmdline(cmd).unwrap();
        assert!(s.running.is_none(), "{cmd}: {}", s.prompt_text());
        let a = as_arc(&last(&s));
        assert!(near(a.center, center.0, center.1) && (a.radius - 1.0).abs() < 1e-6, "{cmd}: {a:?}");
    }
    // Enter at the first prompt continues from the last line.
    let mut s = Session::new();
    s.cmdline("line 0,0 1,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("arc").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("end point"), "{}", s.prompt_text());
    s.cmdline("2,1").unwrap();
    assert!(near(as_arc(&last(&s)).center, 1.0, 1.0));
    assert!(s.prompt_text() == "Command:");
}
