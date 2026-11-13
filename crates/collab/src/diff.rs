//! The change set between two states of a drawing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, Handle, Space};

use crate::CollabError;
use crate::change::{Anchor, Change};
use crate::table::{RecordKind, TableRegistry, TableRenames, explained_by_rename};

/// What a client must apply to turn `before` into `after`, in `space`.
///
/// Returns nothing when the two spaces share every entity and the same draw order. The result is
/// ordered so that applying it in sequence never refers to a missing anchor: deletions first,
/// then creations in draw order (each anchored to an entity already in place), then replacements,
/// then moves in draw order. Moves are correct but not minimal: a rotation may be reported as
/// several moves, which costs a few bytes and no correctness.
pub fn diff(before: &Drawing, after: &Drawing, space: &Space) -> Result<Vec<Change>, CollabError> {
    diff_space(before, after, space, space, &HashSet::new(), &TableRenames::default())
}

/// What a client must apply to turn `before` into `after`, for the whole drawing: table records
/// first, then the entities of every space that exists in both states.
///
/// `registry` hands out session-scoped record identifiers (ADR 0002); `renames` lists the renames
/// the server performed, so a rename is one change and every entity whose layer name followed it is
/// not sent as a replacement.
pub fn diff_all(before: &Drawing, after: &Drawing, registry: &mut TableRegistry, renames: &TableRenames) -> Result<Vec<Change>, CollabError> {
    let mut changes = diff_tables(before, after, registry, renames);
    let deleted_layers = deleted_layers(before, after, renames);
    for (old_space, new_space) in paired_spaces(before, after, renames) {
        changes.extend(diff_space(before, after, &old_space, &new_space, &deleted_layers, renames)?);
    }
    Ok(changes)
}

/// Named records created or deleted between the two states, plus the renames the server declared.
pub fn diff_tables(before: &Drawing, after: &Drawing, registry: &mut TableRegistry, renames: &TableRenames) -> Vec<Change> {
    let mut changes = Vec::new();
    let renamed_from: HashSet<String> = renames.layers.iter().map(|(from, _)| from.to_ascii_lowercase()).collect();
    let renamed_to: HashSet<String> = renames.layers.iter().map(|(_, to)| to.to_ascii_lowercase()).collect();
    let old_layers: HashMap<String, &cadcraft_doc::Layer> = before.layers.iter().map(|l| (l.name.to_ascii_lowercase(), l)).collect();
    let new_layers: HashMap<String, &cadcraft_doc::Layer> = after.layers.iter().map(|l| (l.name.to_ascii_lowercase(), l)).collect();
    for (key, layer) in &old_layers {
        if !new_layers.contains_key(key) && !renamed_from.contains(key) {
            let id = forgotten(registry, RecordKind::Layer, &layer.name);
            changes.push(Change::TableDelete { kind: RecordKind::Layer, id, name: layer.name.clone() });
        }
    }
    for (key, layer) in &new_layers {
        if !old_layers.contains_key(key) && !renamed_to.contains(key) {
            let id = registry.id(RecordKind::Layer, &layer.name);
            let properties = serde_json::to_value(layer).ok();
            changes.push(Change::TableCreate { kind: RecordKind::Layer, id, name: layer.name.clone(), properties });
        }
    }
    for (from, to) in &renames.layers {
        let id = registry.renamed(RecordKind::Layer, from, to);
        changes.push(Change::TableRename { kind: RecordKind::Layer, id, from: from.clone(), to: to.clone() });
    }

    let renamed_from: HashSet<String> = renames.layouts.iter().map(|(from, _)| from.to_ascii_lowercase()).collect();
    let renamed_to: HashSet<String> = renames.layouts.iter().map(|(_, to)| to.to_ascii_lowercase()).collect();
    let old_layouts: HashMap<String, &cadcraft_doc::Layout> = before.layouts.iter().map(|l| (l.name.to_ascii_lowercase(), l)).collect();
    let new_layouts: HashMap<String, &cadcraft_doc::Layout> = after.layouts.iter().map(|l| (l.name.to_ascii_lowercase(), l)).collect();
    for (key, layout) in &old_layouts {
        if !new_layouts.contains_key(key) && !renamed_from.contains(key) {
            let id = forgotten(registry, RecordKind::Layout, &layout.name);
            changes.push(Change::LayoutDelete { id, name: layout.name.clone() });
        }
    }
    for (key, layout) in &new_layouts {
        if !old_layouts.contains_key(key) && !renamed_to.contains(key) {
            let id = registry.id(RecordKind::Layout, &layout.name);
            let properties = serde_json::to_value(layout).ok();
            changes.push(Change::LayoutCreate { id, name: layout.name.clone(), tab_order: layout.tab_order, properties });
        }
    }
    for (from, to) in &renames.layouts {
        let id = registry.renamed(RecordKind::Layout, from, to);
        changes.push(Change::LayoutRename { id, from: from.clone(), to: to.clone() });
    }

    // Blocks carry their own objects, so an unchanged definition is skipped by its `Arc` and a
    // changed one travels whole. They are last in the table changes and therefore before every
    // entity change: a replica that creates a reference after this set already has the block.
    let old_blocks: HashMap<String, &Arc<cadcraft_doc::Block>> = before.blocks.iter().map(|(k, b)| (k.to_ascii_lowercase(), b)).collect();
    let new_blocks: HashMap<String, &Arc<cadcraft_doc::Block>> = after.blocks.iter().map(|(k, b)| (k.to_ascii_lowercase(), b)).collect();
    for (key, old) in &old_blocks {
        match new_blocks.get(key) {
            None => changes.push(Change::BlockDelete { name: old.name.clone() }),
            Some(new) if !Arc::ptr_eq(*old, *new) && ***old != ***new => changes.push(block_change(new)),
            Some(_) => {}
        }
    }
    for (key, block) in &new_blocks {
        if !old_blocks.contains_key(key) {
            changes.push(block_change(block));
        }
    }
    changes
}

