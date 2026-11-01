//! A point held by GCFIX stays where it is when its grip is dragged (#1134): the Fix locks the
//! position the point had, not wherever the edit left it.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

fn grip_drag(s: &mut Session, h: Handle, from: Vec2, to: &str) {
    s.set_selection(vec![h]);
    let i = s.grips_of(h).unwrap().iter().position(|g| g.near(from, 1e-9)).unwrap();
    s.start_grip_edit(&[(h, i)]).unwrap();
    s.cmdline(to).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
}

#[test]
fn dragging_the_grip_of_a_fixed_end_leaves_it_in_place() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("gcfix", &json!({ "h1": a.hex(), "p1": "start" })).unwrap();

    grip_drag(&mut s, a, Vec2::new(0.0, 0.0), "3,4");
    let (start, end) = ends(&s, a);
    assert!(start.near(Vec2::new(0.0, 0.0), 1e-6), "the fixed start moved to {start:?}");
    assert!(end.near(Vec2::new(10.0, 0.0), 1e-6), "{end:?}");

    // The free end still follows its grip.
    grip_drag(&mut s, a, Vec2::new(10.0, 0.0), "10,5");
    let (start, end) = ends(&s, a);
    assert!(start.near(Vec2::new(0.0, 0.0), 1e-6), "{start:?}");
    assert!(end.near(Vec2::new(10.0, 5.0), 1e-6), "{end:?}");
}

#[test]
fn a_fixed_midpoint_stays_in_place_when_an_end_grip_is_dragged() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("gcfix", &json!({ "h1": a.hex(), "p1": "mid" })).unwrap();

    grip_drag(&mut s, a, Vec2::new(10.0, 0.0), "10,5");
    let (start, end) = ends(&s, a);
    assert!(start.mid(end).near(Vec2::new(5.0, 0.0), 1e-6), "the fixed midpoint moved to {:?} ({start:?}, {end:?})", start.mid(end));
    // The dragged end lands where it was put; the line turns about its fixed midpoint.
    assert!(end.near(Vec2::new(10.0, 5.0), 1e-6), "{end:?}");
    assert!(start.near(Vec2::new(0.0, -5.0), 1e-6), "{start:?}");
}

#[test]
fn moving_an_arc_with_a_fixed_end_leaves_it_whole_and_in_place() {
    let mut s = Session::new();
    let r = s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    let a = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.execute("gcfix", &json!({ "h1": a.hex(), "p1": "end" })).unwrap();

    s.execute("move", &json!({ "handles": [a.hex()], "delta": [1, 1] })).unwrap();
    match &s.doc().unwrap().entity(a).unwrap().kind {
        EntityKind::Arc(c) => {
            assert!(c.center.xy().near(Vec2::new(0.0, 0.0), 1e-6), "{:?}", c.center);
            assert!((c.radius - 5.0).abs() < 1e-6, "radius {}", c.radius);
            assert!((c.end - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "end at {}", c.end);
        }
        k => panic!("not an arc: {k:?}"),
    }
}

#[test]
fn dragging_an_end_of_one_of_two_parallel_lines_leaves_its_other_end_in_place() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 2.0], [10.0, 2.0]);
    s.execute("gcparallel", &json!({ "h1": a.hex(), "h2": b.hex() })).unwrap();

    grip_drag(&mut s, a, Vec2::new(10.0, 0.0), "10,5");
    let (start, end) = ends(&s, a);
    assert!(end.near(Vec2::new(10.0, 5.0), 1e-6), "{end:?}");
    assert!(start.near(Vec2::new(0.0, 0.0), 1e-3), "the end that was not dragged moved to {start:?}");
    // The other line took up the change.
    let (b0, b1) = ends(&s, b);
    assert!((b1 - b0).cross(end - start).abs() < 1e-6, "{b0:?} {b1:?}");
}
