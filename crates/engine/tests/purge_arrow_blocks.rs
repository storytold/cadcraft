//! PURGE keeps arrowhead blocks a dimension style or a dimension's overrides name (a file's own
//! `_Dot`, a user's arrow), and still removes blocks nothing references.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Block, Circle, DimStyle, Entity, EntityKind, Handle};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

#[test]
fn purge_keeps_arrowhead_blocks_dimension_styles_name() {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    for (i, name) in ["_Dot", "MyArrow", "Unused"].into_iter().enumerate() {
        let mut b = Block::new(name);
        b.entities.push(Entity::new(Handle(0x9000 + i as u64), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.5 })));
        d.blocks.insert(name.into(), Arc::new(b));
    }
    d.dim_styles.push(DimStyle { name: "Dots".into(), arrow_block1: " _DOT ".into(), leader_arrow_block: "MyArrow".into(), ..DimStyle::default() });
    let r = s.execute("purge", &json!({ "type": "blocks" })).unwrap();
    assert_eq!(r["purged"]["blocks"], json!(["Unused"]), "{r}");
    let d = s.doc().unwrap();
    assert!(d.block("_Dot").is_some() && d.block("MyArrow").is_some());
}
