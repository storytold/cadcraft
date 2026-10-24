//! Base point and alignment parameters of dynamic blocks, and lookup tables that set angles, flip
//! states, XY distances and positions, as AutoCAD applies them:
//! - an action that moves a base point parameter moves the reference's insertion point with it,
//!   and the block's objects move back so the parameter stays at the block's base point;
//! - an alignment grip dragged next to an object moves the reference so the alignment's base is on
//!   the object and turns it so the alignment runs along the object, its right side toward the
//!   cursor;
//! - lookup tables set and read rotation angles (`UpdatedAngle`, radians), flip states
//!   (`UpdatedFlip`), XY distances (`UpdatedDistanceX`/`Y`), polar distances and angles and
//!   positions (`UpdatedX`/`Y`, from the block's base point).
//!
//! The fixture is synthetic, laid out as AutoCAD writes these blocks: Shift (a 10×5 rectangle left
//! of the origin, Distance1 along its bottom, a stretch of its right side holding the base point
//! parameter, an attribute TAG at (-8, 0); a reference at (100, 100) with its attribute at (92, 100)),
//! Align (a 4×2 rectangle, a perpendicular alignment from the origin along X), Turn (a 4×2
//! rectangle turned by Angle1 about the origin; Lookup1 gives Up 90°, Left 180°, Zero 0°) and Many
//! (Flip state1, X/Y Distance1, polar Distance1/Angle1 and Position1, all set by Lookup1's rows A
//! and B, B being the block's own values). A line at 30° runs from (120, 100) to (140, 111.547).

use cadcraft_doc::dynblock::{GripShape, ParameterKind, Value, align_grip, custom_properties, dynamic_block, grips, reference, set_property};
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

fn fixture() -> Drawing {
    read_dxf(dxf(&include_str!("../data/dynblock_params.txt").replace("\r\n", "\n")).as_bytes()).unwrap()
}

/// The fixture with a reference to `block` at (100, 100) turned by `rotation`.
fn reference_to(block: &str, rotation: f64) -> (Drawing, Handle) {
    let mut d = fixture();
    let ins = cadcraft_doc::Insert {
        block: block.into(),
        insert: Vec3::new(100.0, 100.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation,
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

fn prop(d: &Drawing, r: Handle, name: &str) -> String {
    props(d, r).into_iter().find(|(n, _)| n == name).map(|(_, v)| v).unwrap_or_else(|| panic!("no {name}"))
}

fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0 + 0.0
}

fn insert(d: &Drawing, r: Handle) -> &cadcraft_doc::Insert {
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("a reference") };
    i
}

/// The reference's polyline corners (lowest and highest) in block coordinates.
fn outline(d: &Drawing, r: Handle) -> [f64; 4] {
    let b = d.block(&insert(d, r).block).unwrap();
    let pl = b
        .entities
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::LwPolyline(p) => Some(p),
            _ => None,
        })
        .unwrap();
    let xs = pl.vertices.iter().map(|v| v.p.x);
    let ys = pl.vertices.iter().map(|v| v.p.y);
    [
        round(xs.clone().fold(f64::MAX, f64::min)),
        round(ys.clone().fold(f64::MAX, f64::min)),
        round(xs.fold(f64::MIN, f64::max)),
        round(ys.fold(f64::MIN, f64::max)),
    ]
}

fn at(d: &Drawing, r: Handle) -> (f64, f64) {
    let i = insert(d, r);
    (round(i.insert.x), round(i.insert.y))
}

#[test]
fn reads_the_alignment_type_and_shows_its_grip() {
    let (d, r) = reference_to("Align", 0.0);
    let m = dynamic_block(&d, "Align").unwrap();
    assert!(matches!(m.parameters[0].kind, ParameterKind::Alignment { perpendicular: true, .. }));
    let g = grips(&d, r);
    let a = g.iter().find(|g| g.shape == GripShape::Alignment).expect("an alignment grip");
    // At the base point, pointing to the left of the alignment's direction (as AutoCAD draws it).
    assert_eq!((round(a.at.x), round(a.at.y)), (100.0, 100.0));
    assert_eq!((round(a.dir.x), round(a.dir.y)), (0.0, 1.0));
}

