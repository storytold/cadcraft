//! Stored block graphs and references, never expanded geometry. Definitions have independent
//! handles; forward, repeated and cyclic references are small bounded synthetic graphs.

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::{Attrib, AttribModes, AttribProps, Block, Common, Entity, EntityKind, Insert, Line, Lineweight, Space, Text};
use cadcraft_geom::Vec3;

use super::{Case, drawing, text};

pub(crate) fn insert(block: &str) -> Insert {
    Insert {
        block: block.into(),
        insert: Vec3::new(1.25, -2.5, 3.75),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }
}

fn base(name: &str, reference: Insert) -> Case {
    let mut drawing = drawing();
    let mut leaf = Block::new("Z_Leaf");
    leaf.entities.push(Entity::new(drawing.new_handle(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(2.0, 3.0, 4.0) })));
    drawing.blocks.insert(leaf.name.clone(), Arc::new(leaf));
    drawing.add(&Space::Model, Common::default(), EntityKind::Insert(reference)).unwrap();
    Case { name: name.into(), drawing }
}

pub(crate) fn transformations() -> Vec<Case> {
    let mut cases = Vec::new();
    for scale in [Vec3::new(1.0, 1.0, 1.0), Vec3::new(2.0, 0.5, 3.0), Vec3::new(-1.0, 2.0, 1.0)] {
        for rotation in [0.0, 0.7, 5.9] {
            cases.push(base(&format!("insert/scale={scale:?}/rotation={rotation}"), Insert { scale, rotation, ..insert("Z_Leaf") }));
        }
    }
    cases
}

pub(crate) fn arrays() -> Vec<Case> {
    [(1, 3), (4, 1), (3, 2)]
        .map(|(cols, rows)| {
            base(&format!("minsert/{cols}x{rows}"), Insert { cols, rows, col_spacing: 5.25, row_spacing: -3.75, rotation: 0.7, ..insert("Z_Leaf") })
        })
        .into()
}

pub(crate) fn metadata() -> Vec<Case> {
    let mut case = base("block/metadata", insert("Z_Leaf"));
    let block = Arc::make_mut(case.drawing.blocks.get_mut("Z_Leaf").unwrap());
    block.base = Vec3::new(4.0, -3.0, 2.0);
    block.description = "Synthetic Ø 图".into();
    block.explodable = false;
    block.units = 4;
    vec![case]
}

pub(crate) fn nested(cyclic: bool) -> Vec<Case> {
    let mut case = base(&format!("block/nested/cyclic={cyclic}"), insert("A_Parent"));
    let mut parent = Block::new("A_Parent");
    for x in [2.0, 5.0] {
        parent
            .entities
            .push(Entity::new(case.drawing.new_handle(), EntityKind::Insert(Insert { insert: Vec3::new(x, 0.0, 0.0), ..insert("Z_Leaf") })));
    }
    if cyclic {
        parent.entities.push(Entity::new(case.drawing.new_handle(), EntityKind::Insert(insert("A_Parent"))));
    }
    case.drawing.blocks.insert(parent.name.clone(), Arc::new(parent));
    vec![case]
}

fn attribute(tag: &str) -> Attrib {
    Attrib {
        tag: tag.into(),
        text: Text { insert: Vec3::ZERO, height: 1.0, value: tag.into(), ..text() },
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: AttribProps::default(),
        multiline: None,
        modes: AttribModes::default(),
    }
}

pub(crate) fn attributes(family: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    let variants = match family {
        "flags" => 0..2,
        "constant" => 2..4,
        _ => 0..4,
    };
    for variant in variants {
        let mut definition = attribute("ID");
        definition.prompt = "Synthetic identifier".into();
        let mut value = attribute("ID");
        value.text.value = "Part Ø 图".into();
        match family {
            "flags" | "constant" => {
                definition.invisible = variant & 1 != 0;
                definition.constant = variant & 2 != 0;
                value.invisible = definition.invisible;
                value.constant = definition.constant;
            }
            "modes" => {
                definition.modes.verify = variant & 1 != 0;
                definition.modes.preset = variant & 2 != 0;
            }
            "lock" => {
                definition.modes.lock_position = variant & 1 != 0;
                value.modes.lock_position = definition.modes.lock_position;
            }
            "overrides" => {
                value.props = match variant {
                    0 => AttribProps { layer: Some("Survey_Ä".into()), ..Default::default() },
                    1 => AttribProps { color: Some(Color::Index(3)), ..Default::default() },
                    2 => AttribProps { linetype: Some("Dash_test".into()), ..Default::default() },
                    _ => AttribProps { lineweight: Some(Lineweight::Mm100(53)), ..Default::default() },
                };
            }
            _ => panic!("unknown attribute family"),
        }
        let mut case = base(&format!("attributes/{family}/{variant}"), Insert { attribs: vec![value, attribute("SECOND")], ..insert("Z_Leaf") });
        let handle = case.drawing.new_handle();
        let second = case.drawing.new_handle();
        let block = Arc::make_mut(case.drawing.blocks.get_mut("Z_Leaf").unwrap());
        block.entities.push(Entity::new(handle, EntityKind::AttDef(definition)));
        block.entities.push(Entity::new(second, EntityKind::AttDef(attribute("SECOND"))));
        cases.push(case);
    }
    cases
}

