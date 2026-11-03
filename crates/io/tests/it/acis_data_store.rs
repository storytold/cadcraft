//! The ACIS geometry of 3D solids and regions survives a save. From AutoCAD 2013 on those entities
//! don't carry it themselves: it lives in the file's data store section (ACDSDATA), one record per
//! entity bound by a 320 handle. CADCraft doesn't model solids, so it keeps the section as read and
//! writes it back (and the DWG bridge reads it), without the records of deleted entities.

use cadcraft_doc::EntityKind;

/// Stand-in modeler data, carried as opaque bytes: the binary ACIS magic, some bytes, and the
/// end-of-data marker DWG readers delimit each body with.
const SAB_HEX: &str = "414349532042696E61727946696C65000102030405060708090A0B0C0D0E0F456E642D6F662D414349532D64617461";

/// A 2018 DXF with two 3D solids (2A, 2B) and their data store records.
fn dxf() -> String {
    let solid = |h: &str| {
        format!(
            "0\n3DSOLID\n5\n{h}\n330\n1F\n100\nAcDbEntity\n8\n0\n100\nAcDbModelerGeometry\n290\n0\n2\n{{00000000-0000-0000-0000-000000000000}}\n100\nAcDb3dSolid\n350\n0\n"
        )
    };
    let record = |h: &str| {
        format!("0\nACDSRECORD\n90\n1\n2\nAcDbDs::ID\n280\n10\n320\n{h}\n2\nASM_Data\n280\n15\n94\n{}\n310\n{SAB_HEX}\n", SAB_HEX.len() / 2)
    };
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{}{}0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n0\nENDSEC\n\
         0\nSECTION\n2\nACDSDATA\n70\n2\n71\n2\n\
         0\nACDSSCHEMA\n90\n1\n1\nAcDb3DSolid_ASM_Data\n2\nAcDbDs::ID\n280\n10\n91\n8\n2\nASM_Data\n280\n15\n91\n0\n\
         {}{}0\nENDSEC\n0\nEOF\n",
        solid("2A"),
        solid("2B"),
        record("2A"),
        record("2B")
    )
}

/// The handles the ACDSDATA section of DXF text binds ASM_Data to, with each one's hex data.
fn asm_data(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let Some(start) = lines.windows(2).position(|w| w == ["2", "ACDSDATA"]) else { return Vec::new() };
    let mut out: Vec<(String, String)> = Vec::new();
    let (mut handle, mut asm, mut hex) = (None::<String>, false, String::new());
    for pair in lines[start..].chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            if let (Some(h), true, false) = (handle.take(), asm, hex.is_empty()) {
                out.push((h.to_ascii_uppercase(), std::mem::take(&mut hex)));
            }
            asm = false;
            hex.clear();
            if *value == "ENDSEC" {
                break;
            }
        }
        match *code {
            "320" => handle = Some(value.to_string()),
            "2" => asm = *value == "ASM_Data",
            "310" if asm => hex.push_str(value),
            _ => {}
        }
    }
    out
}

#[test]
fn dxf_keeps_the_data_store_and_drops_records_of_deleted_entities() {
    let mut d = cadcraft_io::read_dxf(dxf().as_bytes()).unwrap();
    assert_eq!(d.foreign.data_store_entities().len(), 2);
    let out = String::from_utf8(cadcraft_io::write(&d, "x.dxf").unwrap()).unwrap();
    assert_eq!(asm_data(&out), vec![("2A".to_string(), SAB_HEX.to_string()), ("2B".to_string(), SAB_HEX.to_string())]);

    // Deleting a solid drops its record; the other stays.
    let h = d.model.iter().find(|e| e.handle.hex() == "2B").map(|e| e.handle).unwrap();
    d.remove_entity(h).unwrap();
    let out = String::from_utf8(cadcraft_io::write(&d, "x.dxf").unwrap()).unwrap();
    assert_eq!(asm_data(&out), vec![("2A".to_string(), SAB_HEX.to_string())]);

    // An R2000 file has no data store (its solids would carry SAT text instead).
    let r2000 = String::from_utf8(
        cadcraft_io::write_with(&d, "x.dxf", &cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2000), binary: false }).unwrap(),
    )
    .unwrap();
    assert!(!r2000.contains("ACDSDATA"));

    // No solid left: no section.
    let h = d.model.iter().find(|e| matches!(e.kind, EntityKind::Unknown(_))).map(|e| e.handle).unwrap();
    d.remove_entity(h).unwrap();
    let out = String::from_utf8(cadcraft_io::write(&d, "x.dxf").unwrap()).unwrap();
    assert!(!out.contains("ACDSDATA"));
}

