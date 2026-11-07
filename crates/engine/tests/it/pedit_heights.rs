//! PEDIT on lines, arcs and polylines away from Z 0, as AutoCAD 2024 runs it (accoreconsole,
//! `entget` of the polyline made, on self-built drawings). A mirrored object here is one whose
//! normal is 0,0,-1, kept in world coordinates as the DXF reader keeps it.

use cadcraft_doc::{Entity, EntityKind, Handle, LwPolyline};
use cadcraft_engine::Session;
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().or_else(|| v["handles"][0].as_str()).unwrap()).unwrap()
}

fn entity(s: &Session, h: Handle) -> Entity {
    (**s.doc().unwrap().entity(h).unwrap()).clone()
}

fn edit(s: &mut Session, h: Handle, f: impl FnOnce(&mut Entity)) {
    s.doc_mut().unwrap().modify_entity(h, f).unwrap();
}

/// A line from `a` to `b`.
fn line(s: &mut Session, a: [f64; 3], b: [f64; 3]) -> Handle {
    let h = handle(&s.execute("line", &json!({ "points": [[a[0], a[1]], [b[0], b[1]]] })).unwrap());
    edit(s, h, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            (l.a.z, l.b.z) = (a[2], b[2]);
        }
    });
    h
}

/// An arc round `c` (world) from `a1` to `a2` degrees, counterclockwise in plan.
fn arc(s: &mut Session, c: [f64; 3], r: f64, a1: f64, a2: f64) -> Handle {
    let h = handle(&s.execute("arc", &json!({ "center": [c[0], c[1]], "radius": r, "start": a1, "end": a2 })).unwrap());
    edit(s, h, |e| {
        if let EntityKind::Arc(a) = &mut e.kind {
            a.center.z = c[2];
        }
    });
    h
}

/// An open polyline through `points` at `elevation`.
fn pline(s: &mut Session, points: Value, elevation: f64) -> Handle {
    let h = handle(&s.execute("pline", &json!({ "points": points })).unwrap());
    edit(s, h, |e| {
        if let EntityKind::LwPolyline(p) = &mut e.kind {
            p.elevation = elevation;
        }
    });
    h
}

fn face_down(s: &mut Session, h: Handle) {
    edit(s, h, |e| e.common.extrusion = Vec3::new(0.0, 0.0, -1.0));
}

fn polyline(e: &Entity) -> LwPolyline {
    let EntityKind::LwPolyline(p) = &e.kind else { panic!("not a polyline: {e:?}") };
    p.clone()
}

/// PEDIT's JSON form with nothing to change, which only turns a line or an arc into a polyline.
fn convert(s: &mut Session, h: Handle) -> Entity {
    s.execute("pedit", &json!({ "handle": h.hex(), "option": "ltypegen", "on": false })).unwrap();
    entity(s, h)
}

#[test]
fn a_line_or_an_arc_becomes_a_polyline_at_its_height() {
    // AutoCAD 2024: a line at Z 5 and an arc whose centre is at Z 5 give elevation 5 facing up;
    // a thickness of 2 stays; a line facing down still gives a polyline facing up.
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0, 5.0], [10.0, 0.0, 5.0]);
    edit(&mut s, l, |e| e.common.thickness = 2.0);
    let e = convert(&mut s, l);
    assert_eq!((polyline(&e).elevation, e.common.extrusion, e.common.thickness), (5.0, Vec3::Z, 2.0));
    let a = arc(&mut s, [10.0, 5.0, 5.0], 5.0, 270.0, 0.0);
    let p = polyline(&convert(&mut s, a));
    assert_eq!(p.elevation, 5.0);
    assert!((p.vertices[0].bulge - std::f64::consts::FRAC_PI_8.tan()).abs() < 1e-9, "{p:?}");
    let l = line(&mut s, [0.0, 20.0, 5.0], [10.0, 20.0, 5.0]);
    face_down(&mut s, l);
    let e = convert(&mut s, l);
    assert_eq!((polyline(&e).elevation, e.common.extrusion), (5.0, Vec3::Z));
    // A mirrored arc keeps its normal (AutoCAD 2024: 210 0,0,-1, elevation 5 for the arc at world
    // Z -5) and runs as it does in its own coordinates: from world (15,5) to (10,0), bulge
    // -0.414214 in plan (0.414214 in its own coordinates).
    let a = arc(&mut s, [10.0, 5.0, -5.0], 5.0, 270.0, 0.0);
    face_down(&mut s, a);
    let e = convert(&mut s, a);
    let p = polyline(&e);
    assert_eq!((p.elevation, e.common.extrusion), (5.0, Vec3::new(0.0, 0.0, -1.0)));
    let v: Vec<(f64, f64)> = p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect();
    assert!((v[0].0 - 15.0).abs() < 1e-9 && (v[0].1 - 5.0).abs() < 1e-9 && (v[1].0 - 10.0).abs() < 1e-9 && v[1].1.abs() < 1e-9, "{v:?}");
    assert!((p.vertices[0].bulge + std::f64::consts::FRAC_PI_8.tan()).abs() < 1e-9, "{p:?}");
    // A sloped line still comes out at elevation 0: AutoCAD 2024 makes a polyline in the line's
    // vertical plane (normal 0,1,0), which a CADCraft polyline can't be.
    let l = line(&mut s, [0.0, 40.0, 0.0], [10.0, 40.0, 5.0]);
    assert_eq!(polyline(&convert(&mut s, l)).elevation, 0.0);
    // A refused option leaves a line as it was, its normal included.
    let l = line(&mut s, [0.0, 60.0, 5.0], [10.0, 60.0, 5.0]);
    face_down(&mut s, l);
    let before = entity(&s, l);
    let r = s.execute("pedit", &json!({ "handle": l.hex(), "option": "vertex", "index": 0, "action": "break" }));
    assert!(r.is_err(), "{r:?}");
    assert_eq!(entity(&s, l), before);
}

