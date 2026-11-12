//! Independent bridge expectations, not an independent DWG codec. Writer checks inspect native
//! objects without CADCraft's reader; reader fixtures originate in native objects without our
//! DXF writer. acadrust still supplies the binary codec in both directions, so these tests do
//! not establish interoperability with another DWG implementation. All fixtures are synthetic.

use super::super::cases;
use acadrust::types::Vector3;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfVersion, EntityType};
use cadcraft_doc::{Circle, Common, EntityKind, Line, Text};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};
use std::io::Cursor;

#[test]
fn writer_populates_native_geometry_without_our_reader() {
    let source = cases::common_case("native-writer", Common::default());
    for version in Version::ALL {
        let bytes = write_with(&source.drawing, "memory.dwg", &SaveOptions { version: Some(version), binary: false }).unwrap();
        let native = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        let entities: Vec<_> = native.entities().collect();
        assert_eq!(entities.len(), 3, "{version}");
        let EntityType::Line(line) = entities[0] else { panic!("expected native LINE") };
        assert_eq!(line.start, Vector3::new(-1.25, 2.5, 3.75), "{version}");
        assert_eq!(line.end, Vector3::new(8.125, -13.5, 21.25), "{version}");
        let EntityType::Circle(circle) = entities[1] else { panic!("expected native CIRCLE") };
        assert_eq!(circle.center, Vector3::new(-7.25, 11.5, 4.75), "{version}");
        assert_eq!(circle.radius, 3.125, "{version}");
        let EntityType::Text(text) = entities[2] else { panic!("expected native TEXT") };
        assert_eq!(text.value, "CAD Ø", "{version}");
        assert_eq!(text.insertion_point, Vector3::new(2.5, -3.75, 4.125), "{version}");
        assert_eq!(text.height, 2.25, "{version}");
        assert_eq!(text.width_factor, 1.0, "{version}");
        assert_eq!(text.rotation, 0.0, "{version}");
    }
}

#[test]
#[ignore = "Confirmed native DWG mismatch: a pi/2 linetype rotation is stored as pi/2 degrees; DXF unit interpretation needs investigation"]
fn complex_linetype_rotation_is_correct_in_native_objects() {
    // Both document APIs specify radians. Do not infer correctness from a DXF round trip:
    // reader and writer can share a unit error. Public DXF references disagree about LTYPE
    // group 50, so this asserts the end-to-end native angle, not a guessed DXF encoding.
    for shape in [false, true] {
        for case in cases::properties::complex_linetypes(shape) {
            for version in Version::ALL {
                let bytes = write_with(&case.drawing, "memory.dwg", &SaveOptions { version: Some(version), binary: false }).unwrap();
                let native = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
                let line = native.line_types.iter().find(|l| l.name == "Dash_test").unwrap();
                let elements: Vec<_> = line.elements.iter().filter_map(|e| e.complex.as_ref()).collect();
                assert_eq!(elements.len(), 1, "{} {version}", case.name);
                let rotation = elements[0].rotation;
                assert!(
                    (rotation - std::f64::consts::FRAC_PI_2).abs() <= 8.0 * f64::EPSILON,
                    "{} {version}: native rotation {rotation} radians, expected pi/2",
                    case.name
                );
            }
        }
    }
}

#[test]
fn reader_accepts_native_fixtures_without_our_writer() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut native = CadDocument::with_version(version);
        let handles = [
            native.add_entity(EntityType::Line(acadrust::entities::Line::from_coords(1.25, -2.5, 3.75, -4.5, 5.25, -6.75))).unwrap(),
            native.add_entity(EntityType::Circle(acadrust::entities::Circle::from_coords(7.25, -8.5, 9.75, 2.125))).unwrap(),
            native.add_entity(EntityType::Text(acadrust::entities::Text::with_value("Independent Ø", Vector3::new(-3.25, 4.5, 1.75)))).unwrap(),
        ];
        let expected = [
            EntityKind::Line(Line { a: Vec3::new(1.25, -2.5, 3.75), b: Vec3::new(-4.5, 5.25, -6.75) }),
            EntityKind::Circle(Circle { center: Vec3::new(7.25, -8.5, 9.75), radius: 2.125 }),
            EntityKind::Text(Text { insert: Vec3::new(-3.25, 4.5, 1.75), height: 1.0, value: "Independent Ø".into(), ..cases::text() }),
        ];
        let bytes = DwgWriter::write_to_vec(&native).unwrap();
        let back = read(&bytes, "native-fixture.dwg").unwrap();
        assert!(back.read_notes.is_empty(), "{:?}", back.read_notes);
        assert!(back.skipped_records.is_empty());
        assert_eq!(back.model.len(), expected.len());
        for ((entity, expected), handle) in back.model.iter().zip(&expected).zip(handles) {
            assert_eq!(entity.handle.0, u64::from(handle), "{version:?}");
            assert_eq!(&entity.kind, expected, "{version:?}");
            assert_eq!(entity.common, Common::default(), "{version:?}");
        }
    }
}
