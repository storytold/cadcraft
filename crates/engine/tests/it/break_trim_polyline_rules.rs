//! BREAK on a closed polyline and TRIM of a polyline that is one of its own cutting edges, as
//! AutoCAD 2024 does them (measured with accoreconsole on these drawings):
//!
//! - BREAK between two points of a closed polyline removes the stretch between them that doesn't
//!   run through its start vertex, whichever point comes first; BREAK or BREAKATPOINT at one point
//!   splits it there into two open polylines, and at its start vertex leaves it closed.
//! - In Standard mode, a polyline that is one of the cutting edges (picked, or with every object)
//!   is also cut at its own vertices and where it crosses itself.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

type Vertex = (f64, f64, f64);

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

fn pline(s: &mut Session, vertices: Value, closed: bool) -> Handle {
    handle(&s.execute("pline", &json!({ "vertices": vertices, "closed": closed })).unwrap())
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    handle(&s.execute("line", &json!({ "points": [a, b] })).unwrap())
}

/// Every polyline in model space in order: its handle, closed flag and vertices with bulges.
fn polys(s: &Session) -> Vec<(Handle, bool, Vec<Vertex>)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::LwPolyline(p) => Some((e.handle, p.closed, p.vertices.iter().map(|v| (v.p.x, v.p.y, v.bulge)).collect())),
            _ => None,
        })
        .collect()
}

fn same(got: &[Vertex], want: &[Vertex]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9 && (g.2 - w.2).abs() < 1e-9)
}

/// The polylines are `want` in order, (`kept`, closed, vertices): `kept` is whether it has the
/// handle `h`.
fn expect(s: &Session, h: Handle, want: &[(bool, bool, &[Vertex])]) {
    let got = polys(s);
    assert_eq!(got.len(), want.len(), "{got:?}");
    for ((gh, gc, gv), (kept, closed, wv)) in got.iter().zip(want) {
        assert_eq!((*gh == h, *gc), (*kept, *closed), "{got:?}");
        assert!(same(gv, wv), "{gv:?} != {wv:?}");
    }
}

fn run(s: &mut Session, script: &str) {
    s.script(script).unwrap();
    assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
}

// ---------------- BREAK on a closed polyline ----------------

#[test]
fn break_removes_the_stretch_between_the_points_that_misses_the_start_vertex() {
    // 100 x 10 rectangle broken at (10, 0) and (10, 10): the 190-unit stretch between them goes,
    // the 20 units across the start vertex stay, with the points either way round.
    for script in ["BREAK 10,0 10,10\n", "BREAK 10,10 10,0\n"] {
        let mut s = Session::new();
        let r = pline(&mut s, json!([[0, 0], [100, 0], [100, 10], [0, 10]]), true);
        run(&mut s, script);
        expect(&s, r, &[(true, false, &[(10.0, 10.0, 0.0), (0.0, 10.0, 0.0), (0.0, 0.0, 0.0), (10.0, 0.0, 0.0)])]);
    }
}

#[test]
fn break_with_the_first_point_after_the_second_keeps_the_run_through_the_start() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], { "p": [40, 0], "bulge": 0.5 }, [60, 0], [60, 30], [0, 30]]), true);
    run(&mut s, "BREAK 60,20 10,0\n");
    expect(&s, p, &[(true, false, &[(60.0, 20.0, 0.0), (60.0, 30.0, 0.0), (0.0, 30.0, 0.0), (0.0, 0.0, 0.0), (10.0, 0.0, 0.0)])]);
}

const SQUARE: [[f64; 2]; 4] = [[250.0, 0.0], [270.0, 0.0], [270.0, 20.0], [250.0, 20.0]];

#[test]
fn break_at_one_point_splits_a_closed_polyline_in_two() {
    let first: &[Vertex] = &[(250.0, 0.0, 0.0), (260.0, 0.0, 0.0)];
    let second: &[Vertex] = &[(260.0, 0.0, 0.0), (270.0, 0.0, 0.0), (270.0, 20.0, 0.0), (250.0, 20.0, 0.0), (250.0, 0.0, 0.0)];
    let mut s = Session::new();
    let p = pline(&mut s, json!(SQUARE), true);
    run(&mut s, "BREAK 260,0 260,0\n");
    expect(&s, p, &[(true, false, first), (false, false, second)]);
    // BREAKATPOINT does the same.
    let mut s = Session::new();
    let p = pline(&mut s, json!(SQUARE), true);
    let r = s.execute("breakatpoint", &json!({ "handle": p.hex(), "at": [260, 0] })).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    expect(&s, p, &[(true, false, first), (false, false, second)]);
}

