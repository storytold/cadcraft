//! BREAK on a closed polyline and TRIM of a polyline that is one of its own cutting edges, as
//! AutoCAD 2024 does them (measured with accoreconsole on these drawings):
//!
//! - BREAK between two points of a closed polyline removes the stretch between them that doesn't
//!   run through its start vertex, whichever point comes first; BREAK or BREAKATPOINT at one point
//!   splits it there into two open polylines, and at its start vertex leaves it closed.
//! - A polyline that is one of the cutting edges (picked, or with every object, in Standard or
//!   Quick mode) is also cut at its own vertices, where it crosses itself and where a vertex of it
//!   touches another of its segments. A piece whose ends meet there is left closed. A vertex
//!   between two line segments in line (within 1e-8, straight on or doubling back) doesn't cut it.
//! - A pick within 5e-7 of one of the polyline's own cuts, whatever its size, trims nothing; a cut
//!   by another edge that near the pick is passed over. An edge through a closed polyline's start
//!   vertex cuts it there.
//!
//! Quick mode was measured in AutoCAD 2024 itself, picking on screen: its scripted TRIM runs
//! Standard mode only.

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
fn quick_mode_with_every_object_cuts_a_polyline_at_its_own_vertex_too() {
    // As Standard mode with every object: from the vertex at (40, 0), not from the line at x = 20.
    let mut s = Session::new();
    let (p, ..) = arc_polyline(&mut s);
    s.execute("trim", &json!({ "handle": p.hex(), "pick": [55, -6], "mode": "quick" })).unwrap();
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0)]), (false, false, &FROM_C)]);
}

/// A rectangle, then an open polyline whose last vertex (6.9, 6.2) touches its second segment
/// inside the rectangle (reversed: whose first vertex does). The polyline's handle.
fn touching(s: &mut Session, reversed: bool) -> Handle {
    pline(s, json!([[3.9, 3.8], [8.3, 3.8], [8.3, 6.9], [3.9, 6.9]]), true);
    let mut v = vec![[2.3, 3.3], [5.3, 8.0], [8.5, 4.4], [8.8, 7.8], [6.9, 6.2]];
    if reversed {
        v.reverse();
    }
    pline(s, json!(v), false)
}

const RECT: [Vertex; 4] = [(3.9, 3.8, 0.0), (8.3, 3.8, 0.0), (8.3, 6.9, 0.0), (3.9, 6.9, 0.0)];
/// Where the second segment crosses the rectangle's top and right sides.
const TOP: Vertex = (6.277777777777777, 6.9, 0.0);
const RIGHT: Vertex = (8.3, 4.625, 0.0);

#[test]
fn a_polyline_is_cut_where_its_end_touches_it() {
    let start: [Vertex; 3] = [(2.3, 3.3, 0.0), (5.3, 8.0, 0.0), TOP];
    let looped: [Vertex; 3] = [(6.9, 6.2, 0.0), (8.5, 4.4, 0.0), (8.8, 7.8, 0.0)];
    // Picked above the touch, in the rectangle: cut from the top side to the touch. What is left
    // after it ends where it starts, so it is closed. Quick mode as picked on screen, and Standard
    // mode with every object at the command line.
    let mut s = Session::new();
    let p = touching(&mut s, false);
    s.execute("trim", &json!({ "handle": p.hex(), "pick": [6.58, 6.56], "mode": "quick" })).unwrap();
    expect(&s, p, &[(false, true, &RECT), (true, false, &start), (false, true, &looped)]);
    let mut s = Session::new();
    let p = touching(&mut s, false);
    run(&mut s, "TRIM O S\n\n6.58,6.56\n\n");
    expect(&s, p, &[(false, true, &RECT), (true, false, &start), (false, true, &looped)]);
    // Below the touch: cut from the touch to the right side.
    let mut s = Session::new();
    let p = touching(&mut s, false);
    trim(&mut s, p, [7.604, 5.408], None);
    let end: [Vertex; 4] = [RIGHT, (8.5, 4.4, 0.0), (8.8, 7.8, 0.0), (6.9, 6.2, 0.0)];
    expect(&s, p, &[(false, true, &RECT), (true, false, &[(2.3, 3.3, 0.0), (5.3, 8.0, 0.0), (6.9, 6.2, 0.0)]), (false, false, &end)]);
    // The first vertex touching: the closed loop is the piece that keeps the handle.
    let mut s = Session::new();
    let p = touching(&mut s, true);
    trim(&mut s, p, [6.58, 6.56], None);
    expect(
        &s,
        p,
        &[
            (false, true, &RECT),
            (true, true, &[(6.9, 6.2, 0.0), (8.8, 7.8, 0.0), (8.5, 4.4, 0.0)]),
            (false, false, &[TOP, (5.3, 8.0, 0.0), (2.3, 3.3, 0.0)]),
        ],
    );
}

