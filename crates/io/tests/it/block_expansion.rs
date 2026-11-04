//! Nested, self-referencing and arrayed block references in a hostile DXF must not hang drawing
//! or extents (issue #255): the expansion is bounded, not only its depth.

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, Space, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::{MAX_BLOCK_EXPANSION, Options, build};

fn read(text: &str) -> Drawing {
    cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap()
}

/// Block `A` holds a line and `refs` references to itself; model space holds one `A`.
fn self_referencing(refs: usize) -> String {
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n");
    for k in 1..=refs {
        s += &format!("0\nINSERT\n2\nA\n10\n{k}\n20\n0\n");
    }
    s + "0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n"
}

/// Blocks `F0`..`F14`, each holding four references to the next; `F15` holds a line (drawn
/// 4^15 times: its references sit exactly at `MAX_BLOCK_DEPTH`).
fn fan_out() -> String {
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n");
    for k in 0..15 {
        s += &format!("0\nBLOCK\n2\nF{k}\n");
        for x in 0..4 {
            s += &format!("0\nINSERT\n2\nF{}\n10\n{x}\n20\n0\n", k + 1);
        }
        s += "0\nENDBLK\n";
    }
    s + "0\nBLOCK\n2\nF15\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nF0\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n"
}

/// One MINSERT of a one-line block with `cols` x `rows` copies 2 units apart.
fn minsert(cols: u32, rows: u32) -> String {
    format!(
        "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n10\n0\n20\n0\n70\n{cols}\n71\n{rows}\n44\n2\n45\n2\n0\nENDSEC\n0\nEOF\n"
    )
}

#[test]
fn block_expansion_bombs_are_bounded() {
    for text in [self_referencing(4), self_referencing(12), fan_out(), minsert(10_000, 10_000)] {
        let d = read(&text);
        let ext = d.extents(&Space::Model);
        assert!(!ext.is_empty() && ext.min.is_finite() && ext.max.is_finite(), "{ext:?}");
        let list = build(&d, &Space::Model, &Options::default());
        assert!(!list.prims.is_empty() && list.prims.len() <= MAX_BLOCK_EXPANSION, "{}", list.prims.len());
        // Saving computes the extents again ($EXTMIN/$EXTMAX).
        assert!(cadcraft_io::write_dxf(&d).contains("$EXTMAX"));
    }
}

#[test]
fn ordinary_nested_and_arrayed_blocks_are_drawn_whole() {
    // 3 x 2 copies of a one-line block: six lines, extents spanning all copies.
    let d = read(&minsert(3, 2));
    assert_eq!(build(&d, &Space::Model, &Options::default()).prims.len(), 6);
    let ext = d.extents(&Space::Model);
    assert_eq!((ext.min.x, ext.min.y, ext.max.x, ext.max.y), (0.0, 0.0, 5.0, 3.0));
    // A self-referencing block draws its line at every level down to MAX_BLOCK_DEPTH.
    let d = read(&self_referencing(1));
    assert!(matches!(d.model.iter().next().map(|e| &e.kind), Some(EntityKind::Insert(_))));
    assert_eq!(build(&d, &Space::Model, &Options::default()).prims.len(), cadcraft_doc::MAX_BLOCK_DEPTH);
}

#[test]
fn a_build_that_spends_the_expansion_budget_says_so() {
    // 10,000 x 10,000 copies of a one-line block: far past the budget. The list stops at the
    // budget and says so, so a screen, plot or export missing contents is not silent (#1070).
    let d = read(&minsert(10_000, 10_000));
    let list = build(&d, &Space::Model, &Options::default());
    assert!(list.truncated, "block contents were left out without a word");
    assert!(list.prims.len() <= MAX_BLOCK_EXPANSION, "{}", list.prims.len());
    // A drawing within the budget is whole and says nothing.
    let d = read(&minsert(3, 2));
    let list = build(&d, &Space::Model, &Options::default());
    assert!(!list.truncated);
    assert_eq!(list.prims.len(), 6);
}

#[test]
fn an_empty_followup_viewport_does_not_mark_an_exact_budget_truncated() {
    let mut d = Drawing::new_imperial();
    d.ensure_layer("FROZEN");
    let mut block = Block::new("A");
    block.entities.push(Entity {
        handle: Handle(0x200),
        common: Common { layer: "FROZEN".into(), ..Common::default() },
        kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) }),
    });
    d.blocks.insert(block.name.clone(), Arc::new(block));
    let insert = Insert {
        block: "A".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1_000,
        rows: 1_000,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(insert)).unwrap();
    let viewport = |center: Vec3, view_center: Vec2, frozen_layers| Viewport {
        center,
        width: 100.0,
        height: 100.0,
        view_center,
        view_height: 100.0,
        id: 2,
        locked: false,
        frozen_layers,
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    let paper = Space::Paper("Layout1".into());
    d.add(&paper, Common::default(), EntityKind::Viewport(viewport(Vec3::new(50.0, 50.0, 0.0), Vec2::ZERO, vec!["FROZEN".into()]))).unwrap();
    d.add(&paper, Common::default(), EntityKind::Viewport(viewport(Vec3::new(200.0, 50.0, 0.0), Vec2::new(10_000.0, 10_000.0), Vec::new()))).unwrap();

    let list = build(&d, &paper, &Options::default());
    assert_eq!(list.expanded, MAX_BLOCK_EXPANSION);
    assert!(!list.truncated, "the first viewport consumed the budget exactly and the second saw no model content");
}
