//! MTEXT columns and paragraph codes survive a DXF round trip (#532): the column groups
//! 75/76/78/79/48/49/50, tabs as `^I`, carets as `^ `.
//! Column width must not also become the entity's Common.ltscale (#2247).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, MText, MTextColumnType, MTextColumns, Space};
use cadcraft_dxf::Tag;
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, entity_from_groups, read_dxf, write_dxf, write_with};

fn mtext(contents: &str, rotation: f64, columns: Option<MTextColumns>) -> MText {
    MText {
        insert: Vec3::new(1.0, 2.0, 0.0),
        height: 0.5,
        width: 6.0,
        attach: 1,
        rotation,
        style: "Standard".into(),
        contents: contents.into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns,
    }
}

fn round_trip(t: MText) -> (MText, String) {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    let dxf = write_dxf(&d);
    let back = read_dxf(dxf.as_bytes()).unwrap();
    match back.model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(t) => Some(t.clone()),
        _ => None,
    }) {
        Some(t) => (t, dxf),
        None => panic!("no mtext"),
    }
}

#[test]
fn static_columns_round_trip() {
    let c = MTextColumns { kind: MTextColumnType::Static, count: 3, gutter: 1.25, height: 7.5, ..Default::default() };
    let (t, dxf) = round_trip(mtext("one\\Ntwo", 0.0, Some(c.clone())));
    assert_eq!(t.columns, Some(c));
    assert_eq!(t.width, 6.0);
    // Group 50 after the column type is a column height, not the rotation.
    assert_eq!(t.rotation, 0.0);
    assert!(dxf.contains(" 75\r\n1\r\n"), "column type written");
}

#[test]
fn dynamic_manual_heights_and_rotation_round_trip() {
    let c = MTextColumns {
        kind: MTextColumnType::Dynamic,
        count: 3,
        gutter: 0.5,
        height: 5.0,
        auto_height: false,
        heights: vec![5.0, 3.0, 4.0],
        flow_reversed: true,
    };
    let (t, _) = round_trip(mtext("a b c", 0.5, Some(c.clone())));
    assert_eq!(t.columns, Some(c));
    assert!((t.rotation - 0.5).abs() < 1e-9);
    let auto = MTextColumns { kind: MTextColumnType::Dynamic, count: 1, gutter: 0.5, height: 4.0, auto_height: true, ..Default::default() };
    assert_eq!(round_trip(mtext("x", 0.0, Some(auto.clone()))).0.columns, Some(auto));
}

#[test]
fn paragraph_codes_tabs_and_carets_round_trip() {
    let contents = "\\pxi-3,l3,t3;1.\tFirst\\P2.\tSecond {\\H0.7x;\\S+0.1^-0.2;}";
    let (t, dxf) = round_trip(mtext(contents, 0.0, None));
    assert_eq!(t.contents, contents);
    assert!(t.columns.is_none());
    assert!(dxf.contains("1.^IFirst") && dxf.contains("^ -0.2"), "tab as ^I, caret as ^ ");
    // Older files with a bare caret keep it.
    let old = dxf.replace("^ -0.2", "^-0.2");
    let back = read_dxf(old.as_bytes()).unwrap();
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::MText(m) if m.contents == contents)));
}