#[test]
fn a_closed_piece_keeps_its_arcs() {
    let mut s = Session::new();
    pline(&mut s, json!([[3.9, 3.8], [8.3, 3.8], [8.3, 6.9], [3.9, 6.9]]), true);
    let p = pline(&mut s, json!([[2.3, 3.3], [5.3, 8.0], { "p": [8.5, 4.4], "bulge": 0.3 }, [8.8, 7.8], [6.9, 6.2]]), false);
    trim(&mut s, p, [6.58, 6.56], None);
    let looped: [Vertex; 3] = [(6.9, 6.2, 0.0), (8.5, 4.4, 0.3), (8.8, 7.8, 0.0)];
    expect(&s, p, &[(false, true, &RECT), (true, false, &[(2.3, 3.3, 0.0), (5.3, 8.0, 0.0), TOP]), (false, true, &looped)]);
}

#[test]
fn a_pick_half_a_unit_from_a_vertex_1e9_out_trims_the_segment_it_is_on() {
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [1e9, 0], [1e9, 10]]), false);
    trim(&mut s, p, [1e9, 0.5], None);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (1e9, 0.0, 0.0)])]);
}

// ---------------- a pick on a cut ----------------

type Edge = ([f64; 2], [f64; 2]);

/// The polyline `v`, with lines `edges`, trimmed at `pick` with every object as an edge, or only
/// the lines when `only_lines`; the polyline's handle and whether the trim was made.
fn attempt(v: Value, closed: bool, edges: &[Edge], only_lines: bool, pick: [f64; 2]) -> (Session, Handle, bool) {
    let mut s = Session::new();
    let p = pline(&mut s, v, closed);
    let ls: Vec<String> = edges.iter().map(|(a, b)| line(&mut s, *a, *b).hex()).collect();
    let mut params = json!({ "handle": p.hex(), "pick": pick });
    if only_lines {
        params["edges"] = json!(ls);
    }
    let made = s.execute("trim", &params).is_ok();
    (s, p, made)
}

/// [`attempt`] with at most one line, which must trim.
fn trimmed(v: Value, closed: bool, edge: Option<Edge>, only_line: bool, pick: [f64; 2]) -> (Session, Handle) {
    let (s, p, made) = attempt(v, closed, edge.as_slice(), only_line, pick);
    assert!(made, "{pick:?}");
    (s, p)
}

/// A vertical line through x = `x`.
fn at(x: f64) -> Edge {
    ([x, -5.0], [x, 5.0])
}

#[test]
fn a_pick_on_a_cut_trims_nothing() {
    // The vertex (10, 0) is the only cut. A pick whose nearest point is within 5e-7 of it, on it
    // or off to the side, leaves the polyline as it was; one 6e-7 from it trims the segment it is
    // on.
    let v = || json!([[0, 0], [10, 0], [10, 10]]);
    let whole: [Vertex; 3] = [(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)];
    for pick in [[10.0, 0.0], [10.0, 4e-7], [10.5, 4e-7], [9.99999999, 0.0]] {
        let (s, p) = trimmed(v(), false, None, false, pick);
        expect(&s, p, &[(true, false, &whole)]);
    }
    let (s, p) = trimmed(v(), false, None, false, [10.0, 6e-7]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0)])]);
    let (s, p) = trimmed(v(), false, None, false, [9.9999994, 0.0]);
    expect(&s, p, &[(true, false, &[(10.0, 0.0, 0.0), (10.0, 10.0, 0.0)])]);
}

