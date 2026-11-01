//! Reopening and saving a drawing keeps one anonymous `*D` block per dimension and the same
//! handles: the block read with a dimension is regenerated under its own name instead of being
//! kept next to a fresh copy, `*D` blocks nothing references are dropped, and table records and
//! objects keep their handles because CADCraft's own next handle is saved with the drawing
//! (issue #495, gaps FF7).

use std::sync::Arc;

use cadcraft_doc::{Block, Common, DimKind, Dimension, Drawing, Entity, EntityKind, Handle, Insert, Line, Space};
use cadcraft_geom::Vec3;

fn linear(a: [f64; 2], b: [f64; 2], at: [f64; 2]) -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(at[0], at[1], 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::new(a[0], a[1], 0.0),
        p14: Vec3::new(b[0], b[1], 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

fn insert(block: &str) -> Insert {
    Insert {
        block: block.into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }
}

/// (code, value) pairs of an ASCII DXF.
fn pairs(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim().to_string()))).collect()
}

/// Names of the BLOCK definitions and of the blocks DIMENSIONs and INSERTs reference.
fn blocks_and_refs(dxf: &str) -> (Vec<String>, Vec<String>) {
    let p = pairs(dxf);
    let (mut blocks, mut refs) = (Vec::new(), Vec::new());
    let mut kind = String::new();
    for (code, v) in p {
        if code == 0 {
            kind = v;
        } else if code == 2 && kind == "BLOCK" {
            blocks.push(v);
        } else if code == 2 && (kind == "DIMENSION" || kind == "INSERT") {
            refs.push(v);
        }
    }
    (blocks, refs)
}

fn dim_blocks(dxf: &str) -> Vec<String> {
    let mut v: Vec<String> = blocks_and_refs(dxf).0.into_iter().filter(|n| n.starts_with("*D")).collect();
    v.sort();
    v
}

fn resave(dxf: &str) -> String {
    cadcraft_io::write_dxf(&cadcraft_io::read_dxf(dxf.as_bytes()).unwrap())
}

fn check_cycles(d: &Drawing, dims: usize) {
    let d0 = cadcraft_io::write_dxf(d);
    let d1 = resave(&d0);
    let d2 = resave(&d1);
    let d3 = resave(&d2);
    for (i, f) in [&d0, &d1, &d2, &d3].into_iter().enumerate() {
        let (_, refs) = blocks_and_refs(f);
        let blocks = dim_blocks(f);
        assert_eq!(blocks.len(), dims, "cycle {i}: {blocks:?}");
        assert!(blocks.iter().all(|b| refs.contains(b)), "cycle {i}: unreferenced {blocks:?} / {refs:?}");
        let back = cadcraft_io::read_dxf(f.as_bytes()).unwrap();
        let n = back.model.iter().filter(|e| matches!(e.kind, EntityKind::Dimension(_))).count();
        assert_eq!(n, dims, "cycle {i}");
    }
    // Same names, same handles, same bytes once the drawing has been through one save.
    assert_eq!(dim_blocks(&d1), dim_blocks(&d0));
    assert_eq!(d2, d1, "second reopen-and-save changed the file");
    assert_eq!(d3, d2, "third reopen-and-save changed the file");
}

#[test]
fn one_dimension_keeps_one_block_over_three_cycles() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]))).unwrap();
    check_cycles(&d, 1);
}

#[test]
fn two_dimensions_and_a_user_block_keep_their_blocks() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]))).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 5.0], [20.0, 5.0], [10.0, 9.0]))).unwrap();
    let mut b = Block::new("Mark");
    let h = d.new_handle();
    b.entities.push(Entity::new(h, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })));
    d.blocks.insert("Mark".into(), Arc::new(b));
    let unused = Block::new("Spare");
    d.blocks.insert("Spare".into(), Arc::new(unused));
    let ins = insert("Mark");
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    check_cycles(&d, 2);
    let back = cadcraft_io::read_dxf(resave(&cadcraft_io::write_dxf(&d)).as_bytes()).unwrap();
    assert!(back.block("Mark").is_some_and(|b| b.entities.len() == 1));
    assert!(back.block("Spare").is_some(), "an unused user block is not purged");
}

#[test]
fn stale_dimension_blocks_are_dropped_and_referenced_ones_kept() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]))).unwrap();
    // What older versions left behind: copies nothing references, and one an INSERT uses.
    for name in ["*D7", "*D8"] {
        let mut b = Block::new(name);
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(2.0, 0.0, 0.0) })));
        d.blocks.insert(name.into(), Arc::new(b));
    }
    d.add(&Space::Model, Common::default(), EntityKind::Insert(insert("*D8"))).unwrap();
    let out = cadcraft_io::write_dxf(&d);
    let blocks = dim_blocks(&out);
    assert!(!blocks.contains(&"*D7".to_string()), "{blocks:?}");
    assert!(blocks.contains(&"*D8".to_string()), "{blocks:?}");
    assert_eq!(blocks.len(), 2, "{blocks:?}");
}

