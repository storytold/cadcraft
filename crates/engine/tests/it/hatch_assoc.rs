//! Associative hatches follow their boundary objects (#451): MOVE, STRETCH and SCALE of a
//! boundary fill the hatch again; erasing an island lets the hatch fill it; erasing or opening
//! the outer boundary, or moving the hatch on its own, ends the link.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Hatch, HatchLoop};
use cadcraft_engine::geom::{Bounds2, PolyVertex, Polyline, Vec2, shoelace};
use serde_json::{Value, json};

fn hatch(s: &Session, h: &str) -> Hatch {
    let h = Handle::parse_hex(h).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn outline(l: &HatchLoop) -> Vec<Vec2> {
    Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(1e-4)
}

/// Filled area: outer loops minus islands.
fn area(h: &Hatch) -> f64 {
    h.loops.iter().map(|l| shoelace(&outline(l)).abs() * if l.outer { 1.0 } else { -1.0 }).sum()
}

fn bounds(h: &Hatch) -> Bounds2 {
    Bounds2::from_points(h.loops.iter().flat_map(outline))
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    s.execute("line", &json!({ "points": [a, b] })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

/// A 10 x 10 square of four separate lines, hatched by picking inside.
fn square_of_lines(s: &mut Session) -> (Vec<String>, String) {
    let ls = vec![
        line(s, [0.0, 0.0], [10.0, 0.0]),
        line(s, [10.0, 0.0], [10.0, 10.0]),
        line(s, [10.0, 10.0], [0.0, 10.0]),
        line(s, [0.0, 10.0], [0.0, 0.0]),
    ];
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap());
    (ls, h)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * (1.0 + b.abs())
}

#[test]
fn pick_point_hatch_follows_stretched_lines() {
    let mut s = Session::new();
    let (ls, h) = square_of_lines(&mut s);
    let hh = hatch(&s, &h);
    assert!(hh.associative);
    let mut src: Vec<String> = hh.loops[0].sources.iter().map(|h| h.hex()).collect();
    src.sort();
    let mut want = ls.clone();
    want.sort();
    assert_eq!(src, want, "the outer loop remembers its four lines");
    // Stretch the right edge 5 units to the right: the hatch fills 15 x 10.
    s.execute("stretch", &json!({ "window": [[8, -1], [12, 11]], "delta": [5, 0] })).unwrap();
    let hh = hatch(&s, &h);
    assert!(hh.associative);
    assert!(close(area(&hh), 150.0), "area {}", area(&hh));
    // Undo puts the hatch back with its boundary.
    s.execute("u", &json!({})).unwrap();
    assert!(close(area(&hatch(&s, &h)), 100.0));
}

#[test]
fn hatch_of_selected_polyline_follows_move_and_scale() {
    let mut s = Session::new();
    let r = handle(&s.execute("rectang", &json!({ "p1": [0, 0], "p2": [4, 2] })).unwrap());
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()] })).unwrap());
    s.execute("move", &json!({ "handles": [r.clone()], "delta": [100, 50] })).unwrap();
    let b = bounds(&hatch(&s, &h));
    assert!(b.min.dist(Vec2::new(100.0, 50.0)) < 1e-9 && b.max.dist(Vec2::new(104.0, 52.0)) < 1e-9, "{b:?}");
    s.execute("scale", &json!({ "handles": [r], "base": [100, 50], "factor": 2 })).unwrap();
    let hh = hatch(&s, &h);
    assert!(close(area(&hh), 32.0), "area {}", area(&hh));
    assert!(hh.associative);
}

#[test]
fn moving_hatch_with_its_boundary_keeps_the_link() {
    let mut s = Session::new();
    let (mut ls, h) = square_of_lines(&mut s);
    ls.push(h.clone());
    s.execute("move", &json!({ "handles": ls, "delta": [20, 0] })).unwrap();
    let hh = hatch(&s, &h);
    assert!(hh.associative);
    assert!(close(area(&hh), 100.0));
    assert!(bounds(&hh).min.dist(Vec2::new(20.0, 0.0)) < 1e-6);
}

