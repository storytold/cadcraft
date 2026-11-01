//! Saving a drawing with entities of types CADCraft doesn't model (read from another program's
//! file) as DWG says which ones the file doesn't keep, instead of dropping them silently. DXF
//! writes them back as read: nothing to report.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, SaveAs, set_io, set_write_as};
use cadcraft_engine::doc::{Block, Common, Drawing, EntityKind, RawTag, Space, Unknown};
use serde_json::json;

fn unknown(kind: &str) -> EntityKind {
    EntityKind::Unknown(Unknown { dxf_type: kind.into(), tags: vec![RawTag { code: 8, value: "0".into() }] })
}

#[test]
fn save_lists_the_entities_the_file_does_not_keep() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let mut d = Drawing::new_imperial();
    for k in ["MLINE", "3DSOLID", "MLINE"] {
        d.add(&Space::Model, Common::default(), unknown(k)).unwrap();
    }
    let mut b = Block::new("Part");
    let h = d.new_handle();
    b.entities.push(cadcraft_engine::doc::Entity { handle: h, common: Common::default(), kind: unknown("REGION") });
    d.blocks.insert("Part".into(), std::sync::Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "foreign.dxf", None);

    let path = std::env::temp_dir().join(format!("cadcraft-not-saved-{}.dwg", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["notSaved"], json!({ "3DSOLID": 1, "MLINE": 2, "REGION": 1 }));
    let msg = "Not saved in the DWG file (its writer can't write these object types yet; DXF keeps them, solids only when saved as 2010 or earlier): 1 3DSOLID, 2 MLINE, 1 REGION";
    assert!(s.log.iter().any(|l| l == msg), "{:?}", s.log);

    // DXF keeps them.
    let dxf = std::env::temp_dir().join(format!("cadcraft-not-saved-{}.dxf", std::process::id()));
    let n = s.log.len();
    let r = s.execute("saveas", &json!({ "path": dxf.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&dxf);
    assert!(r.get("notSaved").is_none() && s.log.len() == n);

    // Exporting an image isn't a save of the drawing: no report.
    let png = std::env::temp_dir().join(format!("cadcraft-not-saved-{}.png", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": png.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&png);
    assert!(r.get("notSaved").is_none());

    // A drawing the file keeps completely: nothing to report.
    s.open_drawing(Drawing::new_imperial(), "plain.dxf", None);
    let n = s.log.len();
    let r = s.execute("document.bytes", &json!({})).unwrap();
    assert!(r.get("notSaved").is_none() && s.log.len() == n);
}

#[test]
fn templates_report_what_dwg_does_not_keep() {
    use cadcraft_engine::cmd::file::drops_unknown;
    assert!(drops_unknown("a.dwg") && drops_unknown("B.DWT"));
    assert!(!drops_unknown("a.dxf"));
}

#[test]
fn solids_whose_geometry_the_data_store_keeps_are_not_reported_from_2013() {
    use cadcraft_engine::cmd::file::{Version, not_saved_as};
    use cadcraft_engine::doc::Handle;
    use cadcraft_engine::doc::foreign::Foreign;
    let mut d = Drawing::new_imperial();
    let kept = d.add(&Space::Model, Common::default(), unknown("3DSOLID")).unwrap();
    d.add(&Space::Model, Common::default(), unknown("3DSOLID")).unwrap();
    d.add(&Space::Model, Common::default(), unknown("REGION")).unwrap();
    let record = |h: Handle| {
        vec![
            RawTag { code: 0, value: "ACDSRECORD".into() },
            RawTag { code: 320, value: h.hex() },
            RawTag { code: 2, value: "ASM_Data".into() },
            RawTag { code: 310, value: "00".into() },
        ]
    };
    d.foreign = std::sync::Arc::new(Foreign { data_store: record(kept), ..Foreign::default() });
    // The solid with a data store record keeps its geometry from 2013 on; the others don't.
    assert_eq!(not_saved_as(&d, Version::R2018), [("3DSOLID".to_string(), 1), ("REGION".to_string(), 1)].into_iter().collect());
    // Before 2013 a file has no data store: every solid loses its geometry.
    assert_eq!(not_saved_as(&d, Version::R2010), [("3DSOLID".to_string(), 2), ("REGION".to_string(), 1)].into_iter().collect());
}

/// A DXF of 2013 or later can't hold a solid's SAT-text geometry (it lives in the data store
/// there), so the save leaves it out and says so; 2010 and earlier keep it (#2334).
#[test]
fn saving_sat_text_solids_as_dxf_2013_reports_them() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    set_write_as(|_, _, _: &SaveAs| Ok(b"x".to_vec()));
    let mut d = Drawing::new_imperial();
    let sat = EntityKind::Unknown(Unknown {
        dxf_type: "REGION".into(),
        tags: vec![
            RawTag { code: 8, value: "0".into() },
            RawTag { code: 1, value: "400 1 1 0".into() },
            RawTag { code: 3, value: "End-of-ACIS-data".into() },
        ],
    });
    d.add(&Space::Model, Common::default(), sat).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "region.dxf", None);

    let path = std::env::temp_dir().join(format!("cadcraft-sat-note-{}.dxf", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": path.to_string_lossy(), "version": 2018 })).unwrap();
    assert_eq!(r["notSaved"], json!({ "REGION": 1 }));
    assert!(s.log.iter().any(|l| l.contains("Not saved in the DXF file") && l.contains("1 REGION")), "{:?}", s.log);

    // QSAVE takes no version: a `version` passed to it doesn't change what it writes (the
    // drawing's own 2018), so the note still comes.
    let r = s.execute("qsave", &json!({ "path": path.to_string_lossy(), "version": 2010 })).unwrap();
    assert_eq!(r["notSaved"], json!({ "REGION": 1 }));

    let n = s.log.len();
    let r = s.execute("saveas", &json!({ "path": path.to_string_lossy(), "version": 2010 })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(r.get("notSaved").is_none() && s.log.len() == n);
}

/// In the Block Editor the file still holds the host drawing: its solids and version decide the
/// note, not the edited block's (#2334).
#[test]
fn the_note_counts_the_host_drawing_while_a_block_is_edited() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    set_write_as(|_, _, _: &SaveAs| Ok(b"x".to_vec()));
    let mut d = Drawing::new_imperial();
    let sat = EntityKind::Unknown(Unknown {
        dxf_type: "REGION".into(),
        tags: vec![
            RawTag { code: 8, value: "0".into() },
            RawTag { code: 1, value: "400 1 1 0".into() },
            RawTag { code: 3, value: "End-of-ACIS-data".into() },
        ],
    });
    d.add(&Space::Model, Common::default(), sat).unwrap();
    d.blocks.insert("Door".into(), std::sync::Arc::new(Block::new("Door")));
    let mut s = Session::empty();
    s.open_drawing(d, "region.dxf", None);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();

    let path = std::env::temp_dir().join(format!("cadcraft-sat-edit-{}.dxf", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": path.to_string_lossy(), "version": 2018 })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["notSaved"], json!({ "REGION": 1 }));
}

/// A DWG before 2013 writes a solid's SAT text as it is, so a save there doesn't list a solid that
/// holds it; from 2013 the geometry must be in the data store and SAT text alone is lost, so the
/// save lists it. A solid with neither is always listed (#2334).
#[test]
fn a_dwg_keeps_sat_text_solids_only_before_2013() {
    use cadcraft_engine::cmd::file::{Version, not_saved_as};
    let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
    let solid = |tags: Vec<RawTag>| EntityKind::Unknown(Unknown { dxf_type: "3DSOLID".into(), tags });
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), solid(vec![tag(100, "AcDbModelerGeometry"), tag(1, "400 1 1 0")])).unwrap();
    d.add(&Space::Model, Common::default(), solid(vec![tag(100, "AcDbModelerGeometry"), tag(70, "1")])).unwrap();
    assert_eq!(not_saved_as(&d, Version::R2010), [("3DSOLID".to_string(), 1)].into_iter().collect(), "2010 keeps the SAT-text solid");
    assert_eq!(not_saved_as(&d, Version::R2018), [("3DSOLID".to_string(), 2)].into_iter().collect(), "2018 loses it");
}
