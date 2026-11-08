//! DWG anonymous dimension-block suffixes are not persistent identities (#2257). Match blocks
//! through dimensions with preserved entity handles, then compare authored content and properties
//! strictly. Only anonymous *D<number> names may change; missing references and merged blocks fail.
//! This verifier covers the model-space dimension fixtures, not arbitrary drawing graph remapping.
//!
//! Autodesk documents that anonymous block numbers may change between drawing sessions:
//! https://help.autodesk.com/cloudhelp/2022/ENU/AutoCAD-AutoLISP/files/GUID-EB9BFDAA-487A-4586-A556-1F5F84BCF2F3.htm
//! ODA's DWG specification §20.4.52.1 shows a BLOCK_HEADER named *D without a suffix;
//! DIMENSION records reference their block by handle, not by its reconstructed name:
//! https://www.opendesign.com/files/guestdownloads/OpenDesign_Specification_for_.dwg_files.pdf

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cadcraft_doc::{Block, Circle, Drawing, Entity, EntityKind, Handle, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::Version;

use super::super::{
    cases::{self, Case, complex},
    compare,
};

pub(super) fn check() {
    for case in complex::dimensions().into_iter().chain([paired_dimensions(["*D42", "*D7"])]) {
        for version in Version::ALL {
            let mut expected = case.drawing.clone();
            // DIMFXL has no native field before R2007 (same limit as other dimension-style cases).
            if version < Version::R2007 {
                for style in &mut expected.dim_styles {
                    style.fixed_ext_length = 0.0;
                }
            }
            super::check_version_using(&case.name, &case.drawing, &expected, version, equivalent);
        }
    }
}

// Two independent cache blocks with different authored geometry, plus an ordinary named block.
// Names are supplied explicitly so comparator controls do not learn expectations from a save.
fn paired_dimensions(names: [&str; 2]) -> Case {
    let mut drawing = cases::drawing();
    for (index, (case, name)) in complex::dimensions().into_iter().zip(names).enumerate() {
        let entity = case.drawing.model.iter().next().unwrap();
        let mut kind = entity.kind.clone();
        let EntityKind::Dimension(dim) = &mut kind else { panic!("fixture dimension") };
        dim.block = Some(name.into());
        drawing.add(&Space::Model, entity.common.clone(), kind).unwrap();
        let mut block = Block::new(name);
        for entity in case.drawing.block("*D42").unwrap().entities.iter() {
            block.entities.push(Entity { handle: drawing.new_handle(), ..entity.as_ref().clone() });
        }
        // This circle is authored cache content, not something dimension regeneration produces.
        block
            .entities
            .push(Entity::new(drawing.new_handle(), EntityKind::Circle(Circle { center: Vec3::new(5.0, 3.0, 0.0), radius: 0.25 + index as f64 })));
        drawing.blocks.insert(name.into(), Arc::new(block));
    }
    let mut named = Block::new("UserPart");
    named.entities.push(Entity::new(drawing.new_handle(), EntityKind::Circle(Circle { center: Vec3::new(1.0, 2.0, 0.0), radius: 2.0 })));
    drawing.blocks.insert(named.name.clone(), Arc::new(named));
    Case { name: "dimension/distinct-anonymous-caches-and-named-block".into(), drawing }
}

fn anonymous_dimension_name(name: &str) -> bool {
    name.strip_prefix("*D").is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()))
}

fn equivalent(expected: &Drawing, actual: &Drawing, context: &str) {
    let mut names = BTreeMap::new();
    for entity in expected.model.iter() {
        let EntityKind::Dimension(dim) = &entity.kind else { continue };
        let before = dim.block.as_deref().expect("fixture has a dimension block");
        let back = actual.entity(entity.handle).unwrap_or_else(|| panic!("{context}: missing dimension {}", entity.handle.hex()));
        let EntityKind::Dimension(dim) = &back.kind else { panic!("{context}: dimension changed type") };
        let after = dim.block.as_deref().unwrap_or_else(|| panic!("{context}: dimension lost its block reference"));
        let source = expected.block(before).expect("fixture block exists");
        let target = actual.block(after).unwrap_or_else(|| panic!("{context}: dangling dimension block reference {after}"));
        if before != after {
            assert!(source.anonymous && target.anonymous, "{context}: only anonymous dimension blocks may be renamed");
            assert!(
                anonymous_dimension_name(before) && anonymous_dimension_name(after),
                "{context}: unexpected block name mapping {before} -> {after}"
            );
        }
        if let Some(previous) = names.insert(before.to_string(), after.to_string()) {
            assert_eq!(previous, after, "{context}: one dimension block split into different references");
        }
    }
    assert!(!names.is_empty(), "{context}: fixture must exercise dimension block references");
    assert_eq!(names.values().collect::<BTreeSet<_>>().len(), names.len(), "{context}: distinct dimension blocks were merged");
    assert_eq!(actual.blocks.len(), expected.blocks.len(), "{context}: block lost or extra cache generated");

    // Adapt only the validated names, never content or properties learned from the output.
    // Rebuild the map to handle renumberings that reuse another source block's old name.
    let mut renamed = expected.clone();
    renamed.blocks.clear();
    for (name, block) in &expected.blocks {
        let mapped = names.get(name).unwrap_or(name);
        let mut block = block.clone();
        Arc::make_mut(&mut block).name = mapped.clone();
        assert!(renamed.blocks.insert(mapped.clone(), block).is_none(), "{context}: block name collision");
    }
    for entity in expected.model.iter() {
        if let EntityKind::Dimension(dim) = &entity.kind {
            let mapped = names.get(dim.block.as_ref().expect("fixture block")).expect("mapped reference");
            renamed.model.modify(entity.handle, |e| {
                let EntityKind::Dimension(dim) = &mut e.kind else { panic!("fixture dimension") };
                dim.block = Some(mapped.clone());
            });
        }
    }
    compare::model(&renamed, actual, context);
}

