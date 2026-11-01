//! TRIM and EXTEND options (#397): Quick and Standard modes (TRIMEXTENDMODE), cuTting/Boundary
//! edges, Fence, Crossing, Edge (EDGEMODE), Project (PROJMODE), eRase and Undo, and the JSON
//! forms' `fence`, `crossing`, `edgeMode` and `mode`.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

type Seg = ((f64, f64), (f64, f64));

fn r(v: f64) -> f64 {
    (v * 1e6).round() / 1e6 + 0.0
}

/// Every line with its ends in a fixed order (rounded), sorted.
fn lines(s: &Session) -> Vec<Seg> {
    let mut v: Vec<Seg> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(((r(l.a.x), r(l.a.y)), (r(l.b.x), r(l.b.y)))) } else { None })
        .map(|(a, b)| if (a.0, a.1) <= (b.0, b.1) { (a, b) } else { (b, a) })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn session(ls: &[&str]) -> Session {
    let mut s = Session::new();
    for l in ls {
        s.cmdline(&format!("LINE {l}")).unwrap();
        s.cmdline("").unwrap();
    }
    s
}

/// Two vertical edges at x = 0 and x = 10, crossed by horizontal lines at y = 0 and y = 2.
fn ladder() -> Session {
    session(&["0,-5 0,5", "10,-5 10,5", "-5,0 15,0", "-5,2 15,2"])
}

const EDGES: [Seg; 2] = [((0.0, -5.0), (0.0, 5.0)), ((10.0, -5.0), (10.0, 5.0))];

fn with_edges(more: &[Seg]) -> Vec<Seg> {
    let mut v: Vec<Seg> = EDGES.iter().chain(more).copied().collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn header(s: &Session, name: &str) -> i64 {
    s.doc().unwrap().header.i64(name, -1)
}

#[test]
fn quick_mode_trims_to_all_objects_and_erases_an_object_with_no_edge() {
    let mut s = ladder();
    s.cmdline("LINE 20,20 25,20").unwrap();
    s.cmdline("").unwrap();
    s.script("TRIM 5,0 22,20\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (15.0, 2.0)), ((10.0, 0.0), (15.0, 0.0))]));
}

#[test]
fn standard_mode_uses_the_selected_cutting_edges() {
    let mut s = ladder();
    // Mode Standard, cutting edge: the line at x = 0 only.
    s.script("TRIM O S 0,4\n\n5,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(header(&s, "TRIMEXTENDMODE"), 0);
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (15.0, 2.0))]));
    // Standard mode is remembered: the next TRIM starts by asking for the edges; Enter selects
    // all, and an object no edge crosses is kept.
    s.cmdline("LINE 20,20 25,20").unwrap();
    s.cmdline("").unwrap();
    s.script("TRIM\n\n22,20\n\n").unwrap();
    assert!(lines(&s).contains(&((20.0, 20.0), (25.0, 20.0))));
}

#[test]
fn cutting_edges_option_in_quick_mode() {
    let mut s = ladder();
    s.script("TRIM T 10,4\n\n5,2\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(header(&s, "TRIMEXTENDMODE"), -1, "cuTting edges leaves the mode alone");
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (15.0, 0.0)), ((10.0, 2.0), (15.0, 2.0))]));
}

#[test]
fn fence_trims_every_crossed_object() {
    let mut s = ladder();
    s.script("TRIM F 5,-1 5,3\n\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(
        lines(&s),
        with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (0.0, 2.0)), ((10.0, 0.0), (15.0, 0.0)), ((10.0, 2.0), (15.0, 2.0))])
    );
}

#[test]
fn fence_crossing_one_line_twice_trims_both_parts() {
    let mut s = ladder();
    // A fence round the top crosses y = 2 left of x = 0 and right of x = 10.
    s.script("TRIM F -3,1 -3,6 13,6 13,1\n\n\n").unwrap();
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (15.0, 0.0)), ((0.0, 2.0), (10.0, 2.0))]));
}

