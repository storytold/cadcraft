//! BREAK, BREAKATPOINT and TRIM change the object itself, as AutoCAD 2024 does: the piece from
//! the object's start, or the only piece left, keeps its handle and its place in the drawing
//! order, and the other pieces are new objects added at the end (`entlast`). Groups keep the
//! object, an associative hatch whose boundary no longer closes keeps its shape and its link
//! ends, and a dimension attached to the object stays on the point it measured, or follows the
//! object's new end where that point was cut off.
//!
//! The two chains, the group, the two hatches and the three dimensions on a line were measured in
//! AutoCAD 2024 (accoreconsole) on the same drawings: the objects in database order (`entnext`)
//! after each chain, a new handle marked `+`, and the dimensions' DIMASSOC references. The other
//! cases follow from those rules, or say what they check.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{AssocSnap, Common, DimAssoc, Dimension, EntityKind, Handle, Hatch, Line, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use serde_json::{Value, json};

fn first_handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

/// Adds the object `cmd` makes from `p`, on layer `name`.
fn add(s: &mut Session, name: &str, cmd: &str, p: Value) -> Handle {
    let h = first_handle(&s.execute(cmd, &p).unwrap());
    s.execute("properties.set", &json!({ "handles": [h.hex()], "layer": name })).unwrap();
    h
}

fn line(s: &mut Session, name: &str, a: [f64; 2], b: [f64; 2]) -> Handle {
    add(s, name, "line", json!({ "points": [a, b] }))
}

fn arc(s: &mut Session, name: &str, c: [f64; 2], start: f64, end: f64) -> Handle {
    add(s, name, "arc", json!({ "center": c, "radius": 10, "start": start, "end": end }))
}

fn pline(s: &mut Session, name: &str, vertices: Value, closed: bool) -> Handle {
    add(s, name, "pline", json!({ "vertices": vertices, "closed": closed }))
}

/// The drawing the chains were measured on, each object on a layer named after it, made in this
/// order.
fn drawing() -> Session {
    let mut s = Session::new();
    line(&mut s, "LA", [0.0, 50.0], [100.0, 50.0]);
    line(&mut s, "LB", [50.0, 0.0], [50.0, 100.0]);
    pline(&mut s, "LP", json!([[0, 200], [20, 200], [20, 220]]), false);
    line(&mut s, "LE", [200.0, 0.0], [210.0, 0.0]);
    line(&mut s, "LF", [210.0, 0.0], [220.0, 0.0]);
    line(&mut s, "LC", [0.0, 0.0], [100.0, 100.0]);
    line(&mut s, "LD", [0.0, 100.0], [100.0, 0.0]);
    arc(&mut s, "LR", [300.0, 0.0], 0.0, 180.0);
    add(&mut s, "LO", "circle", json!({ "center": [400, 0], "radius": 10 }));
    pline(&mut s, "LK", json!([[500, 0], [520, 0], [520, 20], [500, 20]]), true);
    line(&mut s, "LG", [600.0, 0.0], [600.0, 100.0]);
    line(&mut s, "LH", [590.0, 40.0], [610.0, 40.0]);
    line(&mut s, "LI", [590.0, 60.0], [610.0, 60.0]);
    line(&mut s, "LT", [650.0, 0.0], [650.0, 100.0]);
    line(&mut s, "LU", [640.0, 40.0], [660.0, 40.0]);
    line(&mut s, "LW", [395.0, -20.0], [395.0, 20.0]);
    line(&mut s, "LX", [10.0, 190.0], [10.0, 210.0]);
    line(&mut s, "LY", [15.0, 190.0], [15.0, 210.0]);
    arc(&mut s, "LJ", [700.0, 0.0], 0.0, 90.0);
    arc(&mut s, "LL", [700.0, 0.0], 90.0, 360.0);
    line(&mut s, "LM", [800.0, 0.0], [810.0, 0.0]);
    arc(&mut s, "LN", [810.0, 10.0], 270.0, 360.0);
    pline(&mut s, "LQ", json!([[900, 0], [910, 0]]), false);
    line(&mut s, "LS", [910.0, 0.0], [910.0, 10.0]);
    line(&mut s, "LZ", [1000.0, 0.0], [1010.0, 0.0]);
    s
}

