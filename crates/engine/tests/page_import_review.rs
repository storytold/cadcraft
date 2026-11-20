use cadcraft_doc::{
    Block, Common, Drawing, EntityKind, Handle, Insert, Line, RawTag, Solid, Space, Unknown,
    color::{Color, Rgb},
};
use cadcraft_engine::{
    Session,
    cmd::file::{IoHooks, set_io},
    geom::{Bounds2, Vec2, Vec3},
};
use serde_json::json;
use std::sync::Arc;

fn page() -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(
        &Space::Model,
        Common { color: Color::True(Rgb(255, 255, 255)), ..Default::default() },
        EntityKind::Solid(Solid { corners: [Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(20.0, 10.0, 0.0)] }),
    )
    .unwrap();
    d
}
fn session() -> Session {
    set_io(IoHooks {
        read: |bytes, _| if bytes == [255] { Err("conversion failed".into()) } else { Ok(page()) },
        write: |_, _| Err("unused".into()),
        plot: None,
    });
    Session::new()
}
fn insert(s: &mut Session, marked: bool, rotation: f64) -> Handle {
    let mut block = Block::new("PDF_ANTET_1");
    if marked {
        block.description = "Converted embedded document".into();
    }
    for source in page().model.iter() {
        let mut entity = (**source).clone();
        entity.handle = s.doc_mut().unwrap().new_handle();
        block.entities.push(entity);
    }
    let mut line = cadcraft_doc::Entity::new(
        s.doc_mut().unwrap().new_handle(),
        EntityKind::Line(Line { a: Vec3::new(100.0, 100.0, 0.0), b: Vec3::new(110.0, 100.0, 0.0) }),
    );
    line.common.color = Color::True(Rgb(255, 0, 0));
    block.entities.push(line);
    s.doc_mut().unwrap().blocks.insert(block.name.clone(), Arc::new(block));
    s.doc_mut()
        .unwrap()
        .add(&Space::Model, Common::default(), EntityKind::Insert(Insert { block: "PDF_ANTET_1".into(), rotation, ..Default::default() }))
        .unwrap()
}
#[test]
fn page_import_without_document_and_response_contract() {
    for name in ["page.PDF", "page.docx"] {
        let mut s = session();
        s.docs.clear();
        let r = s.execute("import", &json!({"name":name,"data":"AA=="})).unwrap();
        assert_eq!(s.docs.len(), 1);
        assert_eq!(r["handles"], json!([r["handle"]]));
        assert_eq!((r["layers"].as_u64(), r["styles"].as_u64(), r["blocks"].as_u64()), (Some(0), Some(0), Some(1)));
        assert!(s.doc().unwrap().block(r["block"].as_str().unwrap()).is_some());
        assert!(s.undo().unwrap().is_some());
        assert!(s.doc().unwrap().model.is_empty());
        assert!(s.doc().unwrap().blocks.is_empty());
        assert!(s.redo().unwrap().is_some());
        let uid = s.state().unwrap().uid;
        s.execute("import", &json!({"name":name,"data":"AA=="})).unwrap();
        assert_eq!(s.state().unwrap().uid, uid, "an existing drawing is reused");
        assert_eq!(s.doc().unwrap().model.len(), 2);
    }
    for name in ["page.dwg", "page.dxf", "bad.pdf"] {
        let mut s = session();
        s.docs.clear();
        assert!(s.execute("import", &json!({"name":name,"data":"/w=="})).is_err());
        assert!(s.docs.is_empty(), "no destination on failed conversion or a drawing-only import");
    }
}
#[test]
fn drawing_with_a_page_extension_imports_as_a_drawing() {
    for data in ["QUMxMDMyAA==", "OTk5ClBsYW4KMApTRUNUSU9OCg=="] {
        let mut s = session();
        let params = json!({"name":"plan.pdf", "data":data});
        let result = s.execute("import", &params).unwrap();
        assert_eq!(result["blocks"], 0);
        assert!(matches!(s.doc().unwrap().model.iter().next().unwrap().kind, EntityKind::Solid(_)));
        s.docs.clear();
        assert!(s.execute("import", &params).is_err());
        assert!(s.docs.is_empty());
    }
}

#[test]
fn page_import_uses_the_destination_space_units() {
    let mut s = session();
    s.doc_mut().unwrap().header.set_i64("INSUNITS", 6);
    for (tab, paper_mm, unit) in [("Model", true, 1000.0), ("Layout1", true, 1.0), ("Layout1", false, 25.4)] {
        s.execute("layout.set", &json!({"name":tab})).unwrap();
        if tab != "Model" {
            s.doc_mut().unwrap().layouts.iter_mut().find(|l| l.name == tab).unwrap().page.paper_mm = Some(paper_mm);
        }
        let r = s.execute("import", &json!({"name":"page.pdf","data":"AA==","scale":2.0})).unwrap();
        let handle = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        let EntityKind::Insert(insert) = &s.doc().unwrap().entity(handle).unwrap().kind else { panic!("expected page block") };
        assert!((insert.scale.x * unit - 2.0).abs() < 1e-9);
    }
}

