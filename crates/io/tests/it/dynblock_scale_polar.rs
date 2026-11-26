//! Scale and polar stretch actions of dynamic blocks, as AutoCAD applies them: a scale action
//! scales its objects about its base point by how much the distance it follows grew (a 2×2 square
//! scaled by a distance going from 2 to 4 is 4×4); a polar stretch turns its objects about the
//! polar parameter's base by the change of its angle and moves the vertices in its frame (and the
//! objects it doesn't stretch) with the end, while its rotate-only objects just turn. Both carry
//! the parameters in their selection, and the results survive a save.
//!
//! The fixture is synthetic: block Grow (a 2×2 square and a circle at (1, 1), Distance1 from the
//! origin to (2, 0), scaled about the distance's base; Position1 at (2, 2) is in the selection),
//! Pinned (the square, scaled about the fixed point (2, 2)) and Sheet (the square, scaled by the
//! diagonal of X Distance1 and Y Distance1, from the origin to (2, 2), as AutoCAD scales by an XY
//! parameter); Tag (a line from (1, 0) to (4, 0), a circle at its end and one at (2, 2), a polar
//! Distance1/Angle1 from the origin to (4, 0); the polar stretch's frame holds the line's end and
//! Position1 at (4, 1); the circle at (2, 2) only turns).

use cadcraft_doc::dynblock::{ActionKind, custom_properties, dynamic_block, set_property};
use cadcraft_doc::geom::Vec3;
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
    reference_in(&include_str!("../data/dynblock_scale_polar.txt").replace("\r\n", "\n"), block)
}

/// Fixture text `src` with a reference to `block` at (100, 100).
fn reference_in(src: &str, block: &str) -> (Drawing, Handle) {
    let mut d = read_dxf(dxf(src).as_bytes()).unwrap();
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
        clip: None,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
    (d, r)
}

fn props(d: &Drawing, r: Handle) -> Vec<(String, String)> {
    custom_properties(d, r).unwrap().1.into_iter().map(|p| (p.name, p.value)).collect()
}

/// Each object of the reference's block, in block coordinates: a polyline's corners (lowest and
/// highest), a circle's centre and radius, a line's ends.
fn shapes(d: &Drawing, r: Handle) -> Vec<[f64; 4]> {
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("a reference") };
    let round = |v: f64| (v * 1000.0).round() / 1000.0 + 0.0;
    d.block(&i.block)
        .unwrap()
        .entities
        .iter()
        .map(|e| match &e.kind {
            EntityKind::LwPolyline(p) => {
                let xs = p.vertices.iter().map(|v| v.p.x);
                let ys = p.vertices.iter().map(|v| v.p.y);
                [
                    round(xs.clone().fold(f64::MAX, f64::min)),
                    round(ys.clone().fold(f64::MAX, f64::min)),
                    round(xs.fold(f64::MIN, f64::max)),
                    round(ys.fold(f64::MIN, f64::max)),
                ]
            }
            EntityKind::Circle(c) => [round(c.center.x), round(c.center.y), round(c.radius), 0.0],
            EntityKind::Line(l) => [round(l.a.x), round(l.a.y), round(l.b.x), round(l.b.y)],
            k => panic!("unexpected {k:?}"),
        })
        .collect()
}

#[test]
fn reads_the_scale_actions_base_and_connections() {
    let (d, _) = reference("Grow");
    let m = dynamic_block(&d, "Grow").unwrap();
    let a = m.actions.iter().find(|a| matches!(a.kind, ActionKind::Scale { .. })).unwrap();
    let ActionKind::Scale { base } = a.kind else { unreachable!() };
    assert!(base.dependent);
    let names: Vec<&str> = a.connections.iter().map(|(_, c)| c.as_str()).collect();
    assert_eq!(names, ["UpdatedBaseX", "UpdatedBaseY", "Scale", "XScale", "YScale"]);
    let m = dynamic_block(&d, "Pinned").unwrap();
    let ActionKind::Scale { base } = m.actions[0].kind else { panic!("a scale action") };
    assert!(!base.dependent);
    assert_eq!((base.at.x, base.at.y), (2.0, 2.0));
}

#[test]
fn a_distance_scales_its_objects_about_the_base() {
    let (mut d, r) = reference("Grow");
    set_property(&mut d, r, "Distance1", "4").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 4.0, 4.0], [2.0, 2.0, 1.0, 0.0]]);
    // The point parameter in the selection is scaled with them.
    let p = props(&d, r);
    assert!(p.contains(&("Position1 X".into(), "4.0000".into())), "{p:?}");
    assert!(p.contains(&("Position1 Y".into(), "4.0000".into())), "{p:?}");
    // From there, by the ratio of the new distance to the one it has now.
    set_property(&mut d, r, "Distance1", "1").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 1.0, 1.0], [0.5, 0.5, 0.25, 0.0]]);
}

