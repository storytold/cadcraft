//! Array actions of dynamic blocks: a reference repeats the arrayed objects as far as its linear,
//! polar or XY parameter reaches, as AutoCAD does (one copy per whole column along a distance; a
//! whole number of columns by rows across an XY parameter, or just the object when either is
//! short of one), marks the copies as AutoCAD marks them, and keeps the right number of copies
//! through later changes and a save.
//!
//! The fixture is synthetic: block Row (a 2×2 square, Distance1 from the origin to (2, 0), copies
//! every 3), Fan (a circle, Distance1/Angle1 from the origin to (3, 4), copies every 2) and Grid
//! (a circle, X Distance1/Y Distance1 from the origin to (10, 10), columns every 5 and rows every
//! 4, distances at least 0).

use cadcraft_doc::dynblock::{ActionKind, GripPart, GripShape, ParameterKind, custom_properties, drag_grip, dynamic_block, grips, set_property};
use cadcraft_doc::geom::{Vec2, Vec3};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_io::{read_dxf, write_dxf};

fn dxf(src: &str) -> String {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (code, value) = l.split_once(' ').unwrap_or((l, ""));
            format!("{code}\n{value}\n")
        })
        .collect()
}

/// The fixture with a reference to `block` at (100, 100).
fn reference(block: &str) -> (Drawing, Handle) {
    let mut d = read_dxf(dxf(&include_str!("../data/dynblock_arrays.txt").replace("\r\n", "\n")).as_bytes()).unwrap();
    let ins = cadcraft_doc::Insert {
        block: block.into(),
        insert: Vec3::new(100.0, 100.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
    (d, r)
}

fn props(d: &Drawing, r: Handle) -> Vec<(String, String)> {
    custom_properties(d, r).unwrap().1.into_iter().map(|p| (p.name, p.value)).collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// Where each object of the reference's block is (a circle's centre, a polyline's first vertex).
fn places(d: &Drawing, r: Handle) -> Vec<(f64, f64)> {
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("a reference") };
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    d.block(&i.block)
        .unwrap()
        .entities
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Circle(c) => (round(c.center.x), round(c.center.y)),
            EntityKind::LwPolyline(p) => (round(p.vertices[0].p.x), round(p.vertices[0].p.y)),
            k => panic!("unexpected {k:?}"),
        })
        .collect()
}

/// Whether each object of the reference's block is marked as an array's copy.
fn marked(d: &Drawing, r: Handle) -> Vec<bool> {
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("a reference") };
    let mark = |e: &cadcraft_doc::Entity| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "AcDbBlockArrayActionTag");
    d.block(&i.block).unwrap().entities.iter().map(|e| mark(e)).collect()
}

#[test]
fn reads_array_actions_and_polar_and_xy_parameters() {
    let (d, _) = reference("Row");
    let m = dynamic_block(&d, "Row").unwrap();
    assert!(matches!(m.actions[0].kind, ActionKind::Array { column: 3.0, row: 1.0 }));
    assert_eq!(m.actions[0].connections.len(), 4);
    let m = dynamic_block(&d, "Fan").unwrap();
    let ParameterKind::Polar { label, angle_label, .. } = &m.parameters[0].kind else { panic!("a polar parameter") };
    assert_eq!((label.as_str(), angle_label.as_str()), ("Distance1", "Angle1"));
    let m = dynamic_block(&d, "Grid").unwrap();
    let ParameterKind::Xy { x_label, y_label, x_values, y_values, .. } = &m.parameters[0].kind else { panic!("an XY parameter") };
    assert_eq!((x_label.as_str(), y_label.as_str()), ("X Distance1", "Y Distance1"));
    assert!(x_values.has_minimum() && y_values.has_minimum());
    assert_eq!(m.parameters[0].grips, vec![3, 2, 4, 5], "base, end and the two corner grips");
    assert!(matches!(m.actions[0].kind, ActionKind::Array { column: 5.0, row: 4.0 }));
}

#[test]
fn polar_and_xy_parameters_show_their_two_properties() {
    let (d, r) = reference("Fan");
    assert_eq!(props(&d, r), pairs(&[("Distance1", "5.0000"), ("Angle1", "53")]));
    let (d, r) = reference("Grid");
    assert_eq!(props(&d, r), pairs(&[("X Distance1", "10.0000"), ("Y Distance1", "10.0000")]));
}

#[test]
fn a_distance_repeats_the_objects_once_per_whole_column() {
    let (mut d, r) = reference("Row");
    set_property(&mut d, r, "Distance1", "9").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0), (3.0, 0.0), (6.0, 0.0)]);
    assert_eq!(marked(&d, r), vec![false, true, true], "AutoCAD marks every copy but the first");
    // Short of a whole column: one fewer; short of one: the object alone.
    set_property(&mut d, r, "Distance1", "8.9").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0), (3.0, 0.0)]);
    set_property(&mut d, r, "Distance1", "2.5").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0)]);
    assert_eq!(marked(&d, r), vec![false]);
    set_property(&mut d, r, "Distance1", "12").unwrap();
    assert_eq!(places(&d, r).len(), 4);
}

