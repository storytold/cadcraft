//! A multileader's spline leader and block content (#408) survive save and reopen: they are
//! written as a native MULTILEADER (#459).

use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, MLeader, Space};
use cadcraft_geom::Vec3;

#[test]
fn spline_leader_and_block_content_are_written() {
    let mut d = Drawing::new_metric();
    let mut blk = Block::new("TAG");
    blk.entities.push(Entity::new(Handle(0x100), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1.0, 1.0, 0.0) })));
    d.blocks.insert(blk.name.clone(), std::sync::Arc::new(blk));
    let m = MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.0, 5.0, 0.0)]],
        landing: Vec3::new(10.0, 0.0, 0.0),
        dogleg: 1.0,
        text: None,
        style: "Standard".into(),
        arrow_size: 1.0,
        spline: true,
        block: Some(Insert {
            block: "TAG".into(),
            insert: Vec3::new(11.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    };
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m)).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::MLeader(m) if m.spline
        && m.leaders.first().is_some_and(|l| l.len() == 2)
        && m.block.as_ref().is_some_and(|i| i.block == "TAG" && i.insert == Vec3::new(11.0, 0.0, 0.0)))));
}