#[test]
fn a_pick_on_a_cut_is_within_the_same_distance_at_any_size() {
    // A polyline a thousandth across, and one 1e9 long with a line through its vertex: 1e-7 from
    // the vertex is on the cut, 1e-6 off it.
    let small = || json!([[0, 0], [0.001, 0], [0.001, 0.001]]);
    let (s, p) = trimmed(small(), false, None, false, [0.001, 1e-7]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (0.001, 0.0, 0.0), (0.001, 0.001, 0.0)])]);
    let (s, p) = trimmed(small(), false, None, false, [0.001, 1e-6]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (0.001, 0.0, 0.0)])]);
    let long = || json!([[0, 0], [10, 0], [10, 1e9]]);
    let through = Some(([0.0, -10.0], [20.0, 10.0]));
    let (s, p) = trimmed(long(), false, through, false, [10.0, 1e-7]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 1e9, 0.0)])]);
    let (s, p) = trimmed(long(), false, through, false, [10.0, 1e-6]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0)])]);
}

#[test]
fn a_cut_by_another_edge_at_the_pick_is_passed_over() {
    // A pick within 5e-7 of a cut by another edge runs the piece on across it, to the next cut
    // either side; 6e-7 away the cut holds.
    let flat = || json!([[0, 0], [10, 0]]);
    let piece = |a: f64, b: f64| [(a, 0.0, 0.0), (b, 0.0, 0.0)];
    for pick in [[3.0, 0.0], [3.0000004, 0.0]] {
        let (s, p, _) = attempt(flat(), false, &[at(3.0), at(7.0)], true, pick);
        expect(&s, p, &[(true, false, &piece(7.0, 10.0))]);
    }
    let (s, p, _) = attempt(flat(), false, &[at(3.0), at(7.0)], true, [3.0000006, 0.0]);
    expect(&s, p, &[(true, false, &piece(0.0, 3.0)), (false, false, &piece(7.0, 10.0))]);
    // Lines at 3, 5 and 7: picked on 5, 3 to 7 goes; picked on 7, 5 to the end.
    let (s, p, _) = attempt(flat(), false, &[at(3.0), at(5.0), at(7.0)], true, [5.0, 0.0]);
    expect(&s, p, &[(true, false, &piece(0.0, 3.0)), (false, false, &piece(7.0, 10.0))]);
    let (s, p, _) = attempt(flat(), false, &[at(3.0), at(5.0), at(7.0)], true, [7.0, 0.0]);
    expect(&s, p, &[(true, false, &piece(0.0, 5.0))]);
    // Every object an edge: on to the polyline's own vertex (10, 0).
    let (s, p, _) = attempt(json!([[0, 0], [10, 0], [10, 10]]), false, &[at(5.0)], false, [5.0, 0.0]);
    expect(&s, p, &[(true, false, &[(10.0, 0.0, 0.0), (10.0, 10.0, 0.0)])]);
    // A closed rectangle cut at x = 30 and 70 top and bottom, picked on the bottom at 30: round
    // its start vertex, from 30 on the top to 70 on the bottom.
    let tall = |x: f64| ([x, -5.0], [x, 15.0]);
    let (s, p, _) = attempt(json!([[0, 0], [100, 0], [100, 10], [0, 10]]), true, &[tall(30.0), tall(70.0)], true, [30.0, 0.0]);
    expect(&s, p, &[(true, false, &[(70.0, 0.0, 0.0), (100.0, 0.0, 0.0), (100.0, 10.0, 0.0), (30.0, 10.0, 0.0)])]);
    // One line only, picked on its cut or 4e-7 past it: no cut is left, and nothing is trimmed.
    for pick in [[5.0, 0.0], [5.0000004, 0.0]] {
        let (s, p, made) = attempt(json!([[0, 0], [10, 0], [10, 10]]), false, &[at(5.0)], true, pick);
        assert!(!made);
        expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)])]);
    }
    // In Quick mode too: the polyline meets the line, so it isn't erased as one no edge crosses.
    let mut s = Session::new();
    let p = pline(&mut s, json!([[0, 0], [10, 0]]), false);
    line(&mut s, [5.0, -5.0], [5.0, 5.0]);
    assert!(s.execute("trim", &json!({ "handle": p.hex(), "pick": [5, 0], "mode": "quick" })).is_err());
    expect(&s, p, &[(true, false, &piece(0.0, 10.0))]);
}

