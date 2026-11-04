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
use cadcraft_engine::doc::{EntityKind, Handle, Space};
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
    // Yes, the default, to "Convert to circle?".
    s.script("JOIN\n\n").unwrap();
    assert!(s.log.iter().any(|l| l == "2 objects joined into 1 circle"), "{:?}", s.log);
}

#[test]
fn arcs_that_come_round_ask_before_making_a_circle() {
    // AutoCAD 2024 with 7°-100° and an arc from -353° (stored 5e-16 rad before 7°) to 50°: "Combined
    // arc segments form a circle.  Convert to circle? [Yes/No] <Yes>:". Yes makes the circle, No
    // leaves both arcs as they are.
    let pair = |s: &mut Session| [arc(s, [0.0, 0.0], 10.0, 7.0, 100.0), arc(s, [0.0, 0.0], 10.0, -353.0, 50.0)];
    let mut s = Session::new();
    let [a, b] = pair(&mut s);
    let before = objects(&s);
    s.execute("select", &json!({ "handles": [a.hex(), b.hex()] })).unwrap();
    s.script("JOIN\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Combined arc segments form a circle.  Convert to circle? [Yes/No] <Yes>:");
    s.cmdline("N").unwrap();
    assert!(s.running.is_none());
    assert_eq!(objects(&s), before);
    s.execute("select", &json!({ "handles": [a.hex(), b.hex()] })).unwrap();
    s.script("JOIN\n\n").unwrap();
    assert!(matches!(&objects(&s)[..], [(_, EntityKind::Circle(c))] if c.radius == 10.0), "{:?}", objects(&s));
    // The JSON form takes the default, Yes.
    let mut s = Session::new();
    let [a, b] = pair(&mut s);
    join(&mut s, &[a, b]);
    assert!(matches!(&objects(&s)[..], [(_, EntityKind::Circle(_))]), "{:?}", objects(&s));
    // There is no tolerance at the source's start: an arc from exactly 7°, or from 5e-7 rad after
    // the source's start, lies inside it and the source stays; from 5e-7 or 2e-6 rad before it,
    // the arcs come round (all measured).
    for (start, circle) in [(7.0, false), (7.0 + 5e-7f64.to_degrees(), false), (7.0 - 5e-7f64.to_degrees(), true), (7.0 - 2e-6f64.to_degrees(), true)]
    {
        let mut s = Session::new();
        let a = arc(&mut s, [0.0, 0.0], 10.0, 7.0, 100.0);
        let b = arc(&mut s, [0.0, 0.0], 10.0, start, 50.0);
        let source = objects(&s)[0].1.clone();
        join(&mut s, &[a, b]);
        let after = objects(&s);
        if circle {
            assert!(matches!(&after[..], [(_, EntityKind::Circle(_))]), "{start}: {after:?}");
        } else {
            assert_eq!(after, [(a, source)], "{start}");
        }
    }
}