#[test]
fn crossing_window_trims_what_it_touches() {
    let mut s = ladder();
    s.script("TRIM C 4,-1 6,1\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (15.0, 2.0)), ((10.0, 0.0), (15.0, 0.0))]));
    // A pick in empty space opens a crossing window too.
    let mut s = ladder();
    s.script("TRIM 4,-4 6,1\n\n").unwrap();
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (15.0, 2.0)), ((10.0, 0.0), (15.0, 0.0))]));
}

#[test]
fn edge_extend_trims_to_an_implied_edge() {
    // A short cutting edge at x = 5 that doesn't reach y = 0.
    let ls = ["5,3 5,4", "-5,0 15,0"];
    let mut s = session(&ls);
    s.script("TRIM O S 5,3.5\n\n8,0\n\n").unwrap();
    assert_eq!(lines(&s), vec![((-5.0, 0.0), (15.0, 0.0)), ((5.0, 3.0), (5.0, 4.0))], "no implied edge: nothing to trim");
    let mut s = session(&ls);
    s.script("TRIM O S 5,3.5\n\nE E 8,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(header(&s, "EDGEMODE"), 1);
    assert_eq!(lines(&s), vec![((-5.0, 0.0), (5.0, 0.0)), ((5.0, 3.0), (5.0, 4.0))]);
    // EXTEND to an implied edge.
    let mut s = session(&["5,3 5,4", "0,0 2,0"]);
    s.script("EXTEND O S 5,3.5\n\nE E 1.8,0\n\n").unwrap();
    assert_eq!(lines(&s), vec![((0.0, 0.0), (5.0, 0.0)), ((5.0, 3.0), (5.0, 4.0))]);
}

#[test]
fn erase_undo_and_project() {
    let mut s = ladder();
    s.script("TRIM R 5,2\n\nP V\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (15.0, 0.0))]));
    assert_eq!(header(&s, "PROJMODE"), 2);
    // Undo takes back the last trim only, then the erase.
    let mut s = ladder();
    s.script("TRIM 5,0 5,2 U\n\n").unwrap();
    assert_eq!(lines(&s), with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (15.0, 2.0)), ((10.0, 0.0), (15.0, 0.0))]));
    s.script("TRIM R 10,4\n\nU U\n\n").unwrap();
    assert_eq!(lines(&s).len(), 5);
    // The whole TRIM is one undo step.
    s.script("TRIM 5,2 F 5,-1 5,1\n\n\n").unwrap();
    s.undo().unwrap();
    assert_eq!(lines(&s).len(), 5);
}

#[test]
fn extend_fence_and_crossing() {
    let mut s = session(&["10,-5 10,5", "0,0 4,0", "0,2 4,2"]);
    s.script("EXTEND F 3,-1 3,3\n\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 0.0)), ((0.0, 2.0), (10.0, 2.0)), ((10.0, -5.0), (10.0, 5.0))]);
    let mut s = session(&["10,-5 10,5", "0,0 4,0", "0,2 4,2"]);
    s.script("EXTEND C 3,-1 5,1\n\n").unwrap();
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 0.0)), ((0.0, 2.0), (4.0, 2.0)), ((10.0, -5.0), (10.0, 5.0))]);
}

