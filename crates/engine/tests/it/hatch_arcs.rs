//! A hatch made by picking an internal point keeps the arcs of its boundary (#1167): the traced
//! outline comes from flattened curves, and its runs along a circle, an arc or a polyline's arc
//! segment become arcs again, so AREA on the hatch equals AREA on the objects. Only edges traced
//! from those circular pieces become arcs: a line or an ellipse near a circle stays as it is.

use std::f64::consts::PI;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Hatch, HatchLoop};
use cadcraft_engine::geom::Polyline;
use serde_json::{Value, json};

fn handle(v: &Value) -> String {
    v["handle"].as_str().or_else(|| v["handles"][0].as_str()).unwrap().to_string()
}

fn hatch(s: &Session, h: &str) -> Hatch {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn pl(l: &HatchLoop) -> Polyline {
    Polyline { vertices: l.vertices.clone(), closed: true }
}

/// Filled area, exact with bulges: outer loops minus islands.
fn area(h: &Hatch) -> f64 {
    h.loops.iter().map(|l| pl(l).area().abs() * if l.outer { 1.0 } else { -1.0 }).sum()
}

fn bulges(l: &HatchLoop) -> usize {
    l.vertices.iter().filter(|v| v.bulge.abs() > 1e-9).count()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * (1.0 + b.abs())
}

fn hatch_at(s: &mut Session, points: &[[f64; 2]]) -> String {
    handle(&s.execute("hatch", &json!({ "points": points, "pattern": "SOLID" })).unwrap())
}

#[test]
fn a_circle_hatched_inside_is_two_half_circles_with_the_circles_area() {
    let mut s = Session::new();
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 40 })).unwrap());
    let h = {
        let hh = hatch_at(&mut s, &[[50.0, 50.0]]);
        hatch(&s, &hh)
    };
    let l = &h.loops[0];
    assert_eq!(l.vertices.len(), 2, "{:?}", l.vertices);
    assert!(l.vertices.iter().all(|v| (v.bulge.abs() - 1.0).abs() < 1e-12));
    assert!(close(area(&h), PI * 1600.0), "{}", area(&h));
    assert!(h.associative && l.sources.iter().any(|x| x.hex() == c));
    // Typed at the command line, as in the report.
    let mut s = Session::new();
    s.script("CIRCLE 50,50 40\n-HATCH P SOLID\n50,50\n\n").unwrap();
    let h = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some(x.clone()) } else { None }).unwrap();
    assert!(close(area(&h), PI * 1600.0), "{}", area(&h));
}

#[test]
fn a_rounded_rectangle_keeps_its_arc_as_one_polyline_or_as_lines_and_an_arc() {
    // 100 x 50 with a semicircular right side: 5000 + π·25²/2.
    let want = 5000.0 + PI * 625.0 / 2.0;
    let mut s = Session::new();
    s.execute("pline", &json!({ "vertices": [[0, 0], { "p": [100, 0], "bulge": 1.0 }, [100, 50], [0, 50]], "closed": true })).unwrap();
    let h = {
        let hh = hatch_at(&mut s, &[[50.0, 25.0]]);
        hatch(&s, &hh)
    };
    assert_eq!(h.loops[0].vertices.len(), 4, "{:?}", h.loops[0].vertices);
    assert_eq!(bulges(&h.loops[0]), 1);
    assert!(close(area(&h), want), "{}", area(&h));
    // The same shape as three lines and an arc, then moved with its objects.
    let mut s = Session::new();
    let mut objs = Vec::new();
    for (a, b) in [([0, 0], [100, 0]), ([100, 50], [0, 50]), ([0, 50], [0, 0])] {
        objs.push(handle(&s.execute("line", &json!({ "points": [a, b] })).unwrap()));
    }
    objs.push(handle(&s.execute("arc", &json!({ "center": [100, 25], "radius": 25, "start": -90, "end": 90 })).unwrap()));
    let hh = hatch_at(&mut s, &[[50.0, 25.0]]);
    let h = hatch(&s, &hh);
    assert_eq!(bulges(&h.loops[0]), 1);
    assert!(close(area(&h), want), "{}", area(&h));
    objs.push(hh.clone());
    s.execute("move", &json!({ "handles": objs, "delta": [7, 3] })).unwrap();
    let h = hatch(&s, &hh);
    assert_eq!(bulges(&h.loops[0]), 1, "{:?}", h.loops[0].vertices);
    assert!(close(area(&h), want), "{}", area(&h));
}