#[test]
fn join_asks_about_a_circle_only_when_it_would_make_one() {
    // Arcs that come round, picked with a line, a spline or on a locked layer: JOIN goes another
    // way or refuses, and doesn't ask.
    let half_circles = |s: &mut Session| [arc(s, [0.0, 0.0], 5.0, 0.0, 180.0), arc(s, [0.0, 0.0], 5.0, 180.0, 0.0)];
    for spline in [false, true] {
        let mut s = Session::new();
        let [a, b] = half_circles(&mut s);
        let other = if spline {
            handle(&s.execute("spline", &json!({ "control": [[50, 0], [53, 3], [56, -3], [59, 0]] })).unwrap())
        } else {
            line(&mut s, [5.0, 0.0], [15.0, 0.0])
        };
        s.execute("select", &json!({ "handles": [a.hex(), b.hex(), other.hex()] })).unwrap();
        s.script("JOIN\n").unwrap();
        assert!(s.running.is_none(), "spline {spline}: {}", s.prompt_text());
        // Not joined onto the arcs' circle: the other object isn't taken with them.
        let objs = objects(&s);
        assert!(objs.iter().any(|(h, _)| *h == other) && !objs.iter().any(|(_, k)| matches!(k, EntityKind::Circle(_))), "spline {spline}: {objs:?}");
    }
    // Arcs that come round exactly, or to 9.9e-7 rad short of the source's start, ask too; 1.01e-6
    // short, they make an arc without asking (measured: CIRCLE and ARC).
    for (short, asks) in [(0.0, true), (9.9e-7f64, true), (1.01e-6, false)] {
        let mut s = Session::new();
        let a = arc(&mut s, [0.0, 0.0], 5.0, 0.0, 180.0);
        let b = arc(&mut s, [0.0, 0.0], 5.0, 180.0, 360.0 - short.to_degrees());
        s.execute("select", &json!({ "handles": [a.hex(), b.hex()] })).unwrap();
        s.script("JOIN\n").unwrap();
        assert_eq!(s.prompt_text().contains("Convert to circle?"), asks, "{short}: {}", s.prompt_text());
        if !asks {
            assert!(s.running.is_none() && matches!(&objects(&s)[..], [(h, EntityKind::Arc(_))] if *h == a), "{:?}", objects(&s));
        }
    }
    // Arcs picked at "Select arcs to join to source" ask too, and No changes nothing.
    let mut s = Session::new();
    arc(&mut s, [0.0, 0.0], 5.0, 0.0, 180.0);
    arc(&mut s, [0.0, 0.0], 5.0, 180.0, 0.0);
    let before = objects(&s);
    s.script("JOIN 0,5\n\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Select arcs to join to source or [cLose]:");
    s.cmdline("0,-5").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Combined arc segments form a circle.  Convert to circle? [Yes/No] <Yes>:");
    s.cmdline("N").unwrap();
    assert!(s.running.is_none());
    assert_eq!(objects(&s), before);
    let mut s = Session::new();
    let [a, b] = half_circles(&mut s);
    s.execute("layer.set", &json!({ "name": "0", "locked": true })).unwrap();
    s.execute("select", &json!({ "handles": [a.hex(), b.hex()] })).unwrap();
    s.script("JOIN\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l == "An object is on a locked layer."), "{:?}", s.log);
}

#[test]
fn what_join_makes_stays_in_the_source_space() {
    // Arcs and lines in a layout joined while model space is current: the circle and the new
    // polyline go in the layout.
    let mut s = Session::new();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let arcs = [arc(&mut s, [0.0, 0.0], 5.0, 0.0, 180.0), arc(&mut s, [0.0, 0.0], 5.0, 180.0, 0.0)];
    let lines = [line(&mut s, [20.0, 0.0], [30.0, 0.0]), line(&mut s, [30.0, 0.0], [30.0, 10.0])];
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    for hs in [arcs, lines] {
        let made = join(&mut s, &hs);
        assert_eq!(s.doc().unwrap().space_of(made), Some(Space::Paper("Layout1".into())), "{hs:?}");
    }
    assert!(objects(&s).is_empty());
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

#[test]
fn a_polyline_met_at_an_inner_vertex_stays_whole() {
    // AutoCAD 2024: a polyline (0,0)-(10,0) takes the line (10,0)-(10,5); the polyline
    // (0,5)-(10,5)-(20,5), which the line meets at its middle vertex, is left as it was.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)]);
    let l = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
    let q_vs = [(0.0, 5.0, 0.0, 0.0, 0.0), (10.0, 5.0, 0.0, 0.0, 0.0), (20.0, 5.0, 0.0, 0.0, 0.0)];
    let q = pline(&mut s, &q_vs);
    assert_eq!(join(&mut s, &[p, l, q]), p);
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![p, q], "{objs:?}");
    assert_eq!(vertices(&objs[0].1), [(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0), (10.0, 5.0, 0.0, 0.0, 0.0)]);
    assert_eq!(vertices(&objs[1].1), q_vs);
    // The same through a new polyline: two lines at a corner, and that polyline at the far end.
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
    let q = pline(&mut s, &q_vs);
    let r = join(&mut s, &[a, b, q]);
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![q, r], "{objs:?}");
    assert_eq!(vertices(&objs[0].1), q_vs);
    assert_eq!(vertices(&objs[1].1), [(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0), (10.0, 5.0, 0.0, 0.0, 0.0)]);
    // Nothing else to join (AutoCAD 2024: "0 segments added to polyline", or nothing joined to
    // a line): refused, the drawing as it was.
    for (first, other) in [
        (
            vec![(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)],
            vec![(10.0, -5.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0), (10.0, 5.0, 0.0, 0.0, 0.0)],
        ),
        (
            vec![(-10.0, 0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)],
            vec![(10.0, -5.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0), (10.0, 5.0, 0.0, 0.0, 0.0)],
        ),
    ] {
        let mut s = Session::new();
        let p = pline(&mut s, &first);
        let q = pline(&mut s, &other);
        let before = objects(&s);
        assert!(s.execute("join", &json!({ "handles": [p.hex(), q.hex()] })).is_err(), "{first:?}");
        assert_eq!(objects(&s), before);
    }
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let q = pline(&mut s, &[(20.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0), (10.0, -10.0, 0.0, 0.0, 0.0)]);
    let before = objects(&s);
    assert!(s.execute("join", &json!({ "handles": [a.hex(), q.hex()] })).is_err());
    assert_eq!(objects(&s), before);
}

