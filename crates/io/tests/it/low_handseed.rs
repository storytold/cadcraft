//! Another program's file can name a $HANDSEED below handles it uses. Entities CADCraft makes
//! while reading (the faces a polyface mesh becomes) must not take handles that records further
//! on still have: a DWG keeps one object per handle, so the duplicates were lost on save.

use std::collections::HashSet;

use cadcraft_doc::{Drawing, EntityKind, Handle};

/// A block holding a polyface mesh of three triangles (handle 2000) and, in model space after it,
/// lines with handles 2001–2003 (the next ones after the mesh's), under a $HANDSEED of 10. The
/// collision is across separately read lists (a block and model space), as in the files it came
/// from: within one list duplicates were already renumbered.
fn dxf() -> String {
    let vertex = |h: &str, x: f64, y: f64| format!("0\nVERTEX\n5\n{h}\n8\n0\n10\n{x}\n20\n{y}\n30\n0.0\n70\n192\n");
    let face = |h: &str, a: i32, b: i32, c: i32| format!("0\nVERTEX\n5\n{h}\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n70\n128\n71\n{a}\n72\n{b}\n73\n{c}\n");
    let line = |h: &str| format!("0\nLINE\n5\n{h}\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n1.0\n21\n1.0\n31\n0.0\n");
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n10\n0\nENDSEC\n\
         0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n30\n8\n0\n2\nMESH\n70\n0\n10\n0.0\n20\n0.0\n30\n0.0\n\
         0\nPOLYLINE\n5\n2000\n8\n0\n66\n1\n10\n0.0\n20\n0.0\n30\n0.0\n70\n64\n71\n4\n72\n3\n\
         {}{}{}{}{}{}{}0\nSEQEND\n5\n2010\n8\n0\n0\nENDBLK\n5\n31\n8\n0\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{}{}{}0\nINSERT\n5\n60\n8\n0\n2\nMESH\n10\n0.0\n20\n0.0\n30\n0.0\n0\nENDSEC\n0\nEOF\n",
        vertex("2004", 0.0, 0.0),
        vertex("2005", 1.0, 0.0),
        vertex("2006", 1.0, 1.0),
        vertex("2007", 0.0, 1.0),
        face("2008", 1, 2, 3),
        face("2009", 1, 3, 4),
        face("200A", 2, 3, 4),
        line("2001"),
        line("2002"),
        line("2003"),
    )
}

fn counts(d: &Drawing) -> (usize, usize) {
    let faces = d.block("MESH").map_or(0, |b| b.entities.iter().filter(|e| matches!(e.kind, EntityKind::Face3d(_))).count());
    let lines = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    (faces, lines)
}

#[test]
fn faces_made_while_reading_do_not_take_the_files_handles() {
    let d = cadcraft_io::read_dxf(dxf().as_bytes()).unwrap();
    assert_eq!(counts(&d), (3, 3));
    let handles: Vec<Handle> = d.model.iter().chain(d.block("MESH").unwrap().entities.iter()).map(|e| e.handle).collect();
    let unique: HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "duplicate handles: {handles:?}");
    // The file's own handles stay with their entities.
    assert!(d.block("MESH").unwrap().entities.iter().any(|e| e.handle == Handle::parse_hex("2000").unwrap()));
    for h in ["2001", "2002", "2003"] {
        assert!(d.entity(Handle::parse_hex(h).unwrap()).is_some(), "{h}");
    }
    // Nothing is lost through a DWG.
    let back = cadcraft_io::read(&cadcraft_io::write(&d, "a.dwg").unwrap(), "a.dwg").unwrap();
    assert_eq!(counts(&back), (3, 3));
}
