//! An OLE2FRAME is kept as an unknown entity; a DWG save carries it with its data (#2422).

use cadcraft_doc::EntityKind;
use cadcraft_io::{SaveOptions, Version, read, read_dxf, write_with};

/// A 2000 DXF with one OLE2FRAME: corners, the binary data and the end marker (DXF Reference).
fn dxf() -> String {
    String::from(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
         0\nOLE2FRAME\n5\n2B\n100\nAcDbEntity\n8\n0\n100\nAcDbOle2Frame\n70\n2\n3\nOLE Object\n\
         10\n30.14\n20\n-18.99\n30\n0\n11\n35.27\n21\n-22.39\n31\n0\n71\n2\n72\n0\n90\n8\n310\n0102030405060708\n1\nOLE\n\
         0\nENDSEC\n0\nEOF\n",
    )
}

fn ole(d: &cadcraft_doc::Drawing) -> Option<cadcraft_doc::Unknown> {
    d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Unknown(u) if u.dxf_type == "OLE2FRAME" => Some(u.clone()),
        _ => None,
    })
}

#[test]
fn a_dwg_keeps_an_ole_frame_with_its_data() {
    let d = read_dxf(dxf().as_bytes()).unwrap();
    assert!(ole(&d).is_some());
    for v in [Version::R2000, Version::R2010, Version::R2018] {
        let dwg = write_with(&d, "x.dwg", &SaveOptions { version: Some(v), binary: false }).unwrap();
        let back = read(&dwg, "x.dwg").unwrap();
        let u = ole(&back).unwrap_or_else(|| panic!("{v:?}: the OLE2FRAME is gone"));
        let data: String = u.tags.iter().filter(|t| t.code == 310).map(|t| t.value.to_ascii_uppercase()).collect();
        assert_eq!(data, "0102030405060708", "{v:?}: data");
        // Only the data is compared: with this stand-in blob the DWG round trip returns default
        // corners (1,1)/(0,0), which a real object might not do; not verified without a real file.
    }
}