#[test]
fn objects_join_whole_by_their_ends_whatever_the_order() {
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    // AutoCAD 2024: the polyline (30,0)-(20,0) takes (0,0)-(10,0)-(20,0) whole, picked before or
    // after the line (10,0)-(10,5), which meets only its middle vertex and is left as it was.
    for order in [[0, 1, 2], [0, 2, 1]] {
        let mut s = Session::new();
        let p0 = pline(&mut s, &v(&[(30.0, 0.0), (20.0, 0.0)]));
        let m = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
        let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]));
        let picked = order.map(|i| [p0, m, p][i]);
        assert_eq!(join(&mut s, &picked), p0, "{order:?}");
        let objs = objects(&s);
        assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![p0, m], "{order:?}");
        assert_eq!(vertices(&objs[0].1), v(&[(30.0, 0.0), (20.0, 0.0), (10.0, 0.0), (0.0, 0.0)]), "{order:?}");
    }
    // AutoCAD 2024: (10,0)-(10,5), then the line (10,5)-(20,0), then (0,0)-(10,0)-(20,0) whole
    // from its end: (10,0) (10,5) (20,0) (10,0) (0,0), not split at its middle vertex.
    let mut s = Session::new();
    let src = pline(&mut s, &v(&[(10.0, 0.0), (10.0, 5.0)]));
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]));
    let m = line(&mut s, [10.0, 5.0], [20.0, 0.0]);
    assert_eq!(join(&mut s, &[src, p, m]), src);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1, "{objs:?}");
    assert_eq!(vertices(&objs[0].1), v(&[(10.0, 0.0), (10.0, 5.0), (20.0, 0.0), (10.0, 0.0), (0.0, 0.0)]));
}

#[test]
fn one_object_alone_joins_nothing_even_when_its_ends_meet() {
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    // AutoCAD 2024 ("0 segments added to polyline"): a polyline that the other meets only at its
    // middle vertex. And a circle, which JOIN can't take, beside a polyline: nothing to join it to.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]));
    let q = pline(&mut s, &v(&[(10.0, 0.0), (10.0, 5.0), (10.0, 10.0)]));
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 5 })).unwrap());
    // A polyline whose last vertex is its first stays open, alone or beside a line it doesn't
    // reach (AutoCAD 2024: unchanged, 70 = 0, all four vertices).
    let r = pline(&mut s, &v(&[(100.0, 0.0), (110.0, 0.0), (110.0, 10.0), (100.0, 0.0)]));
    let l = line(&mut s, [150.0, 50.0], [160.0, 50.0]);
    let before = objects(&s);
    for hs in [&[p, q][..], &[c, p], &[r], &[r, l]] {
        let hex: Vec<String> = hs.iter().map(|h| h.hex()).collect();
        assert!(s.execute("join", &json!({ "handles": hex })).is_err(), "{hs:?}");
        assert_eq!(objects(&s), before, "{hs:?}");
    }
}

#[test]
fn a_polyline_whose_ends_meet_takes_more_where_they_meet() {
    // AutoCAD 2024 with the source (0,0) (10,0) (10,10) (0,0), open, and an object from or to
    // (0,0): it keeps its handle and stays open. A line or an arc goes in turned round, its end
    // at the source's end if it can, else its start at the source's start; a polyline goes before
    // the source, turned round if it starts at (0,0).
    let src = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)];
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    let tail = [(-5.0, -5.0), (-5.0, 0.0)];
    type Make = fn(&mut Session) -> Handle;
    let cases: [(Make, Vec<(f64, f64, f64, f64, f64)>); 5] = [
        (|s| line(s, [0.0, 0.0], [-5.0, 0.0]), v(&[(-5.0, 0.0), (0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)])),
        (|s| line(s, [-5.0, 0.0], [0.0, 0.0]), v(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0), (-5.0, 0.0)])),
        (
            |s| pline(s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (-5.0, 0.0, 0.0, 0.0, 0.0), (-5.0, -5.0, 0.0, 0.0, 0.0)]),
            v(&[tail[0], tail[1], src[0], src[1], src[2], src[3]]),
        ),
        (
            |s| pline(s, &[(-5.0, -5.0, 0.0, 0.0, 0.0), (-5.0, 0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0, 0.0)]),
            v(&[tail[0], tail[1], src[0], src[1], src[2], src[3]]),
        ),
        (|s| arc(s, [0.0, -5.0], 5.0, 90.0, 180.0), {
            let mut w = v(&[(-5.0, -5.0), src[0], src[1], src[2], src[3]]);
            w[0].4 = -((std::f64::consts::PI / 8.0).tan() * 1e12).round() / 1e12;
            w
        }),
    ];
    for (i, (make, want)) in cases.into_iter().enumerate() {
        let mut s = Session::new();
        let p = pline(&mut s, &v(&src));
        let o = make(&mut s);
        assert_eq!(join(&mut s, &[p, o]), p, "case {i}");
        let objs = objects(&s);
        assert_eq!(objs.len(), 1, "case {i}: {objs:?}");
        let EntityKind::LwPolyline(pl) = &objs[0].1 else { panic!("not a polyline") };
        assert!(!pl.closed, "case {i}");
        // An arc's end point is worked out from its centre, so to 1e-9.
        let got = vertices(&objs[0].1);
        let same = |a: &(f64, f64, f64, f64, f64), b: &(f64, f64, f64, f64, f64)| {
            [a.0 - b.0, a.1 - b.1, a.2 - b.2, a.3 - b.3, a.4 - b.4].iter().all(|d| d.abs() < 1e-9)
        };
        assert!(got.len() == want.len() && got.iter().zip(&want).all(|(a, b)| same(a, b)), "case {i}: {got:?}");
    }
    // A join that closes it there closes it, and nothing more joins (AutoCAD 2024): a line of no
    // length at (0,0) gives a closed polyline of its four vertices, the line from (0,0) picked
    // after it staying; a polyline (0,0) (-5,0) (-5,-5) (0,0) goes before it turned round.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&src));
    let zero = line(&mut s, [0.0, 0.0], [1.0, 0.0]);
    s.doc_mut()
        .unwrap()
        .modify_entity(zero, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.b = l.a;
            }
        })
        .unwrap();
    let l = line(&mut s, [0.0, 0.0], [-5.0, 0.0]);
    assert_eq!(join(&mut s, &[p, zero, l]), p);
    let objs = objects(&s);
    assert!(matches!(&objs[..], [(a, EntityKind::LwPolyline(pl)), (b, EntityKind::Line(_))] if *a == p && *b == l && pl.closed), "{objs:?}");
    assert_eq!(vertices(&objs[0].1), v(&src));
    let mut s = Session::new();
    let p = pline(&mut s, &v(&src));
    let q = pline(&mut s, &v(&[(0.0, 0.0), (-5.0, 0.0), (-5.0, -5.0), (0.0, 0.0)]));
    assert_eq!(join(&mut s, &[p, q]), p);
    let objs = objects(&s);
    assert!(matches!(&objs[..], [(a, EntityKind::LwPolyline(pl))] if *a == p && pl.closed), "{objs:?}");
    assert_eq!(vertices(&objs[0].1), v(&[(0.0, 0.0), (-5.0, -5.0), (-5.0, 0.0), (0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]));
}

