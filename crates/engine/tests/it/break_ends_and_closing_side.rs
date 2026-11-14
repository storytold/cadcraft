//! BREAKATPOINT and BREAK where the result is easy to get wrong, as AutoCAD 2024 runs them
//! (accoreconsole, `entget` of every object after the command, on these drawings):
//!
//! - BREAKATPOINT at either end of a line, or at the end vertex of an open polyline, leaves the
//!   object whole;
//! - at an open polyline's middle vertex it splits it there, the original keeping the first part;
//! - BREAK of a closed polyline at two points of one side, either way round and on the side that
//!   closes it too, opens it with its own handle;
//! - a circle trimmed or broken becomes an arc with its own handle.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

/// The model space objects in order: handle and kind.
fn objects(s: &Session) -> Vec<(Handle, EntityKind)> {
    s.doc().unwrap().model.iter().map(|e| (e.handle, e.kind.clone())).collect()
}

/// A polyline's closed flag and vertices (rounded to 1e-9).
fn pts(k: &EntityKind) -> (bool, Vec<(f64, f64)>) {
    let EntityKind::LwPolyline(p) = k else { panic!("not a polyline: {k:?}") };
    let r = |v: f64| (v * 1e9).round() / 1e9;
    (p.closed, p.vertices.iter().map(|v| (r(v.p.x), r(v.p.y))).collect())
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    handle(&s.execute("line", &json!({ "points": [a, b] })).unwrap())
}

fn pline(s: &mut Session, vertices: Value, closed: bool) -> Handle {
    handle(&s.execute("pline", &json!({ "vertices": vertices, "closed": closed })).unwrap())
}

fn run(s: &mut Session, script: &str) {
    s.script(script).unwrap();
    assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
}

#[test]
fn breakatpoint_at_an_end_leaves_the_object_whole() {
    for at in ["100,0", "0,0"] {
        let mut s = Session::new();
        let l = line(&mut s, [0.0, 0.0], [100.0, 0.0]);
        run(&mut s, &format!("BREAKATPOINT {at} {at}\n"));
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "{at}: {objs:?}");
        let EntityKind::Line(k) = &objs[0].1 else { panic!("{objs:?}") };
        assert!(objs[0].0 == l && (k.a.x, k.a.y, k.b.x, k.b.y) == (0.0, 0.0, 100.0, 0.0), "{at}: {objs:?}");
    }
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [50, 0], [50, 30]]), false);
    run(&mut s, "BREAKATPOINT 50,30 50,30\n");
    let objs = objects(&s);
    assert_eq!(objs.len(), 1, "{objs:?}");
    assert_eq!((objs[0].0, pts(&objs[0].1)), (p, (false, vec![(0.0, 0.0), (50.0, 0.0), (50.0, 30.0)])));
}

#[test]
fn breakatpoint_at_a_middle_vertex_splits_there_and_the_original_keeps_the_first_part() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [50, 0], [50, 30]]), false);
    run(&mut s, "BREAKATPOINT 50,0 50,0\n");
    let objs = objects(&s);
    assert_eq!(objs.len(), 2, "{objs:?}");
    let first = objs.iter().find(|o| o.0 == p).expect("the original stays");
    let other = objs.iter().find(|o| o.0 != p).unwrap();
    assert_eq!(pts(&first.1), (false, vec![(0.0, 0.0), (50.0, 0.0)]));
    assert_eq!(pts(&other.1), (false, vec![(50.0, 0.0), (50.0, 30.0)]));
    // One undo brings back the polyline as it was, under its handle, and nothing else.
    s.undo().unwrap();
    let objs = objects(&s);
    assert_eq!(objs.len(), 1, "{objs:?}");
    assert_eq!((objs[0].0, pts(&objs[0].1)), (p, (false, vec![(0.0, 0.0), (50.0, 0.0), (50.0, 30.0)])));
}

#[test]
fn break_at_two_points_of_one_side_opens_a_closed_polyline_with_its_handle() {
    let bottom = vec![(30.0, 0.0), (100.0, 0.0), (100.0, 10.0), (0.0, 10.0), (0.0, 0.0), (20.0, 0.0)];
    // The side from (0, 10) back to the start vertex closes the rectangle.
    let closing = vec![(0.0, 3.0), (0.0, 0.0), (100.0, 0.0), (100.0, 10.0), (0.0, 10.0), (0.0, 8.0)];
    for (script, want) in [("BREAK 20,0 30,0\n", &bottom), ("BREAK 30,0 20,0\n", &bottom), ("BREAK 0,8 0,3\n", &closing)] {
        let mut s = Session::new();
        let r = pline(&mut s, json!([[0, 0], [100, 0], [100, 10], [0, 10]]), true);
        run(&mut s, script);
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "{script}: {objs:?}");
        assert_eq!((objs[0].0, pts(&objs[0].1)), (r, (false, want.clone())), "{script}");
        // Undo closes it again.
        s.undo().unwrap();
        let objs = objects(&s);
        let rect = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 10.0), (0.0, 10.0)];
        assert_eq!(objs.iter().map(|o| (o.0, pts(&o.1))).collect::<Vec<_>>(), vec![(r, (true, rect))], "{script}");
    }
}

#[test]
fn a_circle_trimmed_or_broken_becomes_an_arc_with_its_handle() {
    // Trimmed between the lines x = -5 and x = 5, picked at its top: the arc from 120 to 60
    // degrees, round the bottom.
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap());
    let l = line(&mut s, [-5.0, -20.0], [-5.0, 20.0]);
    let m = line(&mut s, [5.0, -20.0], [5.0, 20.0]);
    s.execute("trim", &json!({ "handle": c.hex(), "pick": [0, 10], "edges": [l.hex(), m.hex()] })).unwrap();
    // The arc's handle, and whether it is AutoCAD's: radius 10 round (0, 0), from `start` to
    // `end` degrees (to 1e-9 radians, 0 and 360 alike).
    let arc_is = |s: &Session, start: f64, end: f64| {
        let objs = objects(s);
        let (h, k) = objs.iter().find(|o| matches!(o.1, EntityKind::Arc(_))).cloned().expect("an arc");
        let EntityKind::Arc(a) = k else { panic!("not an arc") };
        let same = |got: f64, want: f64| {
            let d = (got - want.to_radians()).rem_euclid(std::f64::consts::TAU);
            d < 1e-9 || std::f64::consts::TAU - d < 1e-9
        };
        let ok = (a.radius - 10.0).abs() < 1e-9 && a.center.x.abs() < 1e-9 && a.center.y.abs() < 1e-9 && same(a.start, start) && same(a.end, end);
        (h, ok)
    };
    assert_eq!(arc_is(&s, 120.0, 60.0), (c, true));
    // Broken from (10, 0) to (0, 10): the arc from 90 degrees round to 0.
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap());
    run(&mut s, "BREAK 10,0 0,10\n");
    assert_eq!(objects(&s).len(), 1);
    assert_eq!(arc_is(&s, 90.0, 0.0), (c, true));
    // Undo gives the circle back under its handle.
    s.undo().unwrap();
    let objs = objects(&s);
    assert!(objs.len() == 1 && objs[0].0 == c && matches!(objs[0].1, EntityKind::Circle(_)), "{objs:?}");
}
