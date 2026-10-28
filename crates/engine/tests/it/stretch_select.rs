//! STRETCH with polygon and fence selection: a crossing or window polygon marks the area whose
//! vertices move; a fence marks none, so what it crosses moves whole.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    s.execute("line", &json!({ "points": [a, b] })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

fn ends(s: &Session, h: &str) -> (Vec2, Vec2) {
    let d = s.doc().unwrap();
    let e = d.entity(Handle::parse_hex(h).unwrap()).unwrap();
    match &e.kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        _ => panic!("not a line"),
    }
}

fn near(p: Vec2, x: f64, y: f64) -> bool {
    (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9
}

/// Two lines from the origin side into a triangle around (10, 0) and (10, 6); a third outside it.
fn scene(s: &mut Session) -> (String, String, String) {
    (line(s, [0.0, 0.0], [10.0, 0.0]), line(s, [0.0, 6.0], [10.0, 6.0]), line(s, [0.0, 20.0], [10.0, 20.0]))
}

#[test]
fn crossing_polygon_moves_the_vertices_inside_it() {
    let mut s = Session::new();
    let (a, b, c) = scene(&mut s);
    // A triangle holding both right ends. This line crosses it: (20, 3) is inside, (20, 7) is
    // outside the triangle but inside its bounding box.
    let half = line(&mut s, [20.0, 3.0], [20.0, 7.0]);
    s.script("STRETCH\nCP\n8,-2\n30,3\n8,8\n\n\n0,0\n5,0\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (a0, a1) = ends(&s, &a);
    assert!(near(a0, 0.0, 0.0) && near(a1, 15.0, 0.0), "{a0:?} {a1:?}");
    let (b0, b1) = ends(&s, &b);
    assert!(near(b0, 0.0, 6.0) && near(b1, 15.0, 6.0), "{b0:?} {b1:?}");
    assert!(near(ends(&s, &c).1, 10.0, 20.0));
    let (h0, h1) = ends(&s, &half);
    assert!(near(h0, 25.0, 3.0) && near(h1, 20.0, 7.0), "only the end inside the polygon moves: {h0:?} {h1:?}");
}

#[test]
fn window_polygon_takes_only_what_is_wholly_inside() {
    let mut s = Session::new();
    let (a, _, _) = scene(&mut s);
    let small = line(&mut s, [11.0, 1.0], [12.0, 2.0]);
    s.script("STRETCH\nWP\n8,-2\n30,3\n8,8\n\n\n0,0\n5,0\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(ends(&s, &a).1, 10.0, 0.0), "crosses the polygon: a window polygon leaves it");
    let (p, q) = ends(&s, &small);
    assert!(near(p, 16.0, 1.0) && near(q, 17.0, 2.0), "{p:?} {q:?}");
}

#[test]
fn fence_moves_what_it_crosses_whole() {
    let mut s = Session::new();
    let (a, b, c) = scene(&mut s);
    s.script("STRETCH\nF\n5,-1\n5,7\n\n\n0,0\n0,3\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (a0, a1) = ends(&s, &a);
    assert!(near(a0, 0.0, 3.0) && near(a1, 10.0, 3.0), "{a0:?} {a1:?}");
    assert!(near(ends(&s, &b).0, 0.0, 9.0));
    assert!(near(ends(&s, &c).0, 0.0, 20.0));
}

#[test]
fn undo_takes_back_a_polygon_point_and_too_few_points_select_nothing() {
    let mut s = Session::new();
    let (a, _, _) = scene(&mut s);
    // The stray point (100,100) is undone before the polygon closes.
    s.script("STRETCH\nCP\n8,-2\n100,100\nU\n30,3\n8,8\n\n\n0,0\n5,0\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(ends(&s, &a).1, 15.0, 0.0));
    // Two points make no polygon: nothing is selected and Enter ends the command.
    s.script("STRETCH\nCP\n8,-2\n30,3\n\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(ends(&s, &a).1, 15.0, 0.0));
}

#[test]
fn json_polygon_and_fence() {
    let mut s = Session::new();
    let (a, b, c) = scene(&mut s);
    let r = s.execute("stretch", &json!({ "polygon": [[8, -2], [30, 3], [8, 8]], "delta": [5, 0] })).unwrap();
    assert_eq!(r["stretched"], 2);
    assert!(near(ends(&s, &a).1, 15.0, 0.0) && near(ends(&s, &a).0, 0.0, 0.0));
    assert!(near(ends(&s, &b).1, 15.0, 6.0));
    let r = s.execute("stretch", &json!({ "fence": [[5, 19], [5, 21]], "delta": [0, 1] })).unwrap();
    assert_eq!(r["stretched"], 1);
    let (c0, c1) = ends(&s, &c);
    assert!(near(c0, 0.0, 21.0) && near(c1, 10.0, 21.0));
    // A locked layer is left alone.
    s.execute("layer.set", &json!({ "name": "0", "locked": true })).unwrap();
    assert_eq!(s.execute("stretch", &json!({ "polygon": [[8, -2], [30, 3], [8, 8]], "delta": [5, 0] })).unwrap()["stretched"], 0);
    assert_eq!(s.execute("stretch", &json!({ "fence": [[5, 19], [5, 23]], "delta": [0, 1] })).unwrap()["stretched"], 0);
}

#[test]
fn hostile_polygon_and_fence_never_panic() {
    let mut s = Session::new();
    scene(&mut s);
    let huge: Vec<[f64; 2]> = (0..20_001).map(|i| [f64::from(i), 0.0]).collect();
    for p in [
        json!({ "polygon": [], "delta": [1, 0] }),
        json!({ "polygon": [[0, 0], [1, 1]], "delta": [1, 0] }),
        json!({ "polygon": [[0, 0], [1e308, 1e308], [-1e308, 1e308]], "delta": [1e308, 1e308] }),
        json!({ "polygon": [[0, 0], [0, 0], [0, 0]], "delta": [1, 0] }),
        json!({ "polygon": huge, "delta": [1, 0] }),
        json!({ "fence": [[0, 0]], "delta": [1, 0] }),
        json!({ "fence": huge, "delta": [1, 0] }),
        json!({ "fence": "x", "polygon": 3, "delta": [1, 0] }),
        json!({ "delta": [1, 0] }),
    ] {
        let _ = s.execute("stretch", &p);
    }
    assert!(s.execute("stretch", &json!({ "polygon": [[0, 0], [1, 1]], "delta": [1, 0] })).is_err());
    assert!(s.execute("stretch", &json!({ "fence": [[0, 0]], "delta": [1, 0] })).is_err());
    // Typed: modes started and abandoned, keywords inside a mode.
    for script in ["STRETCH\nCP\n\n\n", "STRETCH\nF\n1,1\n\n\n", "STRETCH\nWP\nU\nU\n1,1\nU\nU\n\n\n", "STRETCH\nCP\n1,1\nF\n0,-1\n0,1\n\n\n"] {
        let _ = s.script(script);
        s.cancel();
    }
}

#[test]
fn polygon_moves_only_the_multiline_vertices_inside_it() {
    let mut s = Session::new();
    s.execute("mline", &json!({ "points": [[0, 0], [3000, 0], [3000, 2000]], "scale": 200 })).unwrap();
    let r = s.execute("stretch", &json!({ "polygon": [[2500, 1500], [3500, 1500], [3500, 2500], [2500, 2500]], "delta": [610, 0] })).unwrap();
    assert_eq!(r["stretched"], 1);
    let pts = |s: &Session| -> Vec<(f64, f64)> {
        s.doc()
            .unwrap()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLine(m) => Some(m.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(pts(&s), vec![(0.0, 0.0), (3000.0, 0.0), (3610.0, 2000.0)]);
    // A window over the first vertex moves that vertex only, not the whole multiline.
    s.execute("stretch", &json!({ "window": [[-100, -100], [100, 100]], "delta": [610, 0] })).unwrap();
    assert_eq!(pts(&s), vec![(610.0, 0.0), (3000.0, 0.0), (3610.0, 2000.0)]);
}

#[test]
fn small_area_around_a_multiline_vertex_moves_it_at_a_large_scale() {
    let mline_points = |s: &Session| -> Vec<(f64, f64)> {
        s.doc()
            .unwrap()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLine(m) => Some(m.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()),
                _ => None,
            })
            .unwrap()
    };
    let mut s = Session::new();
    s.execute("mline", &json!({ "points": [[0, 0], [3000, 0], [3000, 2000]], "scale": 200 })).unwrap();
    // The element lines run 100 away from the middle vertex, so this area touches none of them.
    let r = s.execute("stretch", &json!({ "polygon": [[2995, -5], [3005, -5], [3005, 5], [2995, 5]], "delta": [0, 610] })).unwrap();
    assert_eq!(r["stretched"], 1);
    assert_eq!(mline_points(&s), vec![(0.0, 0.0), (3000.0, 610.0), (3000.0, 2000.0)]);
    s.execute("stretch", &json!({ "window": [[2995, 605], [3005, 615]], "delta": [0, -610] })).unwrap();
    assert_eq!(mline_points(&s), vec![(0.0, 0.0), (3000.0, 0.0), (3000.0, 2000.0)]);
}