/// PEDIT Join from the first object onto the second; whether the second was taken.
fn joins(s: &mut Session, first: Handle, second: Handle) -> bool {
    let r = s.execute("pedit", &json!({ "handle": first.hex(), "option": "join", "join": [second.hex()] })).unwrap();
    let taken = r["joined"] == json!(1);
    assert_eq!(taken, s.doc().unwrap().entity(second).is_none(), "{r}");
    taken
}

#[test]
fn join_takes_only_objects_at_the_polylines_height_facing_its_way() {
    // AutoCAD 2024, from a line at Z 5 ending at (10,0): "1 segments added to polyline" or "0".
    let next: [(&str, bool, fn(&mut Session) -> Handle); 6] = [
        ("a line at Z 5", true, |s| line(s, [10.0, 0.0, 5.0], [10.0, 5.0, 5.0])),
        ("a line at Z 0", false, |s| line(s, [10.0, 0.0, 0.0], [10.0, 5.0, 0.0])),
        ("an arc at Z 5", true, |s| arc(s, [10.0, 5.0, 5.0], 5.0, 270.0, 0.0)),
        ("an arc at Z 0", false, |s| arc(s, [10.0, 5.0, 0.0], 5.0, 270.0, 0.0)),
        ("a mirrored line at Z 5", true, |s| {
            let h = line(s, [10.0, 0.0, 5.0], [10.0, 5.0, 5.0]);
            face_down(s, h);
            h
        }),
        ("a mirrored arc at Z 5", false, |s| {
            let h = arc(s, [10.0, 5.0, 5.0], 5.0, 270.0, 0.0);
            face_down(s, h);
            h
        }),
    ];
    for (what, want, make) in next {
        let mut s = Session::new();
        let base = line(&mut s, [0.0, 0.0, 5.0], [10.0, 0.0, 5.0]);
        let other = make(&mut s);
        assert_eq!(joins(&mut s, base, other), want, "{what}");
        let p = polyline(&entity(&s, base));
        assert_eq!((p.elevation, p.vertices.len()), (5.0, if want { 3 } else { 2 }), "{what}");
    }
    // From a mirrored polyline at elevation 5 (world Z -5, from (0,0) to (10,0)): no line at its
    // world height, but a mirrored arc there and another mirrored polyline at elevation 5; and a
    // polyline at elevation 5 takes no polyline at 0.
    let mirrored: [(&str, bool, fn(&mut Session) -> Handle); 3] = [
        ("a line at Z -5", false, |s| line(s, [10.0, 0.0, -5.0], [10.0, 5.0, -5.0])),
        ("a mirrored arc at Z -5", true, |s| {
            let h = arc(s, [10.0, 5.0, -5.0], 5.0, 270.0, 0.0);
            face_down(s, h);
            h
        }),
        ("a mirrored polyline at elevation 5", true, |s| {
            let h = pline(s, json!([[10, 0], [10, 5]]), 5.0);
            face_down(s, h);
            h
        }),
    ];
    for (what, want, make) in mirrored {
        let mut s = Session::new();
        let base = pline(&mut s, json!([[0, 0], [10, 0]]), 5.0);
        face_down(&mut s, base);
        let other = make(&mut s);
        assert_eq!(joins(&mut s, base, other), want, "{what}");
    }
    let mut s = Session::new();
    let base = pline(&mut s, json!([[0, 0], [10, 0]]), 5.0);
    let other = pline(&mut s, json!([[10, 0], [10, 5]]), 0.0);
    assert!(!joins(&mut s, base, other), "a polyline at elevation 0");
}

