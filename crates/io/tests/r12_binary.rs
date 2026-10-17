//! Binary DXF from R12 and earlier (1-byte group codes) opens like an R13+ binary DXF: an
//! old-style POLYLINE with its VERTEX records, a LINE, and header text in the code page.

use cadcraft_doc::EntityKind;

/// An R12 binary DXF declaring `ANSI_1251`, with a LINE on a Cyrillic layer and a closed
/// three-vertex POLYLINE.
fn r12_binary_dxf() -> Vec<u8> {
    let mut b = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    fn s(b: &mut Vec<u8>, code: u8, v: &[u8]) {
        b.push(code);
        b.extend_from_slice(v);
        b.push(0);
    }
    fn f(b: &mut Vec<u8>, code: u8, v: f64) {
        b.push(code);
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn i(b: &mut Vec<u8>, code: u8, v: i16) {
        b.push(code);
        b.extend_from_slice(&v.to_le_bytes());
    }
    for (c, v) in [(0, &b"SECTION"[..]), (2, b"HEADER"), (9, b"$ACADVER"), (1, b"AC1009"), (9, b"$DWGCODEPAGE"), (3, b"ANSI_1251")] {
        s(&mut b, c, v);
    }
    s(&mut b, 0, b"ENDSEC");
    s(&mut b, 0, b"SECTION");
    s(&mut b, 2, b"ENTITIES");
    s(&mut b, 0, b"LINE");
    s(&mut b, 8, b"\xC4\xEE\xF0\xEE\xE3\xE0"); // "Дорога" in ANSI_1251
    for (c, v) in [(10, 0.0), (20, 0.0), (30, 0.0), (11, 100.0), (21, 50.0), (31, 0.0)] {
        f(&mut b, c, v);
    }
    s(&mut b, 0, b"POLYLINE");
    s(&mut b, 8, b"RAILROAD");
    i(&mut b, 66, 1);
    i(&mut b, 70, 1);
    for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)] {
        s(&mut b, 0, b"VERTEX");
        s(&mut b, 8, b"RAILROAD");
        f(&mut b, 10, x);
        f(&mut b, 20, y);
        f(&mut b, 30, 0.0);
    }
    s(&mut b, 0, b"SEQEND");
    s(&mut b, 0, b"ENDSEC");
    s(&mut b, 0, b"EOF");
    b
}

#[test]
fn r12_binary_dxf_opens() {
    let d = cadcraft_io::read_dxf(&r12_binary_dxf()).unwrap();
    let ents: Vec<_> = d.model.iter().collect();
    assert_eq!(ents.len(), 2);
    let line = ents.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap();
    assert_eq!(line.common.layer, "Дорога");
    let EntityKind::LwPolyline(p) = &ents.iter().find(|e| !matches!(e.kind, EntityKind::Line(_))).unwrap().kind else {
        panic!("expected the POLYLINE as a 2D polyline");
    };
    assert_eq!(p.vertices.len(), 3);
    assert!(p.closed);
}

#[test]
fn truncated_r12_binary_dxf_never_panics() {
    let b = r12_binary_dxf();
    for n in 22..b.len() {
        let _ = cadcraft_io::read_dxf(&b[..n]);
    }
}
