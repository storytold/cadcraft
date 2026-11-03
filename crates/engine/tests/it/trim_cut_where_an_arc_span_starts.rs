//! TRIM on a polyline cut through the vertex where an arc span starts: the cut is at that vertex
//! only. Where the crossing came out a hair before the arc's start angle it was also put at the
//! arc's far end, round the circle, and TRIM took out the arc alone.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn handle(v: serde_json::Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_owned()
}

#[test]
fn the_cut_is_at_the_arc_spans_start_vertex_only() {
    // (0, 0) to (10, 0), a semicircle round (15, 5) to (10, 10), then on to (0, 10). A line
    // through (10, 0) running up to the left crosses the top span at x = 10 + 10 cot(angle);
    // picked on the arc, the polyline goes from (10, 0) to there.
    for k in 0..172 {
        let th = (91.0 + f64::from(k) * 0.25).to_radians();
        let d = [th.cos() * 30.0, th.sin() * 30.0];
        let mut s = Session::new();
        let p = handle(s.execute("pline", &json!({ "vertices": [[0, 0], { "p": [10, 0], "bulge": 1 }, [10, 10], [0, 10]] })).unwrap());
        let e = handle(s.execute("line", &json!({ "points": [[10.0 - d[0], -d[1]], [10.0 + d[0], d[1]]] })).unwrap());
        s.execute("trim", &json!({ "handle": p, "pick": [15, 5], "edges": [e] })).unwrap();
        let pieces: Vec<Vec<(f64, f64)>> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::LwPolyline(l) => Some(l.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()),
                _ => None,
            })
            .collect();
        let x = 10.0 + 10.0 / th.tan();
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        let ok = pieces.len() == 2
            && pieces.iter().any(|v| v.len() == 2 && near(v[0], (0.0, 0.0)) && near(v[1], (10.0, 0.0)))
            && pieces.iter().any(|v| v.len() == 2 && near(v[0], (x, 10.0)) && near(v[1], (0.0, 10.0)));
        assert!(ok, "{:.2} degrees: {pieces:?}", th.to_degrees());
    }
}
