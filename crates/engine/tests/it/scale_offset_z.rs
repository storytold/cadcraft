//! SCALE scales heights too and OFFSET keeps them, as AutoCAD 2024 does (measured with
//! accoreconsole, running these scripts on these drawings): SCALE scales every Z and elevation
//! about the base point's Z (the current elevation for a point given without one) and every
//! thickness by the factor; OFFSET's copy is at the source's height, and a circle's or arc's copy
//! has no thickness while a polyline's keeps it.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Arc, Circle, Common, EntityKind, Handle, Line, LwPolyline, Point};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use serde_json::json;

fn add(s: &mut Session, kind: EntityKind, thickness: f64) -> Handle {
    let space = s.space();
    s.doc_mut().unwrap().add(&space, Common { thickness, ..Common::default() }, kind).unwrap()
}

fn lisp(s: &mut Session, expr: &str) {
    s.cmdline(expr).unwrap();
    assert!(s.running.is_none(), "{expr}: {}", s.prompt_text());
}

fn get(s: &Session, h: Handle) -> (EntityKind, f64) {
    let e = s.doc().unwrap().entity(h).unwrap().clone();
    (e.kind.clone(), e.common.thickness)
}

fn last(s: &Session) -> (EntityKind, f64) {
    let e = s.doc().unwrap().model.iter().last().unwrap().clone();
    (e.kind.clone(), e.common.thickness)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

fn near3(a: Vec3, b: Vec3) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
}

fn scale(s: &mut Session, h: Handle, base: &str) {
    lisp(s, &format!("(command \"_.SCALE\" (handent \"{}\") \"\" '({base}) 1.7)", h.hex()));
}

#[test]
fn scale_scales_a_lines_z() {
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 7.0), b: Vec3::new(40.0, 0.0, 7.0) }), 0.0);
    scale(&mut s, h, "2.0 3.0");
    let EntityKind::Line(l) = get(&s, h).0 else { panic!() };
    assert!(near3(l.a, Vec3::new(-1.4, -2.1, 11.9)) && near3(l.b, Vec3::new(66.6, -2.1, 11.9)), "{l:?}");
    // A sloped line at survey coordinates.
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(612345.678, 2398765.432, 3.0), b: Vec3::new(612375.678, 2398775.432, 8.0) }), 0.0);
    scale(&mut s, h, "612347.678 2398768.432");
    let EntityKind::Line(l) = get(&s, h).0 else { panic!() };
    assert!(near3(l.a, Vec3::new(612344.278, 2398763.332, 5.1)) && near3(l.b, Vec3::new(612395.278, 2398780.332, 13.6)), "{l:?}");
}

#[test]
fn scale_scales_the_centre_z_and_thickness_of_circles_and_arcs() {
    let mut s = Session::new();
    let c = add(&mut s, EntityKind::Circle(Circle { center: Vec3::new(0.5, 0.25, 0.005), radius: 0.01 }), 0.002);
    scale(&mut s, c, "0.502 0.253");
    let (EntityKind::Circle(k), t) = get(&s, c) else { panic!() };
    assert!(near3(k.center, Vec3::new(0.4986, 0.2479, 0.0085)) && close(k.radius, 0.017) && close(t, 0.0034), "{k:?} {t}");
    let mut s = Session::new();
    let arc = Arc { center: Vec3::new(0.0, 0.0, 4.5), radius: 10.0, start: 30f64.to_radians(), end: 200f64.to_radians() };
    let a = add(&mut s, EntityKind::Arc(arc), 1.5);
    scale(&mut s, a, "2.0 3.0");
    let (EntityKind::Arc(k), t) = get(&s, a) else { panic!() };
    assert!(near3(k.center, Vec3::new(-1.4, -2.1, 7.65)) && close(k.radius, 17.0) && close(t, 2.55), "{k:?} {t}");
}

