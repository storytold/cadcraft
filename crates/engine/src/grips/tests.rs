use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Arc, Vec2};
use serde_json::{Value, json};

use super::{GripMode, stretch_grip};
use crate::Session;

fn h(r: &Value) -> Handle {
    Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap()
}

fn kind(s: &Session, h: Handle) -> EntityKind {
    s.doc().unwrap().entity(h).unwrap().kind.clone()
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    a.near(Vec2::new(x, y), 1e-6)
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn line_and_circle_grips() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    s.grip_edit(l, 0, v(1.0, 1.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.a.xy(), 1.0, 1.0) && near(x.b.xy(), 10.0, 0.0)));
    // Midpoint grip moves the whole line.
    let mid = s.grips_of(l).unwrap()[1];
    s.grip_edit(l, 1, mid + v(0.0, 5.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.a.xy(), 1.0, 6.0) && near(x.b.xy(), 10.0, 5.0)));
    s.execute("grip.move", &json!({"handle": l.hex(), "index": 2, "to": [20, 5]})).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 20.0, 5.0)));
    // Each edit is one undo step.
    s.undo().unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 10.0, 5.0)));

    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap());
    s.grip_edit(c, 2, v(0.0, 5.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, c), EntityKind::Circle(x) if (x.radius - 5.0).abs() < 1e-9));
    s.grip_edit(c, 0, v(3.0, 3.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, c), EntityKind::Circle(x) if near(x.center.xy(), 3.0, 3.0) && (x.radius - 5.0).abs() < 1e-9));
    assert!(s.grip_edit(c, 1, v(3.0, 3.0), GripMode::Stretch).is_err(), "zero radius refused");
    assert!(s.grip_edit(c, 99, v(3.0, 3.0), GripMode::Stretch).is_err());
}

#[test]
fn arc_and_ellipse_grips() {
    let mut s = Session::new();
    let a = h(&s.execute("arc", &json!({"center": [0, 0], "radius": 1, "start": 0, "end": 180})).unwrap());
    // Midpoint grip: three-point reshape through the new point.
    s.grip_edit(a, 1, v(0.0, 2.0), GripMode::Stretch).unwrap();
    let EntityKind::Arc(x) = kind(&s, a) else { panic!() };
    let g = Arc::new(x.center.xy(), x.radius, x.start, x.end);
    assert!(near(g.start_point(), 1.0, 0.0) && near(g.end_point(), -1.0, 0.0) && near(g.mid_point(), 0.0, 2.0));
    // End grip.
    s.grip_edit(a, 2, v(-3.0, 0.0), GripMode::Stretch).unwrap();
    let EntityKind::Arc(x) = kind(&s, a) else { panic!() };
    assert!(near(Arc::new(x.center.xy(), x.radius, x.start, x.end).end_point(), -3.0, 0.0));
    // Center grip moves.
    let c0 = s.grips_of(a).unwrap()[3];
    s.grip_edit(a, 3, c0 + v(10.0, 0.0), GripMode::Stretch).unwrap();
    let EntityKind::Arc(y) = kind(&s, a) else { panic!() };
    assert!((y.center.x - x.center.x - 10.0).abs() < 1e-9 && (y.radius - x.radius).abs() < 1e-9);

    let e = h(&s.execute("ellipse", &json!({"center": [0, 0], "major": [4, 0], "ratio": 0.5})).unwrap());
    s.grip_edit(e, 2, v(0.0, 1.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, e), EntityKind::Ellipse(x) if (x.ratio - 0.25).abs() < 1e-9));
    s.grip_edit(e, 1, v(0.0, 6.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, e), EntityKind::Ellipse(x) if near(x.major.xy(), 0.0, 6.0) && (x.ratio - 1.0 / 6.0).abs() < 1e-9));
    // Dragging the minor grip past the major length swaps the axes.
    s.grip_edit(e, 2, v(-8.0, 0.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, e), EntityKind::Ellipse(x) if (x.major.xy().len() - 8.0).abs() < 1e-9 && (x.ratio - 0.75).abs() < 1e-9));
}

#[test]
fn degenerate_ellipse_cannot_move_from_its_center_grip() {
    let ellipse = EntityKind::Ellipse(cadcraft_doc::Ellipse {
        center: cadcraft_geom::Vec3::ZERO,
        major: cadcraft_geom::Vec3::ZERO,
        ratio: 0.5,
        start: 0.0,
        end: cadcraft_geom::TAU,
    });
    assert!(super::stretch_grip(&ellipse, 0, v(1.0, 1.0)).is_none());
}