#[test]
fn reopened_dimension_reuses_its_block_name_and_handles() {
    let mut d = Drawing::new_metric();
    let h = d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]))).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let Some(EntityKind::Dimension(dm)) = back.entity(h).map(|e| &e.kind) else { panic!("dimension lost") };
    let name = dm.block.clone().unwrap();
    let before: Vec<Handle> = back.block(&name).unwrap().entities.iter().map(|e| e.handle).collect();
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&back).as_bytes()).unwrap();
    let Some(EntityKind::Dimension(dm2)) = again.entity(h).map(|e| &e.kind) else { panic!("dimension lost") };
    assert_eq!(dm2.block.as_deref(), Some(name.as_str()));
    let after: Vec<Handle> = again.block(&name).unwrap().entities.iter().map(|e| e.handle).collect();
    assert_eq!(after, before);
    assert_eq!(again.handseed, back.handseed);
}

#[test]
fn hostile_handle_seed_does_not_panic() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]))).unwrap();
    let p = pairs(&cadcraft_io::write_dxf(&d));
    // The dictionary entry names the XRECORD; its group 1 holds the seed.
    let key = p.iter().position(|(c, v)| *c == 3 && v == "CADCRAFT_HANDSEED").unwrap();
    let h = p[key + 1].1.clone();
    let rec = p.iter().position(|(c, v)| *c == 5 && *v == h).unwrap();
    let at = rec + p[rec..].iter().position(|(c, _)| *c == 1).unwrap();
    assert_eq!(p[at].1, format!("{:X}", d.handseed));
    for bad in ["FFFFFFFFFFFFFFFF", "not-hex", "", "-1"] {
        let mut q = p.clone();
        q[at].1 = bad.to_string();
        let text: String = q.iter().map(|(c, v)| format!("{c}\r\n{v}\r\n")).collect();
        let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
        let again = cadcraft_io::write_dxf(&back);
        assert_eq!(dim_blocks(&again).len(), 1, "{bad}");
    }
}

#[test]
fn dwg_reopen_and_save_keeps_one_block_per_dimension() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]))).unwrap();
    let mut bytes = cadcraft_io::write(&d, "a.dwg").unwrap();
    let mut saves = Vec::new();
    for _ in 0..3 {
        let back = cadcraft_io::read(&bytes, "a.dwg").unwrap();
        let dims: Vec<String> = back.blocks.keys().filter(|n| n.starts_with("*D")).cloned().collect();
        assert_eq!(dims.len(), 1, "{dims:?}");
        bytes = cadcraft_io::write(&back, "a.dwg").unwrap();
        saves.push(bytes.clone());
    }
    assert!(saves.windows(2).all(|w| w[0] == w[1]), "DWG reopen-and-save changed the file");
}

#[test]
fn an_unedited_dimension_keeps_the_block_it_was_read_with() {
    use cadcraft_doc::{Circle, Point};
    let mut d = Drawing::new_metric();
    // The block another program drew: geometry CADCraft never generates (a circle) and the
    // definition point on Defpoints.
    let mut b = Block::new("*D3");
    for kind in [
        EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }),
        EntityKind::Circle(Circle { center: Vec3::new(5.0, 3.0, 0.0), radius: 0.25 }),
        EntityKind::Point(Point { p: Vec3::new(0.0, 0.0, 0.0), angle: 0.0 }),
    ] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, kind));
    }
    let kept: Vec<Handle> = b.entities.iter().map(|e| e.handle).collect();
    d.blocks.insert("*D3".into(), Arc::new(b));
    let mut dm = linear([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]);
    dm.block = Some("*D3".into());
    let h = d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();

    // Saved untouched: the same block, entity for entity, over a DXF and a DWG round trip.
    let circles = |d: &Drawing| {
        let Some(EntityKind::Dimension(dm)) = d.entity(h).map(|e| &e.kind) else { panic!("dimension lost") };
        let blk = d.block(dm.block.as_deref().unwrap()).unwrap();
        (blk.entities.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count(), blk.entities.iter().map(|e| e.handle).collect::<Vec<_>>())
    };
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(circles(&back), (1, kept.clone()));
    let dwg = cadcraft_io::read(&cadcraft_io::write(&d, "a.dwg").unwrap(), "a.dwg").unwrap();
    assert_eq!(circles(&dwg).0, 1);

    // Edited (edits drop the link): regenerated from the dimension, so no circle.
    let mut edited = back.clone();
    edited
        .modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.block = None;
            }
        })
        .unwrap();
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&edited).as_bytes()).unwrap();
    assert_eq!(circles(&again).0, 0);
}