#[test]
fn hostile_column_groups_do_not_panic() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mtext("x", 0.0, None))).unwrap();
    let dxf = write_dxf(&d);
    for extra in [
        " 75\r\n2\r\n 76\r\n-5\r\n 50\r\nnan\r\n",
        " 75\r\n1\r\n 76\r\n99999999\r\n 48\r\n-1\r\n 49\r\ninf\r\n",
        " 75\r\n7\r\n",
        &" 75\r\n2\r\n 50\r\n1e308\r\n".repeat(3000),
    ] {
        let bad = dxf.replacen(" 44\r\n", &format!("{extra} 44\r\n"), 1);
        let _ = read_dxf(bad.as_bytes());
    }
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Compare each read against the authored entity, including Common, rather than a first-save baseline.
fn assert_ltscale_round_trips(kind: EntityKind, ltscale: f64) -> TestResult {
    let common = Common { ltscale, color: cadcraft_color::Color::Index(3), visible: false, ..Common::default() };
    let mut authored = Drawing::new_metric();
    let handle = authored.add(&Space::Model, common.clone(), kind.clone())?;
    for version in Version::ALL {
        for binary in [false, true] {
            let options = SaveOptions { version: Some(version), binary };
            let mut drawing = authored.clone();
            for pass in 1..=2 {
                let bytes = write_with(&drawing, "group48.dxf", &options)?;
                drawing = read_dxf(&bytes)?;
                let entity = drawing.model.get(handle).ok_or("entity lost on DXF read")?;
                assert_eq!(entity.common.ltscale, ltscale, "{version:?}, binary={binary}, pass={pass}");
                assert_eq!(entity.common, common, "{version:?}, binary={binary}, pass={pass}");
                assert_eq!(entity.kind, kind, "{version:?}, binary={binary}, pass={pass}");
            }
        }
    }
    Ok(())
}

#[test]
fn uncolumned_mtext_keeps_common_ltscale() -> TestResult {
    let t = MText { width: 12.5, ..mtext("plain text", 0.0, None) };
    for ltscale in [1.0, 2.5] {
        assert_ltscale_round_trips(EntityKind::MText(t.clone()), ltscale)?;
    }
    Ok(())
}

#[test]
fn static_column_width_is_not_common_ltscale() -> TestResult {
    let columns = MTextColumns { kind: MTextColumnType::Static, count: 3, gutter: 1.25, height: 7.5, flow_reversed: true, ..Default::default() };
    let t = MText { width: 12.5, ..mtext("one\\Ntwo", 0.0, Some(columns)) };
    for ltscale in [1.0, 2.5] {
        assert_ltscale_round_trips(EntityKind::MText(t.clone()), ltscale)?;
    }
    Ok(())
}

#[test]
fn dynamic_column_width_is_not_common_ltscale() -> TestResult {
    for auto_height in [false, true] {
        let columns = MTextColumns {
            kind: MTextColumnType::Dynamic,
            count: 3,
            gutter: 1.25,
            height: 7.5,
            auto_height,
            heights: if auto_height { Vec::new() } else { vec![7.5, 5.0, 6.0] },
            flow_reversed: true,
        };
        let t = MText { width: 12.5, ..mtext("dynamic columns", 0.0, Some(columns)) };
        for ltscale in [1.0, 2.5] {
            assert_ltscale_round_trips(EntityKind::MText(t.clone()), ltscale)?;
        }
    }
    Ok(())
}

#[test]
fn line_keeps_common_ltscale() -> TestResult {
    for ltscale in [1.0, 2.5] {
        assert_ltscale_round_trips(EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 2.0, 3.0) }), ltscale)?;
    }
    Ok(())
}

#[test]
fn embedded_groups_do_not_supply_mtext_common_ltscale() -> TestResult {
    for subclass_markers in [false, true] {
        for ltscale in [1.0, 2.5] {
            let mut tags = Vec::new();
            if subclass_markers {
                tags.push(Tag::s(100, "AcDbEntity"));
            }
            tags.push(Tag::s(8, "0"));
            if ltscale != 1.0 {
                tags.push(Tag::f(48, ltscale));
            }
            if subclass_markers {
                tags.push(Tag::s(100, "AcDbMText"));
            }
            tags.extend([
                Tag::f(40, 0.5),
                Tag::f(41, 12.5),
                Tag::s(1, "own text"),
                Tag::s(101, "Embedded Object"),
                // A reused code in the embedded payload is not an entity-level property.
                Tag::f(48, 99.0),
            ]);
            let (common, kind) = entity_from_groups("MTEXT", &tags).ok_or("MTEXT was not read")?;
            assert_eq!(common.ltscale, ltscale);
            assert_eq!(common, Common { ltscale, ..Common::default() });
            assert!(matches!(kind, EntityKind::MText(t) if t.columns.is_none() && t.width == 12.5 && t.rotation == 0.0));
        }
    }
    Ok(())
}