#[test]
fn a_moved_base_point_moves_the_insertion_point() {
    let mut d = fixture();
    let r = Handle(0xE0);
    set_property(&mut d, r, "Distance1", "15").unwrap();
    // Measured in AutoCAD: the reference moves to (105, 100) and its block runs from -15 to 0.
    assert_eq!(at(&d, r), (105.0, 100.0));
    assert_eq!(outline(&d, r), [-15.0, -2.5, 0.0, 2.5]);
    assert_eq!(prop(&d, r, "Distance1"), "15.0000");
    // The values as AutoCAD records them: the base point at the block's base, the distance moved back.
    let v = reference(&d, r).unwrap().values;
    assert_eq!(v.get(&1), Some(&Value::Point { at: Vec3::new(0.0, 0.0, 0.0) }));
    assert_eq!(v.get(&2), Some(&Value::Points { base: Vec3::new(-15.0, -2.5, 0.0), end: Vec3::new(0.0, -2.5, 0.0) }));
    // The attribute stays where it was in the drawing (its definition didn't move).
    let a = &insert(&d, r).attribs[0];
    assert_eq!((round(a.text.insert.x), round(a.text.insert.y)), (92.0, 100.0));
    // And back, from there.
    set_property(&mut d, r, "Distance1", "12").unwrap();
    assert_eq!(at(&d, r), (102.0, 100.0));
    assert_eq!(outline(&d, r), [-12.0, -2.5, 0.0, 2.5]);
    // A save keeps it.
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(at(&back, r), (102.0, 100.0));
    assert_eq!(prop(&back, r, "Distance1"), "12.0000");
}

/// AutoCAD takes a distance of 0 (title block value lists offer it); the distance keeps its
/// direction for the next change.
#[test]
fn a_distance_of_0_keeps_its_direction() {
    let mut d = fixture();
    let r = Handle(0xE0);
    set_property(&mut d, r, "Distance1", "0").unwrap();
    assert_eq!(prop(&d, r, "Distance1"), "0.0000");
    assert_eq!(at(&d, r), (90.0, 100.0));
    set_property(&mut d, r, "Distance1", "15").unwrap();
    assert_eq!(at(&d, r), (105.0, 100.0));
    assert_eq!(outline(&d, r), [-15.0, -2.5, 0.0, 2.5]);
    assert!(set_property(&mut d, r, "Distance1", "-1").is_err());
}

#[test]
fn a_turned_reference_moves_along_its_own_axis() {
    let (mut d, r) = reference_to("Shift", std::f64::consts::FRAC_PI_2);
    set_property(&mut d, r, "Distance1", "15").unwrap();
    assert_eq!(at(&d, r), (100.0, 105.0));
}

#[test]
fn an_alignment_follows_a_line() {
    let line = Vec2::new(30f64.to_radians().cos(), 30f64.to_radians().sin());
    let foot = Vec2::new(120.0, 100.0) + line * 10.0;
    // Measured in AutoCAD: from below the line the reference turns to 30°, from above to 210°, its
    // alignment's base on the line.
    for (side, rotation) in [(-0.5, 30.0), (0.5, 210.0)] {
        let (mut d, r) = reference_to("Align", 0.0);
        let cursor = foot + line.perp() * side;
        align_grip(&mut d, r, 1, cursor, Some((foot, line))).unwrap();
        assert_eq!(round(insert(&d, r).rotation.to_degrees()), rotation);
        assert_eq!(at(&d, r), (round(foot.x), round(foot.y)));
    }
}

#[test]
fn an_alignment_follows_a_circle() {
    // A circle about (150, 150), its nearest point at 225°: outside, the reference turns to 315°
    // (counterclockwise along the circle), inside to 135°, as in AutoCAD.
    let c = Vec2::new(150.0, 150.0);
    let out = Vec2::new(225f64.to_radians().cos(), 225f64.to_radians().sin());
    let foot = c + out * 10.0;
    for (r_cursor, rotation) in [(10.5, 315.0), (9.5, 135.0)] {
        let (mut d, r) = reference_to("Align", 0.0);
        align_grip(&mut d, r, 1, c + out * r_cursor, Some((foot, out.perp()))).unwrap();
        assert_eq!(round(insert(&d, r).rotation.to_degrees()), rotation);
    }
}