/// The change that carries `block` whole: its properties (`Block` serializes without its
/// entities) and its objects in draw order.
fn block_change(block: &cadcraft_doc::Block) -> Change {
    Change::BlockCreate {
        name: block.name.clone(),
        properties: serde_json::to_value(block).unwrap_or(serde_json::Value::Null),
        entities: block.entities.iter().map(|entity| (**entity).clone()).collect(),
    }
}

/// Forget a deleted record, or give it an identifier first when the registry had not seen it.
fn forgotten(registry: &mut TableRegistry, kind: RecordKind, name: &str) -> crate::table::RecordId {
    match registry.forget(kind, name) {
        Some(id) => id,
        None => registry.id(kind, name),
    }
}

/// The layers that exist in `before` and not in `after` and were not renamed: their objects are
/// removed by the layer's own change, so they must not be sent as entity deletions as well.
fn deleted_layers(before: &Drawing, after: &Drawing, renames: &TableRenames) -> HashSet<String> {
    let kept: HashSet<String> = after.layers.iter().map(|l| l.name.to_ascii_lowercase()).collect();
    let renamed: HashSet<String> = renames.layers.iter().map(|(from, _)| from.to_ascii_lowercase()).collect();
    before.layers.iter().map(|l| l.name.to_ascii_lowercase()).filter(|name| !kept.contains(name) && !renamed.contains(name)).collect()
}

/// The spaces to compare, paired across a layout rename.
fn paired_spaces(before: &Drawing, after: &Drawing, renames: &TableRenames) -> Vec<(Space, Space)> {
    let mut pairs = vec![(Space::Model, Space::Model)];
    for layout in &after.layouts {
        let old_name = renames
            .layouts
            .iter()
            .find(|(_, to)| to.eq_ignore_ascii_case(&layout.name))
            .map(|(from, _)| from.clone())
            .unwrap_or_else(|| layout.name.clone());
        if before.layouts.iter().any(|l| l.name == old_name) {
            pairs.push((Space::Paper(old_name), Space::Paper(layout.name.clone())));
        }
    }
    pairs
}