/// `v` to `places` decimals without trailing zeros, as `rtos` printed the measurements.
fn num(v: f64, places: usize) -> String {
    let t = format!("{v:.places$}");
    let t = if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.') } else { &t };
    if t == "-0" { "0".into() } else { t.into() }
}

fn pt(p: Vec2) -> String {
    format!("{},{}", num(p.x, 4), num(p.y, 4))
}

/// An angle in degrees in [0, 360).
fn deg(a: f64) -> String {
    let d = a.to_degrees().rem_euclid(360.0);
    num(if d >= 360.0 - 5e-4 { 0.0 } else { d }, 3)
}

/// The model space objects in order as `layer[+]:TYPE:shape`, `+` marking a handle that wasn't in
/// `original`.
fn table(s: &Session, original: &[Handle]) -> Vec<String> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .map(|e| {
            let shape = match &e.kind {
                EntityKind::Line(l) => format!("LINE:{}>{}", pt(l.a.xy()), pt(l.b.xy())),
                EntityKind::Arc(a) => format!("ARC:c{}r{}a{}>{}", pt(a.center.xy()), num(a.radius, 4), deg(a.start), deg(a.end)),
                EntityKind::Circle(c) => format!("CIRCLE:c{}r{}", pt(c.center.xy()), num(c.radius, 4)),
                EntityKind::LwPolyline(p) => {
                    format!("LWPOLYLINE:f{}{}", u8::from(p.closed), p.vertices.iter().map(|v| format!(";{}", pt(v.p))).collect::<String>())
                }
                k => format!("{k:?}"),
            };
            let new = if original.contains(&e.handle) { "" } else { "+" };
            format!("{}{new}:{shape}", e.common.layer)
        })
        .collect()
}

fn handles(s: &Session) -> Vec<Handle> {
    s.doc().unwrap().model.iter().map(|e| e.handle).collect()
}

/// The object on layer `name` that was there first.
fn named(s: &Session, name: &str) -> Handle {
    s.doc().unwrap().model.iter().find(|e| e.common.layer == name).unwrap().handle
}

/// Runs `commands` on the command line one by one, each to its end.
fn run(s: &mut Session, commands: &[&str]) {
    for c in commands {
        s.script(&format!("{c}\n")).unwrap();
        assert!(s.running.is_none(), "{c}: {}", s.prompt_text());
    }
}

/// The objects AutoCAD leaves untouched in the middle of both tables.
const UNTOUCHED: [&str; 8] = [
    "LG:LINE:600,0>600,100",
    "LH:LINE:590,40>610,40",
    "LI:LINE:590,60>610,60",
    "LT:LINE:650,0>650,100",
    "LU:LINE:640,40>660,40",
    "LW:LINE:395,-20>395,20",
    "LX:LINE:10,190>10,210",
    "LY:LINE:15,190>15,210",
];

/// The measured `entnext` table after the chain of breaks below. (LL and LN end at 360°, written
/// 0° here.)
fn after_breaks() -> Vec<&'static str> {
    let mut v = vec![
        "LA:LINE:10,50>100,50",
        "LB:LINE:50,0>50,20",
        "LP:LWPOLYLINE:f0;0,200;10,200",
        "LE:LINE:200,0>210,0",
        "LF:LINE:210,0>220,0",
        "LC:LINE:0,0>100,100",
        "LD:LINE:0,100>100,0",
        "LR:ARC:c300,0r10a0>45",
        "LO:ARC:c400,0r10a90>0",
        "LK:LWPOLYLINE:f0;520,10;520,20;500,20;500,0;510,0",
    ];
    v.extend(UNTOUCHED);
    v.extend([
        "LJ:ARC:c700,0r10a0>45",
        "LL:ARC:c700,0r10a90>0",
        "LM:LINE:800,0>810,0",
        "LN:ARC:c810,10r10a270>0",
        "LQ:LWPOLYLINE:f0;900,0;910,0",
        "LS:LINE:910,0>910,10",
        "LZ:LINE:1000,0>1005,0",
        "LB+:LINE:50,30>50,100",
        "LR+:ARC:c300,0r10a135>180",
        "LP+:LWPOLYLINE:f0;20,210;20,220",
        "LZ+:LINE:1005,0>1010,0",
        "LJ+:ARC:c700,0r10a45>90",
    ]);
    v
}