#[test]
fn a_nearly_straight_arc_segment_keeps_the_chain_whole() {
    // A bulge of 3e-10 over 100: as an arc its radius is about 4e10, and its ends worked out again
    // from the centre are off by more than the tolerance. The vertices themselves still meet.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 3e-10), (100.0, 0.0, 0.0, 0.0, 0.0), (100.0, 10.0, 0.0, 0.0, 0.0)]);
    let l = line(&mut s, [100.0, 10.0], [110.0, 10.0]);
    assert_eq!(join(&mut s, &[p, l]), p);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1, "{objs:?}");
    let EntityKind::LwPolyline(pl) = &objs[0].1 else { panic!("not a polyline") };
    let pts: Vec<(f64, f64)> = pl.vertices.iter().map(|v| (v.p.x, v.p.y)).collect();
    assert_eq!(pts, [(0.0, 0.0), (100.0, 0.0), (100.0, 10.0), (110.0, 10.0)]);
    assert_eq!(pl.vertices[0].bulge, 3e-10);
}

#[test]
fn a_new_polyline_takes_the_first_joined_objects_properties() {
    // A circle picked first isn't part of the polyline: the polyline is on the first line's layer
    // (AutoCAD 2024 passes over a circle picked as the source; the new polyline's layer itself is
    // not measured).
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 5 })).unwrap());
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
    for (h, layer) in [(c, "C"), (a, "A"), (b, "B")] {
        s.doc_mut().unwrap().modify_entity(h, |e| e.common.layer = layer.into()).unwrap();
    }
    let r = join(&mut s, &[c, a, b]);
    let d = s.doc().unwrap();
    assert_eq!(d.entity(r).unwrap().common.layer, "A");
    assert!(d.entity(c).is_some(), "the circle stays");
}

#[test]
fn the_source_is_the_first_object_join_can_take() {
    // AutoCAD 2024: a circle picked first, then the polyline (0,0)-(10,0) of width 2, then the line
    // (10,0)-(10,5): the polyline takes the line, keeping its handle and its width.
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 5 })).unwrap());
    let p = handle(&s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0]], "width": 2.0 })).unwrap());
    let l = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
    assert_eq!(join(&mut s, &[c, p, l]), p);
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![c, p], "{objs:?}");
    let EntityKind::LwPolyline(pl) = &objs[1].1 else { panic!("not a polyline") };
    assert_eq!((pl.const_width, pl.vertices.len()), (2.0, 3));
}

