//! Independent synthetic entity records prevent matching reader/writer mistakes from passing
//! the round-trip suite. Only public DXF group meanings are used; no reference-app files or assets.
//! Entity semantics: https://ezdxf.readthedocs.io/en/stable/dxfentities/text.html
//! and https://ezdxf.readthedocs.io/en/stable/dxfentities/line.html

use cadcraft_color::Color;
use cadcraft_doc::{Circle, Common, Drawing, Entity, EntityKind, HAlign, Handle, Line, Space, Text, VAlign};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

use super::compare;

const ENTITIES: &[u8] = b"0\nSECTION\n2\nENTITIES\n\
0\nLINE\n5\n100\n8\n0\n62\n3\n60\n1\n39\n-2.5\n10\n1.25\n20\n-2.5\n30\n3.75\n11\n-4.5\n21\n5.25\n31\n-6.75\n\
0\nCIRCLE\n5\n101\n8\n0\n10\n7.25\n20\n-8.5\n30\n9.75\n40\n2.125\n\
0\nTEXT\n5\n102\n8\n0\n10\n-3.25\n20\n4.5\n30\n1.75\n40\n2.5\n1\nfixture text\n50\n30\n41\n1.25\n51\n-15\n7\nStandard\n\
0\nENDSEC\n0\nEOF\n";

pub(super) fn model() -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(
        &Space::Model,
        Common { color: Color::Index(3), visible: false, thickness: -2.5, ..Common::default() },
        EntityKind::Line(Line { a: Vec3::new(1.25, -2.5, 3.75), b: Vec3::new(-4.5, 5.25, -6.75) }),
    )
    .unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(7.25, -8.5, 9.75), radius: 2.125 })).unwrap();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Text(Text {
            insert: Vec3::new(-3.25, 4.5, 1.75),
            align_pt: None,
            height: 2.5,
            value: "fixture text".into(),
            rotation: 30_f64.to_radians(),
            width_factor: 1.25,
            oblique: -15_f64.to_radians(),
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        }),
    )
    .unwrap();
    d
}

#[test]
fn reads_independent_line_circle_and_text_records() {
    let back = read(ENTITIES, "fixture.dxf").unwrap();
    compare::model(&model(), &back, "independent entity fixture");
}