#[test]
fn json_forms_take_fence_crossing_edge_mode_and_mode() {
    let mut s = ladder();
    let r = s.execute("trim", &json!({ "fence": [[5, -1], [5, 3]] })).unwrap();
    assert_eq!(r["trimmed"], 2);
    assert_eq!(lines(&s).len(), 6);

    let mut s = ladder();
    let r = s.execute("trim", &json!({ "crossing": [[4, -1], [6, 1]] })).unwrap();
    assert_eq!(r["trimmed"], 1);

    let mut s = session(&["5,3 5,4", "-5,0 15,0"]);
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    assert!(s.execute("trim", &json!({ "handle": h, "pick": [8, 0] })).is_err());
    s.execute("trim", &json!({ "handle": h, "pick": [8, 0], "edgeMode": "extend" })).unwrap();
    assert_eq!(lines(&s), vec![((-5.0, 0.0), (5.0, 0.0)), ((5.0, 3.0), (5.0, 4.0))]);

    let mut s = session(&["20,20 25,20"]);
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    let r = s.execute("trim", &json!({ "handle": h, "pick": [22, 20], "mode": "quick" })).unwrap();
    assert_eq!(r["handles"], json!([]));
    assert!(lines(&s).is_empty());

    let mut s = ladder();
    assert!(s.execute("trim", &json!({ "fence": [[5, -1]] })).is_err());
    assert!(s.execute("trim", &json!({ "crossing": [[5, -1], [1, 1], [2, 2]] })).is_err());
    assert!(s.execute("trim", &json!({ "fence": [[5, -1], [5, 3]], "edgeMode": "sideways" })).is_err());
    assert!(s.execute("trim", &json!({ "fence": [[5, -1], [5, 3]], "mode": 3 })).is_err());
    assert_eq!(lines(&s).len(), 4);

    let mut s = session(&["10,-5 10,5", "0,0 4,0"]);
    let r = s.execute("extend", &json!({ "fence": [[3, -1], [3, 1]] })).unwrap();
    assert_eq!(r["extended"], 1);
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 0.0)), ((10.0, -5.0), (10.0, 5.0))]);
}

/// The handle of the line with these ends.
fn handle_of(s: &Session, seg: Seg) -> cadcraft_engine::doc::Handle {
    s.doc()
        .unwrap()
        .model
        .iter()
        .find(|e| matches!(&e.kind, EntityKind::Line(l) if ((r(l.a.x), r(l.a.y)), (r(l.b.x), r(l.b.y))) == seg))
        .unwrap()
        .handle
}

#[test]
fn objects_selected_before_trim_are_its_cutting_edges_and_stay_highlighted() {
    let mut s = ladder();
    let edge = handle_of(&s, ((0.0, -5.0), (0.0, 5.0)));
    s.set_selection(vec![edge]);
    s.cmdline("TRIM").unwrap();
    assert_eq!(s.selection(), [edge], "the edge stays selected (highlighted) while TRIM runs");
    // Only the x = 0 line cuts: the y = 0 line loses its left part, the x = 10 line doesn't cut.
    s.script("-2,0\n").unwrap();
    assert_eq!(s.selection(), [edge]);
    assert!(lines(&s).contains(&((0.0, 0.0), (15.0, 0.0))), "{:?}", lines(&s));
    s.script("\n").unwrap();
    assert!(s.running.is_none());
    assert!(s.selection().is_empty(), "the highlight ends with the command");
    // Cancelling ends it too.
    s.set_selection(vec![edge]);
    s.cmdline("EXTEND").unwrap();
    assert_eq!(s.selection(), [edge]);
    s.cancel();
    assert!(s.selection().is_empty());
}

#[test]
fn a_pick_in_empty_space_draws_a_fence_line_and_trims_what_it_crosses() {
    let mut s = ladder();
    // Quick mode. From empty space at 5,-3 to 5,3: both horizontal lines are cut between the edges.
    s.script("TRIM 5,-3\n").unwrap();
    assert_eq!(s.current_prompt().map(|p| p.message), Some("Specify end point of the fence line".into()));
    s.script("5,3\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(
        lines(&s),
        with_edges(&[((-5.0, 0.0), (0.0, 0.0)), ((-5.0, 2.0), (0.0, 2.0)), ((10.0, 0.0), (15.0, 0.0)), ((10.0, 2.0), (15.0, 2.0))])
    );
}