#[test]
fn dxf_bytes_take_precedence_over_page_file_extensions() {
    for binary in [false, true] {
        let bytes = cadcraft_io::write_with(&page(), "source.dxf", &cadcraft_io::SaveOptions { version: None, binary }).unwrap();
        for name in ["source.pdf", "source.docx", "source.olepreview"] {
            let drawing = cadcraft_io::read(&bytes, name).unwrap();
            assert!(matches!(drawing.model.iter().next().unwrap().kind, EntityKind::Solid(_)));
        }
    }
}

fn ole(s: &mut Session, width: f64, height: f64) -> Handle {
    let tags = [(10, 0.0), (20, 0.0), (11, width), (21, -height)].into_iter().map(|(code, v)| RawTag { code, value: v.to_string() }).collect();
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Unknown(Unknown { dxf_type: "OLE2FRAME".into(), tags })).unwrap()
}
#[test]
fn degenerate_ole_is_preserved_and_conversion_reports_collision_name() {
    for (width, height) in [(0.0, 10.0), (10.0, 0.0)] {
        let mut s = session();
        let h = ole(&mut s, width, height);
        assert!(s.execute("oleconvert", &json!({"handle":h.hex()})).is_err());
        assert!(matches!(s.doc().unwrap().entity(h).unwrap().kind, EntityKind::Unknown(_)));
        assert!(s.doc().unwrap().blocks.is_empty());
    }
    let mut s = session();
    let h = ole(&mut s, 20.0, 10.0);
    let overlay = ole(&mut s, 2.0, 1.0);
    let collision = format!("OLE_{}", h.hex());
    s.doc_mut().unwrap().blocks.insert(collision.clone(), Arc::new(Block::new(&collision)));
    let r = s.execute("oleconvert", &json!({"handle":h.hex()})).unwrap();
    assert_eq!(s.doc().unwrap().model.handles().last(), Some(&overlay));
    assert_eq!(r["block"], format!("{collision}_1"));
}
#[test]
fn block_name_is_not_a_page_marker() {
    let mut s = session();
    let h = insert(&mut s, false, 0.0);
    let d = s.doc().unwrap();
    assert!(d.imported_page_frame(d.entity(h).unwrap()).is_none());
    assert_eq!(cadcraft_engine::select::pick(d, &Space::Model, Vec2::new(105.0, 100.0), 0.1), Some(h));
}
#[test]
fn page_selection_is_consistent_above_spatial_index_threshold() {
    let mut s = session();
    let h = insert(&mut s, true, 0.0);
    let bounds = Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(21.0, 11.0));
    for count in [0, 300] {
        for i in 0..count {
            let x = 1000.0 + f64::from(i);
            s.doc_mut()
                .unwrap()
                .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 1.0, 0.0) }))
                .unwrap();
        }
        assert!(cadcraft_engine::select::select_window(s.doc().unwrap(), &Space::Model, bounds, false).contains(&h));
    }
}
#[test]
fn rotated_page_crossing_ignores_empty_bounding_box_corners() {
    let mut s = session();
    let h = insert(&mut s, true, std::f64::consts::FRAC_PI_4);
    let d = s.doc().unwrap();
    let empty = Bounds2::new(Vec2::new(-6.5, 19.0), Vec2::new(-6.0, 19.5));
    assert!(!cadcraft_engine::select::select_window(d, &Space::Model, empty, true).contains(&h));
    let inside = Bounds2::new(Vec2::new(2.0, 9.0), Vec2::new(2.5, 9.5));
    assert!(cadcraft_engine::select::select_window(d, &Space::Model, inside, true).contains(&h));
    let edge = Bounds2::new(Vec2::new(-0.1, -0.1), Vec2::new(0.1, 0.1));
    assert!(cadcraft_engine::select::select_window(d, &Space::Model, edge, true).contains(&h));
}
#[test]
fn successful_shrink_never_disables_subsequent_enlargement() {
    let frame = [Vec2::ZERO, Vec2::new(400.0, 0.0), Vec2::new(400.0, 300.0), Vec2::new(0.0, 300.0), Vec2::ZERO];
    let anchor = frame[0];
    let small = cadcraft_engine::grips::page_resize_transform(&frame, 2, anchor + (frame[2] - anchor) * 1e-6).unwrap();
    let frame = frame.map(|p| small.apply(p));
    assert!(cadcraft_engine::grips::page_resize_transform(&frame, 2, anchor + (frame[2] - anchor) * 1e6).is_some());
    let second = cadcraft_engine::grips::page_resize_transform(&frame, 2, anchor + (frame[2] - anchor) * 1e-6);
    if let Some(transform) = second {
        let next = frame.map(|p| transform.apply(p));
        assert!(cadcraft_engine::grips::page_resize_transform(&next, 2, anchor + (next[2] - anchor) * 1e6).is_some());
    }
}
#[test]
fn page_marker_survives_drawing_roundtrip() {
    let mut s = session();
    insert(&mut s, true, 0.0);
    for extension in ["dxf", "dwg"] {
        let file = format!("page.{extension}");
        let bytes = cadcraft_io::write(s.doc().unwrap(), &file).unwrap();
        let d = cadcraft_io::read(&bytes, &file).unwrap();
        let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap();
        assert!(d.imported_page_frame(e).is_some(), "{extension}: persisted marker");
    }
}