#[test]
fn a_closed_chain_takes_nothing_more() {
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    // AutoCAD 2024: (0,0)-(10,0)-(10,10), the line back to (0,0), then a line on from (0,0): the
    // triangle closes and the last line stays as it was.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]));
    let back = line(&mut s, [10.0, 10.0], [0.0, 0.0]);
    let tail = line(&mut s, [0.0, 0.0], [-5.0, 0.0]);
    assert_eq!(join(&mut s, &[p, back, tail]), p);
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![p, tail], "{objs:?}");
    let EntityKind::LwPolyline(pl) = &objs[0].1 else { panic!("not a polyline") };
    assert!(pl.closed, "{pl:?}");
    assert_eq!(vertices(&objs[0].1), v(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]));
    // The line on picked before the line back: it joins first, and the chain no longer closes.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]));
    let tail = line(&mut s, [0.0, 0.0], [-5.0, 0.0]);
    let back = line(&mut s, [10.0, 10.0], [0.0, 0.0]);
    assert_eq!(join(&mut s, &[p, tail, back]), p);
    let objs = objects(&s);
    assert_eq!(objs.len(), 1, "{objs:?}");
    assert_eq!(vertices(&objs[0].1), v(&[(-5.0, 0.0), (0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)]));
}

#[test]
fn objects_meeting_the_chain_at_its_start_turned_round() {
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    // AutoCAD 2024: (0,0)-(10,0) and the polyline (0,0)-(-10,0)-(-10,10), turned round in front.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0)]));
    let q = pline(&mut s, &v(&[(0.0, 0.0), (-10.0, 0.0), (-10.0, 10.0)]));
    assert_eq!(join(&mut s, &[p, q]), p);
    assert_eq!(vertices(&objects(&s)[0].1), v(&[(-10.0, 10.0), (-10.0, 0.0), (0.0, 0.0), (10.0, 0.0)]));
    // AutoCAD 2024: a polyline meeting both of the source's ends closes it from its start, in
    // front of the source turned round: widths swapped, bulge negated.
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 1.0, 1.0, 0.0), (20.0, 0.0, 1.0, 1.0, 0.0), (20.0, 20.0, 1.0, 1.0, 0.0)]);
    let q = pline(&mut s, &[(0.0, 0.0, 4.0, 5.0, 0.0), (0.0, 20.0, 6.0, 7.0, 0.3), (20.0, 20.0, 8.0, 9.0, 0.0)]);
    assert_eq!(join(&mut s, &[p, q]), p);
    let objs = objects(&s);
    let EntityKind::LwPolyline(pl) = &objs[0].1 else { panic!("not a polyline") };
    assert!(pl.closed && objs.len() == 1, "{objs:?}");
    let want = [(20.0, 20.0, 7.0, 6.0, -0.3), (0.0, 20.0, 5.0, 4.0, 0.0), (0.0, 0.0, 1.0, 1.0, 0.0), (20.0, 0.0, 1.0, 1.0, 0.0)];
    assert_eq!(vertices(&objs[0].1), want);
}

#[test]
fn lines_of_no_length_and_repeated_vertices() {
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    // A line from (x,y) to (x,y).
    let dot = |s: &mut Session, x: f64, y: f64| {
        let h = line(s, [x, y], [x + 1.0, y]);
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let EntityKind::Line(l) = &mut e.kind {
                    l.b = l.a;
                }
            })
            .unwrap();
        h
    };
    // AutoCAD 2024: two of them alone stay as they are.
    let mut s = Session::new();
    let (a, b) = (dot(&mut s, 5.0, 5.0), dot(&mut s, 5.0, 5.0));
    let before = objects(&s);
    assert!(s.execute("join", &json!({ "handles": [a.hex(), b.hex()] })).is_err());
    assert_eq!(objects(&s), before);
    // AutoCAD 2024: a line takes one at its end and stays as it was.
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let z = dot(&mut s, 10.0, 0.0);
    assert_eq!(join(&mut s, &[l, z]), l);
    let objs = objects(&s);
    assert!(objs.len() == 1 && matches!(&objs[0].1, EntityKind::Line(k) if (k.b.x, k.b.y) == (10.0, 0.0)), "{objs:?}");
    // AutoCAD 2024: a polyline takes it as a repeated vertex, and keeps another polyline's.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0)]));
    let z = dot(&mut s, 10.0, 0.0);
    let m = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
    assert_eq!(join(&mut s, &[p, z, m]), p);
    assert_eq!(vertices(&objects(&s)[0].1), v(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 5.0)]));
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (10.0, 0.0)]));
    let q = pline(&mut s, &v(&[(10.0, 0.0), (10.0, 0.0), (10.0, 5.0)]));
    assert_eq!(join(&mut s, &[p, q]), p);
    assert_eq!(vertices(&objects(&s)[0].1), v(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 5.0)]));
    // An arc of no radius (AutoCAD can't draw one) joins nothing.
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let g = arc(&mut s, [10.0, 0.0], 1.0, 0.0, 90.0);
    s.doc_mut()
        .unwrap()
        .modify_entity(g, |e| {
            if let EntityKind::Arc(k) = &mut e.kind {
                k.radius = 0.0;
            }
        })
        .unwrap();
    let before = objects(&s);
    assert!(s.execute("join", &json!({ "handles": [l.hex(), g.hex()] })).is_err());
    assert_eq!(objects(&s), before);
}

