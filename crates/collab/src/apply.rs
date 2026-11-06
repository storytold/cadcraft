//! Applying a change set to a drawing.

use cadcraft_doc::{Drawing, EntityStore, Handle};

use crate::CollabError;
use crate::change::{Anchor, Change};

/// Apply a change set to `drawing`, in the order the changes are given.
///
/// This is the receiving side of [`crate::diff`]: applying `diff(before, after, space)` to a copy
/// of `before` yields a space equal to `after`. Nothing here records an undo step: the
/// authoritative server owns history (ADR 0010), and a remote change is not the client's to undo.
///
/// A change that does not fit (an unknown space or layer, an existing handle, a missing anchor)
/// returns [`CollabError`] and leaves the drawing partly changed; the caller must drop it and
/// resynchronise from a snapshot.
pub fn apply(drawing: &mut Drawing, changes: &[Change]) -> Result<(), CollabError> {
    for change in changes {
        match change {
            Change::EntityCreate { space, handle, order, entity } => {
                if entity.handle != *handle {
                    return Err(CollabError::HandleMismatch { change: *handle, entity: entity.handle });
                }
                if drawing.entity(*handle).is_some() {
                    return Err(CollabError::Duplicate(*handle));
                }
                if !drawing.layers.iter().any(|l| l.name.eq_ignore_ascii_case(&entity.common.layer)) {
                    return Err(CollabError::NoLayer(entity.common.layer.clone()));
                }
                let store = drawing.space_mut(space).ok_or_else(|| CollabError::NoSpace(space.clone()))?;
                store.push(entity.clone());
                place(store, *handle, order)?;
                // A later local command must not allocate this handle again.
                drawing.bump_handseed(*handle);
            }
            Change::EntityReplace { handle, entity } => {
                if entity.handle != *handle {
                    return Err(CollabError::HandleMismatch { change: *handle, entity: entity.handle });
                }
                drawing.modify_entity(*handle, |e| *e = entity.clone()).map_err(|_| CollabError::NoEntity(*handle))?;
            }
            Change::EntityDelete { handle } => {
                drawing.remove_entity(*handle).ok_or(CollabError::NoEntity(*handle))?;
            }
            Change::EntityOrder { handle, order } => {
                let space = drawing.space_of(*handle).ok_or(CollabError::NoEntity(*handle))?;
                let store = drawing.space_mut(&space).ok_or(CollabError::NoEntity(*handle))?;
                place(store, *handle, order)?;
            }
        }
    }
    Ok(())
}

/// Put `handle` at `anchor` in `store`.
fn place(store: &mut EntityStore, handle: Handle, anchor: &Anchor) -> Result<(), CollabError> {
    match anchor {
        Anchor::Top => {
            store.bring_to_front(handle);
            Ok(())
        }
        Anchor::Bottom => {
            store.send_to_back(handle);
            Ok(())
        }
        Anchor::After { handle: below } => {
            if store.move_next_to(handle, *below, true) {
                Ok(())
            } else {
                Err(CollabError::NoAnchor(*below))
            }
        }
        Anchor::Before { handle: above } => {
            if store.move_next_to(handle, *above, false) {
                Ok(())
            } else {
                Err(CollabError::NoAnchor(*above))
            }
        }
    }
}
