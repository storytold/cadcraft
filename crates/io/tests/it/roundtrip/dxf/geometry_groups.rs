//! Independent synthetic DXF records for the extended geometry slice. Literal group streams
//! specify both reader input and writer expectations; neither side obtains its oracle from
//! the other. Group meanings: https://ezdxf.readthedocs.io/en/stable/dxfentities/index.html

use super::super::{cases, compare};
use cadcraft_doc::{Arc, Common, Ellipse, EntityKind, LwPolyline, MText, Point, RayLine};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use cadcraft_io::{SaveOptions, Version, read, write_with};

struct Fixture {
    wire_type: &'static str,
    kind: EntityKind,
    groups: &'static str,
    output: Vec<Tag>,
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            wire_type: "ARC",
            kind: EntityKind::Arc(Arc { center: Vec3::new(1.25, -2.5, 3.75), radius: 2.5, start: 0.0, end: std::f64::consts::FRAC_PI_2 }),
            groups: "10\n1.25\n20\n-2.5\n30\n3.75\n40\n2.5\n50\n0\n51\n90\n",
            output: vec![Tag::f(10, 1.25), Tag::f(20, -2.5), Tag::f(30, 3.75), Tag::f(40, 2.5), Tag::f(50, 0.0), Tag::f(51, 90.0)],
        },
        Fixture {
            wire_type: "ELLIPSE",
            kind: EntityKind::Ellipse(Ellipse {
                center: Vec3::new(1.0, 2.0, 3.0),
                major: Vec3::new(3.0, 4.0, 0.0),
                ratio: 0.5,
                start: 0.0,
                end: std::f64::consts::TAU,
            }),
            groups: "10\n1\n20\n2\n30\n3\n11\n3\n21\n4\n31\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n",
            output: vec![Tag::f(11, 3.0), Tag::f(21, 4.0), Tag::f(31, 0.0), Tag::f(40, 0.5), Tag::f(41, 0.0), Tag::f(42, std::f64::consts::TAU)],
        },
        Fixture {
            wire_type: "LWPOLYLINE",
            kind: EntityKind::LwPolyline(LwPolyline {
                vertices: vec![
                    PolyVertex { p: Vec2::new(1.25, -2.5), bulge: 0.5, start_width: 0.125, end_width: 0.75 },
                    PolyVertex::new(Vec2::new(5.0, 3.0)),
                ],
                closed: true,
                plinegen: true,
                const_width: 0.0,
                elevation: -3.75,
            }),
            groups: "90\n2\n70\n129\n38\n-3.75\n10\n1.25\n20\n-2.5\n40\n0.125\n41\n0.75\n42\n0.5\n10\n5\n20\n3\n",
            output: vec![
                Tag::i(90, 2),
                Tag::i(70, 129),
                Tag::f(38, -3.75),
                Tag::f(10, 1.25),
                Tag::f(10, 5.0),
                Tag::f(20, -2.5),
                Tag::f(20, 3.0),
                Tag::f(40, 0.125),
                Tag::f(41, 0.75),
                Tag::f(42, 0.5),
            ],
        },
        Fixture {
            wire_type: "MTEXT",
            kind: EntityKind::MText(MText {
                insert: Vec3::new(1.25, -2.5, 3.75),
                height: 2.25,
                width: 12.5,
                attach: 5,
                rotation: std::f64::consts::FRAC_PI_2,
                style: "Standard".into(),
                contents: "first\\Psecond".into(),
                line_spacing: 1.75,
                line_spacing_exact: true,
                columns: None,
            }),
            groups: "10\n1.25\n20\n-2.5\n30\n3.75\n40\n2.25\n41\n12.5\n71\n5\n50\n90\n7\nStandard\n1\nfirst\\Psecond\n73\n2\n44\n1.75\n",
            output: vec![
                Tag::f(40, 2.25),
                Tag::f(41, 12.5),
                Tag::i(71, 5),
                Tag::f(50, 90.0),
                Tag::s(7, "Standard"),
                Tag::s(1, "first\\Psecond"),
                Tag::i(73, 2),
                Tag::f(44, 1.75),
            ],
        },
        Fixture {
            wire_type: "POINT",
            kind: EntityKind::Point(Point { p: Vec3::new(1.0, 2.0, 3.0), angle: 0.0 }),
            groups: "10\n1\n20\n2\n30\n3\n",
            output: vec![Tag::f(10, 1.0), Tag::f(20, 2.0), Tag::f(30, 3.0)],
        },
        Fixture {
            wire_type: "RAY",
            kind: EntityKind::Ray(RayLine { base: Vec3::new(1.0, 2.0, 3.0), dir: Vec3::new(0.0, 1.0, 0.0) }),
            groups: "10\n1\n20\n2\n30\n3\n11\n0\n21\n1\n31\n0\n",
            output: vec![Tag::f(11, 0.0), Tag::f(21, 1.0), Tag::f(31, 0.0)],
        },
    ]
}

#[test]
fn reads_extended_entities_from_independent_groups() {
    for fixture in fixtures() {
        let name = fixture.wire_type;
        let bytes = format!("0\nSECTION\n2\nENTITIES\n0\n{name}\n5\n100\n{}0\nENDSEC\n0\nEOF\n", fixture.groups);
        let expected = cases::entity_case(name, Common::default(), fixture.kind.clone());
        let back = read(bytes.as_bytes(), "fixture.dxf").unwrap();
        compare::model(&expected.drawing, &back, name);
    }
}

#[test]
fn writes_extended_entities_with_independent_groups() {
    for fixture in fixtures() {
        let name = fixture.wire_type;
        let source = cases::entity_case(name, Common::default(), fixture.kind.clone());
        for version in Version::ALL {
            for binary in [false, true] {
                let bytes = write_with(&source.drawing, "memory.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
                let tags = cadcraft_dxf::parse(&bytes).unwrap();
                let sections = sections(&tags);
                let records = records(&sections.iter().find(|s| s.name == "ENTITIES").unwrap().tags);
                assert_eq!(records.len(), 1);
                let (kind, groups) = &records[0];
                assert_eq!(kind, name);
                for code in fixture.output.iter().map(|t| t.code) {
                    let expected: Vec<_> = fixture.output.iter().filter(|t| t.code == code).collect();
                    let actual: Vec<_> = groups.iter().filter(|t| t.code == code).collect();
                    assert_eq!(actual, expected, "{name} {version} binary={binary} group={code}");
                }
            }
        }
    }
}
