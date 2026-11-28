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

/// A linear dimension drawn by block `block`, its dimension line at height `y`.
fn dimension(block: &str, y: usize) -> String {
    format!("0\nDIMENSION\n2\n{block}\n3\nStandard\n10\n100\n20\n{}\n11\n50\n21\n{}\n70\n0\n13\n0\n23\n{y}\n14\n100\n24\n{y}\n", y + 5, y + 5)
}

const LINE: &str = "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n";

/// What dimension block `*D1` holds on its way back to itself.
#[derive(Clone, Copy, Debug)]
enum Via {
    /// Dimensions drawn by `*D1`.
    Nothing,
    /// References to block `B`, which holds a dimension drawn by `*D1`.
    Reference,
    /// Dimensions drawn by `*D2`, which holds a dimension drawn by `*D1`.
    Dimension,
}

/// Dimension block `*D1` holds a line and `refs` dimensions or references that lead back to `*D1`
/// (`via`); the block on the way holds a line too. Model space holds one dimension drawn by `*D1`,
/// and a reference to `B` when there is one, so the loop is also entered from `B`. With `closed`
/// false, the first dimension inside a block on the way is drawn by `*D9`, a block the file lacks,
/// so nothing leads back.
fn dimension_loop(refs: usize, via: Via, closed: bool) -> String {
    let back = if closed { "*D1" } else { "*D9" };
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\n*D1\n70\n1\n10\n0\n20\n0\n");
    for k in 0..refs {
        s += &match via {
            Via::Nothing => dimension(back, k * 10),
            Via::Reference => format!("0\nINSERT\n2\nB\n10\n{k}\n20\n0\n"),
            Via::Dimension => dimension(if closed { "*D2" } else { "*D9" }, k * 10),
        };
    }
    s += LINE;
    s += "0\nENDBLK\n";
    match via {
        Via::Nothing => {}
        Via::Reference => s += &format!("0\nBLOCK\n2\nB\n70\n0\n10\n0\n20\n0\n{}{LINE}0\nENDBLK\n", dimension(back, 0)),
        Via::Dimension => s += &format!("0\nBLOCK\n2\n*D2\n70\n1\n10\n0\n20\n0\n{}{LINE}0\nENDBLK\n", dimension("*D1", 0)),
    }
    s += "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n";
    s += &dimension("*D1", 0);
    if matches!(via, Via::Reference) {
        s += "0\nINSERT\n2\nB\n10\n200\n20\n0\n";
    }
    s + "0\nENDSEC\n0\nEOF\n"
}

/// Dimension blocks `*D1`..`*Dn` each hold a line and a reference to block `B`, which holds a
/// dimension drawn by each of them; model space holds a dimension drawn by `*D1`. Every ordering
/// of the blocks is a way round the loop.
fn loop_of_blocks(n: usize) -> String {
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n");
    for i in 1..=n {
        s += &format!("0\nBLOCK\n2\n*D{i}\n70\n1\n10\n0\n20\n0\n0\nINSERT\n2\nB\n10\n0\n20\n0\n{LINE}0\nENDBLK\n");
    }
    s += "0\nBLOCK\n2\nB\n70\n0\n10\n0\n20\n0\n";
    for i in 1..=n {
        s += &dimension(&format!("*D{i}"), i * 10);
    }
    s + "0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n" + &dimension("*D1", 0) + "0\nENDSEC\n0\nEOF\n"
}

/// Dimension blocks `*D1`..`*Dn` in a ring: each holds a line and three dimensions drawn by the
/// next, the last by `*D1`. Model space holds a dimension drawn by `*D1`.
fn ring_of_blocks(n: usize) -> String {
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n");
    for i in 1..=n {
        let next = format!("*D{}", i % n + 1);
        s += &format!("0\nBLOCK\n2\n*D{i}\n70\n1\n10\n0\n20\n0\n{LINE}");
        for k in 0..3 {
            s += &dimension(&next, k * 10);
        }
        s += "0\nENDBLK\n";
    }
    s + "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n" + &dimension("*D1", 0) + "0\nENDSEC\n0\nEOF\n"
}