#[test]
fn break_at_a_vertex_splits_there_and_at_the_start_vertex_changes_nothing() {
    let mut s = Session::new();
    let p = pline(&mut s, json!(SQUARE), true);
    run(&mut s, "BREAK 270,20 270,20\n");
    expect(
        &s,
        p,
        &[
            (true, false, &[(250.0, 0.0, 0.0), (270.0, 0.0, 0.0), (270.0, 20.0, 0.0)]),
            (false, false, &[(270.0, 20.0, 0.0), (250.0, 20.0, 0.0), (250.0, 0.0, 0.0)]),
        ],
    );
    let mut s = Session::new();
    let p = pline(&mut s, json!(SQUARE), true);
    run(&mut s, "BREAK 250,0 250,0\n");
    expect(&s, p, &[(true, true, &[(250.0, 0.0, 0.0), (270.0, 0.0, 0.0), (270.0, 20.0, 0.0), (250.0, 20.0, 0.0)])]);
}

// ---------------- TRIM of a polyline that cuts itself ----------------

/// A polyline with an arc, and vertical lines at x = 20 and x = 60 crossing it.
fn arc_polyline(s: &mut Session) -> (Handle, Handle, Handle) {
    let p = pline(s, json!([[0, 0], { "p": [40, 0], "bulge": 0.4 }, [70, 0], [90, 30]]), false);
    let b = line(s, [20.0, -10.0], [20.0, 40.0]);
    let c = line(s, [60.0, -20.0], [60.0, 40.0]);
    (p, b, c)
}

/// The piece from the x = 60 cut on the arc to the end, as measured.
const FROM_C: [Vertex; 3] = [(60.0, -5.417486860749438, 0.1330399827453447), (70.0, 0.0, 0.0), (90.0, 30.0, 0.0)];

fn trim(s: &mut Session, h: Handle, pick: [f64; 2], edges: Option<&[Handle]>) {
    let mut p = json!({ "handle": h.hex(), "pick": pick });
    if let Some(es) = edges {
        p["edges"] = json!(es.iter().map(|e| e.hex()).collect::<Vec<_>>());
    }
    s.execute("trim", &p).unwrap();
}

#[test]
fn with_every_object_cutting_a_polyline_trims_to_its_own_vertex() {
    let mut s = Session::new();
    let (p, ..) = arc_polyline(&mut s);
    trim(&mut s, p, [55.0, -6.0], None);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0)]), (false, false, &FROM_C)]);
}

#[test]
fn edges_that_leave_the_polyline_out_cut_only_where_they_cross_it() {
    let mut s = Session::new();
    let (p, b, c) = arc_polyline(&mut s);
    trim(&mut s, p, [55.0, -6.0], Some(&[b, c]));
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (20.0, 0.0, 0.0)]), (false, false, &FROM_C)]);
}

#[test]
fn a_polyline_that_is_its_only_edge_loses_the_whole_segment_picked() {
    let mut s = Session::new();
    let (p, ..) = arc_polyline(&mut s);
    trim(&mut s, p, [55.0, -6.0], Some(&[p]));
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0)]), (false, false, &[(70.0, 0.0, 0.0), (90.0, 30.0, 0.0)])]);
    // The first segment: one piece left, the polyline itself.
    let mut s = Session::new();
    let (p, ..) = arc_polyline(&mut s);
    trim(&mut s, p, [10.0, 0.0], Some(&[p]));
    expect(&s, p, &[(true, false, &[(40.0, 0.0, 0.4), (70.0, 0.0, 0.0), (90.0, 30.0, 0.0)])]);
}

