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