#[test]
fn scale_scales_a_polylines_elevation_and_thickness_and_a_points_z() {
    let mut s = Session::new();
    let vs = [(612345.678, 2398765.432, 0.0), (612365.678, 2398765.432, 0.4), (612375.678, 2398780.432, 0.0), (612350.678, 2398785.432, 0.0)];
    let vertices = vs.iter().map(|&(x, y, b)| PolyVertex { bulge: b, ..PolyVertex::new(Vec2::new(x, y)) }).collect();
    let p = add(&mut s, EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 12.5, plinegen: false }), 1.0);
    scale(&mut s, p, "612347.678 2398768.432");
    let (EntityKind::LwPolyline(k), t) = get(&s, p) else { panic!() };
    assert!(close(k.elevation, 21.25) && close(t, 1.7), "{} {t}", k.elevation);
    let first = k.vertices.first().unwrap().p;
    assert!(close(first.x, 612344.278) && close(first.y, 2398763.332), "{first:?}");
    let pt = add(&mut s, EntityKind::Point(Point { p: Vec3::new(5.0, 5.0, 6.0), angle: 0.0 }), 0.0);
    scale(&mut s, pt, "2.0 3.0");
    let EntityKind::Point(k) = get(&s, pt).0 else { panic!() };
    assert!(near3(k.p, Vec3::new(7.1, 6.4, 10.2)), "{k:?}");
}

#[test]
fn scale_scales_heights_about_the_current_elevation() {
    // ELEVATION 5: the base point, given without a Z, is at Z = 5.
    let mut s = Session::new();
    s.doc_mut().unwrap().header.set_f64("ELEVATION", 5.0);
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 7.0), b: Vec3::new(40.0, 0.0, 7.0) }), 0.0);
    scale(&mut s, h, "2.0 3.0");
    let EntityKind::Line(l) = get(&s, h).0 else { panic!() };
    assert!(near3(l.a, Vec3::new(-1.4, -2.1, 8.4)) && near3(l.b, Vec3::new(66.6, -2.1, 8.4)), "{l:?}");
    let c = add(&mut s, EntityKind::Circle(Circle { center: Vec3::new(0.0, 0.0, 4.5), radius: 10.0 }), 1.5);
    scale(&mut s, c, "2.0 3.0");
    let (EntityKind::Circle(k), t) = get(&s, c) else { panic!() };
    assert!(near3(k.center, Vec3::new(-1.4, -2.1, 4.15)) && close(k.radius, 17.0) && close(t, 2.55), "{k:?} {t}");
    let vs = [(0.0, 0.0, 0.0), (20.0, 0.0, 0.4), (30.0, 15.0, 0.0)];
    let vertices = vs.iter().map(|&(x, y, b)| PolyVertex { bulge: b, ..PolyVertex::new(Vec2::new(x, y)) }).collect();
    let p = add(&mut s, EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 12.5, plinegen: false }), 1.0);
    scale(&mut s, p, "2.0 3.0");
    let (EntityKind::LwPolyline(k), t) = get(&s, p) else { panic!() };
    assert!(close(k.elevation, 17.75) && close(t, 1.7), "{} {t}", k.elevation);
    let last = k.vertices.last().unwrap().p;
    assert!(close(last.x, 49.6) && close(last.y, 23.4), "{last:?}");
}

#[test]
fn json_scale_takes_the_base_points_z() {
    let line = || EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 7.0), b: Vec3::new(40.0, 0.0, 7.0) });
    let z = |s: &Session, h: Handle| {
        let EntityKind::Line(l) = get(s, h).0 else { panic!() };
        (l.a.z, l.b.z)
    };
    let mut s = Session::new();
    let h = add(&mut s, line(), 0.0);
    s.execute("scale", &json!({ "handles": [h.hex()], "base": [2.0, 3.0, 1.0], "factor": 1.7 })).unwrap();
    let (a, b) = z(&s, h);
    assert!(close(a, 11.2) && close(b, 11.2), "{a} {b}");
    // Without one, the current elevation (not measured through JSON: the same base point as the
    // command line's).
    let mut s = Session::new();
    s.doc_mut().unwrap().header.set_f64("ELEVATION", 5.0);
    let h = add(&mut s, line(), 0.0);
    s.execute("scale", &json!({ "handles": [h.hex()], "base": [2.0, 3.0], "factor": 1.7 })).unwrap();
    let (a, b) = z(&s, h);
    assert!(close(a, 8.4) && close(b, 8.4), "{a} {b}");
    // A Z that isn't a number is refused, the drawing left as it was.
    assert!(s.execute("scale", &json!({ "handles": [h.hex()], "base": [2.0, 3.0, "1"], "factor": 1.7 })).is_err());
    assert_eq!(z(&s, h), (a, b));
}

