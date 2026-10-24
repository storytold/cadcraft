//! An MTEXT whose extension dictionary has an `ACAD_MTEXT_2008_RT` XRECORD takes its contents from
//! there: AutoCAD keeps the real contents (`\\pxqc;` = centred) in the XRECORD and an older-reader
//! approximation (`\\pi4.86;`, an indent) in the entity.

use cadcraft_doc::EntityKind;

fn mtext(handle: &str, contents: &str, xdict: Option<&str>) -> String {
    let x = xdict.map(|d| format!("102\n{{ACAD_XDICTIONARY\n360\n{d}\n102\n}}\n")).unwrap_or_default();
    format!("  0\nMTEXT\n  5\n{handle}\n{x}100\nAcDbEntity\n  8\n0\n100\nAcDbMText\n 10\n0\n 20\n0\n 30\n0\n 40\n3\n 41\n50\n  1\n{contents}\n")
}

fn objects(owner: &str, dict: &str, xrecord: &str, text: &str) -> String {
    format!(
        "  0\nDICTIONARY\n  5\n{dict}\n330\n{owner}\n100\nAcDbDictionary\n  3\nACAD_MTEXT_2008_RT\n360\n{xrecord}\n  0\nXRECORD\n  5\n{xrecord}\n102\n{{ACAD_REACTORS\n330\n{dict}\n102\n}}\n330\n{dict}\n100\nAcDbXrecord\n280\n1\n  1\n{text}\n"
    )
}

#[test]
fn the_2008_rt_contents_replace_the_entity_contents() {
    let dxf = format!(
        "  0\nSECTION\n  2\nENTITIES\n{}{}  0\nENDSEC\n  0\nSECTION\n  2\nOBJECTS\n{}  0\nENDSEC\n  0\nEOF\n",
        mtext("A1", "\\pi4.86;ABC", Some("D1")),
        mtext("A2", "plain", None),
        objects("A1", "D1", "X1", "\\pxqc;ABC"),
    );
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let contents: Vec<String> =
        d.model.iter().filter_map(|e| if let EntityKind::MText(m) = &e.kind { Some(m.contents.clone()) } else { None }).collect();
    assert_eq!(contents, vec!["\\pxqc;ABC".to_string(), "plain".to_string()]);
}
