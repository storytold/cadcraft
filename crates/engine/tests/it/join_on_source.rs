//! JOIN onto the first object picked, the source, as AutoCAD 2024 runs it (accoreconsole,
//! `entget` of every object after `(command "_.JOIN" source other ... "")`, on these drawings):
//!
//! - lines on the source's line make it one longer line, gaps and overlaps bridged, keeping its
//!   handle and the way it runs;
//! - arcs on the source's circle join counterclockwise from its start to the farthest end, keeping
//!   its handle; arcs that reach round to its start make a circle, a new object;
//! - an open polyline source takes lines, arcs and polylines at its ends and keeps its handle, its
//!   own vertices exactly, its elevation and linetype generation.

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

/// The model space objects in order: handle and kind.
fn objects(s: &Session) -> Vec<(Handle, EntityKind)> {
    s.doc().unwrap().model.iter().map(|e| (e.handle, e.kind.clone())).collect()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    handle(&s.execute("line", &json!({ "points": [a, b] })).unwrap())
}

/// An arc round `c` from `a1` to `a2` degrees.
fn arc(s: &mut Session, c: [f64; 2], r: f64, a1: f64, a2: f64) -> Handle {
    handle(&s.execute("arc", &json!({ "center": c, "radius": r, "start": a1, "end": a2 })).unwrap())
}

/// An open polyline from (x, y, start width, end width, bulge) vertices.
fn pline(s: &mut Session, vs: &[(f64, f64, f64, f64, f64)]) -> Handle {
    let vs: Vec<Value> = vs.iter().map(|&(x, y, w0, w1, b)| json!({ "p": [x, y], "startWidth": w0, "endWidth": w1, "bulge": b })).collect();
    handle(&s.execute("pline", &json!({ "vertices": vs })).unwrap())
}

/// Puts the line `h` at height `z`.
fn raise(s: &mut Session, h: Handle, z: f64) {
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                (l.a.z, l.b.z) = (z, z);
            }
        })
        .unwrap();
}

fn join(s: &mut Session, hs: &[Handle]) -> Handle {
    let hs: Vec<String> = hs.iter().map(|h| h.hex()).collect();
    handle(&s.execute("join", &json!({ "handles": hs })).unwrap())
}

/// Whether two lengths agree to 1e-9 of the size of the numbers.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

/// Whether two angles in radians agree to 1e-9, a whole turn apart alike.
fn same_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(std::f64::consts::TAU);
    d < 1e-9 || std::f64::consts::TAU - d < 1e-9
}