#[test]
fn dwg_keeps_the_solids_geometry() {
    let d = cadcraft_io::read_dxf(dxf().as_bytes()).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    // The DWG read back (as the bridge's DXF) binds the same bytes to both solids.
    let back = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let mut got = asm_data(&back);
    got.sort();
    assert_eq!(got, vec![("2A".to_string(), SAB_HEX.to_string()), ("2B".to_string(), SAB_HEX.to_string())]);
    // And reopening it in CADCraft keeps them for the next save.
    let again = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(again.foreign.data_store_entities().len(), 2);
}

/// A solid with SAT-text ACIS data (an R2000-style file) can't be written to a DXF of 2013 or
/// later, whose files keep that data in the data store: it is left out there and counted, while
/// 2010 and earlier keep it, and solids with a data store record are kept everywhere (#2334).
#[test]
fn sat_text_solids_are_left_out_of_dxf_2013_and_later() {
    let sat = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
        0\nREGION\n5\n2B\n100\nAcDbEntity\n8\n0\n100\nAcDbModelerGeometry\n70\n1\n1\n400 1 1 0\n1\n16 ZWCAD\n3\nEnd-of-ACIS-data\n\
        0\n3DSOLID\n5\n2C\n100\nAcDbEntity\n8\n0\n100\nAcDbModelerGeometry\n70\n1\n1\n400 1 1 0\n3\nEnd-of-ACIS-data\n\
        0\nLINE\n5\n2D\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(sat.as_bytes()).unwrap();
    let count = |v: cadcraft_io::Version, ty: &str| {
        let out = cadcraft_io::write_with(&d, "x.dxf", &cadcraft_io::SaveOptions { version: Some(v), binary: false }).unwrap();
        let t = String::from_utf8(out).unwrap().replace("\r\n", "\n");
        t.matches(&format!("\n0\n{ty}\n")).count() + t.matches(&format!("\n  0\n{ty}\n")).count()
    };
    assert_eq!(cadcraft_doc::foreign::sat_text_dropped(&d), [("3DSOLID".to_string(), 1), ("REGION".to_string(), 1)].into());
    for v in [cadcraft_io::Version::R2000, cadcraft_io::Version::R2010] {
        assert_eq!((count(v, "REGION"), count(v, "3DSOLID")), (1, 1), "{v:?} keeps them");
    }
    for v in [cadcraft_io::Version::R2013, cadcraft_io::Version::R2018] {
        assert_eq!((count(v, "REGION"), count(v, "3DSOLID")), (0, 0), "{v:?} leaves them out");
        assert_eq!(count(v, "LINE"), 1, "{v:?} keeps the rest");
    }
    // Solids with a data store record keep their geometry in 2013 and later.
    let stored = cadcraft_io::read_dxf(dxf().as_bytes()).unwrap();
    assert!(cadcraft_doc::foreign::sat_text_dropped(&stored).is_empty());
}

/// A DWG keeps SAT-text solids at any version (its writer handles them), and a DXF's note leaves
/// out the contents of xref blocks, which the DXF doesn't hold anyway (#2334).
#[test]
fn sat_text_solids_stay_in_dwg_2018_and_xref_contents_are_not_counted() {
    let sat = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
        0\nREGION\n5\n2B\n100\nAcDbEntity\n8\n0\n100\nAcDbModelerGeometry\n70\n1\n1\n400 1 1 0\n3\nEnd-of-ACIS-data\n\
        0\nLINE\n5\n2D\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(sat.as_bytes()).unwrap();
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2018), binary: false };
    let dwg = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let unknown = |d: &cadcraft_doc::Drawing| d.model.iter().filter(|e| matches!(&e.kind, EntityKind::Unknown(u) if u.dxf_type == "REGION")).count();
    assert_eq!(unknown(&back), 1, "the DWG 2018 keeps the region");

    // Xref blocks (and `xref|` ones) hold no entities in the file: not reported as left out.
    let mut x = d.clone();
    let region = d.model.iter().find(|e| matches!(&e.kind, EntityKind::Unknown(_))).cloned().unwrap();
    let mut host = cadcraft_doc::Block::new("REF");
    host.xref_path = Some("other.dwg".into());
    host.entities.push((*region).clone());
    x.blocks.insert("REF".into(), std::sync::Arc::new(host));
    let mut dep = cadcraft_doc::Block::new("REF|PART");
    dep.entities.push((*region).clone());
    x.blocks.insert("REF|PART".into(), std::sync::Arc::new(dep));
    assert_eq!(cadcraft_doc::foreign::sat_text_dropped(&x), [("REGION".to_string(), 1)].into(), "only the model-space region");
}

