//! ATTRIB and ATTDEF keep their vertical justification in group 74 (group 73 is the field length).

use cadcraft_doc::{EntityKind, VAlign};

fn attrib(kind: &str) -> String {
    format!("  0\n{kind}\n  8\n0\n 10\n1\n 20\n1\n 30\n0\n 40\n1.4\n  1\n3\n  2\nVAL\n 70\n0\n 72\n1\n 11\n1\n 21\n1\n 31\n0\n 73\n0\n 74\n2\n")
}

#[test]
fn attribute_vertical_justification_is_group_74() {
    let dxf = format!(
        "  0\nSECTION\n  2\nENTITIES\n{}  0\nINSERT\n  8\n0\n  2\nB\n 66\n1\n 10\n0\n 20\n0\n 30\n0\n{}  0\nSEQEND\n  0\nENDSEC\n  0\nEOF\n",
        attrib("ATTDEF"),
        attrib("ATTRIB")
    );
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let mut seen = 0;
    for e in d.model.iter() {
        match &e.kind {
            EntityKind::AttDef(a) => {
                assert_eq!(a.text.valign, VAlign::Middle);
                seen += 1;
            }
            EntityKind::Insert(i) => {
                assert_eq!(i.attribs.first().map(|a| a.text.valign), Some(VAlign::Middle));
                seen += 1;
            }
            _ => {}
        }
    }
    assert_eq!(seen, 2);
}
