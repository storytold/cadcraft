//! MTEXT columns and paragraph codes survive a DXF round trip (#532): the column groups
//! 75/76/78/79/48/49/50, tabs as `^I`, carets as `^ `.

use cadcraft_doc::{Common, Drawing, EntityKind, MText, MTextColumnType, MTextColumns, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

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
        background: None,
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
