//! A TEXT on a tilted plane keeps its extrusion (210) in the first `AcDbText` subclass, as the DXF
//! Reference orders it: after group 11 and before the second `100 AcDbText` marker and group 73.

use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, Space, Text, VAlign};
use cadcraft_geom::Vec3;
use cadcraft_io::{read, write_dxf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn text(halign: HAlign, valign: VAlign, insert: Vec3, align_pt: Option<Vec3>) -> Text {
    Text {
        insert,
        align_pt,
        height: 2.5,
        value: "Hi".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign,
        valign,
    }
}

fn drawing_with(extrusion: Vec3, t: Text) -> Result<Drawing, Box<dyn std::error::Error>> {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common { extrusion, ..Common::default() }, EntityKind::Text(t))?;
    Ok(d)
}

/// The group codes of the TEXT record, in order.
fn text_codes(dxf: &str) -> Vec<(i64, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let mut pairs = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if let Ok(code) = lines[i].parse::<i64>() {
            pairs.push((code, lines[i + 1].to_string()));
        }
        i += 2;
    }
    let start = pairs.iter().position(|p| p.0 == 0 && p.1 == "TEXT").unwrap_or(0);
    let end = pairs.iter().skip(start + 1).position(|p| p.0 == 0).map_or(pairs.len(), |n| start + 1 + n);
    pairs[start..end].to_vec()
}

#[test]
fn the_extrusion_is_written_before_the_second_text_subclass_marker() -> TestResult {
    // Middle-vertical text writes group 73, the group the old order put the 210 after.
    for z in [1.0, -1.0] {
        let d = drawing_with(Vec3::new(0.0, 1.0, z), text(HAlign::Left, VAlign::Middle, Vec3::new(1.0, 2.0, 0.0), Some(Vec3::new(1.0, 2.0, 0.0))))?;
        let codes = text_codes(&write_dxf(&d));
        let at = |code: i64| codes.iter().position(|p| p.0 == code);
        let (Some(extrusion), Some(group73)) = (at(210), at(73)) else { return Err(format!("no 210 or 73 in {codes:?}").into()) };
        let markers: Vec<usize> = codes.iter().enumerate().filter(|(_, p)| p.0 == 100 && p.1 == "AcDbText").map(|(i, _)| i).collect();
        assert_eq!(markers.len(), 2, "{codes:?}");
        assert!(markers[0] < extrusion && extrusion < markers[1] && markers[1] < group73, "210 sits in the first subclass: {codes:?}");
    }
    Ok(())
}

#[test]
fn a_text_with_an_alignment_point_apart_from_its_insert_keeps_it_through_a_save() -> TestResult {
    // Made through JSON rather than read from a file: the two points differ.
    let t = text(HAlign::Left, VAlign::Top, Vec3::new(5.0, 5.0, 0.0), Some(Vec3::new(9.0, 3.0, 0.0)));
    let d = drawing_with(Vec3::new(0.0, 0.0, -1.0), t)?;
    let back = read(write_dxf(&d).as_bytes(), "a.dxf")?;
    let Some(EntityKind::Text(r)) = back.model.iter().map(|e| &e.kind).find(|k| matches!(k, EntityKind::Text(_))).cloned() else {
        return Err("the TEXT did not come back".into());
    };
    assert_eq!(r.halign, HAlign::Left);
    // Placed by its alignment point, not by the insert it never used.
    assert!((r.insert.x - 9.0).abs() < 1e-9 && (r.insert.y - 3.0).abs() < 1e-9, "{r:?}");
    Ok(())
}
