//! An OLE2FRAME is kept as an unknown entity; a DWG save carries it with its data and frame
//! position (#2422). A DWG's OLE2FRAME object has no corner fields: the position travels in the
//! start of the OLE data (a 2-byte marker, then the frame's four corners as three little-endian
//! doubles each: upper left, upper right, lower right, lower left), which is how the DXF's
//! groups 10 and 11 come back. That layout is read from the acadrust dependency's decoder.

use cadcraft_doc::EntityKind;
use cadcraft_io::{SaveOptions, Version, read, read_dxf, write_with};

const UL: (f64, f64) = (30.14, -18.99);
const LR: (f64, f64) = (35.27, -22.39);

/// The OLE data: the envelope with the frame's corners.
fn blob() -> Vec<u8> {
    let mut b = vec![1u8, 0];
    for (x, y) in [UL, (LR.0, UL.1), LR, (UL.0, LR.1)] {
        for v in [x, y, 0.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b
}

/// A 2000 DXF with one OLE2FRAME: corners, the binary data and the end marker (DXF Reference).
fn dxf() -> String {
    let data = blob();
    let hex: String = data.iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
         0\nOLE2FRAME\n5\n2B\n100\nAcDbEntity\n8\n0\n100\nAcDbOle2Frame\n70\n2\n3\nOLE Object\n\
         10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n71\n2\n72\n0\n90\n{}\n310\n{hex}\n1\nOLE\n\
         0\nENDSEC\n0\nEOF\n",
        UL.0,
        UL.1,
        LR.0,
        LR.1,
        data.len()
    )
}

fn ole(d: &cadcraft_doc::Drawing) -> Option<cadcraft_doc::Unknown> {
    d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Unknown(u) if u.dxf_type == "OLE2FRAME" => Some(u.clone()),
        _ => None,
    })
}

#[test]
fn a_dwg_keeps_an_ole_frame_with_its_data_and_position() {
    let d = read_dxf(dxf().as_bytes()).unwrap();
    assert!(ole(&d).is_some());
    let want: String = blob().iter().map(|b| format!("{b:02X}")).collect();
    for v in [Version::R2000, Version::R2010, Version::R2018] {
        let dwg = write_with(&d, "x.dwg", &SaveOptions { version: Some(v), binary: false }).unwrap();
        let back = read(&dwg, "x.dwg").unwrap();
        let u = ole(&back).unwrap_or_else(|| panic!("{v:?}: the OLE2FRAME is gone"));
        let data: String = u.tags.iter().filter(|t| t.code == 310).map(|t| t.value.to_ascii_uppercase()).collect();
        let corner = |code: i32| u.tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<f64>().ok());
        assert_eq!(data, want, "{v:?}: data");
        assert_eq!((corner(10), corner(20), corner(11), corner(21)), (Some(UL.0), Some(UL.1), Some(LR.0), Some(LR.1)), "{v:?}: frame corners");
    }
}

/// A frame moved through its corner tags (what MOVE does) keeps the new position in a DWG save
/// and read-back (review on #2587).
#[test]
fn a_moved_ole_frame_keeps_its_position_in_a_dwg() {
    let mut d = read_dxf(dxf().as_bytes()).unwrap();
    for h in d.model.handles() {
        d.model.modify(h, |e| e.kind.transform(&cadcraft_geom::Mat3::translate(cadcraft_geom::Vec2::new(100.0, -50.0))));
    }
    let dwg = write_with(&d, "x.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let u = ole(&read(&dwg, "x.dwg").unwrap()).unwrap();
    let c = |code: i32| u.tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<f64>().ok()).unwrap();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(c(10), UL.0 + 100.0) && near(c(20), UL.1 - 50.0) && near(c(11), LR.0 + 100.0) && near(c(21), LR.1 - 50.0), "{:?}", u.tags);
}

/// After a move, a DXF save and read-back carries the moved corners both in groups 10/11 and in
/// the OLE data header (review on #2587).
#[test]
fn a_moved_ole_frame_keeps_its_data_header_in_a_dxf() {
    let mut d = read_dxf(dxf().as_bytes()).unwrap();
    for h in d.model.handles() {
        d.model.modify(h, |e| e.kind.transform(&cadcraft_geom::Mat3::translate(cadcraft_geom::Vec2::new(100.0, -50.0))));
    }
    let bytes = write_with(&d, "x.dxf", &SaveOptions { version: None, binary: false }).unwrap();
    let u = ole(&read_dxf(&bytes).unwrap()).unwrap();
    let hex: String = u.tags.iter().filter(|t| t.code == 310).map(|t| t.value.clone()).collect();
    let b: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
    let f = |o: usize| f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let near = |a: f64, c: f64| (a - c).abs() < 1e-9;
    assert!(near(f(2), UL.0 + 100.0) && near(f(10), UL.1 - 50.0) && near(f(2 + 48), LR.0 + 100.0) && near(f(10 + 48), LR.1 - 50.0));
    let g = |code: i32| u.tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<f64>().ok()).unwrap();
    assert!(near(g(10), UL.0 + 100.0) && near(g(21), LR.1 - 50.0));
}
