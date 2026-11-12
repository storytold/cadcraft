//! An R2018+ MTEXT can carry an embedded object (group 101) that repeats its geometry with
//! other meanings: there, 10 is the X-axis direction and 11 the insertion point. AutoCAD writes
//! one for MTEXT with column settings, and the DWG bridge does too. Reading 11 from it as the
//! direction turned the text towards its own insertion point (#667).

use cadcraft_doc::{Common, Drawing, EntityKind, MText, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

/// An MTEXT at (30, 17) with `rotation`, written by CADCraft, then given an embedded object
/// as AutoCAD writes it: direction `dir` in 10/20/30 and the insertion point in 11/21/31.
fn with_embedded_object(rotation: f64, dir: (f64, f64)) -> String {
    let mut d = Drawing::new_metric();
    let t = MText {
        insert: Vec3::new(30.0, 17.0, 0.0),
        height: 0.5,
        width: 6.0,
        attach: 1,
        rotation,
        style: "Standard".into(),
        contents: "Core Layer\\PThickness 1mm".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    let dxf = write_dxf(&d);
    let (dx, dy) = (dir.0.to_string(), dir.1.to_string());
    let groups = [
        (101, "Embedded Object"),
        (70, "1"),
        (10, &dx),
        (20, &dy),
        (30, "0.0"),
        (11, "30.0"),
        (21, "17.0"),
        (31, "0.0"),
        (40, "6.0"),
        (41, "0.0"),
        (42, "5.0"),
        (43, "1.0"),
        (71, "2"),
        (72, "1"),
        (44, "6.0"),
        (45, "0.5"),
        (73, "0"),
        (74, "0"),
        (46, "0.0"),
    ];
    let embedded: String = groups.iter().map(|(code, value)| format!("{code:>3}\r\n{value}\r\n")).collect();
    let start = dxf.find("AcDbMText").unwrap();
    let end = start + dxf[start..].find("\r\n  0\r\n").unwrap() + 2;
    format!("{}{embedded}{}", &dxf[..end], &dxf[end..])
}

fn read_rotation(dxf: &str) -> f64 {
    let back = read_dxf(dxf.as_bytes()).unwrap();
    match back.model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(t) => Some(t.rotation),
        _ => None,
    }) {
        Some(r) => r,
        None => panic!("no mtext"),
    }
}

#[test]
fn embedded_object_insertion_point_is_not_the_direction() {
    let dxf = with_embedded_object(0.0, (1.0, 0.0));
    assert!(dxf.contains("Embedded Object"));
    assert_eq!(read_rotation(&dxf), 0.0);
}

#[test]
fn rotated_mtext_keeps_its_rotation_with_an_embedded_object() {
    let r = 0.5_f64;
    let dxf = with_embedded_object(r, (r.cos(), r.sin()));
    assert!((read_rotation(&dxf) - r).abs() < 1e-9);
}