#[test]
fn a_pick_on_a_vertex_an_edge_runs_through_trims_nothing() {
    // The line's cut is found on the segments either side of the vertex, a rounding apart. Both
    // are passed over, so no cut is left and the polyline stays as it was, not split there.
    let cases = [
        ([[5.5, 1.1], [23.3, 7.7], [30.1, 29.9]], ([23.3, 7.7], [45.7, 2.3])),
        ([[5.5, 1.1], [23.3, 7.7], [30.1, 29.9]], ([2.0 * 23.3 - 45.7, 2.0 * 7.7 - 2.3], [45.7, 2.3])),
        ([[1.3, 2.7], [37.9, 11.3], [55.1, 48.7]], ([4.5, 43.5], [71.3, -20.9])),
    ];
    for (v, edge) in cases {
        let (s, p, made) = attempt(json!(v), false, &[edge], true, v[1]);
        let whole: Vec<Vertex> = v.iter().map(|q| (q[0], q[1], 0.0)).collect();
        assert!(!made);
        expect(&s, p, &[(true, false, &whole)]);
    }
}

#[test]
fn an_edge_through_a_closed_polylines_start_vertex_cuts_it_there() {
    // The line cuts the rectangle at (0, 0) and at (10, 10) on its top. The pick takes the piece
    // it is on, whichever vertex the rectangle starts at. 4e-7 from (0, 0) that cut is passed
    // over, which leaves one, and the trim is refused.
    let rect = || json!([[0, 0], [100, 0], [100, 10], [0, 10]]);
    let edge = Some(([-5.0, -5.0], [15.0, 15.0]));
    let left: [Vertex; 3] = [(10.0, 10.0, 0.0), (0.0, 10.0, 0.0), (0.0, 0.0, 0.0)];
    let (s, p) = trimmed(rect(), true, edge, true, [50.0, 0.0]);
    expect(&s, p, &[(true, false, &left)]);
    let (s, p) = trimmed(json!([[50, 0], [100, 0], [100, 10], [0, 10], [0, 0]]), true, edge, true, [50.0, 10.0]);
    expect(&s, p, &[(true, false, &left)]);
    let (s, p) = trimmed(rect(), true, edge, true, [0.0, 5.0]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (100.0, 0.0, 0.0), (100.0, 10.0, 0.0), (10.0, 10.0, 0.0)])]);
    let whole: [Vertex; 4] = [(0.0, 0.0, 0.0), (100.0, 0.0, 0.0), (100.0, 10.0, 0.0), (0.0, 10.0, 0.0)];
    let (s, p, made) = attempt(rect(), true, edge.as_slice(), true, [0.0, 4e-7]);
    assert!(!made);
    expect(&s, p, &[(true, true, &whole)]);
    // The rectangle alone, picked on its start vertex, and 4e-7 from it on its last segment.
    for pick in [[0.0, 0.0], [0.0, 4e-7]] {
        let (s, p) = trimmed(rect(), true, None, false, pick);
        expect(&s, p, &[(true, true, &whole)]);
    }
}