#[test]
fn another_polyline_keeps_its_widths_in_a_new_polyline() {
    // A line picked first makes a new polyline; the polyline after it keeps its segment's widths
    // (no data lost; AutoCAD's own JOIN takes no polyline onto a line source, not measured).
    let mut s = Session::new();
    let l = line(&mut s, [-10.0, 0.0], [0.0, 0.0]);
    let q = pline(&mut s, &[(0.0, 0.0, 2.0, 3.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)]);
    let r = join(&mut s, &[l, q]);
    let objs = objects(&s);
    assert!(objs.len() == 1 && objs[0].0 == r, "{objs:?}");
    assert_eq!(vertices(&objs[0].1), [(-10.0, 0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 2.0, 3.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)]);
}

#[test]
fn a_circle_picked_first_is_passed_over_by_lines_and_arcs_too() {
    // AutoCAD 2024 passes over a circle picked as the source: the first line takes the next.
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 5 })).unwrap());
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
    assert_eq!(join(&mut s, &[c, a, b]), a);
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![c, a], "{objs:?}");
    assert!(matches!(&objs[1].1, EntityKind::Line(l) if (l.b.x, l.b.y) == (20.0, 0.0)), "{objs:?}");
    // And the first arc takes the next on its circle.
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 5 })).unwrap());
    let g = arc(&mut s, [0.0, 0.0], 5.0, 0.0, 100.0);
    let h = arc(&mut s, [0.0, 0.0], 5.0, 130.0, 250.0);
    assert_eq!(join(&mut s, &[c, g, h]), g);
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).collect::<Vec<_>>(), vec![c, g], "{objs:?}");
    assert!(matches!(&objs[1].1, EntityKind::Arc(k) if same_angle(k.end, 250f64.to_radians())), "{objs:?}");
}

#[test]
fn an_arc_closing_a_polyline_and_a_source_of_no_length() {
    let v = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| (x, y, 0.0, 0.0, 0.0)).collect::<Vec<_>>();
    // AutoCAD 2024: (0,0)-(20,0)-(20,20) and the half circle from (20,20) round to (0,0) close
    // into (0,0) (20,0) (20,20) with a bulge of 1.
    let mut s = Session::new();
    let p = pline(&mut s, &v(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0)]));
    let g = arc(&mut s, [10.0, 10.0], 200f64.sqrt(), 45.0, 225.0);
    assert_eq!(join(&mut s, &[p, g]), p);
    let objs = objects(&s);
    let EntityKind::LwPolyline(pl) = &objs[0].1 else { panic!("not a polyline") };
    assert!(objs.len() == 1 && pl.closed, "{objs:?}");
    let got = vertices(&objs[0].1);
    assert!(got.len() == 3 && got[..2] == v(&[(0.0, 0.0), (20.0, 0.0)])[..] && near(got[2].0, 20.0) && near(got[2].1, 20.0), "{got:?}");
    assert!(near(got[2].4, 1.0), "{got:?}");
    // AutoCAD 2024: a line of no length picked as the source takes nothing; both stay.
    let mut s = Session::new();
    let z = line(&mut s, [5.0, 5.0], [6.0, 5.0]);
    s.doc_mut()
        .unwrap()
        .modify_entity(z, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.b = l.a;
            }
        })
        .unwrap();
    let l = line(&mut s, [5.0, 5.0], [10.0, 5.0]);
    let before = objects(&s);
    assert!(s.execute("join", &json!({ "handles": [z.hex(), l.hex()] })).is_err());
    assert_eq!(objects(&s), before);
}