/// Dimension blocks nested without a cycle. `*D3` holds the only line, from (0, 0) to (1, 1);
/// `*D2` holds a dimension drawn by `*D3`; `*D1` holds two dimensions drawn by `*D2` and a
/// reference to block `B`, which holds a dimension drawn by `*D2`. Model space holds a dimension
/// drawn by `*D1` and two references to `B`.
fn nested_dimensions() -> String {
    let block = |name: &str, body: &str| format!("0\nBLOCK\n2\n{name}\n70\n{}\n10\n0\n20\n0\n{body}0\nENDBLK\n", u8::from(name.starts_with('*')));
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n");
    s += &block("*D3", "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n");
    s += &block("*D2", &dimension("*D3", 0));
    s += &block("B", &dimension("*D2", 0));
    s += &block("*D1", &(dimension("*D2", 0) + &dimension("*D2", 10) + "0\nINSERT\n2\nB\n10\n5\n20\n0\n"));
    s + "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"
        + &dimension("*D1", 0)
        + "0\nINSERT\n2\nB\n10\n20\n20\n0\n0\nINSERT\n2\nB\n10\n40\n20\n0\n0\nENDSEC\n0\nEOF\n"
}

/// A dimension inside a block whose own block leads back to that block (the same block, or
/// through a reference or another dimension block) is drawn as one whose block is missing: from
/// its definition points. It used to draw the block again at every level down to
/// `MAX_BLOCK_DEPTH` (#1923).
#[test]
fn a_dimension_whose_block_leads_back_draws_as_one_whose_block_is_missing() {
    for via in [Via::Nothing, Via::Reference, Via::Dimension] {
        let closed = build(&read(&dimension_loop(1, via, true)), &Space::Model, &Options::default());
        let open = build(&read(&dimension_loop(1, via, false)), &Space::Model, &Options::default());
        assert!(!open.prims.is_empty());
        assert_eq!((closed.prims.len(), &closed.verts), (open.prims.len(), &open.verts), "via {via:?}");
    }
}

/// Many dimensions that lead back to their block (directly, or through as many references or
/// dimensions), dimension blocks that lead to each other, or a long ring of them, drew the blocks
/// again at every level: up to 300^16 times, or once per ordering of the blocks, until the
/// expansion budget ran out and the rest was left out. Now nothing goes round a loop, so the
/// drawing is whole. A block of 300 references and a ring of 20 blocks are more than the search
/// near a dimension's block looks at, so the whole block graph answers for them. Their extents use
/// the shared block cache (#1850) and stay finite.
#[test]
fn dimensions_on_block_loops_draw_whole() {
    let files = [Via::Nothing, Via::Reference, Via::Dimension].map(|via| dimension_loop(300, via, true));
    for (i, text) in files.iter().chain([&loop_of_blocks(10), &ring_of_blocks(20)]).enumerate() {
        let d = read(text);
        let ext = d.extents(&Space::Model);
        assert!(!ext.is_empty() && ext.min.is_finite() && ext.max.is_finite(), "{ext:?}");
        let list = build(&d, &Space::Model, &Options::default());
        assert!(!list.prims.is_empty() && !list.truncated, "file {i}: {} expanded", list.expanded);
    }
}

