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
fn export_and_wblock_say_what_a_dwg_leaves_out() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let mut d = Drawing::new_imperial();
    let h = d.add(&Space::Model, Common::default(), unknown("ACME_WIDGET")).unwrap().hex();
    let mut s = Session::empty();
    s.open_drawing(d, "foreign.dxf", None);
    let file = |ext: &str| std::env::temp_dir().join(format!("cadcraft-leaves-out-{}.{ext}", std::process::id())).to_string_lossy().into_owned();
    let said = |l: &String| l.starts_with("Not saved in the DWG file") && l.ends_with(": 1 ACME_WIDGET");

    let (dwg, dxf) = (file("dwg"), file("dxf"));
    for (cmd, p) in [("export", json!({ "path": dwg })), ("wblock", json!({ "path": dwg, "handles": [h] }))] {
        s.log.clear();
        let r = s.execute(cmd, &p).unwrap();
        assert_eq!(r["notSaved"], json!({ "ACME_WIDGET": 1 }), "{cmd}");
        assert!(s.log.iter().any(said), "{cmd}: {:?}", s.log);
    }
    // -WBLOCK takes the objects out of the drawing, so last.
    let r = s.execute("-wblock", &json!({ "path": dwg, "handles": [h] })).unwrap();
    assert_eq!(r["notSaved"], json!({ "ACME_WIDGET": 1 }));
    let _ = std::fs::remove_file(&dwg);

    // DXF keeps them.
    s.execute("undo", &json!({})).unwrap();
    for (cmd, p) in [("export", json!({ "path": dxf })), ("wblock", json!({ "path": dxf, "handles": [h] }))] {
        s.log.clear();
        assert!(s.execute(cmd, &p).unwrap().get("notSaved").is_none(), "{cmd}");
        assert!(!s.log.iter().any(said), "{cmd}: {:?}", s.log);
    }
    let _ = std::fs::remove_file(&dxf);
}

#[test]
fn templates_report_what_dwg_does_not_keep() {
    use cadcraft_engine::cmd::file::drops_unknown;
    assert!(drops_unknown("a.dwg") && drops_unknown("B.DWT"));
    assert!(!drops_unknown("a.dxf"));
}

