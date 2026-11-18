//! A damaged DXF can split its entities over several ENTITIES sections (with stray header
//! variables and ENDSECs between them, and no EOF). Every section is read, not just the last
//! one (#865).

use cadcraft_doc::EntityKind;

#[test]
fn entities_of_every_entities_section_are_read() {
    let text = "0\nSECTION\n2\nENTITIES\n\
                0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n1\n1\nA\n\
                0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n0\n\
                0\nENDSEC\n9\n$EXTMIN\n10\n0\n20\n0\n0\nENDSEC\n\
                0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n1\n11\n1\n21\n1\n0\nENDSEC\n\
                0\nSECTION\n2\nENTITIES\n0\nENDSEC\n";
    let d = cadcraft_io::read(text.as_bytes(), "t.dxf").unwrap();
    let lines = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    let texts = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Text(_))).count();
    assert_eq!((lines, texts, d.model.len()), (2, 1, 3));
}