/// Only groups 1 and 3 of the modeler geometry subclass are SAT text, and only a record holding
/// the entity's ASM_Data keeps its geometry in the data store (#2334).
#[test]
fn sat_text_is_told_by_its_subclass_and_a_geometry_record() {
    use cadcraft_doc::foreign::{Foreign, sat_text_acis};
    use cadcraft_doc::{Handle, RawTag, Unknown};
    let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
    let solid = |tags: Vec<RawTag>| Unknown { dxf_type: "3DSOLID".into(), tags };
    let none = std::collections::BTreeSet::new();
    let h = Handle(0x2B);
    // A stray 1 or 3 outside AcDbModelerGeometry is not SAT text.
    let stray =
        solid(vec![tag(100, "AcDbEntity"), tag(1, "name"), tag(100, "AcDbModelerGeometry"), tag(70, "1"), tag(100, "AcDb3dSolid"), tag(3, "x")]);
    assert!(!sat_text_acis(&stray, h, &none));
    let real = solid(vec![tag(100, "AcDbEntity"), tag(100, "AcDbModelerGeometry"), tag(70, "1"), tag(1, "400 1 1 0"), tag(100, "AcDb3dSolid")]);
    assert!(sat_text_acis(&real, h, &none));
    // Without subclass markers (an old file) the groups are the entity's own.
    assert!(sat_text_acis(&solid(vec![tag(8, "0"), tag(1, "400 1 1 0")]), h, &none));

    // A 320 in the data store without an ASM_Data record doesn't keep the geometry.
    let store = |asm: bool| {
        let mut t = vec![tag(0, "ACDSRECORD"), tag(90, "1"), tag(320, "2B")];
        t.push(tag(2, if asm { "ASM_Data" } else { "Other" }));
        Foreign { data_store: t, ..Foreign::default() }
    };
    assert!(store(true).data_store_entities().contains(&h));
    assert!(store(false).data_store_entities().is_empty());
}

/// The writer keeps a data store record by its first 320 only, so only that entity counts as
/// stored (#2334).
#[test]
fn a_data_store_record_stores_only_its_first_entity() {
    use cadcraft_doc::foreign::Foreign;
    use cadcraft_doc::{Handle, RawTag};
    let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
    let store = Foreign { data_store: vec![tag(0, "ACDSRECORD"), tag(320, "2B"), tag(320, "2C"), tag(2, "ASM_Data")], ..Foreign::default() };
    assert_eq!(store.data_store_entities(), [Handle(0x2B)].into());
}

/// A real DWG round trip of a SAT-text REGION: the DWG writer keeps the SAT groups up to 2010, and
/// from 2013 they come back empty (the solid's geometry is not in the file), which the DWG save
/// note has to say (#2334).
#[test]
fn a_dwg_keeps_a_sat_text_region_only_before_2013() {
    let sat = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
        0\nREGION\n5\n2B\n100\nAcDbEntity\n8\n0\n100\nAcDbModelerGeometry\n70\n1\n1\n400 1 1 0\n3\nEnd-of-ACIS-data\n0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(sat.as_bytes()).unwrap();
    let holds_sat = |v: cadcraft_io::Version| {
        let dwg = cadcraft_io::write_with(&d, "x.dwg", &cadcraft_io::SaveOptions { version: Some(v), binary: false }).unwrap();
        let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
        let region = back.model.iter().find_map(|e| match &e.kind {
            EntityKind::Unknown(u) if u.dxf_type == "REGION" => Some(u.clone()),
            _ => None,
        });
        region.is_some_and(|u| cadcraft_doc::foreign::has_sat_text(&u.tags))
    };
    assert!(holds_sat(cadcraft_io::Version::R2010), "2010 keeps the SAT text");
    assert!(!holds_sat(cadcraft_io::Version::R2018), "2018 comes back without it");
}
