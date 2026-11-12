//! LEADER vertex elevations survive the shared DXF read path and DWG bridge (#2245).

use cadcraft_color::Color;
use cadcraft_doc::{Common, DimStyle, Drawing, EntityKind, Handle, Leader, Lineweight, Space};
use cadcraft_dxf::Tag;
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn drawing() -> TestResult<Drawing> {
    let mut d = Drawing::new_metric();
    d.dim_styles.push(DimStyle { name: "Elevation".into(), ..DimStyle::default() });
    let common = Common { color: Color::Index(3), lineweight: Lineweight::Mm100(25), ltscale: 2.5, visible: false, ..Common::default() };
    for vertices in [
        vec![Vec3::new(1.0, 2.0, 4.0), Vec3::new(5.0, 6.0, 7.0), Vec3::new(9.0, 10.0, -3.0)],
        vec![Vec3::new(1.0, 2.0, 0.0), Vec3::new(5.0, 6.0, 0.0), Vec3::new(9.0, 10.0, 0.0)],
    ] {
        for arrow in [false, true] {
            for spline in [false, true] {
                let leader = Leader { vertices: vertices.clone(), arrow, spline, style: "Elevation".into() };
                d.add(&Space::Model, common.clone(), EntityKind::Leader(leader))?;
            }
        }
    }
    Ok(d)
}

fn check_round_trips(name: &str, binary: bool) -> TestResult {
    let original = drawing()?;
    for version in Version::ALL {
        let mut d = original.clone();
        for pass in 1..=2 {
            let bytes = write_with(&d, name, &SaveOptions { version: Some(version), binary })?;
            d = read(&bytes, name)?;
            assert_eq!(d.model.len(), original.model.len(), "{name} {version:?}, pass={pass}");
            for expected in original.model.iter() {
                let actual = d.entity(expected.handle).ok_or("LEADER handle lost")?;
                let (EntityKind::Leader(want), EntityKind::Leader(got)) = (&expected.kind, &actual.kind) else {
                    return Err("LEADER entity type changed".into());
                };
                assert_eq!(got.vertices, want.vertices, "XYZ/order: {name} {version:?}, pass={pass}, {:?}", expected.handle);
                assert_eq!((got.arrow, got.spline, &got.style), (want.arrow, want.spline, &want.style), "LEADER properties");
                assert_eq!(actual.common, expected.common, "Common properties: {name} {version:?}, pass={pass}");
            }
        }
    }
    Ok(())
}

#[test]
fn dxf_preserves_leader_vertex_elevations() -> TestResult {
    for binary in [false, true] {
        check_round_trips("leader.dxf", binary)?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_preserves_leader_vertex_elevations() -> TestResult {
    check_round_trips("leader.dwg", false)
}

#[test]
fn missing_z_defaults_only_the_corresponding_vertex() -> TestResult {
    let vertices = [Vec3::new(1.0, 2.0, 4.0), Vec3::new(5.0, 6.0, 7.0), Vec3::new(9.0, 10.0, -3.0)];
    // All eight presence patterns include wholly 2D input and omitted first/middle/last Zs.
    // Author the groups directly, independently of CADCraft's entity writer.
    for z_mask in 0..8 {
        let mut tags = vec![
            Tag::s(0, "SECTION"),
            Tag::s(2, "ENTITIES"),
            Tag::s(0, "LEADER"),
            Tag::s(5, "10"),
            Tag::s(100, "AcDbEntity"),
            Tag::s(8, "0"),
            Tag::s(100, "AcDbLeader"),
            Tag::s(3, "Standard"),
            Tag::i(71, 0),
            Tag::i(72, 1),
            Tag::i(76, 3),
        ];
        let mut expected = Vec::new();
        for (i, p) in vertices.iter().enumerate() {
            tags.push(Tag::f(10, p.x));
            let has_z = z_mask & (1 << i) != 0;
            if has_z {
                // Y and Z can appear in either order within this vertex.
                tags.push(Tag::f(30, p.z));
            }
            tags.push(Tag::f(20, p.y));
            expected.push(Vec3::new(p.x, p.y, if has_z { p.z } else { 0.0 }));
        }
        tags.extend([Tag::s(0, "ENDSEC"), Tag::s(0, "EOF")]);
        let dxf = cadcraft_dxf::write_ascii(&tags);
        let d = read(dxf.as_bytes(), "missing-z.dxf")?;
        let e = d.entity(Handle(0x10)).ok_or("LEADER missing")?;
        let EntityKind::Leader(l) = &e.kind else { return Err("expected LEADER".into()) };
        assert_eq!(l.vertices, expected, "Z presence mask {z_mask:03b}");
        assert!(!l.arrow && l.spline);
        assert_eq!(l.style, "Standard");
        assert_eq!(e.common, Common::default());
    }
    Ok(())
}
