//! MTEXT columns and paragraph codes survive a DXF round trip (#532): the column groups
//! 75/76/78/79/48/49/50, tabs as `^I`, carets as `^ `.

use cadcraft_doc::{Common, Drawing, EntityKind, MText, MTextColumnType, MTextColumns, Space};
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
fn legacy_column_groups_stay_in_the_mtext_subclass() {
    // Columns as other programs write them (#2247): the column width (48) and the height count
    // (the first 50) are groups of the AcDbMText subclass, not the entity's linetype scale and a
    // column height.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mtext("x", 0.0, None))).unwrap();
    let legacy =
        " 75\r\n1\r\n 76\r\n3\r\n 78\r\n0\r\n 79\r\n0\r\n 48\r\n12.5\r\n 49\r\n2.5\r\n 50\r\n3\r\n 50\r\n20.0\r\n 50\r\n20.0\r\n 50\r\n20.0\r\n";
    let dxf = write_dxf(&d).replacen(" 44\r\n1.0\r\n", &format!(" 44\r\n1.0\r\n{legacy}"), 1);
    assert!(dxf.contains(" 75\r\n"), "legacy groups inserted");
    let back = read_dxf(dxf.as_bytes()).unwrap();
    let Some(e) = back.model.iter().find(|e| matches!(e.kind, EntityKind::MText(_))) else { panic!("no mtext") };
    assert_eq!(e.common.ltscale, 1.0, "48 is the column width, not the linetype scale");
    let EntityKind::MText(t) = &e.kind else { panic!("no mtext") };
    let c = MTextColumns { kind: MTextColumnType::Static, count: 3, gutter: 2.5, height: 20.0, ..Default::default() };
    assert_eq!(t.columns, Some(c));
    // Written back as read: the height count before the heights.
    let out = write_dxf(&back);
    assert!(out.contains(" 50\r\n3\r\n 50\r\n20.0\r\n"), "height count before the heights");
}

#[test]
fn heights_written_without_a_count_are_all_kept() {
    // Files written before the height count (CADCraft up to f536bf4) have group 76 = n and n
    // heights, no count: the first 50 is a height there (#2247 review).
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mtext("x", 0.0, None))).unwrap();
    let dxf = write_dxf(&d);
    for (heights, old) in [
        (vec![7.0], " 75\r\n2\r\n 76\r\n1\r\n 78\r\n0\r\n 79\r\n0\r\n 48\r\n12.5\r\n 49\r\n2.5\r\n 50\r\n7.0\r\n"),
        (
            vec![5.0, 3.0, 4.0],
            " 75\r\n2\r\n 76\r\n3\r\n 78\r\n0\r\n 79\r\n0\r\n 48\r\n12.5\r\n 49\r\n2.5\r\n 50\r\n5.0\r\n 50\r\n3.0\r\n 50\r\n4.0\r\n",
        ),
    ] {
        let back = read_dxf(dxf.replacen(" 44\r\n1.0\r\n", &format!(" 44\r\n1.0\r\n{old}"), 1).as_bytes()).unwrap();
        let Some(EntityKind::MText(t)) = back.model.iter().map(|e| &e.kind).find(|k| matches!(k, EntityKind::MText(_))) else { panic!("no mtext") };
        let n = heights.len();
        let c = MTextColumns {
            kind: MTextColumnType::Dynamic,
            count: n as u16,
            gutter: 2.5,
            height: heights.iter().copied().fold(0.0, f64::max),
            auto_height: false,
            heights: heights.clone(),
            flow_reversed: false,
        };
        assert_eq!(t.columns, Some(c), "{n} heights without a count");
        // Written back with the count, and still n columns.
        let out = write_dxf(&back);
        assert!(out.contains(&format!(" 76\r\n{n}\r\n 78\r\n")), "76 stays {n}: {out}");
        assert!(out.contains(&format!(" 50\r\n{n}\r\n 50\r\n{:.1}\r\n", heights[0])), "count then the first height: {out}");
    }
}