#[test]
fn objects_meeting_only_in_plan_at_another_height_stay_apart() {
    // AutoCAD 2024 (accoreconsole, measured): an end that meets the source only in plan, at
    // another height, is no join. The line (0,0,0)-(10,0,0) keeps (10,0,5)-(20,0,5) apart ("1
    // object discarded"), a polyline at elevation 0 that line, and a polyline at elevation 5 the
    // line (10,0,0)-(20,0,0) ("0 segments added"). Both lines at 5 join into one.
    let elevated = |s: &mut Session, h: Handle, z: f64| {
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let EntityKind::LwPolyline(p) = &mut e.kind {
                    p.elevation = z;
                }
            })
            .unwrap();
    };
    for (source_z, other_z, poly) in [(0.0, 5.0, false), (0.0, 5.0, true), (5.0, 0.0, true)] {
        let mut s = Session::new();
        let a = if poly { pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)]) } else { line(&mut s, [0.0, 0.0], [10.0, 0.0]) };
        if poly {
            elevated(&mut s, a, source_z);
        } else {
            raise(&mut s, a, source_z);
        }
        let b = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
        raise(&mut s, b, other_z);
        let before = objects(&s);
        let _ = s.execute("join", &json!({ "handles": [a.hex(), b.hex()] }));
        assert_eq!(objects(&s), before, "source at {source_z}, other at {other_z}, polyline {poly}");
    }
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
    raise(&mut s, a, 5.0);
    raise(&mut s, b, 5.0);
    join(&mut s, &[a, b]);
    let objs = objects(&s);
    assert!(objs.len() == 1 && matches!(&objs[0].1, EntityKind::Line(l) if l.b.x == 20.0 && l.a.z == 5.0 && l.b.z == 5.0), "{objs:?}");
    // A sloped line ends at its own heights (AutoCAD 2024, measured): after a polyline at 0, the
    // line (20,0,5)-(10,0,0), turned round, leaves the chain at height 5, so a line on from
    // (20,0,5) joins too ("2 segments added") and one from (20,0,0) stays apart ("1 segments
    // added"). An arc whose centre is at another height stays apart ("0 segments added").
    for (next_z, joins) in [(5.0, true), (0.0, false)] {
        let mut s = Session::new();
        let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)]);
        let sloped = line(&mut s, [20.0, 0.0], [10.0, 0.0]);
        s.doc_mut()
            .unwrap()
            .modify_entity(sloped, |e| {
                if let EntityKind::Line(l) = &mut e.kind {
                    l.a.z = 5.0;
                }
            })
            .unwrap();
        let next = line(&mut s, [20.0, 0.0], [30.0, 0.0]);
        raise(&mut s, next, next_z);
        assert_eq!(join(&mut s, &[p, sloped, next]), p);
        let objs = objects(&s);
        let left: Vec<Handle> = objs.iter().map(|o| o.0).collect();
        assert_eq!(left, if joins { vec![p] } else { vec![p, next] }, "next at {next_z}");
        assert_eq!(vertices(&objs[0].1).len(), if joins { 4 } else { 3 }, "next at {next_z}");
    }
    let mut s = Session::new();
    let p = pline(&mut s, &[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)]);
    let a = arc(&mut s, [10.0, 5.0], 5.0, 270.0, 0.0);
    s.doc_mut()
        .unwrap()
        .modify_entity(a, |e| {
            if let EntityKind::Arc(o) = &mut e.kind {
                o.center.z = 5.0;
            }
        })
        .unwrap();
    let before = objects(&s);
    let _ = s.execute("join", &json!({ "handles": [p.hex(), a.hex()] }));
    assert_eq!(objects(&s), before);
}