/// A loop can also pass through a multileader's block or a proxy's graphics, which are drawn as
/// block contents: `*D1` holds one of them leading to block `K`, which holds a dimension drawn by
/// `*D1` (or by a block the file lacks, for comparison). The proxy's graphics also hold such a
/// dimension themselves: they are drawn as part of `*D1`.
#[test]
fn a_loop_through_a_multileader_or_proxy_graphics_is_cut_too() {
    let line = |h: u64| Entity::new(Handle(h), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) }));
    let dim = |h: u64, block: &str| {
        Entity::new(
            Handle(h),
            EntityKind::Dimension(cadcraft_doc::Dimension {
                kind: cadcraft_doc::DimKind::Linear { rotation: 0.0 },
                defpt: Vec3::new(100.0, 5.0, 0.0),
                text_mid: Vec3::new(50.0, 5.0, 0.0),
                p13: Vec3::ZERO,
                p14: Vec3::new(100.0, 0.0, 0.0),
                p15: Vec3::ZERO,
                p16: Vec3::ZERO,
                text: String::new(),
                style: "Standard".into(),
                measurement: 100.0,
                text_rotation: 0.0,
                user_text_pos: false,
                block: Some(block.into()),
                overrides: Default::default(),
                assoc: Vec::new(),
                marks: Default::default(),
                insertion_point: Vec3::ZERO,
            }),
        )
    };
    let to_k = Insert {
        block: "K".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    for through_proxy in [false, true] {
        let drawing = |back: &str| {
            let via = if through_proxy {
                EntityKind::Unknown(cadcraft_doc::Unknown {
                    dxf_type: "ACAD_PROXY_ENTITY".into(),
                    tags: Vec::new(),
                    graphics: vec![Entity::new(Handle(0x300), EntityKind::Insert(to_k.clone())), dim(0x301, back)],
                })
            } else {
                EntityKind::MLeader(cadcraft_doc::MLeader {
                    leaders: vec![vec![Vec3::new(-5.0, 0.0, 0.0)]],
                    landing: Vec3::ZERO,
                    dogleg: 1.0,
                    text: None,
                    style: "Standard".into(),
                    arrow_size: 1.0,
                    spline: false,
                    block: Some(to_k.clone()),
                })
            };
            let mut d = Drawing::new_imperial();
            let mut d1 = Block::new("*D1");
            d1.entities.push(line(0x100));
            d1.entities.push(Entity::new(Handle(0x101), via));
            let mut k = Block::new("K");
            k.entities.push(line(0x200));
            k.entities.push(dim(0x201, back));
            d.blocks.insert("*D1".into(), Arc::new(d1));
            d.blocks.insert("K".into(), Arc::new(k));
            let EntityKind::Dimension(top) = dim(0, "*D1").kind else { unreachable!() };
            d.add(&Space::Model, Common::default(), EntityKind::Dimension(top)).unwrap();
            build(&d, &Space::Model, &Options::default())
        };
        let (closed, open) = (drawing("*D1"), drawing("*D9"));
        assert!(!open.prims.is_empty());
        assert_eq!((closed.prims.len(), &closed.verts), (open.prims.len(), &open.verts), "through a proxy: {through_proxy}");
    }
}

/// Dimension blocks `*E1`..`*En` in a chain: each holds `lines` lines and a dimension drawn by the
/// next; the last holds only lines. Model space holds a dimension drawn by `*E1`.
fn chain_of_blocks(n: usize, lines: usize) -> String {
    let mut s = String::from(
        "0
SECTION
2
BLOCKS
",
    );
    for i in 1..=n {
        s += &format!(
            "0
BLOCK
2
*E{i}
70
1
10
0
20
0
{}",
            LINE.repeat(lines)
        );
        if i < n {
            s += &dimension(&format!("*E{}", i + 1), 0);
        }
        s += "0
ENDBLK
";
    }
    s + "0
ENDSEC
0
SECTION
2
ENTITIES
" + &dimension("*E1", 0)
        + "0
ENDSEC
0
EOF
"
}

/// Dimension blocks nested without a cycle are still drawn from their blocks, on every path: two
/// dimensions deep, by sibling dimensions, and through references to a block holding one. A chain
/// of blocks with more objects than the search near a dimension's block looks at is settled by
/// the whole block graph, which finds no loop either.
#[test]
fn nested_dimension_blocks_without_a_cycle_draw_on_every_path() {
    let only_lines = |list: &cadcraft_render::DisplayList| {
        list.prims.iter().all(|p| {
            let v = &list.verts[p.start as usize..(p.start + p.len) as usize];
            matches!(v, [a, b] if *b - *a == Vec2::new(1.0, 1.0))
        })
    };
    let list = build(&read(&nested_dimensions()), &Space::Model, &Options::default());
    // `*D3`'s line, through `*D1` (two dimensions of `*D2`, and `B`) and the two `B` in model
    // space; nothing else is drawn, so no dimension fell back to its definition points.
    assert_eq!(list.prims.len(), 2 + 1 + 2);
    assert!(only_lines(&list));
    let list = build(&read(&chain_of_blocks(5, 100)), &Space::Model, &Options::default());
    assert_eq!(list.prims.len(), 5 * 100);
    assert!(only_lines(&list));
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
        clip: None,
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
        twist: 0.0,
    };
    let paper = Space::Paper("Layout1".into());
    d.add(&paper, Common::default(), EntityKind::Viewport(viewport(Vec3::new(50.0, 50.0, 0.0), Vec2::ZERO, vec!["FROZEN".into()]))).unwrap();
    d.add(&paper, Common::default(), EntityKind::Viewport(viewport(Vec3::new(200.0, 50.0, 0.0), Vec2::new(10_000.0, 10_000.0), Vec::new()))).unwrap();

    let list = build(&d, &paper, &Options::default());
    assert_eq!(list.expanded, MAX_BLOCK_EXPANSION);
    assert!(!list.truncated, "the first viewport consumed the budget exactly and the second saw no model content");
}