/// The entities of one space, with table cascades already accounted for.
fn diff_space(
    before: &Drawing,
    after: &Drawing,
    old_space: &Space,
    new_space: &Space,
    deleted_layers: &HashSet<String>,
    renames: &TableRenames,
) -> Result<Vec<Change>, CollabError> {
    let old = before.space(old_space).ok_or_else(|| CollabError::NoSpace(old_space.clone()))?;
    let new = after.space(new_space).ok_or_else(|| CollabError::NoSpace(new_space.clone()))?;
    if old_space == new_space && old.same_as(new) {
        return Ok(Vec::new());
    }
    let gone = |entity: &Entity| deleted_layers.contains(&entity.common.layer.to_ascii_lowercase());

    let old_order: Vec<&Arc<Entity>> = old.iter().collect();
    let new_order: Vec<&Arc<Entity>> = new.iter().collect();
    let old_by_handle: HashMap<Handle, &Arc<Entity>> = old_order.iter().map(|e| (e.handle, *e)).collect();
    let new_by_handle: HashMap<Handle, &Arc<Entity>> = new_order.iter().map(|e| (e.handle, *e)).collect();

    let mut changes = Vec::new();
    for e in &old_order {
        if !new_by_handle.contains_key(&e.handle) && !gone(e) {
            changes.push(Change::EntityDelete { handle: e.handle });
        }
    }

    // Creations, in draw order: each is anchored to the entity drawn just below it, which is
    // either one that already exists or one created earlier in this loop.
    let mut below: Option<Handle> = None;
    for e in &new_order {
        if !old_by_handle.contains_key(&e.handle) {
            let order = match below {
                Some(handle) => Anchor::After { handle },
                None => Anchor::Bottom,
            };
            changes.push(Change::EntityCreate { space: new_space.clone(), handle: e.handle, order, entity: e.as_ref().clone() });
        }
        below = Some(e.handle);
    }

    for e in &new_order {
        let Some(previous) = old_by_handle.get(&e.handle) else { continue };
        if gone(e) || explained_by_rename(previous, e, renames) {
            continue;
        }
        changes.push(Change::EntityReplace { handle: e.handle, entity: e.as_ref().clone() });
    }

    changes.extend(moves(&old_order, &new_order, &old_by_handle, &new_by_handle));
    Ok(changes)
}

/// Entities present in both states that changed place, anchored at their immediate predecessor in
/// the new order (which exists by the time moves are applied).
///
/// A move is reported only when the entity's predecessor *among entities present in both* changed:
/// an insertion or a deletion elsewhere must not look like every later entity moving.
fn moves(
    old_order: &[&Arc<Entity>],
    new_order: &[&Arc<Entity>],
    old_by_handle: &HashMap<Handle, &Arc<Entity>>,
    new_by_handle: &HashMap<Handle, &Arc<Entity>>,
) -> Vec<Change> {
    let common: HashSet<Handle> = old_by_handle.keys().copied().filter(|h| new_by_handle.contains_key(h)).collect();
    if common.len() < 2 {
        return Vec::new();
    }
    let mut below_old: HashMap<Handle, Option<Handle>> = HashMap::with_capacity(common.len());
    let mut previous: Option<Handle> = None;
    for e in old_order {
        if common.contains(&e.handle) {
            below_old.insert(e.handle, previous);
            previous = Some(e.handle);
        }
    }
    let mut below_new: HashMap<Handle, Option<Handle>> = HashMap::with_capacity(common.len());
    let mut previous: Option<Handle> = None;
    for e in new_order {
        if common.contains(&e.handle) {
            below_new.insert(e.handle, previous);
            previous = Some(e.handle);
        }
    }
    let mut changes = Vec::new();
    for e in new_order {
        if !common.contains(&e.handle) || below_new.get(&e.handle) == below_old.get(&e.handle) {
            continue;
        }
        let order = match below_in_order(new_order, e.handle) {
            Some(handle) => Anchor::After { handle },
            None => Anchor::Bottom,
        };
        changes.push(Change::EntityOrder { handle: e.handle, order });
    }
    changes
}

/// The entity drawn directly below `handle` in `order`.
fn below_in_order(order: &[&Arc<Entity>], handle: Handle) -> Option<Handle> {
    let mut below = None;
    for e in order {
        if e.handle == handle {
            return below;
        }
        below = Some(e.handle);
    }
    None
}
