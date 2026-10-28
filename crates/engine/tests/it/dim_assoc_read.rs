//! A DIMASSOC point reference read from a file (snap type, subentity, GS marker, parameter) is
//! kept only while the object it describes is unchanged: editing the object drops it, so the
//! writer recomputes the reference from CADCraft's own link.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{AsRead, EntityKind, Handle, OsnapRead};
use serde_json::json;

fn dim(s: &Session, h: Handle) -> cadcraft_engine::doc::Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(dm) => dm.clone(),
        k => panic!("not a dimension: {k:?}"),
    }
}

/// A line with a linear dimension attached to both ends, each attachment carrying a point
/// reference as if the dimension had been read from a file.
fn attached() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    let line = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let line = Handle::parse_hex(line["handles"][0].as_str().unwrap()).unwrap();
    let r = s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 3]})).unwrap();
    let dh = Handle::parse_hex(r["handle"].as_str().or(r["handles"][0].as_str()).unwrap()).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(dh, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                for a in &mut dm.assoc {
                    a.read = AsRead::new(OsnapRead { handle: a.handle, snap: Some(a.snap.clone()), osnap: 1, gs_marker: 7, ..OsnapRead::default() });
                }
            }
        })
        .unwrap();
    assert_eq!(dim(&s, dh).assoc.len(), 2, "the dimension follows both ends of the line");
    (s, line, dh)
}

#[test]
fn editing_the_object_drops_the_file_point_reference() {
    let (mut s, line, dh) = attached();
    s.execute("move", &json!({"handles": [line.hex()], "delta": [0, 5]})).unwrap();
    let dm = dim(&s, dh);
    assert_eq!(dm.assoc.len(), 2);
    assert!(dm.assoc.iter().all(|a| a.read.is_none()), "{:?}", dm.assoc);
}

#[test]
fn an_unrelated_edit_keeps_it() {
    let (mut s, _, dh) = attached();
    let c = s.execute("circle", &json!({"center": [50, 50], "radius": 1})).unwrap();
    let c = c["handle"].as_str().or(c["handles"][0].as_str()).unwrap().to_string();
    s.execute("move", &json!({"handles": [c], "delta": [1, 0]})).unwrap();
    assert!(dim(&s, dh).assoc.iter().all(|a| a.read.get().is_some_and(|r| r.gs_marker == 7)));
}
