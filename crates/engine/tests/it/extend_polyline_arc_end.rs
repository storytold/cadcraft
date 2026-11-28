//! EXTEND on an open polyline whose end segment is an arc: the arc runs on round its own circle
//! to the boundary, as EXTEND does with an ARC, instead of the end vertex sliding along the chord
//! with the old bulge (which changed the arc's radius and centre, or missed the boundary). The
//! polyline keeps its width, elevation and linetype generation.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Entity, EntityKind, Handle, LwPolyline};
use cadcraft_engine::geom::{PolyVertex, Vec2};
use serde_json::json;

const TOL: f64 = 1e-9;

fn add(s: &mut Session, kind: EntityKind) -> String {
    let space = s.space();
    let d = s.doc_mut().unwrap();
    let handle = d.new_handle();
    d.space_mut(&space).unwrap().push(Entity { handle, common: Common::default(), kind });
    handle.hex()
}

fn v(x: f64, y: f64, bulge: f64) -> PolyVertex {
    PolyVertex { p: Vec2::new(x, y), bulge, ..Default::default() }
}

fn pline(vertices: Vec<PolyVertex>) -> LwPolyline {
    LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false }
}

fn edge(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    r["handles"][0].as_str().unwrap().to_string()
}

fn polyline(s: &Session, h: &str) -> LwPolyline {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::LwPolyline(p) => p.clone(),
        k => panic!("not a polyline: {k:?}"),
    }
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    (a.x - x).abs() < 1e-6 && (a.y - y).abs() < 1e-6
}

/// tan(22.5°): the bulge of a quarter circle.
fn quarter() -> f64 {
    (std::f64::consts::PI / 8.0).tan()
}

/// The bulge of an arc turning through `deg` degrees (positive counterclockwise).
fn bulge(deg: f64) -> f64 {
    (deg.to_radians() / 4.0).tan()
}

#[test]
fn a_counterclockwise_arc_at_the_end_runs_on_round_its_circle() {
    let mut s = Session::new();
    let e = edge(&mut s, [-50.0, -5.0], [50.0, -5.0]);
    // A line in from (20,0), then a quarter circle about the origin from (10,0) to (0,10).
    let mut pl = pline(vec![v(20.0, 0.0, 0.0), v(10.0, 0.0, quarter()), v(0.0, 10.0, 0.0)]);
    pl.const_width = 0.5;
    pl.elevation = 3.0;
    pl.plinegen = true;
    let h = add(&mut s, EntityKind::LwPolyline(pl));
    s.execute("extend", &json!({ "handle": h, "pick": [0.5, 9.9], "edges": [e] })).unwrap();
    let p = polyline(&s, &h);
    assert_eq!(p.vertices.len(), 3);
    // Round the circle (radius 10) through (-10,0) to y = -5: a 210° turn in all.
    let end = p.vertices[2].p;
    assert!(near(end, -(75.0f64.sqrt()), -5.0), "end {end:?}");
    assert!((p.vertices[1].bulge - bulge(210.0)).abs() < TOL, "bulge {}", p.vertices[1].bulge);
    assert!(near(p.vertices[0].p, 20.0, 0.0) && near(p.vertices[1].p, 10.0, 0.0));
    assert_eq!((p.const_width, p.elevation, p.plinegen), (0.5, 3.0, true));
}

#[test]
fn an_arc_at_the_start_runs_on_backwards_round_its_circle() {
    let mut s = Session::new();
    let e = edge(&mut s, [-50.0, -5.0], [50.0, -5.0]);
    // The same path drawn the other way: clockwise from (0,10) to (10,0), then out to (20,0).
    let h = add(&mut s, EntityKind::LwPolyline(pline(vec![v(0.0, 10.0, -quarter()), v(10.0, 0.0, 0.0), v(20.0, 0.0, 0.0)])));
    s.execute("extend", &json!({ "handle": h, "pick": [0.5, 9.9], "edges": [e] })).unwrap();
    let p = polyline(&s, &h);
    assert!(near(p.vertices[0].p, -(75.0f64.sqrt()), -5.0), "start {:?}", p.vertices[0].p);
    assert!((p.vertices[0].bulge + bulge(210.0)).abs() < TOL, "bulge {}", p.vertices[0].bulge);
    assert!(near(p.vertices[1].p, 10.0, 0.0) && near(p.vertices[2].p, 20.0, 0.0));
}

