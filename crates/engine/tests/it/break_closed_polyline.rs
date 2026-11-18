//! BREAK on a closed polyline removes the stretch from the first to the second point along the
//! vertex direction and leaves one open polyline (#1173).

use cadcraft_doc::{EntityKind, LwPolyline};
use cadcraft_engine::Session;
use serde_json::{Value, json};

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

fn pts(p: &LwPolyline) -> Vec<(f64, f64)> {
    p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()
}

fn closed_rect(s: &mut Session) -> Value {
    s.execute("pline", &json!({ "points": [[0, 0], [100, 0], [100, 60], [0, 60]], "closed": true })).unwrap()
}

#[test]
fn break_opens_a_closed_polyline_at_the_command_line() {
    let mut s = Session::new();
    s.cmdline("PLINE 0,0 100,0 100,60 0,60 C").unwrap();
    s.cmdline("BREAK 50,0 100,30").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let ps = polys(&s);
    assert_eq!(ps.len(), 1);
    assert!(!ps[0].closed);
    assert_eq!(pts(&ps[0]), vec![(100.0, 30.0), (100.0, 60.0), (0.0, 60.0), (0.0, 0.0), (50.0, 0.0)]);
}

#[test]
fn break_json_on_a_closed_polyline_follows_the_order_of_the_points() {
    let mut s = Session::new();
    let r = closed_rect(&mut s);
    // Second point before the first along the vertices: the long way round goes, the short
    // stretch between them stays.
    s.execute("break", &json!({ "handle": r["handle"], "p1": [100, 30], "p2": [50, 0] })).unwrap();
    let ps = polys(&s);
    assert_eq!(ps.len(), 1);
    assert!(!ps[0].closed);
    assert_eq!(pts(&ps[0]), vec![(50.0, 0.0), (100.0, 0.0), (100.0, 30.0)]);
}