#[test]
fn a_closed_polyline_cut_at_one_point_is_left_as_it_was() {
    // A line touching the rectangle only at its start vertex, and one crossing its bottom once and
    // ending inside: AutoCAD refuses either trim, and Quick mode doesn't erase the rectangle.
    for (a, b, pick) in [([-5.0, 5.0], [5.0, -5.0], [50.0, 0.0]), ([50.0, -5.0], [50.0, 5.0], [20.0, 0.0])] {
        for mode in ["standard", "quick"] {
            let mut s = Session::new();
            let p = pline(&mut s, json!([[0, 0], [100, 0], [100, 10], [0, 10]]), true);
            let l = line(&mut s, a, b);
            let before = polys(&s);
            let r = s.execute("trim", &json!({ "handle": p.hex(), "pick": pick, "edges": [l.hex()], "mode": mode }));
            assert!(r.is_err_and(|e| e.to_string() == "Curve must intersect cutting edges at least twice."), "{a:?} {mode}");
            assert_eq!(polys(&s), before, "{a:?} {mode}");
        }
    }
}

// ---------------- where a polyline runs straight on ----------------

#[test]
fn a_vertex_the_polyline_runs_straight_on_through_doesnt_cut_it() {
    // (5, 0) lies between two segments in line: the pick takes everything back to the cut before
    // it, the start of the polyline or the closed polyline's start vertex.
    let v = || json!([[0, 0], [5, 0], [10, 0], [10, 10]]);
    let (s, p) = trimmed(v(), false, None, false, [7.0, 0.0]);
    expect(&s, p, &[(true, false, &[(10.0, 0.0, 0.0), (10.0, 10.0, 0.0)])]);
    let (s, p) = trimmed(v(), true, None, false, [7.0, 0.0]);
    expect(&s, p, &[(true, false, &[(10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 0.0, 0.0)])]);
    // A crossing on the segment in line before the one picked bounds the piece, as does one on the
    // segment in line after it, and one right at the vertex in line.
    let (s, p) = trimmed(json!([[0, 0], [5, 0], [10, 0], [10, 10], [2, 10], [2, -5]]), false, None, false, [7.0, 0.0]);
    expect(
        &s,
        p,
        &[
            (true, false, &[(0.0, 0.0, 0.0), (2.0, 0.0, 0.0)]),
            (false, false, &[(10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (2.0, 10.0, 0.0), (2.0, -5.0, 0.0)]),
        ],
    );
    let (s, p) = trimmed(json!([[0, 0], [5, 0], [10, 0], [10, 10], [8, 10], [8, -5]]), false, None, false, [3.5, 0.0]);
    expect(&s, p, &[(true, false, &[(8.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (8.0, 10.0, 0.0), (8.0, -5.0, 0.0)])]);
    let at_vertex = || json!([[0, 0], [5, 0], [10, 0], [10, 10], [5, 10], [5, -5]]);
    let rest: [Vertex; 4] = [(10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (5.0, 10.0, 0.0), (5.0, -5.0, 0.0)];
    let (s, p) = trimmed(at_vertex(), false, None, false, [7.0, 0.0]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (5.0, 0.0, 0.0)]), (false, false, &rest)]);
    let (s, p) = trimmed(at_vertex(), false, None, false, [3.0, 0.0]);
    expect(&s, p, &[(true, false, &[(5.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (5.0, 10.0, 0.0), (5.0, -5.0, 0.0)])]);
    // Straight on, or doubling back over itself: no cut at all.
    for (v, pick) in [(json!([[0, 0], [5, 0], [10, 0]]), [7.0, 0.0]), (json!([[0, 0], [10, 0], [5, 0]]), [2.0, 0.0])] {
        let mut s = Session::new();
        let p = pline(&mut s, v, false);
        let before = polys(&s);
        assert!(s.execute("trim", &json!({ "handle": p.hex(), "pick": pick })).is_err());
        assert_eq!(polys(&s), before);
    }
}

#[test]
fn a_bend_of_1e_8_cuts_a_polyline_and_a_smaller_one_doesnt() {
    // The bend at (5, 0), as the sine of its angle, whatever the segments' lengths.
    for (end, pick, cuts) in [
        ([10.0, 4e-8], [7.0, 0.0], false),
        ([10.0, 6e-8], [7.0, 0.0], true),
        ([5005.0, 1e-6], [700.0, 0.0], false),
        ([5005.0, 0.01], [700.0, 0.0], true),
    ] {
        let mut s = Session::new();
        let p = pline(&mut s, json!([[0, 0], [5, 0], end]), false);
        let r = s.execute("trim", &json!({ "handle": p.hex(), "pick": pick }));
        assert_eq!((r.is_ok(), polys(&s)[0].2.len()), if cuts { (true, 2) } else { (false, 3) }, "{end:?}");
    }
    // Two arcs of one circle, or a line and an arc on from it, cut where they meet.
    let q = 0.41421356237309503;
    let (s, p) = trimmed(json!([{ "p": [0, 0], "bulge": -q }, { "p": [5, 5], "bulge": -q }, [10, 0]]), false, None, false, [8.5355339, 3.5355339]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, -q), (5.0, 5.0, 0.0)])]);
    let (s, p) = trimmed(json!([[0, 0], { "p": [5, 0], "bulge": q }, [10, 5]]), false, None, false, [8.5355339, 1.4644661]);
    expect(&s, p, &[(true, false, &[(0.0, 0.0, 0.0), (5.0, 0.0, 0.0)])]);
}

