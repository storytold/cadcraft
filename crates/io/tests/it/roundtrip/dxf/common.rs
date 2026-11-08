//! Independent input and output contracts for common LINE/CIRCLE/TEXT properties. Read fixtures
//! are handwritten DXF, never writer output; write expectations are literal groups, never a
//! reread Drawing. Absence, explicit zero and inheritance are distinct. Pending tests localize
//! the existing extrusion and ByBlock plot-style losses without accepting them as correct.
//! References: https://ezdxf.readthedocs.io/en/stable/dxfentities/dxfgfx.html
//! https://ezdxf.readthedocs.io/en/stable/concepts/true_color.html
//! https://ezdxf.readthedocs.io/en/stable/concepts/transparency.html

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Layer, Lineweight, Transparency};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

fn fixture(properties: &str) -> Drawing {
    let records = [
        ("LINE", "AcDbLine", "10\n1\n20\n2\n30\n3\n11\n4\n21\n5\n31\n6\n"),
        ("CIRCLE", "AcDbCircle", "10\n7\n20\n8\n30\n9\n40\n2.5\n"),
        ("TEXT", "AcDbText", "10\n3\n20\n4\n40\n2.5\n1\nfixture\n"),
    ];
    let mut bytes = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for (kind, subclass, geometry) in records {
        bytes.push_str(&format!("0\n{kind}\n100\nAcDbEntity\n{properties}100\n{subclass}\n{geometry}"));
    }
    bytes.push_str("0\nENDSEC\n0\nEOF\n");
    read(bytes.as_bytes(), "fixture.dxf").unwrap()
}

fn assert_read(properties: &str, expected: &Common) {
    let drawing = fixture(properties);
    assert_eq!(drawing.model.len(), 3);
    assert!(drawing.skipped_records.is_empty());
    for entity in drawing.model.iter() {
        assert_eq!(&entity.common, expected, "{} with groups {properties:?}", entity.kind.type_name());
    }
}