#[test]
fn multiple_join_takes_objects_at_any_height_but_not_facing_the_other_way() {
    // AutoCAD 2024: PEDIT Multiple, a line at Z 5 and one at Z 0 meeting it in plan, convert and
    // Join with no fuzz: "1 segments added to polyline", one polyline at elevation 5.
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0, 5.0], [10.0, 0.0, 5.0]);
    line(&mut s, [10.0, 0.0, 0.0], [10.0, 5.0, 0.0]);
    s.cmdline("pedit m 5,0 10,2.5").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("y j 0").unwrap();
    s.cmdline("").unwrap();
    let objects: Vec<Entity> = s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect();
    assert_eq!(objects.len(), 1, "{objects:?}");
    let p = polyline(&objects[0]);
    assert_eq!((p.elevation, p.vertices.len()), (5.0, 3));
    // A line at Z 0 to (10,0) and a mirrored arc from there to (15,5), picked in either order:
    // both become polylines, the arc's facing down, and Join takes neither ("0 segments added").
    for arc_first in [false, true] {
        let mut s = Session::new();
        line(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let a = arc(&mut s, [10.0, 5.0, 0.0], 5.0, 270.0, 0.0);
        face_down(&mut s, a);
        s.cmdline(if arc_first { "pedit m 13.5,1.5 5,0" } else { "pedit m 5,0 13.5,1.5" }).unwrap();
        s.cmdline("").unwrap();
        s.cmdline("y j 0").unwrap();
        s.cmdline("").unwrap();
        let objects: Vec<Entity> = s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect();
        assert_eq!(objects.len(), 2, "arc first {arc_first}: {objects:?}");
        assert!(objects.iter().all(|e| matches!(e.kind, EntityKind::LwPolyline(_))), "{objects:?}");
    }
}

#[test]
fn join_continues_from_a_sloped_lines_far_end_height() {
    // AutoCAD 2024: from a line at Z 5, a sloped line down to Z 0 and a line on at Z 0 both join
    // ("2 segments added"), at the polyline's end and at its start alike. AutoCAD makes that
    // polyline in the lines' vertical plane; a CADCraft polyline stays at the source's elevation.
    for (base, sloped, next) in [
        ([[0.0, 0.0, 5.0], [10.0, 0.0, 5.0]], [[10.0, 0.0, 5.0], [20.0, 0.0, 0.0]], [[20.0, 0.0, 0.0], [30.0, 0.0, 0.0]]),
        ([[10.0, 0.0, 5.0], [20.0, 0.0, 5.0]], [[0.0, 0.0, 0.0], [10.0, 0.0, 5.0]], [[-10.0, 0.0, 0.0], [0.0, 0.0, 0.0]]),
    ] {
        let mut s = Session::new();
        let h = line(&mut s, base[0], base[1]);
        let a = line(&mut s, sloped[0], sloped[1]);
        let b = line(&mut s, next[0], next[1]);
        let r = s.execute("pedit", &json!({ "handle": h.hex(), "option": "join", "join": [a.hex(), b.hex()] })).unwrap();
        assert_eq!(r["joined"], json!(2), "{base:?}: {r}");
        assert_eq!(polyline(&entity(&s, h)).vertices.len(), 4);
    }
}

#[test]
fn a_chain_closes_only_at_one_height() {
    // AutoCAD 2024: three sides of a triangle at Z 5 join into a closed polyline; with its last
    // side at Z 0, that side stays a line and the polyline stays open ("1 segments added").
    for (last_z, joined, closed) in [(5.0, 2, true), (0.0, 1, false)] {
        let mut s = Session::new();
        let h = line(&mut s, [0.0, 0.0, 5.0], [10.0, 0.0, 5.0]);
        let a = line(&mut s, [10.0, 0.0, 5.0], [10.0, 10.0, 5.0]);
        let b = line(&mut s, [10.0, 10.0, last_z], [0.0, 0.0, last_z]);
        let r = s.execute("pedit", &json!({ "handle": h.hex(), "option": "join", "join": [a.hex(), b.hex()] })).unwrap();
        assert_eq!(r["joined"], json!(joined), "last side at {last_z}: {r}");
        assert_eq!(polyline(&entity(&s, h)).closed, closed, "last side at {last_z}");
    }
}

#[test]
fn the_command_line_converts_and_joins_the_same_way() {
    // PEDIT typed: a mirrored line at Z 5 becomes a polyline at elevation 5 facing up, and Join
    // leaves a line at Z 0 off it ("0 segments added to polyline").
    let mut s = Session::new();
    let h = line(&mut s, [0.0, 0.0, 5.0], [10.0, 0.0, 5.0]);
    face_down(&mut s, h);
    let other = line(&mut s, [10.0, 0.0, 0.0], [10.0, 5.0, 0.0]);
    s.cmdline("pedit 5,0 y").unwrap();
    let e = entity(&s, h);
    assert_eq!((polyline(&e).elevation, e.common.extrusion), (5.0, Vec3::Z));
    let n = s.log.len();
    s.cmdline("j 10,2.5").unwrap();
    s.cmdline("").unwrap();
    assert!(s.log[n..].iter().any(|l| l.contains("0 segments added to polyline")), "{:?}", &s.log[n..]);
    assert!(s.doc().unwrap().entity(other).is_some());
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
}
