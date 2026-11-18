use cadcraft_doc::{
    Block, Common, Entity, EntityKind, Handle, Insert, Solid, Space,
    color::{Color, Rgb},
};
use cadcraft_engine::{Session, geom::Vec3};
use serde_json::json;
use std::sync::Arc;

#[test]
fn corner_resize_keeps_opposite_corner_and_undo_restores_page() {
    let mut session = Session::new();
    let mut block = Block::new("PDF_ANTET");
    let mut background = Entity::new(
        Handle(2),
        EntityKind::Solid(Solid { corners: [Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(20.0, 10.0, 0.0)] }),
    );
    background.common.color = Color::True(Rgb(255, 255, 255));
    block.entities.push(background);
    session.doc_mut().unwrap().blocks.insert(block.name.clone(), Arc::new(block));
    let handle = session
        .doc_mut()
        .unwrap()
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::Insert(Insert { block: "PDF_ANTET".into(), insert: Vec3::new(80.0, 30.0, 0.0), rotation: 0.35, ..Default::default() }),
        )
        .unwrap();
    let frame = session.doc().unwrap().imported_page_frame(session.doc().unwrap().entity(handle).unwrap()).unwrap();
    let anchor = frame[2];
    let to = anchor + (frame[0] - anchor) * 2.0;
    session.execute("page.resize", &json!({"handle":handle.hex(),"corner":0,"to":[to.x,to.y]})).unwrap();
    let drawing = session.doc().unwrap();
    let resized = drawing.imported_page_frame(drawing.entity(handle).unwrap()).unwrap();
    assert!((resized[2] - anchor).len() < 1e-9);
    assert!((resized[0] - to).len() < 1e-9);
    assert!(((resized[1] - resized[0]).len() / (resized[2] - resized[1]).len() - 2.0).abs() < 1e-9);
    session.execute("undo", &json!({})).unwrap();
    let drawing = session.doc().unwrap();
    assert_eq!(drawing.imported_page_frame(drawing.entity(handle).unwrap()).unwrap(), frame);
}