#[test]
fn lines_on_the_source_line_make_it_one_line_with_its_handle() {
    // (others, AutoCAD's line) for the source from (0, 0) to (10, 0): behind its start, ahead of
    // its end the other way round, both sides, overlapping and inside it.
    let cases: [(&[[[f64; 2]; 2]], [[f64; 2]; 2]); 5] = [
        (&[[[-10.0, 0.0], [-5.0, 0.0]]], [[-10.0, 0.0], [10.0, 0.0]]),
        (&[[[-5.0, 0.0], [3.0, 0.0]]], [[-5.0, 0.0], [10.0, 0.0]]),
        (&[[[20.0, 0.0], [15.0, 0.0]]], [[0.0, 0.0], [20.0, 0.0]]),
        (&[[[-6.0, 0.0], [-2.0, 0.0]], [[14.0, 0.0], [18.0, 0.0]]], [[-6.0, 0.0], [18.0, 0.0]]),
        (&[[[2.0, 0.0], [5.0, 0.0]]], [[0.0, 0.0], [10.0, 0.0]]),
    ];
    let mut all: Vec<([[f64; 2]; 2], Vec<[[f64; 2]; 2]>, [[f64; 2]; 2])> =
        cases.iter().map(|(others, want)| ([[0.0, 0.0], [10.0, 0.0]], others.to_vec(), *want)).collect();
    // A gap, an overlap and survey coordinates a hair off each other's line (the source's own
    // drawings, AutoCAD's results).
    all.push((
        [[0.0, 0.0], [-22.430628364140432, 19.921518797265904]],
        vec![[[-26.169066424830504, 23.241771930143557], [-44.861256728280864, 39.84303759453181]]],
        [[0.0, 0.0], [-44.86125672828086, 39.8430375945318]],
    ));
    all.push((
        [[0.0, 0.0], [25.55949563535469, -15.707074293651399]],
        vec![[[21.29957969612891, -13.089228578042833], [51.11899127070938, -31.414148587302797]]],
        [[0.0, 0.0], [51.11899127070937, -31.41414858730279]],
    ));
    all.push((
        [[612345.678, 2398765.432], [612375.0989113144, 2398771.2980018267]],
        vec![[[612375.099009384, 2398771.29802138], [612404.5198226288, 2398777.1640036535]]],
        [[612345.678, 2398765.432], [612404.5198226288, 2398777.164003653]],
    ));
    for (src, others, want) in all {
        let mut s = Session::new();
        let a = line(&mut s, src[0], src[1]);
        let mut hs = vec![a];
        hs.extend(others.iter().map(|o| line(&mut s, o[0], o[1])));
        assert_eq!(join(&mut s, &hs), a, "{others:?}");
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "{others:?}: {objs:?}");
        let EntityKind::Line(l) = &objs[0].1 else { panic!("not a line: {objs:?}") };
        let got = [[l.a.x, l.a.y], [l.b.x, l.b.y]];
        assert!(objs[0].0 == a && got.iter().flatten().zip(want.iter().flatten()).all(|(g, w)| near(*g, *w)), "{others:?}: {got:?}, not {want:?}");
        // One undo brings them all back as they were.
        s.undo().unwrap();
        assert_eq!(objects(&s).len(), 1 + others.len());
        let EntityKind::Line(l) = &s.doc().unwrap().entity(a).unwrap().kind else { panic!("not a line") };
        assert_eq!([[l.a.x, l.a.y], [l.b.x, l.b.y]], src);
    }
}

#[test]
fn arcs_on_the_source_circle_join_counterclockwise_from_its_start() {
    // (source, others, AutoCAD's end angle) in degrees, radius 5 round (0, 0): a gap, others
    // before the source with the larger and the smaller gap, an overlap, three, one inside, one
    // across 0.
    let cases: [((f64, f64), &[(f64, f64)], f64); 7] = [
        ((0.0, 100.0), &[(130.0, 250.0)], 250.0),
        ((100.0, 200.0), &[(0.0, 50.0)], 50.0),
        ((100.0, 200.0), &[(300.0, 90.0)], 90.0),
        ((0.0, 100.0), &[(50.0, 150.0)], 150.0),
        ((0.0, 60.0), &[(200.0, 250.0), (100.0, 150.0)], 250.0),
        ((0.0, 100.0), &[(20.0, 50.0)], 100.0),
        ((30.0, 100.0), &[(300.0, 20.0)], 20.0),
    ];
    for ((a1, a2), others, end) in cases {
        let mut s = Session::new();
        let a = arc(&mut s, [0.0, 0.0], 5.0, a1, a2);
        let mut hs = vec![a];
        hs.extend(others.iter().map(|(b1, b2)| arc(&mut s, [0.0, 0.0], 5.0, *b1, *b2)));
        assert_eq!(join(&mut s, &hs), a, "{a1}-{a2} {others:?}");
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "{objs:?}");
        let EntityKind::Arc(g) = &objs[0].1 else { panic!("not an arc: {objs:?}") };
        assert!(
            objs[0].0 == a && g.radius == 5.0 && same_angle(g.start, a1.to_radians()) && same_angle(g.end, end.to_radians()),
            "{a1}-{a2} {others:?}: {g:?}, not to {end}"
        );
    }
}