#[test]
fn erased_island_is_filled_erased_boundary_ends_the_link() {
    let mut s = Session::new();
    let r = handle(&s.execute("rectang", &json!({ "p1": [0, 0], "p2": [20, 20] })).unwrap());
    let c = handle(&s.execute("circle", &json!({ "center": [10, 10], "radius": 2 })).unwrap());
    let h = handle(&s.execute("hatch", &json!({ "points": [[1, 1]] })).unwrap());
    assert_eq!(hatch(&s, &h).loops.len(), 2, "outer + circle island");
    s.execute("erase", &json!({ "handles": [c] })).unwrap();
    let hh = hatch(&s, &h);
    assert_eq!(hh.loops.len(), 1, "the island's area is hatched");
    assert!(hh.associative);
    assert!(close(area(&hh), 400.0));
    // Erasing the outer boundary keeps the hatch where it is, no longer associative.
    s.execute("erase", &json!({ "handles": [r] })).unwrap();
    let hh = hatch(&s, &h);
    assert!(!hh.associative);
    assert!(hh.loops.iter().all(|l| l.sources.is_empty()));
    assert!(close(area(&hh), 400.0));
    assert!(s.log.iter().any(|m| m.contains("no longer associative")), "{:?}", s.log);
}

#[test]
fn opening_the_boundary_ends_the_link() {
    let mut s = Session::new();
    let (ls, h) = square_of_lines(&mut s);
    // Pull the top edge away: the lines no longer enclose an area.
    s.execute("move", &json!({ "handles": [ls[2].clone()], "delta": [0, 5] })).unwrap();
    let hh = hatch(&s, &h);
    assert!(!hh.associative);
    assert!(close(area(&hh), 100.0), "the hatch keeps its shape");
}

#[test]
fn moving_or_copying_the_hatch_alone_ends_the_link() {
    let mut s = Session::new();
    let (_, h) = square_of_lines(&mut s);
    let n = s.doc().unwrap().model.len();
    s.execute("copy", &json!({ "handles": [h.clone()], "delta": [30, 0] })).unwrap();
    let copy = s.doc().unwrap().model.iter().skip(n).find(|e| matches!(e.kind, EntityKind::Hatch(_))).map(|e| e.handle.hex()).unwrap();
    assert!(!hatch(&s, &copy).associative, "a copy away from the boundary isn't associative");
    assert!(hatch(&s, &h).associative, "the original still is");
    s.execute("move", &json!({ "handles": [h.clone()], "delta": [0, 30] })).unwrap();
    assert!(!hatch(&s, &h).associative);
}

#[test]
fn non_associative_hatch_keeps_no_sources() {
    let mut s = Session::new();
    let r = handle(&s.execute("rectang", &json!({ "p1": [0, 0], "p2": [4, 2] })).unwrap());
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()], "associative": false })).unwrap());
    assert!(hatch(&s, &h).loops.iter().all(|l| l.sources.is_empty()));
    s.execute("move", &json!({ "handles": [r], "delta": [10, 0] })).unwrap();
    assert!(bounds(&hatch(&s, &h)).min.dist(Vec2::ZERO) < 1e-9, "stays put");
}

#[test]
fn hostile_sources_never_panic() {
    let mut s = Session::new();
    let r = handle(&s.execute("rectang", &json!({ "p1": [0, 0], "p2": [4, 2] })).unwrap());
    let rh = Handle::parse_hex(&r).unwrap();
    let sq = vec![PolyVertex::new(Vec2::ZERO), PolyVertex::new(Vec2::new(4.0, 0.0)), PolyVertex::new(Vec2::new(4.0, 2.0))];
    let nan = vec![PolyVertex::new(Vec2::new(f64::NAN, 0.0)); 3];
    let mut sources = vec![Handle(u64::MAX), Handle(0), rh];
    sources.extend((1..10_000).map(Handle));
    let k = EntityKind::Hatch(Hatch {
        pattern: "ANSI31".into(),
        solid: false,
        loops: vec![HatchLoop { vertices: sq, outer: true, sources: sources.clone() }, HatchLoop { vertices: nan, outer: false, sources }],
        scale: 1.0,
        angle: 0.0,
        associative: true,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    });
    s.add_entity(k).unwrap();
    s.execute("move", &json!({ "handles": [r], "delta": [1, 1] })).unwrap();
    s.execute("erase", &json!({ "handles": "all" })).ok();
}

#[test]
fn interactive_stretch_at_the_command_line_refills() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap());
    // Crossing-select the right edge and stretch it 5 to the right: the hatch fills 15 x 10.
    s.script("STRETCH\nC\n12,-1\n8,11\n\n0,0\n5,0\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let hh = hatch(&s, &h);
    assert!(hh.associative);
    assert!(close(area(&hh), 150.0), "area {}", area(&hh));
}