/// A circle of radius 10 at the origin cut by a line `y = c`: the cap above it and the rest.
fn cut_circle(c: f64) -> (Session, f64, f64) {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap();
    s.execute("line", &json!({ "points": [[-20, c], [20, c]] })).unwrap();
    let theta = 2.0 * (c / 10.0).acos();
    let cap = 50.0 * (theta - theta.sin());
    (s, cap, PI * 100.0 - cap)
}

#[test]
fn a_circle_cut_by_a_line_gives_the_exact_segments_even_for_a_shallow_cut() {
    // (A cut shallower than the flattening tolerance finds no boundary, as before.)
    for c in [3.0, 9.9, 9.99] {
        let (mut s, cap, rest) = cut_circle(c);
        let top = {
            let hh = hatch_at(&mut s, &[[0.0, (c + 10.0) / 2.0]]);
            hatch(&s, &hh)
        };
        let bottom = {
            let hh = hatch_at(&mut s, &[[0.0, 0.0]]);
            hatch(&s, &hh)
        };
        assert!(close(area(&top), cap), "cap at {c}: {} vs {cap}", area(&top));
        assert!(close(area(&bottom), rest), "rest at {c}: {} vs {rest}", area(&bottom));
        // Every point of both outlines stays on or inside the circle and on its side of the line.
        for (h, above) in [(&top, true), (&bottom, false)] {
            for q in pl(&h.loops[0]).tessellate(1e-4) {
                assert!(q.len() <= 10.0 + 1e-6, "{q:?} outside the circle (cut at {c})");
                assert!(if above { q.y >= c - 1e-6 } else { q.y <= c + 1e-6 }, "{q:?} past the line (cut at {c})");
            }
        }
    }
}

#[test]
fn an_ellipse_or_a_line_next_to_a_circle_is_not_taken_for_it() {
    // An ellipse a hair wider than the circle crosses it four times; the region inside both is
    // bounded by circle arcs and ellipse pieces, which stay flattened.
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap();
    s.execute("ellipse", &json!({ "center": [0, 0], "major": [10.05, 0], "ratio": 0.99 })).unwrap();
    let h = {
        let hh = hatch_at(&mut s, &[[0.0, 0.0]]);
        hatch(&s, &hh)
    };
    // Within the flattening tolerance of both curves, as the flattened outline was: where the
    // two cross at a shallow angle the corner is only known to that tolerance.
    let tol = 2.0 * (s.doc().unwrap().extents(&cadcraft_engine::doc::Space::Model).width() + 20.1) / 20000.0;
    for q in pl(&h.loops[0]).tessellate(1e-4) {
        assert!(q.len() <= 10.0 + tol, "{q:?} outside the circle");
        let r = 10.05 * (q.x * q.x + q.y * q.y / (0.99 * 0.99)).sqrt() / 10.05;
        assert!(r <= 10.05 + tol, "{q:?} outside the ellipse");
    }
    // A line just outside the circle, tangent-close: the hatch inside the circle is the circle.
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap();
    s.execute("line", &json!({ "points": [[-20, 10.0001], [20, 10.0001]] })).unwrap();
    let h = {
        let hh = hatch_at(&mut s, &[[0.0, 0.0]]);
        hatch(&s, &hh)
    };
    assert!(close(area(&h), PI * 100.0), "{}", area(&h));
}

