//! Applying a change set to a drawing.

use cadcraft_doc::{Drawing, EntityStore, Handle, Layer, Layout};

use crate::CollabError;
use crate::change::{Anchor, Change};
use crate::table::{RecordKind, create_layer, delete_layer, rename_layer};

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
            Change::TableCreate { kind: RecordKind::Layer, name, properties, .. } => {
                create_layer(drawing, name)?;
                if let Some(properties) = properties
                    && let Ok(layer) = serde_json::from_value::<Layer>(properties.clone())
                    && let Some(slot) = drawing.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(name))
                {
                    *slot = layer;
                    slot.name = name.clone();
                }
            }
            Change::TableCreate { kind: RecordKind::Layout, .. } => {
                return Err(CollabError::InvalidName("a layout is created with `layout.create`".into()));
            }
            Change::TableCreate { kind: RecordKind::Block, .. } => {
                return Err(CollabError::InvalidName("a block is created with `block`".into()));
            }
            Change::TableRename { kind: RecordKind::Layer, from, to, .. } => rename_layer(drawing, from, to)?,
            Change::TableRename { kind: RecordKind::Layout, .. } => {
                return Err(CollabError::InvalidName("a layout is renamed with `layout.rename`".into()));
            }
            Change::TableRename { kind: RecordKind::Block, .. } => {
                return Err(CollabError::InvalidName("a block is not renamed through the record registry".into()));
            }
            Change::TableDelete { kind: RecordKind::Layer, name, .. } => delete_layer(drawing, name)?,
            Change::TableDelete { kind: RecordKind::Layout, .. } => {
                return Err(CollabError::InvalidName("a layout is deleted with `layout.delete`".into()));
            }
            Change::TableDelete { kind: RecordKind::Block, .. } => {
                return Err(CollabError::InvalidName("a block travels whole; it is not deleted through the record registry".into()));
            }
            Change::LayoutCreate { name, tab_order, properties, .. } => {
                if drawing.layouts.iter().any(|l| l.name.eq_ignore_ascii_case(name)) {
                    return Err(CollabError::DuplicateName(RecordKind::Layout, name.clone()));
                }
                let mut layout = Layout::new(name, *tab_order);
                if let Some(properties) = properties
                    && let Ok(saved) = serde_json::from_value::<Layout>(properties.clone())
                {
                    layout.page = saved.page;
                    layout.view = saved.view;
                    layout.limits = saved.limits;
                }
                drawing.layouts.push(layout);
            }
            Change::LayoutRename { from, to, .. } => {
                if drawing.layouts.iter().all(|l| !l.name.eq_ignore_ascii_case(from)) {
                    return Err(CollabError::NoRecord(RecordKind::Layout, from.clone()));
                }
                if drawing.layouts.iter().any(|l| l.name.eq_ignore_ascii_case(to)) {
                    return Err(CollabError::DuplicateName(RecordKind::Layout, to.clone()));
                }
                if let Some(layout) = drawing.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(from)) {
                    layout.name = to.clone();
                }
                if drawing.active_layout.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(from)) {
                    drawing.active_layout = Some(to.clone());
                }
            }
            Change::LayoutDelete { name, .. } => {
                let before = drawing.layouts.len();
                drawing.layouts.retain(|l| !l.name.eq_ignore_ascii_case(name));
                if drawing.layouts.len() == before {
                    return Err(CollabError::NoRecord(RecordKind::Layout, name.clone()));
                }
                if drawing.active_layout.as_deref().is_some_and(|active| active.eq_ignore_ascii_case(name)) {
                    drawing.active_layout = None;
                }
            }
            Change::BlockCreate { name, properties, entities } => {
                let key = drawing.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.clone());
                let mut block: cadcraft_doc::Block = serde_json::from_value(properties.clone())
                    .map_err(|_| CollabError::InvalidName(format!("the definition of block `{name}` could not be read")))?;
                block.name = key.clone();
                block.entities = EntityStore::default();
                for entity in entities {
                    if !drawing.layers.iter().any(|l| l.name.eq_ignore_ascii_case(&entity.common.layer)) {
                        return Err(CollabError::NoLayer(entity.common.layer.clone()));
                    }
                    block.entities.push(entity.clone());
                    // The server chose the handle; a later local allocation must not reuse it.
                    drawing.bump_handseed(entity.handle);
                }
                drawing.blocks.insert(key, std::sync::Arc::new(block));
            }
            Change::BlockDelete { name } => {
                let key = drawing.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
                let key = key.ok_or_else(|| CollabError::NoRecord(RecordKind::Block, name.clone()))?;
                drawing.blocks.remove(&key);
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