#[test]
fn polyline_grips() {
    let mut s = Session::new();
    let p = h(&s.execute("pline", &json!({"vertices": [[0, 0], {"p": [10, 0], "bulge": 0.0}, {"p": [10, 10], "bulge": 1.0}, [0, 10]]})).unwrap());
    // Grips: v0, m0, v1, m1, v2, m2, v3.
    assert_eq!(s.grips_of(p).unwrap().len(), 7);
    s.grip_edit(p, 0, v(-1.0, -1.0), GripMode::Stretch).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, p) else { panic!() };
    assert!(near(pl.vertices[0].p, -1.0, -1.0));
    // Line segment midpoint: moves the segment.
    let m1 = s.grips_of(p).unwrap()[3];
    s.grip_edit(p, 3, m1 + v(2.0, 0.0), GripMode::Stretch).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, p) else { panic!() };
    assert!(near(pl.vertices[1].p, 12.0, 0.0) && near(pl.vertices[2].p, 12.0, 10.0));
    // Arc segment midpoint: adjusts the bulge so the arc passes through the point.
    s.grip_edit(p, 5, v(6.0, 12.0), GripMode::Stretch).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, p) else { panic!() };
    assert!(near(pl.vertices[2].p, 12.0, 10.0) && near(pl.vertices[3].p, 0.0, 10.0));
    let segs = cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: false }.segments();
    assert!(segs[2].mid().near(v(6.0, 12.0), 1e-6), "{:?}", segs[2].mid());
    assert!(s.grip_edit(p, 7, v(0.0, 0.0), GripMode::Stretch).is_err());
    // Closed polylines have a closing-segment midpoint grip.
    let r = h(&s.execute("rectang", &json!({"p1": [20, 0], "p2": [24, 2]})).unwrap());
    assert_eq!(s.grips_of(r).unwrap().len(), 8);
    s.grip_edit(r, 7, v(19.0, 1.0), GripMode::Stretch).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, r) else { panic!() };
    assert!(near(pl.vertices[3].p, 19.0, 2.0) && near(pl.vertices[0].p, 19.0, 0.0));
    // A zero-length segment keeps grip indices aligned with EntityKind::grips().
    let d = h(&s.execute("pline", &json!({"vertices": [[0, 50], [0, 50], [5, 50]]})).unwrap());
    let g = s.grips_of(d).unwrap();
    assert_eq!(g.len(), 4);
    s.grip_edit(d, 3, v(5.0, 55.0), GripMode::Stretch).unwrap();
    let EntityKind::LwPolyline(pl) = kind(&s, d) else { panic!() };
    assert!(near(pl.vertices[2].p, 5.0, 55.0), "{pl:?}");
}

#[test]
fn spline_text_insert_and_dimension_grips() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({"fit": [[0, 0], [2, 2], [4, 0]]})).unwrap());
    s.grip_edit(sp, 1, v(2.0, 4.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, sp), EntityKind::Spline(x) if near(x.fit[1], 2.0, 4.0) && near(x.eval(x.domain().0), 0.0, 0.0)));
    let cv = h(&s.execute("spline", &json!({"control": [[0, 10], [2, 12], [4, 10]]})).unwrap());
    s.grip_edit(cv, 2, v(5.0, 9.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, cv), EntityKind::Spline(x) if near(x.control[2], 5.0, 9.0)));

    let t = h(&s.execute("text", &json!({"at": [1, 1], "text": "Hi", "justify": "C"})).unwrap());
    s.grip_edit(t, 0, v(3.0, 4.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, t), EntityKind::Text(x) if near(x.insert.xy(), 3.0, 4.0) && near(x.align_pt.unwrap().xy(), 3.0, 4.0)));

    let dot = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 0.1})).unwrap());
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": [dot.hex()], "keep": "delete"})).unwrap();
    let ins = h(&s.execute("insert", &json!({"name": "B", "at": [5, 5]})).unwrap());
    s.grip_edit(ins, 0, v(6.0, 7.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, ins), EntityKind::Insert(x) if near(x.insert.xy(), 6.0, 7.0)));

    let d = h(&s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 2]})).unwrap());
    let g = s.grips_of(d).unwrap();
    assert_eq!(g.len(), 4);
    s.grip_edit(d, 3, v(5.0, 6.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, d), EntityKind::Dimension(x) if x.user_text_pos && near(x.text_mid.xy(), 5.0, 6.0)));
    s.grip_edit(d, 1, v(12.0, 0.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, d), EntityKind::Dimension(x) if near(x.p14.xy(), 12.0, 0.0)));
}