#[test]
fn a_source_nothing_reaches_past_stays_exactly_as_it_was() {
    // AutoCAD 2024 leaves a line or an arc with only others inside it unchanged. These ends, worked
    // out again from the start along the source, would come out a hair off.
    let mut s = Session::new();
    let a = line(&mut s, [-36.6, 34.7], [26.4, -24.5]);
    let (p, q) = ([-36.6 + 63.0 * 0.25, 34.7 - 59.2 * 0.25], [-36.6 + 63.0 * 0.5, 34.7 - 59.2 * 0.5]);
    let b = line(&mut s, p, q);
    join(&mut s, &[a, b]);
    let EntityKind::Line(l) = &s.doc().unwrap().entity(a).unwrap().kind else { panic!("not a line") };
    assert_eq!([l.a.x, l.a.y, l.b.x, l.b.y], [-36.6, 34.7, 26.4, -24.5]);
    let mut s = Session::new();
    let a = arc(&mut s, [0.0, 0.0], 5.0, 10.0, 25.0);
    let before = s.doc().unwrap().entity(a).unwrap().kind.clone();
    let b = arc(&mut s, [0.0, 0.0], 5.0, 12.0, 20.0);
    join(&mut s, &[a, b]);
    assert_eq!(s.doc().unwrap().entity(a).unwrap().kind, before);
    assert_eq!(objects(&s).len(), 1);
}

#[test]
fn arcs_that_reach_round_to_the_source_start_make_a_circle() {
    // (source, other) in degrees: touching end to end either way, and one across the source's
    // start that leaves a gap behind its end.
    for ((a1, a2), (b1, b2)) in [((100.0, 200.0), (0.0, 100.0)), ((0.0, 100.0), (100.0, 0.0)), ((0.0, 100.0), (350.0, 10.0))] {
        let mut s = Session::new();
        let a = arc(&mut s, [3.0, 4.0], 19.613670371032807, a1, a2);
        let b = arc(&mut s, [3.0, 4.0], 19.613670371032807, b1, b2);
        for (h, layer, color) in [(a, "A", 1), (b, "B", 3)] {
            s.doc_mut().unwrap().modify_entity(h, |e| (e.common.layer, e.common.color) = (layer.into(), Color::Index(color))).unwrap();
        }
        let c = join(&mut s, &[a, b]);
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "{objs:?}");
        let EntityKind::Circle(k) = &objs[0].1 else { panic!("not a circle: {objs:?}") };
        assert!(c != a && c != b && objs[0].0 == c, "a new object");
        assert!(k.radius == 19.613670371032807 && (k.center.x, k.center.y) == (3.0, 4.0), "{k:?}");
        // On the source's layer, in its colour.
        let common = &s.doc().unwrap().entity(c).unwrap().common;
        assert_eq!((common.layer.as_str(), &common.color), ("A", &Color::Index(1)));
        s.undo().unwrap();
        assert_eq!(objects(&s).iter().map(|o| o.0).collect::<Vec<_>>(), vec![a, b]);
    }
}

/// A polyline's (x, y, start width, end width, bulge) vertices, bulges to 1e-12.
fn vertices(k: &EntityKind) -> Vec<(f64, f64, f64, f64, f64)> {
    let EntityKind::LwPolyline(p) = k else { panic!("not a polyline: {k:?}") };
    p.vertices.iter().map(|v| (v.p.x, v.p.y, v.start_width, v.end_width, (v.bulge * 1e12).round() / 1e12)).collect()
}