/// Saving a drawing whose raw dimension class names a `*D` block nothing modelled claims writes
/// a file that holds the record but not the block: SAVEAS says which references dangle instead
/// of passing the broken file silently. The save still happens; the warning is the pipeline's
/// to act on (the drawing keeps the block).
#[test]
fn saveas_reports_dangling_block_references() {
    use cadcraft_engine::cmd::file::set_dangling_blocks;

    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    // The real writer's report (io is a lower layer; the app installs it with the writer).
    set_dangling_blocks(|d| {
        serde_json::json!(
            cadcraft_io::dangling_block_refs(d)
                .iter()
                .map(|x| serde_json::json!({ "code": "DANGLING_BLOCK_REF", "handle": x.handle, "entity": x.entity, "block": x.block }))
                .collect::<Vec<_>>()
        )
    });
    let mut d = Drawing::new_imperial();
    let mut b = Block::new("*D500");
    let h = d.new_handle();
    b.entities.push(cadcraft_engine::doc::Entity {
        handle: h,
        common: Common::default(),
        kind: EntityKind::Line(cadcraft_engine::doc::Line {
            a: cadcraft_engine::geom::Vec3::ZERO,
            b: cadcraft_engine::geom::Vec3::new(1.0, 0.0, 0.0),
        }),
    });
    d.blocks.insert("*D500".into(), std::sync::Arc::new(b));
    let raw = d
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::Unknown(Unknown {
                dxf_type: "LARGE_RADIAL_DIMENSION".into(),
                tags: vec![RawTag { code: 2, value: "*D500".into() }, RawTag { code: 8, value: "0".into() }],
            }),
        )
        .unwrap()
        .hex();
    let mut s = Session::empty();
    s.open_drawing(d, "dangling.dxf", None);

    let dxf = std::env::temp_dir().join(format!("cadcraft-dangling-{}.dxf", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": dxf.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&dxf);
    assert_eq!(
        r["danglingBlockRefs"],
        json!([{ "code": "DANGLING_BLOCK_REF", "handle": raw, "entity": "LARGE_RADIAL_DIMENSION", "block": "*D500" }])
    );
    assert!(s.log.iter().any(|l| l.starts_with("The saved DXF holds 1 dangling block reference(s)")), "{:?}", s.log);

    // A drawing nothing dangles in: no field, nothing said.
    s.open_drawing(Drawing::new_imperial(), "plain.dxf", None);
    let n = s.log.len();
    let plain = std::env::temp_dir().join(format!("cadcraft-dangling-plain-{}.dxf", std::process::id()));
    let r = s.execute("saveas", &json!({ "path": plain.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&plain);
    assert!(r.get("danglingBlockRefs").is_none() && s.log.len() == n);
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

/// Another program's proxy entity reaches a 2013 or later DWG with the class its group 91 names;
/// before 2013, or when the file has no such class, the DWG leaves it out and says so.
#[test]
fn proxy_entities_are_reported_only_before_2013() {
    use cadcraft_engine::cmd::file::{Version, not_saved_as};
    use cadcraft_engine::doc::foreign::Foreign;
    let proxy = |class: i64| {
        EntityKind::Unknown(Unknown {
            dxf_type: "ACAD_PROXY_ENTITY".into(),
            tags: vec![
                RawTag { code: 8, value: "0".into() },
                RawTag { code: 90, value: "498".into() },
                RawTag { code: 91, value: class.to_string() },
            ],
        })
    };
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), proxy(500)).unwrap();
    d.add(&Space::Model, Common::default(), unknown("MLINE")).unwrap();
    d.foreign = std::sync::Arc::new(Foreign { class_order: vec!["MYAPP_THING".into()], ..Foreign::default() });
    for v in [Version::R2013, Version::R2018] {
        assert_eq!(not_saved_as(&d, v), [("MLINE".to_string(), 1)].into_iter().collect(), "{v:?}");
    }
    for v in [Version::R2004, Version::R2010] {
        assert_eq!(not_saved_as(&d, v), [("ACAD_PROXY_ENTITY".to_string(), 1), ("MLINE".to_string(), 1)].into_iter().collect(), "{v:?}");
    }
    // A class number the file's CLASSES don't have, or none at all: the DWG can't name the class,
    // leaves the proxy out, and says so from 2013 on too.
    d.add(&Space::Model, Common::default(), proxy(501)).unwrap();
    d.add(&Space::Model, Common::default(), unknown("ACAD_PROXY_ENTITY")).unwrap();
    assert_eq!(not_saved_as(&d, Version::R2018), [("ACAD_PROXY_ENTITY".to_string(), 2), ("MLINE".to_string(), 1)].into_iter().collect());
}

/// Another program's proxy objects are left out of a DWG the same way as its proxy entities, and
/// said so: before 2013 always, from 2013 when the file names no class for them (#2471 review).
#[test]
fn proxy_objects_are_reported_like_proxy_entities() {
    use cadcraft_engine::cmd::file::{Version, not_saved_as};
    use cadcraft_engine::doc::Handle;
    use cadcraft_engine::doc::foreign::{Foreign, ForeignObject};
    let object = |h: u64, class: i64| ForeignObject {
        handle: Handle(h),
        kind: "ACAD_PROXY_OBJECT".into(),
        tags: vec![
            RawTag { code: 5, value: format!("{h:X}") },
            RawTag { code: 90, value: "499".into() },
            RawTag { code: 91, value: class.to_string() },
        ],
    };
    let mut d = Drawing::new_imperial();
    d.foreign = std::sync::Arc::new(Foreign {
        class_order: vec!["MYAPP_DATA".into()],
        objects: vec![object(0x500, 500), object(0x501, 501)],
        ..Foreign::default()
    });
    let lost = |v| not_saved_as(&d, v).get("ACAD_PROXY_OBJECT").copied().unwrap_or(0);
    assert_eq!(lost(Version::R2004), 2, "before 2013 every proxy object is left out");
    assert_eq!(lost(Version::R2010), 2);
    assert_eq!(lost(Version::R2018), 1, "from 2013 only the one whose class the file doesn't name");
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

/// The interactive save asks with the same list, and EXPORT reports it, so overwriting a 2013+
/// DXF never drops solids on a command-line note alone (#2334).
#[test]
fn a_dxf_2013_loss_is_known_before_saving() {
    use cadcraft_engine::cmd::file::loss_on_save;
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let sat = |d: &mut Drawing| {
        let tags = vec![RawTag { code: 8, value: "0".into() }, RawTag { code: 1, value: "400 1 1 0".into() }];
        d.add(&Space::Model, Common::default(), EntityKind::Unknown(Unknown { dxf_type: "REGION".into(), tags })).unwrap();
    };
    let mut modern = Drawing::new_imperial();
    modern.header.set_str("ACADVER", "AC1032");
    sat(&mut modern);
    let mut s = Session::empty();
    s.open_drawing(modern, "region.dxf", None);
    assert_eq!(loss_on_save(&s, "region.dxf"), [("REGION".to_string(), 1)].into_iter().collect());

    let path = std::env::temp_dir().join(format!("cadcraft-sat-export-{}.dxf", std::process::id()));
    let r = s.execute("export", &json!({ "path": path.to_string_lossy() })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["notSaved"], json!({ "REGION": 1 }));
    assert!(s.log.iter().any(|l| l.contains("Not saved in the DXF file")), "{:?}", s.log);

    let mut old = Drawing::new_imperial();
    old.header.set_str("ACADVER", "AC1024");
    sat(&mut old);
    s.open_drawing(old, "region.dxf", None);
    assert!(loss_on_save(&s, "region.dxf").is_empty(), "2010 keeps SAT text");
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

/// The DWG bridge writes OLE objects (`ole2frame_roundtrip` in the io tests), so saving one as
/// DWG reports nothing for them, at any version, while other unmodelled types are still listed.
/// The type is matched as stored: the bridge gets it as stored too, and acadrust reads only the
/// upper-case spelling into its OLE entity, so any other spelling is still reported.
#[test]
fn ole_objects_are_not_reported() {
    use cadcraft_engine::cmd::file::{Version, not_saved_as};
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), unknown("OLE2FRAME")).unwrap();
    d.add(&Space::Model, Common::default(), unknown("MLINE")).unwrap();
    d.add(&Space::Model, Common::default(), unknown("ole2frame")).unwrap();
    let mut b = Block::new("Logo");
    let h = d.new_handle();
    b.entities.push(cadcraft_engine::doc::Entity { handle: h, common: Common::default(), kind: unknown("OLE2FRAME") });
    d.blocks.insert("Logo".into(), std::sync::Arc::new(b));
    for v in [Version::R2000, Version::R2010, Version::R2018] {
        assert_eq!(not_saved_as(&d, v), [("MLINE".to_string(), 1), ("ole2frame".to_string(), 1)].into_iter().collect(), "{v:?}");
    }
}

#[test]
fn the_dangling_report_counts_the_host_drawing_while_a_block_is_edited() {
    use cadcraft_engine::cmd::file::set_dangling_blocks;

    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    set_write_as(|_, _, _: &SaveAs| Ok(b"0\nEOF\n".to_vec()));
    set_dangling_blocks(|d| {
        serde_json::json!(
            cadcraft_io::dangling_block_refs(d)
                .iter()
                .map(|x| serde_json::json!({ "code": "DANGLING_BLOCK_REF", "handle": x.handle, "entity": x.entity, "block": x.block }))
                .collect::<Vec<_>>()
        )
    });
    let mut d = Drawing::new_imperial();
    let mut b = Block::new("*D500");
    let h = d.new_handle();
    b.entities.push(cadcraft_engine::doc::Entity {
        handle: h,
        common: Common::default(),
        kind: EntityKind::Line(cadcraft_engine::doc::Line {
            a: cadcraft_engine::geom::Vec3::ZERO,
            b: cadcraft_engine::geom::Vec3::new(1.0, 0.0, 0.0),
        }),
    });
    d.blocks.insert("*D500".into(), std::sync::Arc::new(b));
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Unknown(Unknown {
            dxf_type: "LARGE_RADIAL_DIMENSION".into(),
            tags: vec![RawTag { code: 2, value: "*D500".into() }, RawTag { code: 8, value: "0".into() }],
        }),
    )
    .unwrap();
    d.blocks.insert("Door".into(), std::sync::Arc::new(Block::new("Door")));
    let mut s = Session::empty();
    s.open_drawing(d, "dangling-edit.dxf", None);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();

    // document.bytes writes the drawing itself, which waits in the editor while `doc` is the
    // editor's copy: the report reads the drawing the file holds, editor or not.
    let r = s.execute("document.bytes", &json!({ "format": "dxf" })).unwrap();
    assert_eq!(r["danglingBlockRefs"][0]["block"], json!("*D500"), "{r}");
}