#[test]
fn other_entity_grips() {
    let mut s = Session::new();
    let x = h(&s.execute("xline", &json!({"base": [0, 0], "angle": 0})).unwrap());
    s.grip_edit(x, 1, v(0.0, 3.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, x), EntityKind::XLine(r) if near(r.dir.xy(), 0.0, 1.0)));
    let p3 = h(&s.execute("3dpoly", &json!({"points": [[0, 0, 4], [1, 0, 5]]})).unwrap());
    s.grip_edit(p3, 1, v(2.0, 2.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, p3), EntityKind::Polyline3d(p) if near(p.points[1].xy(), 2.0, 2.0) && p.points[1].z == 5.0));
    let w = h(&s.execute("wipeout", &json!({"points": [[0, 0], [2, 0], [2, 2]]})).unwrap());
    s.grip_edit(w, 2, v(3.0, 3.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, w), EntityKind::Wipeout(p) if near(p.boundary[2], 3.0, 3.0)));
    let pt = h(&s.execute("point", &json!({"at": [1, 1]})).unwrap());
    s.grip_edit(pt, 0, v(2.0, 2.0), GripMode::Stretch).unwrap();
    assert!(matches!(kind(&s, pt), EntityKind::Point(p) if near(p.p.xy(), 2.0, 2.0)));
}

#[test]
fn grip_modes() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [2, 0]]})).unwrap());
    // Move about grip 0.
    s.grip_edit(l, 0, v(1.0, 1.0), GripMode::Move).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.a.xy(), 1.0, 1.0) && near(x.b.xy(), 3.0, 1.0)));
    // Rotate about grip 0 by the angle to the new point (90°).
    s.grip_edit(l, 0, v(1.0, 5.0), GripMode::Rotate).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 1.0, 3.0)));
    // Scale about grip 0 by the distance (3).
    s.grip_edit(l, 0, v(4.0, 1.0), GripMode::Scale).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 1.0, 7.0)));
    // Mirror about the line from grip 0 to (2, 1): a horizontal line through y=1.
    s.grip_edit(l, 0, v(2.0, 1.0), GripMode::Mirror).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 1.0, -5.0)));
    assert_eq!(s.state().unwrap().undo.len(), 5);
    // JSON forms with explicit base, angle, factor and copy.
    let r = s.execute("grip.rotate", &json!({"handles": [l.hex()], "base": [1, 1], "angle": 180, "copy": true})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    assert_eq!(s.doc().unwrap().model.len(), 2);
    s.execute("grip.scale", &json!({"handles": [l.hex()], "base": [1, 1], "factor": 0.5})).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 1.0, -2.0)));
    s.execute("grip.move", &json!({"handles": [l.hex()], "base": [1, 1], "to": [2, 2]})).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.a.xy(), 2.0, 2.0)));
    s.execute("grip.mirror", &json!({"handles": [l.hex()], "baseHandle": l.hex(), "index": 0, "to": [3, 2]})).unwrap();
    s.execute("grip.stretch", &json!({"handle": l.hex(), "index": 2, "to": [9, 9]})).unwrap();
    assert!(matches!(kind(&s, l), EntityKind::Line(x) if near(x.b.xy(), 9.0, 9.0)));
    assert!(s.execute("grip.scale", &json!({"handles": [l.hex()], "base": [1, 1], "factor": 0})).is_err());
    assert!(s.execute("grip.mirror", &json!({"handles": [l.hex()], "base": [1, 1], "to": [1, 1]})).is_err());
    assert!(s.execute("grip.move", &json!({"handle": l.hex(), "to": [1, 1]})).is_err(), "stretch needs an index");
}

#[test]
fn grips_on_locked_layers_and_hostile_values() {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [2, 0]]})).unwrap());
    for (i, p) in [(0usize, v(f64::NAN, 0.0)), (1, v(f64::INFINITY, 1.0)), (usize::MAX, v(1.0, 1.0))] {
        assert!(s.grip_edit(l, i, p, GripMode::Stretch).is_err());
    }
    for mode in [GripMode::Move, GripMode::Rotate, GripMode::Scale, GripMode::Mirror] {
        let _ = s.grip_edit(l, 0, v(0.0, 0.0), mode);
        let _ = s.grip_edit(l, 42, v(1e308, -1e308), mode);
    }
    s.execute("layer.new", &json!({"name": "L"})).ok();
    s.doc_mut().unwrap().ensure_layer("LOCKED");
    s.doc_mut().unwrap().layer_mut("LOCKED").unwrap().locked = true;
    s.doc_mut().unwrap().modify_entity(l, |e| e.common.layer = "LOCKED".into()).unwrap();
    assert!(s.grip_edit(l, 0, v(5.0, 5.0), GripMode::Stretch).is_err());
    // Every kind accepts every grip index without panicking.
    let mut s = Session::new();
    crate::cmd::command_specs();
    let sample = crate::sample::bracket();
    s.open_drawing(sample, "sample", None);
    let hs: Vec<(Handle, usize)> = s.doc().unwrap().model.iter().map(|e| (e.handle, e.kind.grips().len())).collect();
    for (hd, n) in hs {
        for i in 0..n + 1 {
            for to in [v(0.0, 0.0), v(1e9, -1e9), v(3.0, 4.0)] {
                if let Some(k) = s.doc().unwrap().entity(hd).map(|e| e.kind.clone()) {
                    let _ = stretch_grip(&k, i, to);
                }
            }
        }
    }
}
