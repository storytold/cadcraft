//! OLE objects (OLE2FRAME: a picture, a spreadsheet or a document embedded in the drawing)
//! survive a DWG save. CADCraft doesn't model them and keeps them as read; the DWG bridge reads
//! them from our DXF into acadrust's own entity and writes them to DWG with their frame and the
//! embedded object's bytes, so a drawing saved as DWG and reopened still has them.

use cadcraft_doc::EntityKind;

/// Stand-in OLE data as a DWG carries it: a two-byte marker, the frame's four corners (upper
/// left, upper right, lower right, lower left; x, y, z doubles), then the object's bytes, here
/// opaque text long enough to take several 310 groups.
fn payload() -> Vec<u8> {
    let mut data = vec![0x80, 0x00];
    for [x, y] in [[10.0f64, 40.0], [70.0, 40.0], [70.0, 5.0], [10.0, 5.0]] {
        for v in [x, y, 0.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    for i in 0..12u8 {
        data.extend_from_slice(b"synthetic embedded object ");
        data.push(i);
    }
    data
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

/// A 2018 DXF with one embedded OLE object in model space on layer `Pictures`.
fn dxf() -> String {
    let data = payload();
    let chunks: String = data.chunks(127).map(|c| format!("310\n{}\n", hex(c))).collect();
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n\
         0\nOLE2FRAME\n5\n2A\n330\n1F\n100\nAcDbEntity\n8\nPictures\n100\nAcDbOle2Frame\n70\n2\n\
         10\n10\n20\n40\n30\n0\n11\n70\n21\n5\n31\n0\n71\n2\n72\n0\n73\n2\n90\n{}\n{chunks}1\nOLE\n\
         0\nENDSEC\n0\nEOF\n",
        data.len()
    )
}

/// Each OLE2FRAME of DXF text as its (code, value) pairs.
fn frames(text: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            inside = *value == "OLE2FRAME";
            if inside {
                out.push(Vec::new());
            }
        } else if inside && let Some(rec) = out.last_mut() {
            rec.push((code.to_string(), value.to_string()));
        }
    }
    out
}

fn group<'a>(rec: &'a [(String, String)], code: &str) -> Option<&'a str> {
    rec.iter().find(|(c, _)| c == code).map(|(_, v)| v.as_str())
}

fn number(rec: &[(String, String)], code: &str) -> Option<f64> {
    group(rec, code).and_then(|v| v.parse().ok())
}

/// The embedded object's bytes: the 310 groups joined.
fn data(rec: &[(String, String)]) -> String {
    rec.iter().filter(|(c, _)| c == "310").map(|(_, v)| v.as_str()).collect()
}

/// The frame, its object's bytes, type and layer as the DWG keeps them.
fn assert_kept(rec: &[(String, String)]) {
    assert_eq!(data(rec), hex(&payload()), "the embedded object's bytes changed");
    assert_eq!(number(rec, "90"), Some(payload().len() as f64));
    let corners: Vec<Option<f64>> = ["10", "20", "11", "21"].iter().map(|c| number(rec, c)).collect();
    assert_eq!(corners, [Some(10.0), Some(40.0), Some(70.0), Some(5.0)]);
    assert_eq!(group(rec, "71"), Some("2"), "an embedded object");
    assert_eq!(group(rec, "73"), Some("2"));
    assert_eq!(group(rec, "8"), Some("Pictures"));
}

#[test]
fn dwg_keeps_ole_objects() {
    let d = cadcraft_io::read_dxf(dxf().as_bytes()).unwrap();
    let ole = |d: &cadcraft_doc::Drawing| d.model.iter().filter(|e| matches!(&e.kind, EntityKind::Unknown(u) if u.dxf_type == "OLE2FRAME")).count();
    assert_eq!(ole(&d), 1);

    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let got = frames(&back);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_kept(&got[0]);

    // Reopened in CADCraft it is in model space again, and a second DWG save keeps it too.
    let again = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(ole(&again), 1);
    let dwg = cadcraft_io::write(&again, "x.dwg").unwrap();
    let got = frames(&String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap());
    assert_eq!(got.len(), 1, "{got:?}");
    assert_kept(&got[0]);

    // Older DWG versions keep it the same way.
    for version in [cadcraft_io::Version::R2000, cadcraft_io::Version::R2010] {
        let dwg = cadcraft_io::write_with(&d, "x.dwg", &cadcraft_io::SaveOptions { version: Some(version), binary: false }).unwrap();
        let got = frames(&String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap());
        assert_eq!(got.len(), 1, "{version:?}: {got:?}");
        assert_kept(&got[0]);
    }
}
