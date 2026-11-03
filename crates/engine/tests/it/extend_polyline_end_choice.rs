//! Which end of an open polyline EXTEND runs on, as AutoCAD 2024 chooses it (measured with
//! accoreconsole on these drawings): the end on the pick's side of the polyline's middle parameter.
//! The pick's parameter is the segment it is nearest, counted from 0, plus how far along that
//! segment it is; from half the number of segments on, the last end extends, below it the first.
//! Neither the end vertex nearer the pick nor the length along the polyline decides.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

type Vertex = (f64, f64, f64);

fn handle(v: &Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_owned()
}

/// The open polyline `vertices` with lines `edges`, extended at `pick` to the lines; its vertices
/// with their bulges.
fn extended(vertices: &[Vertex], edges: &[([f64; 2], [f64; 2])], pick: [f64; 2]) -> Vec<Vertex> {
    let mut s = Session::new();
    let vs: Vec<Value> = vertices.iter().map(|(x, y, b)| json!({ "p": [x, y], "bulge": b })).collect();
    let p = handle(&s.execute("pline", &json!({ "vertices": vs, "closed": false })).unwrap());
    let es: Vec<String> = edges.iter().map(|(a, b)| handle(&s.execute("line", &json!({ "points": [a, b] })).unwrap())).collect();
    s.execute("extend", &json!({ "handle": p, "pick": pick, "edges": es })).unwrap();
    match &s.doc().unwrap().entity(Handle::parse_hex(&p).unwrap()).unwrap().kind {
        EntityKind::LwPolyline(l) => l.vertices.iter().map(|v| (v.p.x, v.p.y, v.bulge)).collect(),
        k => panic!("{k:?}"),
    }
}

fn same(got: &[Vertex], want: &[Vertex]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6 && (g.2 - w.2).abs() < 1e-6)
}

/// Vertical lines through x = `xs`, from `y0` to `y1`.
fn at(xs: &[f64], y0: f64, y1: f64) -> Vec<([f64; 2], [f64; 2])> {
    xs.iter().map(|x| ([*x, y0], [*x, y1])).collect()
}

fn flat(pts: &[(f64, f64)]) -> Vec<Vertex> {
    pts.iter().map(|(x, y)| (*x, *y, 0.0)).collect()
}