#[test]
fn a_chain_of_breaks_leaves_the_objects_in_autocads_order() {
    let mut s = drawing();
    let original = handles(&s);
    run(
        &mut s,
        &[
            // A line picked nearer its end: the piece from its start still keeps the handle.
            "BREAK 50,30 50,20",
            // Off the start: one piece, the line itself.
            "BREAK 10,50 -10,50",
            "BREAK 307.0710678,7.0710678 292.9289322,7.0710678",
            // A circle becomes an arc with its handle.
            "BREAK 410,0 400,10",
            "BREAK 5,200 F 10,200 20,210",
            // A closed polyline opens with its handle.
            "BREAK 510,0 520,10",
        ],
    );
    // BREAKATPOINT, picking each object at the point it is broken at.
    for (name, at) in [("LZ", [1005.0, 0.0]), ("LJ", [707.0710678, 7.0710678])] {
        s.execute("breakatpoint", &json!({ "handle": named(&s, name).hex(), "at": at })).unwrap();
    }
    assert_eq!(table(&s, &original), after_breaks());
}

#[test]
fn a_chain_of_trims_leaves_the_objects_in_autocads_order() {
    let mut s = drawing();
    let original = handles(&s);
    // Quick mode: every object cuts. Each pick is between the measured cutting edges.
    run(&mut s, &["TRIM 600,50\n", "TRIM 650,80\n", "TRIM 50,10\n", "TRIM 390,0\n", "TRIM 12,200\n"]);
    let mut want = vec![
        "LA:LINE:0,50>100,50",
        "LB:LINE:50,50>50,100",
        "LP:LWPOLYLINE:f0;0,200;10,200",
        "LE:LINE:200,0>210,0",
        "LF:LINE:210,0>220,0",
        "LC:LINE:0,0>100,100",
        "LD:LINE:0,100>100,0",
        "LR:ARC:c300,0r10a0>180",
        "LO:ARC:c400,0r10a240>120",
        "LK:LWPOLYLINE:f1;500,0;520,0;520,20;500,20",
        "LG:LINE:600,0>600,40",
    ];
    want.extend(&UNTOUCHED[1..3]);
    want.push("LT:LINE:650,0>650,40");
    want.extend(&UNTOUCHED[4..]);
    want.extend([
        "LJ:ARC:c700,0r10a0>90",
        "LL:ARC:c700,0r10a90>0",
        "LM:LINE:800,0>810,0",
        "LN:ARC:c810,10r10a270>0",
        "LQ:LWPOLYLINE:f0;900,0;910,0",
        "LS:LINE:910,0>910,10",
        "LZ:LINE:1000,0>1010,0",
        "LG+:LINE:600,60>600,100",
        "LP+:LWPOLYLINE:f0;15,200;20,200;20,220",
    ]);
    assert_eq!(table(&s, &original), want);
}

