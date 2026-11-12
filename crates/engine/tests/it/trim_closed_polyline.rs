//! TRIM on a closed polyline: the piece containing the pick goes, the complementary run stays,
//! whichever side of the stored start vertex the pick is on (a pick after the last cutting edge
//! used to keep one extra circuit of the shape).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

type Vertex = (f64, f64, f64);

fn r(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Every polyline in model space: its closed flag and its vertices with their bulges (rounded).
fn polys(s: &Session) -> Vec<(bool, Vec<Vertex>)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::LwPolyline(p) => Some((p.closed, p.vertices.iter().map(|v| (r(v.p.x), r(v.p.y), r(v.bulge))).collect())),
            _ => None,
        })
        .collect()
}

fn lines(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count()
}

fn handle(v: serde_json::Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_owned()
}

/// A closed polyline from `vertices` (each `[x, y]` or `{p, bulge}`) and a vertical cutting line
/// at x = 2 through the 4 x 4 square, trimmed at `pick`.
fn trim(vertices: serde_json::Value, pick: [f64; 2]) -> Session {
    let mut s = Session::new();
    let p = handle(s.execute("pline", &json!({ "vertices": vertices, "closed": true })).unwrap());
    let cutter = handle(s.execute("line", &json!({ "points": [[2, -1], [2, 5]] })).unwrap());
    s.execute("trim", &json!({ "handle": p, "pick": pick, "edges": [cutter] })).unwrap();
    s
}

fn square() -> serde_json::Value {
    json!([[0, 0], [4, 0], [4, 4], [0, 4]])
}

const RIGHT: [Vertex; 4] = [(2.0, 0.0, 0.0), (4.0, 0.0, 0.0), (4.0, 4.0, 0.0), (2.0, 4.0, 0.0)];
const LEFT: [Vertex; 4] = [(2.0, 4.0, 0.0), (0.0, 4.0, 0.0), (0.0, 0.0, 0.0), (2.0, 0.0, 0.0)];

#[test]
fn a_pick_after_the_last_cutting_edge_keeps_only_the_complementary_run() {
    // Left edge, and top-left: both lie past the second intersection along the stored order.
    for pick in [[0.0, 2.0], [0.0, 4.0], [1.0, 4.0]] {
        let s = trim(square(), pick);
        assert_eq!(polys(&s), vec![(false, RIGHT.to_vec())], "pick {pick:?}");
    }
}

#[test]
fn picks_on_the_other_pieces_still_leave_the_other_run() {
    // Before the first intersection (across the start vertex) and between the two.
    for pick in [[1.0, 0.0], [0.0, 0.0]] {
        let s = trim(square(), pick);
        assert_eq!(polys(&s), vec![(false, RIGHT.to_vec())], "pick {pick:?}");
    }
    for pick in [[4.0, 2.0], [3.0, 0.0]] {
        let s = trim(square(), pick);
        assert_eq!(polys(&s), vec![(false, LEFT.to_vec())], "pick {pick:?}");
    }
}

#[test]
fn the_stored_start_vertex_does_not_change_the_result() {
    // The same square stored from (4, 0), (4, 4) and (0, 4).
    let starts = [json!([[4, 0], [4, 4], [0, 4], [0, 0]]), json!([[4, 4], [0, 4], [0, 0], [4, 0]]), json!([[0, 4], [0, 0], [4, 0], [4, 4]])];
    for vertices in starts {
        for (pick, kept) in [([0.0, 2.0], RIGHT), ([1.0, 0.0], RIGHT), ([4.0, 2.0], LEFT)] {
            let s = trim(vertices.clone(), pick);
            let polys = polys(&s);
            assert_eq!(polys.len(), 1, "{vertices} {pick:?}");
            let (closed, vs) = &polys[0];
            assert!(!closed);
            // The same run, from either end.
            let mut want = kept.to_vec();
            let mut rev = want.clone();
            rev.reverse();
            want.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut got = vs.clone();
            got.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(got, want, "{vertices} {pick:?}: {vs:?}");
            assert!(*vs == kept.to_vec() || *vs == rev, "{vertices} {pick:?}: {vs:?}");
        }
    }
}

#[test]
fn a_closing_arc_is_dropped_with_the_picked_piece() {
    // The last segment, (0,4) to (0,0), is a semicircle bulging left to x = -2.
    let d = json!([[0, 0], [4, 0], [4, 4], { "p": [0, 4], "bulge": 1 }]);
    for pick in [[-2.0, 2.0], [-1.0, 3.5]] {
        let s = trim(d.clone(), pick);
        assert_eq!(polys(&s), vec![(false, RIGHT.to_vec())], "pick {pick:?}");
    }
    // Picking the right-hand side keeps the arc and its bulge.
    let s = trim(d, [4.0, 2.0]);
    let polys = polys(&s);
    assert_eq!(polys.len(), 1);
    let (closed, vs) = &polys[0];
    assert!(!closed);
    assert_eq!((vs.first().map(|v| (v.0, v.1)), vs.last().map(|v| (v.0, v.1))), (Some((2.0, 4.0)), Some((2.0, 0.0))));
    assert!(vs.iter().any(|v| v.2 != 0.0), "the arc keeps its bulge: {vs:?}");
}

#[test]
fn undo_restores_the_closed_polyline() {
    let mut s = trim(square(), [0.0, 2.0]);
    assert_eq!(lines(&s), 1);
    s.undo().unwrap();
    assert_eq!(polys(&s), vec![(true, vec![(0.0, 0.0, 0.0), (4.0, 0.0, 0.0), (4.0, 4.0, 0.0), (0.0, 4.0, 0.0)])]);
}

#[test]
fn a_single_cut_still_opens_the_outline_there() {
    let mut s = Session::new();
    let p = handle(s.execute("pline", &json!({ "vertices": square(), "closed": true })).unwrap());
    let cutter = handle(s.execute("line", &json!({ "points": [[2, -1], [2, 1]] })).unwrap());
    s.execute("trim", &json!({ "handle": p, "pick": [0, 2], "edges": [cutter] })).unwrap();
    let all = [(2.0, 0.0, 0.0), (4.0, 0.0, 0.0), (4.0, 4.0, 0.0), (0.0, 4.0, 0.0), (0.0, 0.0, 0.0), (2.0, 0.0, 0.0)];
    assert_eq!(polys(&s), vec![(false, all.to_vec())]);
}
