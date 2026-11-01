//! ID, DIST and LIST report Z: the Z of what is drawn at a picked point (a survey point, a 3D
//! line, a contour polyline's elevation, a TIN face), and the Z of the objects LIST shows. They
//! all said Z = 0, so a topographic drawing's levels couldn't be read in CADCraft.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Face3d, Handle, Line, LwPolyline, Point};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use serde_json::json;

fn add(s: &mut Session, k: EntityKind) -> Handle {
    s.add_entity(k).unwrap()
}

fn z(s: &mut Session, at: [f64; 2]) -> f64 {
    s.execute("id", &json!({ "at": at })).unwrap()["z"].as_f64().unwrap()
}

fn contour(elevation: f64) -> EntityKind {
    EntityKind::LwPolyline(LwPolyline {
        vertices: vec![PolyVertex::new(Vec2::new(0.0, 50.0)), PolyVertex::new(Vec2::new(40.0, 55.0)), PolyVertex::new(Vec2::new(80.0, 50.0))],
        closed: false,
        const_width: 0.0,
        elevation,
        plinegen: false,
    })
}

#[test]
fn id_reports_the_z_of_what_is_at_the_point() {
    let mut s = Session::new();
    add(&mut s, EntityKind::Point(Point { p: Vec3::new(10.0, 10.0, 1883.546), angle: 0.0 }));
    add(&mut s, EntityKind::Line(Line { a: Vec3::new(100.0, 0.0, 10.0), b: Vec3::new(200.0, 0.0, 30.0) }));
    add(&mut s, contour(1880.0));
    // A sloped TIN face: z = 1000 + x / 10 over (300..400, 0..100).
    add(
        &mut s,
        EntityKind::Face3d(Face3d {
            corners: [Vec3::new(300.0, 0.0, 1030.0), Vec3::new(400.0, 0.0, 1040.0), Vec3::new(400.0, 100.0, 1040.0), Vec3::new(300.0, 100.0, 1030.0)],
            hidden_edges: 0,
        }),
    );
    s.cmdline("zoom e").ok();
    assert!((z(&mut s, [10.0, 10.0]) - 1883.546).abs() < 1e-9, "survey point");
    assert!((z(&mut s, [150.0, 0.0]) - 20.0).abs() < 1e-9, "middle of the 3D line");
    assert!((z(&mut s, [40.0, 55.0]) - 1880.0).abs() < 1e-9, "contour elevation");
    assert!((z(&mut s, [350.0, 0.0]) - 1035.0).abs() < 1e-9, "on the face's edge");
    // Nothing there: the current elevation.
    assert_eq!(z(&mut s, [-500.0, -500.0]), 0.0);
    s.doc_mut().unwrap().header.set_f64("ELEVATION", 7.5);
    assert_eq!(z(&mut s, [-500.0, -500.0]), 7.5);
    // A Z given with the point wins.
    assert_eq!(z_given(&mut s), 3.0);
    // At the command line it is in the message.
    s.cmdline("id").unwrap();
    s.cmdline("10,10").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Z = 1883.5460")), "{:?}", s.log.iter().rev().take(3).collect::<Vec<_>>());
}

fn z_given(s: &mut Session) -> f64 {
    s.execute("id", &json!({ "at": [10, 10, 3] })).unwrap()["z"].as_f64().unwrap()
}

#[test]
fn a_mirrored_contours_elevation_is_down_z() {
    let mut s = Session::new();
    let h = add(&mut s, contour(25.0));
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.extrusion = Vec3::new(0.0, 0.0, -1.0)).unwrap();
    s.cmdline("zoom e").ok();
    assert!((z(&mut s, [40.0, 55.0]) + 25.0).abs() < 1e-9);
}

#[test]
fn dist_between_two_levels_gives_the_3d_distance_and_the_rise() {
    let mut s = Session::new();
    add(&mut s, EntityKind::Point(Point { p: Vec3::new(0.0, 0.0, 100.0), angle: 0.0 }));
    add(&mut s, EntityKind::Point(Point { p: Vec3::new(30.0, 40.0, 150.0), angle: 0.0 }));
    s.cmdline("zoom e").ok();
    let r = s.execute("dist", &json!({ "p1": [0, 0], "p2": [30, 40] })).unwrap();
    assert!((r["dz"].as_f64().unwrap() - 50.0).abs() < 1e-9, "{r}");
    assert!((r["distance"].as_f64().unwrap() - (50.0f64 * 50.0 * 2.0).sqrt()).abs() < 1e-9, "{r}");
    assert!((r["angleFromXY"].as_f64().unwrap() - 45.0).abs() < 1e-9, "{r}");
    assert!(r["message"].as_str().unwrap().contains("Delta Z = 50.0000"), "{r}");
    // Flat points keep the 2D answer.
    let r = s.execute("dist", &json!({ "p1": [500, 500], "p2": [503, 504] })).unwrap();
    assert!((r["distance"].as_f64().unwrap() - 5.0).abs() < 1e-9 && r["dz"].as_f64().unwrap() == 0.0, "{r}");
}

#[test]
fn list_shows_the_z_values_objects_have() {
    let mut s = Session::new();
    let l = add(&mut s, EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 10.0), b: Vec3::new(30.0, 40.0, 60.0) }));
    let c = add(&mut s, contour(1880.0));
    let text = s.execute("list", &json!({ "handles": [l.hex(), c.hex()] })).unwrap()["message"].as_str().unwrap_or_default().to_string();
    let text = if text.is_empty() { s.log.join("\n") } else { text };
    assert!(text.contains("Z=10.0000") && text.contains("Z=60.0000"), "{text}");
    assert!(text.contains("Delta Z = 50.0000"), "{text}");
    assert!(text.contains("elevation   1880.0000") && text.contains("Z=1880.0000"), "{text}");
}