#[test]
fn the_json_forms_return_the_kept_object_first() {
    let mut s = Session::new();
    let l = first_handle(&s.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap());
    let r = s.execute("break", &json!({ "handle": l.hex(), "p1": [20, 0], "p2": [30, 0] })).unwrap();
    let hs = r["handles"].as_array().unwrap();
    assert_eq!(hs.len(), 2);
    assert_eq!(hs[0], l.hex());
    assert_eq!(handles(&s), vec![l, Handle::parse_hex(hs[1].as_str().unwrap()).unwrap()]);
    let r = s.execute("breakatpoint", &json!({ "handle": l.hex(), "at": [10, 0] })).unwrap();
    assert_eq!(r["handles"][0], l.hex());
    // Undo puts the line back, whole, under its own handle.
    s.execute("u", &json!({})).unwrap();
    s.execute("u", &json!({})).unwrap();
    assert_eq!(table(&s, &[l]), ["0:LINE:0,0>100,0"]);
}

#[test]
fn the_new_piece_goes_to_the_space_of_the_object() {
    // A paper space line broken through the JSON form while model space is current.
    let mut s = Session::new();
    let paper = Space::Paper("Layout1".into());
    let line = EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 0.0, 0.0) });
    let l = s.doc_mut().unwrap().add(&paper, Common::default(), line).unwrap();
    s.execute("break", &json!({ "handle": l.hex(), "p1": [20, 0], "p2": [30, 0] })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.iter().count(), 0);
    let pieces: Vec<Handle> = d.space(&paper).unwrap().iter().map(|e| e.handle).collect();
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces[0], l);
}

#[test]
fn a_group_keeps_the_broken_object_and_not_the_new_piece() {
    let mut s = Session::new();
    let a = first_handle(&s.execute("line", &json!({ "points": [[0, 50], [100, 50]] })).unwrap());
    let b = first_handle(&s.execute("line", &json!({ "points": [[50, 0], [50, 100]] })).unwrap());
    s.execute("group", &json!({ "name": "G1", "handles": [a.hex(), b.hex()] })).unwrap();
    s.execute("break", &json!({ "handle": b.hex(), "p1": [50, 20], "p2": [50, 30] })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.iter().count(), 3);
    assert_eq!(d.groups.iter().find(|g| g.name == "G1").unwrap().members, vec![a, b]);
}

fn hatch(s: &Session, h: Handle) -> Hatch {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn loop_points(h: &Hatch) -> Vec<Vec<(f64, f64)>> {
    h.loops.iter().map(|l| l.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()).collect()
}

#[test]
fn a_hatch_whose_polyline_is_broken_open_keeps_its_shape_and_loses_its_link() {
    let mut s = Session::new();
    let k = first_handle(&s.execute("pline", &json!({ "vertices": [[500, 0], [520, 0], [520, 20], [500, 20]], "closed": true })).unwrap());
    let h = first_handle(&s.execute("hatch", &json!({ "handles": [k.hex()] })).unwrap());
    assert!(hatch(&s, h).associative);
    let square = loop_points(&hatch(&s, h));
    s.execute("break", &json!({ "handle": k.hex(), "p1": [510, 0], "p2": [520, 10] })).unwrap();
    assert!(matches!(&s.doc().unwrap().entity(k).unwrap().kind, EntityKind::LwPolyline(p) if !p.closed));
    let after = hatch(&s, h);
    assert!(!after.associative);
    assert_eq!(loop_points(&after), square);
    s.execute("u", &json!({})).unwrap();
    assert!(hatch(&s, h).associative);
}

#[test]
fn a_hatch_whose_boundary_line_is_trimmed_keeps_its_shape_and_loses_its_link() {
    let mut s = Session::new();
    let sides = [
        line(&mut s, "0", [500.0, 0.0], [520.0, 0.0]),
        line(&mut s, "0", [520.0, 0.0], [520.0, 20.0]),
        line(&mut s, "0", [520.0, 20.0], [500.0, 20.0]),
        line(&mut s, "0", [500.0, 20.0], [500.0, 0.0]),
    ];
    let h = first_handle(&s.execute("hatch", &json!({ "points": [[510, 10]] })).unwrap());
    assert!(hatch(&s, h).associative);
    let square = loop_points(&hatch(&s, h));
    let k1 = line(&mut s, "0", [505.0, -5.0], [505.0, 5.0]);
    let k2 = line(&mut s, "0", [515.0, -5.0], [515.0, 5.0]);
    let before = handles(&s);
    s.execute("trim", &json!({ "handle": sides[0].hex(), "pick": [510, 0], "edges": [k1.hex(), k2.hex()] })).unwrap();
    let after = handles(&s);
    assert_eq!(after[..before.len()], before[..]);
    assert_eq!(after.len(), before.len() + 1);
    let hh = hatch(&s, h);
    assert!(!hh.associative);
    assert_eq!(loop_points(&hh), square);
}

fn dim(s: &Session, h: Handle) -> Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        k => panic!("not a dimension: {k:?}"),
    }
}