#[test]
fn objects_facing_the_other_way_from_the_source_stay_apart() {
    // AutoCAD 2024 (accoreconsole, measured) with polylines whose normal is 0,0,-1, here in world
    // coordinates as the DXF reader keeps them: one at elevation 5 takes no line on from its end,
    // at Z -5 or 5; it takes another such polyline at elevation 5 ("1 segments added") but not one
    // at -5; and a polyline facing up at elevation -5 doesn't take one at 5, though they meet at Z
    // -5 ("0 segments added").
    let poly = |s: &mut Session, from: f64, elevation: f64, down: bool| {
        let h = pline(s, &[(from, 0.0, 0.0, 0.0, 0.0), (from + 10.0, 0.0, 0.0, 0.0, 0.0)]);
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if down {
                    e.common.extrusion = cadcraft_engine::geom::Vec3::new(0.0, 0.0, -1.0);
                }
                if let EntityKind::LwPolyline(p) = &mut e.kind {
                    p.elevation = elevation;
                }
            })
            .unwrap();
        h
    };
    for line_z in [-5.0, 5.0] {
        let mut s = Session::new();
        let p = poly(&mut s, 0.0, 5.0, true);
        let l = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
        raise(&mut s, l, line_z);
        let before = objects(&s);
        let _ = s.execute("join", &json!({ "handles": [p.hex(), l.hex()] }));
        assert_eq!(objects(&s), before, "line at {line_z}");
    }
    for (first, second, joins) in [((5.0, true), (5.0, true), true), ((5.0, true), (-5.0, true), false), ((-5.0, false), (5.0, true), false)] {
        let mut s = Session::new();
        let p = poly(&mut s, 0.0, first.0, first.1);
        let q = poly(&mut s, 10.0, second.0, second.1);
        let before = objects(&s);
        let _ = s.execute("join", &json!({ "handles": [p.hex(), q.hex()] }));
        let objs = objects(&s);
        if joins {
            assert!(objs.len() == 1 && objs[0].0 == p && vertices(&objs[0].1).len() == 3, "{first:?} {second:?} {objs:?}");
        } else {
            assert_eq!(objs, before, "{first:?} {second:?}");
        }
    }
    // A line faces up whatever its normal (AutoCAD 2024, measured): a polyline facing up takes a
    // line with normal 0,0,-1 from its end, square to it ("1 segments added") or straight on with
    // an arc facing up after it ("2 segments added"), and one facing down at elevation 0 takes no
    // line from its end, whichever way the line's normal points ("0 segments added").
    let face_down = |s: &mut Session, h: Handle| {
        s.doc_mut().unwrap().modify_entity(h, |e| e.common.extrusion = cadcraft_engine::geom::Vec3::new(0.0, 0.0, -1.0)).unwrap();
    };
    let mut s = Session::new();
    let p = poly(&mut s, 0.0, 0.0, false);
    let l = line(&mut s, [10.0, 0.0], [10.0, 10.0]);
    face_down(&mut s, l);
    assert_eq!(join(&mut s, &[p, l]), p);
    assert_eq!(objects(&s).len(), 1);
    let mut s = Session::new();
    let p = poly(&mut s, 0.0, 0.0, false);
    let l = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
    face_down(&mut s, l);
    let a = arc(&mut s, [20.0, 5.0], 5.0, 270.0, 0.0);
    assert_eq!(join(&mut s, &[p, l, a]), p);
    let objs = objects(&s);
    assert!(objs.len() == 1 && vertices(&objs[0].1).len() == 4, "{objs:?}");
    for line_down in [false, true] {
        let mut s = Session::new();
        let p = poly(&mut s, 0.0, 0.0, true);
        let l = line(&mut s, [10.0, 0.0], [10.0, 10.0]);
        if line_down {
            face_down(&mut s, l);
        }
        let before = objects(&s);
        let _ = s.execute("join", &json!({ "handles": [p.hex(), l.hex()] }));
        assert_eq!(objects(&s), before, "line facing down {line_down}");
    }
    // An arc faces its normal (AutoCAD 2024, measured, with the arc from (10,0) to (15,5) round
    // (10,5)): a polyline facing up takes no arc facing down, and one facing down at elevation 0 no
    // arc facing up, but it takes one facing down, with the bulge and vertex AutoCAD writes (-0.414
    // and (-15,5) in the polyline's own coordinates). At elevation 5, its world height is -5: it
    // takes the arc facing down whose centre is at Z -5 and not one at 5.
    for (poly_down, elevation, arc_down, arc_z, joins) in [
        (false, 0.0, true, 0.0, false),
        (true, 0.0, false, 0.0, false),
        (true, 0.0, true, 0.0, true),
        (true, 5.0, true, -5.0, true),
        (true, 5.0, true, 5.0, false),
    ] {
        let mut s = Session::new();
        let p = poly(&mut s, 0.0, elevation, poly_down);
        let a = arc(&mut s, [10.0, 5.0], 5.0, 270.0, 0.0);
        s.doc_mut()
            .unwrap()
            .modify_entity(a, |e| {
                if let EntityKind::Arc(o) = &mut e.kind {
                    o.center.z = arc_z;
                }
            })
            .unwrap();
        if arc_down {
            face_down(&mut s, a);
        }
        let before = objects(&s);
        let r = s.execute("join", &json!({ "handles": [p.hex(), a.hex()] }));
        let objs = objects(&s);
        let case = format!("polyline down {poly_down} at {elevation}, arc down {arc_down} at {arc_z}");
        if joins {
            assert!(r.is_ok() && objs.len() == 1 && objs[0].0 == p, "{case}: {objs:?}");
            let v = vertices(&objs[0].1);
            assert!(v.len() == 3 && near(v[1].0, 10.0) && near(v[2].0, 15.0) && near(v[2].1, 5.0), "{case}: {v:?}");
            assert!(near(v[1].4, std::f64::consts::FRAC_PI_8.tan()), "{case}: {v:?}");
        } else {
            assert_eq!(objs, before, "{case}");
        }
    }
    // A polyline of one vertex has no segment to join: the source is the first object that has
    // one, for the way it faces as for its properties.
    let mut s = Session::new();
    let dot = poly(&mut s, 50.0, 0.0, true);
    s.doc_mut()
        .unwrap()
        .modify_entity(dot, |e| {
            if let EntityKind::LwPolyline(p) = &mut e.kind {
                p.vertices.truncate(1);
            }
        })
        .unwrap();
    let p = poly(&mut s, 0.0, 0.0, false);
    let l = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
    assert_eq!(join(&mut s, &[dot, p, l]), p);
    assert_eq!(objects(&s).iter().map(|o| o.0).collect::<Vec<_>>(), [dot, p]);
    // JOIN onto a line source takes a collinear line with normal 0,0,-1 (AutoCAD 2024, measured:
    // "1 line joined to source").
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [20.0, 0.0]);
    face_down(&mut s, b);
    assert_eq!(join(&mut s, &[a, b]), a);
    let objs = objects(&s);
    assert!(objs.len() == 1 && matches!(&objs[0].1, EntityKind::Line(l) if l.b.x == 20.0), "{objs:?}");
    // Onto a line source, objects facing the other way weren't measured: the core console's JOIN
    // takes only collinear lines onto one. An arc facing down still joins it into a new polyline,
    // as before.
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = arc(&mut s, [10.0, 5.0], 5.0, 270.0, 0.0);
    face_down(&mut s, b);
    join(&mut s, &[a, b]);
    let objs = objects(&s);
    assert!(objs.len() == 1 && vertices(&objs[0].1).len() == 3, "{objs:?}");
}
