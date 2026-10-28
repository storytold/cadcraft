//! Attribute group 73 is field length, whereas TEXT uses it for vertical alignment.
use cadcraft_doc::{EntityKind, HAlign, VAlign};
use cadcraft_io::{read_dxf, write_dxf};

#[test]
fn imported_attributes_use_group_74_and_keep_the_alignment_point() {
    for kind in ["ATTRIB", "ATTDEF", "TEXT"] {
        for (horizontal, expected_h) in [(0, HAlign::Left), (1, HAlign::Center), (2, HAlign::Right)] {
            let group_73: &[Option<i32>] =
                if kind == "TEXT" { &[None, Some(0), Some(1), Some(2), Some(3), Some(80)] } else { &[Some(0), Some(1), Some(3), Some(80)] };
            for &field_length in group_73 {
                for vertical in [None, Some(0), Some(1), Some(2), Some(3)] {
                    let field = field_length.map_or(String::new(), |v| format!("73\n{v}\n"));
                    let v = vertical.map_or(String::new(), |v| format!("74\n{v}\n"));
                    let prefix = if kind == "ATTRIB" { "0\nINSERT\n2\nTEST\n66\n1\n" } else { "" };
                    let suffix = if kind == "ATTRIB" { "0\nSEQEND\n" } else { "" };
                    let source = format!(
                        "0\nSECTION\n2\nENTITIES\n{prefix}0\n{kind}\n10\n1\n20\n2\n40\n1\n1\nValue\n2\nTAG\n72\n{horizontal}\n{field}{v}11\n7\n21\n9\n{suffix}0\nENDSEC\n0\nEOF\n"
                    );
                    let drawing = read_dxf(source.as_bytes()).expect("import synthetic DXF");
                    let expected = match if kind == "TEXT" { field_length.unwrap_or(0) } else { vertical.unwrap_or(0) } {
                        1 => VAlign::Bottom,
                        2 => VAlign::Middle,
                        3 => VAlign::Top,
                        _ => VAlign::Baseline,
                    };
                    for d in [&drawing, &read_dxf(write_dxf(&drawing).as_bytes()).expect("reopen")] {
                        let entity = d.model.iter().next().expect("entity");
                        let text = match &entity.kind {
                            EntityKind::Insert(i) => &i.attribs.first().expect("attribute").text,
                            EntityKind::AttDef(a) => &a.text,
                            EntityKind::Text(t) => t,
                            other => panic!("unexpected {other:?}"),
                        };
                        assert_eq!(text.valign, expected, "{kind} 73={field_length:?} 74={vertical:?}");
                        assert_eq!(text.halign, expected_h);
                        assert_eq!(
                            text.align_pt.map(|p| (p.x, p.y, p.z)),
                            (horizontal != 0 || expected != VAlign::Baseline).then_some((7.0, 9.0, 0.0))
                        );
                        assert_eq!(text.value, "Value");
                    }
                }
            }
        }
    }
}