#[test]
fn writes_standard_entity_groups_not_just_self_readable_data() {
    let expected = [
        (
            "LINE",
            vec![
                Tag::s(5, "100"),
                Tag::i(62, 3),
                Tag::i(60, 1),
                Tag::f(39, -2.5),
                Tag::f(10, 1.25),
                Tag::f(20, -2.5),
                Tag::f(30, 3.75),
                Tag::f(11, -4.5),
                Tag::f(21, 5.25),
                Tag::f(31, -6.75),
            ],
        ),
        ("CIRCLE", vec![Tag::s(5, "101"), Tag::f(10, 7.25), Tag::f(20, -8.5), Tag::f(30, 9.75), Tag::f(40, 2.125)]),
        (
            "TEXT",
            vec![
                Tag::s(5, "102"),
                Tag::f(10, -3.25),
                Tag::f(20, 4.5),
                Tag::f(30, 1.75),
                Tag::f(40, 2.5),
                Tag::s(1, "fixture text"),
                Tag::f(41, 1.25),
                Tag::s(7, "Standard"),
            ],
        ),
    ];
    for version in Version::ALL {
        for binary in [false, true] {
            let bytes = write_with(&model(), "memory.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
            let tags = cadcraft_dxf::parse(&bytes).unwrap();
            let sections = sections(&tags);
            let entities = records(&sections.iter().find(|s| s.name == "ENTITIES").unwrap().tags);
            assert_eq!(entities.len(), expected.len());
            for ((kind, groups), (expected_kind, expected_groups)) in entities.iter().zip(&expected) {
                let context = format!("{version} binary={binary} {expected_kind}");
                assert_eq!(kind, expected_kind, "{context}");
                for tag in expected_groups {
                    let values: Vec<_> = groups.iter().filter(|t| t.code == tag.code).collect();
                    assert_eq!(values, [tag], "{context}: group {}", tag.code);
                }
                if kind == "TEXT" {
                    for (code, degrees) in [(50, 30.0), (51, -15.0)] {
                        let values: Vec<_> = groups.iter().filter(|t| t.code == code).collect();
                        assert_eq!(values.len(), 1, "{context}: group {code}");
                        assert!((values[0].f64() - degrees).abs() < 1e-13, "{context}: group {code} is in degrees");
                    }
                }
            }
        }
    }
}

#[test]
fn reads_text_alignment_codes_independently_of_the_writer() {
    let horizontal = [(0, HAlign::Left), (1, HAlign::Center), (2, HAlign::Right), (3, HAlign::Aligned), (4, HAlign::Middle), (5, HAlign::Fit)];
    let vertical = [(0, VAlign::Baseline), (1, VAlign::Bottom), (2, VAlign::Middle), (3, VAlign::Top)];
    for (hcode, halign) in horizontal {
        for (vcode, valign) in vertical {
            if hcode >= 3 && vcode != 0 {
                continue;
            }
            let bytes = format!(
                "0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n3\n20\n4\n30\n5\n40\n2.5\n1\nA\n72\n{hcode}\n73\n{vcode}\n11\n6\n21\n7\n31\n8\n0\nENDSEC\n0\nEOF\n"
            );
            let back = read(bytes.as_bytes(), "fixture.dxf").unwrap();
            let EntityKind::Text(text) = &back.model.iter().next().unwrap().kind else { panic!("expected TEXT") };
            assert_eq!((text.halign, text.valign), (halign, valign), "72={hcode}, 73={vcode}");
            assert_eq!(text.insert, Vec3::new(3.0, 4.0, 5.0));
            let expected_point = if hcode == 0 && vcode == 0 { None } else { Some(Vec3::new(6.0, 7.0, 8.0)) };
            assert_eq!(text.align_pt, expected_point, "72={hcode}, 73={vcode}");
        }
    }
}

// Odd values above 2^53, above i64::MAX and near u64::MAX expose floating-point coercion,
// signed conversion and truncation. Literal hex and numeric values are independent expectations.
const LARGE_HANDLES: [(u64, &str); 3] =
    [(9_007_199_254_740_993, "20000000000001"), (9_223_372_036_854_775_809, "8000000000000001"), (18_446_744_073_709_486_081, "FFFFFFFFFFFF0001")];

#[test]
fn reads_large_literal_hex_handles_without_numeric_coercion() {
    let mut fixture = String::from("0\nSECTION\n2\nENTITIES\n");
    for (_, hex) in LARGE_HANDLES {
        fixture.push_str(&format!("0\nLINE\n5\n{hex}\n8\n0\n10\n0\n20\n0\n11\n1\n21\n2\n"));
    }
    fixture.push_str("0\nENDSEC\n0\nEOF\n");
    let back = read(fixture.as_bytes(), "fixture.dxf").unwrap();
    assert!(back.read_notes.is_empty());
    assert!(back.skipped_records.is_empty());
    assert_eq!(back.model.iter().map(|e| e.handle.0).collect::<Vec<_>>(), LARGE_HANDLES.map(|(numeric, _)| numeric));
}

#[test]
fn writes_large_handles_as_exact_hex_groups() {
    let mut drawing = Drawing::new_metric();
    for (numeric, _) in LARGE_HANDLES {
        drawing.model.push(Entity::new(Handle(numeric), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 2.0, 0.0) })));
    }
    drawing.handseed = 0xffff_ffff_ffff_1000;
    for version in Version::ALL {
        for binary in [false, true] {
            let bytes = write_with(&drawing, "memory.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
            let tags = cadcraft_dxf::parse(&bytes).unwrap();
            let sections = sections(&tags);
            let entities = records(&sections.iter().find(|s| s.name == "ENTITIES").unwrap().tags);
            assert_eq!(entities.len(), LARGE_HANDLES.len());
            for ((kind, groups), (_, hex)) in entities.iter().zip(LARGE_HANDLES) {
                assert_eq!(kind, "LINE");
                assert_eq!(groups.iter().filter(|t| t.code == 5).collect::<Vec<_>>(), [&Tag::s(5, hex)], "{version} binary={binary} handle={hex}");
            }
        }
    }
}

#[test]
fn left_baseline_text_ignores_the_unused_second_alignment_point() {
    // DXF TEXT groups 11/21/31 are meaningful only when 72 or 73 is nonzero. The model
    // intentionally represents the unused point as None, not Some(Vec3::ZERO).
    let bytes = b"0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n3\n20\n4\n40\n2.5\n1\nA\n72\n0\n73\n0\n11\n0\n21\n0\n31\n0\n0\nENDSEC\n0\nEOF\n";
    let back = read(bytes, "fixture.dxf").unwrap();
    let EntityKind::Text(text) = &back.model.iter().next().unwrap().kind else { panic!("expected TEXT") };
    assert_eq!(text.align_pt, None);
    assert_eq!(text.insert, Vec3::new(3.0, 4.0, 0.0));
}