#[test]
fn one_hatch_inside_and_around_a_circle_fills_both() {
    // Picked inside the circle and in the square around it: the circle is filled too.
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [100, 100] })).unwrap();
    s.execute("circle", &json!({ "center": [50, 50], "radius": 20 })).unwrap();
    let hh = hatch_at(&mut s, &[[50.0, 50.0], [5.0, 5.0]]);
    let h = hatch(&s, &hh);
    assert!(close(filled(&h), 10000.0), "{} from {:?}", filled(&h), h.loops.iter().map(|l| (l.outer, l.vertices.len())).collect::<Vec<_>>());
    // As drawn and as AREA measures it: the circle's outer loop and island cancel, the square
    // is filled whole.
    let drawn = cadcraft_engine::render::hatch_fill_loops(&h, 1e-3);
    let centre = cadcraft_engine::geom::Vec2::new(50.0, 50.0);
    assert_eq!(drawn.iter().filter(|l| cadcraft_engine::geom::point_in_polygon(l, centre)).count() % 2, 1, "the circle is filled");
    let a = s.execute("area", &json!({ "handle": hh })).unwrap()["area"].as_f64().unwrap();
    assert!(close(a, 10000.0), "{a}");
    // The same with a square inside the square (exact on main too, so merged there).
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [100, 100] })).unwrap();
    s.execute("rectang", &json!({ "p1": [40, 40], "p2": [60, 60] })).unwrap();
    let hh = hatch_at(&mut s, &[[50.0, 50.0], [5.0, 5.0]]);
    let h = hatch(&s, &hh);
    assert!(close(filled(&h), 10000.0), "{}", filled(&h));
}

/// The area the even-odd fill covers: points on a fine grid inside an odd number of loops.
fn filled(h: &Hatch) -> f64 {
    let polys: Vec<Vec<cadcraft_engine::geom::Vec2>> = h.loops.iter().map(|l| pl(l).tessellate(1e-3)).collect();
    let n = 400;
    let step = 100.0 / n as f64;
    let mut inside = 0usize;
    for i in 0..n {
        for j in 0..n {
            let q = cadcraft_engine::geom::Vec2::new((i as f64 + 0.5) * step, (j as f64 + 0.5) * step);
            if polys.iter().filter(|p| cadcraft_engine::geom::point_in_polygon(p, q)).count() % 2 == 1 {
                inside += 1;
            }
        }
    }
    inside as f64 * step * step
}

/// HPORIGINMODE's corners take the circle's extents, not only its two vertices (a circle kept as
/// two half-circles spans its whole height).
#[test]
fn a_circles_hatch_origin_takes_its_extents() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [50, 50], "radius": 40 })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[50, 50]], "pattern": "ANSI31", "originMode": "bottomLeft" })).unwrap());
    let o = hatch(&s, &h).origin;
    assert!(close(o.x, 10.0) && close(o.y, 10.0), "{o:?}");
    let h = handle(&s.execute("hatch", &json!({ "points": [[50, 50]], "pattern": "ANSI31", "originMode": "topRight" })).unwrap());
    let o = hatch(&s, &h).origin;
    assert!(close(o.x, 90.0) && close(o.y, 90.0), "{o:?}");
}