#[test]
fn a_fixed_base_point_stays() {
    let (mut d, r) = reference("Pinned");
    set_property(&mut d, r, "Distance1", "4").unwrap();
    assert_eq!(shapes(&d, r), vec![[-2.0, -2.0, 2.0, 2.0]]);
}

#[test]
fn an_xy_parameter_scales_by_its_diagonal() {
    let (mut d, r) = reference("Sheet");
    // From (2, 2) to (4, 2): the diagonal grows from √8 to √20.
    set_property(&mut d, r, "X Distance1", "4").unwrap();
    let s = (20.0f64 / 8.0).sqrt() * 2.0;
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, round(s), round(s)]]);
    // A distance of 0 along one axis still leaves a diagonal: 4 long, twice the square's √8 / 2.
    set_property(&mut d, r, "Y Distance1", "0").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 2.828, 2.828]]);
}

#[test]
fn scaled_objects_and_values_survive_a_save() {
    let (mut d, r) = reference("Grow");
    set_property(&mut d, r, "Distance1", "3").unwrap();
    let mut back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(props(&back, r), props(&d, r));
    assert_eq!(shapes(&back, r), shapes(&d, r));
    set_property(&mut back, r, "Distance1", "6").unwrap();
    assert_eq!(shapes(&back, r), vec![[0.0, 0.0, 6.0, 6.0], [3.0, 3.0, 1.5, 0.0]]);
}

#[test]
fn hostile_scales_are_refused_and_change_nothing() {
    let (mut d, r) = reference("Grow");
    for v in ["0", "-3", "NaN", "inf", "1e-300", "1e200"] {
        assert!(set_property(&mut d, r, "Distance1", v).is_err(), "{v}");
    }
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 2.0, 2.0], [1.0, 1.0, 0.5, 0.0]]);
    // A large factor within the limit is a large block.
    set_property(&mut d, r, "Distance1", "2e6").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 2e6, 2e6], [1e6, 1e6, 5e5, 0.0]]);
    // Both XY distances 0 would scale the objects to nothing; from there no ratio is known.
    let (mut d, r) = reference("Sheet");
    set_property(&mut d, r, "X Distance1", "0").unwrap();
    assert!(set_property(&mut d, r, "Y Distance1", "0").is_err());
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 1.414, 1.414]]);
}

#[test]
fn reads_the_polar_stretch_action() {
    let (d, _) = reference("Tag");
    let m = dynamic_block(&d, "Tag").unwrap();
    let ActionKind::PolarStretch { frame, vertices, rotate_only, multiplier, .. } = &m.actions[0].kind else { panic!("a polar stretch") };
    assert_eq!(frame.len(), 2);
    assert_eq!(vertices, &vec![(Handle(0xB0), vec![1])]);
    assert_eq!(rotate_only, &vec![Handle(0xB2)]);
    assert_eq!(*multiplier, 1.0);
    assert_eq!(m.actions[0].connections.len(), 6);
}

#[test]
fn a_polar_distance_stretches_the_frame_along_it() {
    let (mut d, r) = reference("Tag");
    set_property(&mut d, r, "Distance1", "6").unwrap();
    // The line's end and the head follow the end; the rotate-only circle stays.
    assert_eq!(shapes(&d, r), vec![[1.0, 0.0, 6.0, 0.0], [6.0, 0.0, 0.25, 0.0], [2.0, 2.0, 0.5, 0.0]]);
    let p = props(&d, r);
    assert!(p.contains(&("Position1 X".into(), "6.0000".into())), "{p:?}");
}

#[test]
fn a_polar_angle_turns_everything_about_the_base() {
    let (mut d, r) = reference("Tag");
    set_property(&mut d, r, "Angle1", "90").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 1.0, 0.0, 4.0], [0.0, 4.0, 0.25, 0.0], [-2.0, 2.0, 0.5, 0.0]]);
    let p = props(&d, r);
    assert!(p.contains(&("Position1 X".into(), "-1.0000".into())) && p.contains(&("Position1 Y".into(), "4.0000".into())), "{p:?}");
    // Then a distance along the new direction, and back to the start.
    set_property(&mut d, r, "Distance1", "6").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 1.0, 0.0, 6.0], [0.0, 6.0, 0.25, 0.0], [-2.0, 2.0, 0.5, 0.0]]);
    set_property(&mut d, r, "Angle1", "0").unwrap();
    set_property(&mut d, r, "Distance1", "4").unwrap();
    assert_eq!(shapes(&d, r), vec![[1.0, 0.0, 4.0, 0.0], [4.0, 0.0, 0.25, 0.0], [2.0, 2.0, 0.5, 0.0]]);
}

