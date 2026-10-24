//! An object snap gives the point the Z of what it snapped to, as in AutoCAD (OSNAPZ 0): a line
//! drawn between a survey point and a contour runs from the point's level to the contour's
//! elevation instead of lying at z = 0. OSNAPZ 1 uses the current elevation; an unsnapped pick
//! stays at z = 0. LINE and POINT keep the Z of typed points too.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, LwPolyline, Point, Space};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use cadcraft_engine::snap::{self, mode};

fn survey(s: &mut Session) {
    s.add_entity(EntityKind::Point(Point { p: Vec3::new(0.0, 0.0, 1880.0), angle: 0.0 })).unwrap();
    s.add_entity(EntityKind::LwPolyline(LwPolyline {
        vertices: vec![PolyVertex::new(Vec2::new(10.0, 0.0)), PolyVertex::new(Vec2::new(10.0, 10.0))],
        closed: false,
        const_width: 0.0,
        elevation: 1881.0,
        plinegen: false,
    }))
    .unwrap();
    s.cmdline("zoom e").ok();
}

/// Click at `at` with the object snaps in `osmode`, as the canvas does.
fn click(s: &mut Session, at: Vec2, osmode: u32) {
    let ap = s.pixel_size() * 20.0;
    let hit = snap::osnap(s.doc().unwrap(), &Space::Model, at, ap, osmode, None, false).expect("a snap");
    s.input_snapped(hit).unwrap();
}

fn lines(s: &Session) -> Vec<(Vec3, Vec3)> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a, l.b)) } else { None }).collect()
}

#[test]
fn a_line_snapped_to_a_survey_point_and_a_contour_keeps_their_levels() {
    let mut s = Session::new();
    survey(&mut s);
    s.cmdline("line").unwrap();
    click(&mut s, Vec2::new(0.01, 0.0), mode::NOD);
    click(&mut s, Vec2::new(10.0, 0.01), mode::END);
    s.input(cadcraft_engine::Input::Enter).unwrap();
    assert_eq!(lines(&s), vec![(Vec3::new(0.0, 0.0, 1880.0), Vec3::new(10.0, 0.0, 1881.0))]);
}

#[test]
fn osnapz_on_uses_the_current_elevation_and_a_plain_pick_stays_flat() {
    let mut s = Session::new();
    survey(&mut s);
    s.execute("setvar", &serde_json::json!({ "name": "OSNAPZ", "value": 1 })).unwrap();
    s.doc_mut().unwrap().header.set_f64("ELEVATION", 5.0);
    s.cmdline("line").unwrap();
    click(&mut s, Vec2::new(0.01, 0.0), mode::NOD);
    s.input(cadcraft_engine::Input::Point(Vec2::new(20.0, 20.0))).unwrap();
    s.input(cadcraft_engine::Input::Enter).unwrap();
    assert_eq!(lines(&s), vec![(Vec3::new(0.0, 0.0, 5.0), Vec3::new(20.0, 20.0, 0.0))]);
    assert_eq!(s.execute("getvar", &serde_json::json!({ "name": "OSNAPZ" })).unwrap()["value"], 1);
}

#[test]
fn point_takes_a_snapped_z_and_line_a_typed_one() {
    let mut s = Session::new();
    survey(&mut s);
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(10.0, 10.01), mode::END);
    let pts: Vec<Vec3> = s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p) } else { None }).collect();
    assert!(pts.contains(&Vec3::new(10.0, 10.0, 1881.0)), "{pts:?}");
    s.cmdline("line").unwrap();
    s.cmdline("100,100,3").unwrap();
    s.cmdline("@10,0,2").unwrap();
    s.cmdline("").unwrap();
    assert!(lines(&s).contains(&(Vec3::new(100.0, 100.0, 3.0), Vec3::new(110.0, 100.0, 5.0))), "{:?}", lines(&s));
}

/// M2P between two snapped points is midway in Z too: halfway between a survey point's level
/// and a contour's elevation.
#[test]
fn m2p_between_two_snapped_points_is_midway_in_z() {
    let mut s = Session::new();
    survey(&mut s);
    s.cmdline("point").unwrap();
    s.cmdline("m2p").unwrap();
    click(&mut s, Vec2::new(0.01, 0.0), mode::NOD);
    click(&mut s, Vec2::new(10.0, 0.01), mode::END);
    let pts: Vec<Vec3> = s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p) } else { None }).collect();
    assert!(pts.contains(&Vec3::new(5.0, 0.0, 1880.5)), "{pts:?}");
}

/// A one-off snap typed for an M2P point gives that point the Z of what it snapped to, and
/// the next plain point is back at z = 0.
#[test]
fn m2p_with_a_typed_snap_for_each_point_takes_their_levels() {
    let mut s = Session::new();
    survey(&mut s);
    s.cmdline("point").unwrap();
    s.cmdline("m2p").unwrap();
    s.cmdline("nod").unwrap();
    s.input(cadcraft_engine::Input::Point(Vec2::new(0.01, 0.0))).unwrap();
    s.cmdline("end").unwrap();
    s.input(cadcraft_engine::Input::Point(Vec2::new(10.0, 0.01))).unwrap();
    s.cmdline("point").unwrap();
    s.input(cadcraft_engine::Input::Point(Vec2::new(50.0, 50.0))).unwrap();
    let pts: Vec<Vec3> = s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p) } else { None }).collect();
    assert!(pts.contains(&Vec3::new(5.0, 0.0, 1880.5)), "{pts:?}");
    assert!(pts.contains(&Vec3::new(50.0, 50.0, 0.0)), "{pts:?}");
}

/// A point a LISP expression gives at a point prompt keeps its Z, as a typed `x,y,z` does.
#[test]
fn a_lisp_point_keeps_its_z() {
    let mut s = Session::new();
    s.cmdline("line").unwrap();
    s.cmdline("(list 1 2 7)").unwrap();
    s.cmdline("(list 5 2)").unwrap();
    s.cmdline("").unwrap();
    assert!(lines(&s).contains(&(Vec3::new(1.0, 2.0, 7.0), Vec3::new(5.0, 2.0, 0.0))), "{:?}", lines(&s));
}