/// The polyline BOUNDARY makes keeps the arcs: a circle gives two half-circles, a rounded
/// rectangle one arc segment, each with the object's exact area.
#[test]
fn boundary_keeps_the_arcs() {
    let lw = |s: &Session, h: &str| -> Polyline {
        match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
            EntityKind::LwPolyline(p) => Polyline { vertices: p.vertices.clone(), closed: p.closed },
            k => panic!("not a polyline: {k:?}"),
        }
    };
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [50, 50], "radius": 40 })).unwrap();
    let b = handle(&s.execute("boundary", &json!({ "points": [[50, 50]] })).unwrap());
    let p = lw(&s, &b);
    assert_eq!(p.vertices.len(), 2, "{:?}", p.vertices);
    assert!(p.vertices.iter().all(|v| (v.bulge.abs() - 1.0).abs() < 1e-12));
    assert!(close(p.area().abs(), PI * 1600.0), "{}", p.area());

    let mut s = Session::new();
    s.execute("pline", &json!({ "vertices": [[0, 0], { "p": [100, 0], "bulge": 1.0 }, [100, 50], [0, 50]], "closed": true })).unwrap();
    let b = handle(&s.execute("boundary", &json!({ "points": [[50, 25]] })).unwrap());
    let p = lw(&s, &b);
    assert_eq!(p.vertices.iter().filter(|v| v.bulge.abs() > 1e-9).count(), 1, "{:?}", p.vertices);
    assert!(close(p.area().abs(), 5000.0 + PI * 625.0 / 2.0), "{}", p.area());
}

/// HATCHEDIT adding a boundary the hatch already has (its island) changes nothing: a second copy
/// of the island would cancel it and fill the circle.
#[test]
fn hatchedit_adding_an_existing_island_keeps_it() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [100, 100] })).unwrap();
    let c = handle(&s.execute("circle", &json!({ "center": [50, 50], "radius": 20 })).unwrap());
    // Not associative: its island is linked to nothing until the circle is added.
    let hh = handle(&s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID", "associative": false })).unwrap());
    assert!(hatch(&s, &hh).loops.iter().all(|l| l.sources.is_empty()));
    let before = hatch(&s, &hh).loops.len();
    s.execute("hatchedit", &json!({ "handles": [hh], "addBoundary": [c] })).unwrap();
    let h = hatch(&s, &hh);
    assert_eq!(h.loops.len(), before, "{:?}", h.loops.iter().map(|l| (l.outer, l.vertices.len())).collect::<Vec<_>>());
    let drawn = cadcraft_engine::render::hatch_fill_loops(&h, 1e-3);
    let centre = cadcraft_engine::geom::Vec2::new(50.0, 50.0);
    assert_eq!(drawn.iter().filter(|l| cadcraft_engine::geom::point_in_polygon(l, centre)).count() % 2, 0, "the island stays empty");
    // The island is linked to the circle that was added.
    assert!(
        h.loops.iter().any(|l| !l.outer && l.sources.iter().any(|x| x.hex() == c)),
        "{:?}",
        h.loops.iter().map(|l| (l.outer, l.sources.clone())).collect::<Vec<_>>()
    );
}

/// AREA tests a loop's nesting at an edge's middle, as the fill does: an island whose first vertex
/// lies on the outer loop (here on its right edge, where a vertex test says "outside") is still
/// subtracted.
#[test]
fn area_subtracts_an_island_touching_the_outer_loop() {
    use cadcraft_engine::geom::{PolyVertex, Vec2};
    let lp = |pts: &[(f64, f64)], outer: bool| HatchLoop {
        vertices: pts.iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect(),
        outer,
        sources: vec![],
    };
    let mut s = Session::new();
    let h = Hatch {
        loops: vec![lp(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true), lp(&[(10.0, 5.0), (5.0, 4.0), (5.0, 6.0)], false)],
        pattern: "SOLID".into(),
        solid: true,
        scale: 1.0,
        ..hatch_template(&mut s)
    };
    let id = s.add_entity(EntityKind::Hatch(h)).unwrap();
    let a = s.execute("area", &json!({ "handle": id.hex() })).unwrap()["area"].as_f64().unwrap();
    assert!(close(a, 100.0 - 5.0), "{a}");
}

/// Any hatch's fields, to build one by hand.
fn hatch_template(s: &mut Session) -> Hatch {
    s.execute("rectang", &json!({ "p1": [200, 200], "p2": [210, 210] })).unwrap();
    let hh = hatch_at(s, &[[205.0, 205.0]]);
    hatch(s, &hh)
}
