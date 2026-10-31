//! TRIM keeps what the object has beyond its outline, as BREAK does: a line's Z, an arc's centre
//! Z (a circle trimmed into an arc), a polyline's elevation, and a polyline's constant width on
//! every piece. The Z cases and the width on BREAK were measured in AutoCAD 2024 (accoreconsole)
//! on these drawings.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Circle, Common, Entity, EntityKind, Handle, Line, LwPolyline};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use serde_json::json;

fn add(s: &mut Session, kind: EntityKind, thickness: f64) -> Handle {
    let space = s.space();
    let d = s.doc_mut().unwrap();
    let handle = d.new_handle();
    d.space_mut(&space).unwrap().push(Entity { handle, common: Common { thickness, ..Common::default() }, kind });
    handle
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let v = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(v["handles"][0].as_str().unwrap()).unwrap()
}

fn trim(s: &mut Session, h: Handle, pick: [f64; 2], edge: Handle) {
    s.execute("trim", &json!({ "handle": h.hex(), "pick": pick, "edges": [edge.hex()] })).unwrap();
}

fn kind(s: &Session, h: Handle) -> EntityKind {
    s.doc().unwrap().entity(h).unwrap().kind.clone()
}

fn polyline(vertices: &[(f64, f64, f64)], const_width: f64, elevation: f64, plinegen: bool) -> EntityKind {
    let vertices = vertices.iter().map(|&(x, y, bulge)| PolyVertex { bulge, ..PolyVertex::new(Vec2::new(x, y)) }).collect();
    EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width, elevation, plinegen })
}

fn near(a: Vec3, b: Vec3, tol: f64) -> bool {
    (a.x - b.x).abs() < tol && (a.y - b.y).abs() < tol && (a.z - b.z).abs() < tol
}

#[test]
fn trim_keeps_a_lines_z() {
    let mut s = Session::new();
    let a = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.5, 0.25, 0.007), b: Vec3::new(0.54, 0.25, 0.007) }), 0.0);
    let b = line(&mut s, [0.52, 0.24], [0.52, 0.26]);
    trim(&mut s, a, [0.53, 0.25], b);
    let EntityKind::Line(l) = kind(&s, a) else { panic!("not a line") };
    assert!(near(l.a, Vec3::new(0.5, 0.25, 0.007), 1e-12) && near(l.b, Vec3::new(0.52, 0.25, 0.007), 1e-12), "{l:?}");
}

#[test]
fn trim_keeps_the_centre_z_of_a_circle_made_an_arc() {
    // At survey coordinates, with a thickness of 2.
    let mut s = Session::new();
    let c = add(&mut s, EntityKind::Circle(Circle { center: Vec3::new(612345.678, 2398765.432, 5.0), radius: 10.0 }), 2.0);
    let b = line(&mut s, [612350.678, 2398745.432], [612350.678, 2398785.432]);
    trim(&mut s, c, [612335.678, 2398765.432], b);
    let e = s.doc().unwrap().entity(c).unwrap().clone();
    let EntityKind::Arc(a) = &e.kind else { panic!("not an arc: {:?}", e.kind) };
    assert!(near(a.center, Vec3::new(612345.678, 2398765.432, 5.0), 1e-9), "{a:?}");
    assert!((a.start - 5.235987755978257).abs() < 1e-9 && (a.end - 1.047197551201328).abs() < 1e-9, "{a:?}");
    assert_eq!(e.common.thickness, 2.0);
}

#[test]
fn trim_keeps_a_polylines_elevation() {
    let mut s = Session::new();
    let p = add(&mut s, polyline(&[(0.0, 0.0, 0.0), (20.0, 0.0, 0.4), (30.0, 15.0, 0.0), (5.0, 20.0, 0.0)], 0.0, 12.5, false), 1.0);
    let b = line(&mut s, [10.0, -10.0], [10.0, 30.0]);
    trim(&mut s, p, [5.0, 0.0], b);
    let EntityKind::LwPolyline(l) = kind(&s, p) else { panic!("not a polyline") };
    assert_eq!(l.elevation, 12.5);
    let want = [(10.0, 0.0, 0.0), (20.0, 0.0, 0.4), (30.0, 15.0, 0.0), (5.0, 20.0, 0.0)];
    assert_eq!(l.vertices.len(), want.len());
    for (v, w) in l.vertices.iter().zip(want) {
        assert!(near(Vec3::new(v.p.x, v.p.y, v.bulge), Vec3::new(w.0, w.1, w.2), 1e-12), "{v:?} != {w:?}");
    }
}

#[test]
fn break_and_trim_keep_a_polylines_constant_width_on_every_piece() {
    // Measured for BREAK: a width-2 polyline broken from (705, 0) to (735, 0) leaves two width-2
    // pieces. TRIM leaves its pieces the same way. Linetype generation is the polyline's setting
    // too, so the pieces keep it (not measured).
    let shape = [(700.0, 0.0, 0.0), (720.0, 0.0, 0.0), (740.0, 0.0, 0.0)];
    let mut s = Session::new();
    let p = add(&mut s, polyline(&shape, 2.0, 0.0, true), 0.0);
    let r = s.execute("break", &json!({ "handle": p.hex(), "p1": [705, 0], "p2": [735, 0] })).unwrap();
    let mut s2 = Session::new();
    let q = add(&mut s2, polyline(&shape, 2.0, 0.0, true), 0.0);
    let k1 = line(&mut s2, [705.0, -5.0], [705.0, 5.0]);
    let k2 = line(&mut s2, [735.0, -5.0], [735.0, 5.0]);
    let t = s2.execute("trim", &json!({ "handle": q.hex(), "pick": [720, 0], "edges": [k1.hex(), k2.hex()] })).unwrap();
    for (s, r) in [(&s, &r), (&s2, &t)] {
        let hs = r["handles"].as_array().unwrap();
        assert_eq!(hs.len(), 2);
        for h in hs {
            let EntityKind::LwPolyline(l) = kind(s, Handle::parse_hex(h.as_str().unwrap()).unwrap()) else { panic!("not a polyline") };
            assert_eq!((l.const_width, l.plinegen), (2.0, true));
        }
    }
}
