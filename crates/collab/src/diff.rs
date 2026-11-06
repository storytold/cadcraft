//! The change set between two states of one space.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, Handle, Space};

use crate::CollabError;
use crate::change::{Anchor, Change};

/// What a client must apply to turn `before` into `after`, in `space`.
///
/// Returns nothing when the two spaces share every entity and the same draw order. The result is
/// ordered so that applying it in sequence never refers to a missing anchor: deletions first,
/// then creations in draw order (each anchored to an entity already in place), then replacements,
/// then moves in draw order. Moves are correct but not minimal: a rotation may be reported as
/// several moves, which costs a few bytes and no correctness.
pub fn diff(before: &Drawing, after: &Drawing, space: &Space) -> Result<Vec<Change>, CollabError> {
    let old = before.space(space).ok_or_else(|| CollabError::NoSpace(space.clone()))?;
    let new = after.space(space).ok_or_else(|| CollabError::NoSpace(space.clone()))?;
    if old.same_as(new) {
        return Ok(Vec::new());
    }

    let old_order: Vec<&Arc<Entity>> = old.iter().collect();
    let new_order: Vec<&Arc<Entity>> = new.iter().collect();
    let old_by_handle: HashMap<Handle, &Arc<Entity>> = old_order.iter().map(|e| (e.handle, *e)).collect();
    let new_by_handle: HashMap<Handle, &Arc<Entity>> = new_order.iter().map(|e| (e.handle, *e)).collect();

    let mut changes = Vec::new();
    for e in &old_order {
        if !new_by_handle.contains_key(&e.handle) {
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
            changes.push(Change::EntityCreate { space: space.clone(), handle: e.handle, order, entity: e.as_ref().clone() });
        }
        below = Some(e.handle);
    }

    for e in &new_order {
        let Some(previous) = old_by_handle.get(&e.handle) else { continue };
        if !Arc::ptr_eq(previous, e) && previous.as_ref() != e.as_ref() {
            changes.push(Change::EntityReplace { handle: e.handle, entity: e.as_ref().clone() });
        }
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