#[test]
fn a_polyline_source_keeps_its_handle_and_its_vertices() {
    let corner = [(0.0, 0.0, 0.0, 0.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0), (20.0, 20.0, 0.0, 0.0, 0.0)];
    // A line at its end.
    let mut s = Session::new();
    let p = pline(&mut s, &corner);
    let l = line(&mut s, [20.0, 20.0], [0.0, 30.0]);
    assert_eq!(join(&mut s, &[p, l]), p);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1);
    assert_eq!(vertices(&objs[0].1), [&corner[..], &[(0.0, 30.0, 0.0, 0.0, 0.0)]].concat());
    s.undo().unwrap();
    assert_eq!(objects(&s).iter().map(|o| o.0).collect::<Vec<_>>(), vec![p, l]);
    assert_eq!(vertices(&s.doc().unwrap().entity(p).unwrap().kind), corner);
    // A half circle at its end: AutoCAD's bulge 1.
    let mut s = Session::new();
    let p = pline(&mut s, &corner);
    let a = arc(&mut s, [10.0, 20.0], 10.0, 0.0, 180.0);
    assert_eq!(join(&mut s, &[p, a]), p);
    let want = [(0.0, 0.0, 0.0, 0.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0), (20.0, 20.0, 0.0, 0.0, 1.0), (0.0, 20.0, 0.0, 0.0, 0.0)];
    assert_eq!(vertices(&objects(&s)[0].1), want);
    // Lines at both ends: the one at its start goes first.
    let mut s = Session::new();
    let p = pline(&mut s, &corner);
    let b = line(&mut s, [0.0, 0.0], [-10.0, -10.0]);
    let c = line(&mut s, [20.0, 20.0], [40.0, 30.0]);
    assert_eq!(join(&mut s, &[p, b, c]), p);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1);
    assert_eq!(vertices(&objs[0].1), [&[(-10.0, -10.0, 0.0, 0.0, 0.0)], &corner[..], &[(40.0, 30.0, 0.0, 0.0, 0.0)]].concat());
}

#[test]
fn a_polyline_source_gives_its_widths_to_lines_and_arcs_and_keeps_its_settings() {
    // Lines at both ends take the widths of the source's vertex at that end.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.5, 1.0, 0.0), (20.0, 0.0, 1.5, 2.5, 0.0), (20.0, 20.0, 3.0, 3.5, 0.0)]);
    let b = line(&mut s, [20.0, 20.0], [0.0, 30.0]);
    let c = line(&mut s, [0.0, 0.0], [-10.0, -10.0]);
    join(&mut s, &[p, b, c]);
    let want = [
        (-10.0, -10.0, 0.5, 1.0, 0.0),
        (0.0, 0.0, 0.5, 1.0, 0.0),
        (20.0, 0.0, 1.5, 2.5, 0.0),
        (20.0, 20.0, 3.0, 3.5, 0.0),
        (0.0, 30.0, 3.0, 3.5, 0.0),
    ];
    assert_eq!(vertices(&objects(&s)[0].1), want);
    // An arc ending at its start, so it goes first counterclockwise: tan(90 / 4) and the first
    // vertex's widths.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 1.0, 2.0, 0.0), (20.0, 0.0, 3.0, 4.0, 0.0), (20.0, 20.0, 5.0, 6.0, 0.0)]);
    let a = arc(&mut s, [0.0, -10.0], 10.0, 0.0, 90.0);
    join(&mut s, &[p, a]);
    let got = vertices(&objects(&s)[0].1);
    assert_eq!(got.len(), 4);
    let (x, y, w0, w1, bulge) = got[0];
    assert!(near(x, 10.0) && near(y, -10.0) && (w0, w1) == (1.0, 2.0) && near(bulge, (std::f64::consts::PI / 8.0).tan()), "{got:?}");
    assert_eq!(got[1..], [(0.0, 0.0, 1.0, 2.0, 0.0), (20.0, 0.0, 3.0, 4.0, 0.0), (20.0, 20.0, 5.0, 6.0, 0.0)]);
    // Constant width, elevation and linetype generation stay.
    let mut s = Session::new();
    let p = handle(&s.execute("pline", &json!({ "vertices": [[0, 0], [20, 0], [20, 20]], "width": 2.0 })).unwrap());
    s.doc_mut()
        .unwrap()
        .modify_entity(p, |e| {
            if let EntityKind::LwPolyline(pl) = &mut e.kind {
                (pl.elevation, pl.plinegen) = (5.0, true);
            }
        })
        .unwrap();
    let b = line(&mut s, [20.0, 20.0], [0.0, 30.0]);
    raise(&mut s, b, 5.0);
    join(&mut s, &[p, b]);
    let EntityKind::LwPolyline(pl) = &objects(&s)[0].1 else { panic!("not a polyline") };
    assert_eq!((pl.const_width, pl.elevation, pl.plinegen, pl.vertices.len()), (2.0, 5.0, true, 4));
}