#[test]
fn a_closed_polyline_that_is_its_only_edge_opens_without_the_segment_picked() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [40, 0], [40, 30], [0, 30]]), true);
    trim(&mut s, p, [20.0, 0.0], Some(&[p]));
    expect(&s, p, &[(true, false, &[(40.0, 0.0, 0.0), (40.0, 30.0, 0.0), (0.0, 30.0, 0.0), (0.0, 0.0, 0.0)])]);
}

#[test]
fn a_polyline_crossing_itself_is_cut_at_the_crossing() {
    let x = json!([[0, 0], [40, 20], [40, 0], [0, 20]]);
    let mut s = Session::new();
    let p = pline(&mut s, x.clone(), false);
    trim(&mut s, p, [10.0, 5.0], Some(&[p]));
    expect(&s, p, &[(true, false, &[(20.0, 10.0, 0.0), (40.0, 20.0, 0.0), (40.0, 0.0, 0.0), (0.0, 20.0, 0.0)])]);
    // Every object (the polyline alone): cut between the crossing and the next vertex.
    let mut s = Session::new();
    let p = pline(&mut s, x, false);
    trim(&mut s, p, [30.0, 15.0], None);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (20.0, 10.0, 0.0)]), (false, false, &[(40.0, 20.0, 0.0), (40.0, 0.0, 0.0), (0.0, 20.0, 0.0)])]);
}

#[test]
fn a_closed_polyline_at_survey_coordinates_trims_to_its_own_vertex() {
    let mut s = Session::new();
    let p = pline(
        &mut s,
        json!([[612345.678, 2398765.432], { "p": [612385.678, 2398765.432], "bulge": 0.4 }, [612415.678, 2398765.432], [612415.678, 2398795.432], [612345.678, 2398795.432]]),
        true,
    );
    let b = line(&mut s, [612365.678, 2398755.432], [612365.678, 2398805.432]);
    let c = line(&mut s, [612405.678, 2398745.432], [612405.678, 2398805.432]);
    trim(&mut s, p, [612400.678, 2398759.432], Some(&[p, b, c]));
    let got = polys(&s);
    assert_eq!(got.len(), 1);
    let (h, closed, vs) = &got[0];
    assert!(*h == p && !closed);
    let want: [Vertex; 6] = [
        (612405.678, 2398760.014513139, 0.1330399827461127),
        (612415.678, 2398765.432, 0.0),
        (612415.678, 2398795.432, 0.0),
        (612345.678, 2398795.432, 0.0),
        (612345.678, 2398765.432, 0.0),
        (612385.678, 2398765.432, 0.0),
    ];
    // Survey coordinates: a micrometre on the points, the bulge to 1e-9.
    assert_eq!(vs.len(), want.len());
    for (g, w) in vs.iter().zip(&want) {
        assert!((g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6 && (g.2 - w.2).abs() < 1e-9, "{g:?} != {w:?}");
    }
}

#[test]
fn standard_mode_at_the_command_line_with_every_object_cuts_at_the_polyline_vertex() {
    let mut s = Session::new();
    let (p, ..) = arc_polyline(&mut s);
    // Mode Standard, Enter: every object is a cutting edge.
    run(&mut s, "TRIM O S\n\n55,-6\n\n");
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0)]), (false, false, &FROM_C)]);
}

#[test]
fn a_polyline_picked_as_an_edge_in_quick_mode_cuts_itself_too() {
    // Quick mode with the edges given (its cuTting edges option): picked is picked.
    let mut s = Session::new();
    let (p, _, c) = arc_polyline(&mut s);
    s.execute("trim", &json!({ "handle": p.hex(), "pick": [55, -6], "mode": "quick", "edges": [p.hex(), c.hex()] })).unwrap();
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0)]), (false, false, &FROM_C)]);
}

#[test]
fn quick_mode_still_cuts_only_at_other_objects() {
    // Not measured: AutoCAD's scripted TRIM runs Standard mode only.
    let mut s = Session::new();
    let (p, ..) = arc_polyline(&mut s);
    s.execute("trim", &json!({ "handle": p.hex(), "pick": [55, -6], "mode": "quick" })).unwrap();
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (20.0, 0.0, 0.0)]), (false, false, &FROM_C)]);
}