#[test]
fn a_polyline_of_thousands_of_segments_is_cut_where_it_crosses_itself() {
    // 3,003 segments, the last crossing the first at (50, 0): the pick on the first takes it from
    // there to the vertex (100, 0), as with a few segments.
    let mut v = vec![json!([0, 0]), json!([100, 0]), json!([100, 20])];
    v.extend((1..=3000).map(|i| json!([100.0 - f64::from(i) / 60.0, 20.0 + f64::from(i % 2)])));
    v.push(json!([50, -10]));
    let (s, p) = trimmed(json!(v), false, None, false, [75.0, 0.0]);
    let got = polys(&s);
    assert_eq!(got.len(), 2, "{:?}", got.iter().map(|g| g.2.len()).collect::<Vec<_>>());
    assert!(got[0].0 == p && same(&got[0].2, &[(0.0, 0.0, 0.0), (50.0, 0.0, 0.0)]), "{:?}", got[0]);
    assert_eq!((got[1].2.len(), got[1].2.first().copied()), (3003, Some((100.0, 0.0, 0.0))));
}

#[test]
fn a_polyline_too_long_to_search_for_its_own_crossings_is_left_as_it_was() {
    // 4,100 segments in line: the run picked times those near it is past the bound searched. 300
    // segments in line over the same stretch, back and forth, crossed by 300 more: 90,000
    // crossings, past the bound kept. The trim is refused, and Quick mode doesn't erase the
    // polyline as one no edge crosses.
    let mut back_and_forth: Vec<[f64; 2]> = (0..=300).map(|i| [if i % 2 == 0 { 0.0 } else { 10.0 }, 0.0]).collect();
    back_and_forth.extend((0..300).map(|i| [0.5 + f64::from(i) * 0.03, if i % 2 == 0 { 5.0 } else { -5.0 }]));
    for (v, why) in [(json!((0..=4100).map(|i| [i, 0]).collect::<Vec<_>>()), "Too many segments"), (json!(back_and_forth), "Too many crossings")] {
        let mut s = Session::new();
        let p = pline(&mut s, v, false);
        let before = polys(&s);
        let r = s.execute("trim", &json!({ "handle": p.hex(), "pick": [0.2, 0], "mode": "quick" }));
        assert!(r.is_err_and(|e| e.to_string().contains(why)), "{why}");
        assert_eq!(polys(&s), before);
    }
}