fn output(common: &Common, version: Version, binary: bool) -> Vec<(String, Vec<Tag>)> {
    let mut drawing = super::conformance::model();
    if drawing.layer(&common.layer).is_none() {
        drawing.layers.push(Layer::new(&common.layer));
    }
    let handles: Vec<_> = drawing.model.iter().map(|e| e.handle).collect();
    for handle in handles {
        drawing.model.modify(handle, |e| e.common = common.clone());
    }
    let bytes = write_with(&drawing, "memory.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
    let tags = cadcraft_dxf::parse(&bytes).unwrap();
    let sections = sections(&tags);
    let entities = records(&sections.iter().find(|s| s.name == "ENTITIES").unwrap().tags);
    assert_eq!(entities.iter().map(|(kind, _)| kind.as_str()).collect::<Vec<_>>(), ["LINE", "CIRCLE", "TEXT"]);
    entities
}

fn assert_write(common: Common, present: &[Tag], absent: &[i32]) {
    for version in Version::ALL {
        for binary in [false, true] {
            for (kind, groups) in output(&common, version, binary) {
                let context = format!("{version} binary={binary} {kind} {common:?}");
                for tag in present {
                    assert_eq!(groups.iter().filter(|t| t.code == tag.code).collect::<Vec<_>>(), [tag], "{context}: group {}", tag.code);
                }
                for code in absent {
                    assert!(!groups.iter().any(|t| t.code == *code), "{context}: group {code} should be omitted");
                }
            }
        }
    }
}

#[test]
fn missing_optional_groups_and_explicit_defaults() {
    assert_read("", &Common::default());
    assert_read("8\n0\n6\nByLayer\n62\n256\n370\n-1\n48\n1\n60\n0\n39\n0\n210\n0\n220\n0\n230\n1\n", &Common::default());
    // Omission is this writer's convention, not a requirement that other DXF writers omit them.
    assert_write(Common::default(), &[Tag::s(8, "0")], &[6, 62, 370, 48, 60, 39, 420, 440, 210, 220, 230]);
}

#[test]
fn layer_linetype_scale_visibility_and_thickness_have_independent_groups() {
    let common =
        Common { layer: "Fixture".into(), linetype: "Continuous".into(), ltscale: 2.75, visible: false, thickness: -1.25, ..Common::default() };
    assert_read("8\nFixture\n6\nContinuous\n48\n2.75\n60\n1\n39\n-1.25\n", &common);
    assert_write(common, &[Tag::s(8, "Fixture"), Tag::s(6, "Continuous"), Tag::f(48, 2.75), Tag::i(60, 1), Tag::f(39, -1.25)], &[62, 420, 440]);
    assert_read("6\nByBlock\n", &Common { linetype: "ByBlock".into(), ..Common::default() });
    assert_write(Common { linetype: "ByBlock".into(), ..Common::default() }, &[Tag::s(6, "ByBlock")], &[]);
}

#[test]
fn lineweight_distinguishes_logical_modes_and_explicit_zero() {
    for (value, lineweight) in [
        (-1, Lineweight::ByLayer),
        (-2, Lineweight::ByBlock),
        (-3, Lineweight::Default),
        (0, Lineweight::Mm100(0)),
        (53, Lineweight::Mm100(53)),
        (211, Lineweight::Mm100(211)),
    ] {
        let common = Common { lineweight, ..Common::default() };
        assert_read(&format!("370\n{value}\n"), &common);
        if value == -1 {
            assert_write(common, &[], &[370]);
        } else {
            assert_write(common, &[Tag::i(370, value)], &[]);
        }
    }
}

#[test]
fn aci_color_distinguishes_inheritance_and_explicit_color() {
    for (value, color) in
        [(256, Color::ByLayer), (0, Color::ByBlock)].into_iter().chain((1..=255).map(|index| (i64::from(index), Color::Index(index))))
    {
        let common = Common { color, ..Common::default() };
        assert_read(&format!("62\n{value}\n"), &common);
        if value == 256 {
            assert_write(common, &[], &[62, 420]);
        } else {
            assert_write(common, &[Tag::i(62, value)], &[420]);
        }
    }
}

#[test]
fn true_color_overrides_aci_in_either_tag_order_including_black() {
    // 0x12_34_56 = 1,193,046. The black value 0 must not be mistaken for an absent group.
    for (value, rgb) in [(1_193_046, Rgb(0x12, 0x34, 0x56)), (0, Rgb(0, 0, 0))] {
        let common = Common { color: Color::True(rgb), ..Common::default() };
        for aci in [0, 1, 256] {
            assert_read(&format!("62\n{aci}\n420\n{value}\n"), &common);
            assert_read(&format!("420\n{value}\n62\n{aci}\n"), &common);
        }
        assert_write(common, &[Tag::i(420, value)], &[]);
    }
}

#[test]
fn transparency_distinguishes_inheritance_and_explicit_opacity() {
    assert_read("", &Common::default());
    assert_write(Common::default(), &[], &[440]);
    // High byte: 01 = ByBlock, 02 = explicit alpha. Low byte: opacity, not transparency.
    // Independent integer oracle: floor(255 * (1 - p/100)) = 255 - ceil(255*p/100).
    // No production alpha conversion is used. Check every supported integer percentage.
    for (raw, transparency) in std::iter::once((0x0100_0000, Transparency::ByBlock)).chain((0..=90).map(|percent| {
        let opacity = 255 - (255 * i64::from(percent) + 99) / 100;
        (0x0200_0000 + opacity, Transparency::Percent(percent))
    })) {
        let common = Common { transparency, ..Common::default() };
        assert_read(&format!("440\n{raw}\n"), &common);
        assert_write(common, &[Tag::i(440, raw)], &[]);
    }
    // Another writer may round up instead; both neighboring alpha values map to the same
    // integer percentage. This is a read expectation, not normalization of writer output.
    assert_read("440\n33554560\n", &Common { transparency: Transparency::Percent(50), ..Common::default() });
    assert_read("440\n33554458\n", &Common { transparency: Transparency::Percent(90), ..Common::default() });
}

#[test]
fn reads_line_extrusion_without_changing_world_space_endpoints() {
    for (groups, expected) in [("210\n0\n220\n0\n230\n-1\n", Vec3::new(0.0, 0.0, -1.0)), ("220\n1\n230\n0\n", Vec3::new(0.0, 1.0, 0.0))] {
        let drawing = fixture(groups);
        let line = drawing.model.iter().next().unwrap();
        assert_eq!(line.common.extrusion, expected);
        let EntityKind::Line(geometry) = &line.kind else { panic!("expected LINE") };
        assert_eq!(geometry.a, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(geometry.b, Vec3::new(4.0, 5.0, 6.0));
    }
}

#[test]
#[ignore = "Known writer loss: a non-default extrusion is omitted; the independent LINE read test passes"]
fn writes_nondefault_extrusion_groups() {
    // X/Y may be omitted at zero, but Z=-1 must not disappear into the default Z=1.
    assert_write(Common { extrusion: Vec3::new(0.0, 0.0, -1.0), ..Common::default() }, &[Tag::f(230, -1.0)], &[]);
}

#[test]
#[ignore = "Known writer loss: ByLayer and ByBlock plot styles produce identical files"]
fn writer_must_distinguish_bylayer_from_byblock_plot_style() {
    // The available group-380 HEADER enum is not sufficient evidence for a universal entity
    // encoding; do not prescribe an unverified tag here. Identical complete outputs prove the
    // loss is already in the writer, independently of any reader or representation choice.
    let by_layer = super::conformance::model();
    let mut by_block = by_layer.clone();
    let handles: Vec<_> = by_block.model.iter().map(|e| e.handle).collect();
    for handle in handles {
        by_block.model.modify(handle, |e| e.common.plot_style = "ByBlock".into());
    }
    for version in Version::ALL {
        for binary in [false, true] {
            let options = SaveOptions { version: Some(version), binary };
            let a = write_with(&by_layer, "memory.dxf", &options).unwrap();
            let b = write_with(&by_block, "memory.dxf", &options).unwrap();
            assert!(a != b, "{version} binary={binary}: plot style is not encoded anywhere in the file");
        }
    }
}
