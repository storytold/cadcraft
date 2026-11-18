//! MTEXT line spacing style (group 73: 1 at least, 2 exactly) round-trips through DXF (#408).

use cadcraft_doc::{Common, Drawing, EntityKind, MText, Space};
use cadcraft_geom::Vec3;

#[test]
fn line_spacing_style_roundtrips() {
    for exact in [false, true] {
        let mut d = Drawing::new_metric();
        let t = MText {
            insert: Vec3::new(1.0, 2.0, 0.0),
            height: 2.5,
            width: 40.0,
            attach: 5,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "a\\Pb".into(),
            line_spacing: 1.5,
            line_spacing_exact: exact,
            columns: None,
            background: None,
        };
        d.add(&Space::Model, Common::default(), EntityKind::MText(t.clone())).unwrap();
        let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
        let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
        let Some(EntityKind::MText(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no mtext") };
        assert_eq!(r.line_spacing_exact, exact);
        assert_eq!(r.line_spacing, 1.5);
    }
}

#[test]
fn background_mask_roundtrips() {
    use cadcraft_color::{Color, Rgb};
    use cadcraft_doc::MTextBackground;
    for bg in [
        MTextBackground { mask: 2, color: None, scale: 1.5 },
        MTextBackground { mask: 1, color: Some(Color::Index(3)), scale: 2.0 },
        MTextBackground { mask: 1, color: Some(Color::True(Rgb(12, 34, 56))), scale: 1.25 },
    ] {
        let mut d = Drawing::new_metric();
        let t = MText {
            insert: Vec3::new(1.0, 2.0, 0.0),
            height: 2.5,
            width: 40.0,
            attach: 5,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "MASKED".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
            background: Some(bg.clone()),
        };
        d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
        let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
        let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
        let Some(EntityKind::MText(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no mtext") };
        assert_eq!(r.background, Some(bg));
    }
}

#[test]
fn background_mask_roundtrips_with_legacy_and_embedded_columns() {
    use cadcraft_color::Color;
    use cadcraft_doc::{MTextBackground, MTextColumnType, MTextColumns};
    use cadcraft_io::{SaveOptions, Version, write_with};
    let bg = MTextBackground { mask: 1, color: Some(Color::Index(3)), scale: 1.5 };
    let cols = MTextColumns {
        kind: MTextColumnType::Static,
        count: 2,
        gutter: 2.0,
        height: 20.0,
        auto_height: false,
        heights: Vec::new(),
        flow_reversed: false,
    };
    for version in [Version::R2013, Version::R2018] {
        let mut d = Drawing::new_metric();
        let t = MText {
            insert: Vec3::new(1.0, 2.0, 0.0),
            height: 2.5,
            width: 22.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "COL1\\NCOL2".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: Some(cols.clone()),
            background: Some(bg.clone()),
        };
        d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
        let bytes = write_with(&d, "a.dxf", &SaveOptions { version: Some(version), ..Default::default() }).unwrap();
        let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
        let Some(EntityKind::MText(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no mtext") };
        assert_eq!(r.background, Some(bg.clone()), "version={version:?}");
        assert!(r.columns.is_some(), "columns lost for version={version:?}");
    }
}