#[test]
fn another_polyline_keeps_its_own_widths_and_bulges() {
    // At the source's end: the source's end vertex takes the first segment's bulge and widths,
    // and the new end vertex has none.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 1.0, 1.0, 0.0), (20.0, 0.0, 1.0, 1.0, 0.3), (20.0, 20.0, 1.0, 1.0, 0.0)]);
    let q = pline(&mut s, &[(20.0, 20.0, 4.0, 5.0, 0.5), (0.0, 30.0, 6.0, 7.0, 0.0), (-5.0, 40.0, 8.0, 9.0, 0.0)]);
    assert_eq!(join(&mut s, &[p, q]), p);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1);
    let want =
        [(0.0, 0.0, 1.0, 1.0, 0.0), (20.0, 0.0, 1.0, 1.0, 0.3), (20.0, 20.0, 4.0, 5.0, 0.5), (0.0, 30.0, 6.0, 7.0, 0.0), (-5.0, 40.0, 0.0, 0.0, 0.0)];
    assert_eq!(vertices(&objs[0].1), want);
    // Before the source's start, from a polyline of constant width 1: that width is spelled out
    // on the source's own segments and the end vertex's widths cleared.
    let mut s = Session::new();
    let p = handle(&s.execute("pline", &json!({ "vertices": [[0, 0], [20, 0], [20, 20]], "width": 1.0 })).unwrap());
    let q = pline(&mut s, &[(-5.0, -20.0, 4.0, 5.0, 0.0), (-10.0, -10.0, 6.0, 7.0, 0.25), (0.0, 0.0, 8.0, 9.0, 0.0)]);
    join(&mut s, &[p, q]);
    let EntityKind::LwPolyline(pl) = &objects(&s)[0].1 else { panic!("not a polyline") };
    assert_eq!(pl.const_width, 0.0);
    let want = [
        (-5.0, -20.0, 4.0, 5.0, 0.0),
        (-10.0, -10.0, 6.0, 7.0, 0.25),
        (0.0, 0.0, 1.0, 1.0, 0.0),
        (20.0, 0.0, 1.0, 1.0, 0.0),
        (20.0, 20.0, 0.0, 0.0, 0.0),
    ];
    assert_eq!(vertices(&objects(&s)[0].1), want);
    // The other polyline running the other way: its segments turned round, widths swapped and
    // bulge negated.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0), (20.0, 20.0, 0.0, 0.0, 0.0)]);
    let q = pline(&mut s, &[(0.0, 30.0, 2.0, 3.0, 0.4), (20.0, 20.0, 0.0, 0.0, 0.0)]);
    join(&mut s, &[p, q]);
    let got = vertices(&objects(&s)[0].1);
    assert_eq!(got[2..], [(20.0, 20.0, 3.0, 2.0, -0.4), (0.0, 30.0, 0.0, 0.0, 0.0)], "{got:?}");
    // And before its start.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0), (20.0, 20.0, 0.0, 0.0, 0.0)]);
    let q = pline(&mut s, &[(0.0, 0.0, 2.0, 3.0, 0.4), (-10.0, -10.0, 5.0, 6.0, 0.0)]);
    join(&mut s, &[p, q]);
    let got = vertices(&objects(&s)[0].1);
    assert_eq!(got[..2], [(-10.0, -10.0, 3.0, 2.0, -0.4), (0.0, 0.0, 0.0, 0.0, 0.0)], "{got:?}");
}

