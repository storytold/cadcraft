//! A damaged DXF can split its entities over several ENTITIES sections (with stray header
//! variables and ENDSECs between them, and no EOF). Every section is read, not just the last
//! one (#865).

use cadcraft_doc::{EntityKind, Handle};

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

#[test]
fn repeated_handles_across_entities_sections_keep_both_entities() {
    // A damaged file can repeat a handle across its sections (within one section duplicates are
    // already renumbered). The store replaces an entity with the same handle, so the earlier
    // section's line was silently lost — and a save kept only the later one (#939).
    let text = "0\nSECTION\n2\nENTITIES\n\
                0\nLINE\n5\n300\n8\n0\n10\n0\n20\n0\n11\n1\n21\n0\n\
                0\nENDSEC\n\
                0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n300\n8\n0\n10\n0\n20\n1\n11\n1\n21\n1\n0\nENDSEC\n\
                0\nEOF\n";
    let d = cadcraft_io::read(text.as_bytes(), "t.dxf").unwrap();
    let lines = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!(lines, 2, "the second section's line replaced the first");
    let handles: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    let unique: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "duplicate handles: {handles:?}");
    assert!(d.entity(Handle::parse_hex("300").unwrap()).is_some(), "the file's own handle stays with one of them");
}

#[test]
fn duplicate_mleader_handles_keep_each_deferred_style_fixup() {
    let mleader = |style: &str| format!("0\nMULTILEADER\n5\n300\n100\nAcDbEntity\n8\n0\n100\nAcDbMLeader\n340\n{style}\n");
    let text = format!(
        "0\nSECTION\n2\nOBJECTS\n\
         0\nDICTIONARY\n5\nD\n3\nACAD_MLEADERSTYLE\n350\nE\n\
         0\nDICTIONARY\n5\nE\n3\nFirst\n350\nA\n3\nSecond\n350\nB\n3\nThird\n350\nC\n\
         0\nMLEADERSTYLE\n5\nA\n0\nMLEADERSTYLE\n5\nB\n0\nMLEADERSTYLE\n5\nC\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{}{}0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n",
        mleader("A"),
        mleader("B"),
        mleader("C"),
    );
    let d = cadcraft_io::read(text.as_bytes(), "t.dxf").unwrap();
    let mut handles_by_style = std::collections::HashMap::new();
    for e in d.model.iter() {
        if let EntityKind::MLeader(m) = &e.kind {
            assert!(handles_by_style.insert(m.style.as_str(), e.handle).is_none(), "duplicate style {}", m.style);
        }
    }
    assert_eq!(handles_by_style.len(), 3, "styles were assigned to the wrong records: {handles_by_style:?}");
    assert_eq!(handles_by_style.get("First"), Some(&Handle::parse_hex("300").unwrap()));
    assert_ne!(handles_by_style.get("First"), handles_by_style.get("Second"));
    assert_ne!(handles_by_style.get("Second"), handles_by_style.get("Third"));
}

#[test]
fn duplicate_table_handles_keep_each_deferred_style_fixup() {
    let table = |style: &str| format!("0\nACAD_TABLE\n5\n400\n100\nAcDbEntity\n8\n0\n100\nAcDbTable\n91\n1\n92\n1\n141\n1\n142\n1\n342\n{style}\n");
    let text = format!(
        "0\nSECTION\n2\nOBJECTS\n\
         0\nDICTIONARY\n5\nD\n3\nACAD_TABLESTYLE\n350\nE\n\
         0\nDICTIONARY\n5\nE\n3\nFirst\n350\nA\n3\nSecond\n350\nB\n\
         0\nTABLESTYLE\n5\nA\n100\nAcDbTableStyle\n70\n0\n280\n0\n281\n0\n7\nStandard\n140\n0.45\n\
         0\nTABLESTYLE\n5\nB\n100\nAcDbTableStyle\n70\n0\n280\n0\n281\n0\n7\nStandard\n140\n0.85\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{}{}0\nENDSEC\n0\nEOF\n",
        table("A"),
        table("B"),
    );
    let d = cadcraft_io::read(text.as_bytes(), "t.dxf").unwrap();
    let tables: Vec<_> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Table(t) => Some((e.handle, t)),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 2);
    assert_eq!((tables[0].1.style.as_str(), tables[0].1.text_height), ("First", 0.45));
    assert_eq!((tables[1].1.style.as_str(), tables[1].1.text_height), ("Second", 0.85));
    assert_ne!(tables[0].0, tables[1].0);
}