/// The arc and the wipeout a polar stretch acts on move whole after its turn, as before: the
/// lists it stores for them (the arc's start and the wipeout's second point, both in its frame)
/// aren't used, as for a stretch whose frame can't be tested (a wipeout's or an arc's list isn't
/// measured).
#[test]
fn a_polar_stretch_moves_an_arc_and_a_wipeout_whole() {
    // Arc B3 round (3, 0) from (4, 0), in the frame, to (2, 0); wipeout B4 from x = 3 to x = 4.
    let extra = "0 ARC\n5 B3\n330 1A\n100 AcDbEntity\n8 0\n100 AcDbCircle\n10 3\n20 0\n30 0\n40 1\n100 AcDbArc\n50 0\n51 180\n\
                 0 WIPEOUT\n5 B4\n330 1A\n100 AcDbEntity\n8 0\n100 AcDbWipeout\n10 0\n20 0\n30 0\n11 1\n21 0\n31 0\n12 0\n22 1\n32 0\n\
                 13 1\n23 1\n71 2\n91 4\n14 2.5\n24 0.7\n14 3.5\n24 0.7\n14 3.5\n24 0.3\n14 2.5\n24 0.3\n";
    let src = include_str!("../data/dynblock_scale_polar.txt")
        .replace("\r\n", "\n")
        .replace("1005 B2\n0 ENDBLK\n", &format!("1005 B2\n{extra}0 ENDBLK\n"))
        .replace("71 3\n330 B0\n330 B1\n330 93\n", "71 5\n330 B0\n330 B1\n330 B3\n330 B4\n330 93\n")
        .replace("74 1\n332 B0\n75 1\n76 1\n", "74 3\n332 B0\n75 1\n76 1\n332 B3\n75 1\n76 0\n332 B4\n75 1\n76 1\n");
    let (mut d, r) = reference_in(&src, "Tag");
    let m = dynamic_block(&d, "Tag").unwrap();
    let ActionKind::PolarStretch { vertices, .. } = &m.actions[0].kind else { panic!("a polar stretch") };
    assert_eq!(vertices, &vec![(Handle(0xB0), vec![1]), (Handle(0xB3), vec![0]), (Handle(0xB4), vec![1])]);
    let parts = |d: &Drawing| {
        let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("a reference") };
        let b = d.block(&i.block).unwrap();
        let arc = b.entities.iter().find_map(|e| match &e.kind {
            EntityKind::Arc(a) => Some((a.center, a.radius, a.start, a.end)),
            _ => None,
        });
        let wipe = b.entities.iter().find_map(|e| match &e.kind {
            EntityKind::Wipeout(w) => Some(w.boundary.clone()),
            _ => None,
        });
        (arc.unwrap(), wipe.unwrap())
    };
    let ((c, radius, start, end), outline) = parts(&d);
    set_property(&mut d, r, "Distance1", "6").unwrap();
    let ((c2, radius2, start2, end2), outline2) = parts(&d);
    let v = cadcraft_doc::geom::Vec2::new(2.0, 0.0);
    assert!(c2.xy().near(c.xy() + v, 1e-9) && c2.z == c.z && radius2 == radius && start2 == start && end2 == end, "{c2:?} {radius2} {start2} {end2}");
    assert_eq!(outline2.len(), outline.len());
    assert!(outline2.iter().zip(&outline).all(|(p, q)| p.near(*q + v, 1e-9)), "{outline2:?}");
}

#[test]
fn polar_stretched_objects_survive_a_save() {
    let (mut d, r) = reference("Tag");
    set_property(&mut d, r, "Angle1", "90").unwrap();
    let mut back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(props(&back, r), props(&d, r));
    assert_eq!(shapes(&back, r), shapes(&d, r));
    set_property(&mut back, r, "Distance1", "2").unwrap();
    assert_eq!(shapes(&back, r), vec![[0.0, 1.0, 0.0, 2.0], [0.0, 2.0, 0.25, 0.0], [-2.0, 2.0, 0.5, 0.0]]);
}

#[test]
fn hostile_polar_values_are_refused() {
    let (mut d, r) = reference("Tag");
    for (n, v) in [("Distance1", "0"), ("Distance1", "NaN"), ("Distance1", "-inf"), ("Angle1", "nan"), ("Angle1", "1e400")] {
        assert!(set_property(&mut d, r, n, v).is_err(), "{n} {v}");
    }
    assert_eq!(shapes(&d, r), vec![[1.0, 0.0, 4.0, 0.0], [4.0, 0.0, 0.25, 0.0], [2.0, 2.0, 0.5, 0.0]]);
}