#[test]
fn an_alignment_grip_away_from_objects_only_moves() {
    let (mut d, r) = reference_to("Align", 0.5);
    align_grip(&mut d, r, 1, Vec2::new(90.0, 80.0), None).unwrap();
    assert_eq!(at(&d, r), (90.0, 80.0));
    assert_eq!(insert(&d, r).rotation, 0.5);
    // On the object itself, the side the grip came from decides.
    let (mut d, r) = reference_to("Align", 0.0);
    let line = Vec2::new(1.0, 0.0);
    align_grip(&mut d, r, 1, Vec2::new(110.0, 90.0), Some((Vec2::new(110.0, 90.0), line))).unwrap();
    // The grip was above the line (y = 90): its right side faces up, so the alignment runs to -X.
    assert_eq!(round(insert(&d, r).rotation.to_degrees()), 180.0);
    assert!(align_grip(&mut d, r, 1, Vec2::new(f64::NAN, 0.0), None).is_err());
    assert!(align_grip(&mut d, r, 7, Vec2::new(0.0, 0.0), None).is_err());
}

#[test]
fn a_lookup_sets_and_reads_an_angle() {
    let (mut d, r) = reference_to("Turn", 0.0);
    assert_eq!(prop(&d, r, "Lookup1"), "Zero");
    set_property(&mut d, r, "Lookup1", "Up").unwrap();
    assert_eq!(prop(&d, r, "Angle1"), "90");
    assert_eq!(outline(&d, r), [-2.0, 0.0, 0.0, 4.0]);
    // Reverse: an angle in the table gives its row.
    set_property(&mut d, r, "Angle1", "180").unwrap();
    assert_eq!(prop(&d, r, "Lookup1"), "Left");
    set_property(&mut d, r, "Angle1", "45").unwrap();
    assert_eq!(prop(&d, r, "Lookup1"), "Custom");
    assert!(custom_properties(&d, r).unwrap().1.iter().any(|p| p.name == "Lookup1" && p.editable));
}

#[test]
fn a_lookup_sets_flips_xy_polar_and_positions() {
    let (mut d, r) = reference_to("Many", 0.0);
    // The block's own values are row B.
    assert_eq!(prop(&d, r, "Lookup1"), "B");
    set_property(&mut d, r, "Lookup1", "A").unwrap();
    // As AutoCAD sets them.
    for (name, value) in [
        ("Flip state1", "Flipped"),
        ("X Distance1", "6.0000"),
        ("Y Distance1", "3.0000"),
        ("Distance1", "5.0000"),
        ("Angle1", "30"),
        ("Position1 X", "1.0000"),
        ("Position1 Y", "0.5000"),
        ("Lookup1", "A"),
    ] {
        assert_eq!(prop(&d, r, name), value, "{name}");
    }
    // The values are recorded as AutoCAD lays them out (it doesn't take the reference otherwise): a
    // flip state after its points, and the flip grip's state on its own after two header groups.
    let codes: Vec<Vec<i32>> = d
        .foreign
        .objects
        .iter()
        .filter(|o| o.kind == "XRECORD")
        .map(|o| o.tags.iter().map(|t| t.code).filter(|c| [10, 70].contains(c)).collect())
        .collect();
    assert!(codes.contains(&vec![10, 10, 70]), "{codes:?}");
    assert!(codes.contains(&vec![70, 70, 70, 70]), "{codes:?}");
    // A save keeps it, and the reverse lookup reads it back.
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(prop(&back, r, "Lookup1"), "A");
    assert_eq!(prop(&back, r, "Flip state1"), "Flipped");
    // One input off the table's rows: no row matches.
    set_property(&mut d, r, "Flip state1", "Not flipped").unwrap();
    assert_eq!(prop(&d, r, "Lookup1"), "Custom");
    set_property(&mut d, r, "Lookup1", "B").unwrap();
    assert_eq!(prop(&d, r, "X Distance1"), "4.0000");
    assert_eq!(prop(&d, r, "Position1 X"), "2.0000");
}

#[test]
fn a_lookup_cell_that_isnt_a_number_is_refused() {
    let src = include_str!("../data/dynblock_params.txt").replace("\r\n", "\n").replace("302 1.570796326794896", "302 up-ish");
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let ins = cadcraft_doc::Insert {
        block: "Turn".into(),
        insert: Vec3::new(0.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
    let before = insert(&d, r).block.clone();
    assert!(set_property(&mut d, r, "Lookup1", "Up").is_err());
    assert_eq!(insert(&d, r).block, before);
    // The other rows still work, and reading the table doesn't trip on the bad cell.
    set_property(&mut d, r, "Lookup1", "Left").unwrap();
    assert_eq!(prop(&d, r, "Angle1"), "180");
}
