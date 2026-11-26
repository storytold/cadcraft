//! MTEXT background mask (DXF groups 90, 63/421, 45) conditional emission and round-trip.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, MText, MTextBackground, MTextColumnType, MTextColumns, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn make_mtext(background: Option<MTextBackground>, columns: Option<MTextColumns>) -> MText {
    MText {
        insert: Vec3::new(10.0, 20.0, 0.0),
        height: 2.5,
        width: 50.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "Hello MTEXT".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns,
        background,
    }
}

fn round_trip(t: MText) -> (MText, String) {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    let dxf = write_dxf(&d);
    let back = read_dxf(dxf.as_bytes()).unwrap();
    let entity = back
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::MText(m) => Some(m.clone()),
            _ => None,
        })
        .expect("MText after round trip");
    (entity, dxf)
}

/// Extract `(group_code, value)` pairs of the first `MTEXT` entity in `ENTITIES`.
fn mtext_entity_tags(dxf: &str) -> Vec<(i32, String)> {
    let mut lines = dxf.lines();
    let mut pairs = Vec::new();
    let mut in_entities = false;
    let mut in_mtext = false;
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        let code: i32 = c.trim().parse().expect("group code");
        let val = v.trim().to_string();
        if code == 0 && val == "SECTION" {
            if let (Some(_), Some(sec)) = (lines.next(), lines.next()) {
                in_entities = sec.trim() == "ENTITIES";
            }
            continue;
        }
        if !in_entities {
            continue;
        }
        if code == 0 {
            if in_mtext {
                break;
            }
            if val == "MTEXT" {
                in_mtext = true;
            }
            continue;
        }
        if in_mtext {
            pairs.push((code, val));
        }
    }
    pairs
}

#[test]
fn mtext_background_mask_zero_omits_conditional_tags() {
    let bg = MTextBackground { mask: 0, color: Some(Color::True(Rgb(10, 20, 30))), scale: 2.0 };
    let (back, dxf) = round_trip(make_mtext(Some(bg), None));
    assert_eq!(back.background, None);

    let tags = mtext_entity_tags(&dxf);
    for forbidden in [90, 63, 421, 45, 441] {
        assert!(!tags.iter().any(|(c, _)| *c == forbidden), "mask 0 must not emit group {forbidden}: {tags:?}");
    }
}

#[test]
fn mtext_background_window_fill_omits_color_tags_and_roundtrips() {
    for mask in [2u8, 3u8] {
        let bg = MTextBackground { mask, color: Some(Color::Index(1)), scale: 2.25 };
        let (back, dxf) = round_trip(make_mtext(Some(bg), None));
        assert_eq!(back.background, Some(MTextBackground { mask, color: None, scale: 2.25 }));

        let tags = mtext_entity_tags(&dxf);
        assert!(tags.iter().any(|(c, v)| *c == 90 && v == &mask.to_string()), "expected group 90={mask}: {tags:?}");
        assert!(tags.iter().any(|(c, v)| *c == 45 && v.parse::<f64>().ok() == Some(2.25)), "expected group 45=2.25: {tags:?}");
        for color_code in [63, 421, 441] {
            assert!(!tags.iter().any(|(c, _)| *c == color_code), "window background (mask={mask}) must not emit group {color_code}: {tags:?}");
        }
    }
}

#[test]
fn mtext_background_custom_aci_and_truecolor_before_columns_roundtrips() {
    let cols = MTextColumns { kind: MTextColumnType::Static, count: 2, gutter: 5.0, height: 20.0, ..Default::default() };

    let bg_aci = MTextBackground { mask: 1, color: Some(Color::Index(3)), scale: 1.75 };
    let (back_aci, dxf_aci) = round_trip(make_mtext(Some(bg_aci.clone()), Some(cols.clone())));
    assert_eq!(back_aci.background, Some(bg_aci));
    let tags_aci = mtext_entity_tags(&dxf_aci);
    let col_pos_aci = tags_aci.iter().position(|(c, _)| *c == 75 || *c == 101).expect("column tags");
    let p90 = tags_aci.iter().position(|(c, v)| *c == 90 && v == "1").expect("group 90=1");
    let p63 = tags_aci.iter().position(|(c, v)| *c == 63 && v == "3").expect("group 63=3");
    let p45 = tags_aci.iter().position(|(c, v)| *c == 45 && v.parse::<f64>().ok() == Some(1.75)).expect("group 45=1.75");
    assert!(!tags_aci.iter().any(|(c, _)| *c == 421), "ACI color must not emit 421: {tags_aci:?}");
    assert!(p90 < col_pos_aci && p63 < col_pos_aci && p45 < col_pos_aci, "background tags must precede columns");

    let rgb = Rgb(0x12, 0x34, 0x56);
    let bg_rgb = MTextBackground { mask: 1, color: Some(Color::True(rgb)), scale: 2.0 };
    let (back_rgb, dxf_rgb) = round_trip(make_mtext(Some(bg_rgb.clone()), Some(cols)));
    assert_eq!(back_rgb.background, Some(bg_rgb));
    let tags_rgb = mtext_entity_tags(&dxf_rgb);
    let col_pos_rgb = tags_rgb.iter().position(|(c, _)| *c == 75 || *c == 101).expect("column tags");
    let r90 = tags_rgb.iter().position(|(c, v)| *c == 90 && v == "1").expect("group 90=1");
    let r63 = tags_rgb.iter().position(|(c, _)| *c == 63).expect("group 63");
    let r421 = tags_rgb.iter().position(|(c, v)| *c == 421 && v == &rgb.to_u32().to_string()).expect("group 421");
    let r45 = tags_rgb.iter().position(|(c, v)| *c == 45 && v.parse::<f64>().ok() == Some(2.0)).expect("group 45=2.0");
    assert!(r90 < col_pos_rgb && r63 < col_pos_rgb && r421 < col_pos_rgb && r45 < col_pos_rgb, "truecolor background tags must precede columns");
}
