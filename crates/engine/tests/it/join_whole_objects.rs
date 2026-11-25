//! JOIN chains whole objects: each one joins the chain only at its own two ends, so it is joined
//! whole or left as it is. A polyline touched at a middle vertex used to be split into segments,
//! and the part the chain didn't reach was deleted with it.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use serde_json::json;

/// What the model holds: each line's end points and each polyline's vertices, sorted so the
/// result doesn't depend on handle order.
fn shapes(s: &Session) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Line(l) => vec![(l.a.x, l.a.y), (l.b.x, l.b.y)],
            EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect(),
            k => panic!("unexpected {k:?}"),
        })
        .collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

fn handles(s: &Session) -> Vec<String> {
    s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect()
}

/// A line that ends on a polyline's middle vertex doesn't join it: neither is changed, in either
/// selection order. Before, JOIN made (0,5) (10,0) (20,0) and deleted the polyline, losing its
/// segment from (10,0) to (0,-5).
#[test]
fn a_polyline_touched_at_a_middle_vertex_is_left_whole() {
    for reversed in [false, true] {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[10, 0], [20, 0]] })).unwrap();
        s.execute("pline", &json!({ "points": [[0, 5], [10, 0], [0, -5]] })).unwrap();
        let before = shapes(&s);
        let mut hs = handles(&s);
        if reversed {
            hs.reverse();
        }
        let err = s.execute("join", &json!({ "handles": hs })).expect_err("nothing joins at a middle vertex").to_string();
        assert!(err.contains("not connected"), "{err}");
        assert_eq!(shapes(&s), before, "selection order reversed: {reversed}");
    }
}

/// Whole objects still join end to end, each turned round as one piece when it runs the other
/// way and added at either end of the chain; an object the chain never reaches is left alone.
#[test]
fn whole_objects_join_end_to_end_in_either_direction() {
    let mut s = Session::new();
    s.execute("pline", &json!({ "points": [[0, 0], [10, 0], [10, 10]] })).unwrap();
    // Ends where the polyline ends.
    s.execute("line", &json!({ "points": [[20, 10], [10, 10]] })).unwrap();
    // Runs the other way: turned round whole.
    s.execute("pline", &json!({ "points": [[30, 20], [30, 10], [20, 10]] })).unwrap();
    // Ends where the first polyline starts: added before it.
    s.execute("line", &json!({ "points": [[-10, 0], [0, 0]] })).unwrap();
    // Touches nothing.
    s.execute("line", &json!({ "points": [[100, 100], [110, 100]] })).unwrap();
    let hs = handles(&s);
    s.execute("join", &json!({ "handles": hs })).unwrap();
    let joined = vec![(-10.0, 0.0), (0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (20.0, 10.0), (30.0, 10.0), (30.0, 20.0)];
    let mut want = vec![joined, vec![(100.0, 100.0), (110.0, 100.0)]];
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(shapes(&s), want);
}
