//! Records the DXF reader can't place are counted by the section they came from (#865), so a
//! damaged file no longer loses part of the drawing silently. End markers in the wrong place
//! (ENDSEC, ENDBLK, SEQEND) carry nothing and are not counted.

use std::collections::BTreeMap;

use cadcraft_doc::*;

/// ASCII DXF from one `code value` group per line.
fn dxf(groups: &str) -> Vec<u8> {
    let mut out = String::new();
    for g in groups.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = g.split_once(' ').unwrap_or((g, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out.into_bytes()
}

const DAMAGED: &str = "
    0 SECTION
    2 HEADER
    9 $ACADVER
    1 AC1015
    0 ENDSEC
    0 LINE
    8 0
    10 9
    20 9
    11 8
    21 8
    0 ENDSEC
    0 SECTION
    2 BLOCKS
    0 CIRCLE
    8 0
    10 0
    20 0
    40 1
    0 BLOCK
    8 0
    2 B
    70 0
    10 0
    20 0
    0 VERTEX
    8 0
    10 1
    20 1
    0 LINE
    8 0
    10 0
    20 0
    11 1
    21 1
    0 ENDBLK
    8 0
    0 ENDBLK
    0 ENDSEC
    0 SECTION
    2 ENTITIES
    0 LINE
    8 0
    10 0
    20 0
    11 5
    21 5
    0 VERTEX
    8 0
    10 1
    20 1
    0 ATTRIB
    8 0
    2 TAG
    1 V
    10 0
    20 0
    40 1
    0 SEQEND
    0 HATCH
    8 0
    2 SOLID
    70 1
    0 POLYLINE
    8 0
    66 1
    70 64
    0 SEQEND
    0 ENDSEC
    0 EOF
";

#[test]
fn records_the_reader_cannot_place_are_counted_by_section() {
    let d = cadcraft_io::read_dxf(&dxf(DAMAGED)).unwrap();
    // ENTITIES: a VERTEX and an ATTRIB with no POLYLINE/INSERT before them, a HATCH without
    // boundary data, a polyface mesh without faces. BLOCKS: a CIRCLE before any BLOCK, a VERTEX
    // inside one. Outside sections: the LINE between HEADER and BLOCKS.
    let expected = BTreeMap::from([("(no section)".to_string(), 1), ("BLOCKS".to_string(), 2), ("ENTITIES".to_string(), 4)]);
    assert_eq!(d.skipped_records, expected);
    // What could be placed still is.
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 1);
    assert_eq!(d.blocks.get("B").map(|b| b.entities.len()), Some(1));
}

#[test]
fn a_file_cadcraft_wrote_skips_nothing() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Default::default(), b: cadcraft_geom::Vec3::new(1.0, 1.0, 0.0) })).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert!(back.skipped_records.is_empty(), "{:?}", back.skipped_records);
}