/// Repeated nested instances must retain their own ordered values, including empty text and
/// explicit zero overrides. Depth changes the stored owner chain, not attribute identity.
pub(crate) fn nested_attributes() -> Vec<Case> {
    [1, 3]
        .map(|depth| {
            let mut case = attributes("flags").remove(0);
            case.name = format!("block/nested-attributes/depth={depth}");
            let mut child = "Z_Leaf".to_owned();
            for level in 0..depth {
                let mut parent = Block::new(&format!("Parent_{level}"));
                for instance in 0..2 {
                    let mut value = attribute("ID");
                    value.text.value = if instance == 0 { String::new() } else { format!("level={level}/instance={instance} Ø") };
                    value.props = if instance == 0 {
                        AttribProps::default()
                    } else {
                        AttribProps {
                            layer: Some("Survey_Ä".into()),
                            color: Some(Color::ByBlock),
                            linetype: Some("Continuous".into()),
                            lineweight: Some(Lineweight::Mm100(0)),
                        }
                    };
                    parent.entities.push(Entity::new(
                        case.drawing.new_handle(),
                        EntityKind::Insert(Insert {
                            insert: Vec3::new(f64::from(instance) * 10.0, f64::from(level), 0.0),
                            attribs: vec![value, attribute("SECOND")],
                            ..insert(&child)
                        }),
                    ));
                }
                // Each definition owns its own ATTDEF handles, even when its tags are repeated.
                for tag in ["ID", "SECOND"] {
                    parent.entities.push(Entity::new(case.drawing.new_handle(), EntityKind::AttDef(attribute(tag))));
                }
                child.clone_from(&parent.name);
                case.drawing.blocks.insert(parent.name.clone(), Arc::new(parent));
            }
            let handle = case.drawing.model.iter().next().unwrap().handle;
            case.drawing.model.modify(handle, |e| {
                let EntityKind::Insert(i) = &mut e.kind else { panic!("expected INSERT") };
                i.block = child;
            });
            case
        })
        .into()
}

pub(crate) fn multiline_attributes() -> Vec<Case> {
    // Keep the original origin witness and distinguish the fallback TEXT's point from MTEXT's.
    [(Vec3::ZERO, Vec3::ZERO), (Vec3::new(12.0, 7.0, 0.0), Vec3::new(20.0, 8.0, 0.0))]
        .into_iter()
        .map(|(text_point, mtext_point)| {
            let mut case = attributes("flags").remove(0);
            case.name = format!("attributes/multiline/text-point={text_point:?}");
            let decorate = |a: &mut Attrib| {
                a.text.insert = text_point;
                a.text.value = "first\nsecond".into();
                a.multiline = Some(cadcraft_doc::MultilineAttrib {
                    mtext: cadcraft_doc::MText {
                        insert: mtext_point,
                        height: 1.0,
                        width: 6.0,
                        contents: "first\\Psecond".into(),
                        ..super::geometry::mtext()
                    },
                    read_as: "first\nsecond".into(),
                });
            };
            let handles: Vec<_> = case.drawing.model.iter().map(|e| e.handle).collect();
            for handle in handles {
                case.drawing.model.modify(handle, |e| {
                    let EntityKind::Insert(i) = &mut e.kind else { panic!("expected INSERT") };
                    decorate(i.attribs.first_mut().unwrap());
                });
            }
            let block = Arc::make_mut(case.drawing.blocks.get_mut("Z_Leaf").unwrap());
            let handle = block.entities.iter().find(|e| matches!(&e.kind, EntityKind::AttDef(a) if a.tag == "ID")).unwrap().handle;
            block.entities.modify(handle, |e| {
                let EntityKind::AttDef(a) = &mut e.kind else { panic!("expected ATTDEF") };
                decorate(a);
            });
            case
        })
        .collect()
}

pub(crate) fn unreferenced() -> Vec<Case> {
    let mut case = base("block/unreferenced", insert("Z_Leaf"));
    case.drawing.blocks.insert("Empty".into(), Arc::new(Block::new("Empty")));
    let mut unused = Block::new("Unused");
    unused.entities.push(Entity::new(case.drawing.new_handle(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(5.0, 6.0, 7.0) })));
    case.drawing.blocks.insert(unused.name.clone(), Arc::new(unused));
    vec![case]
}
