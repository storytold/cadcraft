//! Named table records: their session-scoped identifiers, and the cascades that keep the drawing
//! consistent when one is created, renamed or deleted.
//!
//! Entities reference a layer by name, so renaming a layer rewrites every entity that names it, in
//! model space, every layout and every block definition. Deleting a layer deletes its objects
//! everywhere. Both are one change, not a burst of property edits: otherwise two participants
//! could interleave and leave a dangling name.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, Handle, Layer};
use serde::{Deserialize, Serialize};

use crate::CollabError;

/// Which named table a record belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordKind {
    /// A layer; entities, viewports and the current-layer header variable name it.
    Layer,
    /// A layout (paper space); its entities live in the layout.
    Layout,
    /// A block definition; block references name it. Blocks carry their own objects, so they
    /// travel whole ([`crate::Change::BlockCreate`]) rather than through the record registry.
    Block,
}

/// A record identifier, unique for the session and never reused (ADR 0002).
///
/// The server allocates it the first time it sees a record's name; a rename keeps the identifier,
/// so a late operation that still names the old name can be told from a different record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RecordId(pub u64);

/// The server's map from a table record to its identifier.
#[derive(Clone, Debug, Default)]
pub struct TableRegistry {
    next: u64,
    ids: BTreeMap<(RecordKind, String), RecordId>,
    /// Names whose records are gone: an operation that still names one is stale (ADR 0002).
    tombstones: BTreeMap<(RecordKind, String), RecordId>,
}

impl TableRegistry {
    /// An empty registry; identifiers start at 1.
    pub fn new() -> Self {
        TableRegistry { next: 1, ..Default::default() }
    }

    /// The identifier of `name`, allocated on first sight.
    pub fn id(&mut self, kind: RecordKind, name: &str) -> RecordId {
        let key = (kind, name.to_ascii_lowercase());
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        let id = RecordId(self.next);
        self.next = self.next.saturating_add(1);
        self.ids.insert(key, id);
        id
    }

    /// The identifier of `name` if the registry has seen it.
    pub fn get(&self, kind: RecordKind, name: &str) -> Option<RecordId> {
        self.ids.get(&(kind, name.to_ascii_lowercase())).copied()
    }

    /// Give every record of `drawing` an identifier, so a later operation can name it.
    pub fn reconcile(&mut self, drawing: &Drawing) {
        for layer in &drawing.layers {
            self.id(RecordKind::Layer, &layer.name);
        }
        for layout in &drawing.layouts {
            self.id(RecordKind::Layout, &layout.name);
        }
    }

    /// Forget `name` and keep a tombstone for it.
    pub fn forget(&mut self, kind: RecordKind, name: &str) -> Option<RecordId> {
        let key = (kind, name.to_ascii_lowercase());
        let id = self.ids.remove(&key)?;
        self.tombstones.insert(key, id);
        Some(id)
    }

    /// The tombstone identifier of a deleted record, when there is one.
    pub fn tombstone(&self, kind: RecordKind, name: &str) -> Option<RecordId> {
        self.tombstones.get(&(kind, name.to_ascii_lowercase())).copied()
    }

    /// Move the identifier of `from` to `to`.
    pub fn renamed(&mut self, kind: RecordKind, from: &str, to: &str) -> RecordId {
        let id = self.id(kind, from);
        self.ids.remove(&(kind, from.to_ascii_lowercase()));
        self.ids.insert((kind, to.to_ascii_lowercase()), id);
        id
    }
}

/// Renames the server knows were renames, so a diff reports `TableRename` instead of a delete and
/// a create, and an entity whose layer name changed only because of the rename is not sent as a
/// replacement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableRenames {
    /// `(from, to)` for layers, in the drawing's own spelling.
    pub layers: Vec<(String, String)>,
    /// `(from, to)` for layouts.
    pub layouts: Vec<(String, String)>,
}

impl TableRenames {
    /// Whether nothing was renamed.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.layouts.is_empty()
    }

    /// The new layer name for `from`, case-insensitively.
    pub fn layer(&self, from: &str) -> Option<&str> {
        self.layers.iter().find(|(old, _)| old.eq_ignore_ascii_case(from)).map(|(_, new)| new.as_str())
    }
}

/// Create a layer with the standard properties.
pub fn create_layer(drawing: &mut Drawing, name: &str) -> Result<(), CollabError> {
    if drawing.layers.iter().any(|l| l.name.eq_ignore_ascii_case(name)) {
        return Err(CollabError::DuplicateName(RecordKind::Layer, name.into()));
    }
    drawing.layers.push(Layer::new(name));
    Ok(())
}