#[test]
fn the_end_on_the_picks_side_of_the_middle_parameter_extends() {
    // (name, vertices, edges, pick, the vertices AutoCAD leaves). Lines at both ends, so either
    // end could extend.
    let cases: Vec<(&str, Vec<Vertex>, Vec<([f64; 2], [f64; 2])>, [f64; 2], Vec<(f64, f64)>)> = vec![
        // 1.2 of 3 segments: the first end, though the pick is on a vertex of the middle one.
        (
            "1.2 of 3",
            flat(&[(0.0, 160.0), (10.0, 160.0), (10.0, 165.0), (20.0, 165.0)]),
            at(&[-5.0, 25.0], 155.0, 170.0),
            [10.0, 161.0],
            vec![(-5.0, 160.0), (10.0, 160.0), (10.0, 165.0), (20.0, 165.0)],
        ),
        // 0.8: the first end, though the last vertex is nearer the pick.
        (
            "0.8, last vertex nearer",
            flat(&[(0.0, 220.0), (20.0, 220.0), (20.0, 230.0), (15.0, 221.0)]),
            vec![([-5.0, 215.0], [-5.0, 235.0]), ([0.0, 210.0], [30.0, 210.0])],
            [16.0, 220.0],
            vec![(-5.0, 220.0), (20.0, 220.0), (20.0, 230.0), (15.0, 221.0)],
        ),
        (
            "0.2",
            flat(&[(0.0, 240.0), (20.0, 240.0), (20.0, 250.0), (5.0, 241.0)]),
            vec![([-5.0, 235.0], [-5.0, 255.0]), ([-10.0, 230.0], [30.0, 230.0])],
            [4.0, 240.0],
            vec![(-5.0, 240.0), (20.0, 240.0), (20.0, 250.0), (5.0, 241.0)],
        ),
        (
            "1.9",
            flat(&[(0.0, 260.0), (2.0, 260.0), (2.0, 280.0), (4.0, 280.0)]),
            at(&[-5.0, 10.0], 255.0, 285.0),
            [2.0, 278.0],
            vec![(0.0, 260.0), (2.0, 260.0), (2.0, 280.0), (10.0, 280.0)],
        ),
        // 0.9: the first end, though 18 of the 22 units along is nearer the last.
        (
            "0.9, length nearer the last",
            flat(&[(0.0, 300.0), (20.0, 300.0), (20.0, 301.0), (21.0, 301.0)]),
            at(&[-5.0, 25.0], 295.0, 305.0),
            [18.0, 300.0],
            vec![(-5.0, 300.0), (20.0, 300.0), (20.0, 301.0), (21.0, 301.0)],
        ),
        (
            "1.1",
            flat(&[(0.0, 320.0), (2.0, 320.0), (2.0, 340.0), (4.0, 340.0)]),
            at(&[-5.0, 10.0], 315.0, 345.0),
            [2.0, 322.0],
            vec![(-5.0, 320.0), (2.0, 320.0), (2.0, 340.0), (4.0, 340.0)],
        ),
        (
            "1.25",
            flat(&[(0.0, 400.0), (30.0, 400.0), (30.0, 420.0), (31.0, 420.0)]),
            at(&[-5.0, 40.0], 395.0, 425.0),
            [30.0, 405.0],
            vec![(-5.0, 400.0), (30.0, 400.0), (30.0, 420.0), (31.0, 420.0)],
        ),
        (
            "1.75",
            flat(&[(0.0, 440.0), (1.0, 440.0), (1.0, 460.0), (31.0, 460.0)]),
            at(&[-5.0, 40.0], 435.0, 465.0),
            [1.0, 455.0],
            vec![(0.0, 440.0), (1.0, 440.0), (1.0, 460.0), (40.0, 460.0)],
        ),
        ("0.6 of 1", flat(&[(0.0, 480.0), (20.0, 480.0)]), at(&[-5.0, 25.0], 475.0, 485.0), [12.0, 480.0], vec![(0.0, 480.0), (25.0, 480.0)]),
        // 1.8 of 4, the end of the segment picked: the first end.
        (
            "1.8 of 4",
            flat(&[(0.0, 500.0), (10.0, 500.0), (10.0, 510.0), (20.0, 510.0), (20.0, 520.0)]),
            vec![([-5.0, 495.0], [-5.0, 525.0]), ([15.0, 530.0], [25.0, 530.0])],
            [10.0, 508.0],
            vec![(-5.0, 500.0), (10.0, 500.0), (10.0, 510.0), (20.0, 510.0), (20.0, 520.0)],
        ),
        (
            "2.8 of 4",
            flat(&[(0.0, 540.0), (10.0, 540.0), (10.0, 550.0), (20.0, 550.0), (20.0, 560.0)]),
            vec![([-5.0, 535.0], [-5.0, 565.0]), ([15.0, 570.0], [25.0, 570.0])],
            [18.0, 550.0],
            vec![(0.0, 540.0), (10.0, 540.0), (10.0, 550.0), (20.0, 550.0), (20.0, 570.0)],
        ),
        // Right on the middle parameter, 1.5 of 3: the last end.
        (
            "1.5 of 3",
            flat(&[(0.0, 600.0), (10.0, 600.0), (10.0, 610.0), (20.0, 610.0)]),
            at(&[-5.0, 25.0], 595.0, 615.0),
            [10.0, 605.0],
            vec![(0.0, 600.0), (10.0, 600.0), (10.0, 610.0), (25.0, 610.0)],
        ),
    ];
    for (name, vertices, edges, pick, want) in cases {
        let got = extended(&vertices, &edges, pick);
        assert!(same(&got, &flat(&want)), "{name}: {got:?}");
    }
}