#[test]
fn a_clockwise_arc_at_the_end_runs_on_clockwise() {
    let mut s = Session::new();
    let e = edge(&mut s, [-50.0, 5.0], [50.0, 5.0]);
    // Clockwise about the origin from (10,0) to (0,-10); on round through (-10,0) to y = 5.
    let h = add(&mut s, EntityKind::LwPolyline(pline(vec![v(20.0, 0.0, 0.0), v(10.0, 0.0, -quarter()), v(0.0, -10.0, 0.0)])));
    s.execute("extend", &json!({ "handle": h, "pick": [0.5, -9.9], "edges": [e] })).unwrap();
    let p = polyline(&s, &h);
    assert!(near(p.vertices[2].p, -(75.0f64.sqrt()), 5.0), "end {:?}", p.vertices[2].p);
    assert!((p.vertices[1].bulge + bulge(210.0)).abs() < TOL, "bulge {}", p.vertices[1].bulge);
}

#[test]
fn the_nearest_crossing_ahead_on_the_circle_wins() {
    let mut s = Session::new();
    // A boundary crossing the circle twice ahead of the end: x = -6 at about 127° and 233°.
    let e = edge(&mut s, [-6.0, -50.0], [-6.0, 50.0]);
    let h = add(&mut s, EntityKind::LwPolyline(pline(vec![v(20.0, 0.0, 0.0), v(10.0, 0.0, quarter()), v(0.0, 10.0, 0.0)])));
    s.execute("extend", &json!({ "handle": h, "pick": [0.5, 9.9], "edges": [e] })).unwrap();
    let p = polyline(&s, &h);
    assert!(near(p.vertices[2].p, -6.0, 8.0), "end {:?}", p.vertices[2].p);
    let turn = 8.0f64.atan2(-6.0).to_degrees();
    assert!((p.vertices[1].bulge - bulge(turn)).abs() < TOL, "bulge {}", p.vertices[1].bulge);
}

#[test]
fn an_arc_end_whose_circle_misses_every_edge_is_left_alone() {
    let mut s = Session::new();
    // The chord (10,0)→(0,10) runs on to this edge, but the circle never reaches it.
    let e = edge(&mut s, [-50.0, 30.0], [50.0, 30.0]);
    let before = pline(vec![v(20.0, 0.0, 0.0), v(10.0, 0.0, quarter()), v(0.0, 10.0, 0.0)]);
    let h = add(&mut s, EntityKind::LwPolyline(before.clone()));
    assert!(s.execute("extend", &json!({ "handle": h, "pick": [0.5, 9.9], "edges": [e] })).is_err());
    assert_eq!(polyline(&s, &h), before);
}

#[test]
fn a_straight_end_keeps_the_polyline_width_and_elevation() {
    let mut s = Session::new();
    let e = edge(&mut s, [50.0, -50.0], [50.0, 50.0]);
    let mut pl = pline(vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)]);
    pl.const_width = 0.5;
    pl.elevation = 3.0;
    let h = add(&mut s, EntityKind::LwPolyline(pl));
    s.execute("extend", &json!({ "handle": h, "pick": [9.0, 0.0], "edges": [e] })).unwrap();
    let p = polyline(&s, &h);
    assert!(near(p.vertices[1].p, 50.0, 0.0), "end {:?}", p.vertices[1].p);
    assert_eq!((p.const_width, p.elevation), (0.5, 3.0));
}