fn attachments(d: &Dimension) -> Vec<(String, Handle, AssocSnap)> {
    let mut v: Vec<(String, Handle, AssocSnap)> = d.assoc.iter().map(|a: &DimAssoc| (a.point.clone(), a.handle, a.snap.clone())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn origins(d: &Dimension) -> [(f64, f64); 2] {
    [(d.p13.x, d.p13.y), (d.p14.x, d.p14.y)]
}

/// A vertical line from (x, 0) to (x, 100) with a linear dimension attached to both its ends.
fn dimensioned_line(s: &mut Session, x: f64) -> (Handle, Handle) {
    let l = first_handle(&s.execute("line", &json!({ "points": [[x, 0], [x, 100]] })).unwrap());
    let d = first_handle(&s.execute("dimlinear", &json!({ "p1": [x, 0], "p2": [x, 100], "at": [x + 20.0, 50] })).unwrap());
    assert_eq!(attachments(&dim(s, d)), [("p13".into(), l, AssocSnap::Start), ("p14".into(), l, AssocSnap::End)]);
    (l, d)
}

#[test]
fn a_dimension_across_a_line_broken_in_its_middle_stays_on_both_ends() {
    // Measured: the reference to the line's end moves to the new piece, the dimension is unchanged.
    let mut s = Session::new();
    let (l, d) = dimensioned_line(&mut s, 50.0);
    run(&mut s, &["BREAK 50,20 50,30"]);
    let piece = *handles(&s).last().unwrap();
    assert!(piece != l && piece != d);
    let dm = dim(&s, d);
    assert_eq!(attachments(&dm), [("p13".into(), l, AssocSnap::Start), ("p14".into(), piece, AssocSnap::End)]);
    assert_eq!(origins(&dm), [(50.0, 0.0), (50.0, 100.0)]);
    s.execute("u", &json!({})).unwrap();
    assert_eq!(attachments(&dim(&s, d)), [("p13".into(), l, AssocSnap::Start), ("p14".into(), l, AssocSnap::End)]);
}

#[test]
fn a_dimension_on_a_trimmed_off_end_follows_the_new_end() {
    // Measured: the references stay on the line, the dimension measures 40 instead of 100.
    let mut s = Session::new();
    let (l, d) = dimensioned_line(&mut s, 650.0);
    let edge = first_handle(&s.execute("line", &json!({ "points": [[640, 40], [660, 40]] })).unwrap());
    s.execute("trim", &json!({ "handle": l.hex(), "pick": [650, 80], "edges": [edge.hex()] })).unwrap();
    let dm = dim(&s, d);
    assert_eq!(attachments(&dm), [("p13".into(), l, AssocSnap::Start), ("p14".into(), l, AssocSnap::End)]);
    assert_eq!(origins(&dm), [(650.0, 0.0), (650.0, 40.0)]);
}

#[test]
fn a_dimension_on_a_broken_off_start_follows_the_new_start() {
    // Measured: BREAK from the start to (650, 20); the dimension starts at (650, 20).
    let mut s = Session::new();
    let (l, d) = dimensioned_line(&mut s, 650.0);
    run(&mut s, &["BREAK 650,10 F 650,0 650,20"]);
    assert_eq!(handles(&s), [l, d]);
    let dm = dim(&s, d);
    assert_eq!(attachments(&dm), [("p13".into(), l, AssocSnap::Start), ("p14".into(), l, AssocSnap::End)]);
    assert_eq!(origins(&dm), [(650.0, 20.0), (650.0, 100.0)]);
}

#[test]
fn a_dimension_follows_a_piece_a_later_pick_of_the_same_trim_shortens() {
    // The measured rules pick by pick: the first pick moves the reference to the line's end onto
    // the new piece, the second cuts that end off, so the dimension ends at the piece's new end.
    let mut s = Session::new();
    let other = first_handle(&s.execute("line", &json!({ "points": [[0, 20], [0, 30]] })).unwrap());
    let l = first_handle(&s.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap());
    for x in [40, 60, 80] {
        s.execute("line", &json!({ "points": [[x, -5], [x, 5]] })).unwrap();
    }
    let d = first_handle(&s.execute("dimlinear", &json!({ "p1": [0, 30], "p2": [100, 0], "at": [50, 60] })).unwrap());
    assert_eq!(attachments(&dim(&s, d)), [("p13".into(), other, AssocSnap::End), ("p14".into(), l, AssocSnap::End)]);
    s.script("TRIM 50,0 90,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let piece = *handles(&s).last().unwrap();
    assert_eq!(table(&s, &[l]).last().unwrap(), "0+:LINE:60,0>80,0");
    let dm = dim(&s, d);
    assert_eq!(attachments(&dm), [("p13".into(), other, AssocSnap::End), ("p14".into(), piece, AssocSnap::End)]);
    assert_eq!(origins(&dm), [(0.0, 30.0), (80.0, 0.0)]);
}

#[test]
fn a_dimension_on_a_closed_polyline_opened_by_break_keeps_its_vertices() {
    // The vertices it measured are still vertices of the polyline, numbered from the break.
    let mut s = Session::new();
    let k = first_handle(&s.execute("pline", &json!({ "vertices": [[500, 0], [520, 0], [520, 20], [500, 20]], "closed": true })).unwrap());
    let d = first_handle(&s.execute("dimlinear", &json!({ "p1": [500, 20], "p2": [520, 20], "at": [510, 40] })).unwrap());
    assert_eq!(attachments(&dim(&s, d)), [("p13".into(), k, AssocSnap::Vertex { index: 3 }), ("p14".into(), k, AssocSnap::Vertex { index: 2 })]);
    s.execute("break", &json!({ "handle": k.hex(), "p1": [510, 0], "p2": [520, 10] })).unwrap();
    let dm = dim(&s, d);
    assert_eq!(attachments(&dm), [("p13".into(), k, AssocSnap::Vertex { index: 2 }), ("p14".into(), k, AssocSnap::Vertex { index: 1 })]);
    assert_eq!(origins(&dm), [(500.0, 20.0), (520.0, 20.0)]);
    // Still attached: moving the polyline takes the dimension along.
    s.execute("move", &json!({ "handles": [k.hex()], "delta": [0, 10] })).unwrap();
    assert_eq!(origins(&dim(&s, d)), [(500.0, 30.0), (520.0, 30.0)]);
}

#[test]
fn constraints_on_a_broken_object_go_and_leave_the_others_where_they_are() {
    let mut s = Session::new();
    let a = first_handle(&s.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap());
    let b = first_handle(&s.execute("line", &json!({ "points": [[100, 0], [100, 50]] })).unwrap());
    s.execute("geomconstraint", &json!({ "type": "coincident", "h1": a.hex(), "p1": "end", "h2": b.hex(), "p2": "start" })).unwrap();
    s.execute("break", &json!({ "handle": a.hex(), "p1": [40, 0], "p2": [60, 0] })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.constraints.is_empty());
    let original = [a, b];
    assert_eq!(table(&s, &original), ["0:LINE:0,0>40,0", "0:LINE:100,0>100,50", "0+:LINE:60,0>100,0"]);
}