#[test]
fn comparison_accepts_only_anonymous_dimension_renumbering() {
    let original = paired_dimensions(["*D42", "*D7"]).drawing;
    equivalent(&original, &original, "unchanged");
    for names in [["*D1", "*D2"], ["*D7", "*D42"]] {
        let renamed = paired_dimensions(names).drawing;
        equivalent(&original, &renamed, "renumbered, including reused source names");
        // The general comparator must still reject renaming outside this DWG-specific check.
        assert!(std::panic::catch_unwind(|| compare::model(&original, &renamed, "strict comparison")).is_err());
    }
}

fn dimension_handles(d: &Drawing) -> Vec<Handle> {
    d.model.iter().filter(|e| matches!(e.kind, EntityKind::Dimension(_))).map(|e| e.handle).collect()
}

fn set_reference(d: &mut Drawing, handle: Handle, name: Option<&str>) {
    assert!(d.model.modify(handle, |e| {
        let EntityKind::Dimension(dim) = &mut e.kind else { panic!("fixture dimension") };
        dim.block = name.map(str::to_string);
    }));
}

#[test]
fn comparison_rejects_broken_references_content_and_block_properties() {
    let original = paired_dimensions(["*D42", "*D7"]).drawing;
    let renamed = paired_dimensions(["*D1", "*D2"]).drawing;
    equivalent(&original, &renamed, "valid control");
    let mutations: [fn(&mut Drawing); 14] = [
        |d| set_reference(d, dimension_handles(d)[0], None),
        |d| set_reference(d, dimension_handles(d)[0], Some("*D999")),
        |d| set_reference(d, dimension_handles(d)[0], Some("*D2")),
        |d| {
            let hs = dimension_handles(d);
            set_reference(d, hs[0], Some("*D2"));
            set_reference(d, hs[1], Some("*D1"));
        },
        |d| {
            d.blocks.remove("*D1");
        },
        |d| Arc::make_mut(d.blocks.get_mut("*D1").unwrap()).entities = Default::default(),
        |d| {
            let block = Arc::make_mut(d.blocks.get_mut("*D1").unwrap());
            let h = block.entities.iter().next().unwrap().handle;
            block.entities.modify(h, |e| e.common.visible = false);
        },
        |d| {
            let block = Arc::make_mut(d.blocks.get_mut("*D1").unwrap());
            let h = block.entities.iter().last().unwrap().handle;
            block.entities.modify(h, |e| {
                let EntityKind::Circle(circle) = &mut e.kind else { panic!("fixture circle") };
                circle.radius += 1.0;
            });
        },
        |d| {
            let block = Arc::make_mut(d.blocks.get_mut("*D1").unwrap());
            let h = block.entities.iter().next().unwrap().handle;
            block.entities.modify(h, |e| e.handle = Handle(0xFFFF));
        },
        |d| Arc::make_mut(d.blocks.get_mut("*D1").unwrap()).base.x += 1.0,
        |d| Arc::make_mut(d.blocks.get_mut("*D1").unwrap()).anonymous = false,
        |d| {
            d.blocks.insert("*D999".into(), Arc::new(Block::new("*D999")));
        },
        |d| {
            let mut block = d.blocks.remove("UserPart").unwrap();
            Arc::make_mut(&mut block).name = "RenamedPart".into();
            d.blocks.insert("RenamedPart".into(), block);
        },
        |d| {
            let h = dimension_handles(d)[0];
            d.model.modify(h, |e| {
                let EntityKind::Dimension(dim) = &mut e.kind else { panic!("fixture dimension") };
                dim.p14.x += 1.0;
            });
        },
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut changed = renamed.clone();
        mutate(&mut changed);
        assert!(std::panic::catch_unwind(|| equivalent(&original, &changed, "mutation")).is_err(), "mutation {index} must fail");
    }
}

#[test]
fn comparison_rejects_changed_prefixes_and_named_dimension_cache_renaming() {
    let original = paired_dimensions(["*D42", "*D7"]).drawing;
    for name in ["*U1", "*D", "*Dcustom", "UserCache"] {
        let changed = paired_dimensions([name, "*D2"]).drawing;
        assert!(std::panic::catch_unwind(|| equivalent(&original, &changed, "wrong name kind")).is_err());
    }
    let original = paired_dimensions(["AuthoredCache", "*D7"]).drawing;
    let changed = paired_dimensions(["RenamedCache", "*D2"]).drawing;
    assert!(std::panic::catch_unwind(|| equivalent(&original, &changed, "named cache")).is_err());
}
