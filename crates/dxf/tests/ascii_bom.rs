//! A UTF-8 byte order mark at the start of an ASCII DXF is skipped.

const BODY: &str = "  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";

#[test]
fn utf8_bom_bytes_are_skipped() {
    let plain = cadcraft_dxf::parse(BODY.as_bytes()).expect("plain parses");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(BODY.as_bytes());
    let with_bom = cadcraft_dxf::parse(&bytes);
    assert!(with_bom.is_ok(), "{with_bom:?}");
    assert_eq!(with_bom.map(|t| t.len()).unwrap_or(0), plain.len());
}

#[test]
fn bom_text_is_skipped() {
    let text = format!("\u{feff}{BODY}");
    assert!(cadcraft_dxf::parse_ascii(&text).is_ok());
}