#[test]
fn the_parameter_runs_along_an_arc_segment_too() {
    // The middle segment an arc of bulge -18, swinging round to within 2 of the first vertex:
    // picked 60% along it (1.6 of 3), 3.6 from the first vertex and 18.3 from the last, the last
    // end extends.
    let got = extended(
        &[(0.0, 0.0, 0.0), (10.0, 0.0, -18.0), (10.0, 1.0, 0.0), (20.0, 1.0, 0.0)],
        &at(&[-5.0, 25.0], -10.0, 10.0),
        [1.804081304825758, 3.0714821777453793],
    );
    assert!(same(&got, &[(0.0, 0.0, 0.0), (10.0, 0.0, -18.0), (10.0, 1.0, 0.0), (25.0, 1.0, 0.0)]), "{got:?}");
    // The end segment an arc of bulge -0.5: picked on it (1.6 of 2) the last end runs on round
    // its circle to x = 14.3; the same polyline turned round, picked on its first segment (0.4),
    // the first end does.
    let got = extended(&[(0.0, 40.0, 0.0), (10.0, 40.0, -0.5), (14.0, 40.0, 0.0)], &at(&[14.3], 30.0, 50.0), [12.0, 41.0]);
    assert!(same(&got, &[(0.0, 40.0, 0.0), (10.0, 40.0, -0.5776771824075506), (14.3, 39.47979589711327, 0.0)]), "{got:?}");
    let got = extended(&[(0.0, 60.0, -0.5), (4.0, 60.0, 0.0), (14.0, 60.0, 0.0)], &at(&[-0.3], 50.0, 70.0), [2.0, 61.0]);
    assert!(same(&got, &[(-0.3, 59.47979589711327, -0.5776771824075504), (4.0, 60.0, 0.0), (14.0, 60.0, 0.0)]), "{got:?}");
}

#[test]
fn a_repeated_vertex_counts_as_a_segment() {
    // (10, 0) twice: three segments, the middle one of no length. Picked on (10, 0), the end of
    // the first, 1.0 of 3: the first end extends (counting two segments it would be the last).
    // Picked 1 up the last segment, 2.1 of 3: the last end.
    let vs = flat(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
    let edges = vec![([-5.0, -10.0], [-5.0, 10.0]), ([-10.0, 15.0], [20.0, 15.0])];
    let got = extended(&vs, &edges, [10.0, 0.0]);
    assert!(same(&got, &flat(&[(-5.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 10.0)])), "{got:?}");
    let got = extended(&vs, &edges, [10.0, 1.0]);
    assert!(same(&got, &flat(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 15.0)])), "{got:?}");
}

#[test]
fn a_pick_a_hair_outside_an_arc_end_counts_as_that_end() {
    // A single semicircle from (1, 0) to (-1, 0). Picked 1e-13 outside its sweep at the first
    // vertex, the pick is on the arc's start, not round the circle at its end: the first end runs
    // on to x = 0, three quarters of the way round.
    let b = 1.0 + 2f64.sqrt();
    let got = extended(&[(1.0, 0.0, 1.0), (-1.0, 0.0, 0.0)], &at(&[0.0], -2.0, -0.5), [1.0, -1e-13]);
    assert!(same(&got, &[(0.0, -1.0, b), (-1.0, 0.0, 0.0)]), "{got:?}");
    // A hair past its end is the last end.
    let got = extended(&[(1.0, 0.0, 1.0), (-1.0, 0.0, 0.0)], &at(&[0.0], -2.0, -0.5), [-1.0, -1e-13]);
    assert!(same(&got, &[(1.0, 0.0, b), (0.0, -1.0, 0.0)]), "{got:?}");
    // Turning clockwise, the arc's start angle is the polyline's last vertex: a hair outside it is
    // the last end.
    let got = extended(&[(1.0, 0.0, -1.0), (-1.0, 0.0, 0.0)], &at(&[0.0], 0.5, 2.0), [-1.0, 1e-13]);
    assert!(same(&got, &[(1.0, 0.0, -b), (0.0, 1.0, 0.0)]), "{got:?}");
}