#[test]
fn a_piece_whose_ends_only_come_near_stays_open_far_from_the_origin() {
    // 1e9 out, as survey coordinates in millimetres are: the piece left runs from the line's cut
    // to the polyline's end, 0.58 away, and stays open.
    let o = 1e9;
    let mut s = Session::new();
    let p = pline(&mut s, json!([[o + 2.3, 3.3], [o + 5.3, 8.0], [o + 8.5, 4.4], [o + 8.8, 7.8], [o + 7.4, 6.5]]), false);
    line(&mut s, [o + 6.9, 5.0], [o + 6.9, 7.0]);
    trim(&mut s, p, [o + 6.1, 7.1], None);
    let got = polys(&s);
    assert_eq!(got.len(), 2, "{got:?}");
    let (h, closed, vs) = &got[1];
    assert!(*h != p && !closed && vs.len() == 4, "{got:?}");
    let last = vs[3];
    assert!((last.0 - (o + 7.4)).abs() < 1e-6 && (last.1 - 6.5).abs() < 1e-6, "{last:?}");
}

#[test]
fn a_polyline_is_cut_where_a_vertex_of_it_touches_it() {
    // The vertex (6, 6) lies on the first segment, which is cut there.
    let v = json!([[0, 0], [10, 10], [12, 4], [6, 6], [4, 12]]);
    let mut s = Session::new();
    let p = pline(&mut s, v.clone(), false);
    trim(&mut s, p, [3.0, 3.0], None);
    expect(&s, p, &[(true, false, &[(6.0, 6.0, 0.0), (10.0, 10.0, 0.0), (12.0, 4.0, 0.0), (6.0, 6.0, 0.0), (4.0, 12.0, 0.0)])]);
    let mut s = Session::new();
    let p = pline(&mut s, v, false);
    trim(&mut s, p, [8.0, 8.0], None);
    expect(
        &s,
        p,
        &[
            (true, false, &[(0.0, 0.0, 0.0), (6.0, 6.0, 0.0)]),
            (false, false, &[(10.0, 10.0, 0.0), (12.0, 4.0, 0.0), (6.0, 6.0, 0.0), (4.0, 12.0, 0.0)]),
        ],
    );
}

#[test]
fn a_long_run_in_line_on_a_large_polyline_is_cut_where_it_is_crossed() {
    // 1,000 segments in line along y = 0, then 20,002 more, the last crossing the run at x = 500.25:
    // only the few segments near the run are searched, and the pick takes it from the crossing to
    // the corner (1000, 0).
    let mut v: Vec<Value> = (0..=1000).map(|i| json!([i, 0])).collect();
    v.push(json!([1000, 200]));
    v.extend((1..=20000).map(|i| json!([1000.0 - f64::from(i) * 499.75 / 20000.0, 200.0 + f64::from(i % 2)])));
    v.push(json!([500.25, -10]));
    let (s, p) = trimmed(json!(v), false, None, false, [999.5, 0.0]);
    let got = polys(&s);
    assert_eq!(got.len(), 2, "{:?}", got.iter().map(|g| g.2.len()).collect::<Vec<_>>());
    assert!(got[0].0 == p && got[0].2.len() == 502 && got[0].2.last() == Some(&(500.25, 0.0, 0.0)), "{:?}", got[0].2.last());
    assert_eq!((got[1].2.first().copied(), got[1].2.last().copied()), (Some((1000.0, 0.0, 0.0)), Some((500.25, -10.0, 0.0))));
}

#[test]
fn a_crossing_window_whose_nearest_point_is_a_vertex_trims_the_segment_it_ends() {
    // The window's first corner is nearest the vertex (10, 0), a cut of the polyline: the segment
    // before it goes, either way round the window, as in AutoCAD.
    for (a, b) in [([10.5, -0.5], [8.0, 2.0]), ([8.0, 2.0], [10.5, -0.5])] {
        let mut s = Session::new();
        let p = pline(&mut s, json!([[0, 0], [10, 0], [10, 10]]), false);
        s.execute("trim", &json!({ "crossing": [a, b], "mode": "standard" })).unwrap();
        expect(&s, p, &[(true, false, &[(10.0, 0.0, 0.0), (10.0, 10.0, 0.0)])]);
    }
}