/// The glow under the cursor: its polylines' points (rounded).
fn glow_points(s: &Session, at: (f64, f64)) -> Option<(Vec<Vec<(f64, f64)>>, bool)> {
    let (ents, removing) = s.glow(cadcraft_engine::geom::Vec2::new(at.0, at.1))?;
    let pts = ents
        .iter()
        .filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.vertices.iter().map(|v| (r(v.p.x), r(v.p.y))).collect()) } else { None })
        .collect();
    Some((pts, removing))
}

#[test]
fn trim_glows_the_part_a_pick_takes_away() {
    let mut s = ladder();
    s.cmdline("TRIM").unwrap();
    let (pts, removing) = glow_points(&s, (5.0, 0.0)).expect("a glow over the line");
    assert!(removing);
    assert_eq!(pts.len(), 1);
    let run = &pts[0];
    let (lo, hi) = run.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    assert!((lo - 0.0).abs() < 1e-3 && (hi - 10.0).abs() < 1e-3, "{lo} {hi}");
    assert!(run.iter().all(|p| p.1 == 0.0));
    // Nothing under the cursor: no glow. Nothing changes in the drawing.
    assert!(s.glow(cadcraft_engine::geom::Vec2::new(5.0, 30.0)).is_none());
    assert_eq!(lines(&s).len(), 4);
}

#[test]
fn extend_glows_the_part_a_pick_adds() {
    let mut s = session(&["0,0 5,0", "10,-5 10,5"]);
    s.cmdline("EXTEND").unwrap();
    let (pts, removing) = glow_points(&s, (4.0, 0.0)).expect("a glow over the line");
    assert!(!removing);
    let (lo, hi) = pts.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    assert!((lo - 5.0).abs() < 1e-2 && (hi - 10.0).abs() < 1e-3, "{lo} {hi}");
}

#[test]
fn trim_cuts_a_hatch_area() {
    use cadcraft_engine::doc::{Common, Hatch, HatchLoop, Space};
    use cadcraft_engine::geom::{PolyVertex, Vec2};
    let mut s = session(&["4,-5 4,15"]);
    let vertices = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect();
    let hatch = Hatch {
        pattern: "SOLID".into(),
        solid: true,
        loops: vec![HatchLoop { vertices, outer: true, sources: Vec::new() }],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    };
    let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Hatch(hatch)).unwrap();
    s.execute("trim", &json!({ "handle": h.hex(), "pick": [2.0, 5.0] })).unwrap();
    let left: Vec<_> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).collect();
    assert_eq!(left.len(), 1);
    let xs: Vec<f64> = left[0].loops.iter().flat_map(|l| l.vertices.iter().map(|v| v.p.x)).collect();
    assert!(xs.iter().all(|x| *x >= 4.0 - 1e-6), "{xs:?}");
    assert!(xs.iter().any(|x| (*x - 10.0).abs() < 1e-6));
}

#[test]
fn lines_inside_a_block_cut_a_hatch() {
    use cadcraft_engine::doc::{Common, Hatch, HatchLoop, Space};
    use cadcraft_engine::geom::{PolyVertex, Vec2};
    let mut s = Session::new();
    // A block whose line crosses the hatch at x = 4.
    let l = s.execute("line", &json!({ "points": [[4, -5], [4, 15]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("block", &json!({ "name": "DIVIDER", "base": [0, 0], "handles": [l], "keep": "convert" })).unwrap();
    let vertices = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect();
    let hatch = Hatch {
        pattern: "SOLID".into(),
        solid: true,
        loops: vec![HatchLoop { vertices, outer: true, sources: Vec::new() }],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    };
    let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Hatch(hatch)).unwrap();
    // Quick mode: a hatch no edge crossed would be erased.
    s.execute("trim", &json!({ "handle": h.hex(), "pick": [2.0, 5.0], "mode": "quick" })).unwrap();
    let left: Vec<_> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).collect();
    assert_eq!(left.len(), 1, "trimmed, not erased");
    assert!(left[0].loops.iter().flat_map(|l| l.vertices.iter()).all(|v| v.p.x >= 4.0 - 1e-6));
}
