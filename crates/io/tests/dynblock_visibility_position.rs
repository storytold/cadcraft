//! A visibility parameter in a stretch's selection is carried along with the stretch: its grip
//! goes with it, its record keeps the new position (with its state) through a save, and a state
//! change leaves it where it is.
//!
//! The fixture is the synthetic DOOR block of `dynblock_read`, with Visibility1 (at (18, 40))
//! given a grip and put in the Width stretch's selection, whose frame is widened to hold it.

use cadcraft_doc::dynblock::{GripShape, grips, reference, set_property};
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

/// DOOR as described above, and a reference to it at (100, 0).
fn door() -> (Drawing, Handle) {
    let src = include_str!("data/dynblock_door.txt")
        .replace("\r\n", "\n")
        .replace("1030 0\n93 0\n170 0\n171 0\n100 AcDbBlockVisibilityParameter", "1030 0\n93 10\n170 0\n171 0\n100 AcDbBlockVisibilityParameter")
        .replace("71 1\n330 30\n1010 40", "71 2\n330 30\n330 62\n1010 40")
        .replace("1011 34\n1021 -2\n1011 38\n1021 2\n", "1011 10\n1021 -2\n1011 38\n1021 50\n");
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let ins = cadcraft_doc::Insert {
        block: "Door".into(),
        insert: Vec3::new(100.0, 0.0, 0.0),
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

/// Where the reference's visibility grip is.
fn visibility_grip(d: &Drawing, r: Handle) -> Vec2 {
    grips(d, r).into_iter().find(|g| g.shape == GripShape::List && g.property == "Visibility1").map(|g| g.at).unwrap()
}

fn near(a: Vec2, b: (f64, f64)) -> bool {
    (a.x - b.0).abs() < 1e-9 && (a.y - b.1).abs() < 1e-9
}

#[test]
fn a_stretch_carries_the_visibility_grip() {
    let (mut d, r) = door();
    assert!(near(visibility_grip(&d, r), (118.0, 40.0)), "{:?}", grips(&d, r));

    // Width 36 → 30: the stretch moves its selection 6 to the left, Visibility1 with it.
    set_property(&mut d, r, "Width", "30").unwrap();
    assert!(near(visibility_grip(&d, r), (112.0, 40.0)), "{:?}", visibility_grip(&d, r));
    let state = |d: &Drawing| reference(d, r).and_then(|x| x.values.get(&1).cloned());
    assert_eq!(
        serde_json::to_value(state(&d)).unwrap(),
        serde_json::json!({ "type": "text", "value": "Open", "at": { "x": 12.0, "y": 40.0, "z": 0.0 } })
    );

    // Saved and read back where it went.
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert!(near(visibility_grip(&again, r), (112.0, 40.0)));

    // Another state: the grip stays where the stretch put it.
    set_property(&mut d, r, "Visibility1", "Closed").unwrap();
    assert!(near(visibility_grip(&d, r), (112.0, 40.0)));
}
