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

/// A LINE started with a deferred tangent keeps the Z typed for its next point, and the
/// segment after it starts there.
#[test]
fn a_deferred_tangent_start_keeps_the_next_points_z() {
    let mut s = Session::new();
    s.cmdline("circle 0,0 2").unwrap();
    s.cmdline("zoom e").ok();
    s.cmdline("line").unwrap();
    s.cmdline("tan").unwrap();
    s.input(cadcraft_engine::Input::Point(Vec2::new(-1.4, 1.4))).unwrap();
    s.cmdline("10,10,5").unwrap();
    s.cmdline("20,10,1").unwrap();
    s.cmdline("").unwrap();
    let ls = lines(&s);
    assert_eq!(ls.len(), 2, "{ls:?}");
    assert_eq!(ls[0].0.z, 0.0, "{ls:?}");
    assert_eq!(ls[0].1, Vec3::new(10.0, 10.0, 5.0), "{ls:?}");
    assert_eq!(ls[1], (Vec3::new(10.0, 10.0, 5.0), Vec3::new(20.0, 10.0, 1.0)), "{ls:?}");
}

fn points(s: &Session) -> Vec<Vec3> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p) } else { None }).collect()
}

/// A center snap takes the circle's Z, though no object passes under the cursor there.
#[test]
fn a_center_snap_takes_the_circles_z() {
    let mut s = Session::new();
    s.add_entity(EntityKind::Circle(cadcraft_engine::doc::Circle { center: Vec3::new(0.0, 0.0, 5.0), radius: 10.0 })).unwrap();
    s.cmdline("zoom e").ok();
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(0.01, 0.01), mode::CEN);
    assert_eq!(points(&s), vec![Vec3::new(0.0, 0.0, 5.0)]);
}

/// An intersection of two objects at different levels takes the Z of the first one drawn,
/// whichever is on top.
#[test]
fn an_intersection_takes_the_z_of_the_first_object() {
    let mut s = Session::new();
    s.add_entity(EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::new(-5.0, 0.0, 0.0), b: Vec3::new(5.0, 0.0, 0.0) })).unwrap();
    s.add_entity(EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::new(0.0, -5.0, 3.0), b: Vec3::new(0.0, 5.0, 3.0) })).unwrap();
    s.cmdline("zoom e").ok();
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(0.01, 0.01), mode::INT);
    assert_eq!(points(&s), vec![Vec3::new(0.0, 0.0, 0.0)]);
}

/// A reference to `block` at `at`, scaled by `z_scale` in Z only.
fn reference(block: &str, at: Vec3, z_scale: f64) -> EntityKind {
    EntityKind::Insert(cadcraft_engine::doc::Insert {
        block: block.into(),
        insert: at,
        scale: Vec3::new(1.0, 1.0, z_scale),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

/// Block B: a line rising from (0,0,0) to (10,0,10) and a point at (5,5,5). Block C: B placed
/// 3 units up.
fn blocks(s: &mut Session) {
    let d = s.doc_mut().unwrap();
    let mut b = cadcraft_engine::doc::Block::new("B");
    let line = cadcraft_engine::doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 10.0) };
    b.entities.push(cadcraft_engine::doc::Entity::new(d.new_handle(), EntityKind::Line(line)));
    b.entities.push(cadcraft_engine::doc::Entity::new(d.new_handle(), EntityKind::Point(Point { p: Vec3::new(5.0, 5.0, 5.0), angle: 0.0 })));
    d.blocks.insert("B".into(), std::sync::Arc::new(b));
    let mut c = cadcraft_engine::doc::Block::new("C");
    c.entities.push(cadcraft_engine::doc::Entity::new(d.new_handle(), reference("B", Vec3::new(0.0, 0.0, 3.0), 1.0)));
    d.blocks.insert("C".into(), std::sync::Arc::new(c));
}

/// A snap on an object inside a block reference takes that object's Z, placed by the
/// reference (its insertion Z and Z scale), through nested references too.
#[test]
fn a_snap_inside_a_block_takes_the_z_of_the_object_there() {
    let mut s = Session::new();
    blocks(&mut s);
    s.add_entity(reference("B", Vec3::new(100.0, 0.0, 2.0), 2.0)).unwrap();
    s.add_entity(reference("C", Vec3::new(300.0, 0.0, 1.0), 1.0)).unwrap();
    s.cmdline("zoom e").ok();
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(110.01, 0.01), mode::END);
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(105.01, 5.01), mode::NOD);
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(310.01, 0.01), mode::END);
    assert_eq!(points(&s), vec![Vec3::new(110.0, 0.0, 22.0), Vec3::new(105.0, 5.0, 12.0), Vec3::new(310.0, 0.0, 14.0)]);
}

/// Center and quadrant snaps on an ellipse take its Z.
#[test]
fn ellipse_snaps_take_the_ellipses_z() {
    let mut s = Session::new();
    let e = cadcraft_engine::doc::Ellipse {
        center: Vec3::new(300.0, 0.0, 3.0),
        major: Vec3::new(10.0, 0.0, 0.0),
        ratio: 0.5,
        start: 0.0,
        end: std::f64::consts::TAU,
    };
    s.add_entity(EntityKind::Ellipse(e)).unwrap();
    s.cmdline("zoom e").ok();
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(300.01, 0.01), mode::CEN);
    s.cmdline("point").unwrap();
    click(&mut s, Vec2::new(310.01, 0.01), mode::QUA);
    assert_eq!(points(&s), vec![Vec3::new(300.0, 0.0, 3.0), Vec3::new(310.0, 0.0, 3.0)]);
}
