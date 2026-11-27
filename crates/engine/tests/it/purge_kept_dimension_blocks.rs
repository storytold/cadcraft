//! PURGE keeps the `*D` block a dimension kept as read names (LARGE_RADIAL_DIMENSION,
//! ARC_DIMENSION): the dimension draws it, and AutoCAD needs it to read the file (#3438, #1161).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Block, Common, Entity, EntityKind, Handle, Line, RawTag, Space, Unknown};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

#[test]
fn purge_keeps_the_block_a_kept_dimension_names() {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    for name in ["*D1", "*D2"] {
        let mut b = Block::new(name);
        b.entities.push(Entity::new(Handle(0x9000), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })));
        d.blocks.insert(name.into(), Arc::new(b));
    }
    let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
    let dim = Unknown {
        dxf_type: "ARC_DIMENSION".into(),
        tags: vec![tag(100, "AcDbEntity"), tag(8, "0"), tag(100, "AcDbDimension"), tag(2, "*D1"), tag(100, "AcDbArcDimension")],
        graphics: Vec::new(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Unknown(dim)).unwrap();
    s.execute("purge", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("*D1").is_some(), "the kept dimension's block stays");
    assert!(d.block("*D2").is_none(), "an unused one goes");
}