#[test]
fn a_polar_parameter_repeats_along_its_angle() {
    let (mut d, r) = reference("Fan");
    set_property(&mut d, r, "Distance1", "7").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0), (1.2, 1.6), (2.4, 3.2)]);
    set_property(&mut d, r, "Angle1", "90").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0), (0.0, 2.0), (0.0, 4.0)]);
    assert_eq!(props(&d, r), pairs(&[("Distance1", "7.0000"), ("Angle1", "90")]));
}

#[test]
fn an_xy_parameter_repeats_in_columns_and_rows() {
    let (mut d, r) = reference("Grid");
    // 10 × 10 with columns every 5 and rows every 4: 2 by 2.
    set_property(&mut d, r, "Y Distance1", "10").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0), (5.0, 0.0), (0.0, 4.0), (5.0, 4.0)]);
    set_property(&mut d, r, "Y Distance1", "13").unwrap();
    assert_eq!(places(&d, r).len(), 6, "2 columns by 3 rows, the earlier copies replaced");
    assert_eq!(marked(&d, r), vec![false, true, true, true, true, true]);
    // Short of one column: the object alone, however many rows fit.
    set_property(&mut d, r, "X Distance1", "4").unwrap();
    assert_eq!(places(&d, r), vec![(0.0, 0.0)]);
    // Below the value set's minimum: refused, as AutoCAD refuses it.
    assert!(set_property(&mut d, r, "X Distance1", "-10").is_err());
    assert_eq!(props(&d, r), pairs(&[("X Distance1", "4.0000"), ("Y Distance1", "13.0000")]));
}

#[test]
fn xy_grips_move_the_base_end_and_corners() {
    let (mut d, r) = reference("Grid");
    let gs = grips(&d, r);
    let summary: Vec<(GripPart, GripShape, (f64, f64))> = gs.iter().map(|g| (g.part, g.shape, (g.at.x, g.at.y))).collect();
    assert_eq!(
        summary,
        vec![
            (GripPart::Base, GripShape::Point, (100.0, 100.0)),
            (GripPart::End, GripShape::Point, (110.0, 110.0)),
            (GripPart::BaseXEndY, GripShape::Point, (100.0, 110.0)),
            (GripPart::BaseYEndX, GripShape::Point, (110.0, 100.0)),
        ]
    );
    // The base X and end Y corner, dragged left and up: the base moves left (the circle with
    // it, as AutoCAD's arrays follow the base), the end up.
    drag_grip(&mut d, r, 1, GripPart::BaseXEndY, Vec2::new(95.0, 112.0)).unwrap();
    assert_eq!(props(&d, r), pairs(&[("X Distance1", "15.0000"), ("Y Distance1", "12.0000")]));
    assert_eq!(places(&d, r), vec![(-5.0, 0.0), (0.0, 0.0), (5.0, 0.0), (-5.0, 4.0), (0.0, 4.0), (5.0, 4.0), (-5.0, 8.0), (0.0, 8.0), (5.0, 8.0)]);
    // A corner past the base: below the minimum, refused.
    assert!(drag_grip(&mut d, r, 1, GripPart::BaseYEndX, Vec2::new(90.0, 100.0)).is_err());
    assert!(drag_grip(&mut d, r, 1, GripPart::At, Vec2::new(90.0, 100.0)).is_err());
}

#[test]
fn polar_grips_move_the_end_and_the_base() {
    let (mut d, r) = reference("Fan");
    let parts: Vec<GripPart> = grips(&d, r).into_iter().map(|g| g.part).collect();
    assert_eq!(parts, vec![GripPart::Base, GripPart::End]);
    drag_grip(&mut d, r, 1, GripPart::End, Vec2::new(100.0, 106.0)).unwrap();
    assert_eq!(props(&d, r), pairs(&[("Distance1", "6.0000"), ("Angle1", "90")]));
    assert_eq!(places(&d, r), vec![(0.0, 0.0), (0.0, 2.0), (0.0, 4.0)]);
}

#[test]
fn copies_and_values_survive_a_save() {
    let (mut d, r) = reference("Grid");
    set_property(&mut d, r, "X Distance1", "12").unwrap();
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(props(&back, r), props(&d, r));
    assert_eq!(places(&back, r), places(&d, r));
    // Changed again after the save: still one set of copies.
    let mut back = back;
    set_property(&mut back, r, "Y Distance1", "5").unwrap();
    assert_eq!(places(&back, r), vec![(0.0, 0.0), (5.0, 0.0)]);
}

#[test]
fn hostile_arrays_are_refused_not_built() {
    let (mut d, r) = reference("Grid");
    // A billion columns: refused before any copy is made.
    assert!(set_property(&mut d, r, "X Distance1", "1e10").is_err());
    assert!(set_property(&mut d, r, "X Distance1", "NaN").is_err());
    assert!(set_property(&mut d, r, "X Distance1", "inf").is_err());
    assert_eq!(places(&d, r), vec![(0.0, 0.0)]);
    let (mut d, r) = reference("Row");
    assert!(set_property(&mut d, r, "Distance1", "1e300").is_err());
    let (mut d, r) = reference("Fan");
    assert!(set_property(&mut d, r, "Angle1", "nan").is_err());
    assert!(set_property(&mut d, r, "Distance1", "0").is_err());
}