#[test]
fn move_and_rotate_leave_heights_alone() {
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 7.0), b: Vec3::new(40.0, 0.0, 7.0) }), 2.0);
    lisp(&mut s, &format!("(command \"_.ROTATE\" (handent \"{}\") \"\" '(0.0 0.0) 90)", h.hex()));
    lisp(&mut s, &format!("(command \"_.MOVE\" (handent \"{}\") \"\" '(0.0 0.0) '(5.0 5.0))", h.hex()));
    let (EntityKind::Line(l), t) = get(&s, h) else { panic!() };
    assert!(close(l.a.z, 7.0) && close(l.b.z, 7.0) && close(t, 2.0), "{l:?} {t}");
}

#[test]
fn offset_keeps_a_lines_z() {
    let mut s = Session::new();
    let h = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 7.0), b: Vec3::new(40.0, 0.0, 7.0) }), 0.0);
    lisp(&mut s, &format!("(command \"_.OFFSET\" 3.0 (list (handent \"{}\") '(20.0 0.0)) '(10.0 5.0) \"\")", h.hex()));
    let EntityKind::Line(l) = last(&s).0 else { panic!() };
    assert!(near3(l.a, Vec3::new(0.0, 3.0, 7.0)) && near3(l.b, Vec3::new(40.0, 3.0, 7.0)), "{l:?}");
}

#[test]
fn offset_keeps_the_centre_z_of_circles_and_arcs_but_not_their_thickness() {
    let mut s = Session::new();
    let c = add(&mut s, EntityKind::Circle(Circle { center: Vec3::new(612345.678, 2398765.432, 5.0), radius: 10.0 }), 2.0);
    lisp(&mut s, &format!("(command \"_.OFFSET\" 3.0 (list (handent \"{}\") '(612355.678 2398765.432)) '(612345.678 2398780.432) \"\")", c.hex()));
    let (EntityKind::Circle(k), t) = last(&s) else { panic!() };
    assert!(close(k.center.z, 5.0) && close(k.radius, 13.0) && t == 0.0, "{k:?} {t}");
    let mut s = Session::new();
    let arc = Arc { center: Vec3::new(-35000.0, 81000.0, 225.0), radius: 500.0, start: 30f64.to_radians(), end: 200f64.to_radians() };
    let a = add(&mut s, EntityKind::Arc(arc), 75.0);
    lisp(&mut s, &format!("(command \"_.OFFSET\" 150.0 (list (handent \"{}\") '(-35000.0 81500.0)) '(-35000.0 81750.0) \"\")", a.hex()));
    let (EntityKind::Arc(k), t) = last(&s) else { panic!() };
    assert!(close(k.center.z, 225.0) && close(k.radius, 650.0) && t == 0.0, "{k:?} {t}");
}

#[test]
fn offset_keeps_a_polylines_elevation_and_thickness() {
    let mut s = Session::new();
    let vs = [(0.0, 0.0, 0.0), (20.0, 0.0, 0.4), (30.0, 15.0, 0.0), (5.0, 20.0, 0.0)];
    let vertices = vs.iter().map(|&(x, y, b)| PolyVertex { bulge: b, ..PolyVertex::new(Vec2::new(x, y)) }).collect();
    let p = add(&mut s, EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 12.5, plinegen: false }), 1.0);
    lisp(&mut s, &format!("(command \"_.OFFSET\" 3.0 (list (handent \"{}\") '(10.0 0.0)) '(12.0 6.0) \"\")", p.hex()));
    let (EntityKind::LwPolyline(k), t) = last(&s) else { panic!() };
    assert!(close(k.elevation, 12.5) && close(t, 1.0), "{} {t}", k.elevation);
    let first = k.vertices.first().unwrap().p;
    assert!(close(first.x, 0.0) && close(first.y, 3.0), "{first:?}");
}
