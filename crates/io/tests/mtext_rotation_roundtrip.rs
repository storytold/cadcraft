//! MTEXT rotation survives DXF and DWG round trips at any angle, and DXF files from other programs
//! that give the direction as a vector (groups 11/21/31) or as an angle (group 50) are read (#900).

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

fn mtext(d: &Drawing) -> MText {
    d.model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap()
}

fn turned(degrees: f64) -> Drawing {
    let mut d = Drawing::new_imperial();
    let t = MText {
        insert: Vec3::new(1.0, 2.0, 0.0),
        height: 0.5,
        width: 10.0,
        attach: 5,
        rotation: degrees.to_radians(),
        style: "Standard".into(),
        contents: "Hello".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    d
}

/// Same direction, whatever the turn count (DWG keeps angles in 0..360).
fn same_angle(got: f64, want: f64) -> bool {
    ((got - want + 540.0).rem_euclid(360.0) - 180.0).abs() < 1e-6
}

#[test]
fn the_angle_survives_a_dxf_and_a_dwg_round_trip() {
    for degrees in [0.0, 30.0, 45.0, 90.0, 180.0, 270.0, 359.0, -45.0] {
        for name in ["t.dxf", "t.dwg"] {
            let back = cadcraft_io::read(&cadcraft_io::write(&turned(degrees), name).unwrap(), name).unwrap();
            let t = mtext(&back);
            assert!(same_angle(t.rotation.to_degrees(), degrees), "{name} {degrees}: {}", t.rotation.to_degrees());
            // Nothing else moves with it.
            assert_eq!((t.attach, t.width, t.height, t.contents.as_str()), (5, 10.0, 0.5, "Hello"), "{name} {degrees}");
        }
    }
}

fn dxf_with(direction: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nMTEXT\n5\n2A\n8\n0\n100\nAcDbEntity\n100\nAcDbMText\n10\n1.0\n20\n2.0\n30\n0.0\n40\n0.5\n41\n10.0\n71\n1\n72\n5\n1\nHello\n{direction}0\nENDSEC\n0\nEOF\n"
    )
}

#[test]
fn a_direction_vector_or_an_angle_from_another_program_is_read() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let cases = [
        ("vector only", format!("11\n{s}\n21\n{s}\n31\n0.0\n"), 45.0),
        ("angle only", "50\n45.0\n".to_string(), 45.0),
        ("both", format!("11\n{s}\n21\n{s}\n31\n0.0\n50\n45.0\n"), 45.0),
        ("vertical vector", "11\n0.0\n21\n1.0\n31\n0.0\n".to_string(), 90.0),
        ("vector into the second quadrant", format!("11\n-{s}\n21\n{s}\n31\n0.0\n"), 135.0),
    ];
    for (what, group, want) in cases {
        let d = cadcraft_io::read(dxf_with(&group).as_bytes(), "x.dxf").unwrap();
        assert!(same_angle(mtext(&d).rotation.to_degrees(), want), "{what}: {}", mtext(&d).rotation.to_degrees());
    }
}
