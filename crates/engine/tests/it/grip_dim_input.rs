//! Grip dimensional input (#519): while one grip is stretched, the dimensions it changes can be
//! typed (DYNDIVIS, DYNDIGRIP).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::grip_dims::{GripDimKind, grip_dims, resolve};
use cadcraft_engine::{Input, sysvars};
use serde_json::json;

fn handle(r: serde_json::Value) -> Handle {
    Handle::parse_hex(r.get("handle").unwrap_or(&r["handles"][0]).as_str().unwrap()).unwrap()
}

fn grip_index(s: &Session, h: Handle, x: f64, y: f64) -> usize {
    s.grips_of(h).unwrap().iter().position(|g| g.near(Vec2::new(x, y), 1e-9)).unwrap()
}

fn kinds(s: &Session) -> Vec<GripDimKind> {
    grip_dims(s).iter().map(|d| d.kind).collect()
}

/// Lock `values` (by dimension index), resolve with the cursor at `cursor` and place the grip.
fn place(s: &mut Session, values: &[(usize, f64)], cursor: Vec2) {
    let dims = grip_dims(s);
    let p = resolve(&dims, values, cursor);
    s.input(Input::Point(p)).unwrap();
}

#[test]
fn typed_length_and_angle_place_a_line_end() {
    let mut s = Session::new();
    let h = handle(s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let end = grip_index(&s, h, 10.0, 0.0);
    s.start_grip_edit(&[(h, end)]).unwrap();
    // DYNDIVIS = 1: the first two, the resulting length and its change.
    assert_eq!(kinds(&s), [GripDimKind::Length, GripDimKind::LengthChange]);
    let dims = grip_dims(&s);
    assert_eq!(dims[0].anchor, Vec2::ZERO);
    assert!((dims[0].value(Vec2::new(0.0, 12.0)) - 12.0).abs() < 1e-12);
    // Typed: the length reads 20 and the angle follows the cursor.
    assert!((dims[0].parse(&s, "20").unwrap() - 20.0).abs() < 1e-12);
    place(&mut s, &[(0, 20.0)], Vec2::new(3.0, 4.0));
    assert!(s.running.is_none());
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => {
            assert!(l.a.xy().near(Vec2::ZERO, 1e-9));
            assert!(l.b.xy().near(Vec2::new(12.0, 16.0), 1e-9), "{:?}", l.b);
        }
        k => panic!("{k:?}"),
    }

    // DYNDIVIS = 2 shows what DYNDIGRIP asks for: angle only, then length and angle typed.
    s.execute("setvar", &json!({"name": "DYNDIVIS", "value": 2})).unwrap();
    s.execute("setvar", &json!({"name": "dyndigrip", "value": 5})).unwrap();
    let end = grip_index(&s, h, 12.0, 16.0);
    s.start_grip_edit(&[(h, end)]).unwrap();
    assert_eq!(kinds(&s), [GripDimKind::Length, GripDimKind::Angle]);
    let angle = grip_dims(&s)[1].parse(&s, "90").unwrap();
    place(&mut s, &[(0, 5.0), (1, angle)], Vec2::new(40.0, 40.0));
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => assert!(l.b.xy().near(Vec2::new(0.0, 5.0), 1e-9), "{:?}", l.b),
        k => panic!("{k:?}"),
    }
}

#[test]
fn only_a_single_stretched_grip_has_dimensions() {
    let mut s = Session::new();
    let h = handle(s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let mid = grip_index(&s, h, 5.0, 0.0);
    s.start_grip_edit(&[(h, mid)]).unwrap();
    assert!(grip_dims(&s).is_empty(), "the middle grip moves the line");
    s.cancel();
    s.start_grip_edit(&[(h, 0), (h, 2)]).unwrap();
    assert!(grip_dims(&s).is_empty(), "several hot grips");
    s.cancel();
    s.start_grip_edit(&[(h, 2)]).unwrap();
    assert!(!grip_dims(&s).is_empty());
    // Enter: MOVE mode, no dimensions.
    s.input(Input::Enter).unwrap();
    assert!(grip_dims(&s).is_empty());
    s.cancel();
    // Dynamic Input off: none.
    s.settings.dynmode = false;
    s.start_grip_edit(&[(h, 2)]).unwrap();
    assert!(grip_dims(&s).is_empty());
}

#[test]
fn circle_radius_and_polyline_segments() {
    let mut s = Session::new();
    let c = handle(s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap());
    let q = grip_index(&s, c, 5.0, 0.0);
    s.start_grip_edit(&[(c, q)]).unwrap();
    assert_eq!(kinds(&s), [GripDimKind::Radius, GripDimKind::LengthChange]);
    // Radius change +2: radius 7.
    place(&mut s, &[(1, 2.0)], Vec2::new(0.0, 1.0));
    match &s.doc().unwrap().entity(c).unwrap().kind {
        EntityKind::Circle(k) => assert!((k.radius - 7.0).abs() < 1e-9, "{k:?}"),
        k => panic!("{k:?}"),
    }

    let p = handle(s.execute("pline", &json!({ "points": [[0, 0], [10, 0], [10, 10]] })).unwrap());
    let v = grip_index(&s, p, 10.0, 0.0);
    s.start_grip_edit(&[(p, v)]).unwrap();
    // The two segments on either side of the vertex.
    let dims = grip_dims(&s);
    assert_eq!(kinds(&s), [GripDimKind::Length, GripDimKind::Length]);
    assert_eq!((dims[0].anchor, dims[1].anchor), (Vec2::ZERO, Vec2::new(10.0, 10.0)));
    // Both lengths typed: the vertex goes where they agree, on the cursor's side.
    let r = 50.0_f64.sqrt();
    place(&mut s, &[(0, r), (1, r)], Vec2::new(9.0, 1.0));
    match &s.doc().unwrap().entity(p).unwrap().kind {
        EntityKind::LwPolyline(k) => assert!(k.vertices[1].p.near(Vec2::new(5.0, 5.0), 1e-9), "{:?}", k.vertices[1].p),
        k => panic!("{k:?}"),
    }
}

#[test]
fn dyndigrip_and_dyndivis_are_checked_settings() {
    let mut s = Session::new();
    assert_eq!(sysvars::get(&s, "DYNDIGRIP"), Some(json!(31)));
    assert_eq!(sysvars::get(&s, "DYNDIVIS"), Some(json!(1)));
    for (name, bad) in [("DYNDIGRIP", json!(32)), ("DYNDIGRIP", json!(-1)), ("DYNDIVIS", json!(3)), ("DYNDIVIS", json!("x"))] {
        assert!(s.execute("setvar", &json!({"name": name, "value": bad})).is_err(), "{name} {bad}");
    }
    s.execute("setvar", &json!({"name": "DYNDIVIS", "value": 0})).unwrap();
    assert_eq!(s.settings.dyndivis, 0);
}