#[test]
fn heights_inside_an_application_group_are_not_column_heights() {
    // An application-defined group (102 {...}) in the record may hold a 50 of its own: it is
    // neither a height nor the count that tells a legacy record apart (#2247 review).
    let tags = [
        Tag::s(8, "0"),
        Tag::f(40, 0.5),
        Tag::f(41, 12.5),
        Tag::s(1, "x"),
        Tag::i(75, 2),
        Tag::i(76, 3),
        Tag::i(78, 0),
        Tag::i(79, 0),
        Tag::f(48, 12.5),
        Tag::f(49, 2.5),
        Tag::f(50, 5.0),
        Tag::f(50, 3.0),
        Tag::f(50, 4.0),
        Tag::s(102, "{ACME_APP"),
        Tag::f(50, 8.0),
        Tag::s(102, "}"),
    ];
    let Some((_, EntityKind::MText(t))) = entity_from_groups("MTEXT", &tags) else { panic!("MTEXT was not read") };
    let c = MTextColumns {
        kind: MTextColumnType::Dynamic,
        count: 3,
        gutter: 2.5,
        height: 5.0,
        auto_height: false,
        heights: vec![5.0, 3.0, 4.0],
        flow_reversed: false,
    };
    assert_eq!(t.columns, Some(c));
}