#[test]
fn a_polyline_source_closed_by_the_join_is_closed_without_its_first_vertex_again() {
    // (source, lines, polyline, AutoCAD's closed vertices): a line back to its start, two lines,
    // and a polyline of its own widths.
    type V = (f64, f64, f64, f64, f64);
    let cases: [(&[V], &[[[f64; 2]; 2]], &[V], &[V]); 3] = [
        (
            &[(0.0, 0.0, 1.0, 2.0, 0.0), (20.0, 0.0, 3.0, 4.0, 0.0), (20.0, 20.0, 5.0, 6.0, 0.0)],
            &[[[20.0, 20.0], [0.0, 0.0]]],
            &[],
            &[(0.0, 0.0, 1.0, 2.0, 0.0), (20.0, 0.0, 3.0, 4.0, 0.0), (20.0, 20.0, 5.0, 6.0, 0.0)],
        ),
        (
            &[(0.0, 0.0, 1.0, 2.0, 0.0), (20.0, 0.0, 3.0, 4.0, 0.0)],
            &[[[20.0, 0.0], [20.0, 20.0]], [[0.0, 0.0], [20.0, 20.0]]],
            &[],
            &[(0.0, 0.0, 1.0, 2.0, 0.0), (20.0, 0.0, 3.0, 4.0, 0.0), (20.0, 20.0, 3.0, 4.0, 0.0)],
        ),
        (
            &[(0.0, 0.0, 1.0, 1.0, 0.0), (20.0, 0.0, 1.0, 1.0, 0.0), (20.0, 20.0, 1.0, 1.0, 0.0)],
            &[],
            &[(20.0, 20.0, 4.0, 5.0, 0.2), (0.0, 20.0, 6.0, 7.0, 0.0), (0.0, 0.0, 8.0, 9.0, 0.0)],
            &[(0.0, 0.0, 1.0, 1.0, 0.0), (20.0, 0.0, 1.0, 1.0, 0.0), (20.0, 20.0, 4.0, 5.0, 0.2), (0.0, 20.0, 6.0, 7.0, 0.0)],
        ),
    ];
    for (src, lines, other, want) in cases {
        let mut s = Session::new();
        let p = pline(&mut s, src);
        let mut hs = vec![p];
        hs.extend(lines.iter().map(|l| line(&mut s, l[0], l[1])));
        if !other.is_empty() {
            hs.push(pline(&mut s, other));
        }
        assert_eq!(join(&mut s, &hs), p);
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "{objs:?}");
        let EntityKind::LwPolyline(pl) = &objs[0].1 else { panic!("not a polyline") };
        assert!(pl.closed, "{pl:?}");
        assert_eq!(vertices(&objs[0].1), want);
    }
}

#[test]
fn a_source_named_twice_is_one_object() {
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0), (20.0, 20.0, 0.0, 0.0, 0.0)]);
    let l = line(&mut s, [20.0, 20.0], [0.0, 30.0]);
    assert_eq!(join(&mut s, &[p, p, l]), p);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1, "{objs:?}");
    let want = [(0.0, 0.0, 0.0, 0.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0), (20.0, 20.0, 0.0, 0.0, 0.0), (0.0, 30.0, 0.0, 0.0, 0.0)];
    assert_eq!(vertices(&objs[0].1), want);
}

#[test]
fn join_names_what_the_objects_became() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [12.0, 0.0], [20.0, 0.0]);
    s.execute("select", &json!({ "handles": [a.hex(), b.hex()] })).unwrap();
    s.script("JOIN\n").unwrap();
    assert!(s.log.iter().any(|l| l == "2 objects joined into 1 line"), "{:?}", s.log);
    let mut s = Session::new();
    let a = arc(&mut s, [0.0, 0.0], 5.0, 0.0, 100.0);
    let b = arc(&mut s, [0.0, 0.0], 5.0, 130.0, 250.0);
    s.execute("select", &json!({ "handles": [a.hex(), b.hex()] })).unwrap();
    s.script("JOIN\n").unwrap();
    assert!(s.log.iter().any(|l| l == "2 objects joined into 1 arc"), "{:?}", s.log);
    let c = arc(&mut s, [0.0, 0.0], 5.0, 250.0, 0.0);
    s.execute("select", &json!({ "handles": [a.hex(), c.hex()] })).unwrap();
    s.script("JOIN\n").unwrap();
    assert!(s.log.iter().any(|l| l == "2 objects joined into 1 circle"), "{:?}", s.log);
}

