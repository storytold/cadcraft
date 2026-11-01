//! The linetype scale of a block reference (DXF group 48 on the INSERT) multiplies the linetype
//! scale of the objects in the block, as LTSCALE does for the whole drawing: a DASHED line inside
//! a block inserted at linetype scale 2 has dashes twice as long.

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, Space, library};
use cadcraft_geom::Vec3;
use cadcraft_render::{Kind, Options, build};

/// A block «B» with a 100-unit DASHED line (pattern 0.5 on, 0.25 off), inserted once at the
/// origin with the given linetype scale; the line's own linetype `own` (DASHED or BYBLOCK).
fn drawing(ltscale: f64, own: &str) -> Drawing {
    let mut d = Drawing::new_metric();
    d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
    let mut blk = Block::new("B");
    let mut line = Entity::new(Handle(1), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) }));
    line.common.linetype = own.into();
    blk.entities.push(line);
    d.blocks.insert("B".into(), Arc::new(blk));
    let reference = Common { linetype: "DASHED".into(), ltscale, ..Default::default() };
    let ins = Insert {
        block: "B".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, reference, EntityKind::Insert(ins)).unwrap();
    d
}

/// The lengths of the dashes drawn for the reference, all but the last one (which may be cut).
fn dashes(d: &Drawing) -> Vec<f64> {
    let l = build(d, &Space::Model, &Options::default());
    let mut out: Vec<f64> =
        l.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| l.points(p).windows(2).map(|w| w[0].dist(w[1])).sum()).collect();
    out.pop();
    out
}

#[test]
fn the_linetype_scale_of_a_block_reference_scales_the_dashes_inside() {
    for own in ["DASHED", "BYBLOCK"] {
        let one = dashes(&drawing(1.0, own));
        let two = dashes(&drawing(2.0, own));
        assert!(!one.is_empty() && one.iter().all(|l| (l - 0.5).abs() < 1e-6), "{own}: {one:?}");
        assert!(!two.is_empty() && two.iter().all(|l| (l - 1.0).abs() < 1e-6), "{own}: {two:?}");
    }
}
