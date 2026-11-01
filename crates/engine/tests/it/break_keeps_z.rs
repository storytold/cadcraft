//! BREAK and BREAKATPOINT keep the Z / elevation of the pieces they leave.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Arc, Common, Entity, EntityKind, Handle, Line, LwPolyline};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use serde_json::{Value, json};

fn add(s: &mut Session, kind: EntityKind) -> String {
    let space = s.space();
    let d = s.doc_mut().unwrap();
    let handle = d.new_handle();
    d.space_mut(&space).unwrap().push(Entity { handle, common: Common::default(), kind });
    handle.hex()
}

fn pieces(s: &Session, r: &Value) -> Vec<EntityKind> {
    let d = s.doc().unwrap();
    r["handles"].as_array().unwrap().iter().map(|h| d.entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap().kind.clone()).collect()
}

#[test]
fn break_keeps_line_z() {
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 5.0), b: Vec3::new(10.0, 0.0, 5.0) }));
    let r = s.execute("break", &json!({"handle": h, "p1": [4, 0], "p2": [6, 0]})).unwrap();
    let ps = pieces(&s, &r);
    assert_eq!(ps.len(), 2);
    for p in ps {
        let EntityKind::Line(l) = p else { panic!("not a line: {p:?}") };
        assert_eq!((l.a.z, l.b.z), (5.0, 5.0));
    }
}

#[test]
fn breakatpoint_keeps_line_z() {
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 5.0), b: Vec3::new(10.0, 0.0, 5.0) }));
    let r = s.execute("breakatpoint", &json!({"handle": h, "at": [4, 0]})).unwrap();
    let ps = pieces(&s, &r);
    assert_eq!(ps.len(), 2);
    for p in ps {
        let EntityKind::Line(l) = p else { panic!("not a line: {p:?}") };
        assert_eq!((l.a.z, l.b.z), (5.0, 5.0));
    }
}

#[test]
fn break_keeps_polyline_elevation() {
    let mut s = Session::new();
    let vertices = vec![PolyVertex::new(Vec2::new(0.0, 0.0)), PolyVertex::new(Vec2::new(10.0, 0.0))];
    let h = add(&mut s, EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 7.0, plinegen: false }));
    let r = s.execute("break", &json!({"handle": h, "p1": [4, 0], "p2": [6, 0]})).unwrap();
    let ps = pieces(&s, &r);
    assert_eq!(ps.len(), 2);
    for p in ps {
        let EntityKind::LwPolyline(pl) = p else { panic!("not a polyline: {p:?}") };
        assert_eq!(pl.elevation, 7.0);
    }
}

#[test]
fn break_keeps_arc_centre_z() {
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Arc(Arc { center: Vec3::new(0.0, 0.0, 3.0), radius: 5.0, start: 0.0, end: std::f64::consts::PI }));
    let p1 = [5.0 * 0.5f64.sqrt(), 5.0 * 0.5f64.sqrt()];
    let r = s.execute("break", &json!({"handle": h, "p1": p1, "p2": [0, 5]})).unwrap();
    let ps = pieces(&s, &r);
    assert_eq!(ps.len(), 2);
    for p in ps {
        let EntityKind::Arc(a) = p else { panic!("not an arc: {p:?}") };
        assert_eq!(a.center.z, 3.0);
    }
}