#[test]
fn lines_and_arcs_less_than_a_millionth_off_join_and_further_off_do_not() {
    // AutoCAD 2024 with the line (0, 0)-(30, 0) and (35, d)-(50, d), and the same at survey
    // coordinates: joined for d = 9.99e-7, left apart for d = 1.001e-6.
    for (x, y) in [(0.0, 0.0), (612345.678, 2398765.432)] {
        for (d, joins) in [(9.99e-7, true), (1.001e-6, false)] {
            let mut s = Session::new();
            let a = line(&mut s, [x, y], [x + 30.0, y]);
            let b = line(&mut s, [x + 35.0, y + d], [x + 50.0, y + d]);
            let r = s.execute("join", &json!({ "handles": [a.hex(), b.hex()] }));
            assert_eq!((r.is_ok(), objects(&s).len()), (joins, if joins { 1 } else { 2 }), "({x}, {y}) {d}");
        }
    }
    // Arcs of radius 10 from 0 to 90 and from 120 to 200 degrees: joined with the radius
    // 10.000001, or the centre 1e-6 right at (1000, 1000) (both less than 1e-6 off in doubles);
    // left apart with the radius 10.00001, or the centre 1e-6 right at (0, 0).
    for (c, r, c2, joins) in [
        ([0.0, 0.0], 10.000001, [0.0, 0.0], true),
        ([1000.0, 1000.0], 10.0, [1000.000001, 1000.0], true),
        ([0.0, 0.0], 10.00001, [0.0, 0.0], false),
        ([0.0, 0.0], 10.0, [1e-6, 0.0], false),
    ] {
        let mut s = Session::new();
        let a = arc(&mut s, c, 10.0, 0.0, 90.0);
        let b = arc(&mut s, c2, r, 120.0, 200.0);
        let res = s.execute("join", &json!({ "handles": [a.hex(), b.hex()] }));
        assert_eq!((res.is_ok(), objects(&s).len()), (joins, if joins { 1 } else { 2 }), "{c2:?} {r}");
    }
    // A line from the source's end that turns off its line joins it the other way, into a
    // polyline (as JOIN in the AutoCAD 2024 window does), not as one line.
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [20.0, 5.0]);
    join(&mut s, &[a, b]);
    let objs = objects(&s);
    assert!(objs.len() == 1 && matches!(&objs[0].1, EntityKind::LwPolyline(p) if p.vertices.len() == 3), "{objs:?}");
    // Arcs of radius 1, from 0 to 180 degrees and from 180 degrees to d short of a whole turn,
    // or from d to 180 and from 180 round to 0: a circle for d = 9.9e-7 radians, an arc for
    // d = 1.01e-6, as in AutoCAD 2024.
    let half = std::f64::consts::PI;
    for (d, circle) in [(9.9e-7, true), (1.01e-6, false)] {
        for ((a1, a2), (b1, b2)) in [((0.0, half), (half, 2.0 * half - d)), ((d, half), (half, 2.0 * half))] {
            let mut s = Session::new();
            let a = arc(&mut s, [0.0, 0.0], 1.0, a1.to_degrees(), a2.to_degrees());
            let b = arc(&mut s, [0.0, 0.0], 1.0, b1.to_degrees(), b2.to_degrees());
            join(&mut s, &[a, b]);
            let objs = objects(&s);
            assert_eq!((objs.len(), matches!(objs[0].1, EntityKind::Circle(_))), (1, circle), "{d} {a1} {objs:?}");
        }
    }
    // A collinear line a hair above the source still joins it into one line.
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [12.0, 0.0], [20.0, 0.0]);
    raise(&mut s, b, 1e-12);
    join(&mut s, &[a, b]);
    let objs = objects(&s);
    assert!(objs.len() == 1 && matches!(&objs[0].1, EntityKind::Line(l) if l.b.x == 20.0 && l.b.z == 0.0), "{objs:?}");
    // A line in line with the source but at another height is not on its line.
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [12.0, 0.0], [20.0, 0.0]);
    raise(&mut s, b, 5.0);
    let before = objects(&s);
    assert!(s.execute("join", &json!({ "handles": [a.hex(), b.hex()] })).is_err());
    assert_eq!(objects(&s), before);
}