#[test]
fn empty_manual_heights_round_trip() {
    // A dynamic manual column with no heights stays without heights: no count is written for an
    // empty list (a lone 50 = 0 would read back as one height of 0) (#2247 review).
    let c = MTextColumns {
        kind: MTextColumnType::Dynamic,
        count: 1,
        gutter: 0.5,
        height: 0.0,
        auto_height: false,
        heights: Vec::new(),
        flow_reversed: false,
    };
    let (t, dxf) = round_trip(mtext("x", 0.0, Some(c.clone())));
    assert_eq!(t.columns, Some(c));
    assert!(!dxf.contains(" 50\r\n0\r\n"), "no height count for an empty list: {dxf}");
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

#[test]
fn embedded_groups_do_not_supply_mtext_common_ltscale() {
    // Without subclass markers, an embedded object (101) reuses 48 for its own data (#2247, test from #2276).
    let tags = [Tag::s(8, "0"), Tag::f(40, 0.5), Tag::f(41, 12.5), Tag::s(1, "own text"), Tag::s(101, "Embedded Object"), Tag::f(48, 99.0)];
    let Some((common, _)) = entity_from_groups("MTEXT", &tags) else { panic!("MTEXT was not read") };
    assert_eq!(common.ltscale, 1.0, "48 after 101 belongs to the embedded object");
}

#[test]
fn layer_after_an_entmake_subclass_marker_is_kept() {
    // An entity list built by hand may put a subclass marker before the layer and have no AcDbEntity
    // marker at all: the whole list is then the entity's (#2247 review).
    let tags = [Tag::s(100, "AcDbLine"), Tag::s(8, "Walls"), Tag::i(62, 3), Tag::f(10, 0.0), Tag::f(20, 0.0), Tag::f(11, 1.0), Tag::f(21, 0.0)];
    let Some((common, _)) = entity_from_groups("LINE", &tags) else { panic!("LINE was not read") };
    assert_eq!(common.layer, "Walls");
    assert_eq!(common.color, cadcraft_color::Color::from_aci(3));
}

#[test]
fn markers_inside_an_application_group_do_not_end_the_entity_groups() {
    // An application-defined group (102 {...} before AcDbEntity) may hold any code, 100 and 101
    // included: the entity's layer and colour come after it (#2247 review).
    let tags = [
        Tag::s(5, "A1"),
        Tag::s(102, "{ACME_APP"),
        Tag::s(100, "AcmeMarker"),
        Tag::s(101, "AcmeObject"),
        Tag::s(102, "}"),
        Tag::s(100, "AcDbEntity"),
        Tag::s(8, "Walls"),
        Tag::i(62, 3),
        Tag::s(100, "AcDbLine"),
        Tag::f(10, 0.0),
        Tag::f(20, 0.0),
        Tag::f(11, 1.0),
        Tag::f(21, 0.0),
    ];
    let Some((common, _)) = entity_from_groups("LINE", &tags) else { panic!("LINE was not read") };
    assert_eq!(common.layer, "Walls");
    assert_eq!(common.color, cadcraft_color::Color::from_aci(3));
}

// ---- #2248: R2018 embedded column object ----

fn embedded_tags(count: i64, width: f64, gutter: f64, heights: &[f64]) -> Vec<cadcraft_dxf::Tag> {
    use cadcraft_dxf::Tag;
    let mut v = vec![
        Tag::s(8, "0"),
        Tag::f(10, 10.0),
        Tag::f(20, 10.0),
        Tag::f(30, 0.0),
        Tag::f(40, 2.5),
        Tag::f(41, width),
        Tag::i(71, 5),
        Tag::i(72, 5),
        Tag::s(1, "Hello"),
        Tag::s(101, "Embedded Object"),
        Tag::i(70, 1),
        Tag::f(10, 1.0),
        Tag::f(20, 0.0),
        Tag::f(30, 0.0),
        Tag::f(11, 10.0),
        Tag::f(21, 10.0),
        Tag::f(31, 0.0),
        Tag::f(40, width),
        Tag::f(41, 0.0),
        Tag::f(42, width),
        Tag::f(43, 0.0),
        Tag::i(71, 1),
        Tag::i(72, count),
        Tag::f(44, width),
        Tag::f(45, gutter),
        Tag::i(73, 0),
        Tag::i(74, 0),
    ];
    v.extend(heights.iter().map(|h| Tag::f(46, *h)));
    v
}

fn mtext_from_groups(tags: &[cadcraft_dxf::Tag]) -> (Common, MText) {
    let Some((common, EntityKind::MText(t))) = entity_from_groups("MTEXT", tags) else { panic!("MTEXT was not read") };
    (common, t)
}

/// The DXF of `t` written as `version` and its MTEXT record only (other sections reuse the codes).
fn dxf_as(t: &MText, version: Version) -> (String, String) {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::MText(t.clone())).unwrap();
    let bytes = write_with(&d, "x.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let from = text.find("  0\r\nMTEXT\r\n").expect("an MTEXT");
    let len = text[from + 5..].find("  0\r\n").map_or(text.len() - from, |n| n + 5);
    let record = text[from..from + len].to_string();
    (text, record)
}

#[test]
fn embedded_object_does_not_leak_into_the_entity() {
    // A single column (the usual R2018 block of a plain MTEXT): 44 is the column width, not the
    // line spacing factor, and the columns are kept.
    let (_, t) = mtext_from_groups(&embedded_tags(1, 38.8, 12.5, &[0.0]));
    assert_eq!(t.line_spacing, 1.0, "44 of the embedded object is the column width");
    assert_eq!(t.attach, 5);
    assert_eq!(t.width, 38.8);
    let c = MTextColumns { kind: MTextColumnType::Static, count: 1, gutter: 12.5, height: 0.0, ..Default::default() };
    assert_eq!(t.columns, Some(c));
}

#[test]
fn embedded_static_columns_are_read() {
    let (_, t) = mtext_from_groups(&embedded_tags(3, 12.5, 2.5, &[20.0, 20.0, 20.0]));
    let c = MTextColumns { kind: MTextColumnType::Static, count: 3, gutter: 2.5, height: 20.0, ..Default::default() };
    assert_eq!(t.columns, Some(c));
}

#[test]
fn embedded_column_height_is_the_entitys_defined_height() {
    // As ezdxf and AutoCAD write static columns: the height in the entity's 46, none in the object.
    let mut tags = embedded_tags(3, 12.5, 2.5, &[]);
    let at = tags.iter().position(|t| t.code == 101).unwrap();
    tags.insert(at, cadcraft_dxf::Tag::f(46, 20.0));
    let (_, t) = mtext_from_groups(&tags);
    assert_eq!(t.columns.map(|c| c.height), Some(20.0));
}

#[test]
fn r2018_writes_the_embedded_object_and_older_versions_the_legacy_groups() {
    let (_, t) = mtext_from_groups(&embedded_tags(3, 12.5, 2.5, &[20.0, 20.0, 20.0]));
    let (full, r2018) = dxf_as(&t, Version::R2018);
    assert!(r2018.contains("101\r\nEmbedded Object\r\n"), "R2018 keeps the embedded object");
    assert!(!r2018.contains(" 75\r\n"), "no legacy column groups in R2018");
    assert!(r2018.contains(" 72\r\n5\r\n"), "the flow direction (72) is kept");
    assert!(r2018.contains(" 44\r\n12.5\r\n 45\r\n2.5\r\n"), "column width and gutter");
    let own = r2018.split("Embedded Object").next().unwrap_or_default();
    assert!(own.contains(" 44\r\n1.0\r\n") && !own.contains(" 44\r\n12.5\r\n"), "the entity's line spacing is not the column width");
    let back = read_dxf(full.as_bytes()).unwrap();
    let Some(EntityKind::MText(b)) = back.model.iter().map(|e| e.kind.clone()).find(|k| matches!(k, EntityKind::MText(_))) else {
        panic!("no mtext")
    };
    assert_eq!(b.columns, t.columns);
    let (_, r2000) = dxf_as(&t, Version::R2000);
    assert!(!r2000.contains("Embedded Object"), "R2000 has no embedded object");
    assert!(r2000.contains(" 75\r\n1\r\n"), "R2000 keeps the legacy column groups");
}

#[test]
fn dynamic_auto_height_embedded_count_comes_from_the_total_width() {
    // A dynamic auto-height object stores no column count (72 = 0): the count is what 42 (total
    // width), 44 (column width) and 45 (gutter) hold, 3 * 50 + 2 * 12.5 = 175.
    let mut tags = embedded_tags(1, 50.0, 12.5, &[]);
    let at = tags.iter().position(|t| t.code == 101).unwrap();
    for t in &mut tags[at..] {
        *t = match t.code {
            42 => cadcraft_dxf::Tag::f(42, 175.0),
            71 => cadcraft_dxf::Tag::i(71, 2),
            72 => cadcraft_dxf::Tag::i(72, 0),
            73 => cadcraft_dxf::Tag::i(73, 1),
            _ => t.clone(),
        };
    }
    let (_, t) = mtext_from_groups(&tags);
    let c = t.columns.expect("columns");
    assert_eq!((c.kind, c.auto_height, c.count), (MTextColumnType::Dynamic, true, 3));
    assert_eq!(c.gutter, 12.5);
}

#[test]
fn dynamic_auto_height_count_comes_from_the_widths_when_72_is_zero() {
    // A dynamic auto-height object stores 0 in 72; the count is implied by the total width
    // (42 = n * 44 + (n - 1) * 45): (175 + 12.5) / (50 + 12.5) = 3.
    let mut tags = embedded_tags(0, 50.0, 12.5, &[]);
    let mut set = |code: i32, v: cadcraft_dxf::Tag| {
        let at = tags.iter().rposition(|t| t.code == code).unwrap();
        tags[at] = v;
    };
    set(71, cadcraft_dxf::Tag::i(71, 2));
    set(73, cadcraft_dxf::Tag::i(73, 1));
    set(42, cadcraft_dxf::Tag::f(42, 175.0));
    let (_, t) = mtext_from_groups(&tags);
    let c = t.columns.expect("columns");
    assert_eq!((c.kind, c.auto_height, c.count), (MTextColumnType::Dynamic, true, 3));
}

#[test]
fn embedded_column_width_wins_over_the_entitys_width() {
    // ZWCAD (and AutoCAD) write the total width of the columns in the entity's 41 and the column
    // width in the object's 44: an MTEXT's `width` is one column's, so 44 is what is read, and
    // what the R2018 object gets back (#2467 review).
    let mut tags = embedded_tags(3, 50.0, 12.5, &[20.0, 20.0, 20.0]);
    let own_41 = tags.iter().position(|t| t.code == 41).unwrap();
    tags[own_41] = cadcraft_dxf::Tag::f(41, 175.0);
    for code in [40, 42] {
        let at = tags.iter().rposition(|t| t.code == code).unwrap();
        tags[at] = cadcraft_dxf::Tag::f(code, 175.0);
    }
    let (_, t) = mtext_from_groups(&tags);
    assert_eq!(t.width, 50.0, "the column width, not the total");
    let (_, r2018) = dxf_as(&t, Version::R2018);
    assert!(r2018.contains(" 42\r\n175.0\r\n"), "total width kept:\n{r2018}");
    assert!(r2018.contains(" 44\r\n50.0\r\n 45\r\n12.5\r\n"), "column width and gutter kept:\n{r2018}");
}