/// Rename a layer and every entity that names it, in model space, layouts and block definitions.
pub fn rename_layer(drawing: &mut Drawing, from: &str, to: &str) -> Result<(), CollabError> {
    let to = to.trim();
    if to.is_empty() {
        return Err(CollabError::InvalidName("a layer name cannot be empty".into()));
    }
    if drawing.layers.iter().all(|l| !l.name.eq_ignore_ascii_case(from)) {
        return Err(CollabError::NoRecord(RecordKind::Layer, from.into()));
    }
    if drawing.layers.iter().any(|l| l.name.eq_ignore_ascii_case(to) && !l.name.eq_ignore_ascii_case(from)) {
        return Err(CollabError::DuplicateName(RecordKind::Layer, to.into()));
    }
    if let Some(layer) = drawing.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(from)) {
        layer.name = to.into();
    }

    for store in std::iter::once(&mut drawing.model).chain(drawing.layouts.iter_mut().map(|l| &mut l.entities)) {
        let handles = handles_on_layer(store, from);
        for handle in handles {
            store.modify(handle, |e| e.common.layer = to.into());
        }
    }
    for block in drawing.blocks.values_mut() {
        let block = Arc::make_mut(block);
        let handles = handles_on_layer(&block.entities, from);
        for handle in handles {
            block.entities.modify(handle, |e| e.common.layer = to.into());
        }
    }
    if drawing.header.str("CLAYER", "0").eq_ignore_ascii_case(from) {
        drawing.header.set_str("CLAYER", to);
    }
    rename_in_viewports(drawing, from, to);
    Ok(())
}

/// Delete a layer and every object on it, in model space, layouts and block definitions.
///
/// Layer `0` and `Defpoints` are never deleted, and neither is the current layer, as in `LAYDEL`.
pub fn delete_layer(drawing: &mut Drawing, name: &str) -> Result<(), CollabError> {
    if name == "0" || name.eq_ignore_ascii_case("Defpoints") {
        return Err(CollabError::InvalidName(format!("layer `{name}` cannot be deleted")));
    }
    if drawing.header.str("CLAYER", "0").eq_ignore_ascii_case(name) {
        return Err(CollabError::InvalidName(format!("the current layer `{name}` cannot be deleted")));
    }
    let Some(index) = drawing.layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)) else {
        return Err(CollabError::NoRecord(RecordKind::Layer, name.into()));
    };
    let on_layer: Vec<Handle> = std::iter::once(&drawing.model)
        .chain(drawing.layouts.iter().map(|l| &l.entities))
        .flat_map(|store| store.iter())
        .filter(|e| e.common.layer.eq_ignore_ascii_case(name))
        .map(|e| e.handle)
        .collect();
    for handle in on_layer {
        drawing.remove_entity(handle);
    }
    for block in drawing.blocks.values_mut() {
        let block = Arc::make_mut(block);
        let handles = handles_on_layer(&block.entities, name);
        for handle in handles {
            block.entities.remove(handle);
        }
    }
    drawing.layers.remove(index);
    remove_from_viewports(drawing, name);
    Ok(())
}

/// Whether `old` differs from `new` only by the names `renames` changed.
pub(crate) fn explained_by_rename(old: &cadcraft_doc::Entity, new: &cadcraft_doc::Entity, renames: &TableRenames) -> bool {
    if old == new {
        return true;
    }
    let mut probe = old.clone();
    if let Some(to) = renames.layer(&probe.common.layer) {
        probe.common.layer = to.into();
    }
    probe == *new
}

fn handles_on_layer(store: &cadcraft_doc::EntityStore, name: &str) -> Vec<Handle> {
    store.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(name)).map(|e| e.handle).collect()
}

/// Rewrite a viewport's frozen-layer and colour-override lists after a rename.
fn rename_in_viewports(drawing: &mut Drawing, from: &str, to: &str) {
    for layout in &mut drawing.layouts {
        let handles: Vec<Handle> = layout.entities.iter().filter(|e| matches!(e.kind, EntityKind::Viewport(_))).map(|e| e.handle).collect();
        for handle in handles {
            layout.entities.modify(handle, |e| {
                if let EntityKind::Viewport(v) = &mut e.kind {
                    for name in &mut v.frozen_layers {
                        if name.eq_ignore_ascii_case(from) {
                            *name = to.into();
                        }
                    }
                    for (name, _) in &mut v.layer_colors {
                        if name.eq_ignore_ascii_case(from) {
                            *name = to.into();
                        }
                    }
                }
            });
        }
    }
}

/// Drop a deleted layer from a viewport's frozen-layer and colour-override lists.
fn remove_from_viewports(drawing: &mut Drawing, name: &str) {
    for layout in &mut drawing.layouts {
        let handles: Vec<Handle> = layout.entities.iter().filter(|e| matches!(e.kind, EntityKind::Viewport(_))).map(|e| e.handle).collect();
        for handle in handles {
            layout.entities.modify(handle, |e| {
                if let EntityKind::Viewport(v) = &mut e.kind {
                    v.frozen_layers.retain(|n| !n.eq_ignore_ascii_case(name));
                    v.layer_colors.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
                }
            });
        }
    }
}
